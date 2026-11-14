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
