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

function makeHandler({ deviceBase, html, desktopDir, token, activityHtml, capture }) {
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
