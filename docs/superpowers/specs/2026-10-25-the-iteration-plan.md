# The iteration plan: from a console that works to the workspace the operator used

> **Status: plan, 2026-10-25.** Inputs: the tool-surface audit
> (`2026-10-24-the-tool-surface-parity.md`, 56 legacy tools → 36 new), the legacy panel's inventory
> (`summrise-legacy: agent/resources/panel-react/src/components`, 52 files), the two-runtime
> architecture, and the first week of canary work. The plan is ordered by what an operator loses
> first, not by what is cheapest to build.

## 1. Where the project stands

Proven on the canary (device 0.3.29, console 0.3.32, helper in the user's session):

- **The runtime**: `summrise-launch` (SCM service, sole writer of the app binary) supervises
  `summrise-device` (38 tools in 10 capabilities); the console is the only control plane and every
  action is a signed grant plus an audit row.
- **The face**: a web console with five pages (Devices, Terminal, Browser, Memory, Audit) that an operator
  reaches through Cloudflare Access or the operator token; the device's own panel at
  `127.0.0.1:18090` for the person standing at the machine.
- **The presence**: `summrise-shell` in the user's session — a tray icon that promotes itself into the
  visible area, a window on the console page, and two verbs the service cannot do: launch Chrome in
  the user's session, and capture that session's screen (a locked or disconnected session is refused
  with a sentence, not a black frame).
- **The facts**: the device reports its host on the channel — OS build, uptime, memory, volumes, and
  the Windows sessions with their state — and the console renders them on the device card.

What is missing is not a runtime capability. It is the **workspace** the legacy panel gave the
operator: several terminal sessions at once, their saved connections, a memory browser, monitors,
runs, approvals and update state — the things an operator touches every hour.

## 2. What the legacy panel had

The legacy app was an Electron shell around 52 React components. They group into nine areas:

| area | legacy components | what it gave the operator |
|---|---|---|
| Terminal workspace | `TerminalWorkspace`, `TabBar`, `ConnModal`, `ConnectCard`, `SessionControl`, `IdleSessionsBar`, `RunStrip`, `GoalBar` | many sessions, tabs, saved SSH/serial presets, per-session control |
| Browser | `BrowserPage`, `EmbeddedBrowserPane`, `BrowserSessions` | a live embedded browser, tabs, native input |
| Memory | `MemoryPage`, `PathView` | browse, search, edit and delete remembered entries |
| Monitors | `MonitorsCard`, `MonitorAlerts`, `MonitorChip` | add/list targets, see status and alerts |
| Runs & history | `HistoryPage`, `ActivityPage`, `ArchivePage`, `TrajectoryView`, `EvidenceDrawer` | what ran, when, with what result, and the evidence |
| Approvals | `ApprovalGate`, `WaitingChip` | approve or refuse a tool the device wants to run |
| Updates & restarts | `UpdateCard`, `RestartHistoryCard`, `BootChip` | update state, restart history, boot marker |
| Device vitals | `DeviceHealthCard`, `DeviceLogsCard`, `VitalsDial`, `Sparkline`, `LoadChip` | metrics over time, logs, load |
| Settings / workspaces / plugins | `SettingsPage`, `WorkspacesPanel`, `PluginsPage`, `DshPage` | device settings, workspaces, plugin list, DSH panes |

The last row is the part that does **not** transfer: those surfaces were the legacy agent's own chat
and workspace UI, not device capabilities.

## 3. The gaps, one row at a time

| area | legacy | now | decision |
|---|---|---|---|
| Terminal workspace | tabs, saved connections, session control | one session per grant, no tabs, no presets | **build** (P1) |
| Browser | live embedded view, native input | screenshot polling over CDP | **build** (P6: screencast) |
| Memory | browse/edit/delete | ids, `memory.update`/`memory.delete`, the Memory page | **done** (P2) |
| Monitors | card, alerts, status | five tools, no view | **build** (P3) |
| Runs & history | history, activity, archive, trajectories | `runs.*`, the audit table | **build** (P4) |
| Approvals | approve/refuse per call | grants only; the device's Ask rule has no console surface | **build** (P5) |
| Updates & restarts | update card, restart history | version in `/health`, release notes in the runbook | **build** (P6) |
| Vitals | sparks over time | one snapshot on the device card | **defer** — a snapshot answers "what is it", history answers "is it degrading"; do it after the workspace |
| Settings / workspaces / plugins / DSH | device settings, plugins, chat | out of scope by the two-runtime decision | **do not port** |

## 4. Principles every phase keeps

1. **One updater.** No phase introduces a second way to write the app binary.
2. **One audited path.** The console page is the operator surface; a tool call is a grant plus an
   audit row. Nothing bypasses that to save a click.
3. **The helper is the only user-session surface.** Screen and browser live there; the service is
   session 0.
4. **Degrade, never break.** No helper → the service's own browser; no console → the device's panel.
5. **Evidence over prose.** Each phase lands with tests and a canary observation in its commit.
6. **The console is not a terminal emulator with extra steps.** Panes are built from the tools that
   exist; a new tool is a decision recorded in the registry first.

## 5. The phases

### P1 — The terminal workspace (the largest loss)

**Goal**: the operator opens Terminal and works, instead of opening a terminal.

- **Tabs**: several live sessions at once, each with its own xterm instance; a tab survives page
  navigation within the console session (state in the console page, not in the URL).
- **Saved connections**: named presets for `kind: ssh|serial|pty` with host/port/user/baud; stored in
  the console's registry per device (they are operator convenience, not device state), listed in the
  connect dialog.
- **Session control**: close, resize, and a visible idle/created-at line per session; "New" opens the
  dialog with the last-used preset selected.
- **Acceptance**: a console test for the connections registry; a canary session with two tabs, one
  SSH to the device itself and one PTY, both surviving a tab switch; the audit shows one
  `terminal.create` per tab.

### P2 — Memory, and the two tools nobody chose — **done 2026-10-25**

Landed as `memory.update` / `memory.delete` with entry ids and a journal store, and the console's Memory
page (`memory.jsonl` replay, compaction, 38 tools / 10 capabilities). Evidence:
`2026-10-25-memory-entries-get-ids-design.md` § "Verified on the canary".

**Goal**: the memory store is browsable, and an entry can be corrected or retired.

- `memory.update` and `memory.delete` in the registry (the audit's two undecided tools), with ids that
  survive appends: a stable id per entry, and a tombstones file rather than rewriting history.
- A Memory page: search, list with tags, edit, soft-delete; the page reads through the same grant the
  terminal uses.
- **Acceptance**: registry and wire tests for the two tools; a device test that update/delete round
  trips and that a deleted entry is not returned by search or export.

### P3 — Monitors — **done 2026-10-25**

The console has a Monitors page (add, status, probe now, remove, 15 s polling) over the existing five
tools; the canary flow is in `2026-10-25-monitors-page.md` § Task 2.

**Goal**: an operator sees the reachability they configured.

- A Monitors card on the device page (or its own page): `monitor.list` + `monitor.status`, refreshed;
  add/remove in a dialog; a down target rendered as a fact with its last error.
- **Acceptance**: the existing monitor tools are unchanged; the card shows a target that was added
  through it, and a target that is down shows `ok: false` with its latency gap.

### P4 — Runs and history

**Goal**: "what ran and how did it end" without reading JSONL.

- A Runs page: `runs.list` with duration and exit status, `runs.get` for the output; the audit table
  stays for authorization, not execution.
- **Acceptance**: a run started from the console appears in the list with its exit code; the page
  refuses to render output larger than the tool's cap.

### P5 — Approvals

**Goal**: a tool the device would answer `Ask` for reaches a human, not a timeout.

- The device already has the authorization decision (`authorize`); the console page gains an
  approvals card that lists pending asks for the selected device and answers them with a signed
  decision.
- **Acceptance**: a destructive call with an Ask rule waits, appears in the card, is approved, and the
  call completes; refusing it leaves an audit row that says who refused.

### P6 — Updates, restarts, and the browser's live view

**Goal**: the two remaining pieces of the legacy feel.

- **Updates**: the device's update state (staged release, last attempt, version) on the device card,
  from the facts the launcher already owns.
- **Restarts**: a restart history from the state the launcher writes.
- **Browser**: screencast — CDP `Page.startScreencast` frames forwarded to the console page, with a
  fallback to the screenshot polling that exists.
- **Acceptance**: an update staged from the console shows as "staged" before the swap and "running
  <version>" after; a screencast at 5 fps is visible while a page scrolls.

## 6. Ordering, and why

P1 first because it is the largest daily loss, and because its shape (tabs + a console-side store)
is the pattern the other pages reuse. P2 next because it closes the only two tools the audit left
undecided, which is a hole in the surface, not a preference. P3 and P4 are cheap — the tools exist
and the pages are read-mostly — so they land quickly after. P5 is a security surface, deliberately
after the read paths it will guard. P6 is the largest single build (screencast) and the one whose
absence the operator feels least; updates and restarts ride along because they are the same card.

## 7. What this plan does not do

- It does not resurrect the Electron shell, the embedded Chromium, or the chat surface.
- It does not port `page_view`, the MCP-client tools, plugins, or DSH panes: those were the legacy
  agent's own UI, and the console is the new agent's face.
- It does not invent a second update channel for any page.

## 8. Cadence

One phase per iteration, each with: the tool tests, the console page, the canary evidence, and the
spec updated in the same commit series. A phase that cannot be observed on the canary is not done.

## 9. Status (2026-10-27)

Written after the console rounds that took P1–P6 from "planned" to "shipped, with two deviations
from the acceptance text above". The deviations are the point of this section.

| Phase | State | What differs from §5 |
|---|---|---|
| P1 terminal, P2 tools, P3 monitors, P4 runs | shipped | as specified; `runs.get` opens a history row (rows, not output — the device keeps no output) |
| P5 approvals | shipped | the console HOLDS the client's call and re-sends it with consent for that call only, instead of signing a decision; a token keeps saying no to the next call. The deadline is **90 s**, not the legacy 15 min: the console is behind Cloudflare Access, whose origin bound cuts a request at about 100 s, so a longer hold would answer a request nobody holds. Expiry fails closed; the badge on every page reports a held call |
| P6 updates | shipped | the Install card on Vitals (generations, last swap with its reason, repairs), from `host.facts`; the console keeps no copy |
| P6 restarts | partial | the launcher's swap outcome and repairs are shown; a per-restart history is still not exposed by any tool |
| P6 browser live view | **built, not yet deployed** | `browser.cast` (device) is a real CDP `Page.startScreencast` stream: a reader per tab, newest frame returned, every frame acked, idle stop after 60 s, closed with the tab. The console asks for it and falls back to `browser.screenshot` when the device predates the tool, which the deployed runtime does today. The polling cadence is still the console's (1 fps), so the 5 fps scrolling acceptance is a cadence change, not a missing build |

Two decisions made during those rounds belong beside the phases:

- **The pages stay static except two scripts** (`assets/approvals.js` badge poll, `assets/browser.js`
  frame pump). A held call waits 90 s, and no static page can report a window that short.
- **A held call cannot outlive its request**: a `Held` guard removes the approval row when the
  holding future drops, and the client is told the outcome is unknown (504) rather than "needs
  approval" when the re-send fails — a retry after that answer could run the command twice.
