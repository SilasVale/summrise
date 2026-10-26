# A live browser view, and end facts tested everywhere

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the operator can watch the device's browser change without pressing a button every time, and
the terminal's end-fact parser (exit code vs signal) is covered on the platforms CI runs, not only on
Unix.

**Architecture:** the console's browser pane already captures `browser.screenshot` on demand. A live
toggle polls that same capture on a slow timer (1.5 s), with one request in flight, pausing while the
tab is hidden, stopping on a refusal, unmount, grant change or no target. The device's `exit_facts` is
a pure function of `portable_pty::ExitStatus`; its tests move out of the Unix-only pty module into a
module that compiles and runs everywhere.

**Tech Stack:** React + TS (console page), Rust (device terminal tests).

**Spec:** the legacy-parity sweep's "Browser live view" item (the native pane cannot be ported; a
polled screenshot stream is the remote equivalent at the console's scale) and the round-7 record that
the pty tests are Unix-only.

## Global Constraints

- **Live is polling, not a screencast.** One `browser.screenshot` at a time, every 1.5 s, only while
  the toggle is on, a target is selected, and the document is visible. A tick during an in-flight
  capture is skipped, never queued. The UI says "Live · 1.5 s", not "real time".
- **It stops instead of looping on refusal.** A `ToolError` with 403, `-32001` or `-32002` turns live
  off and shows the existing failure sentence; any other error also stops live and shows the
  sentence. The operator restarts it with the button.
- **The on-demand path is unchanged.** The Screenshot button and the click-on-image path behave as
  today; the tests for them stay green. The live image is the same base64 `<img>`.
- **No background work.** Unmount, toggle off, grant change, target change to none, or a hidden
  document stops the timer; returning to visible resumes only if the toggle is still on.
- **The device change is tests only.** `exit_facts` and `EndFacts` are untouched; the new tests
  construct `portable_pty::ExitStatus::with_exit_code`/`with_signal`, which exist on every target.
  The pty-creation tests remain Unix-only; the ConPTY reader still is not executed on Windows CI, and
  the module comment says so.
- English; `npm run typecheck`, `npm test`, `./scripts/build-web.sh` (exit code checked, bundle
  committed); `cargo test -p summrise-device`, clippy, fmt.

## Review Focus

1. **One capture in flight.** A slow screenshot makes the next tick a no-op; the test drives a
   deferred promise and asserts a single call across several intervals.
2. **Every stop is covered.** Toggle, unmount, a changed grant, no target, a hidden document, and a
   refusal each end the timer; no interval survives its owner.
3. **No new data path.** The live frame is the same tool answer and `<img>`; no second endpoint, no
   device change, no grant widening (the operator already minted `browser.screenshot`).
4. **The tests are honest about Windows.** The new parser tests run on Linux and Windows; the pty
   tests stay Unix; nothing claims the ConPTY path is executed on Windows.
5. **The label matches the behaviour.** The UI says 1.5 s and the code uses one constant; a stopped
   live view says why.

---

### Task 1: Live browser view

**Files:**
- Modify: `device/console/web/src/BrowserPane.tsx` + `BrowserPane.test.tsx`
- Modify: `device/console/web/src/app.css` only if a class is new

**Interfaces:**
- `const LIVE_MS = 1_500;`
- A **Live** toggle beside the Screenshot button: label `Start live view` / `Stop live view`; while
  on and a frame exists, a short caption `Live · 1.5 s · frame N` (the exact words may be the
  implementer's, but the interval must appear).
- The polling effect depends on `live`, `grant`, `target`; it clears on every dependency change and on
  unmount. A `document.visibilitychange` listener skips ticks while hidden.
- Reuse the existing `capture` implementation or extract a `captureOnce` that the button and the
  timer share; the shot gate must still drop stale answers.
- Tests: start → several ticks over fake time with one call each; in-flight guard; stop via toggle,
  unmount, hidden, grant change, empty target; refusal stops and shows the sentence; the existing
  screenshot/click/rename tests unchanged.

- [ ] **Step 1: Tests first**
- [ ] **Step 2: Implement, gates, commit**
  `console: the browser pane can watch at a slow live rate`.

### Task 2: End facts on every platform

**Files:**
- Modify: `device/device/src/capabilities/terminal.rs` (tests only)

**Interfaces:**
- A `#[cfg(test)] mod end_facts` **outside** `#[cfg(all(test, unix))]`, with:
  - `exit_facts(&ExitStatus::with_exit_code(7))` → `exit_code == Some(7)`, `signal == None`;
  - `exit_facts(&ExitStatus::with_exit_code(0))` → `Some(0)`, `None`;
  - `exit_facts(&ExitStatus::with_signal("Killed"))` → `signal == Some("Killed")`,
    `exit_code == None` (the parser's Display prefix);
  - `EndFacts::default()` → both None.
- Keep the Unix pty module and its integration assertions; add a comment there that the parser's
  cross-platform cases live in `end_facts`. Do not move the pty tests.

- [ ] **Step 1: Write the tests; confirm they fail if the parser is mutated (e.g. swap the branches).**
- [ ] **Step 2: Run the device gates, commit**
  `device: end facts are tested on every platform, not only Unix`.

## Self-review

- Spec: the live view's value (watch without clicking) and the recorded Windows test gap.
- Review Focus: single-flight, all stops, same data path, honest Windows claim, label.
- Deliberate: 1.5 s polling rather than CDP screencast/WebSocket (a transport project of its own); no
  device change for the tests.
