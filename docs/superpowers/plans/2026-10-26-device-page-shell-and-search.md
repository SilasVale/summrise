# The console's page shell, a searchable terminal, and the monitors page's tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** remove the console's three copied picker→grant→page blocks into one shell, add scrollback
search to a terminal session, and put the Monitors page's state machine under test.

**Architecture:** `DevicePage` owns the three states every device page repeats (pick, grant, render)
and hands its child the matching grant, so no page can render with another device's envelope. Search
is `@xterm/addon-search` on each tab's xterm — local scrollback only, nothing on the wire. The Monitors
tests are test-only and mock `callTool`.

**Tech Stack:** React 19 + TypeScript, `@xterm/xterm` 6 and `@xterm/addon-search` 0.16 (new dependency,
build-time), vitest/jsdom/RTL.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` § "Worth doing later" (terminal
scrollback search; the Monitors test gap is the round-3 review's); the standards review of round 3
named the picker→grant→page duplication.

## Global Constraints

- **Task 1 is a behaviour-preserving refactor.** Every heading, hint, button label and `GrantForm`
  result the operator sees stays the same; the App shell tests keep passing without edits. The child
  receives a `Grant`, not `Grant | null`.
- **The search dependency is a deliberate decision.** `@xterm/addon-search` is the xterm project's own
  addon for xterm 6, bundled at build time by vite, with no runtime fetch, no console route and no
  device call. Writing a scrollback matcher by hand would be worse than taking the canonical one.
- **Search reads only what the page already has.** It never asks the device for anything; the ring's
  retained output is what the xterm holds. Closing clears the decorations it added.
- Console web only: `device/console/web/src/*`, `package.json`/`package-lock.json` for the one addon,
  the committed `dist/index.html`, tests. No Rust.
- The Monitors tests are test-only; a production mismatch is reported, not fixed.
- English UI/comments. `npm run typecheck` 0; `npm test` green (110 at the base); `./scripts/build-web.sh`
  and the committed bundle agree.

## Review Focus

1. **One shell, same words.** The DevicePage refactor must not silently rewrite a hint or a heading;
   the App tests' sentences are the contract.
2. **The child cannot get the wrong grant.** `DevicePage` renders children only for a grant whose
   `deviceId` is the chosen device, and passes that grant as its argument; a grant for another device
   shows the grant panel instead.
3. **Search is local and leaves no residue.** A search never produces a `callTool`; closing or
   switching tabs clears the decorations of the xterm that had them; the addon instance belongs to the
   tab's terminal and dies with it.
4. **The search bar is usable without guessing.** Enter searches, Shift+Enter goes back, Escape closes,
   a miss says so, and the Find control is disabled with no session open.
5. **The Monitors tests pin the device calls, not the markup.** The exact `monitor.add` arguments for
   an http and a tcp target, the probe's read-back, the remove, and the device's refusal sentence.

---

### Task 1: One DevicePage shell

**Files:**
- Create: `device/console/web/src/DevicePicker.tsx` (moved verbatim from `App.tsx`)
- Create: `device/console/web/src/DevicePage.tsx`, `device/console/web/src/DevicePage.test.tsx`
- Modify: `device/console/web/src/App.tsx` (import both; replace the `screen`, `memory`, `events`
  blocks with one `DevicePage` each)
- Modify: `device/console/web/src/App.test.tsx` only if a sentence actually moved (it should not)

**Interfaces:**
- `DevicePicker({devices, onChoose}: {devices: Device[]; onChoose: (id: string) => void})` — the
  existing picker, unchanged, exported.
- `DevicePage({title, pickHint, grantHint, devices, chosen, grant, onChoose, onGrant, children})`:
  ```ts
  type DevicePageProps = {
    title: string;
    pickHint: string;
    grantHint: string;
    devices: Device[];
    chosen: Device | null;
    grant: Grant | null;
    onChoose: (id: string) => void;
    onGrant: (deviceId: string, envelope: Grant["envelope"]) => void;
    children: (grant: Grant) => ReactNode;
  };
  ```
  - `!chosen` → the title panel, `pickHint`, `DevicePicker`.
  - `chosen` with no matching grant → the grant heading (`Grant what the console may do on
    {name ?? id.slice(0,12)}`), `grantHint`, `GrantForm` whose `onGranted` calls
    `onGrant(chosen.id, envelope)`.
  - matching grant → `<section className="panel">{children(grant)}</section>`.

- [ ] **Step 1: Extract the picker**

Copy `DevicePicker` from `App.tsx:775-793` into `DevicePicker.tsx` with the `Device` import; import it
in `App.tsx` and delete the local copy. Run the App tests: unchanged.

- [ ] **Step 2: DevicePage, test first**

`DevicePage.test.tsx` (mock `./GrantForm` only to keep the test about the shell; render the real
picker): no chosen → the title, the pick hint, one **Use** per device, no `Grant access`; chosen with
`grant: null` → the heading with the device name (and `id.slice(0,12)` when the name is null), the
grant hint, `Grant access`; chosen with a matching grant → the child receives that grant and no grant
heading; chosen with a grant for another device → the grant heading, not the child.

- [ ] **Step 3: Adopt the shell in the three pages**

Replace the three repeated trios in `App.tsx` with:

```tsx
<DevicePage
  title="Screen"
  pickHint="…the existing sentence, verbatim…"
  grantHint="…"
  devices={devices}
  chosen={chosen}
  grant={grant}
  onChoose={(id) => setSelected(id)}
  onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
>
  {(grant) => <ScreenPage grant={grant} />}
</DevicePage>
```

The same for Memory (`MemoryPage`) and Events (`EventsPage`). The hints are the current sentences,
copied, not reworded.

- [ ] **Step 4: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: one device page shell instead of three copies`.

### Task 2: Scrollback search

**Files:**
- Modify: `device/console/web/package.json`, `package-lock.json` (`@xterm/addon-search` ^0.16.0)
- Modify: `device/console/web/src/TerminalWorkspace.tsx` (instance search, bar, actions)
- Modify: `device/console/web/src/TerminalWorkspace.test.tsx` (mock the addon)
- Modify: `device/console/web/src/app.css` if the bar needs a class

**Interfaces:**
- The `Instance` type gains `search: SearchAddon`; the creation effect loads it on the tab's xterm.
- The bar: a **Find** action (disabled with no active tab) toggles it; the bar holds an input
  (placeholder **Find in this session**), **Next**, **Previous**, **Close**. Enter = Next,
  Shift+Enter = Previous, Escape = Close. A miss renders **No match.**; a hit clears it. Closing calls
  `clearDecorations()` on the active instance; a tab switch clears the decorations on the tab being
  left (the bar may stay open).

- [ ] **Step 1: Add the dependency and the mock**

`npm install @xterm/addon-search@^0.16.0` updates both files. In the test, mock
`@xterm/addon-search` with a class that records instances and returns a controllable boolean from
`findNext`/`findPrevious`; expose `vi.hoisted` arrays so the test can inspect the active addon.

- [ ] **Step 2: Component tests first**

Open a pty tab; click **Find**; type `error`; Enter → the addon's `findNext` was called with `error`
and no `caseSensitive`; Shift+Enter → `findPrevious`; make `findNext` return false → **No match.**
appears, then true → it clears; click **Close** → `clearDecorations` on that instance; with two tabs,
switching clears the previous tab's decorations and Next searches the new active tab. Also: **Find**
is disabled before a tab exists.

- [ ] **Step 3: Implement**

Add `search` to the instance record and `term.loadAddon(search)`; the bar state; the handlers; the
status sentence; the tab-switch clearing; the input autofocus. No `callTool` in any of it.

- [ ] **Step 4: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh`, bundle committed.
Commit: `console: a terminal session can be searched`.

### Task 3: The Monitors page's tests

**Files:**
- Create: `device/console/web/src/MonitorsPage.test.tsx` (test-only)

**Interfaces:**
- Mock `./terminal`'s `callTool`; the page renders with a `Grant`. No production change.

- [ ] **Step 1: Read the page, then write the tests**

`MonitorsPage` calls `monitor.list {}`, `monitor.status {id}`, `monitor.add {kind, host, port,
interval_ms, path?, expect?}` (the device requires `interval_ms`), `monitor.probe {id}` (then reads
`monitor.status`), `monitor.remove {id}`. Cover:

1. a listed http target renders host:port and its status; a down target says so as a fact;
2. adding a tcp target sends exactly `{kind: "tcp", host, port}` and re-lists;
3. adding an http target with a path and an expect sends both;
4. **Probe** sends `monitor.probe` with the id and then a `monitor.status` read;
5. **Remove** (through the page's confirmation) sends `monitor.remove` with the id;
6. a device refusal (`ToolError(..., fromDevice)`) is the sentence shown, and the form keeps its draft.

No class-name assertions; roles, labels, placeholders and sentences only. Do not wait on a wall-clock
poll; drive the actions the test needs and assert their calls.

- [ ] **Step 2: Run, fix the tests (never the page), report any production mismatch**

`npx vitest run src/MonitorsPage.test.tsx`, then the whole suite and typecheck. Bundle unchanged.
Commit: `test: the monitors page adds, probes and removes through the device`.

## Self-review

- Spec coverage: the shell (round-3 standards review's deferred item), search (the sweep's "terminal
  scrollback search"), Monitors tests (the round-3 review's test-gap note; the sweep's untested-page
  family).
- Review Focus: the shell's words and grant (Task 1 tests), search locality/residue (Task 2 tests),
  Monitors calls (Task 3).
- Deliberate dependency: `@xterm/addon-search` 0.16, recorded in the constraint and the commit.
