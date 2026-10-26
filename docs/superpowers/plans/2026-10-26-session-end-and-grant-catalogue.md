# The end of a session, the grant form's catalogue, and the small shared pieces

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** make a terminal tab say that its session ended (and with what exit), move the grant form's
tool catalogue onto the tested client, and give `Modal`, `copyText` and `useRequestGate` their tests.

**Architecture:** `terminal.read` already reads a session that has ended (the session survives a
natural exit with `eof` set) but reports only data and a cursor, so the console cannot tell a finished
shell from a quiet one. The device gains the end facts on that answer and the console marks the tab and
stops polling — no new tool, no new route. `api.tools()` replaces the grant form's raw `fetch`. The
small pieces are test-only.

**Tech Stack:** Rust (device `terminal.rs`, `mcp.rs`), React 19 + TypeScript, vitest/jsdom/RTL.

**Spec:** `docs/superpowers/specs/2026-10-25-legacy-parity-sweep.md` § "Worth doing later"
("Liveness/exit-code mark and hide … a per-tab exit code is a device change"); the raw `/tools` fetch
in `GrantForm.tsx` is the last component-level `fetch`; `Modal`/`copyText`/`useRequestGate` are used
everywhere and tested nowhere.

## Global Constraints

- **No new tool, no registry change.** `terminal.read`'s answer grows `alive`, `exit_code` and
  `signal`; a client that ignores them sees the old `{data, cursor}`. An **older device** that sends
  neither is treated as alive — the console must never mark a tab ended on a missing field.
- **The end is a fact, not an error.** A session that ended naturally keeps its retained output; the
  console appends the last bytes, then marks the tab. Closing the tab is still the operator's act.
  No auto-hide (the output is the reason to keep the tab).
- **Stop asking once ended.** The poll's existing `closed` guard already skips a faulted tab; the read
  that discovers the end sets that fault, and the next tick must send nothing.
- `api.tools()` is one `call<{name:string}[]>("/tools")`; the grant form keeps its fallback on any
  failure and no longer imports `getToken` or calls `fetch`.
- Task 3 is test-only **except the defects its tests found**: `Modal`'s focus re-running on every
  parent render (an inline `onClose` in the effect's deps) and `copyText` leaking/rejecting on a
  throwing `execCommand` are fixed in the same task, with regression tests. No other production change.
  English. `npm run typecheck` 0; `cargo test -p summrise-device`; clippy/fmt;
  `npm test`; `./scripts/build-web.sh` with its **exit code checked directly** (never piped), bundle
  committed.

## Review Focus

1. **A natural exit is reported once and honestly.** `alive` is false only after the reader saw EOF or
   the child's status; the exit facts are the child's, captured where it is reaped (the ConPTY
   would-block path reaps it too — do not drop that status); a session still running says alive.
2. **An older device is not a dead one.** A `read` answer without `alive` leaves the tab alone; only an
   explicit `false` marks it.
3. **The sentence says what is known.** Exit code, signal, or neither — three sentences, never an
   invented code; the retained output stays in the tab.
4. **The catalogue path is the client's.** `api.tools()` carries the token and the JSON rules like
   every other call; a rejection keeps the 38-name fallback usable; no raw `fetch` remains in
   `GrantForm`.
5. **The shared pieces do what their comments claim.** Modal closes on Escape and the scrim, not on a
   click inside, and focuses the first field; `copyText` reports success only when the clipboard took
   the text; `useRequestGate`'s newest wins and its unmount invalidates.

---

### Task 1: The end of a session, on the wire and on the tab

**Files:**
- Modify: `device/device/src/capabilities/terminal.rs` (`Output`, `drain_reader`, `read` and its call
  sites/tests)
- Modify: `device/device/src/mcp.rs` (`Handler::TerminalRead`, its tests)
- Modify: `device/console/web/src/TerminalWorkspace.tsx` (`readOnce`, `poll`, the tab label)
- Modify: `device/console/web/src/TerminalWorkspace.test.tsx`

**Interfaces:**
- `ReadAnswer { data: Vec<u8>, cursor: u64, alive: bool, exit_code: Option<i32>, signal: Option<String> }`;
  `alive` is `!output.eof`; the exit facts come from reaping the child (`portable_pty::ExitStatus`;
  verify the crate's 0.8.1 accessors before choosing the sentence fields).
- The MCP answer: `{data, cursor, alive, exit_code: number|null, signal: string|null}`.
- The console: `readOnce` returns the same optional fields; when `alive === false`, `poll` sets
  `{kind: "closed", sentence}` and returns. The sentence is:
  - `The session ended with exit code 7. The output above is what it printed.`
  - `The session ended on signal Killed. …` (the crate's public spelling — `portable-pty` 0.8.1
    exposes no `signal()` accessor, so the device parses its `Display`; a signal is never invented as
    exit code 1)
  - `The session ended. …`
  The tab's visible label gains ` · ended` while the fault is closed. The tab is never auto-hidden.

- [x] **Step 1: Device tests first**

In `terminal.rs`, a pty test: create a local session, write `exit 7\n`, poll `read` with a deadline
(about 5 s) until `alive` is false, then assert `exit_code` is `Some(7)`, the data still contains the
output, and a second session that is still running reports `alive: true` with `exit_code: None`. The
pty test module is `#[cfg(all(test, unix))]` (pre-existing gate), so **Windows compiles the ConPTY
branch but does not execute this test**; that gap is recorded, not promised closed. In `mcp.rs`, the
`terminal.read` integration test asserts the JSON has `alive: true` for a live session.

- [x] **Step 2: Implement the device half**

`Output` gains `exit_code: Option<i32>` and `signal: Option<String>`. In `drain_reader`, keep the
`ExitStatus` wherever `try_wait` returns `Some` (the ConPTY would-block branch consumes it once) and set
the fields before `eof = true`; a serial session has no child, so it keeps `None`. `read` returns
`ReadAnswer`; update every call site (tests destructure the tuple today) and the handler.

- [x] **Step 3: The console half, tests first**

`TerminalWorkspace.test.tsx`: a read answer `{data: "bye\n", cursor: 3, alive: false, exit_code: 7}`
→ the tab shows the fault sentence with `exit code 7`, the tab label ends with `ended`, and after the
next 250 ms tick no further `terminal.read` was sent. A live answer without `alive` changes nothing; a
live answer with `alive: true` clears a previous read fault as today.

- [x] **Step 4: Implement the console half**

Extend the `readOnce` type; in `poll`, after appending data and the cursor, branch on `alive === false`
to set the closed fault (three sentences) and return (the next poll's existing closed guard skips it).
Add the label suffix for a closed tab. Do not hide the tab.

- [x] **Step 5: Verify and commit**

```bash
source ~/.cargo/env
cargo test --manifest-path device/Cargo.toml -p summrise-device
cargo clippy --manifest-path device/Cargo.toml -p summrise-device --all-targets -- -D warnings
cargo fmt --manifest-path device/Cargo.toml --all -- --check
cd device/console/web && npm run typecheck && npm test
cd - && ./scripts/build-web.sh
```

Commit: `device+console: a tab learns that its session ended, and with what`.

### Task 2: The grant form asks the client for its catalogue

**Files:**
- Modify: `device/console/web/src/api.ts` (add `tools`)
- Modify: `device/console/web/src/api.test.ts`
- Modify: `device/console/web/src/GrantForm.tsx` (the mount effect)
- Modify: `device/console/web/src/GrantForm.test.tsx`
- Modify: `device/console/web/src/ConnectCard.tsx` and `ConnectCard.test.tsx` (the second raw
  `/tools` fetch the plan missed; the same `api.tools()` swap and its first tests)
- Modify: `device/console/web/src/App.test.tsx` (its hand-written `api` mock needs `tools`)

**Interfaces:**
- `api.tools: () => Promise<{name: string}[]>`; GET `/tools`, the token header the client already adds.
- `GrantForm`'s effect calls `api.tools()`; the fallback and the selection logic are untouched.

- [x] **Step 1: Tests first**

`api.test.ts`: `api.tools()` GETs `/tools`, carries the bearer header after `setToken`, and parses the
array. `GrantForm.test.tsx`: the catalogue success and failure cases mock `api.tools` instead of
`fetch` (success replaces the list; a rejection keeps the 38-name fallback usable). Keep the
pre-existing consent test's body byte-identical; only its render helper's stub changes, and that is
stated in the commit.

- [x] **Step 2: Implement**

Add `tools` to the `api` object; replace the effect's `fetch("/tools", {headers: {authorization: ...}})`
with `api.tools()`; drop the now-unused `getToken` import. No other change.

- [x] **Step 3: Verify and commit**

`npm run typecheck`, `npm test`, build with the exit code checked, bundle committed.
Commit: `console: the grant form's catalogue goes through the client`.

### Task 3: The small shared pieces' tests

**Files:**
- Create: `device/console/web/src/Modal.test.tsx`, `device/console/web/src/copy.test.ts`,
  `device/console/web/src/requests.test.ts` (test-only)

**Interfaces:** the existing exports; no production change.

- [x] **Step 1: Modal**

Render `Modal` with a child input; assert the first field has focus; Escape calls `onClose`; a
`mousedown` on the scrim calls it; a `mousedown` inside the dialog does not; unmounting removes the
listener (a later Escape does not call again). Use the role `dialog` and its label.

- [x] **Step 2: copyText**

Stub `navigator.clipboard.writeText`: resolve → `true`; reject → `document.execCommand("copy")` is used
(stub it `true`/`false`), the temporary textarea is removed, and the result matches the command's
answer; a throwing `execCommand` does not escape as an unhandled rejection (it may reject — pin what it
does and say so).

- [x] **Step 3: useRequestGate**

`renderHook`: a first ticket is current; a second `begin` makes the first stale; the returned gate's
`stale` answers for its own tickets; unmounting makes the last ticket stale.

- [x] **Step 4: Run and commit**

`npx vitest run src/Modal.test.tsx src/copy.test.ts src/requests.test.ts`, then the whole suite and
typecheck; Modal and copy are production fixes, so the bundle changes — rebuild and commit it. Commit:
`test: the dialog, the clipboard and the newest-request gate`.

## Self-review

- Spec coverage: the sweep's liveness/exit-code (now device + tab, mark not hide), the last raw
  component `fetch`, and three shared pieces used everywhere with no test.
- Review Focus: exit capture/reporting (Task 1 tests), older-device compatibility (no `alive`), the
  three sentences, the client path and fallback (Task 2), the shared pieces' comments (Task 3).
- Deliberate scope: no auto-hide, no new tool, no notification.
