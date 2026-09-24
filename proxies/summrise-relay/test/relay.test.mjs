// The relay's contract, end to end and in-process: an agent that polls out, a client that arrives from anywhere, and the
// answers travelling back. No network beyond loopback, no dependencies — plain `node --test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRelay } from "../relay.mjs";

const TOKEN = "relay-sekret";

async function withRelay(fn) {
  const relay = createRelay({ token: TOKEN, deviceName: "d1" });
  const addr = await relay.listen(0, "127.0.0.1");
  const base = `http://127.0.0.1:${addr.port}`;
  try {
    return await fn({ relay, base });
  } finally {
    await relay.close();
  }
}

/// The agent side, as the real one behaves: poll, answer whatever arrives, repeat until told to stop.
///
/// STOP ABORTS THE IN-FLIGHT POLL. `stop` used to only set a flag, and the loop only re-reads it after the
/// current `await fetch` resolves — which for `/agent/pull` is the relay's LONG_POLL_MS, 25 seconds. So
/// `await agent.stop()` waited out the whole window, five times per run: the suite did not finish in 120 s
/// and its seventh test was cancelled rather than failed (eleventh exploration, round 124). The abort also
/// exercises something the real agent does — a poll dropped mid-flight — which the relay handles by design.
function startFakeAgent(base, { delayMs = 0, answer = (f) => ({ status: 200, headers: { "content-type": "text/plain" }, bodyB64: Buffer.from(`echo:${f.path}`).toString("base64") }) } = {}) {
  let stop = false;
  const ctrl = new AbortController();
  const done = (async () => {
    while (!stop) {
      let pull;
      try {
        pull = await fetch(`${base}/agent/pull`, {
          headers: { authorization: `Bearer ${TOKEN}` },
          signal: ctrl.signal,
        });
      } catch {
        return; // aborted by stop(): the poll was dropped, which is the point
      }
      if (pull.status === 204) continue;
      const frame = await pull.json();
      if (delayMs) await new Promise((r) => setTimeout(r, delayMs));
      const reply = answer(frame);
      await fetch(`${base}/agent/answer`, {
        method: "POST",
        headers: { authorization: `Bearer ${TOKEN}`, "content-type": "application/json" },
        body: JSON.stringify({ id: frame.id, ...reply }),
      });
    }
  })();
  return { stop: () => { stop = true; ctrl.abort(); return done; } };
}

test("a request with no agent connected is refused, and says why", async () => {
  await withRelay(async ({ base }) => {
    const res = await fetch(`${base}/mcp`, { method: "POST", body: "{}" });
    assert.equal(res.status, 503);
    const body = await res.json();
    assert.equal(body.ok, false);
    assert.match(body.error, /no agent/);
  });
});

test("a client's request reaches the agent and its answer comes back unchanged", async () => {
  await withRelay(async ({ base }) => {
    const agent = startFakeAgent(base);
    await new Promise((r) => setTimeout(r, 50)); // let it park
    const res = await fetch(`${base}/mcp?x=1`, { method: "POST", body: "hello" });
    assert.equal(res.status, 200);
    assert.equal(await res.text(), "echo:/mcp?x=1");
    await agent.stop();
  });
});

test("the agent's endpoints refuse a wrong or missing token", async () => {
  await withRelay(async ({ base }) => {
    for (const headers of [{}, { authorization: "Bearer nope" }, { authorization: TOKEN }]) {
      const res = await fetch(`${base}/agent/pull`, { headers });
      assert.equal(res.status, 401, `headers ${JSON.stringify(headers)} must be refused`);
    }
  });
});

test("the body travels both ways, byte for byte", async () => {
  await withRelay(async ({ base }) => {
    const payload = "x".repeat(50_000) + "≠\u0000é";
    const agent = startFakeAgent(base, {
      answer: (f) => ({ status: 201, headers: { "content-type": "application/octet-stream", "x-seen": "1" }, bodyB64: f.bodyB64 }),
    });
    await new Promise((r) => setTimeout(r, 50));
    const res = await fetch(`${base}/api/echo`, { method: "POST", body: payload });
    assert.equal(res.status, 201);
    assert.equal(res.headers.get("x-seen"), "1");
    assert.equal(await res.text(), payload);
    await agent.stop();
  });
});

test("healthz tells an operator whether an agent is parked", async () => {
  await withRelay(async ({ base }) => {
    const before = await (await fetch(`${base}/healthz`)).json();
    assert.equal(before.ok, true);
    assert.equal(before.waiting, 0);
    const agent = startFakeAgent(base);
    await new Promise((r) => setTimeout(r, 50));
    const after = await (await fetch(`${base}/healthz`)).json();
    assert.equal(after.waiting, 1);
    await agent.stop();
  });
});

test("a client that arrived first is served when the agent connects (parked-agent path)", async () => {
  await withRelay(async ({ base }) => {
    const inflight = fetch(`${base}/panel/`, { method: "GET" });
    await new Promise((r) => setTimeout(r, 50));
    const agent = startFakeAgent(base);
    const res = await inflight;
    assert.equal(res.status, 200);
    assert.equal(await res.text(), "echo:/panel/");
    await agent.stop();
  });
});

test("a caller that vanishes mid-request does not take the relay with it", async () => {
  // THE DEFECT THIS PINS (round 212). An agent being swapped for an update is killed mid-poll, which destroys the socket the
  // answer was going to be written to; the write threw asynchronously and the unhandled throw ENDED THE PROCESS — three times
  // while the feature was being brought up, with the device correctly reporting "relay unreachable". The fake agent here
  // answers LATE on purpose, so the client is certainly gone by the time the answer arrives.
  await withRelay(async ({ base }) => {
    const agent = startFakeAgent(base, { delayMs: 150 });
    await new Promise((r) => setTimeout(r, 50));
    const ac = new AbortController();
    const inflight = fetch(`${base}/slow`, { method: "POST", signal: ac.signal }).catch(() => "client gave up");
    await new Promise((r) => setTimeout(r, 30));
    ac.abort();
    await inflight;
    await new Promise((r) => setTimeout(r, 400)); // long enough for the late answer to be written to a socket that is gone
    const health = await (await fetch(`${base}/healthz`)).json();
    assert.equal(health.ok, true, "the relay must still be answering after a caller vanished");
    await agent.stop();
  });
});

test("the server's error listeners do not grow with requests", async () => {
  // The registration used to live INSIDE the request handler, so the count rose by one per request. It is
  // invisible until Node warns at eleven, and then the log line for one real error arrives N times.
  await withRelay(async ({ relay, base }) => {
    assert.equal(relay.server.listenerCount("error"), 1, "one listener before any traffic");
    for (let i = 0; i < 25; i++) await fetch(`${base}/healthz`);
    assert.equal(relay.server.listenerCount("error"), 1, "…and still one after 25 requests");
  });
});
