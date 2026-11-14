"use strict";
// The panel the Electron window shows, served beside the device.
//
// The React panel is the legacy UI (the one in `summrise-main/agent/resources/panel`). It was written
// against the legacy agent's `/api/*` surface; the 2.0 device does not serve that surface, so this
// server is the thin translation the operator asked for: the current device interfaces stay the
// truth (`/api/panel`, `/api/health`, terminal/journal routes) and only the handful of paths the
// panel actually reads are answered here. No legacy backend is copied.
const http = require("http");
const fs = require("fs");
const path = require("path");
const os = require("os");

function toolLog(line) {
  try { fs.appendFileSync(path.join(os.tmpdir(), "summrise-panel.log"), line + "\n"); } catch { /* diagnostics are best effort */ }
}

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".json": "application/json; charset=utf-8",
};

function readBody(url, timeoutMs = 5000) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => { body += chunk; });
      res.on("end", () => resolve({ status: res.statusCode || 502, body }));
    });
    req.setTimeout(timeoutMs, () => req.destroy(new Error("device_timeout")));
    req.on("error", reject);
  });
}

/** The panel's localStorage bootstrap: its own origin and the token this server requires. */
function bootstrapScript(token) {
  return `<script>try{localStorage.setItem("summriseHost",location.host);localStorage.setItem("summriseToken",${JSON.stringify(token)});}catch(e){}</script>`;
}

function panelHtml(html, token) {
  return html.replace("</head>", `${bootstrapScript(token)}</head>`);
}

function statusFromPanel(snapshot, deviceBase) {
  let port = 0;
  try { port = Number(new URL(deviceBase).port) || 0; } catch { /* keep 0 */ }
  const state = snapshot && snapshot.state && typeof snapshot.state === "object" ? snapshot.state : {};
  const counts = state.counts && typeof state.counts === "object" ? state.counts : {};
  const live = Number(counts.sessions ?? counts.live_sessions ?? 0) || 0;
  const dataDir = typeof snapshot?.data_dir === "string" ? snapshot.data_dir : "";
  return {
    ok: true,
    version: String(snapshot?.version ?? ""),
    release: String(snapshot?.version ?? ""),
    host: "127.0.0.1",
    port,
    live_sessions: live,
    serial_ports: [],
    config_path: dataDir ? path.win32.join(dataDir, "etc", "config.yaml") : "",
    relay: {
      configured: false,
      connected: false,
      last_ok_ms: null,
      consecutive_failures: 0,
      last_error: null,
    },
  };
}

function deviceEvents(deviceBase) {
  return readBody(`${deviceBase}/api/panel`, 4000).then((r) => {
    const snapshot = JSON.parse(r.body);
    const events = Array.isArray(snapshot.events) ? snapshot.events : [];
    return { snapshot, events };
  });
}


/** Read one JSON request body; the panel's tool calls are small. */
function readRequestBody(req) {
  return new Promise((resolve) => {
    let body = "";
    req.on("data", (chunk) => { body += chunk; });
    req.on("end", () => resolve(body));
  });
}

/** The device's per-boot panel token, served inside `/panel`'s meta tag. */
async function devicePanelToken(deviceBase, cache) {
  if (cache.token) return cache.token;
  const page = await readBody(`${deviceBase}/panel`);
  const match = /name="summrise-panel" content="([0-9a-f]{64})"/.exec(page.body);
  if (!match) throw new Error("panel_token_missing");
  cache.token = match[1];
  return cache.token;
}

function deviceRequest(deviceBase, path, { method = "GET", body, token } = {}) {
  return new Promise((resolve, reject) => {
    const url = new URL(path, deviceBase);
    const req = http.request(url, {
      method,
      headers: {
        "x-summrise-panel": token,
        ...(body ? { "content-type": "application/json", "content-length": Buffer.byteLength(body) } : {}),
      },
    }, (res) => {
      let data = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => { data += chunk; });
      res.on("end", () => resolve({ status: res.statusCode || 502, body: data }));
    });
    req.setTimeout(8000, () => req.destroy(new Error("device_timeout")));
    req.on("error", reject);
    req.end(body);
  });
}

/** Legacy session row from the device's own terminal list. */
function sessionRow(session) {
  const id = String(session.id ?? session.session_id ?? "");
  const kind = String(session.kind ?? "pty");
  return {
    id,
    kind,
    title: String(session.title ?? kind),
    closed: session.alive === false,
    active: session.alive !== false,
    rows: session.rows ?? 24,
    cols: session.cols ?? 80,
  };
}

function makeHandler({ deviceBase, html, desktopDir, token, activityHtml, capture }) {
  const cache = { token: "" };
  return async (req, res) => {
    const url = req.url || "/";
    const pathOnly = url.split("?")[0];

    if (pathOnly === "/legacy" || pathOnly === "/") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
      res.end(html);
      return;
    }

    if (pathOnly === "/screenshot.png") {
      if (typeof capture !== "function") {
        res.writeHead(501, { "content-type": "application/json" });
        res.end('{"error":"no_capture"}');
        return;
      }
      try {
        const png = await capture();
        res.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" });
        res.end(Buffer.from(png, "base64"));
      } catch (error) {
        res.writeHead(500, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: String(error) }));
      }
      return;
    }

    if (pathOnly === "/activity") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
      res.end(panelHtml(activityHtml, token));
      return;
    }

    if (pathOnly === "/desktop" || pathOnly === "/desktop/") {
      try {
        const page = fs.readFileSync(path.join(desktopDir, "index.html"), "utf8");
        res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
        res.end(panelHtml(page, token));
      } catch (error) {
        res.writeHead(500, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: false, error: `panel_html: ${error.message}` }));
      }
      return;
    }

    if (pathOnly.startsWith("/desktop/")) {
      const rel = pathOnly.slice("/desktop/".length);
      const file = path.resolve(desktopDir, rel);
      if (!file.startsWith(path.resolve(desktopDir)) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end('{"error":"not_found"}');
        return;
      }
      res.writeHead(200, {
        "content-type": TYPES[path.extname(file).toLowerCase()] || "application/octet-stream",
        "cache-control": "no-store",
      });
      fs.createReadStream(file).pipe(res);
      return;
    }

    if (pathOnly.startsWith("/api/")) {
      const auth = String(req.headers.authorization || "");
      if (token && auth !== `Bearer ${token}`) {
        res.writeHead(401, { "content-type": "application/json" });
        res.end('{"ok":false,"error":"unauthorized"}');
        return;
      }
      if (pathOnly === "/api/panel") {
        try {
          const upstream = await readBody(`${deviceBase}/api/panel`);
          res.writeHead(upstream.status, { "content-type": "application/json", "cache-control": "no-store" });
          res.end(upstream.body);
        } catch {
          res.writeHead(502, { "content-type": "application/json" });
          res.end('{"error":"device_unreachable"}');
        }
        return;
      }
      if (pathOnly.startsWith("/api/runs/")) {
        try {
          const panelToken = await devicePanelToken(deviceBase, cache);
          const upstream = await deviceRequest(deviceBase, pathOnly, { token: panelToken });
          res.writeHead(upstream.status, { "content-type": "application/json", "cache-control": "no-store" });
          res.end(upstream.body);
        } catch {
          res.writeHead(502, { "content-type": "application/json" });
          res.end('{"ok":false,"error":"device_unreachable"}');
        }
        return;
      }
      if (pathOnly === "/api/status") {
        try {
          const snapshot = JSON.parse((await readBody(`${deviceBase}/api/panel`)).body);
          res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          res.end(JSON.stringify(statusFromPanel(snapshot, deviceBase)));
        } catch {
          res.writeHead(502, { "content-type": "application/json" });
          res.end('{"ok":false,"error":"device_unreachable"}');
        }
        return;
      }
      if (pathOnly === "/api/events") {
        res.writeHead(200, {
          "content-type": "text/event-stream",
          "cache-control": "no-store",
          connection: "keep-alive",
        });
        let closed = false;
        let seen = new Set();
        let cursor = 0;
        const send = (event) => {
          const key = `${event.event}:${event.id}`;
          if (seen.has(key)) return;
          seen.add(key);
          res.write(`data: ${JSON.stringify(event)}\n\n`);
        };
        const poll = async () => {
          if (closed) return;
          try {
            const { events } = await deviceEvents(deviceBase);
            for (const event of events) {
              if (typeof event.id === "number" && event.id > cursor) cursor = event.id;
              send(event);
            }
          } catch { /* device asleep: the stream stays open and retries */ }
          if (!closed) setTimeout(poll, 1000);
        };
        const heartbeat = setInterval(() => { if (!closed) res.write(": keep-alive\n\n"); }, 15000);
        req.on("close", () => {
          closed = true;
          clearInterval(heartbeat);
        });
        void poll();
        return;
      }
      if (pathOnly.startsWith("/api/tools/")) {
        const name = pathOnly.slice("/api/tools/".length);
        const raw = await readRequestBody(req);
        toolLog(`tool ${name} args=${raw.slice(0, 200)}`);
        let args = {};
        try { args = raw ? JSON.parse(raw) : {}; } catch { args = {}; }
        try {
          const panelToken = await devicePanelToken(deviceBase, cache);
          toolLog(`tool ${name} token=${panelToken ? "yes" : "no"}`);
          if (name === "terminal_saved_connections" || name === "terminal_list_ports") {
            res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
            res.end(JSON.stringify({ ok: true, result: [] }));
            return;
          }
          if (name === "terminal_list") {
            const r = await deviceRequest(deviceBase, "/api/terminal", { token: panelToken });
            const body = JSON.parse(r.body);
            const rows = (body.sessions || []).map(sessionRow);
            toolLog(`terminal_list rows=${JSON.stringify(rows)}`);
            res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
            res.end(JSON.stringify({ ok: true, result: rows }));
            return;
          }
          if (name === "terminal_open") {
            const kind = String(args.kind || "pty");
            const payload = { kind };
            if (kind === "serial" && args.target) payload.port = String(args.target);
            if (kind === "pty" && args.target) payload.shell = String(args.target);
            const r = await deviceRequest(deviceBase, "/api/terminal/create", {
              method: "POST", token: panelToken, body: JSON.stringify(payload),
            });
            const body = JSON.parse(r.body);
            if (r.status >= 400 || !body.id) {
              res.writeHead(200, { "content-type": "application/json" });
              res.end(JSON.stringify({ ok: false, error: body.error || "terminal_open_failed" }));
              return;
            }
            res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
            res.end(JSON.stringify({ ok: true, result: body.id }));
            return;
          }
          if (name === "terminal_close") {
            const sid = String(args.session_id || "");
            const r = await deviceRequest(deviceBase, `/api/terminal/${encodeURIComponent(sid)}/close`, {
              method: "POST", token: panelToken, body: "{}",
            });
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: r.status < 400 }));
            return;
          }
          if (name === "terminal_write") {
            const sid = String(args.session_id || "");
            const data = String(args.data ?? "");
            const r = await deviceRequest(deviceBase, `/api/terminal/${encodeURIComponent(sid)}/write`, {
              method: "POST", token: panelToken, body: JSON.stringify({ data }),
            });
            const body = JSON.parse(r.body || "{}");
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: r.status < 400, written: body.written ?? 0 }));
            return;
          }
          if (name === "terminal_read") {
            const sid = String(args.session_id || "");
            const offset = Number(args.offset ?? 0) || 0;
            const r = await deviceRequest(deviceBase, `/api/terminal/${encodeURIComponent(sid)}/read?cursor=${offset}`, { token: panelToken });
            const body = JSON.parse(r.body || "{}");
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: r.status < 400, text: body.text || "", end: body.cursor ?? offset }));
            return;
          }
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: "tool_not_implemented", tool: name }));
        } catch (error) {
          toolLog(`tool ${name} error=${error && error.message || error}`);
          cache.token = "";
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: String(error && error.message || error) }));
        }
        return;
      }

      if (pathOnly === "/api/events/term") {
        res.writeHead(200, {
          "content-type": "text/event-stream",
          "cache-control": "no-store",
          connection: "keep-alive",
        });
        let closed = false;
        const cursors = new Map();
        let known = "";
        const send = (frame) => res.write(`data: ${JSON.stringify(frame)}\n\n`);
        const poll = async () => {
          if (closed) return;
          try {
            const panelToken = await devicePanelToken(deviceBase, cache);
            const list = JSON.parse((await deviceRequest(deviceBase, "/api/terminal", { token: panelToken })).body);
            const rows = (list.sessions || []).map(sessionRow).filter((row) => !row.closed);
            const sig = rows.map((row) => row.id).sort().join(",");
            if (sig !== known) { known = sig; send({ ev: "sessions-changed" }); }
            for (const row of rows) {
              const cursor = cursors.get(row.id) || 0;
              const r = await deviceRequest(deviceBase, `/api/terminal/${encodeURIComponent(row.id)}/read?cursor=${cursor}&wait_ms=0`, { token: panelToken });
              if (r.status >= 400) continue;
              const body = JSON.parse(r.body || "{}");
              if (typeof body.cursor === "number") cursors.set(row.id, body.cursor);
              if (body.text) send({ session_id: row.id, data: Array.from(Buffer.from(body.text, "utf8")) });
            }
          } catch { /* device asleep: keep the stream open */ }
          if (!closed) setTimeout(poll, 400);
        };
        const ping = setInterval(() => { if (!closed) res.write(": ping\n\n"); }, 30000);
        req.on("close", () => { closed = true; clearInterval(ping); });
        void poll();
        return;
      }
      if (pathOnly === "/api/logs") {
        try {
          const snapshot = JSON.parse((await readBody(`${deviceBase}/api/panel`)).body);
          const dataDir = typeof snapshot?.data_dir === "string" ? snapshot.data_dir : "";
          const dir = dataDir ? path.join(dataDir, "logs") : "";
          const logs = [];
          if (dir && fs.existsSync(dir)) {
            for (const name of fs.readdirSync(dir).filter((n) => n.endsWith(".log"))) {
              const file = path.join(dir, name);
              const size = fs.statSync(file).size;
              const read = Math.min(size, 64 * 1024);
              const buf = Buffer.alloc(read);
              const fd = fs.openSync(file, "r");
              fs.readSync(fd, buf, 0, read, Math.max(0, size - read));
              fs.closeSync(fd);
              logs.push({ name, present: true, log: buf.toString("utf8") });
            }
          }
          res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          res.end(JSON.stringify({ ok: true, logs, dir }));
        } catch {
          res.writeHead(200, { "content-type": "application/json" });
          res.end('{"ok":false,"error":"logs_unreadable"}');
        }
        return;
      }
      if (pathOnly === "/api/sessions") {
        res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
        res.end(JSON.stringify({ ok: true, sessions: [] }));
        return;
      }
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(JSON.stringify({ ok: false, error: "not_implemented" }));
      return;
    }

    res.writeHead(404, { "content-type": "application/json" });
    res.end('{"error":"not_found"}');
  };
}

/** Start the panel on an ephemeral loopback port; resolves to its origin and a closer. */
function startLegacyPanel({ deviceBase, html, desktopDir, token, activityHtml, capture }) {
  return new Promise((resolve, reject) => {
    const server = http.createServer(makeHandler({ deviceBase, html, desktopDir, token, activityHtml, capture }));
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const base = `http://127.0.0.1:${server.address().port}`;
      resolve({
        base,
        close: () => new Promise((done) => server.close(done)),
      });
    });
  });
}

module.exports = { startLegacyPanel };
