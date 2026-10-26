# The console's parity close-out, and the gate that keeps it honest

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** close the five console gaps the 2026-10-25 parity sweep left open, and put the page under a
CI gate — typecheck, tests and the committed bundle — so none of them can silently reopen.

**Architecture:** Every gap is console-only: the device tools already exist and the page calls them
through `callTool(grant, …)`. The one infrastructure change is a `console` job in `ci.yml` plus
vitest/jsdom in the page's dev dependencies, because the page currently has no test runner and no CI
step at all: a broken page or a stale bundle passes today.

**Tech Stack:** React 19 + TypeScript, vite (single-file build), vitest + @testing-library/react +
jsdom, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` (§ "Worth doing now", items 4-9;
item 9's decision is recorded in Task 6).

## Global Constraints

- Console web only: `device/console/web/src/*`, `device/console/web/*.json|ts`, the rebuilt
  `device/console/web/dist/index.html`, `scripts/*` only if a gate command needs one, and
  `.github/workflows/ci.yml`. **No Rust changes.**
- No runtime dependency is added. Test-only dev dependencies (`vitest`, `jsdom`,
  `@testing-library/*`) are the one permitted addition, and they must not ship in the bundle.
- Every device call goes through `callTool(grant, …)`. No console route is added.
- Credentials are per open and never stored. No new localStorage/sessionStorage key carries one.
- Reads that can race use `useRequestGate`; the newest answer wins and a stale answer drops both its
  data and its error.
- English UI and comments. The page speaks to the operator in sentences, not codes.
- A refusal from the device is shown as the device's sentence; a transport failure says it never
  reached the device. Neither is an empty panel.
- Gates: `cd device/console/web && npm run typecheck && npm test`; `./scripts/build-web.sh`; the
  committed `dist/index.html` must equal the build (`git diff --exit-code`).
- CI is the same commands, read from `ci.yml` — do not invent a local variant.

## Review Focus

1. **A page that is hidden is not unmounted.** `TerminalWorkspace` is rendered on every page and
   hidden; the new pages must not steal its state, its timers or its tab layout.
2. **The device's sentence versus the transport's.** `ToolError.fromDevice` is the difference; a page
   that prints `HttpError` codes instead of the refusal sentence is a regression.
3. **A 2 MiB export in the DOM.** The export text is rendered and copied whole; a page that silently
   copies a clipped string is worse than one that refuses.
4. **The screen image is the user's session.** No helper, a locked session and a disconnected session
   are three refusals with three sentences; none may render a black frame or leak the token.
5. **Filter composition.** Device + tool + status must narrow each other, and "nothing matches" must
   read differently from "the audit is empty".

---

### Task 0: The gate, the test harness, and the crash boundary

**Files:**
- Modify: `.github/workflows/ci.yml` (new `console` job)
- Modify: `device/console/web/package.json`, `device/console/web/package-lock.json`
- Create: `device/console/web/vitest.config.ts`, `device/console/web/src/test/setup.ts`
- Create: `device/console/web/src/ErrorBoundary.tsx`, `device/console/web/src/ErrorBoundary.test.tsx`
- Modify: `device/console/web/src/main.tsx`
- Modify: `README.md` ("Build and test" gains the page's commands)

**Interfaces:**
- Produces: `class ErrorBoundary extends Component<{children: ReactNode}, {error: Error | null}>`,
  default export none; the page's root wraps `<App />` in it.
- Produces: `npm test` in `device/console/web` runs vitest once against `src/**/*.test.tsx`.

- [ ] **Step 1: Install the test-only dependencies and write the config**

`npm i -D vitest jsdom @testing-library/react @testing-library/dom @testing-library/jest-dom`.
Add scripts `"test": "vitest run"` and `"typecheck": "tsc --noEmit"`. `vitest.config.ts` uses
`environment: "jsdom"`, `setupFiles: ["./src/test/setup.ts"]`, and the React plugin; the setup file
imports `@testing-library/jest-dom/vitest`. Do **not** reuse `vite.config.ts` — its single-file
plugin is for the shipped build only.

- [ ] **Step 2: Write the failing boundary test**

`ErrorBoundary.test.tsx`: a `Boom` component throws `new Error("the audit table exploded")`.
Assert the fallback has `role="alert"`, the text **Something broke**, and that it names the thrown
message; a second test renders a healthy child and asserts it passes through. Silence React's
expected `console.error` with a spy in the throwing test.

- [ ] **Step 3: Run it and watch it fail**

Run: `npm test -- ErrorBoundary`
Expected: FAIL — `Cannot find module './ErrorBoundary'`.

- [ ] **Step 4: Implement the boundary and mount it**

A class component (React has no hook for this), `static getDerivedStateFromError`, a
`componentDidCatch` that logs, and a fallback panel: heading, one sentence saying the console can be
reloaded, the message in `.mono`, a primary **Reload** button calling `window.location.reload()`.
Wrap `<App />` in `main.tsx` inside `StrictMode`.

- [ ] **Step 5: Verify green**

Run: `npm test -- ErrorBoundary`, then `npm run typecheck`, then `npm test`.
Expected: all pass, `tsc` silent.

- [ ] **Step 6: Add the CI job and prove it locally**

The job is named `console`, runs on `ubuntu-latest` with a 20-minute timeout, sets up Node 22 with
the npm cache keyed on `device/console/web/package-lock.json`, then runs `npm ci`,
`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, and
`git diff --exit-code -- device/console/web/dist/index.html` from the repository root. Run exactly
those commands locally in that order; the last one must be silent after the bundle is committed.

- [ ] **Step 7: Commit**

`git add` the files above, including the rebuilt bundle, and commit as
`console: a test runner, an error boundary, and the CI job that runs both`.

### Task 1: The browser can navigate, close a tab, and stop

**Files:**
- Modify: `device/console/web/src/BrowserPane.tsx`
- Create: `device/console/web/src/BrowserPane.test.tsx`
- Modify: `device/console/web/src/app.css` (only if a class is genuinely new)

**Interfaces:**
- Consumes: `browser.navigate {target, url}`, `browser.close {target}`,
  `browser.shutdown {}`, `browser.list {}` — all already in the registry.
- Produces: no new export; the pane's behaviour is its interface.

- [ ] **Step 1: Write the failing tests**

With `callTool` mocked: `browser.list` answers one target `{id: "A", title: "A", url: "https://a.test"}`,
`browser.screenshot` answers `{data: "AAAA", bytes: 3}`. Test 1: type a URL and click **Navigate**;
assert `browser.navigate` was called with `{target: "A", url: "https://b.test"}`. Test 2: click
**Close tab**; assert `browser.close` with `{target: "A"}` and that `browser.list` ran again. Test 3:
click **Stop browser**; assert `browser.shutdown` and that the pane then says no browser is open.

- [ ] **Step 2: Run and watch fail**

Run: `npm test -- BrowserPane`
Expected: FAIL — no button named Navigate.

- [ ] **Step 3: Implement**

Keep **Open** as "a new tab" and add **Navigate** (selected target; disabled without one), **Close
tab** (selected; disabled without one) and **Stop browser** (always enabled). After navigate/close,
re-list and capture the surviving target; after shutdown, clear the target, the shot and the list and
say the browser is stopped. Fix the placeholder: it now says the truth — opens a new tab, or
navigates the selected one.

- [ ] **Step 4: Verify**

`npm test -- BrowserPane`, `npm run typecheck`, `./scripts/build-web.sh`, commit the bundle.
Commit: `console: the browser navigates, closes a tab, and stops`.

### Task 2: Memory can be exported

**Files:**
- Modify: `device/console/web/src/MemoryPage.tsx`
- Create: `device/console/web/src/MemoryPage.test.tsx`
- Modify: `device/console/web/src/app.css` (only if new)

**Interfaces:**
- Consumes: `memory.export {}` → `{text, entries, truncated, total_bytes}`; the cap is
  `2 * 1024 * 1024` (`memory::MAX_EXPORT_BYTES`). `memory.export` is not destructive.
- Produces: no new export; an **Export** button and a modal.

- [ ] **Step 1: Write the failing tests**

Mock `callTool`: `memory.list` → `[]`; `memory.export` → `{text: "{\"id\":\"1\"}\n", entries: 1,
truncated: true, total_bytes: 9000000}`. Test 1: click **Export**; the dialog contains the JSONL line
and a sentence that says the export was cut at 2 MiB and how many bytes are in the store. Test 2:
click **Copy**; `navigator.clipboard.writeText` received the whole text. Test 3: `memory.export`
rejects with a device refusal; the page shows that sentence and opens no dialog.

- [ ] **Step 2: Run and watch fail**

Run: `npm test -- MemoryPage` — no Export button.

- [ ] **Step 3: Implement**

An **Export** button in the panel head (reads the device, then opens a modal). The modal holds the
JSONL in a read-only `<textarea className="mono">`, a line with entries / bytes / truncation, **Copy**,
**Download** (a `Blob` named `summrise-memory-<YYYY-MM-DD>.jsonl`), and **Close**. The truncation
sentence names the cap and the total, so a cut export is never mistaken for the whole store.

- [ ] **Step 4: Verify and commit**

`npm test -- MemoryPage`, `npm run typecheck`, `./scripts/build-web.sh`.
Commit: `console: memory exports the device's own JSONL`.

### Task 3: Audit answers "what happened to this device"

**Files:**
- Modify: `device/console/web/src/AuditTable.tsx`
- Create: `device/console/web/src/auditFilter.ts`, `device/console/web/src/auditFilter.test.ts`,
  `device/console/web/src/AuditTable.test.tsx`
- Modify: `device/console/web/src/App.tsx` (the audit fetch and the table's props)
- Modify: `device/console/web/src/api.ts` only if a type is missing

**Interfaces:**
- Produces: `type AuditFilter = {device: string; tool: string; status: "all" | "ok" | "failed"}` and
  `filterAudit(lines: AuditLine[], filter: AuditFilter): AuditLine[]` from `auditFilter.ts`.
- Produces: `<AuditTable lines={AuditLine[]} devices={Device[]} />`.
- Consumes: `/audit?limit=` accepts up to 1000; the page asks for 200.

- [ ] **Step 1: Write the failing tests**

Pure: three lines — device A / memory.export / 200, device B / terminal.write / 403, device A /
terminal.list / 200. Assert each filter narrows alone and together (`A` + `terminal` leaves the list
line; `A` + `failed` leaves none). Component: render three lines; choose device A; the caption says
`2 of 3`; open the first line's detail; the detail names the tool and the status; set a tool that
matches nothing; assert **No audit lines match** (different from the empty-table sentence).

- [ ] **Step 2: Run and watch fail**

Run: `npm test -- auditFilter AuditTable`.

- [ ] **Step 3: Implement**

`auditFilter.ts` is pure (case-insensitive tool substring; `ok` is status < 400; `failed` is >= 400;
device is an exact id). `AuditTable` owns the filter controls in a toolbar: a device `<select>` built
from `devices` plus any id the lines carry that the fleet no longer knows, a tool input, a status
select, and a **Clear** button. A caption says `N of M lines`. Each row gains a **Details** button
that expands a second row (ISO timestamp, kind, device id, tool, detail, status). Import `AuditLine`
from `api.ts` and delete the duplicate type. `App` raises its audit fetch from 20 to 200 and passes
`devices`; the diagnostics copy still uses the newest 8 for the selected device.

- [ ] **Step 4: Verify and commit**

`npm test -- auditFilter AuditTable`, `npm run typecheck`, `./scripts/build-web.sh`.
Commit: `console: the audit table filters by device, tool and status, and opens a line`.

### Task 4: The screen page

**Files:**
- Create: `device/console/web/src/ScreenPage.tsx`, `device/console/web/src/ScreenPage.test.tsx`
- Modify: `device/console/web/src/Rail.tsx` (the page union, label, icon, and the rendered list)
- Modify: `device/console/web/src/App.tsx` (a `screen` block with the picker → grant → page shape)
- Modify: `device/console/web/src/app.css` (only if new)

**Interfaces:**
- Consumes: `screen.capture {}` → `{format: "png", data: <base64>, width, height}`; a device with no
  helper answers a refusal sentence (`ToolError.fromDevice`).
- Produces: `ScreenPage({grant}: {grant: Grant})`; `Page` gains `"screen"`, labelled **Screen**.

- [ ] **Step 1: Write the failing tests**

Success: `screen.capture` → `{format: "png", data: "iVBORw0KGgo=", width: 1280, height: 720}`; click
**Capture screen**; an `<img>` whose `src` is `data:image/png;base64,iVBORw0KGgo=` appears, the line
says `1280×720`, and a download anchor names the file. Refusal: reject with
`new ToolError("no per-user helper is running on this device", 200, null, true)`; the page shows that
sentence and no image.

- [ ] **Step 2: Run and watch fail**

Run: `npm test -- ScreenPage`.

- [ ] **Step 3: Implement**

A panel with **Capture screen**, the last capture's moment, the image, and **Download**. The copy
says where the picture comes from (the per-user helper's session) and that a locked or disconnected
session is a refusal, not a black frame. The capture is on demand, not on a timer. `App` renders the
page with the same three states as System: no device → picker; no grant → `GrantForm`; grant →
`ScreenPage`. `Rail` gains the entry between `browser` and `memory`.

- [ ] **Step 4: Verify and commit**

`npm test -- ScreenPage`, `npm run typecheck`, `./scripts/build-web.sh`.
Commit: `console: the device's screen is a page, and a refusal is a sentence`.

### Task 5: The consent checkbox says what it does

**Files:**
- Modify: `device/console/web/src/GrantForm.tsx`
- Create: `device/console/web/src/GrantForm.test.tsx`

**Interfaces:**
- Consumes: the device's `allow_destructive`; the console cannot answer the `Ask` that follows when
  it is false.
- Produces: no new export.

- [ ] **Step 1: Write the failing test**

Render the form with `deviceId="d1"`; untick **Allow destructive tools**; assert a sentence appears
that says the device will refuse a destructive call and the console cannot approve it one call at a
time. Tick it again; the sentence goes.

- [ ] **Step 2: Implement the copy**

Keep the toggle (a read-only grant is a real policy; the device refuses with a typed sentence). Add a
`.hint` under it that says exactly what unticking does. Do not build an approvals queue: that is a
new protocol and the legacy sweep records why it is not ported.

- [ ] **Step 3: Verify and commit**

`npm test -- GrantForm`, `npm run typecheck`, `./scripts/build-web.sh`.
Commit: `console: the destructive-grant checkbox states the policy it applies`.

### Task 6: The sweep records the closures

**Files:**
- Modify: `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` (status line and items 4-9)

- [ ] **Step 1: Rewrite each closed item's heading with what landed and the commit that landed it.**
- [ ] **Step 2: Leave the deferred items (memory create, terminal search/export, tab identity,
  eviction notice, monitor alerts) as open, with one sentence on why.**
- [ ] **Step 3: Commit as `docs: the parity sweep says which gaps are closed, and by what`.**

## Self-review

- Spec coverage: items 4, 5, 6, 7, 8 and 9 of the sweep are Tasks 1, 0, 3, 2, 4 and 5; item 5's
  boundary is in Task 0 because it must exist before any page can be trusted.
- Type consistency: `AuditFilter`, `filterAudit`, `AuditLine`, `ScreenPage`, `Page` are named once.
- Review Focus: each line names the test that pins it — stale/gate (Task 3), refusal-vs-transport
  (Tasks 2 and 4), whole-text copy (Task 2), screen refusal (Task 4), filter composition (Task 3).
- Proportion: the plan states decisions (names, values, copy intent) and leaves bodies to the
  implementer; it is shorter than the spec it implements.
