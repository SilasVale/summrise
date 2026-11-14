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
