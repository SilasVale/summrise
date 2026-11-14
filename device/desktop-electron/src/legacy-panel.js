"use strict";
// The legacy panel, served beside the device.
//
// The pre-rewrite panel is one HTML page that calls `GET /api/panel`. The device's origin belongs to
// the device, so the Electron shell serves the page from its own loopback origin and proxies that one
// path. Keeping the server here (rather than inline in main.js) is what lets the proxy be tested
// without a display, an Electron window, or a device.
const http = require("http");

function handler({ deviceBase, html }) {
  return (req, res) => {
    if (req.url === "/legacy" || req.url === "/") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
      res.end(html);
      return;
    }
    if (req.url.startsWith("/api/panel")) {
      const upstream = http.get(`${deviceBase}${req.url}`, (r) => {
        res.writeHead(r.statusCode || 502, {
          "content-type": r.headers["content-type"] || "application/json",
          "cache-control": "no-store",
        });
        r.pipe(res);
      });
      upstream.on("error", () => {
        res.writeHead(502, { "content-type": "application/json" });
        res.end('{"error":"device_unreachable"}');
      });
      return;
    }
    res.writeHead(404, { "content-type": "application/json" });
    res.end('{"error":"not_found"}');
  };
}

/** Start the panel on an ephemeral loopback port; resolves to its origin and a closer. */
function startLegacyPanel({ deviceBase, html }) {
  return new Promise((resolve, reject) => {
    const server = http.createServer(handler({ deviceBase, html }));
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
