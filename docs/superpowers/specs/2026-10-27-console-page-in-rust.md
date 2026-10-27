# The console page, in Rust

**Date:** 2026-10-27
**Status:** shipped. `/` redirects to `/next`, every page is maud-rendered by the console, the
terminal page runs pty/ssh/serial in tabs, and `device/console/web` (with its npm build, committed
bundle, CI job and `scripts/build-web.sh`) is deleted. The only JavaScript left is vendored xterm
and the terminal page's glue; `scripts/console-smoke.mjs` is its browser check.

## Problem

The product is Rust except one surface: `device/console/web`, a React + Vite
application whose `dist/index.html` the console embeds. It is a second language,
a second package manager, a second lockfile and a second CI gate — and it is the
only place where a product change is written in TypeScript.

`AGENTS.md` says "PREFER RUST. The runtime is Rust; a change that adds another
language needs a reason, not a habit." The page predates that rule; every page
change since has paid the exception. The legacy repository is not a
counter-example: its console **rendering** is still React (`gateway/ui`); only
its logic is moving to Rust (`gateway/ui-logic`). The product can do better than
porting the logic and keeping the rendering.

The visible symptom that started this: the new device is a headless service, so
a user sees nothing on the machine. That is fixed in Rust by the tray
(`device/shell`), not by the page.

## Decision

Serve the console page from the **Rust console** itself:

- `maud` (or `askama`) templates for the HTML; the page is the console's own
  routes, not a static bundle.
- **htmx** for partial updates (one small, vendored JS file) and plain forms
  for mutations.
- **xterm.js** only for the terminal surface; it is the one browser API the
  product cannot reasonably reimplement.
- An **HttpOnly session cookie** set by `POST /auth`, with CSRF tokens on
  mutations; the page no longer keeps a bearer in a JavaScript variable.
- Delete `device/console/web`, the npm build, the committed `dist/index.html`,
  and the `console` CI job's npm steps; the page is built by `cargo`.

Authored UI code becomes Rust. The remaining JavaScript is two vendored
libraries (htmx, xterm.js), not project source.

## Alternatives

- **Leptos/Yew (Rust → WASM).** All-Rust source, but a WASM toolchain, a client
  runtime, a hydration model and a second build to keep in step. The page is a
  forms-and-tables operator console; the Rust server already owns the routes,
  the session and the data. Server rendering is the smaller machine.
- **Keep React, move logic to Rust/WASM** (the legacy pattern).
  `gateway/ui-logic` shows it works, but it keeps React, Vite, npm and the
  second CI gate — the exact exception this spec removes.

## What moves, what stays

| today | after |
|---|---|
| `web/src/*.tsx`, `api.ts`, tests | Rust templates, handlers, Rust tests |
| `vite build` → embedded `dist/index.html` | `maud`/`askama` rendered by the console |
| bearer token in a JS variable | HttpOnly cookie + CSRF |
| `/api` calls from the page | same routes, called by the browser via htmx/forms |
| xterm.js in the terminal workspace | xterm.js, vendored, loaded only on the terminal page |
| npm lockfile, `build-web.sh --check` | no npm; the page is a build artifact of `cargo` |

## Phases

1. **The shell and the session.** Layout, login form, cookie session, CSRF,
   static assets, the audit/denial tables (read-only), the console health.
2. **The fleet.** Device list/cards, rename/remove, enrollment codes, the
   connect card (grant and permanent token), the Security page's fleet tokens.
3. **The terminal.** xterm.js, the console's terminal WebSocket, tabs, resize.
4. **The rest.** Vitals/monitors, runs, files, browser, screen, memory, events.
5. **The removal.** Delete `web/`, the npm CI steps, the build script; port the
   page's tests to Rust HTTP tests (and, where a browser is the only honest
   test, a Playwright check that does not own the build).

Each phase ships behind the same URL and keeps the previous page until it is
replaced, so the operator never loses a surface to a half-migration.

## Acceptance

- No `.ts`/`.tsx` under `device/console`; no npm/pnpm lockfile; no npm in CI.
- `GET /` is rendered by `cargo test`-covered Rust handlers.
- The operator session is an HttpOnly cookie; mutating routes reject a missing
  CSRF token.
- The terminal still works through xterm.js, loaded only there.
- The console's auth, audit and device routes are unchanged on the wire.

## Risks

- **The session model changes.** The current page holds a bearer in memory; a
  cookie needs CSRF and logout/expiry semantics. This is the security-critical
  part; it lands first, with tests.
- **The terminal is the hardest surface.** xterm.js and the console's WebSocket
  stay; the page around them changes.
- **Real-time data.** Vitals and health poll; htmx polling replaces the React
  interval, with the same cadence.
- **A big migration.** The phases above are independently shippable; the old
  page stays until its replacement is proven.

## Status

Proposed. The tray (Rust) is the first visible half; this spec is the second.
