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
| `panel_logic_bg.wasm` | **173,812 raw / 81,189 gz** | `agent/resources/panel/` | served by the agent beside panel.js, **fetched and compiled by `index.html`'s inline module and awaited before the first render** |
| `panel_logic.js` (the `--target web` glue) | **48,381 raw / 13,082 gz** | `panel-react/src/wasm/` | imported by the panel's loader; minified into panel.js |
| `panel_logic.d.ts` | — | `panel-react/src/wasm/` | `tsc --noEmit` needs it for the glue's types |

**These numbers move with every family, and they are the FIRST-LOAD payload's business only where the
glue and `index.html` are.** The wasm is a separate fetch — it is not in the first-load payload — and
the glue is minified into `panel.js`, so it is. Measured per family, with
`docs/superpowers/p0/sizes.sh`:

| family | wasm gz | glue gz (first-load) | first-load payload gz |
|---|---|---|---|
| the floor, before anything moved | 10,003 | 3,076 | 271,487 (P0's baseline) |
| `archive.rs` (1 export) | 10,003 | 3,076 | **273,636** (+2,149, its commit) |
| `boot.rs` (1 export) | 11,494 (+1,491) | 3,121 (+45) | **273,532** (−104, its commit) |
| `monitors.rs` (2 exports) | 14,173 (+2,679) | 3,603 (+482) | **273,252** (HEAD re-measured with the same instrument: 273,523 → **−271**) |
| `runs.rs` (2 exports, the first RENDER-path family) | 23,167 (+8,994) | 4,268 (+665) | **272,375** (**−877**) |
| the eight families after it, to `attention.rs` | 26,159 | 5,460 | **272,344** (the tree the preload landed on) |
| `session_labels.rs` (1 export) | 27,967 (+1,808) | 6,141 (+681) | **273,160** (+816) |
| `liveness.rs` (7 exports) | 29,877 (+1,910) | 7,840 (+1,699) | **273,240** (+120) |
| `path.rs` (5 functions + the endings table) | 38,487 (+8,610) | 8,617 (+777) | **272,623** (**−617**) |
| `events.rs` (4 functions: the two groupings, the marker rule, ANSI) | **62,553** (+24,066) | **9,382** (+765) | **271,904** (**−719**) |
| `update.rs` (5 functions: the four-way log verdict, the update readers, two age formatters) | **68,025** (+5,472) | **10,092** (+710) | **279,038** (**−607**) |
| `spark.rs` (5 exports: the polyline geometry, its summary, the sustained-load rule) | **72,125** (+4,100) | **10,802** (+710) | **278,585** (**−453**) |
| `recipe.rs` (3 functions: the recipe body, its title, its warnings) | **75,962** (+3,837) | **11,183** (+381) | **278,181** (**−404**) |
| `boot_notice.rs` (3 functions: the kind's words, the crash test, the chip) | **76,964** (+1,002) | **11,729** (+546) | **277,950** (**−231**) |
| `adopt.rs` (3 functions: the paging decision, the page bound, the write slices) | **77,536** (+572) | **11,950** (+221) | **277,998** (**+48**) |
| `idle.rs` (4 functions: the silence formatter, the idle rule, its one-line offer, the view prune) | **80,919** (+3,383) | **12,804** (+854) | **278,135** (**+137**) |
| `version.rs` (2 functions: which of the device's two versions wins, and its label) | **81,189** (+270) | **13,082** (+278) | **278,117** (**−18**) |

**AND `spark.rs` IS THE ONE FAMILY WHOSE WASM COST WAS MEASURED TWICE, because the first version was
8 KB more expensive than the family.** It formats coordinates with `toFixed(2)`, and the first port
reached for `format!("{:.2}", f64)` — which LINKS RUST'S FLOAT FORMATTER: **80,038 gz, +12,013 for the
family**, the largest single-family jump this crate has taken. The digits are computed with integer
arithmetic now (exact, from the float's own mantissa and exponent) and the same 2,763-case differential
passes: **72,125 gz, +4,100 — the decision was worth 7,913 gz.** It is P0's `{:.3}` lesson (8,879 gz,
26% of the landing page's payload) arriving in the panel, and the fix is the same one.

**THREE FAMILIES HAVE COST THE PAYLOAD SOMETHING, AND ALL THREE ARE SMALL**: `adopt.rs` +48 gz
(0.017 %), the console's `mask_token` +72 gz (0.06 %) and `idle.rs` +137 gz (0.05 %). All are kept, with
the numbers stated — the rule is "if a move makes the payload CLEARLY worse, keep the JavaScript", and
none of these is that. Every other family in this table took the payload DOWN.

**AND THE PAYLOAD COLUMN IS MEASURED AGAINST THE TREE THE FAMILY LANDED ON, not against the row above
it** — the panel gains product features between families, so `update.rs`'s before is 279,645 gz at
`445a4eca` and its after is 279,038: the family is worth **−607 gz** even though the number is larger
than `events.rs`'s 271,904. A column read as a sequence would say the panel grew by 7 KB in one
family, which is the opposite of what happened.

**The first-load column is the one the operator pays, and for six of the eight families it went DOWN**
— the TypeScript a family deletes is larger than the glue it adds. `session_labels.rs` is the
exception and the reason is not the family: **the inline module in `index.html` costs +482 gz of the
+816**, and it is a ONE-TIME cost that every later family benefits from. **`liveness.rs` is the other
exception and it is the honest shape of a seven-export family: +1,699 gz of glue for ~1,580 gz of
deleted TypeScript, i.e. +120 gz on the payload — the widest family so far costs about a tenth of a
percent of it.** The glue's raw growth (6,081 bytes) is much larger than its minified share, which is
why the column is measured on `panel.js` and not on the glue file. The wasm column is what a
page pays once, separately, and `runs.rs` remains by far the most expensive family (~350 lines of
`lib/runs.ts`, the panel's first `String.prototype.localeCompare`, its first UTF-16-aware slice, and
a fold that allocates a JS object per row of an 18-field shape).

**All three artifacts are committed.** A checkout has no `wasm-pack`, and both CI jobs (`panel`,
`agent`) build from the committed artifact — the same arrangement `resources/panel/panel.js` has
always had.

**A CHANGE HERE OWES A REBUILD.** `agent/build.rs` folds the wasm into `PANEL_BUNDLE_HASH` (so a
changed wasm changes the panel's cache key) and fails the agent build if the artifact is missing —
but its staleness gate compares the newest SOURCE against the newest PRODUCT, so it cannot see a
rebuild that refreshed `panel.js` while leaving a stale wasm behind. **Run `build.sh` when you change
this crate, then `npm run build` in `panel-react`** (the TS wrapper may need the new export names).

## What has moved, and what a move costs

| module | the TypeScript it replaced | exports |
|---|---|---|
| `archive.rs` | `lib/archive.ts`'s `archiveEntries` | 1 |
| `boot.rs` | `hooks/useBootHistory.ts`'s `parseBootHistory` | 1 |
| `monitors.rs` | `hooks/useMonitors.ts`'s `parseMonitors` + `parseMonitorChange` | 2 |
| `runs.rs` | `lib/runs.ts`'s `groupOperation` + `operationRows` | 2 |
| `session_labels.rs` | `lib/sessionLabels.ts`'s `disambiguateLabels` | 1 |
| `liveness.rs` | `lib/liveness.ts`'s seven predicates (`livenessOf`, `deviceLiveness`, `sessionWaiting`, `sessionActive`, `anyCommandRunning`, `sessionLiveness`, `sessionFailed`) | 7 |
| `path.rs` | `lib/path.ts`'s `stateFromEnd`, `cardState`, `derivePath`, `summarizePath`, `attentionSteps`, and the `END_STATE`/`END_LABEL` tables | 5 |
| `events.rs` | `useCommandEvents.ts`'s `terminalStatus` + `groupEvents`, `useTrajectory.ts`'s `groupRounds`, `lib/ansi.ts`'s `stripAnsi` | 4 |
| `update.rs` | `lib/updateDiagnosis.ts`'s `diagnoseUpdate`, `components/UpdateCard.tsx`'s `parseUpdateStatus`, `parseAttempt`, `checkedAge`, `attemptAge` | 5 |
| `spark.rs` | `lib/spark.ts`'s `sparkSegments`, `seriesStats`, `loadNotice` (its two exported numbers stay in TypeScript and are passed in) | 3 |
| `recipe.rs` | `lib/recipe.ts`'s `buildRecipe`, `suggestedTitle`, `recipeWarnings` (its marker and tag stay in TypeScript and are passed in) | 3 |
| `boot_notice.rs` | `lib/bootNotice.ts`'s `bootNotice`, `bootKindLabel`, `isCrash` (the notice window stays in TypeScript and is passed in; the boot vocabulary comes from `vocabulary.rs`, not a third copy) | 3 |
| `adopt.rs` | `lib/terminalAdopt.ts`'s `adoptNeedsAnotherPage`, `adoptPageExceeded`, `splitWriteSlices` (both numbers stay in TypeScript and are passed in — the slice budget is also the JavaScript DEFAULT, which the wrapper decides) | 3 |
| `idle.rs` | `lib/duration.ts`'s `humanIdle`, `lib/idleSessions.ts`'s `idleSessions` + `idleOfferText`, `lib/sessionViews.ts`'s `pruneSessionViews` (the threshold stays in TypeScript, and it is also the JavaScript DEFAULT) | 4 |
| `version.rs` | `lib/agentVersion.ts`'s `releaseVersion`, `releaseVersionLabel` — AND `agent/tests/device_version_rule.rs` followed the rule into the crate, because it holds this half equal to the gateway's `wireVersion` | 2 |

## THE SYNC STORY, AND IT ENDED ON 2026-09-29

**A migrated function CAN be called synchronously during render.** That was false for the whole of P2
up to this point, and it was the constraint every remaining family was classified against: with the
module fetched at the first call, React's synchronous render could not wait for a fetch, so a family
moved only if its derivation could be re-derived at the DATA BOUNDARY. `runs.rs` is the worked example
of that road and it is still the right shape for the families that took it:

```
before   fetch → {events, boundaries} → render → useMemo(groupOperation) → useMemo(operationRows)
after    fetch → {events, boundaries, groups, rows} → render reads `groups` / `rows`
```

**WHAT CHANGED IS THE LOAD, NOT THE ROAD.** `index.html` now carries an inline module that fetches and
`WebAssembly.compileStreaming`s the artifact while panel.js is still downloading, and `main.tsx` awaits
it before it mounts anything. The compile is the part that mattered: a `<link rel="preload">` starts
the fetch at the same moment but leaves the compile to the glue, i.e. to the first render, and it cost
**+11 ms** (11 runs each, median 133.8 → 145.0). Started in the inline module, it overlaps the bundle's
own download, and the alternating A/B (10 pairs, one browser, `/tmp/ab/ab.mjs`) answers the other way:

```
lazy (first call)     median 141.4 ms   mean 142.4
inline + await        median 128.6 ms   mean 126.7
paired difference     median −12.6 ms   mean −15.6   negative in 10 of 10 pairs
```

**So the eager load is 12.6 ms FASTER, which is not the answer the plan expected** — and the number is
the first child of `#root`, not FCP, because `main.tsx` starts the decorative particle field before
React mounts and FCP is the canvas (measured: FCP 88 ms while the module was still in flight).

**THE THREE PRECONDITIONS ARE THEREFORE NO LONGER A FILTER.** The boundary road is still available and
still right where a family's derivation belongs with its data, but a family that fails all three —
`disambiguateLabels` was the plan's own example, because its input is a SUBSET chosen by the call site
(`TabBar` numbers `sessions`, `DesktopShell` numbers `openTabs`) — now moves as a plain synchronous
call. `session_labels.rs` is that family, and both strips call it with whatever list they render.

**AND THE RULE THAT CAME WITH THE BOUNDARY ROAD STILL HOLDS: a function that reads the DERIVED value is
not a second derivation.** `groupCount`, `runStateNote`, `RUN_STATE_LABEL` and `ActivityPage`'s `extent`
read the grouped result, so they cannot disagree with it. What moves is everything that reads the WIRE.

**AND THE ONE CONSTANT THAT HAS TO EXIST IS CHECKED.** A reader starts from an empty value before the
first reply lands, so `EMPTY_GROUPS` (`lib/runs.ts`) spells what `groupOperation([], [])` answers — and
the panel's own suite asserts that the wasm's answer for the empty input IS that object. A claim
nobody checked is a lie the reader believes; this one is checked, and the check carries a non-vacuous
probe beside it.

**WHAT IS STILL RENDER-PATH, AND WHAT EACH ONE COSTS NOW.** `lib/path.ts`'s `derivePath` +
`attentionSteps`, `cardState`/`stateFromEnd`, and `fmtSince` / `unstableTargets` / `downTargets` in
`useMonitors.ts` are all still TypeScript — **and `liveness.rs` is the proof that the road is open**:
seven predicates, every one of them called during render, moved with their signatures unchanged and
the panel's own test file untouched. **None of the rest is blocked either** — each is a move that has
not been made, and the only question left per family is whether its derivation belongs at the data
boundary or in the component.

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
computes nothing, it talks to the platform). It exposes two doors: `panelLogic()`, the promise the
loader has always been, and **`logic()`, the SYNCHRONOUS one a render-path call site uses.**

**`index.html` fetches and compiles the artifact while panel.js is still downloading, and `main.tsx`
awaits it before the first render** — so `logic()` is legal from inside a component, and it THROWS
rather than falling back to a JavaScript copy when it is reached before the load (a silent second
implementation is the thing this migration exists to delete). Under vitest, `src/test-setup.ts`
awaits the same bytes off disk before any test body runs, so a component test needs no `beforeAll`.

**THE MEASUREMENT IS IN THE SYNC STORY ABOVE, and it is the reason the load changed**: the inline
module is **12.6 ms faster** to the panel's first frame than fetching at the first call, because the
module's compile stops competing with React's first render. `wasm/panelLogic.ts`'s header carries the
samples and the `<link rel=preload>` variant that lost (+11 ms).

**THE CONSEQUENCE IS THAT THE MIGRATION'S OLD CONSTRAINT IS GONE.** A move is no longer restricted to
call sites that were already asynchronous: `newestFirst`/`pageOf`/`lastEventWords` (an `ArchivePage`
render path), `fmtSince`/`unstableTargets`/`downTargets` (a `MonitorChip` render path), `derivePath`
and `livenessOf` are all still TypeScript because their move has not been made, not because there is
no way to make it.

**vitest and the browser load THE SAME BYTES FROM THE SAME FILE**, by the two doors each has:
`readFileSync` + `initSync` under vitest (there is no server to fetch from), and in the browser the
inline module's `fetch` + `WebAssembly.compileStreaming`, whose URL carries panel.js's own content
hash. That is deliberate: a test that loaded a *different* artifact than the operator's browser would
be a test of a different program.

## Where the artifact is served

`agent/src/web/panel.rs` whitelists it by name beside panel.js and serves it as
`application/wasm` (the one type here the browser does not sniff — `WebAssembly` refuses a module
served as anything else). It is embedded with `include_bytes!`, not `include_str!`: a wasm module is
not text.
