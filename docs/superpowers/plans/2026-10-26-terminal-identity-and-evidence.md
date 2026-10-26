# The terminal's identity and evidence, and the shell's first tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** make a terminal tab identifiable and its output exportable, recover a tab whose read cursor
fell out of the device's 1 MiB ring, let an operator write a new memory entry, and give `App` the tests
the shell has never had.

**Architecture:** the logic moves into two pure modules (`terminalTabs.ts`, `terminalTranscript.ts`)
that are unit-tested without xterm; `TerminalWorkspace` wires them. `MemoryPage` gains one create
dialog. `App.test.tsx` drives the shell against a mocked `api`, so the page's boot, sign-in, rail and
device-selection paths are pinned.

**Tech Stack:** React 19 + TypeScript, vitest/jsdom/RTL, the existing `callTool` grant path.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` § "Worth doing later" (memory
create, terminal transcript export, tab identity); the device's terminal capability is
`device/device/src/capabilities/terminal.rs` (ring, cursors, `IDLE_TIMEOUT_MS`), and the legacy
panel's export is `summrise-legacy: agent/resources/panel-react/src/hooks/useSessions.ts:603`.

## Global Constraints

- Console web only: `device/console/web/src/*`, the rebuilt `dist/index.html`, and tests. **No Rust.**
- No new dependency: xterm and its fit addon are already there; search would need an addon and is not
  in this plan.
- Every device call is `callTool(grant, …)`. No console route; no stored credential.
- **The device retains the newest 1 MiB of a session's output** (`terminal::BUFFER_BYTES`). The export
  must never claim to be the whole session when the ring has wrapped, and it must say how many bytes
  are gone. The export's cursor is its own; it must not move the live tab's cursor.
- A rename is console-local: it changes a label, never a device session.
- Generated labels are unique among the current titles. A generated label whose tab was renamed to
  something else is free again, because no tab carries it any more; a name the operator typed is never
  rewritten.
- The device closes a session after 30 minutes with no reads, writes or resizes
  (`IDLE_TIMEOUT_MS`); a sentence about that is a fact, not a guess about this session.
- English UI and comments. `npm run typecheck` 0 errors; `npm test` green; `./scripts/build-web.sh`
  rebuilds the committed bundle.

## Review Focus

1. **A renamed tab survives the poll and a page change.** The tabs live in `TerminalWorkspace` (kept
   mounted by `App`), so a poll that re-renders must not regenerate the title, and a fault must not
   reset it.
2. **Duplicate labels.** Close a middle tab, open another, attach to a device session: no two tabs may
   carry the same generated label, and a name the operator typed must not be silently rewritten.
3. **The export does not disturb the live read, and a transient failure does not rewind it.** The
   export reads from cursor 0 (or the earliest the device still has) through its own calls; the tab's
   `cursors` map is untouched. A read that failed without a `CursorTooOld` sentence keeps its cursor;
   only the ring's lost bytes move it, to the earliest the device still serves.
4. **The ring's lost bytes are stated.** A wrapped stream exports from `earliest` and the file says
   `dropped: N bytes`, never "the whole session".
5. **The shell's tests do not sleep.** The 5-second poll is cleaned on unmount; tests use the initial
   render and explicit calls, not a wall clock.

---

### Task 1: A tab has a name

**Files:**
- Create: `device/console/web/src/terminalTabs.ts`, `device/console/web/src/terminalTabs.test.ts`
- Modify: `device/console/web/src/TerminalWorkspace.tsx` (the `Tab` type, `openWith`, `attachTo`,
  `submitConnect`, the tab strip, a Rename dialog, one action button)
- Create: `device/console/web/src/TerminalWorkspace.test.tsx` (with the xterm mock described below)

**Interfaces:**
- Produces from `terminalTabs.ts`:
  ```ts
  export type Tab = { key: string; session: string; kind: string; title: string };
  export function nextTabTitle(kind: string, tabs: readonly Tab[]): string;
  export function titleFromInput(value: string): string | null;
  export function renameTab(tabs: readonly Tab[], key: string, title: string): Tab[];
  ```
  `nextTabTitle` is `${kind} ${n}` for the smallest `n >= 1` whose title is not already in `tabs`.
  `titleFromInput` trims, collapses internal whitespace runs to one space, rejects empty, and caps at
  60 characters. `renameTab` returns a new array with only the matching `key` replaced (other objects
  keep identity), and returns the same array when the key is absent.

- [ ] **Step 1: Write the pure tests first**

`terminalTabs.test.ts`: empty tabs → `pty 1`; tabs `pty 1`, `pty 2` → `pty 3`; close `pty 2` and ask
again → `pty 2` (the gap fills); a tab renamed `pty 1` keeps `nextTabTitle` from offering `pty 1`
again; different kinds number independently. `titleFromInput("  prod   db  ")` → `"prod db"`;
`("   ")` → null; a 70-character input is capped at 60. `renameTab` changes one title, leaves the rest
identical by `toBe`, and returns the same array for an unknown key.

- [ ] **Step 2: Run and watch fail**

`npx vitest run src/terminalTabs.test.ts` — module missing.

- [ ] **Step 3: Implement the module and wire it**

Move `Tab` into `terminalTabs.ts` and import it. `openWith` no longer takes a generated title: it
takes `title: string | null` and, inside the `setTabs` updater, uses `title ?? nextTabTitle(kind,
previous)`. `submitConnect` passes `connect.name` or null; `openPreset` passes `saved.name` for pty and
serial; `attachTo` uses `nextTabTitle(kind, previous)`. The tab strip keeps its fault chip and close
control. Add a **Rename tab** button beside the other actions (disabled when no active tab) that opens
a `Modal` with an input prefilled with the active title; Enter or **Save** calls `renameTab` and sets
`active`. The status line says `Renamed to "<title>".`

- [ ] **Step 4: Write the component test**

`TerminalWorkspace.test.tsx` mocks `@xterm/xterm` (`Terminal` with `open`, `loadAddon`, `onData`,
`write`, `reset`, `dispose`, `onResize`, `cols`, `rows`, `options`, `buffer`), `@xterm/addon-fit`
(`FitAddon` with `fit`), `./terminal` (`callTool`) and `./api` (`connections` → `[]`). `terminal.create`
answers `{id: "t1"}`; `terminal.read` answers `{data: "", cursor: 0}`; `terminal.list` answers `[]`.

1. Render with a grant; click **+ New**; submit the pty dialog; a tab labelled `pty 1` appears.
2. Click **Rename tab**, clear the input, type `prod db`, save; the tab reads `prod db`.
3. Open a second tab; it is labelled `pty 2`, not `pty 1`.

Also test `titleFromInput` refusal in the component: an empty name leaves the old title and the dialog
open with a sentence.

- [ ] **Step 5: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: a terminal tab has a name of its own`.

### Task 2: The transcript, and the cursor that fell out of the ring

**Files:**
- Create: `device/console/web/src/terminalTranscript.ts`,
  `device/console/web/src/terminalTranscript.test.ts`
- Modify: `device/console/web/src/TerminalWorkspace.tsx` (an **Export log** button, `diagnose`'s
  cursor recovery, the closed-without-reason sentence)
- Modify: `device/console/web/src/TerminalWorkspace.test.tsx` (an export test on the same harness)

**Interfaces:**
- Produces from `terminalTranscript.ts`:
  ```ts
  export const RETAINED_BYTES = 1024 * 1024;
  export function earliestCursor(sentence: string): number | null;
  export type TranscriptRead = (cursor: number) => Promise<{ data: string; cursor: number }>;
  export type Transcript = { text: string; from: number; to: number; dropped: number };
  export function readTranscript(read: TranscriptRead): Promise<Transcript>;
  export function transcriptFile(transcript: Transcript, meta: { session: string; at: Date }): string;
  ```
  `earliestCursor` reads the device's own sentence `the cursor N is older than the retained output,
  which starts at M` and returns M; any other sentence returns null.
  `readTranscript` reads at 0. If that fails and `earliestCursor` recognises the sentence, it reads at
  M and reports `from: M, dropped: M`; if the retry fails, that failure propagates. If the first
  failure is unrecognised, it propagates. Success at 0 is `from: 0, dropped: 0`.
  `transcriptFile` is:
  ```
  Summrise terminal transcript
  session: <session>
  exported: <at.toISOString()>
  range: bytes <from>..<to> of this session's output
  retained: the device keeps the newest 1 MiB of a session's output
  dropped: <dropped> bytes — older output is no longer retained and cannot be recovered
  ---
  <text>
  ```
  When `dropped` is 0 the line reads `dropped: none — the whole session is retained`.

- [ ] **Step 1: Write the pure tests first**

`terminalTranscript.test.ts`: a read at 0 succeeding gives `from 0`, `to` the returned cursor, no
dropped bytes; a first read refusing with the device's sentence and a second at 4096 succeeding gives
`from 4096`, `dropped 4096`; a first read refusing with an unrelated sentence rejects, and the read
function is called once; a recognised failure whose retry also fails propagates the retry's error.
`transcriptFile` contains each header line, the ISO time, the session, and the payload after `---`.

- [ ] **Step 2: Run and watch fail**

`npx vitest run src/terminalTranscript.test.ts`.

- [ ] **Step 3: Implement the module, wire the export and fix the recovery**

The **Export log** button appears beside **Rename tab** and is disabled without an active tab. It runs
`readTranscript((cursor) => JSON.parse(await callTool(grantRef.current, "terminal.read", { id:
tab.session, cursor })))`, then downloads `transcriptFile(...)` as `<safe session>.log` through a
`Blob` and an anchor (the memory export's pattern), and sets the status: with dropped bytes,
`Exported N bytes; M bytes are no longer retained by the device.`; otherwise `Exported N bytes.` The
export's calls do not touch `cursors.current`.

In `diagnose`, when a `read` failed and the device still lists the session, recover ONLY a cursor the
device refused as too old: if `earliestCursor(messageOf(cause))` finds one, set the cursor there and
continue from the earliest retained byte; if it does not, leave the cursor where it was. The old code
reset to 0 on every read failure, which replays the retained output after a transient network error
and duplicates it in the terminal — a failure that never moved the cursor must not move it.

When a session disappears without the device giving a reason, the fault sentence gains: `The device
closes a session after 30 minutes with no reads, writes or resizes.`

- [ ] **Step 4: Extend the component test**

With the Task 1 harness, open one tab; make `terminal.read` answer `{data: "hello\n", cursor: 6}`;
click **Export log**; stub `URL.createObjectURL` and the anchor click; assert the blob text carries the
header, `session: t1`, and `hello`, and that the tab's own poll cursor did not change (the next poll
still asks for the cursor it had before the export). A second test drives a `CursorTooOld` refusal on
the tab's poll and asserts the next `terminal.read` asks for the earliest cursor from the sentence.

- [ ] **Step 5: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: a terminal transcript says what the device still retains`.

### Task 3: Memory create

**Files:**
- Modify: `device/console/web/src/MemoryPage.tsx` (a **New entry** button and one create modal)
- Modify: `device/console/web/src/MemoryPage.test.tsx`

**Interfaces:**
- Consumes: `memory.append {text, tags?}` → `{appended: true, id}`. The device enforces the tag and
  size caps (32 tags × 128 bytes, 1 MiB serialized line). **It does NOT refuse an empty text** —
  verified in `device/device/src/capabilities/memory.rs::validate`, which checks only the caps, and the
  schema declares no `minLength`. The page refuses an empty text; a device-side refusal is recorded
  below as a follow-up, not silently assumed.
- Produces: no new export.

- [ ] **Step 1: Write the failing tests**

Mock `memory.list` → `[]` and `memory.append` → `{"appended":true,"id":"m1"}`, with `memory.list`
answering one entry on the second call. Click **New entry**, type `rotate the cert on the 1st`, add tag
`ops`, submit; assert `memory.append` was called with `{text: "rotate the cert on the 1st", tags:
["ops"]}`, the dialog closes, and the list is re-read. An empty text keeps **Remember** disabled (the
page's guard; the device would accept it today). A device refusal (`ToolError(..., fromDevice)`) keeps
the dialog open and shows the sentence.

- [ ] **Step 2: Run and watch fail**

`npx vitest run src/MemoryPage.test.tsx` — no New entry button.

- [ ] **Step 3: Implement**

A primary **New entry** button in the panel head opens a `Modal` titled **Remember something** with a
textarea (text), the tag chips UI the edit form already uses, and **Remember** / **Cancel**. Submit
calls `memory.append`; on success close, clear, re-read the current view, and set the status
`Remembered <id>.`; on failure keep the dialog and show `Could not remember: <sentence>`. Do not reuse
the edit state.

- [ ] **Step 4: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: the memory page can add an entry`.

### Task 4: The shell's first tests

**Files:**
- Create: `device/console/web/src/App.test.tsx` (test-only; no production change)

**Interfaces:**
- Mocks `./api`: `api.session`, `api.health`, `api.devices`, `api.audit`, `api.connections`,
  `api.signIn`, `api.enroll`, `api.rename`, `api.remove`, `api.grant`, plus `setToken`/`getToken`,
  `HttpError`, `NoAnswer`. No real fetch, no timers longer than a microtask.

- [ ] **Step 1: Write the tests**

1. **Boot signed out:** `api.session` answers `{authenticated:false}`; the page shows the login form
   with the token field. Typing a token and submitting calls `api.signIn`; after it, `api.session`
   answers authenticated and `api.devices` answers two devices; the fleet renders both names and the
   status bar counts them.
2. **The shell is whole:** the rail has the ten sections in order (`Devices` … `Audit`), the brand and
   the console version from `/health` are on screen, and the status bar names the sign-in path.
3. **Rail navigation:** clicking **Audit** renders the audit panel and its empty sentence from
   `api.audit` → `[]`; clicking **Memory** renders the device picker, not a blank page.
4. **Device selection:** clicking a device card's **Use** (or selecting it) lands on Terminal with the
   grant form for that device's name.
5. **Sign out:** returns to the login form and clears the fleet.

Unmount each render so the 5-second interval is cleared, and do not use fake timers to advance it.

- [ ] **Step 2: Run and watch fail**

`npx vitest run src/App.test.tsx` — the file is the test, so it should pass once the mocks match the
real `api` signatures; a failure here is a real mismatch, fix the test (not `App`) and report any
production defect it exposes.

- [ ] **Step 3: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh` (the bundle does not change; commit it only
if the build says so).
Commit: `test: the console shell boots, navigates and signs out`.

## Recorded, not done

- **The device accepts an empty `memory.append`.** The requirement's schema says "The fact to
  remember" and declares no `minLength`; `validate` checks the caps only. The page refuses it, but an
  MCP client can still write an empty fact that occupies a slot and shows up in the store. The device
  should refuse it with a typed error and a `minLength: 1`, in a Rust change with its own tests.
- **Terminal scrollback search** needs an xterm search addon; it is a dependency decision, not a
  wiring one, so it stays out.
- **The monitor chip** (a down target visible from every page) is a shared poller and a notification
  permission; it is its own design.

## Self-review

- Spec coverage: memory create (Task 3), transcript export (Task 2), tab identity (Task 1), the shell's
  tests (Task 4); search and the monitor chip stay out, so the "Worth doing later" list shrinks by
  three of its five real items.
- Review Focus: rename survivability (Task 1 component test and the tab state), duplicate labels (Task
  1 pure test), export vs live cursor (Task 2 component test), lost bytes (Task 2 pure test), no sleeps
  (Task 4).
- Proportion: the plan names interfaces and exact sentences; it does not write the component bodies.
- The ring is 1 MiB and `read` returns the ring from a cursor; that is why export is one call with a
  recovery, not the legacy's many pages.
