# The device's events, a terminal that zooms, and the files page's tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** put the device's own event tail on the wire and in front of the operator, let the terminal's
text size be adjusted (and remembered), and give the largest untested console page — Files — the tests
its state machine has never had.

**Architecture:** `state.get` grows one additive field, `events`, read best-effort from the device's
existing `events.jsonl` tail (the same ten the local panel shows). A new console page renders it. The
font size is a pure module plus one effect in `TerminalWorkspace`. The Files tests are test-only and
mock `callTool`, so no production line changes for them.

**Tech Stack:** Rust (device `mcp.rs`, registry description), React 19 + TypeScript, vitest/jsdom/RTL.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` § "Worth doing later" (device
events page; terminal font size); the local panel's shape is `device/device/src/http.rs::panel_snapshot`
(`tail_events(10)`), and the audit page's rendering is the model for the new page.

## Global Constraints

- **`state.get` stays one tool.** No new registry entry, no tool-count change, no input schema change.
  The event tail is an additive field of the answer, best-effort: a **missing** `events.jsonl`
  contributes nothing (the call's own audit line is still the tail), an **unreadable** one is `null`
  with a sentence rather than "no events", and neither fails the call; a poisoned state document still
  returns its events. An older device that sends no field reads as an empty tail.
- Console web only for the page and the font: `device/console/web/src/*`, the committed
  `dist/index.html`, tests. No new dependency.
- Every console device call is `callTool(grant, …)`; no console route; no credential stored.
- The font size lives in `localStorage` under one key (a preference, not a secret), clamped to
  10–20, default 13; it never travels over the wire.
- English UI and comments. `npm run typecheck` 0; `npm test` green; `./scripts/build-web.sh` and the
  committed bundle agree. The device change runs `cargo test -p summrise-device`, clippy and fmt.
- The App shell test's rail list grows to eleven sections; that test is part of the events task.

## Review Focus

1. **A poisoned document is not an empty page, and an unreadable log is not an empty one either.**
   `state: null` + `health.poisoned: true` must render the degraded health and the events that still
   exist, not a blank panel or a fake "offline"; `events: null` says the log could not be read, and
   only `[]` says the device has recorded nothing.
2. **An event is whatever the device wrote.** Tool-call lines are `{ts_ms, event: "begin"|"end",
   tool, ...}`, state lines are `{ts_ms, kind: "repaired"|…, ...}`, and the store falls back to a JSON
   string for a line that is not an object. The label is `kind` or `event`, the other keys are fields,
   and a non-object line is text — never `[object Object]`, even when a label is itself an object
   (JSON, or nothing).
3. **The device's order is not the operator's.** The tail arrives oldest-first; the page shows newest
   first without mutating the array it was given.
4. **A font change reaches every open tab and the next one.** Existing xterms are updated in place and
   refitted; a new tab is created at the current size; a reload reads the stored size; the clamp wins
   over a stored 99 or a NaN.
5. **The Files tests pin behaviour, not markup.** A refusal is the device's sentence; a directory change
   is a new list; a save and a mkdir each call the tool with the exact arguments; no test asserts a
   class name that a later style change would break.

---

### Task 1: The device's event tail, on the wire and on a page

**Files:**
- Modify: `device/device/src/mcp.rs` (`Handler::StateGet`, its comment, the state.get tests)
- Modify: `device/core/src/registry.rs` (the `state.get` description only)
- Create: `device/console/web/src/EventsPage.tsx`, `device/console/web/src/EventsPage.test.tsx`
- Modify: `device/console/web/src/Rail.tsx` (the page union, label, icon, rendered list)
- Modify: `device/console/web/src/App.tsx` (an `events` block with the picker → grant → page shape)
- Modify: `device/console/web/src/App.test.tsx` (the rail list is now eleven sections)
- Modify: `device/console/web/src/app.css` only if a class is genuinely new

**Interfaces:**
- `state.get {}` now answers `{state, health, events}`; `events` is `serde_json::Value[]`, at most the
  10 newest, oldest-first, each the line the device wrote (an object, or a string for a malformed line).
  **The tail is the device's audit log**, so it contains the MCP surface's own begin/end lines,
  including the `state.get` call being answered: tool-call lines are `{ts_ms, event: "begin"|"end",
  tool, ...}`, state lines are `{ts_ms, kind: "repaired"|"state_quarantined", ...}`. The page reads the
  label from `kind` or `event` and never repeats it among the fields. `events` is `null` when the
  device could not read its own log; the page names that case instead of calling it empty.
- Produces: `EventsPage({grant}: {grant: Grant})`; `Page` gains `"events"`, labelled **Events**.
- The events page's own copy: **The device keeps its last ten events.** and, for an empty tail,
  **The device has recorded no events.**

- [ ] **Step 1: The device half, test first**

In `mcp.rs`, add a test that starts a device, writes two object lines and one bare-string line to
`<data>/events.jsonl`, calls `state.get`, and asserts the three survive in order, the string as a
string, and this call's own audit begin line is the newest (the surface audits before dispatch). A
missing or unreadable log is `[]` **for the log's contribution** — the test asserts the call's own
line is still the only event, which is the truthful shape. Also extend the existing
`state_get_returns_the_document_and_health_over_mcp` to assert `events` is an array and carries that
call's line. Then add to `Handler::StateGet`:

```rust
// Best-effort, like the local panel: a device that cannot read its own event log still answers
// the document, and the console shows an empty tail rather than an offline device.
let events = self.store.tail_events(10).unwrap_or_default();
```

and return it as `"events": events`. Update the handler's comment and the registry description to
name the tail ("the last ten events the device recorded"). No schema/args change.

- [ ] **Step 2: The console page, test first**

`EventsPage.test.tsx` mocks `./terminal`: `state.get` → `{state: {...}, health: {status:"ok",
version:"0.3.44", pid: 42, poisoned:false}, events:[{ts_ms:1,kind:"repaired",from:"baseline"},
{ts_ms:2,kind:"state_quarantined"}]}`. Assert: both events render, newest first, each with its kind and
its extra field; the health line names the version and pid; the caption says the device keeps ten.
A second test: `health.poisoned: true`, `state: null`, one event → the degraded sentence, the reason if
present, and the event still visible. A third: `events: []` → the empty sentence. A fourth: the call
rejects with a `ToolError` → the tool's sentence, and no invented rows.

`EventsPage` reads once on mount and on **Refresh**; it shows the device's health (status, version,
pid) in a `.facts` list and the events in a table: local time from `ts_ms`, kind, and the other keys as
`key=value` (a string event is the whole cell). Render a copy reversed for newest-first.

- [ ] **Step 3: Wire the rail and the App**

`Page` gains `"events"` after `"audit"` (the rail order ends **Audit**, **Events**), label **Events**,
icon: a list with a dot (24×24; do not reuse Audit's page path). `App` renders the page with the same
three states as Screen (no device → picker; no grant → `GrantForm`; grant → `EventsPage`). Update
`App.test.tsx`'s rail assertion to the eleven labels.

- [ ] **Step 4: Verify and commit**

```bash
source ~/.cargo/env
cargo test --manifest-path device/Cargo.toml -p summrise-device
cargo clippy --manifest-path device/Cargo.toml -p summrise-device --all-targets -- -D warnings
cargo fmt --manifest-path device/Cargo.toml --all -- --check
cd device/console/web && npm run typecheck && npm test
cd - && ./scripts/build-web.sh
```

Commit as `device+console: the device's event tail is on the wire and on a page` with a body naming the
best-effort rule and the poisoned-document case.

### Task 2: A terminal that zooms

**Files:**
- Create: `device/console/web/src/terminalFont.ts`, `device/console/web/src/terminalFont.test.ts`
- Modify: `device/console/web/src/TerminalWorkspace.tsx` (state, the instance effect, an update effect,
  two buttons)
- Modify: `device/console/web/src/TerminalWorkspace.test.tsx`

**Interfaces:**
- Produces from `terminalFont.ts`:
  ```ts
  export const MIN_FONT_SIZE = 10;
  export const MAX_FONT_SIZE = 20;
  export const DEFAULT_FONT_SIZE = 13;
  export const FONT_SIZE_KEY = "summrise.console.terminal.fontSize";
  export function clampFontSize(value: number): number;   // finite, rounded, clamped; NaN → default
  export function loadFontSize(storage: Pick<Storage, "getItem"> | null): number;
  export function saveFontSize(storage: Pick<Storage, "setItem"> | null, size: number): void;
  export function fontStorage(): Storage | null;   // the one place window.localStorage is touched
  ```
  `fontStorage` exists because evaluating `window.localStorage` itself can throw when the browser
  blocks storage; the component passes its answer to both functions. The effect saves on mount too,
  which normalises a malformed stored value.
  A storage that throws (private mode) leaves the default; a stored value that is not a number is the
  default.

- [ ] **Step 1: Pure tests first**

Clamp: 9→10, 21→20, 13.6→14, NaN→13, `Infinity`→20. Load: null storage → 13; a fake storage returning
`"16"` → 16; `"abc"` → 13; `"99"` → 20. Save writes the clamped value under `FONT_SIZE_KEY`; a throwing
storage does not throw.

- [ ] **Step 2: Implement and wire**

`TerminalWorkspace` holds `const [fontSize, setFontSize] = useState(() => loadFontSize(window.localStorage))`.
`new Terminal({..., fontSize})`. An effect on `fontSize` updates every instance in `instances.current`
(`instance.term.options.fontSize = fontSize`) and calls `fit.fit()` in a try, then `saveFontSize`. Add
**A−** and **A+** buttons beside the other actions (disabled at the bounds), with the current size as
their title; changing them calls `setFontSize(clampFontSize(fontSize ± 1))`.

- [ ] **Step 3: The component test**

Extend the harness so the mocked `Terminal` records its instances (a hoisted array). Open a tab, assert
the instance's `options.fontSize` is 13; click **A+**, assert 14 and that the stored value is `"14"`;
with a pre-set `"18"` before render, a new tab's instance is 18. Clear the storage key in
`afterEach`.

- [ ] **Step 4: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: the terminal's text size is adjustable and remembered`.

### Task 3: The Files page's tests

**Files:**
- Create: `device/console/web/src/FilesPage.test.tsx` (test-only)

**Interfaces:**
- Mock `./terminal`'s `callTool`; the page renders with a `Grant`. No production change.

- [ ] **Step 1: Read the page's state machine, then write the tests**

`FilesPage` calls `files.list {path}`, `files.read {path}` (utf8, and `encoding: "base64"` for a
download), `files.write {path, content}` (and the base64 form for upload), `files.mkdir {path}`. Cover:

1. the root lists, a directory click lists that path, the breadcrumb says where you are;
2. opening a text file calls `files.read` with its path and shows the text; closing returns to the list;
3. saving sends the edited text with `files.write` and the page says it saved;
4. a new folder sends `files.mkdir` with the joined path and re-lists;
5. a refusal (`ToolError(..., fromDevice)`) is shown as the device's sentence, not an empty panel;
6. a binary file's utf8 read refusal offers the download path (`files.read` with `encoding: "base64"`).

Do not assert class names; use roles, labels and the sentences. Mock `URL.createObjectURL`, `atob`
(native in jsdom) and an anchor click where a download is exercised.

- [ ] **Step 2: Run, fix the tests (never the page), report any production mismatch**

`npx vitest run src/FilesPage.test.tsx`, then the whole suite and typecheck. The bundle does not change.
Commit: `test: the files page lists, opens, saves and creates through the device`.

## Self-review

- Spec coverage: the events page (the sweep's "expose `state.events` and render it", realised as an
  additive `state.get` field instead of a new tool — recorded in the commit), font size (the sweep's
  "font size is still absent"), Files tests (the largest uncovered page).
- Review Focus: poisoned state (Task 1 test 2), event shapes (Task 1 tests 1 and 2), newest-first
  (Task 1 test 1), font reach/reload/clamp (Task 2 tests), Files behaviour (Task 3).
- Proportion: interfaces and exact sentences, not component bodies.
