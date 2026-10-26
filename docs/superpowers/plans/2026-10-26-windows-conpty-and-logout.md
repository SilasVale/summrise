# The ConPTY path on Windows, and a logout that ends the session

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the terminal's Windows ConPTY path — the code the vendored reader was patched for — is
executed by CI, and signing out of the console also removes the session on the server, not only in
the tab's memory.

**Architecture:** a `#[cfg(all(test, windows))]` test builds the same small registry fixture the Unix
tests build, starts `cmd.exe`, writes a command, reads its output through the registry, exits, and
asserts the end facts — so `windows-latest`'s workspace test run drives ConPTY for real. Logout is a
`POST /logout` behind the operator session that removes the presented token from the in-memory session
list; the page calls it best-effort and clears regardless.

**Tech Stack:** Rust (device terminal tests; console `http.rs`), React + TS (console page api/App).

**Spec:** the round-7/13 note that the pty tests are Unix-only and ConPTY is not executed on Windows
CI; the console's session model (`auth.rs`: in-memory, 24 h, no revocation).

## Global Constraints

- **The Windows test is Windows-only and bounded.** `#[cfg(all(test, windows))]`; a 20-second overall
  deadline per read loop; no `pwsh` dependency (`cmd.exe` is the shell whose availability is certain);
  the Unix tests and the `end_facts` module are untouched. It must not hang the job: every loop checks
  its deadline and panics with the output it has.
- **No production change for the test.** The registry API is used as the app uses it: `create(
  CreateTarget::Pty { shell: Some("cmd.exe") }, 24, 80)`, `write`, `read`, and the `alive`/`end`
  fields; the test closes nothing it did not create (the registry drops sessions on its own).
- **Logout is authenticated and idempotent in effect.** `POST /logout` sits behind `OperatorSession`,
  removes the session whose token the request presents, and answers 204. A second call after the
  session is gone is 401 (the extractor refuses), which is the correct answer: there is nothing to
  log out. No audit kind changes; session lifecycle is not a device action.
- **The page never hangs on logout.** `signOut` calls the endpoint best-effort and clears the local
  state whether it answers, fails or times out; the browser's session is in memory and must not
  survive a failed call.
- English; `cargo test -p summrise-device` (Linux) plus the Windows CI for the new test; console
  `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **The Windows test exercises the real reader.** It creates a pty via the registry (not a raw
   `portable-pty` call), reads through `PtyRegistry::read`, and asserts the session's own end facts —
   the path the device serves. It does not assert a prompt shape or timing beyond a marker, and every
   wait is bounded.
2. **It cannot wedge the job.** A failed start, a marker that never arrives, or a session that never
   ends panics at its deadline with the accumulated output; no unbounded `wait`.
3. **Logout removes the right session.** The token compared is the presented bearer; another
   operator's session is untouched; the removed session's next request is 401; a session from Access
   (no in-memory token) is not affected.
4. **The page clears regardless.** A rejected or slow logout still clears the session, devices, audit,
   tokens, grant and page; no token or grant survives in memory.
5. **The Windows claim is honest.** The test runs only on Windows; the report says whether it was
   verified by CI, and does not claim the serial or SSH backends are exercised.

---

### Task 1: The Windows ConPTY test

**Files:**
- Modify: `device/device/src/capabilities/terminal.rs` (tests only; a new `#[cfg(all(test, windows))]`
  module)

**Interfaces:**
- Fixture: `TempDir` from `crate::test_support`; `Store::new(dir.join("install"), &data,
  Release::standalone())` + `store.save(&store.initial())`; a `Clock` returning 1_000;
  `PtyRegistry::with_clock(store, Arc::new(tokio::sync::Mutex::new(())), clock,
  Limits::default())`.
- Test `cmd.exe`:
  1. `create(CreateTarget::Pty { shell: Some("cmd.exe") }, 24, 80)`;
  2. `write(&id, b"echo SUMMRISE42\r\n")`;
  3. `read(id, cursor)` until the output contains `SUMMRISE42` or 20 s;
  4. `write(&id, b"exit\r\n")`;
  5. `read` until `alive == false` or 20 s; assert `end.exit_code == Some(0)` and the output contains
     the marker.
- If `cargo xwin check --manifest-path device/Cargo.toml -p summrise-device --target
  x86_64-pc-windows-msvc --tests` is available locally, run it for a compile check; otherwise say so
  and rely on the Windows CI. Do not add a Linux-runnable fake of the test.
- The Unix module gets a one-line pointer to the Windows module; no other production change.

- [ ] **Step 1: Write the test, compile-check what is possible, run the Linux suite (which excludes it).**
- [ ] **Step 2: Commit `device: the Windows ConPTY path is exercised by a test`.**
- The round is not complete until the Windows CI run of the PR is green; report the CI result.

### Task 2: Logout

**Files:**
- Modify: `device/console/src/http.rs` (route + handler + tests in `tests/console.rs`)
- Modify: `device/console/web/src/api.ts` (+ `api.test.ts`), `App.tsx` (+ `App.test.tsx`)

**Interfaces:**
- `POST /logout` behind `OperatorSession`: `bearer_secret(headers)` → remove the matching session
  (`constant_time_eq` over the 32 bytes) → 204. The route is additive; `/auth`, `/session` unchanged.
- `api.logout(): Promise<void>` — `POST /logout`.
- `signOut`: fire `api.logout()` best-effort (catch), then clear exactly what it clears today (token,
  session, devices, audit, tokens, selected, grant, grantStale, page).
- Tests: Rust — two sessions, logout one, the other still works and the logged-out one is 401; the
  memory list no longer holds it; an unauthenticated logout is 401. Web — `api.logout` path/method;
  signOut calls it and clears; a rejected logout still clears; no `srm_`/token in the page.

- [ ] **Step 1: Tests first**
- [ ] **Step 2: Implement, gates, commit `console: signing out ends the session on the server too`.**

## Self-review

- Spec: the Windows execution gap; the session's missing revocation.
- Review Focus: real reader path, bounded waits, right session removed, page always clears, honest
  Windows claim.
- Deliberate: no audit kind for logout, no server-side session list UI, no Access session revocation
  (Access is the identity provider's session).
