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

- No `.ts`/`.tsx` under `device/console`; no npm/pnpm lockfile; no npm in CI. ✅
- `GET /` is rendered by `cargo test`-covered Rust handlers. ✅
- The operator session is an HttpOnly cookie; mutating routes reject a missing
  CSRF token. ✅ (30-day session, persisted across restarts as a token hash.)
- The terminal still works through xterm.js, loaded only there. ✅ (xterm plus a
  ~150-line glue per terminal page; the emulator is the one accepted exception —
  measured and argued in `plans/2026-10-27-terminal-in-the-browser.md`.)
- The console's auth, audit and device routes are unchanged on the wire. ✅

Beyond the original acceptance, the shipped page has:

- the legacy panel's shape — a 52px icon rail, a 244px context rail that changes
  per page, a canvas, a status strip — for every page, light and dark;
- responsive behaviour in three steps (the context rail goes first, then the
  icon rail narrows, then the content tightens; a wide table scrolls in its card);
- a contrast gate: `every_text_colour_reads_on_the_surface_it_sits_on` measures
  eleven pairs in each theme against WCAG AA's 4.5:1;
- a screenshot tool: `scripts/console-screenshots.bash` renders every page,
  photographs it light and dark, and with `--diff` reports which pages a change
  moved, per page, in pixels;
- keyboard entry: a skip link to `#content`, a caption on every table,
  `aria-current` on the rail, and a visible focus ring.

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

Shipped on 2026-10-27, in the phases this document described: the cookie session
first (with CSRF and tests), then the pages one at a time behind `/next`, then
the terminal, then `/` itself. The React page and its build are gone.

What the phases did not anticipate, and what the page learned in use:

- the operator notices **state**, not layout: "how long ago did this device
  check in", "is this token destructive", "what did the console just do" —
  which is why the fleet row carries an age, the token table names the access,
  and the fleet page ends with the console's own last six lines;
- a rail is only useful when it holds the page's own subject (sessions, runs,
  processes, targets, memory entries, disks), not a second copy of the
  navigation;
- "looks good" needs a gate: the contrast test and the screenshot diff are what
  keep a styling change from being an opinion.

## Acceptance record (2026-10-27, console 0.3.87)

The evidence for the paragraphs above, in the order a reader would want it:

| Check | Result |
|---|---|
| the device workspace's tests (`cargo test` over nine crates, CI's own command) | **1214 passed, 0 failed, 3 ignored** |
| clippy, every target, warnings as errors | clean |
| `cargo fmt --all -- --check` | clean |
| `scripts/console-screenshots.bash --out /tmp/acceptance` | **26 pages, 52 images**, gallery written |
| every live page (fleet, terminal, files, runs, system, state, memory, monitors, browser, tokens, vitals, audit, denials) | **13/13 answered 200**; nine carry a context rail |
| `/health` on the running console | `ok`, version 0.3.87, three devices |

## Acceptance record (2026-10-27, console 0.3.120)

The record above was measured at 0.3.87. This is the same measurement twenty-odd rounds later,
after the console gained the approvals gate, the install card, the vitals trend, the browser frame
pump and the approvals badge — measured with the commands the table names, in this session.

| Check | Result |
|---|---|
| the device workspace's tests (`cargo test` over nine crates, CI's own command) | **1289 passed, 0 failed, 3 ignored** (1214 at 0.3.87) |
| `cargo test -p summrise-console` alone | 108 unit + 246 integration + 1 bin |
| clippy, every target, warnings as errors | clean |
| `cargo fmt --all -- --check` | clean |
| `scripts/console-screenshots.bash --out …` | **28 pages, 56 images**, gallery written (26 at 0.3.87) |
| every live page (fleet, terminal, files, runs, system, state, memory, monitors, browser, tokens, vitals, audit, denials) | **13/13 answered 200** |
| `/health` on the running console | `ok`, version 0.3.120, three devices |

What the page gained since the first record, each with a test that can fail:

- **approvals** — a destructive call a token did not consent to is held, listed on the dashboard and
  counted in a badge on every page; approving re-sends it with consent for that call only, refusing
  or expiry returns the device's sentence, and a failed re-send answers "outcome unknown" instead of
  inviting a retry that would run it twice. The deadline is 90 s because Cloudflare Access cuts the
  origin at about 100 s — the plan's §9 says why that differs from the legacy gate.
- **install** — generations, the launcher's last swap with its reason, and its repairs, read from
  `host.facts` and kept nowhere in the console.
- **trend** — the vitals page's own visits, labelled "Not a sampler" so nobody reads it as a
  background series.
- **browser** — a frame pump at 1 fps, single-flight and paused while hidden, replacing one static
  capture. Not the CDP screencast the plan's P6 acceptance asked for; that remains the open build.
- **audit** — the filter offers every kind the log can hold, not the four it started with.
| the browser smoke (`scripts/console-smoke.mjs --terminal`) | `terminal ok — session session-4 · powershell.exe, 2 tabs` |

What is intentionally not here: a per-page pixel budget, a performance
benchmark of the rendered page, and a translated UI. Each is a new goal, not a
gap in this one.
