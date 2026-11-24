# summrise-desktop

The legacy Electron local surface, vendored into the product: tray, window, native menu, single
instance, CDP/browser-session host — the app the operator asked for instead of the Rust/WebView2
`summrise-shell`.

## What is different from the legacy copy

- **It finds the 2.0.0 device, not the legacy agent.** `src/device-port.js` reads
  `%ProgramData%\SummriseHelper\device.port` (the user-readable copy the device writes), then
  `<data>\device.port`, then falls back to the legacy `SUMMRISE_AGENT_PORT` / `config.yaml` /
  18080 path. The app's liveness probe asks the device's `/api/health`.
- **It loads `/panel`.** The legacy agent served the panel at `/desktop/`; the device serves its own
  page at `/panel`. The window opens that page today.

## What is not done yet

The **legacy panel UI** (`panel-react`) is not wired up. Its pages call the legacy agent's
`/api/sessions`, `/api/settings`, `/api/workspace/*`, `/api/tools/*` and so on, which the 2.0.0
device does not serve; shipping it means building the SPA and adding a compatibility layer (or
mapping each page onto the device's capabilities). Until then the window shows the device's own
panel, and the tray/health facts come from the device.

## Commands

```
npm install
npm test            # node --test: shell policies, browser sessions, embedded views, device port
npm run typecheck   # tsc --noEmit
npm run dist -- --win dir   # dist/win-unpacked (no installer; no wine needed)
```

The tests read `device/tests/fixtures/embedded-bridge.json`.
