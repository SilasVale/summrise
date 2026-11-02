# Capability Hub v1 Implementation Plan

> **For agentic workers:** work one task at a time; each task ends with a green command and a commit.
> The spec is `docs/design/capability-hub-2026-11-02.md` — read it before the first task. Tasks are
> dispatched to subagents with the task number in the brief; the Lead reviews the diff and runs the
> final gates.

**Goal:** Ship the capability hub v1: a device plugin host behind the existing MCP surface, and a
five-destination console whose Home, Capabilities, Plugins, Connect and Device screens make that hub
legible and testable — then deploy the console and verify it against a real device.

**Architecture:** The hub is the existing device runtime plus a plugin supervisor: a plugin is an
MCP server child process, its tools are namespaced and merged into `tools/list`, and `tools/call`
routes by name. The console keeps its existing proxy and adds one read route (`/fleet/:id/tools`) so
the page, the token form and the catalogue all read the device's live tools. The page is rebuilt
from a seven-item device rail into five hub destinations; the machine workspace becomes one
destination with five tabs.

**Tech Stack:** Rust 1.98.1 (tokio, axum, serde_json, rmcp), React 19 + Vite (one HTML file embedded
with `include_str!`), PowerShell for the shipped example plugin, no new runtime dependency.

**Spec:** `docs/design/capability-hub-2026-11-02.md`

## Global Constraints

- The repository is written in English; commits are conventional (`feat:`, `fix:`, `docs:`), each
  names what it proves.
- Work on `hub/v1`. Do not merge `main` into it; `origin/main` is an ancestor already.
- `source ~/.cargo/env` before cargo. `cargo` is not on PATH in a plain shell.
- No new JavaScript/CSS dependency. The page is React 19, built to one `dist/index.html`.
- No new service, database or long-lived file format beyond `<data>/plugins.json` and
  `<data>/logs/plugin-<namespace>.log`.
- Windows is the target. Every plugin path is a Windows path in the docs; the example plugin is
  PowerShell. Unix tests must not assume a Windows executable.
- Public JSON contracts fixed by this plan (do not improvise):
  - `plugins.list` → `[{ "id", "namespace", "name", "version", "description", "state", "tools", "path", "last_error" }]`
    (`state` is one of `running|starting|stopped|failed`; `tools` is a number; `last_error` is a
    string or null).
  - `plugins.inspect` → `{ "id", "namespace", "name", "version", "description" }` — read-only: it
    resolves the directory and validates `plugin.json`, starts nothing and writes no record. The
    add sheet's blur calls it.
  - `plugins.add` → `{ "plugin": <list entry>, "tools": <number> }`.
  - `plugins.remove` / `plugins.restart` → `{ "plugin": <list entry> }` (remove's entry has state
    `stopped`).
  - `plugins.logs` → `{ "namespace": string, "lines": [string] }`.
  - `GET /fleet/:id/tools` → `{ "tools": [{ "name", "description", "inputSchema", "source", "capability", "destructive" }] }`
    where `source` is `"builtin"` or `"plugin:<namespace>"`, `capability` is the built-in group or
    the namespace, and `destructive` is a boolean.
- The type scale is the committed seven (10, 11, 12, 13, 14, 17, 22 px); spacing is the committed
  rhythm (2, 4, 6, 8, 10, 12, 14, 16, 24); radii are 6/8/10/14/20. A literal outside those fails
  `device/console/tests/tokens.rs`; a new scale value needs the token and that test in one commit.

## Review Focus

The spec implies these but no single task's happy path tests them; each owning task pins its line.

1. A plugin that writes a malformed stdout line or a notification must not wedge the device or lose
   the next answer (A2).
2. A plugin that dies while a call is in flight must fail that call with a sentence, not hang (A3).
3. The device process exiting must not leave a plugin child alive (A3).
4. An offline device must render as "offline", never as "no tools" (C3/C4).
5. Minting a token with a plugin tool while the device is offline must be refused with an action,
   not silently accepted or dropped (B2).
6. A path containing spaces or non-ASCII must work on Windows (A1/A3).
7. A device running an older build must show the Plugins page's "newer Summrise" state, not a broken
   Add button (C5).
8. A plugin tool name colliding with a built-in must be refused at install, and the plugin must not
   start (A1/A3).
9. A sheet must trap focus, close on Escape, and return focus to its trigger (C2).
10. The old `#terminal` and `#run/<device>/<id>` links must survive the shell change (C1/C7).

---

# Workstream A — Device: the plugin host

## Task A1: Manifest and validation in `summrise-core`

**Files:**
- Create: `device/core/src/plugins.rs`
- Modify: `device/core/src/lib.rs` (add `pub mod plugins;`)
- Test: unit tests inside `device/core/src/plugins.rs`

**Interfaces (produces):**
```rust
pub struct PluginManifest {
    pub id: String, pub namespace: String, pub name: String, pub version: String,
    pub description: Option<String>, pub command: Vec<String>,
    pub cwd: Option<String>, pub env: BTreeMap<String, String>, pub transport: Option<String>,
}
pub enum ManifestError { /* one variant per rule, Display = the sentence */ }
impl PluginManifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError>;   // serde_json, deny_unknown_fields
    pub fn check(&self) -> Result<(), ManifestError>;          // every non-path rule
}
pub fn tool_name(namespace: &str, tool: &str) -> Result<String, ManifestError>; // ns.tool, charset
```
Rules: spec §8.2's table, plus: `cwd` must be relative and contain no `..`; `transport` if present
must be `stdio`; `env` keys `[A-Za-z_][A-Za-z0-9_]*`; duplicate tool names after namespacing are
caught by the caller (the manifest does not know the tool list).

**Steps:**
- [ ] Write failing tests for: a valid manifest; an unknown key; a namespace with an uppercase letter
      or underscore; an empty `command`; `cwd: "../x"`; `transport: "sse"`; a 65-char tool name; a
      tool name containing a space; `tool_name("hello", "greet") == "hello.greet"`.
- [ ] Run `cargo test --manifest-path device/Cargo.toml -p summrise-core plugins` — fails to compile.
- [ ] Implement the module; make the tests pass; `cargo fmt` and `cargo clippy -p summrise-core`.
- [ ] Commit: `feat(core): the plugin manifest and its validation`.

**Acceptance:** invalid manifests are refused with a sentence naming the field; no I/O in the module.

## Task A2: The MCP stdio session

**Files:**
- Create: `device/device/src/plugins/mod.rs` (module wiring)
- Create: `device/device/src/plugins/session.rs`
- Modify: `device/device/src/lib.rs` or `main.rs` module declarations (follow the existing tree)
- Test: unit tests in `session.rs` using `tokio::io::duplex`

**Interfaces (produces):**
```rust
pub struct PluginTool {
    pub name: String,                 // WITHOUT the namespace
    pub title: Option<String>, pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub read_only: Option<bool>, pub destructive: Option<bool>,
}
pub struct Session<S> { /* generic over AsyncRead + AsyncWrite */ }
impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Session<S> {
    pub fn new(read: S::Read, write: S::Write, log: LogSink) -> Self; // or split halves
    pub async fn initialize(&mut self, timeout: Duration) -> Result<String, PluginError>; // returns negotiated version
    pub async fn list_tools(&mut self, timeout: Duration) -> Result<Vec<PluginTool>, PluginError>;
    pub async fn call_tool(&mut self, name: &str, args: serde_json::Value, timeout: Duration) -> Result<String, PluginError>;
    pub async fn ping(&mut self, timeout: Duration) -> Result<(), PluginError>;
    pub async fn shutdown(self, grace: Duration);
}
```
- Newline-delimited JSON-RPC 2.0 over the stream; one reader task routes answers by id; ids are
  `u64` from an `AtomicU64`.
- `initialize` sends `protocolVersion: "2025-06-18"`, `capabilities: {}`, `clientInfo: {name:
  "summrise", version: env!("CARGO_PKG_VERSION")}`, then `notifications/initialized`.
- A malformed line is logged and skipped; a notification (method, no id) is logged and skipped; an
  answer for an unknown id is logged. `call_tool` returns the concatenated `content[].text` for text
  blocks, or the raw JSON text for any other result.
- Every request is bounded by its timeout; a timeout removes the waiter and returns
  `PluginError::Timeout { method, timeout }`.

**Steps:**
- [ ] Test with a duplex peer that answers `initialize`, then `tools/list` with one tool carrying
      annotations, then a `tools/call`; assert the values and the namespacing-free tool name.
- [ ] Test a malformed line followed by a valid answer: the valid answer still resolves.
- [ ] Test a timeout: a request with no answer returns `Timeout` and a later request still works.
- [ ] Implement; `cargo test -p summrise-device plugins::session`.
- [ ] Commit: `feat(device): an MCP stdio session for plugin children`.

**Acceptance:** protocol behaviour is proven without spawning a process.

## Task A3: The plugin host and supervisor

**Files:**
- Create: `device/device/src/plugins/host.rs`
- Create: `device/device/src/plugins/logs.rs`
- Modify: `device/device/src/plugins/mod.rs`
- Test: unit tests in `host.rs` with a fake launcher

**Interfaces (produces):**
```rust
pub struct PluginHost { /* Arc<Inner> */ }
pub struct PluginStatus { id, namespace, name, version, description, state, tools, path, last_error }
impl PluginHost {
    pub fn load(data_dir: &Path) -> Self;               // reads plugins.json; no spawn
    pub async fn start_all(&self);                      // boot: start every registered plugin
    pub async fn list(&self) -> Vec<PluginStatus>;
    pub async fn add(&self, source: &Path) -> Result<(PluginStatus, usize), PluginError>;
    pub async fn remove(&self, namespace: &str) -> Result<PluginStatus, PluginError>;
    pub async fn restart(&self, namespace: &str) -> Result<PluginStatus, PluginError>;
    pub async fn logs(&self, namespace: &str, lines: usize) -> Result<Vec<String>, PluginError>;
    pub fn destructive(&self, tool: &str) -> Option<bool>;     // only when Running
    pub fn tools(&self) -> Vec<PluginToolView>;                // namespaced, only when Running
    pub async fn call(&self, tool: &str, args: Value) -> Result<String, PluginError>;
    pub async fn stop_all(&self);
}
pub trait Launcher: Send + Sync { /* spawn -> SessionHandle; tests replace it */ }
```
- Registry file `<data>/plugins.json`: `{ "version": 1, "plugins": [{ manifest, path, installed_at_ms }] }`,
  written atomically (temp + rename), sorted by namespace.
- `add`: canonicalise, require directory, parse `plugin.json`, `check()`, refuse duplicate id or
  namespace, refuse a namespace equal to a built-in capability, spawn and handshake, then refuse and
  roll back the record if the plugin's namespaced tools collide with a built-in (use
  `summrise_core::registry::tool`) or with another running plugin. The record is persisted only
  after the handshake and the collision check succeed.
- State machine as the spec's diagram. Backoff: 1, 2, 4, 8, 16, 30 s; five starts in five minutes →
  `failed` until a manual restart. Health ping every 30 s; three misses restart.
- One call at a time per plugin; 120 s call timeout; a plugin that dies during a call returns
  `PluginError::Exited` to that waiter.
- Logs: `<data>/logs/plugin-<namespace>.log`, 4 MiB cap with `.1` rotation; child stdout is *not*
  logged except malformed lines and protocol notes; stderr is copied to the log.
- `stop_all` closes stdin, waits 2 s, kills; `start_all` is called once at boot.
- Dropping the host must not leave children; use a `Drop` that kills (best effort) in addition to
  `stop_all`.

**Steps:**
- [ ] Fake launcher: a `Session` over a duplex peer that answers tools/list with `greet` and echoes
      calls; assert `add` makes `tools()` contain `hello.greet`, `call` returns the echo, and `list`
      reports `running`.
- [ ] Fake launcher that fails initialize: `add` returns `Failed` and the registry file has no entry.
- [ ] Collision: a fake plugin exposing `files.read` is refused, not started.
- [ ] Health: a fake session that stops answering `ping` three times triggers a restart; the tools
      disappear while `starting`.
- [ ] In-flight death: a call whose peer drops returns `Exited` in under the timeout.
- [ ] Implement; `cargo test -p summrise-device plugins::`.
- [ ] Commit: `feat(device): the plugin supervisor, its registry and its logs`.

**Acceptance:** registry is atomic; no orphan children; failed installs leave no record.

## Task A4: `plugins.*` tools, the merged catalogue and routing

**Files:**
- Modify: `device/core/src/registry.rs` (add one `plugins` capability, five tools)
- Create: `device/device/src/capabilities/plugins.rs`
- Modify: `device/device/src/capabilities/mod.rs`
- Modify: `device/device/src/mcp.rs` (DeviceServer field, `new`, `service_with_terminal`,
  `tools/list`, `call_tool` authorization, `dispatch`, handler table)
- Test: `device/device/src/mcp.rs` tests + `device/device/src/capabilities/plugins.rs` tests

**Interfaces:**
- `registry::CAPABILITIES` gains `plugins` with `plugins.list` (read), `plugins.inspect` (read),
  `plugins.add` (destructive), `plugins.remove` (destructive), `plugins.restart` (destructive),
  `plugins.logs` (read). Descriptions are the operator's sentences (one sentence each, namespaced,
  actionable). `inspect` resolves and validates the manifest without spawning or recording.
- `DeviceServer` gains `plugins: Arc<PluginHost>`; `new` takes it or builds it from `Config.data`.
- `tools/list` = registry tools in registry order, then plugin tools sorted by namespace then name.
  Built-in tools keep their `input_schema(name)`; plugin tools use the plugin's `inputSchema`,
  `description` fallback to the tool name, and annotations passed through.
- `call_tool` authorization: the tool's destructive flag is `registry::is_destructive(name)` or, for
  a name not in the registry, `PluginHost::destructive(name)`. An unknown name still denies. The
  audit begin/end line names the namespaced tool exactly as called. Denials keep the existing shape.
- `dispatch`: `plugins.*` go to `capabilities::plugins::dispatch(host, name, args)`; a name whose
  first segment is a running plugin's namespace goes to `host.call(name, args)`; the existing table
  handles the rest and its `None` arm stays a bug.
- The alignment tests (`every_registry_tool_has_an_implementation`,
  `every_implementation_is_registered`, `dispatch_supports`) include the five new tools.

**Steps:**
- [ ] Failing tests: registry contains the six tools; a fake host advertising `hello.greet` makes
      `tools/list` contain it after the built-ins; `tools/call` for `hello.greet` reaches the fake
      host; a token without the tool is denied; an unknown plugin tool is denied.
- [ ] Implement the capability module and the MCP changes; fix the alignment tests.
- [ ] `cargo test --manifest-path device/Cargo.toml -p summrise-device -p summrise-core`;
      `cargo clippy ... -D warnings`; `cargo fmt --all -- --check`.
- [ ] Commit: `feat(device): plugins are tools on the one endpoint`.

**Acceptance:** a plugin call is an ordinary audited, authorized call.

## Task A5: Wire the host into the running device

**Files:**
- Modify: `device/device/src/main.rs` and/or `device/device/src/http.rs` (where
  `service_with_terminal` is called), `device/device/src/config.rs` if a data-dir helper is missing
- Test: the existing device tests still pass; add a test that `PluginHost::load` is called with
  `config.data` (a small unit around the constructor).

**Steps:**
- [ ] Create the host once at boot, `start_all().await`, pass it to the MCP service, call
      `stop_all` on shutdown (the service drop may be too late; do it in the run loop's shutdown).
- [ ] Confirm a device with no `plugins.json` boots with an empty host and one log line.
- [ ] `cargo test -p summrise-device`; commit: `feat(device): the hub starts its plugins at boot`.

---

# Workstream B — Console server

## Task B1: `GET /fleet/:id/tools`

**Files:**
- Modify: `device/console/src/http.rs` (route + handler)
- Test: `device/console/src/http.rs` tests

**Behaviour:** operator session; build an operator token scope for the device (the existing
`/mcp` path builds one — reuse the helper), send `tools/list` with `channels.request`, parse
`result.tools`; map each to the contract; `source`/`capability` come from
`summrise_core::registry::CAPABILITIES` by name, else `plugin:<first segment>`; `destructive` uses
the same MCP default as the device. 503 offline, 504 timeout, 500 malformed. An empty list is 200
with `[]` (the page distinguishes "older build" by the absence of `plugins.*`, not emptiness).

**Steps:**
- [ ] Failing test with the existing fake device: the route returns the device's tools and marks
      `terminal.create` builtin/`terminal`/destructive, `hello.greet` plugin/`hello`.
- [ ] Offline test: 503 and a sentence.
- [ ] Implement, run `cargo test -p summrise-console fleet` and commit:
      `feat(console): the page can read a device's live tool catalogue`.

## Task B2: Token scopes can name plugin tools

**Files:**
- Modify: `device/console/src/http.rs` (`create_token` / `mint_token` validation; the same rule for
  rotate), and `device/console/src/tokens.rs` if validation lives there
- Test: the token tests

**Behaviour:** a tool in `summrise_core::registry` is accepted as today. A tool outside it is
accepted only when the target device is online and its live `tools/list` contains the exact name;
otherwise 400 `unknown_tool` with the sentence "the device does not advertise {tool}; connect it and
try again". For an aggregate token, keep the static registry (v1.1) and refuse a plugin tool with a
sentence that says so. The check must not change the token's stored shape.

**Steps:**
- [ ] Failing tests: plugin tool online accepted; offline refused; unknown name refused; built-in
      unchanged.
- [ ] Implement; `cargo test -p summrise-console tokens`; commit:
      `feat(console): a token can scope a plugin tool the device advertises`.

---

# Workstream C — Console page

> One worker owns this whole workstream: `App.tsx`, `app.css`, `Rail.tsx`, the new pages and the
> harness. Do the shell first (C1), then pages in order; commit each task. The existing page
> components are reused inside Device and must keep their tests passing.

## Task C1: The five-destination shell

**Files:**
- Modify: `device/console/web/src/Rail.tsx` (five destinations, labelled sidebar, bottom tabs)
- Modify: `device/console/web/src/App.tsx` (Page type, routing, selected device persistence, header,
  account menu, status strip deletion)
- Modify: `device/console/web/src/app.css` (shell, rail, header, tab bar, responsive)
- Modify: `device/console/web/src/surface-link.ts` and its test (new fragments)
- Test: `App.test.tsx`, `surface-link.test.ts`

**Behaviour:**
- `Page = "home" | "capabilities" | "plugins" | "activity" | "device"`; `device` has a tab
  `terminal|screen|files|runs|machine` in the fragment (`#device/terminal`).
- Default: no selected device → `home` shows the fleet zero state; selected → `home` shows the hub.
- Selecting a machine stores it in localStorage and stays on `home` (not terminal).
- `#terminal` (legacy) maps to `device/terminal`; `#audit` maps to `activity`; `#run/<device>/<id>`
  selects the device and opens `device/runs` with the run focus; `#files` etc. map to `device/<tab>`.
- The rail: 200 px labelled at ≥768, 56 px icons between 640 and 767, bottom tabs below 640. Five
  items. The current item is ink with a 3 px inset bar, never an accent fill (the page's primary
  action owns the accent). The account menu at the foot holds version, docs, sign out. No status
  strip.
- The header is one control — `[● workshop · Running · v0.3.246 ▾]` — whose menu lists All machines,
  each machine, and Add a machine. No second `.machine-head` row and no `<select>` beside it.
- One `h1` per page: **Machines** for Home's fleet zero state, **Home** for the hub state, then the destination name. The machine's name is part of the header control, not an h1.
- One accent fill per screen. On the fleet, the whole row opens the machine and `Add a machine` is
  the only accent; the row's overflow holds Rename / Remove / Copy diagnostics.
- Keep the existing sign-in screen and the error banner behavior.
- Do not change the data layer beyond adding `selected` persistence.

**Steps:**
- [ ] Update `surface-link.test.ts` for the new fragments and the legacy aliases; run it and see it
      fail; implement; run `npm test`.
- [ ] `npm run typecheck && npm test && npm run build`; screenshots at 1280/390; commit:
      `feat(console-web): the hub's five destinations and one device workspace`.

## Task C2: `Sheet` and the mobile primitives

**Files:** Create `device/console/web/src/Sheet.tsx`, `Sheet.test.tsx`; modify `app.css`; migrate
`Modal.tsx` consumers where trivial (do not break `Modal.test.tsx`).

**Behaviour:** right drawer ≥800 px, bottom sheet below; focus trap; Escape and backdrop close
(disabled while `busy`); focus returns to the trigger; `aria-modal`, labelled by its title; body
scroll lock; `prefers-reduced-motion` respected.

**Steps:** failing test (Tab cycles, Escape calls onClose, focus returns); implement; commit:
`feat(console-web): one sheet for every focused job`.

## Task C3: Home

**Files:** Create `HomePage.tsx`, `HomePage.test.tsx`, `useLiveTools.ts`, `useLiveTools.test.ts`;
modify `App.tsx`, `app.css`, `DeviceCard.tsx` (Open/Connect copy and styling), `api.ts` (the live
tools call and its types).

**Behaviour:** spec §6.1, in its lean form. The fleet zero state is rows (whole row opens the
machine; overflow holds Rename / Remove / Copy diagnostics), with `Add a machine` as the only accent.
The hub state is one status line, the endpoint with Copy, one accent (Connect a client), one counts
line to Capabilities, the last three audit rows, and at most one notice (offline, or a catalogue the
console could not read). No tile grid, no second header, no second Connect button, no version-drift
banner on Home. `plugins.list` supplies the plugin count through the existing operator MCP helper;
if the tool is absent, the count is 0 and there is no plugin notice. Offline: the endpoint is dimmed,
Copy is disabled, actions say why; no fake counts. `useLiveTools` caches per device for 30 s, returns
`{tools, stale, error}`, and a stale catalogue is labelled.

**Steps:** tests for counts, empty calls, offline, stale catalogue; implement; screenshots; commit:
`feat(console-web): Home states the hub and the next step`.

## Task C4: Capabilities + Try it

**Files:** Create `CapabilitiesPage.tsx`, `CapabilitiesPage.test.tsx`, `capabilities.ts`,
`capabilities.test.ts`, `ToolSheet.tsx`, `ToolSheet.test.tsx`; modify `App.tsx`, `app.css`, `api.ts`.

**Behaviour:** spec §6.2. `capabilities.ts` is pure: `groupByCapability(tools)`, `filterTools`,
`summariseSchema`, `exampleArguments`, `formatResult`, `initialForm(schema)`, `validateForm(schema,
values)`. The form is generated from `inputSchema` (flat object; JSON fallback per field and a raw
toggle) with the states the spec fixes: blank optional = omitted, blank required = inline error and
focus, booleans are `Not sent / true / false`, a nested property forces Raw JSON, the toggle refuses
to switch back on invalid JSON, Run is pinned in the footer, results are capped with Show all/Copy,
and a two-minute timeout says the call may still be running. Calls go through the existing `callTool`
with the operator grant. A destructive tool confirms inline with the exact JSON and `Run once`. The
Activity row appears because the call goes through `/mcp`. No source segmented filter; the group line
summarises destruction.

**Steps:** pure tests first (grouping/filter/schema/example/validate/raw); then the page test (a fake
tool call, required-error, destructive confirm, timeout copy); implement; commit:
`feat(console-web): every capability can be seen and tried`.

## Task C5: Plugins

**Files:** Create `PluginsPage.tsx`, `PluginsPage.test.tsx`; modify `App.tsx`, `app.css`, `api.ts`
(plugin tool calls through the operator MCP helper).

**Behaviour:** spec §6.3. `plugins.list` drives the page. The add sheet's path blur calls
`plugins.inspect` (read-only) and shows the manifest summary (name, version, privileges — no tool
count before start) or the failure before `Add and run` enables; the trust sentence leads with the
consequence. Row overflow (Logs, Restart, Remove with confirmation); a failed row shows its reason
and a visible Restart. Logs sheet loads on open, scrolled to the last line on failure, Refresh only
while Starting. States: empty (no manifest example), loading, failed, offline. Old build: if the live
catalogue has no `plugins.*`, show the "newer Summrise" sentence and no Add. Add a
`usePlugins(deviceId)` hook in the page file.

**Steps:** tests for empty, old build, invalid path, add error, remove confirm; implement; commit:
`feat(console-web): plugins are installed, watched and removed in one place`.

## Task C6: Connect

**Files:** Create `ConnectSheet.tsx`, `ConnectSheet.test.tsx`; adapt `ConnectCard.tsx`,
`FleetTokens.tsx` (keep their tests) into the sheet's Tokens section; modify `App.tsx`, `app.css`,
`api.ts`.

**Behaviour:** spec §6.5. Endpoint/Clients/Tokens segmented sections. Tokens use the live catalogue
for scope; presets All/Read-only; destructive consent; the created value shown once and dropped on
close. Client blocks for DSH/Claude/Codex/Cursor with `<token>` placeholder. Escape closes.
`aria-label`s and focus order.

**Steps:** tests for section switching, token shown once, close clears; implement; commit:
`feat(console-web): Connect is a sheet, not a page`.

## Task C7: Device tabs and Activity

**Files:** Modify `App.tsx`, `app.css`, `TerminalWorkspace.tsx`, `ScreenPage.tsx`, `FilesPage.tsx`,
`RunsPage.tsx`, `SystemPage.tsx`, `AuditTable.tsx`; create `DevicePageShell.tsx` if useful.

**Behaviour:** the five existing pages render under `Device` with a tab bar; no per-page device
picker; one h1 (the tab name); existing actions preserved. Desktop puts the tab bar in the page head,
not a third band; phone uses a 40 px scrollable segmented row. The terminal becomes one frame
(panel padding 0, canvas edge to edge, dark canvas one step below the page); a live PTY close asks
`Close this session? The process keeps running on the machine.` Files' limits move into a `Limits`
disclosure and Runs' help becomes field help; all five `pickHint` paragraphs are deleted. Activity
gets the new shell and the spec §6.4 fixes: no nested max-height (the row opens and the page scrolls),
the whole row toggles, the device column shows the machine's name, no `KIND` column and no line
caption, sentence-case headers, relative time with the exact date in `title`, day grouping, Export CSV
in the overflow, and the subtitle deleted. Run deep links still open Runs.

Delete the dead tree in the same commit after `grep` proves nothing imports it: `MemoryPage`,
`MonitorsPage`, `EventsPage`, `SecurityPage`, `MonitorChip`, `NotificationsCard`, `VitalsChart`, their
tests, `DevicePicker`/`ToolPicker` if superseded, the unused rail tokens (`--rail`, `--rail-line`,
`--rail-ink`, `--rail-ink-hover`, `--rail-focus`, `--tracking-micro`) and any imports/types only they
used. A dead page is a second product; a dead token is a second vocabulary.

**Steps:** tests for tab routing, the run deep link, the close-confirmation copy and the Activity
record layout; `npm run typecheck && npm test && npm run build`; screenshots; commit:
`feat(console-web): one device workspace, one record`.

## Task C8: Screenshots, accessibility and copy

**Files:** `device/console/web/scripts/screenshot.mjs` (new states), `app.css`, page files.

**Behaviour:** the gallery covers **360, 390, 800 and 1280 × light and dark for all five
destinations**, plus the states: Home fleet / hub / offline / stale catalogue; Capabilities list and
empty search; tool sheet idle / running / error / destructive; Plugins empty / failed / old build;
Connect Endpoint and Tokens; Device terminal and Activity record. Fix focus order, `aria-current`,
skip link, `input/select/textarea:focus-visible`, hover/press per spec §7, contrast if the token test
complains. Delete the status-strip strings. Read every new sentence against the code. Run
`scripts/build-web.sh --check` and `node scripts/layout-gate.mjs` on the rendered pages where
applicable.

**Commit:** `feat(console-web): the hub's screens, measured`.

---

# Workstream D — Example and docs

## Task D1: The reference plugin

**Files:** Create `examples/plugins/hello/plugin.json`, `examples/plugins/hello/plugin.ps1`,
`examples/plugins/hello/README.md`.

**Behaviour:** a PowerShell MCP stdio server with `hello.greet` (`{name?}` → text) and `hello.echo`
(`{text}` → text), annotations read-only, newline JSON-RPC, `initialize`/`tools/list`/`tools/call`/
`ping`, exits on stdin EOF. It must be readable as the reference: 80 lines, comments where the
protocol is not obvious. No dependencies.

**Steps:** run it locally by piping the four JSON lines (document the command); commit:
`feat(examples): a reference plugin, two tools and no dependencies`.

## Task D2: The documents tell the truth

**Files:** `docs/PRODUCT.md` (§ one sentence, pillars, nouns, surfaces, not-list, principles),
`README.md` (the hub first, build/test map), `docs/QUICKSTART.md` (install → enrol → connect → first
call → add a plugin), `docs/CONNECT-MCP.md` (the endpoint is the same; add plugin tools and the live
catalogue), `docs/PLUGINS.md` (new: manifest, protocol, trust, lifecycle, errors, example),
`docs/design/capability-hub-2026-11-02.md` (fix only if implementation proved a sentence wrong),
`docs/console-deploy.md` (the new version and measurements), `AGENTS.md` if a rule moved.

**Rule:** every claim is checked against the code by the writer. No promised CLI, no git/npm source,
no sandbox. The five destinations and the trust sentence are the spine. The copy deck in the spec is
the source for UI strings; the docs may quote it.

**Commit:** `docs: the hub, its five destinations and the plugin contract`.

---

# Workstream E — Verification and deploy

## Task E1: The full gate

```bash
cd /home/zss/summrise-product
source ~/.cargo/env
cargo fmt --manifest-path device/Cargo.toml --all -- --check
cargo clippy --manifest-path device/Cargo.toml -p summrise-core -p summrise-device -p summrise-launch \
  -p summrise-mesh -p summrise-release -p summrise-console -p summrise-winsec -p summrise-winsys \
  -p summrise-shell --all-targets -- -D warnings
cargo test --manifest-path device/Cargo.toml -p summrise-core -p summrise-device -p summrise-console
cd device/console/web && npm run typecheck && npm test && npm run build
cd ../../.. && scripts/build-web.sh --check
node scripts/layout-gate.mjs <rendered-pages> 360 390 800 1280
```

Read the exit code, not a grep. Each task's tests must be green before E1; E1 is the whole tree.

## Task E2: Review the diff

A fresh reviewer reads `git diff origin/main...hub/v1` against the spec: five destinations, no dead
page, the plugin path, the trust sentence, the live catalogue, the old-build degradation, the copy.
Findings are fixed before deploy. The review records what it deliberately leaves.

## Task E3: Deploy and verify

1. Build and stage the console (`scripts/stage-console-deploy.bash <version> --mirror-dir
   /tmp/canary-mirror`) from a clean tree containing `origin/main`.
2. On the console host: fetch and run the deploy `.cmd`; verify `/health` version, served page size
   equals `device/console/web/dist/index.html`, `/?v=<n>` stays on the React page; run
   `scripts/real-device-check.ps1` → `REAL_TERMINAL_OK`.
   Then stage the example plugin on the device host at `C:\SummrisePlugin\hello` and run
   `scripts/real-plugin-check.ps1` → `REAL_PLUGIN_OK` (it adds, lists, calls `hello.greet`, reads the
   log, removes, and proves the tool disappears; it reads the operator token without printing it).
3. Device half: only with a signed release document, or a supervised swap with a backup. Add the
   example plugin through the console on a machine running the new device build; `hello.greet`
   appears in Capabilities, Try it returns its answer, Activity shows the row, Remove makes it
   disappear, and the process is gone. If the device build cannot be deployed safely, record the
   deferral and prove acceptance 12 on an old build instead.
4. Update `docs/console-deploy.md` with the measured facts and the rollback binary. Keep the last two
   backups.
