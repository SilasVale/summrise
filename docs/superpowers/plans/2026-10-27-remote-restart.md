# A deliberate remote restart: `host.restart`

> **For agentic workers:** Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** an operator can restart a supervised device's app from the console's System page, through a
new destructive MCP tool `host.restart`, with a confirmation that names what is lost and a refusal
that is a sentence when the app has no supervisor to bring it back.

**Architecture:** the device already restarts itself after a supervised panel enrollment: a shared
hook exits after two seconds and the launcher starts the app again. This round makes that hook a
module both paths use (`restart.rs`), adds the tool to the registry and the device handler, and adds
the button to the System page (which already has a session grant and a destructive kill confirmation
to model). No console-server route and no new credential.

**Tech Stack:** Rust (`summrise-core` registry, `summrise-device` mcp/http/restart), React + TS
(`SystemPage`, `Modal`, tests).

**Spec:** the supervised-restart contract from round 20 (`SUMMRISE_SUPERVISED=1`, the launcher's
restart budget of three consecutive restarts).

## Global Constraints

- **The tool exists once, in the registry.** `host.restart` is a `ToolDef` in `summrise-core`'s host
  capability with `destructive: true`; the device's schema/handler and the console's `/tools` are
  readings of that registry, not copies.
- **A standalone app refuses.** `Config.supervised` is the launcher's mark; without it the handler
  returns the sentence "this device was not started by the launcher; restarting it here would not
  bring it back" and **never calls the hook**. The refusal is the device's, not the page's.
- **The answer precedes the exit.** The handler builds `{"restarting": true}`, calls the shared hook,
  and returns it; the hook sleeps two seconds before `std::process::exit(0)`. A shared
  `restart::Hook` replaces the HTTP module's private type and closure, so enrollment and this tool
  exit by the same code. Tests pass recorders and never end the process.
- **The budget is stated.** The tool's description and the page's confirmation say the launcher
  allows three consecutive restarts and that repeated restarts can leave the device down; the tool
  returns success as a fact, not a promise that the device reconnects.
- **Destructive consent is the grant's.** The button is not hidden on a guess: the console/device
  enforce the grant's tools and `allow_destructive`; a refusal is shown in the device's words. The
  page's confirmation is the operator's deliberate step, not the authorization.
- **The confirmation names the loss.** Open terminals, browser tabs and runs on the device end; the
  device goes offline briefly; Cancel is the first focus and makes no call; only "Restart app" calls
  the tool, once.
- English; backend `cargo test -p summrise-device -p summrise-core`, clippy, fmt, `cargo xwin check
  --tests` for both; web `npm run typecheck`, `npm test`, `build-web.sh` (exit code, bundle
  committed).

## Review Focus

1. **The refusal path is real.** Tests: an unsupervised server refuses, the hook is not called; a
   supervised one calls it once and returns `restarting: true`. The UI test covers a refusal shown
   verbatim.
2. **One hook, two callers.** `http::serve`'s enrollment restart and `mcp`'s tool both use
   `crate::restart::Hook`; no second exit implementation; the existing enrollment/supervised tests
   still pass, and the HTTP router passes the same hook into the MCP service.
3. **The registry stays the surface.** The core tool list and the device's `tools/list` include
   `host.restart`; the destructive table says `true`; the device's schema has no properties and
   `additionalProperties: false`.
4. **The page is deliberate.** The confirm names losses and the budget; cancel makes no call; one
   click makes one call; success says the device is restarting and does not claim it is back.
5. **No new authority.** No console route, no token, no grant shortcut; the tool is reached through
   the same `/mcp` path as every other tool, audited like them.

---

### Task 1: The device path

**Files:** create `device/device/src/restart.rs`; modify `device/device/src/main.rs` (mod),
`http.rs` (hook type/closure → `restart::Hook`; pass the hook into `mcp::service`), `mcp.rs`
(DeviceServer `supervised`/`restart`, `with_restart`, `service(..., restart)`, handler, schema, tool
list test), `device/core/src/registry.rs` (+ tests).

**Interfaces:**
- `restart::Hook = Arc<dyn Fn() + Send + Sync>`; `restart::none()`; `restart::exit_soon()` (2 s then
  `exit(0)`).
- `DeviceServer::new(config, limits)` (supervised from `config.supervised`, hook `restart::none()`);
  `with_restart(hook)`; `mcp::service(..., restart: Hook)`.
- `host.restart` schema `{type:"object", properties:{}, additionalProperties:false}`; handler result
  `{"restarting": true}`; unsupervised refusal sentence above.

- [ ] **Step 1: Tests first** (core list/destructive; mcp refusal/success + tool list)
- [ ] **Step 2: Implement, gates, commit**
  `device: a supervised app can be restarted through host.restart`.

### Task 2: The page

**Files:** modify `device/console/web/src/SystemPage.tsx` + `SystemPage.test.tsx` (and `app.css` if
needed; bundle).

**Interfaces:**
- A "Device app" section after the host header (before processes) with a **Restart app** button.
- Clicking opens a `Modal` titled "Restart the device app?" naming the losses, the launcher budget,
  and a "Restart app" / "Cancel" pair; cancel first.
- Confirm: `callTool(grant, "host.restart", {})`; success shows "The device is restarting through
  its launcher. It will reconnect in a few seconds; terminals, browser tabs and runs on the device
  ended."; refusal shows "The device did not restart: <sentence>"; a failure shows the tool error.
  One call; the confirm closes; the button is disabled while the call is on the wire and after a
  success until the page is reloaded.
- Tests: confirm opens; cancel no call; confirm exactly one `host.restart`; success sentence; refusal
  sentence; no client-side hiding based on the grant.

- [x] **Step 1: Tests first**
- [x] **Step 2: Implement, gates, commit**
  `console: the System page restarts a supervised device app`.

### Task 3: The record

- [x] Tick the plan, record the rulings (registry as the single list; the refusal sentence; the
  budget copy; one hook), commit `docs: the remote restart's decisions`.

## Self-review

- Spec: the supervised restart exists; the operator now has a deliberate path and the standalone case
  refuses.
- Review Focus: refusal test, one hook, registry surface, deliberate page, no new authority.
- Deliberate: no console button outside the System page; no restart history; no auto-retry; the
  launcher budget is a stated limit, not a queue.

## Rulings

Recorded when the page shipped (`24a72cb`). The device half is Task 1's; these are the decisions the
round rests on, and the page's tests pin the ones it can see.

- **The registry is the single list.** `host.restart` is one `ToolDef` in `summrise-core`'s host
  capability with `destructive: true`; the device's schema and handler and the console's `/tools` are
  readings of it. The System page keeps no copy and does not ask which tools the grant has: hiding
  the button on that guess would make the device's refusal unreachable, and the grant's tools and
  `allow_destructive` are enforced on the call. The cost: a grant that cannot restart still sees the
  button and learns the boundary from the refusal.

- **The refusal is the device's sentence.** Without `Config.supervised` the handler rejects with
  "this device was not started by the launcher; restarting it here would not bring it back" and never
  calls the hook. The page wraps that verbatim in "The device did not restart: …"; it does not
  inspect a process list or a grant to predict it. The cost: the reason is prose, not a code the page
  can branch on.

- **The budget is stated, not queued.** The confirmation names the losses (open terminals, browser
  tabs, runs), the brief offline, and the launcher's three-consecutive-restart allowance with "three
  quick restarts can leave the device down". Cancel is first and makes no call; only "Restart app"
  calls, once; after success the button is disabled until a reload. The cost: a legitimate second
  restart needs a reload, because the page cannot know when the device is back.

- **One hook, two callers.** Enrollment's restart and `host.restart` exit through `restart::Hook`:
  the handler builds `{"restarting": true}`, calls the hook, returns the answer, and the hook sleeps
  two seconds before `std::process::exit(0)`. The page therefore says "restarting", never "back",
  and tests pass recorders rather than ending a process. The cost: success is a fact about the
  request, not evidence of reconnection.
