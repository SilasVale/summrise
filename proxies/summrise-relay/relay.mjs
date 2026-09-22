// summrise-relay — the outbound half of "no Cloudflare, no VPN client": an agent DIALS OUT to this process, and this process
// makes the agent's own /mcp and /panel reachable to whoever can reach the relay.
//
// WHY THIS SHAPE. The research in docs/research/remote-mcp-access.md found that every comparable product inverts the
// direction — the agent dials out and the server never dials in: Portainer Edge Agent ("there is now no need to expose the
// Portainer agents to the Internet"), Microsoft dev tunnels ("No inbound connections are required"), Home Assistant Cloud via
// SniTun, GitLab and GitHub runners. MCP itself prescribes no relay and cannot reach loopback from the cloud, so a NAT'd agent
// is unreachable without one.
//
// WHY LONG-POLL AND NOT WEBSOCKET. Portainer's edge agent polls, and Node ships a WebSocket CLIENT but no server, so polling
// keeps this file dependency-free — it runs on any machine with plain `node`. The cost is one round-trip of latency per
// request; the upgrade path (a real duplex channel) is a change to this file alone, because the frames below are already
// request/response with an id.
//
// WHAT IT IS NOT: it does not terminate TLS, and it holds no identity of its own beyond the shared token. Put it on a host
// you control, give it a token, and prefer a TLS terminator in front of it (the research is unambiguous that a public URL
// with a static bearer token is not a complete posture — RFC 6750 requires TLS for bearer tokens).
//
//   node relay.mjs --listen 127.0.0.1:18990 --token <secret> [--device d1]
//
// The agent side is `summrise connect <relay-url>` (see agent/src/relay.rs): it polls /agent/pull, runs each request through the
// same dispatch its local listener uses, and posts the answer to /agent/answer. What a client sees is the agent's own HTTP
// surface, unchanged: /mcp, /panel/*, /api/*.
import { createServer } from "node:http";
import { randomUUID, timingSafeEqual } from "node:crypto";
import { pathToFileURL } from "node:url";

const MAX_BODY = 8 * 1024 * 1024;
const LONG_POLL_MS = 25_000;

function arg(name, dflt) {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : dflt;
}

function tokenOk(given, want) {
  if (!want) return false;
  const a = Buffer.from(String(given || ""));
  const b = Buffer.from(want);
  return a.length === b.length && timingSafeEqual(a, b);
}

/// A LAST-RESORT NET: a relay that exits because one socket misbehaved is worse than a relay that logs and keeps carrying.
/// Nothing here swallows a BUG silently — both handlers log with a stack — they only refuse to die for a single caller.
function survive(log) {
  process.on("uncaughtException", (e) => log(`uncaught exception, carrying on: ${e && e.stack ? e.stack : e}`));
  process.on("unhandledRejection", (e) => log(`unhandled rejection, carrying on: ${e && e.stack ? e.stack : e}`));
}

export function createRelay({ token, deviceName = "device", log = () => {} }) {
  /** Requests waiting for the agent, and answers waiting for the client. */
  const pending = new Map();   // id -> { resolve, timer }
  const waiting = [];          // agent long-polls parked here: { res, timer }
  let agentSeenMs = 0;

  function parkAgent(res) {
    const entry = { res, timer: setTimeout(() => { drop(entry); res.writeHead(204).end(); }, LONG_POLL_MS) };
    waiting.push(entry);
    log(`agent parked (${waiting.length} waiting)`);
  }
  function drop(entry) {
    clearTimeout(entry.timer);
    const i = waiting.indexOf(entry);
    if (i >= 0) waiting.splice(i, 1);
  }
  function handToAgent(job) {
    while (waiting.length) {
      const entry = waiting.shift();
      clearTimeout(entry.timer);
      if (entry.res.writableEnded) continue;
      entry.res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(job));
      return true;
    }
    return false;
  }

  const server = createServer((req, res) => {
    // THE 'error' EVENT WITH NO LISTENER IS WHAT ACTUALLY KILLED IT (round 212). Writing to a response whose socket is gone does
    // NOT throw — Node emits 'error' on the response instead, and an 'error' event with no listener is THROWN, which ends the
    // process. That is how an agent being killed mid-poll took the relay down three times while this was being brought up.
    // A vanished caller is normal for a pipe; this is where the pipe says so.
    res.on("error", (e) => log(`a caller's socket failed: ${e && e.message ? e.message : e}`));
    const url = new URL(req.url, "http://relay");
    const auth = req.headers.authorization || "";
    const bearer = auth.startsWith("Bearer ") ? auth.slice(7) : "";

    // ---- the agent's two endpoints ---------------------------------------------------------------------------------
    if (url.pathname === "/agent/pull") {
      if (!tokenOk(bearer, token)) return respond(res, 401, '{"ok":false}');
      agentSeenMs = Date.now();
      const job = pendingJob();
      if (job) return handToAgentNow(res, job);
      return parkAgent(res);
    }
    if (url.pathname === "/agent/answer" && req.method === "POST") {
      if (!tokenOk(bearer, token)) return res.writeHead(401, { "Content-Type": "application/json" }).end('{"ok":false}');
      return readBody(req, (err, body) => {
        if (err) return res.writeHead(413).end();
        let frame;
        try { frame = JSON.parse(body.toString("utf8")); } catch { return res.writeHead(400).end(); }
        const job = pending.get(frame.id);
        if (!job) return res.writeHead(404, { "Content-Type": "application/json" }).end('{"ok":false,"error":"unknown id"}');
        pending.delete(frame.id);
        clearTimeout(job.timer);
        job.resolve(frame);
        res.writeHead(200, { "Content-Type": "application/json" }).end('{"ok":true}');
      });
    }

    // ---- everything else is for the agent --------------------------------------------------------------------------
    server.on("error", (e) => log(`server error, carrying on: ${e && e.message ? e.message : e}`));

    if (url.pathname === "/healthz") {
      return res.writeHead(200, { "Content-Type": "application/json" }).end(
        JSON.stringify({ ok: true, device: deviceName, agentConnectedMsAgo: agentSeenMs ? Date.now() - agentSeenMs : null, waiting: waiting.length }),
      );
    }
    forwardToAgent(req, res, url);
  });

  /// ONE QUEUE, ONE FLAG. A job sits in `pending` until its answer arrives (the answer arrives BY ID), and `taken` says
  /// whether an agent already has it — so the parked-agent path and the polling path cannot both hand out the same request.
  function pendingJob() {
    for (const [id, job] of pending) {
      if (!job.taken) {
        job.taken = true;
        return { id, job };
      }
    }
    return null;
  }
  function handToAgentNow(res, { id, job }) {
    clearTimeout(job.timer);
    job.timer = setTimeout(() => { pending.delete(id); job.resolve(null); }, LONG_POLL_MS);
    res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(job.frame));
  }

  function forwardToAgent(req, res, url) {
    readBody(req, (err, body) => {
      if (err) return res.writeHead(413, { "Content-Type": "application/json" }).end('{"ok":false,"error":"body too large"}');
      // TWO DIFFERENT SITUATIONS, TWO DIFFERENT ANSWERS. "No agent has EVER connected" is an operator's mistake — a wrong URL,
      // a missing token, a service that never started — and waiting 25 seconds to say so helps nobody. "An agent was here and
      // has not polled back yet" is the normal case (its poll window is 25s), so the request is queued and answered when it
      // returns. The first version refused both, which made a client that arrived a moment before its own agent fail.

      const id = randomUUID();
      const frame = {
        id, method: req.method, path: url.pathname + url.search,
        headers: Object.fromEntries(Object.entries(req.headers).filter(([k]) => k !== "host")),
        bodyB64: body.length ? body.toString("base64") : "",
      };
      // HOW LONG TO WAIT IS THE WHOLE QUESTION. If no agent has EVER connected, the most likely truth is an operator's
      // mistake — a wrong URL, a missing token, a service that never started — so the wait is short (2s) and the answer is
      // 503. That short wait also covers the honest race where a client arrives a moment before its agent's first poll. If an
      // agent HAS been here, it is between polls (its window is 25s), so the request waits the full window and a missing
      // answer is a 504.
      const grace = agentSeenMs ? LONG_POLL_MS : 2_000;
      const promise = new Promise((resolve) => {
        const job = { frame, resolve, taken: false, expired: false, timer: setTimeout(() => { pending.delete(id); job.expired = true; resolve(null); }, grace) };
        pending.set(id, job);
      });
      const job = pending.get(id);
      // A parked agent gets it at once; otherwise it waits in the one queue until an agent polls.
      if (handToAgent(frame)) job.taken = true;
      promise.then((answer) => {
        // THE CLIENT MAY BE GONE, AND THAT MUST NOT TAKE THE RELAY WITH IT (round 212). An agent being swapped for an update
        // is killed mid-poll, which destroys the socket this response was going to be written to; the write then throws
        // asynchronously, and an unhandled throw in a promise callback ENDS THE PROCESS. It ended this one three times while
        // the feature was being brought up, and each time the device correctly reported "relay unreachable" — the relay really
        // was dead. A relay is a pipe: a vanished caller is normal, not fatal.
        if (res.writableEnded || res.destroyed) return;
        if (!answer) {
          return job.expired && !agentSeenMs
            ? res.writeHead(503, { "Content-Type": "application/json" }).end('{"ok":false,"error":"no agent has connected to this relay yet"}')
            : res.writeHead(504, { "Content-Type": "application/json" }).end('{"ok":false,"error":"the agent did not answer in time"}');
        }
        const headers = { ...(answer.headers || {}) };
        delete headers["content-length"];
        res.writeHead(answer.status || 502, headers).end(Buffer.from(answer.bodyB64 || "", "base64"));
      });
    });
  }

  /// Write, or don't, but never throw: this is the point where a vanished caller used to end the process.
  function respond(res, status, body, headers = { "Content-Type": "application/json" }) {
    try {
      if (res.writableEnded || res.destroyed) return;
      res.writeHead(status, headers).end(body);
    } catch (e) {
      log(`could not answer a caller that went away: ${e && e.message ? e.message : e}`);
    }
  }

  function readBody(req, cb) {
    const chunks = [];
    let n = 0;
    req.on("data", (c) => { n += c.length; if (n > MAX_BODY) { cb(new Error("too large")); req.destroy(); } else chunks.push(c); });
    req.on("end", () => cb(null, Buffer.concat(chunks)));
    req.on("error", cb);
  }

  return {
    server,
    listen(port, host) {
      return new Promise((resolve) => server.listen(port, host, () => resolve(server.address())));
    },
    close() { for (const e of waiting) drop(e); return new Promise((r) => server.close(r)); },
    /// Exposed so a caller can install the safety net it wants; the CLI entry point installs this one.
    survive: () => survive(log),
    stats() { return { waiting: waiting.length, pending: pending.size, agentSeenMs }; },
  };
}

// IS THIS FILE THE PROGRAM, OR A MODULE SOMEONE IMPORTED? The obvious comparison — `import.meta.url === "file://" +
// process.argv[1]` — is UNIX-SHAPED and never true on Windows, where argv[1] is `D:\dir\relay.mjs` and import.meta.url is
// `file:///D:/dir/relay.mjs`. The relay therefore did NOTHING when run directly on the device, while every test passed,
// because tests IMPORT it. Found by running it end to end (round 209); `pathToFileURL` is the platform-correct answer.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const listen = arg("listen", "127.0.0.1:18990");
  const token = arg("token", process.env.SUMMRISE_RELAY_TOKEN || "");
  const device = arg("device", "d1");
  if (!token) {
    console.error("summrise-relay: --token is required (or SUMMRISE_RELAY_TOKEN). Without it every agent is refused, which is the only safe default.");
    process.exit(1);
  }
  const [host, port] = [listen.slice(0, listen.lastIndexOf(":")), Number(listen.slice(listen.lastIndexOf(":") + 1))];
  const relay = createRelay({ token, deviceName: device, log: (m) => console.log(`[relay] ${m}`) });
  relay.survive();
  relay.listen(port, host).then(() => {
    console.log(`summrise-relay listening on ${host}:${port} for agent "${device}"`);
    if (host !== "127.0.0.1" && host !== "localhost" && host !== "::1") {
      console.log("NOTE: this host is reachable from outside. Terminate TLS in front of it before putting real credentials through it.");
    }
  });
}
