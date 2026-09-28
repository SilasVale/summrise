# panel-logic — the panel's LOGIC, in Rust, compiled to wasm

P2 of [`docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md`](../../../docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md).
The work list is P0's inventory, [`docs/superpowers/p0/README.md`](../../../docs/superpowers/p0/README.md):
**156 LOGIC exports across 46 files** in the panel. This crate is where they move, **one family per
commit**, and it is a crate of FUNCTIONS — React, Radix and Tailwind stay exactly where they are.

```bash
cd agent/resources/panel-logic
WASM_OPT=/tmp/wasmopt/node_modules/binaryen/bin/wasm-opt ./build.sh
```

`WASM_OPT` is optional if `wasm-opt` is on PATH (`npm i binaryen` gives one). The command builds,
optimizes, installs all three artifacts and prints their sizes.

## What it emits, and where each piece goes

| artifact | size | goes to | why there |
|---|---|---|---|
| `panel_logic_bg.wasm` | **21,275 raw / 10,003 gz** | `agent/resources/panel/` | served by the agent beside panel.js, **fetched at the panel's first call** |
| `panel_logic.js` (the `--target web` glue) | **11,067 raw / 3,076 gz** | `panel-react/src/wasm/` | imported by the panel's loader; minified into panel.js |
| `panel_logic.d.ts` | — | `panel-react/src/wasm/` | `tsc --noEmit` needs it for the glue's types |

**All three are committed.** A checkout has no `wasm-pack`, and both CI jobs (`panel`, `agent`) build
from the committed artifact — the same arrangement `resources/panel/panel.js` has always had.

**A CHANGE HERE OWES A REBUILD.** `agent/build.rs` folds the wasm into `PANEL_BUNDLE_HASH` (so a
changed wasm changes the panel's cache key) and fails the agent build if the artifact is missing —
but its staleness gate compares the newest SOURCE against the newest PRODUCT, so it cannot see a
rebuild that refreshed `panel.js` while leaving a stale wasm behind. **Run `build.sh` when you change
this crate, then `npm run build` in `panel-react`** (the TS wrapper may need the new export names).

## The numbers this crate was chosen by, measured before it was written

Same machine, same `wasm-opt -Oz`, three ways of passing the panel's data across the boundary:

| FFI | wasm | glue | total gz | over the floor |
|---|---|---|---|---|
| P0's proof crate (`add` + `echo`) — **the floor** | 7,016 | 2,089 | 9,105 | — |
| `serde_json` (a JSON string in, a JSON string out) | 43,824 | 2,140 | 45,964 | **+36,859** |
| `serde-wasm-bindgen` (`JsValue` in, `JsValue` out) | 31,046 | 3,967 | 35,013 | +25,908 |
| **this crate** (`JsValue` + `js_sys`, no serde) | **10,003** | **3,076** | **13,079** | **+3,974** |

For scale: the panel's whole first-load payload is **271,487 gz** and the console's is **112,718 gz**
(P0's baseline), so `serde_json` would have been **13.5% of the panel's payload and 39% of the
console's** — for a serialization round trip this crate does not need, because the panel's parsers
are already handed parsed JS values. The brief's warning (one `format!("{:.3}")` on an `f64` cost
8,879 gz) is the same class of cost, four times smaller.

**The comparison is the reason the API is `JsValue`-shaped rather than JSON-shaped**, and the reason
the port is a transliteration rather than a `#[derive(Deserialize)]`: a derive would answer
differently for the shapes the panel actually receives (`{state:{}}` is an ABSENCE in
`lib/archive.ts`, and a derive would call it a struct with default fields). See `src/archive.rs`.

## How it is loaded, and what that decides

`panel-react/src/wasm/panelLogic.ts` is the seam (P0 classifies such an export as **BOUNDARY** — it
computes nothing, it talks to the platform). It loads the artifact **at the first call, never at page
load**, which is criterion ③ of the plan: the wasm is in neither the first-load payload nor the
page's critical path.

**THE CONSEQUENCE IS THE MIGRATION'S REAL CONSTRAINT, so it is stated here rather than discovered
per file: a migrated function cannot be called synchronously during render.** A move is free exactly
where the call site is already asynchronous — a `useDeviceRead` fold, an SSE handler, an event
handler — which is why `archiveEntries` is `async` and `newestFirst`/`pageOf`/`lastEventWords`
(a `ArchivePage` render path) are still TypeScript. Deciding the sync story (inline bytes: +X gz on
the first-load payload, measured; or deriving at the data boundary instead of during render) is what
unblocks the render-path majority.

**vitest and the browser load THE SAME BYTES FROM THE SAME FILE**, by the two doors each has:
`readFileSync` + `initSync` under vitest (there is no server to fetch from), `fetch` of
`panel.js`'s own directory in the browser. That is deliberate: a test that loaded a *different*
artifact than the operator's browser would be a test of a different program.

## Where the artifact is served

`agent/src/web/panel.rs` whitelists it by name beside panel.js and serves it as
`application/wasm` (the one type here the browser does not sniff — `WebAssembly` refuses a module
served as anything else). It is embedded with `include_bytes!`, not `include_str!`: a wasm module is
not text.
