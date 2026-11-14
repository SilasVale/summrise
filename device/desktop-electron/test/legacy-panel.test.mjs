import assert from "node:assert/strict";
import http from "node:http";
import test from "node:test";

import { startLegacyPanel } from "../src/legacy-panel.js";

function deviceServer(body = '{"version":"2.0.0","events":[]}', status = 200) {
  return new Promise((resolve) => {
    const server = http.createServer((_req, res) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(body);
    });
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

test("the panel page is served and /api/panel is proxied to the device", async () => {
  const device = await deviceServer();
  const panel = await startLegacyPanel({
    deviceBase: `http://127.0.0.1:${device.address().port}`,
    html: "<title>Summrise — this machine</title>",
  });
  try {
    const page = await fetch(`${panel.base}/legacy`);
    assert.equal(page.status, 200);
    assert.match(await page.text(), /this machine/);
    const snapshot = await fetch(`${panel.base}/api/panel`);
    assert.equal(snapshot.status, 200);
    assert.deepEqual(await snapshot.json(), { version: "2.0.0", events: [] });
  } finally {
    await panel.close();
    device.close();
  }
});

test("a device that refuses the proxy is a 502, not a dead page", async () => {
  const panel = await startLegacyPanel({ deviceBase: "http://127.0.0.1:9", html: "<title>x</title>" });
  try {
    const response = await fetch(`${panel.base}/api/panel`);
    assert.equal(response.status, 502);
    assert.deepEqual(await response.json(), { error: "device_unreachable" });
  } finally {
    await panel.close();
  }
});

test("anything else is a 404", async () => {
  const panel = await startLegacyPanel({ deviceBase: "http://127.0.0.1:9", html: "<title>x</title>" });
  try {
    assert.equal((await fetch(`${panel.base}/nope`)).status, 404);
  } finally {
    await panel.close();
  }
});

test("the status endpoint answers the legacy panel's shape from the device's own facts", async () => {
  const device = await deviceServer(
    JSON.stringify({ version: "2.0.0", data_dir: "C:\\ProgramData\\Summrise", events: [] }),
  );
  const panel = await startLegacyPanel({
    deviceBase: `http://127.0.0.1:${device.address().port}`,
    html: "<head></head>",
    activityHtml: "<head></head>",
    token: "t",
  });
  try {
    const response = await fetch(`${panel.base}/api/status`, { headers: { Authorization: "Bearer t" } });
    assert.equal(response.status, 200);
    const body = await response.json();
    assert.equal(body.ok, true);
    assert.equal(body.version, "2.0.0");
    assert.equal(body.release, "2.0.0");
    assert.equal(body.host, "127.0.0.1");
    assert.match(body.config_path, /etc\\config\.yaml$/);
  } finally {
    await panel.close();
    device.close();
  }
});

test("the event stream carries the device's tool calls as SSE frames", async () => {
  const device = await deviceServer(
    JSON.stringify({
      version: "2.0.0",
      events: [
        { event: "begin", id: 7, tool: "runs.exec", decision: "allow", ts_ms: 1000 },
        { event: "end", id: 7, ok: true, ts_ms: 1500 },
      ],
    }),
  );
  const panel = await startLegacyPanel({
    deviceBase: `http://127.0.0.1:${device.address().port}`,
    html: "<head></head>",
    activityHtml: "<head></head>",
    token: "t",
  });
  try {
    const frames = await new Promise((resolve, reject) => {
      const req = http.get(`${panel.base}/api/events`, { headers: { Authorization: "Bearer t" } }, (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          body += chunk;
          if ((body.match(/^data: /gm) || []).length >= 2) {
            req.destroy();
            resolve(body.split("\n\n").filter((f) => f.startsWith("data: ")));
          }
        });
      });
      req.on("error", () => {});
      setTimeout(() => reject(new Error("no SSE frames")), 5000);
    });
    const events = frames.map((f) => JSON.parse(f.replace(/^data: /, "")));
    assert.deepEqual(events[0], { event: "begin", id: 7, tool: "runs.exec", decision: "allow", ts_ms: 1000 });
    assert.deepEqual(events[1], { event: "end", id: 7, ok: true, ts_ms: 1500 });
  } finally {
    await panel.close();
    device.close();
  }
});

function fakeDevice(handlers) {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const handler = handlers[req.url.split("?")[0]] || handlers.default;
      if (!handler) { res.writeHead(404); res.end(); return; }
      handler(req, res);
    });
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

test("the terminal tools speak the legacy panel's shapes over the device's own endpoints", async () => {
  const device = await fakeDevice({
    "/panel": (_req, res) => {
      res.writeHead(200, { "content-type": "text/html" });
      res.end('<meta name="summrise-panel" content="' + "a".repeat(64) + '">');
    },
    "/api/terminal": (_req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ sessions: [{ id: "s1", kind: "pty", alive: true }] }));
    },
    "/api/terminal/create": (_req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ id: "s2", kind: "pty" }));
    },
    "/api/terminal/s2/close": (_req, res) => { res.writeHead(200); res.end("{}"); },
    "/api/terminal/s1/read": (_req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ text: "hello", data_base64: "", cursor: 5 }));
    },
  });
  const panel = await startLegacyPanel({
    deviceBase: `http://127.0.0.1:${device.address().port}`,
    html: "<head></head>",
    activityHtml: "<head></head>",
    token: "t",
  });
  try {
    const post = (name, body) => fetch(`${panel.base}/api/tools/${name}`, {
      method: "POST",
      headers: { Authorization: "Bearer t", "content-type": "application/json" },
      body: JSON.stringify(body),
    }).then((r) => r.json());

    const list = await post("terminal_list", {});
    assert.deepEqual(list, { ok: true, result: [{ id: "s1", kind: "pty", title: "pty", closed: false, active: true, rows: 24, cols: 80 }] });

    const opened = await post("terminal_open", { kind: "pty", target: "cmd", rows: 30, cols: 120 });
    assert.deepEqual(opened, { ok: true, result: "s2" });

    const read = await post("terminal_read", { session_id: "s1", offset: 0 });
    assert.deepEqual(read, { ok: true, text: "hello", end: 5 });

    const closed = await post("terminal_close", { session_id: "s2" });
    assert.deepEqual(closed, { ok: true });
  } finally {
    await panel.close();
    device.close();
  }
});

test("the logs route reads the device's own log directory", async () => {
  const { mkdtempSync, mkdirSync, writeFileSync } = await import("node:fs");
  const path = await import("node:path");
  const { tmpdir } = await import("node:os");
  const root = mkdtempSync(path.join(tmpdir(), "summrise-logs-"));
  mkdirSync(path.join(root, "logs"));
  writeFileSync(path.join(root, "logs", "device.log"), "first line\nlast line\n");
  const device = await fakeDevice({
    "/api/panel": (_req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ version: "2.0.0", data_dir: root, events: [] }));
    },
  });
  const panel = await startLegacyPanel({
    deviceBase: `http://127.0.0.1:${device.address().port}`,
    html: "<head></head>",
    activityHtml: "<head></head>",
    token: "t",
  });
  try {
    const response = await fetch(`${panel.base}/api/logs`, { headers: { Authorization: "Bearer t" } });
    const body = await response.json();
    assert.equal(body.ok, true);
    assert.equal(body.dir, path.join(root, "logs"));
    assert.equal(body.logs.length, 1);
    assert.equal(body.logs[0].name, "device.log");
    assert.equal(body.logs[0].present, true);
    assert.match(body.logs[0].log, /last line/);
  } finally {
    await panel.close();
    device.close();
  }
});
