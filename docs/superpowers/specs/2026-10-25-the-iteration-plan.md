# The iteration plan: from a console that works to the workspace the operator used

> **Status: plan, 2026-10-25.** Inputs: the tool-surface audit
> (`2026-10-24-the-tool-surface-parity.md`, 56 legacy tools → 36 new), the legacy panel's inventory
> (`summrise-legacy: agent/resources/panel-react/src/components`, 52 files), the two-runtime
> architecture, and the first week of canary work. The plan is ordered by what an operator loses
> first, not by what is cheapest to build.

## 0. Where this plan stands now (2026-10-28)

Written when the plan was new; kept because a plan that is silently rewritten cannot be judged.

- **Fleet**: three devices on the console (two online, one offline since an installer run of 0.3.130).
  Both online machines run 0.3.149 with the launcher and shell the installer now ships, and the console
  runs 0.3.144 with its stderr going to `console.log` (a 504 is diagnosable now; before, its own two
  diagnostic lines went to a handle nobody could read).
- **The plan's §2 workspace gaps are closed**: terminal (sessions, presets, attach, resize), files,
  memory, monitors, runs, audit, approvals, tokens, channels — each with its own page and its own
  handler, and the UI they share lives in `device/ui` (tokens, chips, cards, the page shell).
- **The runtime is proven in every install shape**: replace (twice), seed (once, from an emptied
  directory), uninstall, the repair task after a deleted service, and the canary's own device update.
- **Enrollment is proven on both sides.** The console's half: a one-time code (`POST /enroll`), a fresh
  Ed25519 key signing `register\n{id}\n{code}\n{ts_ms}`, `POST /devices` accepted it (200, id = public
  key), the fleet showed it offline, `DELETE /fleet/:id` removed it. The device's half, on a scratch data
  directory against the real console: `summrise-device --enroll http://127.0.0.1:19000 <code> --data …
  --allow-plaintext --force` printed `enrolled 1deb3887…`, wrote `console.json`, `console-token.json` and
  `identity.key`, and the id it printed was in the fleet (removed afterwards, 204).
- **What is NOT proven**: the session-reconnect path end to end (the hand-off is measured, the
  Windows-side trigger is not), and DESKTOP-CPH4KB2's
  recovery — that machine has been offline since an install left it with no runtime, and the
  installer/repair discipline above is the answer it is waiting for.

## 0b. What 2026-10-27/28 delivered

The two days' work, in the shape a reader can check against `git log` (144 commits, all under
`device/`):

- **The runtime's install discipline**: every shape exercised on real machines — replace, seed (from an
  emptied directory), uninstall, the repair task after a deleted service, and the canary's own device
  update. The installer now kills the app before rotating the baseline, registers a repair task, and
  runs its own log under a name that does not collide with the launcher's.
- **The launcher's five crash bugs** (`--apply` indexing, the swap wait's clock, `stop_started`, the
  rollback message, the boot check's key set) and the release-signed generation it now accepts.
- **The shell**: menu ids that match their handler, one WebView2 profile (sign in once), a minimized
  window that comes back, a wake hand-off so a reconnection can re-add the tray, a reconnect task that
  repairs its own action, and no visible console window for its `schtasks` children.
- **The console**: a shared `summrise-ui` crate (tokens, chips, cards, the page shell) so the device's
  panel and the console cannot drift; 28 pages rendered and compared on every UI change; a ninety-second
  proxy bound under Cloudflare's ceiling; its own stderr in a file; sessions that say what happened while
  they were gone; CPU share and monitor state duration on the pages.
- **The device**: six capability fixes from a review (write-through-rename, bounded capture join,
  `net_test` off the reactor, bounded history reads, pty reaping, bounded cast table), and a Windows test
  target that compiles for the first time because three tests were gated to Unix.
- **The measures**: enrollment proven on both sides (console protocol + the device's own CLI), a
  ninety-second proxy answer timed end to end, and the 504 rate after the fixes (1 in 78, attributable to
  a deploy window; before, 4 in 6).

## 0a. What is left, and how to start

Four operator-visible gaps against the legacy panel remain, in the order they were judged to hurt:

1. **Session replay** — legacy could reopen a finished session's command-by-command trail with output and exit
   codes; the device keeps `runs.jsonl` rows without output and the console says so. This needs a device-side
   decision (what to retain, for how long) before any page.
2. **Settings** — buffer size, start-on-logon, memory caps, gateway URL: a device tool plus a console page.
   The registry has no settings tools, so this is a new capability, not a page.
3. **Update check/apply** — the device reports `current`/`previous`/`pending` and the launcher owns the swap;
   nothing asks it to check a channel or to apply, and the console has no route for either.
4. **Remote workspaces** — the legacy panel could browse and edit another machine over SSH; the files page
   here is the device's own `files_root`.

Two things need a human, not code: DESKTOP-CPH4KB2's recovery (one elevated `recover.cmd` on that machine), and
one real RDP disconnect to prove the reconnect trigger fires (the hand-off itself is measured; the tasks'
"last run" is still 1999).

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
| P6 restarts | **met, observed** | the device journals each boot (when, from which release and digest, pid) in `<data>/restarts.jsonl`, bounded at 50 records, and `host.facts` carries the last ten; the Vitals page's Install card lists them under **Restarts**, beside the launcher's swap outcome and repairs. Observed on canary 0.3.128 (2026-10-28): after the update both DESKTOP-9VH92BJ and DESKTOP-14RJCR8 answer `/api/health` 0.3.128 and their Vitals pages render the card with the boot that the upgrade wrote (`{"at_ms":1793156311919,"version":"0.3.128","pid":14824}` on 14RJCR8); a device still on an older runtime renders no card, which is the honest answer |
| P6 browser live view | **met, observed** | `browser.cast` is a real CDP `Page.startScreencast` stream: a reader per tab, newest frame returned, every frame acked, idle stop after 60 s, closed with the tab. The pane polls at 5 fps while a stream serves it and falls back to 1 fps screenshots on a device without the tool. Observed on canary 0.3.124 (9vh92bj, 2026-10-28): the frame route answered `200 format=jpeg stream=true 764×485` in 27 ms; ten polls 200 ms apart while scrolling returned **ten distinct frames** at 9–13 ms each, and the same ten polls on a still page returned **one** frame — the stream serves the newest paint instead of forcing captures |

### The fleet at the end of the session (2026-10-28)

Three canaries, one runtime each, the legacy desktop/agent retired from all three (see RELEASE.md):

| device | runtime | tray | notes |
|---|---|---|---|
| DESKTOP-9VH92BJ | 0.3.130, online | running (session 2, user 13122) | also runs the console at `C:\SummriseConsole` and the canary mirror |
| DESKTOP-14RJCR8 | 0.3.130, online | running | its console link is a tailnet address, which is why the plaintext rule had to learn that private links are private |
| DESKTOP-CPH4KB2 | 0.3.129, **offline** | not running | installer 0.3.130 removed its service and then failed; the recovery is in RELEASE.md and needs one command at the machine |

**Second install, DESKTOP-9VH92BJ, 0.3.141:** hash `a121f922…` verified before it ran, `install exit 0`,
launcher `61c0931e…` and shell `bdb026fb…` installed, `repair.cmd` now writes `repair-task.log` (the rename
reaches a machine for the first time), `SummriseRepair` registered, device answered `0.3.141` with a new pid
nine seconds after the task started, dead-man deleted. Both installs were driven from a scheduled task with
a hash check, so a partial download cannot reach NSIS again.

**The installer was first exercised end to end (2026-10-28, DESKTOP-14RJCR8, 0.3.139):** silent,
hash-verified, `install exit 0`; the launcher was replaced (`7b50ac60…` → `61c0931e…`), the tray binary
(`bdb026fb…`), `repair.cmd` and the `SummriseRepair` task were installed, the baseline was rotated by the
new staged path (`replaced baseline/current from the installer's pair`), and the device answered
`/api/health` with a new pid. That machine is the first in the fleet running the fixed launcher. One
known gap: the installer it used predates the log rename, so its `repair.cmd` still appends to
`repair.log`; the next install carries `repair-task.log`.

The installer is the only stream that had not been exercised end to end before that attempt, and the
attempt is what found the ordering bug (service removed before the replacement was ready) and the
missing step (the app was not killed before its image was rotated). Since 0.3.131 the registration is
kept and the app is killed first; since 0.3.132 a boot-time `SummriseRepair` task re-registers a
missing service. Neither has run on a machine yet: CPH4KB2 is the next test, and the other two are
deliberately not touched until it passes.

### The inbound authorization findings (2026-10-28)

A read-only review of the device's inbound path and of the console's credential core found nine
defects; all nine are fixed, tested, and on the canary (0.3.127). The two that mattered most:

- **A mesh peer could authorize itself.** A token scope (`{"token":{...}}`) carries no signature —
  the console's authenticated channel is what stands behind it — and `admit` accepted one on every
  wire. Any joined peer could send `scopes:["*"]`, `allow_destructive:true` and run anything here.
  `admit` now takes the transport, and a scope is admitted only over the console channel.
- **A captured frame ran twice.** Grants had no one-shot property: the same signed frame replayed
  into the device ran again, and the audit showed two ordinary `allow` lines. A grant now carries a
  signed nonce; the device keeps the ones it admitted (pruned by the grant's expiry + skew, capped)
  and refuses a repeat with `error_kind:"replay"`. A grant without a nonce (an older console) is
  still admitted, so a rollout cannot strand a live console.

The rest: plaintext `ws://` to a public host needs `"allow_plaintext": true` (private links — LAN,
tailnet, ULA — do not, which a rollback taught us: refusing the tailnet address stopped the app from
starting on DESKTOP-14RJCR8 and the launcher reverted to the previous generation by itself), the
device's audit lines name the token that authorized a call, panel terminal operations are audited as
their own transport, logout persists before adopting, removing a device revokes the aggregate tokens
that list it, and an approved call re-reads the token before it is re-sent.

Two decisions made during those rounds belong beside the phases:

- **The pages stay static except two scripts** (`assets/approvals.js` badge poll, `assets/browser.js`
  frame pump). A held call waits 90 s, and no static page can report a window that short.
- **A held call cannot outlive its request**: a `Held` guard removes the approval row when the
  holding future drops, and the client is told the outcome is unknown (504) rather than "needs
  approval" when the re-send fails — a retry after that answer could run the command twice.
