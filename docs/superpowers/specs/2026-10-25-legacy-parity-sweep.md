# Legacy panel → console: the parity gap sweep

> **Status: audit, 2026-10-25.** Legacy: `summrise-legacy: agent/resources/panel-react/src/components`
> (50 components + tests). Console: `device/console/web/src` (10 pages) over `device/core/src/registry.rs`
> (38 tools, 10 capabilities). Method: every legacy component's name and core interaction, then each
> against the console page that would own it. Not every line, and no code changed.
>
> The 2026-10-25 iteration plan's "now" column is stale: terminal tabs and presets, monitors, runs and
> memory edit/delete have shipped. At the time of the audit, P1 still lacked the connect dialog and the
> promise that a tab survives page navigation; items 1 and 2 below closed both.

> **Closed since, and what closed it.** Items 1–3 landed on 2026-10-25; items 4–9 landed together on
> 2026-10-26, each with tests, and each item below names its commit. The same batch added the console
> a CI job (`console`: typecheck, vitest, and a diff of the rebuilt `dist/index.html`), because the
> page had no gate at all — a stale bundle compiled and shipped.

## Verdict

The console covers the transferable half: fleet, enrollment, grants, terminal tabs/attach/presets,
files, memory browse/edit/delete, monitors, runs, processes/net, browser screenshot/click/type, audit,
host facts.

What is left is not "pages". It is one broken path (SSH cannot be opened at all), state that dies on
navigation, silence when a channel or grant fails, a browser that can only open tabs, a record too thin
to answer "what happened on this device", and a handful of tools with no console surface.

Most other legacy components are Electron-local chrome or the old AI-session workspace (goals, plans,
per-command approval, trajectories). The new runtime deliberately dropped that model; porting those
views without the data would be theater, and restoring the data is a new device protocol.

## Worth doing now

### 1. The console cannot open an SSH session — M — closed 2026-10-25

Legacy `ConnModal` + `ContextRail`'s new-session menu + `TerminalWorkspace` opened pty/ssh/serial, with
a password or a private-key path, and saved presets prefilled the form.

Console `TerminalWorkspace` has tabs, attach and presets, but `+ New` is pty-only, and a saved SSH
preset calls `terminal.create` with no `auth` — which the device requires (`device/src/mcp.rs:735`).
The preset dialog's own copy says "type it when the session opens", and there is no field to type it
into. Key auth (`ssh::Auth::Key`) is unreachable; serial `parity`/`data_bits`/`stop_bits` are accepted
by the tool and absent from the form. Serial presets do work; SSH presets always fail.

Close with: console only, existing `terminal.create` — one New-connection dialog (kind, host/user/port,
key path or password, serial framing), credentials per open and never stored. M.

**Closed (`f7108be`).** `+ New` opens pty, ssh and serial; the dialog sends each kind only its own
fields, the SSH form takes a password **or** a key path, serial gets its framing, and the credential
lives in the form's state — never in a preset, a URL or storage.

### 2. Tabs do not survive a page change — M — closed 2026-10-25

Legacy `TerminalWorkspace` + `TabBar` kept every session's tab and cursor; the operator could open
Settings and come back.

Console `App` renders `<TerminalWorkspace>` only while `page === "terminal"`, so any rail click
unmounts it; returning shows an empty page and the operator must "Attach" each live session again. The
grant survives; the tabs do not. The iteration plan's P1 acceptance explicitly requires a tab to
survive page navigation; it does not.

Close with: console only — lift `tabs`/`active` to `App` (or keep the page mounted), re-attach by
`terminal.list` on mount. M.

**Closed (`8829219`).** The workspace is rendered whichever page is showing and hidden rather than
unmounted; a rail click no longer closes its tabs.

### 3. A failed terminal is silent — S–M — closed 2026-10-25

Legacy `TerminalWorkspace` showed "Connection lost — reconnecting…" when the stream dropped;
`EvictedNotice` said when the device took a session away (cap or idle TTL).

Console `poll()` catches and discards every `terminal.read` failure, and `write` failures are swallowed
(`TerminalWorkspace.tsx:73,97`). A device that goes offline, a grant that expires (default 3600 s) and
the device's 30-minute idle close all look identical: a frozen terminal with no sentence.

Close with: console only — per-tab error state, a grant expiry/renew line, and a `terminal.list` probe
on failure to tell "closed" from "unreachable". S–M.

**Closed (`ea98ea4`).** A terminal that stops answering says which failure it was, in the tab, instead
of freezing silently.

### 4. The browser can open a tab but never navigate or close one — S — closed 2026-10-26

Legacy `BrowserPage` had an address bar, back/forward/reload and a real view; `PluginsPage` could stop
the browser.

Console `BrowserPane` opens (always `browser.open`, a NEW tab), screenshots, clicks and types.
`browser.navigate`, `browser.close` and `browser.shutdown` are unexposed; at the device's 8-tab cap the
page is stuck, and its URL placeholder says "opens or navigates" while it only opens.

Close with: console + existing tools — navigate the selected target, close tab, stop browser. S.

**Closed (`b28132c`).** The pane has **Navigate**, **Close tab** and **Stop browser**, and **Open new
tab** says what it does; the placeholder no longer promises navigation the button did not perform.
Lists and screenshots each keep their own newest-answer rule. Six tests.

### 5. A render error blanks the console — S — closed 2026-10-26

Legacy `ErrorBoundary` wrapped the app; a page crash showed "Something broke" + Reload.

Console has no boundary anywhere; React 18 unmounts the root on a render throw. One bad page blanks the
whole control plane.

Close with: console only, one wrapper. S.

**Closed (`179ff00`).** `ErrorBoundary` wraps the root: a render throw now shows **Something broke**,
the thrown message and a **Reload**, instead of a blank tab. Two tests, including that a healthy tree
passes through untouched.

### 6. Audit cannot answer "what happened to this device" — M — closed 2026-10-26

Legacy `ArchivePage`/`ActivityPage` gave a bounded, paginated, filterable record with per-session trails.

Console `/audit` supports 1..1000 lines; `AuditTable` asks for 20 newest, with no device filter, no
paging and no detail. The index records the tool and outcome, not the call's arguments — so it can say
"terminal.write succeeded", not what was typed.

Close with: console only — filter by device/tool/status, limit or paging, expandable detail (the server
already supports the limit). A per-device activity view then becomes a filter, not a new store. M.

**Closed (`2c443a5`, caption precision `9188eda`).** The page loads 200 lines (the console caps at
1000), filters them by device, tool/detail substring and outcome, opens a line for its ISO timestamp and
full ids, and tells "nothing matches" apart from "nothing audited". Retired device ids stay selectable.
Seven pure-filter tests and four table tests; the caption states what was loaded, not what the console
might hold.

### 7. Memory cannot be exported — S — closed 2026-10-26

Legacy `MemoryPage` had Export (whole store as JSONL, copy) and "+ New".

Console lists/searches/edits/deletes, but has no export and no create, although `memory.export` and
`memory.append` exist.

Close with: console + existing tools — Export now (show as text, copy/download, state the 2 MiB cut);
New entry later. S.

**Closed (`56ac71c`).** **Export** shows the device's JSONL, states entries, bytes and whether the
2 MiB cap cut it, and offers **Copy** (the whole answer) and **Download** (`summrise-memory-<date>.jsonl`,
through a Blob, not a console route). A refusal opens no dialog. Six tests. **Memory create is still
open** and stays on the later list.

### 8. The device's screen cannot be looked at — S (not a legacy feature) — closed 2026-10-26

The per-user helper was built and `screen.capture` is in the registry, but no console page calls it. The
legacy panel had no remote screen; the current device does, and this is the one new operator capability
the console leaves on the floor.

Close with: console + existing `screen.capture` — a Screen pane (capture, timestamp, download; show the
helper's refusal sentence for locked/disconnected sessions). S.

**Closed (`cc05e6a`).** The rail gains **Screen**; the page captures on demand, shows the PNG with its
dimensions and moment, offers the file by name, and lands the device's refusal as a sentence. A failed
capture clears the previous frame. Four tests. The tenth destination also exposed a fixed-height rail:
at 52 px per button the column is ~617 px, so a short window would have clipped the last entries; the
rail scrolls now (`47c6c9f`).

### 9. The destructive-consent checkbox is a dead end — S — closed 2026-10-26

Legacy `ApprovalGate` armed a session and answered a waiting command.

Console's replacement is the grant's `allow_destructive`, but `GrantForm` offers "Allow destructive
tools" and nothing in the console can answer the `Ask` that follows when it is unticked — the call just
fails. One control promises a policy the product cannot apply.

Close with: decide now — remove the toggle (consent always granted, matching the default) or build the
approvals card (later, M). S to remove; M to build.

**Closed (`69ba5e8`) — the toggle stays, and the copy stops implying an approval flow.** A read-only
grant is a real policy, and the device's refusal already carries the fix: it says the grant did not
consent and asks for one minted with `allow_destructive=true`. The console's audit keeps the refused
exchange (`mcp`, HTTP 200, `jsonrpc error -32002`) and now marks it, because the HTTP status alone
read as success; the device's own audit is where the attempt is recorded as an ask. Unticking says
that under the checkbox. No approvals queue is built: it is a new protocol, not a port.

## Worth doing later

**Closed in the 2026-10-26 close-out.** Memory create (`a9c89c0`, one dialog that re-reads the filter
it was opened from); tab identity's local rename and unique generated labels (`d3e8b12`); transcript
export with the ring's lost bytes stated, and the idle notice in a closed-without-reason sentence
(`a4fc798`); the AI-client card, which this list still called unbuilt (`dd32014`, `c0af880`); the
console's first shell tests (`0ba4d31`) and its Files page tests (`595ae19`); the terminal's font size
(`ef9f015`); and the device events page (`6424bd7`, an additive `state.get` field rather than a new
tool). The device now refuses an empty memory text (`6fd8a01`), so the page's disabled submit is no
longer the only guard.

What remains:

- **Terminal scrollback search — closed 2026-10-26 (`4f440ee`).** `@xterm/addon-search` (0.16, the
  xterm project's own addon, bundled at build time) searches the tab's own scrollback with highlighted
  matches, a miss sentence and Escape from anywhere in the bar. The workspace has rename, export, a
  font size and search.
- **Liveness/exit-code — mark is closed 2026-10-26 (`d3398f6`); hide is deliberately not built.**
  `terminal.read` now answers `alive`, `exit_code` and `signal`; the tab appends its last bytes, marks
  itself `· ended` with the exit or signal sentence, and stops reading. The tab is never auto-hidden:
  the retained output is the reason to keep it, and closing stays the operator's act. A signal death
  is reported in `portable-pty`'s public wording (e.g. `Killed`), because 0.8.1 exposes no `signal()`
  accessor; a serial or SSH end has no child facts and says only that it ended.
- **Monitor alerts — the chip is closed 2026-10-26 (`7606b5d`); notifications remain.** The top bar
  reads the chosen device's `monitor.list` every 20 seconds and shows `<n> monitors down` from every
  page; clicking it opens Monitors with the same grant. It never shows a count it did not read, keeps
  the last fact through a failed refresh rather than implying an all-clear, and stops polling when the
  grant refuses. Browser notifications (`NotificationsCard`) are not built: the permission prompt is
  its own decision.
- **Restart history and vitals history** (`RestartHistoryCard`/`DeviceHealthCard`): the device card has
  one snapshot, not a series; neither a boot list nor 30 s samples cross the channel. New device facts,
  L. The iteration plan defers this; keep it deferred.
- **Browser live view** (`EmbeddedBrowserPane`): the native Electron view cannot be ported; a CDP
  screencast is the remote equivalent (P6). L, transport + device.

## Do not port, and why

- **`SettingsPage`, `PluginsPage`, `UpdateCard`'s apply button, `DeviceLogsCard`.** Device-local install
  config and the launcher's job. Buffer size, memory caps, gateway registration, auto-launch, plugin
  enable/disable and "install this build" are not remote operator actions: the launcher owns updates,
  capabilities are the static registry, auto-launch is Electron-only, and the local panel at
  `127.0.0.1:18090` is the surface that still works when the console does not. Editing them from the
  console needs new tools and duplicates the installer. Only the read half survives — version,
  staged/current/previous generations, last swap, repairs — and `DeviceCard` already has it.
- **`ApprovalGate` as a per-command queue, `GoalBar`, `WaitingChip`.** The device has no session goal,
  no plan and no approval queue: `authorize` returns `Ask` to the caller, and the console's answer is
  the grant's `allow_destructive`, recorded in the audit. Rebuilding the queue is a second consent model
  and a new device protocol (L). Keep the grant.
- **`RunStrip`, `TrajectoryView`, `PathView`, `DetailsPanel`, `CommandCard`, `HistoryPage`'s structure.**
  They render `run_begin`/`run_end` attribution and a session command trail, both dropped from the new
  runtime. `runs.exec` history stores argv, exit and byte counts, not bodies; the audit stores the tool,
  not the command. Porting the views without the data is a mock-up; restoring the data is a new event
  model.
- **`ArchivePage` / `ActivityPage` as AI-session surfaces.** The question ("what happened while I was
  away") is real; the answer here is recorded terminal sessions and browser actions, which the new
  device does not keep. Do not port the renderer; the device's own event tail is the honest answer and
  it now has a page (`6424bd7`).
- **`DshPage`, `DshPane`, `HarnessHosts`, `DesktopShell`, `EmbeddedBrowserPane`'s native view.**
  Electron-native `WebContentsView`s, desktop density and the DSH harness. A web page cannot compose
  them, and none is a device capability; the harness is a separate product surface.
- **`WorkspacesPanel`.** Remote-host file browsing/editing/exec is an SSH client's job, and the console
  already gives an SSH terminal plus per-device presets. The new runtime deliberately dropped SFTP and
  has no remote-fs capability; rebuilding it is a new capability (L), not a port.
- **`GettingStarted`.** First-run onboarding for a locally installed desktop product; the console's
  login page, picker and empty states already say what to do. A tour is new surface, not recovered
  behaviour.
- **`Sparkline`, `VitalsDial`, `LoadChip`, `BootChip`, `MonitorChip`, `StatusBar`'s instrument line.**
  No series and no restart record to draw; the status bar's remaining job (counts, identity, version)
  is done. Revisit with vitals.

## What the console has that the panel did not

So this list is not read as a one-way loss: a fleet instead of one machine; enrollment codes; signed
per-tool grants and an audit index; files as a page; host facts and update generations on the device
card; one `/mcp` credential that reaches every device. None had a legacy equivalent.
