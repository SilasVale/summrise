# The monitor chip and the last two untested pages

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** make a down monitor visible from every page, and put the console's last two untested pages
(Runs and System) under test.

**Architecture:** `MonitorChip` owns one `monitor.list` read every 20 seconds for the current grant
and renders only when the device says something is down; it is a read, not a new route. The Runs and
System tests are test-only and mock `callTool`.

**Tech Stack:** React 19 + TypeScript, vitest/jsdom/RTL.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` § "Worth doing later"
("Monitor alerts and notifications — a global chip … is the legacy behaviour"), and the round-3
review's untested-page note; Runs and System are the last two pages without tests.

## Global Constraints

- **The chip is console-only and has no new route.** `monitor.list {}` through `callTool` with the
  grant the console already holds; no browser notification this round (deferred and recorded), no
  permission prompt, no new dependency.
- **Unknown is not zero.** A refused or failed list, a target with no `summary`, and "no targets" all
  render nothing — the chip never claims "0 down" and never becomes an error banner in the top bar.
  Only `summary.up_now === false` counts.
- **One call at a time.** A busy guard skips a tick already in flight; the 20-second interval is
  cleared on unmount and restarted when the grant changes; no call without a grant.
- **Clicking the chip keeps the grant and the device.** It only changes the page to Monitors.
- Runs and System tests are test-only: a production mismatch is reported, not fixed.
- English UI/comments. `npm run typecheck` 0; `npm test` green (131 at the base); `./scripts/build-web.sh`
  and the committed bundle agree. Every build is checked by its exit code, never by a piped tail.

## Review Focus

1. **No false all-clear.** A refusal, a transport failure, a null summary or an empty list hides the
   chip; only a target the device marked down produces it.
2. **No pile-up.** A slow list does not overlap the next tick; the timer is cleared on unmount and on
   a grant change; a stale answer from an old grant never paints the new one.
3. **The click is only navigation.** The grant and the selected device are untouched; the Monitors
   page opens with the same envelope.
4. **Runs pins the request, not the form.** An argv run sends `{command, args}`, a shell run sends
   `{command, shell: true}`, `cwd` only when non-empty, `timeout_ms` as the number; the history read
   asks for the page's limit; a row's record is `runs.get {id}`.
5. **System pins the destructive identity.** The kill request is the pid the device reported (never a
   name), only after the confirmation, and the list re-reads after the attempt; the net test's args
   are exactly host/port and the optional timeout.

---

### Task 1: The monitor chip

**Files:**
- Create: `device/console/web/src/MonitorChip.tsx`, `device/console/web/src/MonitorChip.test.tsx`
- Modify: `device/console/web/src/App.tsx` (the top bar, next to the session line)
- Modify: `device/console/web/src/App.test.tsx` (a marker mock + one test)
- Modify: `device/console/web/src/app.css` only if the chip needs a class

**Interfaces:**
- `MonitorChip({grant, onOpen}: {grant: Grant; onOpen: () => void})`.
- The device's `monitor.list` answer is the targets array the Monitors page already reads; a target's
  `summary.up_now` is the fact the chip counts.
- Produces the sentence `${down} monitor${down === 1 ? "" : "s"} down` as the button's text and
  accessible name; the title is `${down} of ${total} monitors down — open Monitors`.

- [ ] **Step 1: Component tests first**

Mock `./terminal`'s `callTool` (real `ToolError` kept). Five tests:

1. two targets, one `summary: {up_now: false}` → the button says `1 monitor down`, its title names
   1 of 2; a click calls `onOpen`;
2. all up (or no summaries) → `queryByRole("button")` is null;
3. a refusal (`new ToolError("the grant expired", 403, -32001)`) → null, and no console error;
4. an empty target list → null, and the chip did call `monitor.list {}` once;
5. the poll: `vi.useFakeTimers`, advance 20 s → a second `monitor.list` call; unmount → no third call
   after another 20 s (and `clearInterval` got the timer). Use `act` around the timer advance.

- [ ] **Step 2: Implement**

`useEffect` on `grant`: a `read()` that begins with a busy ref, calls `callTool(grant, "monitor.list",
{})`, parses an array, counts `summary?.up_now === false`, and sets `{down, total}` only on success
(any failure sets nothing). Start one read immediately and `setInterval(read, 20_000)`; the cleanup
clears the timer and marks the effect dead so a late answer cannot set state. Render the button only
when `down > 0`; a `chip chip-down` class (add the rule beside the existing chips if it is new).

- [ ] **Step 3: Wire the top bar**

In `App.tsx`, inside `.session` before the online dot: `{grant && <MonitorChip grant={grant}
onOpen={() => setPage("monitors")} />}`. In `App.test.tsx`, mock `./MonitorChip` as
`({onOpen}) => <button type="button" onClick={onOpen}>monitor-chip</button>`; extend the existing
grant test (the terminal workspace one) to assert the marker appears after **Grant access** and that
clicking it shows the **Monitors** heading. The chip's behaviour is its own test's job.

- [ ] **Step 4: Verify and commit**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh` (exit code checked), bundle committed.
Commit: `console: a down monitor is visible from every page`.

### Task 2: The Runs page's tests

**Files:**
- Create: `device/console/web/src/RunsPage.test.tsx` (test-only)

**Interfaces:**
- Mock `./terminal`'s `callTool`; the page renders with a `Grant`. No production change.

- [ ] **Step 1: Read the page, then write the tests**

`RunsPage` calls `runs.list {limit: 20}` on mount and on **Refresh history**, `runs.exec` on submit,
`runs.get {id}` when a history row is opened. Cover:

1. the history lists (command, exit, bytes) and the note says how many rows; **Refresh history**
   sends `runs.list {limit: 20}` again;
2. an argv run: type `echo hello`, submit → `runs.exec` with exactly `{command: "echo", args:
   ["hello"], timeout_ms: 60000}`, the result is shown, and the history is read again;
3. a shell run: tick **Through the shell**, type `echo a && echo b`, submit → `{command: "echo a &&
   echo b", shell: true, timeout_ms: 60000}`;
4. a working directory typed → `cwd` is sent trimmed; empty → no `cwd` key;
5. a row's record: click a history row → `runs.get {id}`, its detail shown, Close returns;
6. a device refusal (`ToolError(..., fromDevice)`) is the page's sentence, and a previous result is
   not blanked by a later failure.

No class-name assertions; roles, labels, placeholders and sentences only.

- [ ] **Step 2: Run, fix the tests (never the page), report any production mismatch**

`npx vitest run src/RunsPage.test.tsx`, then the whole suite and typecheck. Bundle unchanged.
Commit: `test: the runs page executes and records through the device`.

### Task 3: The System page's tests

**Files:**
- Create: `device/console/web/src/SystemPage.test.tsx` (test-only)

**Interfaces:**
- Mock `./terminal`'s `callTool`; the page renders with a `Grant`. No production change.

- [ ] **Step 1: Read the page, then write the tests**

`SystemPage` calls `system.process_list {name?}` on mount (no `name` for an empty filter) and on the
filter form's submit, `system.process_kill {pid}` after the confirmation, `system.net_test {host,
port, timeout_ms?}`. Cover:

1. the process list renders and the note counts it; filtering sends `{name: "nginx"}` (trimmed) and
   the note says the filter is the device's; an empty box sends `{}`;
2. **Kill** opens the confirmation naming the pid and process; **Cancel** sends nothing; confirming
   sends `system.process_kill {pid}` with the pid (not the name), shows the report, and re-reads the
   list;
3. a kill refusal is the device's sentence next to the process, the modal stays open, and the list is
   still re-read;
4. a net test sends `{host, port}` (and the timeout only when typed), shows the report; a refusal is
   `Could not test host:port: …` and the previous result stays.

No class-name assertions; roles, labels, placeholders and sentences only.

- [ ] **Step 2: Run, fix the tests (never the page), report any production mismatch**

`npx vitest run src/SystemPage.test.tsx`, then the whole suite and typecheck. Bundle unchanged.
Commit: `test: the system page lists, kills and tests through the device`.

## Self-review

- Spec coverage: the chip (the sweep's "global chip"), the last two untested pages (Runs, System).
- Review Focus: unknown-is-not-zero and the click (Task 1 tests), the exact requests (Tasks 2–3).
- Proportion: interfaces and sentences, not component bodies.
- Deferred: browser notifications (the permission prompt is its own decision), and a `MonitorChip`
  visual/hover treatment beyond the existing chip styles.
