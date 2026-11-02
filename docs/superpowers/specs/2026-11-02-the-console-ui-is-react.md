# The console UI is React, and the logic is Rust/WASM

**Date:** 2026-11-02. **Status:** decision; the React base is restored, the legacy design and
WASM logic are the next slices.

## Decision

The product console's **rendering layer is React 19 + Vite (one self-contained `index.html`)**.
The **runtime, routes, auth and data remain Rust**. Pure decisions — formatting, state derivation,
filters, validation — move to a **Rust/WASM crate** (`console-logic`) behind a thin TypeScript seam.
The page is embedded in `summrise-console` with `include_str!`; there is **no Node at runtime**, no
Electron, no Playwright and no cloudflared in the default install.

This reverses the rendering half of PR #61, not the runtime half. The Rust console page (maud) stays
as a fallback while the React app reaches parity; it is removed only after the React page serves the
same routes, auth and data.

## Why

PR #61 made the page Rust because "PREFER RUST" and because a page in the binary is one deploy
artifact. Both are real. But the result was **deployment-first, UI-last**: the page is server-rendered
forms and tables, while the product it replaces (the legacy panel/console) was React with a measured
Rust/WASM logic layer. The operator's judgement is the product requirement: **legacy's page design is
the one they are satisfied with.**

React is better here for the things this UI is made of: a tabbed terminal workspace, live session
state, a waiting/takeover surface, keyboard/focus contracts, and component tests. It also lets the
team reuse the legacy **design grammar** (rail, context rail, status strip, cards, type/spacing) and
rebuild it faithfully instead of approximating it in maud.

The legacy proof is concrete:

- `agent/resources/panel-logic` compiles the panel's pure decisions to WASM (`wasm-bindgen` pinned,
  `opt-level=z`, no serde); the panel's wasm is ~104 KB on disk.
- `gateway/ui-logic` does the same for the console; the served wasm is ~48 KB.
- The React shell was ~373 KB JS / ~39 KB CSS for the console; the panel was ~1.46 MB static.
- The legacy install was heavy because of **Electron (115 MB), cloudflared (54.8 MB), Playwright
  (~31 MB) and npm** — not because React was built at build time.

## Constraints

1. **The page is one artifact in the binary.** `dist/index.html` is committed; `npm run build` must
   reproduce it byte-for-byte; `scripts/build-web.sh --check` and CI fail on a stale bundle.
2. **No bearer in JavaScript.** The React app uses the console's HttpOnly session cookie and the CSRF
   token on mutations. The old app's `setToken`/localStorage path is not the target; the API/auth
   seam is adapted before the React page is the default.
3. **CI owns the npm steps.** `scripts/check.sh` must replay `npm ci`, `typecheck`, `test` and the
   bundle check, or the rule "run the command the other end runs" is false.
4. **No runtime Node, no second update path.** React is a build dependency. The console binary, the
   device binary and the shell remain the only shipped runtimes.
5. **The Rust page stays until parity.** `/next` keeps serving the maud page while React is served at
   `/` (or `/app`); the switch happens page by page, with a route test and a screenshot per page.

## Shape

```text
device/console/web/          React 19 + Vite singlefile (restored as the base)
  src/App.tsx                the shell: rail, topbar, status strip
  src/api.ts                 thin fetch seam (cookie + CSRF)
  src/workspace/             Terminal / Screen / Files / Runs / Machine / Activity
device/console-logic/        Rust -> WASM: pure formatting/state/filters (new)
device/console/src/http.rs   JSON routes + auth; `include_str!` the built page
device/ui/                   the one token palette, generated into the page's CSS
device/shell/                WebView2 loads the same page; no Electron
```

## What is not decided here

- The page's IA and the approval removal. Those are product decisions recorded separately; this
  document only fixes the rendering stack.
- The old React app's visual design. It is restored as **infrastructure** (API client, xterm
  adapters, tests), not as the final look; the legacy tokens and components replace it.

## Verification

- `cd device/console/web && npm ci && npm run typecheck && npm test && npm run build` — 34 files /
  334 tests, build green at the time this decision was recorded.
- `scripts/build-web.sh --check` after the Rust `include_str!` wiring.
- The console's existing cargo suite, layout gate and screenshots stay green.

## Consequences

- A second build/language returns; it is build-time only, and it buys the UI the product needs.
- The old React page's auth (bearer in JS) and stale `dist` are debts to pay in the first slice, not
  patterns to preserve.
- If React cannot reach the legacy interaction quality without a second runtime, the decision is
  wrong; the fallback is to keep the maud page, not to add Electron.
