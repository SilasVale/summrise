# Console polish: the working backlog

**Status:** running. The operator's instruction on 2026-11-05: find the problems myself, write them
down, solve them, and keep going — a goal that can run for hours without a question.

Each item ships on its own: the web gates, the console's Rust tests, a live check on
`console.saisi.online` with a screenshot, then one commit. An item is done when all four are true.

## P0 — the record of what the AI did

1. **Timeline + evidence** *(Shipped after the 2026-11-04 outage: live by `0.3.273-ux44` and
   measured in the fifth list; `78fd4a6` opened Activity on the timeline, `8d07eab` made a row open
   its evidence.)* One row per action (time, sentence, machine, outcome,
   duration); a click opens the evidence (command, output tail, session, links). The audit table
   stays behind the `Table` toggle.
2. **Screenshots as evidence.** *(Shipped: `550da0a` — 4 per device, 32 total, LRU, memory only; the row opens the frame with its capture time; the route is authenticated and a restart forgets it; the live build carries it.)* A `screen.capture` call keeps its last image in console memory
   (bounded per device) and the row opens it inline. The image is ephemeral: nothing is written to
   disk and a restart forgets it.
3. **The session card.** *(Shipped: `1561857` — the terminal opens with the card, not a black rectangle: machine, session, kind, age, last keystroke from the live evidence.)* The Terminal page shows the session before the terminal: machine, shell,
   how long it has been open, the last command, the last few lines, and one button to attach. The
   raw terminal is what you get after that card, not the first thing.
4. **Home stops leading with the endpoint** *(Shipped: `f6ff6aa` deleted the endpoint hero; the
   fifth list measured the hub live at `0.3.273-ux49` — Now band, today's timeline, one `Connect a
   client` action, the endpoint only in the sheet.)* The hub becomes this machine's live page: Now (presence,
   rate, last steps), today's timeline, the session preview. The endpoint lives in the Connect sheet.

## Device-side, still open

The 2026-11-04 outage is over: the fleet has run `0.3.279` since 2026-11-05 (three machines, two
online; the third asleep). What the outage queued has shipped — the first list's statuses say which
commit and which live list prove it.

5. **The device's own panel gets the same record.** *(Done 2026-11-06, 27179dd, live 0.3.281 on DESKTOP-9VH92BJ and DESKTOP-CPH4KB2 via the release channel: the panel's Timeline card reads the newest 200 lines of the device's own `events.jsonl`, groups them by the local day and draws one row per begin/end pair with the time, tool, outcome, door and the end's error; a bound, empty and unreadable sentence. 604 device tests, panel 28, fmt/clippy clean, the review test's `panel-timeline.html` (105 rows, failed tone, bound). The published 0.3.281 installer (6,635,054 B, sha256 `928f30c5…`) is what the console's `/installer` now serves; 9VH's `/panel` HTML carries the Timeline card with 105 rows. DESKTOP-84T3LIN is still 0.3.280 — its serial session wedged, so its update is pending.)*: the panel's Timeline card reads the newest 200 lines of the device's own `events.jsonl`, groups them by the local day and draws one row per begin/end pair with the time, tool, outcome, door and the end's error; a bound sentence when the 200 cut it, an empty sentence and an unreadable sentence. 604 device tests, the panel's 28, fmt/clippy clean; the ignored review test wrote `panel-timeline.html` (105 rows, failed tone, bound). Device-side: it ships with a device release — the release is staged separately and the operator's next install is the first live run.)* The panel the device serves
   (`device/device/src/panel.rs`, reachable on the machine and from the console's Screen page) still
   shows raw tables. It should show the same day-grouped timeline — the device already has the audit
   and every call it serves, so this is device-side work and ships with a device release, never alone.

## P0 — the next list (2026-11-05 audit of the emptied product)

Ranked by honesty first. Each item names its measurement and its gate.

1. **The Now band must count the AI, not the console's own polling.** *(Done 2026-11-05, 7de1358 + 43e6e0d + c3d6edd: the route names the caller; the band counts only `client`; the console's own rows carry a muted `console` mark; the session poll runs only when the page is in front, visible, online and empty.)* With a machine chosen and no
   session, `TerminalWorkspace.tsx:1367` calls `terminal.list` every 10 s through `/mcp` with no
   `visible` guard; each call is audited and streamed, so `LiveNow.tsx:58` reads "Active now · 6 calls
   in the last minute" for an idle machine and the window fills in under three hours. Publish the
   caller (`console` for an ungranted `/mcp` and `/terminal/*/stream`, `client` for `/mcp/:id`) on the
   event and the line; count only `client`; mark the console's own rows; stop the poll when hidden.
   Gate: a `console` line never sets "Active now"; the browser gate serves one `terminal.list` and
   expects "Quiet". **M.**
2. **When the stream lags, say what was lost and reread it.** *(Done 2026-11-05, b441f8e: the dropped count is parsed, summed, and only cleared by a completed reread; the page says "N lines were dropped while this tab was behind; rereading" and returns to Following after.)* `/events` sends `{"dropped":N}`; the
   client discards it, so the page says "Following…" over a hole. Parse it, refetch `/audit`, say
   "N lines were dropped while this tab was behind; rereading" until the answer lands. **S.**
3. **A machine's scope must read that machine's window.** *(Done 2026-11-05, 3dfaaeb: one window device derived from the scope; a switch is exactly one `/audit?device=` request; the cap sentence names the machine; chips list every machine and refetch rather than filter the wrong window.)* Activity fetches the fleet's 1000 and
   filters in the page; `GET /audit?device=` exists. Fetch the machine's own window when a chip is
   selected, and let the cap sentence name whose window was cut. **S.**
4. **The hub's day must be reread, not remembered.** *(Done 2026-11-05, f8ded1c: reread on the 5 s tick and on stream recovery, single-flight per machine; the band reads the machine's own merged window; a stale window says "Read N ago." until a reread includes the live line.)* The machine window is fetched once per device;
   after a reconnect or a hiccup the day shows a hole with no cap sentence and the band says "Quiet"
   while rows exist. Refetch on the app's 5 s tick and on stream recovery; say when it was read. **M.**
5. **Stop claiming every call is in the record.** *(Done 2026-11-05, f394b63: both empty states name the boundary — calls routed through the console are written; a call straight to a machine's own endpoint is not.)*** Both empty states say "Every call a client makes is
   written here"; a call straight to a machine's own endpoint is not. Name the boundary. **S.**
6. **Give every tool a sentence and gate the table against the registry.** *(Done 2026-11-05, a9d9dff: 16 templates added, 10 dead ones removed after a history measurement; `every_registry_tool_has_a_sentence` walks the registry and fails for a declared tool without a sentence.)* 16 of the 46 registry tools
   fall back to "Used `tool`" (`host.restart` among them) and eight sentences name tools the registry
   does not have. Write the missing templates and test the table by walking `registry.rs`. **S.**
7. **Say where one stretch ends.** *(Done 2026-11-05, dfc35c0: at each boundary the page says what the stretch above it held — "3 calls · 1 m" — from the same rule that draws the hairline; nothing collapsed or inferred.)* The 60 s gap is computed and drawn as a hairline; a full day reads
   as one column. Label each stretch with what it held — "4 calls · 2 m 10 s" — inferring nothing. **S.**
8. **Delete the retired components and the calls nobody makes.** *(Done 2026-11-05: `TerminalPane.tsx` (159 lines), `ToolPicker.tsx` + its test, `api.denials`, `api.meshStatus`, `api.meshReadiness` and twelve tests deleted; the server routes stay.)*** `TerminalPane.tsx` (159 lines, no
   importer), `ToolPicker.tsx` (42 + 35), `api.denials`, `api.meshStatus`/`meshReadiness` and their
   tests; the server routes stay. **S.**

## P0 — the operator's day (2026-11-05, second audit)

All six console-only; the last is the deletion. Ranked by how often the friction happens.

1. **Make the morning's failures countable.** *(Done 2026-11-05, c51aad9: one "Today N client calls · M failed · last HH:MM" line from the window already in hand, "Today none · last … yesterday" otherwise; failed rows read `failed`, including 200 with `ok:false`.)* The hub cuts "today" at the browser's midnight, so an
   08:00 page says "No calls from X today" after a night that ended at 23:50 (`HomePage.tsx:307-314,
   329-334`); a failed call reads with its HTTP **200** in the timeline (`ActivityTimeline.tsx:345-349`),
   and "what failed overnight" costs four controls. One "Today 24 client calls · 2 failed · last 14:32"
   line from the lines already on the page, "last 23:42 yesterday" when there are none, and the
   timeline's outcome word "failed". Gate: a fixture with 2 ok + 1 failed today plus a 20 h-old line.
   **S.**
2. **Name the token on the newest rows.** *(Done 2026-11-05, b554e00 + 587dca3 + 3dadcfe, live in ux42: a live event carries `token_id`; a row names its token or "signed grant"/"operator session", and Manage opens the Tokens sheet on that row.)* A live event carries no `token_id` (`http.rs:455-467,
   679-710`) and the client drops it from the snapshot (`liveActivity.ts:148-172`), so the newest rows
   read "—" and the record says "operator session" (`AuditTable.tsx:579-581`); the evidence has no
   Token fact. Carry `identity.token_id` through the event and `LiveLine`, show "Token <name>", and a
   Manage that opens the Tokens sheet on that record; no token means "signed grant". **M.**
3. **Show the call that is in flight.** *(Done 2026-11-05, 03eb857 + 0438445: a memory-only registry publishes `started`/`finished` and carries the running set in the snapshot; the band shows `running runs.exec · 1m 12s` for client calls and hides console ones. Note: the proxy cuts a call at 90 s and its 504 publishes `finished`, so the audit's `3m 12s` sample is unreachable without raising that cut — the bound is the truth.)* Calls publish only after the device answers and the proxy cuts
   a run at 90 s while it keeps running, so the band says "Quiet" for a three-minute `runs.exec` and
   the timeline has no row. A non-audit `started` frame at `tools/call`, cleared by the result or the
   90 s cut; the band shows "running runs.exec · 3m 12s". **M.**
4. **Let the machine menu compare machines.** *(Done 2026-11-05, 616f447 + 08386ae: `/fleet` serves `last_outage {away_ms, back_at_ms}` from memory; the menu row states the tightest disk and, after a drop, how long it was away — each half omitted when unknown, never zeroed.)* *(Measured: the fleet read carries `host` facts and `last_attempt` (`attempts`, `reason`), not an outage duration — so the line can name disk free and, when offline, the last attempt, but "back 2 h ago after 41 m" needs a new server field first. The item is split: the line now, the duration when the fleet records it.)* It shows dot, name, state, version (`MachineMenu.tsx:72-89`)
   while the fleet read already carries `host` and `last_outage` the web's `Device` does not name; one
   muted line — "C: 312 GiB free · back 2 h ago after 41 m". **S.**
5. **Put the last column on screen at 390.** *(Done 2026-11-05, 188d078: at 390 the audit disclosure is the first cell's control and the sixth column is not drawn; the token list is a card layout inside the 600 px drawer — measured Revoke inside the sheet at 390 and 1280; desktop page outside the drawer unchanged.)* The audit table is 44 rem wide with its only opener in the
   sixth column, and the token sheet's Revoke/Rotate are clipped in the 600 px sheet even at 1280;
   move the disclosure to the first cell at ≤639 and stack each token's actions under its name. **S.**
6. **Delete the device id from the Machine tab.** *(Done 2026-11-05: the 64 hex is gone; build and online stay; the record's Copy id and the Connect endpoint keep it.)* (`SystemPage.tsx:391-397`) prints 64 hex that nothing
   there acts on; the record's Copy id and the Connect endpoint keep it. **S.**

7. **A device id the fleet does not know must say so.** *(Done 2026-11-05, f370876, live in ux40: the chip reads "unknown machine · desktop-9vh9" and its title says it is not in the fleet; a known id is unchanged.)* One audit line carries `desktop-9vh92bj`
   as its device id (an old enrollment), so the timeline grows a machine chip that is not in the
   fleet. The chip is true — the line is real — but it reads like a machine. Label an unknown id
   "unknown machine · <short id>" and give it no online dot. *(Measured on the live 1000-line window:
   865 + 93 + 41 known, 1 unknown.)* **S.**

## P0 — the fourth list (opened 2026-11-05, from the live fleet)

1. **An absence the console did not witness is forgotten.** *(Done 2026-11-05, 8449fc8: `device.joined`/`device.left` lines in the audit; `/fleet` derives the absence from the newest 2,000 lines, so the CPH case now serves away_ms 10,298,000 instead of null; omitted when no pair exists.)* DESKTOP-CPH4KB2 was away ~5 h
   (last frame 03:58:22, back 06:50) and `/fleet` serves `last_outage: None`, because the record is
   memory-only and the console restarted at 06:47 — the close was never seen by the new process, so
   the open had nothing to subtract. The machine's own `last_attempt` says `7 attempts — the console
   accepted the channel and then dropped it`, which is the *reason* but not *how long*. The fix:
   write `device.left` / `device.joined` lines to the audit on channel close/open, and derive the
   absence from the audit so a console restart cannot erase it. **S/M.**

2. **A socket is not presence.** *(Done 2026-11-05: `online` is a frame within `PRESENCE_WINDOW` =
   150 s = 2.5 × the device's 60 s report; `/fleet` serves `last_frame_ms` only when a frame was seen;
   a silent channel reads offline and its row shows no reconnect line; a reopened channel starts with
   no frame.)* DESKTOP-14RJCR8 was offline on the tailnet for 7+ minutes (`tailscale status`:
   `offline, last seen 7m ago`) while `/fleet` still served `"online": true`. The console asked
   whether a channel socket existed, so a half-open TCP connection to a sleeping machine kept
   answering yes. The device already sends a host frame every 60 s, so presence is that frame's
   recency: 150 s is two and a half reports — one missed report is not an outage, and a sleeping
   machine stops claiming presence after 02:30. A channel silent past the window serves
   `online: false` with the exact `last_frame_ms`, or omits the key when the console has never seen a
   frame; the page's "seen 7m ago" reads the same moment, and the row does not explain silence as
   "reconnected after N attempts", which requires a real reconnect. **S.**

3. **A machine that slept must not look like a failed update, and a machine that woke must prove
   its version.** *(Done 2026-11-05, 4eed9a5: the 60 s frame carries a top-level `update` section
   beside `host` — `current`, `verdict {outcome, detail}`, `last_update {to, outcome, at_ms}` and
   `rolled_back_to` — read from `state.json` and omitted whole when it cannot be read; `/fleet`
   serves it per device, newest frame wins; the fleet row and every machine tab draw the device's
   own sentence, `started 0.3.275 (update to 0.3.276 refused)` or `started 0.3.276 (updated)`, as a
   quiet line beside the silence. A silent device that sent no section still shows only
   `offline · last seen 7m ago`; no page claims the machine slept.)* DESKTOP-14RJCR8's update check
   was triggered at 08:21 and the machine left the tailnet inside the same minute (`tailscale ping`
   timed out), so the console could only say "offline": a machine that stopped reporting and one an
   update broke looked the same, and after it woke nobody could read whether the pointer moved
   without a shell on it. The device already wrote `lastVerdict`, `update {phase, to, outcome,
   lastResultAtMs}` and `baseline` into `state.json`; the fix puts those facts on the frame the
   console already receives and lets the row repeat the device's own words instead of inferring a
   cause. Measured: 6 new device host tests, 2 console integration tests (a frame's section reaches
   `/fleet`; the newest frame wins; an absent section leaves the row without one), 1 channels unit
   test, 7 sentence tests, 3 fleet-row tests and 2 machine-page tests; `cargo test -p
   summrise-device -p summrise-console` 943 passed / 0 failed / 3 ignored; `npx vitest run` 598
   passed; typecheck and build clean; `build-web.sh --check` clean (the page is committed); browser
   gate PASS 17 steps; `a11y-audit` 0 findings across 16 page states. **S/M.**

## P1 — the console stops feeling like a configuration tool

5. **Activity filters become chips.** *(Done 2026-11-04, e918981: the C2 cut — machine chips and a
   search on the timeline, the six controls table-only, the shared state surviving a switch.)*
   Machine chips and a search field; the time-range select, the token column and the status select
   move into the Table view where they belong.
6. **Terminal tabs carry facts.** *(Done 2026-11-04, bdefee2: a tab says `pty · 12m`; the shell is not in `terminal.list`, so it is not claimed; a renamed tab keeps its name.)* A tab says the shell and the age, not just `pty 1`.
7. **Connect has one token tab.** *(Done 2026-11-04, 2b681e3: one list, scope column, one mint form; `FleetTokens.tsx` deleted; the second read that 405'd is gone in 44e60f7.)* Per-device tokens and fleet tokens are the same idea at two
   scopes; one list with a scope column, not two places to mint a credential.
8. **Empty and offline states say what to do.** *(Done 2026-11-04, 1329499: ten states rewritten to name the page, the reason and the one action; empty and offline kept distinct.)* Every page that can be empty (timeline, runs, files,
   plugins) gets a sentence that names the next action, not just "none".

## P2 — consolidation, then polish

9. **Capabilities and Plugins become one Tools tab** under the machine. *(Done 2026-11-04, 695be48: the rail is Home · Activity · Device; Tools sits in the machine's tabs; `ToolCatalogue` and `PluginList` are two sections of it; `#capabilities`/`#plugins` map to it.)* One place answers "what can
   this machine do", with plugins as a section of the same answer.
10. **The Runs page's exec form moves** under Tools; Runs becomes the history, not a second way to
    run a command. *(Done 2026-11-04, d1e354f: the form is Tools' third section and calls `runs.exec`
    with the same arguments; Runs is `runs.list` and its record; `#device/tools/run` was not added
    because the fragment parser reads a sub-path as a tool name.)*
11. **Accessibility sweep.** *(Done 2026-11-04, b7ba553: `scripts/a11y-audit.mjs` measures 17 page states and reports 0 findings and 3 allowed tab stops — the named scrollable regions; the audit row's opener is a real button with `aria-expanded`; the warning colour is in the AA pair test.)* Every row that opens is a button with `aria-expanded`; every icon has a
    name; focus order follows reading order; the warning colour is checked against the AA pairs.
12. **Long timelines.** *(Done 2026-11-04: measured before optimising. The page now asks the
    console's 1000-line cap instead of 200, and the stub's 1000-row day takes **427 ms** from
    navigation to the last row painted at 1280 (403 ms at 390), so virtualisation was not done; the
    merged window is capped at 1000 and the page says "Showing the newest 1000 lines; older ones are not on this page." when the cut
    happens, in the timeline and in the Table (4b26900).)* Measure a 1000-row day before optimising; virtualise only if the frame
    budget says so.
13. **Phone.** *(Done 2026-11-04, 8b50f22: time column 64→40 px at 390 and 360, age hidden ≤480 px, the day label is sticky against `main` (measured offset 0 after 600 px of scroll), evidence was already one column; no row overflows.)* The timeline at 390 px: the time column narrows, the age drops, the evidence panel
    stacks (already true) and the day label sticks.

## Cuts — what should not exist

A product gets better by removing things, not only by adding them. These are the deletions this
workstream owns; each one ships with the same gates as a feature.

- **C1. The hub's endpoint hero.** *(Done 2026-11-04, f6ff6aa.)* The machine page leads with an URL and a Copy button while the
  Connect sheet already owns that job. Delete the hero; the sheet is the only place the endpoint
  appears. *(P0-4 covers the page; C1 is the deletion.)*
- **C2. The Activity filter bar in the timeline view.** *(Done 2026-11-04, e918981: machine chips and a search on the timeline, the six controls table-only, the shared state survives a switch.)* Six controls (scope, time, device, token,
  search, status) belong to the table. The timeline gets machine chips and a search field; the rest
  moves behind `Table`. *(P1-5.)*
- **C3. The tool sheet's "Try it" panel.** *(Done 2026-11-04: 634c438 removed the panel, ce4602d the helpers only it used.)* A browser that can invoke any tool with raw JSON is a
  developer toy with the operator's full authority; Capabilities is a catalogue, not a REPL. Delete
  the panel, keep the catalogue. *(Revisit: the operator may want it under a "Developer" sheet.)*
- **C4. The legacy server-rendered pages under `/next`.** *(Done 2026-11-04, cec8b28: 55 routes, 7002 lines of page.rs, 91 HTML-only tests, maud and six page-only assets deleted; `GET /next` kept as a 303 shim because `--login-link` prints `?code=` and the React page does not redeem it.)* They predate the React console and the
  React page is `/`. Measure what still links to them before deleting; if nothing does, the routes
  and `page.rs` go in one commit. *(Needs the measurement first — not tonight.)*
- **C5. Fleet tokens as a second minting place.** *(Done via P1-7: one list with a scope column and one mint form; `FleetTokens.tsx` deleted. Closed 2026-11-04.)* One credential list with a scope column replaces
  two pages.

*(Verified live 2026-11-06 on 0.3.273-ux74: all five hold — the hub has no endpoint hero (the only
endpoint is in the Connect sheet), the timeline's filter bar is machine chips + search, the tool
sheet has no Try-it, no page links to `/next` (the shim answers 303) and one mint form sits behind
every door. Audit: `.superpowers/sdd/2026-11-05-console-polish-backlog/audit-20-report.md`.)*


## P0 — the ninth list (opened 2026-11-06, from the operator's first client and the token confusion)

Measured on 2026-11-06, after the first real install and the first outside MCP client: the console
printed an enrollment command the machine could not run, and the person looking for the token could
not find it. The console's own Tokens sheet holds five tokens with different, fixed device scopes;
nothing on the fleet says so.

1. **The enrollment command names a binary the installer never puts on PATH.** *(Done 2026-11-06, e763259, live 0.3.273-ux67: the enrollment block now says to open the elevated Command Prompt in the folder chosen on the installer's directory page (the default is `C:\Program Files\Summrise`), run this command, then restart the `SummriseLaunch` service; the command itself is unchanged because the console cannot know a new machine's root. The installer's PATH entry remains item 3. jsdom 718 tests, typecheck 0, gate PASS 36 steps, a11y 0, `build-web --check` clean; live at 1440 and 390 the sentence read verbatim, the panel's Tokens control stayed, and the page did not scroll sideways; screenshots /tmp/live-enroll-sentence-1440.png and /tmp/live-enroll-sentence-390.png.)* The Add-a-machine
   panel prints `summrise-device.exe --enroll …` (`App.tsx:863-864`, hardcoded); `summrise.nsi`
   writes no PATH entry (the only `PATH` mention is a comment); the live operator installed to
   `E:\Program Files\Summrise` and the command runs in an ordinary elevated prompt, whose cwd is
   not that folder. **Cost:** the first command the product prints fails with "not recognized",
   after the person has already downloaded the installer and minted the code. **Fix:** the installer
   owns the root, so it adds `$INSTDIR` to the machine PATH (and the uninstaller removes it), or the
   panel's sentence names the install folder; decide once, in the installer. **M.** (installer +
   one console sentence.) *(Split 2026-11-06: the console sentence is task 6 now; the PATH entry is
   item 3 below, because only the installer knows the root and it needs the Windows run.)*
2. **A credential has no door from the fleet, and its fixed scope is unstated.** *(Done 2026-11-06, 5bc53c4, live 0.3.273-ux66: a Tokens control in the fleet header and the Add-a-machine panel opens the existing Connect sheet's Tokens section — no second token UI; the section states "A token's device scope is fixed when it is minted. Rotate keeps it; to cover different machines, mint a new token."; the mint form names the machines the new token will cover and the Rotate dialog says the fixed scope is kept. jsdom 717 tests (+6), typecheck 0, browser gate PASS 36 steps (the 390 door opens the Tokens section and the sentence is visible; screenshot /tmp/console-browser-gate/06b-fleet-tokens-390.png), a11y 0 findings, `build-web --check` clean. Live: the fleet's door opened the Tokens sheet with the sentence; screenshots /tmp/live-tokens-door-1440.png and /tmp/live-tokens-door-390.png.)* Tokens live only
   under Account → Tokens; the live fleet, the Add-a-machine panel and the Connect sheet have no
   Tokens link (`onTokens` is wired only for the Connect sheet). The operator's colleague measured
   "console.saisi.online 根本没有看到 token" on 2026-11-06, while the console actually held five
   tokens with different device scopes — one of them missing the machine being asked about. The
   mint form never says the device scope is fixed at creation and Rotate keeps it. **Cost:** a
   person cannot find where a token comes from, and the multi-token model reads as one token with
   missing machines. **Fix:** a Tokens door from the fleet header and the Add-a-machine panel; the
   sheet and the mint form state "the device scope is fixed at creation; Rotate keeps it; to cover
   different machines, mint a new token", and the mint form names the machines it will cover.
   **M.**
3. **The enrollment command still depends on a PATH entry the installer never writes.** *(Implemented 2026-11-06, 8f1775c: after the install's `/api/health` proof the setup reads the machine `Path` in the 64-bit view, appends the delimited `$INSTDIR` only when absent, writes it as REG_EXPAND_SZ and broadcasts `WM_SETTINGCHANGE`; the uninstaller removes exactly its delimited entry (four positions) and broadcasts. A read error, a Path over the ruled 2000 or over NSIS's 1023-character string cap is logged and the value is left alone; the install continues. Compiled: `SummriseSetup-0.3.282.exe` 172,206 B, exit 0 with stub payloads. NOT RUN ON WINDOWS: the append, idempotency, empty/trailing separator cases, the System::Call broadcast, both length guards and the uninstaller's removal are unexercised; the operator's next install is the first proof. Read-only fleet check: 9VH Path 342 chars, CPH 869, both REG_SZ and the entry absent; 14R did not answer.)* The console
   sentence (task 6) tells the operator to open the prompt in the install folder; the product's own
   command is still a bare `summrise-device.exe`, so a prompt opened anywhere else fails. The
   installer knows the root; the console does not. **Fix:** the installer appends `$INSTDIR` to the
   machine PATH (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`), only
   when absent, and the uninstaller removes exactly its entry; the Windows run is the first proof.
   **M.** (installer; measured on the E: install.)


## P1 — the tenth list (opened 2026-11-06, from the live token sheet)

Measured in the live Tokens sheet (0.3.273-ux66) after the operator's "which token has 84T3LIN?"
question: every multi-machine token's SCOPE cell reads a count ("3 machines it was minted for"), never
the names, and the all-machines mint form lists a machine that has not answered in 23 hours as if it
were live. The sheet shows last-used and created dates, so a stale credential can be told from a live
one only by date, not by scope.

1. **A token's scope counts machines but never names them.** *(Done 2026-11-06, 651d337, live 0.3.273-ux68: the SCOPE cell maps the token's `device_ids` through the fleet's names — a single device is named, a fleet token lists up to three names and says "and N more", and the title carries the full list, the count and the recorded-at-mint rule; unknown ids fall back to the console's short id. Live: the fleet token's row reads `DESKTOP-9VH92BJ, DESKTOP-CPH4KB2, DESKTOP-84T3LIN` instead of "3 machines it was minted for". vitest 720 passed/53 files, typecheck 0, gate PASS 36 steps (the fleet row names the gate machine), a11y 0 findings/3 allowed tab stops, `build-web --check` clean; live screenshot /tmp/live-token-scope-names-1440.png.)* The sheet's SCOPE cell for a fleet token
   is "3 machines it was minted for"; the operator cannot see that one of five tokens covers
   84T3LIN and another covers CPH4KB2. The refusal `device not permitted by token` then has no
   visible cause, and the operator mints another token to guess. **Fix:** the scope cell names the
   machines from the token's `device_ids` and the fleet's names (single device: its name; fleet:
   names with "and N more" when long), keeping the count for the totals. **M.**
2. **The all-machines mint form lists a dead machine as if it were live.** *(Done 2026-11-06, be7cda1, live 0.3.273-ux69: the coverage sentence marks each machine from `/fleet` with the fleet row's own evidence — `(online)` or `(offline, last seen 1d 1h ago)` — and a never-heard machine reads `(offline)` in the console's existing wording; the token still covers every enrolled machine (the test asserts an offline id stays in `device_ids`). vitest 721 passed/53 files, typecheck 0, gate PASS 37 steps, a11y 0 findings/3 allowed tab stops, `build-web --check` clean. Live read: "DESKTOP-9VH92BJ (online), DESKTOP-14RJCR8 (offline, last seen 1d 1h ago), DESKTOP-CPH4KB2 (online), DESKTOP-84T3LIN (online)"; screenshots /tmp/live-coverage-states-1440.png and -390.png; no sideways scroll at 390.)* The coverage sentence
   read "This token will cover the 4 machines enrolled now: DESKTOP-9VH92BJ, DESKTOP-14RJCR8,
   DESKTOP-CPH4KB2, DESKTOP-84T3LIN" while 14RJCR8 had not answered for 23 hours; minting "all
   machines" silently adds a machine that cannot answer to every future scope. **Fix:** the
   coverage sentence marks each machine's state from `/fleet` (online, or `offline, last seen …`),
   so the person choosing "all machines" sees what they are covering; the token still covers what
   it says. **S.**


## P2 — the eleventh list (opened 2026-11-06, from the live token sheet's dates)

Measured on 0.3.273-ux69: the Tokens sheet renders created/used/expires through
`toLocaleDateString()` — the live rows read `used 11/6/2026`, `created 11/4/2026` in an en-US
browser. The console's other time language is relative (`1d 1h ago`, `seen 37s ago`) or a clock
(`last 17:23`); the sheet is the one place a bare numeric date can be read as 4 November or 11
April. The device-side panel item stays in its own section ("Device-side, still open"), because it
ships with a device release and never alone.

1. **The token sheet's dates are the browser's locale, not the console's clock.** *(Done 2026-11-06, 0fad059, live 0.3.273-ux70: created/used/expires share one `<time>` element — local-day ISO `2026-11-04`, `title` the local clock to the second plus the UTC ISO instant; a null expiry renders `never` and a null last use `never used`, with no date element. vitest 726 passed/53 files (+5), typecheck 0, gate PASS 38 steps (asserts ISO text, exact titles and `never`), a11y 0 findings/3 allowed tab stops, `build-web --check` clean. Live read: 8 times all `YYYY-MM-DD`, the first `2026-11-06` with title `2026-11-06 08:21:12 (2026-11-06T08:21:12.787Z)`; screenshot /tmp/live-token-dates-1440.png.)* Same audit: the
   dates are the only absolute dates in the console; a CN operator sees `11/4/2026` with no way to
   know the order. **Fix:** one locale-stable format for created/used/expires — `2026-11-04` (the
   audit's own ISO dates) or a fixed `4 Nov 2026` — with the exact time in the cell's `title`. **S.**


## P0 — the twelfth list (opened 2026-11-06, from the live terminal)

Measured live on 0.3.273-ux70, DESKTOP-84T3LIN's serial session-3: the tab read `serial · 41m #3
read failed`, the pane was an empty cursor, and the sentence said `Reading this session failed:
terminal.read: the cursor 7812078 is older than the retained output, which starts at 8612223. The
tab continues from the earliest output the device retains.` Try again repeats the same refusal.

1. **A stream fault from an old cursor leaves the tab dead.** *(Done 2026-11-06, 4f91236, live 0.3.273-ux71: the feed owns the recovery — a cursor-too-old refusal moves to the earliest byte and reconnects (400 ms), a second newer earliest moves again (1.5 s), then the tab degrades to the newest 4 KiB tail read just below the session's end with a sticky sentence naming the dropped bytes, and the stream reopens from the read cursor; a 12 s catch-up guard covers a silent recovery stream; Try again restarts the feed at the current cursor. The feed effect now depends on `grant.deviceId`, so the 5 s fleet poll no longer restarts it every poll. 734 tests (+14), typecheck 0, gate PASS 39 steps (a stubbed stream fault reconnects at 1024, draws the retained bytes, no `.terminal-fault`), a11y 0/3, `build-web --check` clean. Live: the 9VH pty session-1 (a >1 MiB flood) drew 1,516 chars of retained output with 0 `.terminal-fault` and no page errors on ux71, where the old page drew nothing and kept the fault; the 84T3LIN serial session reproduced the dead tab on the old page and its silent catch-up is why the guard exists. Screenshot /tmp/live-terminal-9vh-fixed.png.)* `TerminalWorkspace.tsx`'s `fail()`
   moves the cursors map to the earliest byte the device retains, but the stream that sent the
   refusal is not reconnected and the polling feed is only used when the stream is absent; no read
   ever uses the new cursor, and the pane never draws the output the sentence promises. A tab that
   fell behind the 1 MiB ring (a hidden tab, a fast serial line) is therefore unrecoverable until
   Try again — and Try again reopens against the same faulted state. **Cost:** the operator sees a
   dead tab that claims it continues, while the device still holds a megabyte of the session.
   **Fix:** on a cursor-too-old refusal, reconnect the stream (or poll) with the earliest cursor
   automatically and clear the fault when bytes arrive; if the ring keeps advancing faster than the
   read, degrade to the newest retained output with a sentence naming the dropped bytes, never an
   empty dead tab. **M.**


## P0 — the thirteenth list (opened 2026-11-06, from the flooded serial session)

Measured while checking the terminal fix against DESKTOP-84T3LIN (0.3.280, session-3, a serial line
that had produced 18.1 MB): `terminal.read` at cursor 0 answered in 2 s with the too-old refusal
(`the cursor 0 is older than the retained output, which starts at 18153607`); the read at
**18153607** was answered by Cloudflare with a **504 after 33.8 s** (16-byte error body), and every
subsequent MCP call on that device (`host.facts`) timed out at 20 s. The device's own channel still
reports frames, so the console shows it online while its MCP service is no longer answering. The
console's terminal tab is the surface that finds this; the device's read is the cause.

1. **A flooded serial session's read blocks the device's whole MCP service.** The read of the ring at
   the oldest retained cursor waits (for the serial feed or on a lock held across it) instead of
   copying the retained bytes; the call runs past the proxy's timeout, the device stops answering
   other tools, and the only recovery is a local restart. **Cost:** one busy serial line takes the
   machine off the console; the terminal fix can degrade the tab but cannot get the ring or keep the
   device answering. **Fix:** the read copies the retained bytes from the cursor without holding the
   registry across the feed — an old-cursor read answers immediately, and an optional byte `limit`
   lets the console page the catch-up; the too-old refusal and the ring semantics stay. **M/L.**
   (device; a device release. The still-wedged session-3 is the reproduction; 9VH/CPH's 0.3.281
   channel can carry the fix to a fresh flooded pty for the live proof.)

2. **The console still asks for the whole ring, so the fix's paging is unused.** *(Done 2026-11-06, 73df5d4, live 0.3.273-ux72: on a cursor-too-old the console pages the retained ring through polling `terminal.read` with `limit` 65,536, draws each page, advances the cursor, treats a short/empty page as the end, then opens the stream at the reached cursor; a page cap of 17 and the 12 s guard cut into the newest 4 KiB with the existing loss sentence; an older device refusing `limit` disables the field once for the grant and takes the old reconnect/tail path. 737 tests (+3), typecheck 0, gate PASS 39 steps (a stubbed too-old refused once and paged from 1024, the stream resumed at 1050), a11y 0/3, `build-web --check` clean. Live: a fresh 9VH 0.3.282 PowerShell pty session-2 flooded >1 MiB; the deployed page drew it with 0 `.terminal-fault` and no page errors (screenshot /tmp/live-ux72-paged.png), and the implementer's candidate-page run made 16 limited reads of 65,536 + one short page, advanced 299,744 → 1,351,568, resumed the stream there and drew the output (/tmp/live-task14-paged.png); each MCP answer ~160 KB, not the whole-ring frame.)* The device's
   `terminal.read` now takes an optional byte `limit` (184ee8c), but the console's recovery reconnects
   the stream at the earliest cursor and the device's subscribe frame carries the whole ring (up to
   1 MiB, `data` + `data_base64`) — the frame whose 30 s bound the measured 504 came from. **Cost:**
   the registry no longer wedges, but the catch-up can still be too large to arrive, and the console's
   degraded tab throws away the ring it could have paged. **Fix:** the console pages the catch-up
   with `terminal.read`'s `limit` (64 KiB), drawing each page and advancing the cursor, with the
   `wait_ms`-style capability check for devices that refuse the field; only after the pages are drawn
   (or the guard expires) does it open the stream at the reached cursor. **M.** (console; live on a
   fresh flooded pty after the 0.3.282 channel update.)

3. **The console server closes a subscription whose ack arrives empty.** *(Done 2026-11-06, 18163fd, live 0.3.273-ux73: `subscription_stream` now runs the ack and the pushed frames through one loop and one `FrameStep` — empty-and-alive frames are skipped in both positions, and the stream ends only on `recv() == None`, `alive:false`, or a fault/error; a refused subscribe is normalised to the same `{"error": sentence}` shape, so ack and follow share one fault payload. Four new integration tests (empty ack then output; empty frame between outputs; end; fault) failed against the unmodified server and pass after; 243 console tests, fmt/clippy clean. Live 2026-11-06 on 0.3.273-ux74: the console's stream `/terminal/9VH/session-4/stream?from=407` answered 200 and stayed open with no data for 6 s (the reader's next read timed out rather than ending), and a `terminal.write` then delivered `event: output` on that same stream at cursor 430 — the empty ack no longer ends it. The earlier attempt was blocked by the tunnel, which is item 4/6.)* The paged catch-up resumes
   the stream at the reached cursor; the console server's subscription ack for an up-to-date cursor
   is an empty read, `read_event` emits no SSE event, the stream closes, the page reconnects once at
   the same cursor and then falls back to 250 ms polling. **Cost:** every tab that catches up pays a
   reconnect and then polls until new output; the stream's job is lost exactly when the tab is
   current. **Fix:** keep the subscription open across an empty ack (a keep-alive frame or a wait for
   the next output) so the next bytes are pushed, not polled. **S/M.** (console server; measured by
   task 14's live run.)

4. **A tool call through the console can spend 45 s upstream of a healthy device.** *(Reframed
   2026-11-06 by the device-side audit: the device's 12,674 `terminal.read` pairs all end — the
   longest is 20.003 s for a requested 20,000 ms, the long polls end at 15.002 s, and at most 2 of
   its 16 in-flight permits were used; current reads are 1 ms. The earlier 45 s (wait_ms 1000) and
   20 s (wait_ms 0) client timeouts, and the 0-byte `runs.exec`, are therefore upstream: the
   console's aggregate MCP / device channel / route. The console's own audit rows carry the
   console-side duration — compare them with the device's 15 s pairs. **Fix:** remove the console's
   queue/loss so a tool answer arrives within its wait; task 18. **M.** (console; no device
   release.)*
   *(Measured 2026-11-06 through task 18: the direct nginx->console path answers terminal.read
   wait_ms 1000 in 1.012 s (device pair 1002 ms), terminal.list in 0.010 s, and 20 concurrent reads
   with 16 at 1.02 s and 4 at 2.03 s, no loss. The same requests via mcp.saisi.online (the Cloudflare
   tunnel) took 1.7-65.7 s with two resets, while even initialize (pure console) took 2.3-8.4 s; an
   unrelated zss-dsh endpoint was equally slow, so the tunnel is degraded generally. The 45 s/20 s
   reads left no console audit row and no device pair — they never reached nginx. The measured 45 s
   is the tunnel, not the console. The console's only related defect is its 30 s reply bound cutting a
   terminal.read whose device-side legal wait is also 30 s; task 18 fixes it by deriving the bound
   from wait_ms, like run_wait does for runs.exec.)* *(Done 2026-11-06, 7841b9d, live 0.3.273-ux74: on the direct tailnet path a `terminal.read` with `wait_ms: 30000` returned the device's empty answer at **30.0197 s** (200), where the console's 30 s reply bound used to cut it; 245 console tests, fmt/clippy clean. The measured 45 s/20 s were the Cloudflare tunnel (item 6's deployment note), not the console.)*

5. **The console transport's in-flight acquire is unbounded.** *(Done 2026-11-06, cd88550, live 0.3.284 on DESKTOP-CPH4KB2: the acquire is bounded by `IN_FLIGHT_WAIT` 5 s; on expiry the frame gets the transport's error envelope (-32003 "the device is busy: too many requests are already running; retry shortly") and is audited deny/busy with its end recorded. 612 device tests, fmt/clippy clean. Live: 17 concurrent `terminal.read` calls (wait_ms 15000, cursor 407) through the console — 16 answered at 15.04-15.09 s, the 17th at 5.1 s with exactly the -32003 sentence; the console forwards the envelope unchanged and needs no change.)*
   `in_flight.acquire_owned()` with no bound; if the 16 permits ever do run out, every tool call
   hangs instead of answering "the device is busy". **S.** (device; hardening, later release.)

6. **The console hands its own operator the slow door.** The token/connect copy names
   `https://mcp.saisi.online/mcp/all`, which crosses the Cloudflare tunnel (measured 1.7-65.7 s and
   two resets for calls that the direct path answers in 1.012 s). **Fix:** for a client already on
   the tailnet, name the direct address beside the public one; outside the tailnet the public door
   stays. **S.** (console copy + the operator's clients; the tunnel itself is deployment.)
   *(Reframed 2026-11-06: the console's copy cannot remove the tunnel's wait, and a direct address
   needs a trusted certificate (the nginx's is self-signed), so this is a **deployment item**, not a
   console code item: repair the tunnel, or serve the tailnet door with a trusted (Tailscale)
   certificate and name it. The console keeps stating the public door honestly; nothing is added to
   the page that would not shorten the wait.)*

## P0 — the fourteenth list (opened 2026-11-06, from the audit of 0.3.273-ux74)

The five cuts hold on the live console. What the tour found is four residuals the thirteen lists do
not cover: two honesty gaps at the moment of minting and counting, and two phone layouts the 390
checks missed. Measured at 1440×960 and 390×844, Asia/Shanghai, selected machine 9VH92BJ (v0.3.284);
every number is a live DOM measurement and the screenshots are `/tmp/audit-20-*.png`.

1. **The fleet token form says no tool can change a machine while the machine form says 15.** *(Done 2026-11-06, ad0d655, live 0.3.273-ux75: the catalogue hook now keeps `/tools`'s destructive flags (a fallback name carries `null`, never `false`), and the token form counts the live list or the registry, so the fleet form reads `46 tools · 15 can change the machine · one list for every machine in the token's scope`; when the flags are genuinely unread the sentence says the count is read from a machine, never `0 can change`. 739 tests (+2), gate PASS 41 steps, a11y 0/3, `build-web --check`. Live screenshot /tmp/live-ux75-fleet-tokens.png.)*
   Without a current machine the Tokens form falls back to the 46 tool names with every destructive
   flag false and prints `46 tools · 0 can change the machine · one list for every machine in the
   token's scope`; the same sheet with 9VH92BJ selected prints `46 tools · 15 can change the
   machine`. The `Allow destructive tools` checkbox sits under the `0` as if it had nothing to
   allow. **Cost:** the person minting a fleet token cannot tell whether the destructive tools are
   in scope; the page and the device's `allow_destructive` gate disagree at the moment of minting.
   **Fix:** use the registry's destructive flags, or the last live catalogue, for the fleet count;
   say `the count is read from a machine` when the flags are unknown, never `0`. **S.**

2. **The hub's day line is four calls longer than its own fold can reach.** *(Done 2026-11-06, f5b42f5, live 0.3.273-ux76: the hub keeps `/fleet`'s day pair (count + failed) and draws a repeats sentence beside the fold whenever the merged window drops repeats and no cut-window sentence already names them; the audit's shape now reads `Today 183 client calls · 8 failed · last 22:51`, `Show the rest of today (119 more)`, `4 repeats not drawn.` (60+119+4=183). 741 tests (+2), gate PASS 41 steps, a11y 0/3, `build-web --check`. Live: the hub draws `36 repeats not drawn.` beside its fold — the console's own reads are that population, while the day line stays the client calls' pair; screenshot /tmp/live-ux76-hub3.png.)* The hub read
   `Today 183 client calls · 8 failed · last 22:51` with `Show the rest of today (119 more)` over
   60 rows; 60 + 119 = 179, and clicking drew 179. The window held 183 client lines and four
   repeated `(ts_ms, tool, status)` keys; the merge drops them and the hub says nothing, while
   Activity's cap sentence names its repeats. **Cost:** the one number and the list under it cannot
   be reconciled; four calls disappear after the operator asks for the rest of the day. **Fix:**
   draw the hub's repeats sentence whenever the window drops repeats, not only when it is cut; or
   count the day line from the drawable rows, one rule with Activity. **S.**

3. **At 390 the Runs history is a 320 px table with an 80 px Command column.** *(Done 2026-11-06, e43e30c, live 0.3.273-ux77: at ≤639 the history is a card list — the command full width and wrapping (294×70 in the live check), the header `display:none`, Exit/Duration/Started/stdout/stderr as labelled lines, and no sideways scroll (document 390/390); the 1440 table is unchanged (header 1,004 px, command 655 px, row 58 px). 743 tests (+2), gate PASS 43 steps (a 1440 table leg and a 390 card leg), a11y 0/3, `build-web --check`. Screenshot /tmp/live-ux77-runs390.png.)* The first run's
   command is 283 px tall (`powershe / ll.exe - / NoProfil / e - …`), the `stdout / stderr` header
   wraps in a 52 px cell, and at that row height the 20-run history is a 5,660 px column. At 1440
   Command is 679 px and a row is 57 px. **Cost:** the phone's run history cannot be scanned or a
   command identified; the page's stated job — "Choose a command to read its record" — fails.
   **Fix:** at ≤639 card each run: command full width and wrapping, then exit, duration, started,
   output sizes; hide the header. **S.**

4. **At 390 the token card squeezes the token's name to 26 px.** *(Done 2026-11-06, b2a1836, live 0.3.273-ux78: at ≤639 the card is one column — the name full width on one line (live 349×20, was 26×176 over 9 lines), the scope sentence under it, the Rotate/Revoke row and the dates kept, document 390/390 and the list 349/349 (no sideways); above 640 the desktop card is untouched. 743 tests, gate PASS 44 steps (the audit's fleet-token shape; the 390 name box > 200 px), a11y 0/3, `build-web --check`. Screenshot /tmp/live-ux78-tokens390.png.)* The card grid puts the name in
   `minmax(0, 1fr)` beside an `auto` scope; a three-machine scope is 337 px, the name cell 0 px and
   `.token-name strong` 26 × 176 px. `zss-harness-20261106-fleet` reads `zs / har / nes / s- / 202 /
   611 / 06- / flee / t` and the row is 414 px tall; at 1440 the same row is one line. **Cost:** on
   the phone the operator cannot tell which credential they are rotating or revoking, the one
   field that identifies it being unreadable. **Fix:** at ≤639 make the card one column — name full
   width with the scope under it — or cap the scope cell and let it wrap; keep the action row.
   **S.**

## P0 — the fifteenth list (opened 2026-11-06, from the audit of 0.3.273-ux78)

Measured on the live console at 1440×960 and 390×844, Asia/Shanghai, selected machine 9VH92BJ
(0.3.284), with 14RJCR8 offline (0.3.275), 84T3LIN 0.3.280, the device panel at 127.0.0.1:18090,
and both MCP endpoints exercised with live tokens. The fourteenth list's four items hold. Ranked by
the setup failure first, then honesty, then the phone, then recovery.

1. **The two MCP endpoints take different token kinds, and the token list never says which endpoint a
   token belongs to.** *(Done 2026-11-06, 1302298, live 0.3.273-ux79: `tokens.json` records carry `kind: device|fleet` and the list/create/rotate metadata carries `TokenMeta.kind`; an old file infers the door only from the `device_ids` shape (a contradicting kind quarantines the file), and the web prints `unknown · endpoint not recorded` when no fact proves it. The Scope cell prints `fleet · /mcp/all` vs `device · /mcp/<short>`; the Endpoint/Clients copy names the URL for the sheet's kind; a wrong-door live token gets the additive `message` naming its door. 411 console tests, 751 web tests, gate PASS 44 steps, a11y 0/3, `build-web --check`. Live: a fleet probe at `/mcp/<9VH>` answered `401 {"error":"unauthorized","message":"this token is for /mcp/all; a device endpoint takes a device token"}` and 200 at `/mcp/all`; the Tokens list reads `fleet · /mcp/all` and `device · /mcp/4c718d6e44c8…` (screenshot /tmp/live-ux79-doors.png); the probe was revoked.)* `/mcp/all` prints a fleet-scoped token's config, `/mcp/<id>` a device-scoped
   token's. `test` (device, 9VH) and `e2e-connect` (aggregate with one device, 9VH) both read
   `DESKTOP-9VH92BJ` in the fleet Tokens door; the device token is **401** at `/mcp/all`, the
   aggregate **401** at `/mcp/9VH`, and the 401 is a bare `{"error":"unauthorized"}`. **Cost:** a
   token that names the machine is refused at the URL the Clients tab prints; the operator and
   client cannot tell the kinds apart and the first call fails. **Fix:** label `fleet · /mcp/all`
   vs `device · /mcp/<short>` in the Scope cell and the Endpoint/Clients copy; or accept a token
   whose recorded `device_ids` include the machine at `/mcp/<id>`; make the 401 name the door the
   token opens. **M.**

2. **The offline machine's token form counts the console's 46-tool fallback as the machine's.** *(Done 2026-11-06, fe74135, live 0.3.273-ux80: with 14R offline the form reads `46 tools (console registry; the machine is offline) · 15 can change the machine`, covers `DESKTOP-14RJCR8 (offline, last seen 1d 10h ago)`, and draws `The machine is offline. This form counts the static fallback; the console refuses a plugin tool it cannot verify.` above the presets; a known last live list is labelled as that. The online 9VH form is unchanged (`46 tools · 15 can change the machine`, plain coverage, no note). 754 tests, gate PASS 46 steps, a11y 0/3, `build-web --check`. Screenshots /tmp/live-ux80-14r-tokens.png, /tmp/live-ux80-9vh-tokens.png.)* With
   14R offline, `/fleet/14R/tools` is 503 and `/tools` is 46, but the form says `This token will
   cover DESKTOP-14RJCR8.` and `46 tools · 15 can change the machine`; the static-fallback sentence
   appears only under `Choose tools…`, and the coverage sentence omits the offline state the fleet
   form shows. **Cost:** the operator cannot tell the count is not the 0.3.275 machine's list; a
   token minted with all 46 names may name tools the device does not advertise. **Fix:** show the
   offline/static sentence on every preset, label the count as the console registry, and mark the
   coverage sentence offline; use the last live list when known. **S.**

3. **The update story is stale on the device panel and absent from the console.** *(Done 2026-11-06, 48d679b, live 0.3.273-ux81 / device 0.3.285: the panel reads `Last target 0.3.273`, `Last outcome applied`, `Last result 2026-11-05 03:11:05` (exact UTC in the title) and `Last check 2026-11-07 03:08:05` (UTC title 2026-11-06T19:08:05Z); the old `Target`/`Outcome` labels are gone. The Machine tab reads `running v0.3.285 · channel not reported · last check 2026-11-06 19:08 · last outcome applied to 0.3.273 on 2026-11-04 19:11` (missing facts are named, no invented source). The fleet marks `behind 0.3.285` on 84T3LIN v0.3.280 and CPH v0.3.284. 614 device tests, 766 web tests, gate PASS 48 steps, a11y 0/3, `build-web --check`. Screenshots /tmp/live-ux81-fleet.png, /tmp/live-ux81-machine.png; the 0.3.285 installer is on the console's /installer.)* The panel's
   `BUILD Version 0.3.284` sits above `UPDATES Target 0.3.273 / Outcome applied / Last result
   2026-11-04T19:11:05Z`, while `/api/panel`'s last check is today; `Target` is the last applied
   target, and the card does not say so. The console's Machine tab has no update line; the fleet
   row `84T3LIN v0.3.280` sits beside the 0.3.284 installer with no cue. **Cost:** the panel's
   target reads as an older current target; the operator cannot tell if a machine is current, which
   source it uses, or that 84T3LIN is at 0.3.280 while the installer is 0.3.284. **Fix:** label
   `Last target` / `Last outcome` and show `Last check` locally; render the fleet's
   update/channel on the Machine tab and mark versions behind the staged installer. **M.**

4. **An empty range's Export JSONL downloads a 0-byte file.** *(Done 2026-11-06, 3ae4194, live 0.3.273-ux82: the JSONL predicate is now `loading || rangeError || lines.length === 0`, so a chosen range with 0 lines disables both formats and a press makes no `/audit/export` request; a non-empty range keeps both enabled with the same attachment. 768 tests (+2), gate PASS 48 steps, a11y 0/3, `build-web --check`. Live: `Showing 00:00–23:59 of 11-08, 0 lines` leaves Export JSONL and Export CSV both disabled; screenshot /tmp/live-ux82-empty-range.png.)* `Showing 00:00–23:59 of 11-08, 0
   lines` leaves `Export JSONL` enabled while `Export CSV` is disabled; a press downloads a 0-byte
   `.ndjson` with no sentence. **Cost:** the export appears to succeed but carries nothing; an empty
   range is indistinguishable from a failed export. **Fix:** disable both exports on a 0-line range,
   or answer with the range's empty sentence. **S.**

5. **The offline machine's Tools tab says "Reading the tool list…" forever.** *(Done 2026-11-06, d8d586b, live 0.3.273-ux83: the header is state-derived — the count when loaded, `Reading the tool list…` only while loading, `No tool list from this machine; it is offline.` when finished empty and offline, `No tool list from this machine.` when finished empty online. Live 14R: no Reading, the offline header and the body's no-list sentence; 9VH: `46 tools · 11 capabilities`. 771 tests (+3), gate PASS 48 steps, a11y 0/3, `build-web --check`. Screenshots /tmp/live-ux83-14r-tools.png, /tmp/live-ux83-9vh-tools.png.)* At 9 s the header still
   reads `Reading the tool list…` above `The console has no tool list from this machine yet.`; the
   header keys on `tools.length === 0`, not `loading`. **Cost:** a read that cannot finish is
   announced as in progress; the operator waits or retries against an offline machine. **Fix:** use
   `loading` for `Reading…`, and the offline state for `No tool list from this machine`. **S.**

6. **At 390 the Device page hides its Connect button.** *(Done 2026-11-06, d29c45b, live 0.3.273-ux84: at ≤639 `.device-connect` is a visible 59×28 on the title row (right 374, document 390/390), named "Connect a client", and a click opens this machine's Connect sheet with its `/mcp/<id>` endpoint; at 1440 it is unchanged (59×28 at right 1358). 772 tests, gate PASS 50 steps (a 1440 measurement and a 390 visible/click leg), a11y 0/3, `build-web --check`. Screenshots /tmp/live-ux84-connect-390.png, /tmp/live-ux84-connect-1440.png.)* `.device-connect` is `display:none` at ≤639
   (the button exists but `offsetParent === null`); the only doors are the header's unlabeled account
   icon or Home. At 1440 the button is a visible 59×28 at x 1299. **Cost:** on a phone the machine
   page loses its one per-machine endpoint/credential action. **Fix:** keep a compact Connect control
   at ≤639. **S.**

7. **Screen → Browser shows the raw `chrome exited with exit code: 1002` error with no recovery
   sentence.** *(Done 2026-11-06, d466224, live 0.3.273-ux85: a launch-failed `browser.list` reads `The device's browser did not come up.` (or `The device's browser page endpoint did not answer.` for the timeout shape) with one hint, `Stop the browser, then choose Refresh pages to start it again.`; the raw answer (exit code, loopback port, stderr note) sits in a closed `<details>` named `The device's answer`, not the primary line. Live 9VH: the failure shows the sentence and the hint, and the raw text is not in the page text; unmapped refusals keep their own sentence. 774 tests (+2), gate PASS 50 steps, a11y 0/3, `build-web --check`. Screenshot /tmp/live-ux85-browser.png.)* The primary line includes `http://127.0.0.1:62544/json/version` and `no stderr
   output`; only `Refresh pages` and `Stop browser` are enabled, and neither is explained. **Cost:**
   the operator sees an internal failure and can only repeat the same call. **Fix:** map it to a
   sentence with a Stop/Retry/Restart action; keep the raw text behind evidence. **S.**

8. **The session card's Last input is `[I`.** *(Done 2026-11-06, 2f6fd3d (web) + 8b32e64 (console), live 0.3.273-ux87: the live `/events` evidence was `{"input":"[I","session":"session-1"}` — the console's sanitizer had stripped the ESC, so the web's typedText could not tell it was control-only. The console now reads the raw data and omits the input for a control-only write, keeping the session; the web's typedText stays as the second line. Live: a fresh pty session-2's card reads `No keystroke recorded for this session yet.` before and after the xterm focus, and `x · 2s ago` after typing. 783 web tests, 245 console integration + 157 lib, fmt/clippy, gate PASS 50 steps, a11y 0/3, `build-web --check`. Screenshot /tmp/live-ux87-card.png.)* Opening a pty with no typing shows `Last input [I 4s
   ago`; the xterm focus sequence `ESC[I` was written to the session. **Cost:** the card's first
   input fact is a phantom control sequence, not a keystroke. **Fix:** ignore control-only input in
   `lastTypedInput`, or label it; fall back to `No keystroke recorded`. **S.**

9. **The Machine tab says "Helper not running" when the fact is "not registered".** *(Done 2026-11-06, 96f9c28, live 0.3.273-ux87: the Machine tab reads `Helper | not registered this boot` for the fleet's `registered:false`, `not reported` when no fact exists, and appends pid/session only when the fleet reports them; `not running` is gone from the built page. 783 web tests (+3), gate PASS 50 steps, a11y 0/3, `build-web --check`. Screenshot /tmp/live-ux87-machine.png.)* The fleet read is
   `helper: {pid:null, registered:false, session:null}` and the Screen page says "has not
   registered"; the Machine tab's `not running` claims a process state the console did not read.
   **Cost:** the operator may hunt a stopped process; the two surfaces describe one fact
   differently. **Fix:** say `not registered` / `no helper registered this boot`; show pid/session
   only when known. **S.**

## P0 — the sixteenth list (opened 2026-11-06, from the audit of 0.3.273-ux87)

Measured on the live console at 1440×960 and 390×844, Asia/Shanghai, selected machine 9VH92BJ
(0.3.285), with 14RJCR8 0.3.275 and CPH4KB2 offline at tour time, 84T3LIN 0.3.280 online, and the
device panel at 127.0.0.1:18090. Ranked by the setup failure and the data loss first, then the
record, then the panel, then the smaller corrections.

1. **"Add a machine" in the machine menu mints a code the page never shows.** *(Done 2026-11-06, f43bb88, live 0.3.273-ux88: `addMachine` clears the selected machine (`choose(null)`) before minting, so the machine chip's Add a machine lands on the fleet (header `All machines`) with the server's code in the enrollment panel, Copy and the command; one press is exactly one `POST /enroll`, a live code in hand is shown again, and an in-flight mint is blocked. 785 tests (+2), gate PASS 50 steps, a11y 0/3, `build-web --check`. Live: the chip → Add a machine → 64-hex code `b6e70129a4…` + Copy + command, one POST /enroll; screenshot /tmp/live-ux88-addmachine.png.)* The menu item calls
   `POST /enroll` (200, 64-hex code, 15-minute expiry) and leaves the selected machine's hub on
   screen; the code appears only after `All machines` is chosen. **Cost:** the setup action looks
   dead; repeated presses mint more one-time codes; the operator cannot copy the command. **Fix:**
   clear the selected machine before minting (`choose(null)`) or show the code in a sheet. **S/M.**

2. **The Files editor discards unsaved changes when Close is pressed.** *(Done 2026-11-06, 4a395a3, live 0.3.273-ux89: a clean Close behaves as before; a dirty one opens a modal titled `Discard unsaved changes?` naming the file and saying the device keeps the last saved bytes, with Cancel (focused) and `Discard changes`; the viewer and draft stay until Discard, Cancel/Escape keeps them, and discarding sends no `files.write`. 787 tests (+2), gate PASS 50 steps, a11y 0/3, `build-web --check`. Live: the audit's 22 B file edited → Close shows the dialog; Cancel keeps the dirty editor; screenshot /tmp/live-ux89-close.png.)* `Unsaved changes.` is
   drawn, but `Close` clears the viewer with no dialog; reopening returns the original bytes.
   **Cost:** a file edit is lost in one click; the page's warning is not a guard. **Fix:** ask
   `Discard unsaved changes?` when `dirty`, or disable Close until Save/Discard. **S.**

3. **The Runs record's full command buries exit/duration/started.** *(Done 2026-11-06, fbbd71f, live 0.3.273-ux90: the record draws Exit/Duration/Started/Working directory/stdout/stderr directly under its header, and a long command is painted as a 109-char short form with `4,481 of 4,590 characters hidden.` and a `Show full command` toggle (aria-expanded/aria-controls) that swaps in the exact line; Copy command still copies the full. Gate: at 1440 the card is 1,551→461 px and the facts move y 1,400/1,433/1,466 → 275/308/341; at 390 2,264→519 and 2,106/2,139/2,172 → 104/137/170. 789 tests (+2), gate PASS 51 steps, a11y 0/3, `build-web --check`. Live: the audit's long run's record shows the hidden count, the toggle and the facts above the command (screenshot /tmp/live-ux90-record.png).)* The record paints the full
   4,590-character `-EncodedCommand` line: 1,190 px of a 1,551 px card at 1440, 1,866 px of a
   2,299 px card at 390; the facts begin at y 1,393/2,114. **Cost:** the record's one job is below
   the fold; the operator scrolls base64 to read the exit. **Fix:** `shortCommand` in the record
   with a `Show full command` toggle, or facts above the command. **S/M.**

4. **The panel's 200-row Timeline card buries Build, Updates and Connect.** *(Done 2026-11-06, 51c4465, live 0.3.286 on the 9VH: the cards are Console → Build → Updates → Connect → Timeline → Terminal → Diagnostics, and the Timeline draws its 20 newest rows with the rest in `<details class="timeline-rest">` (`Show the other 80 calls`, closed; all 100 rows in the DOM; the cut day reads `Today (continued)`). Live render of the 9VH's own 0.3.286 panel: Build y 427/561 (was 4,636/7,625), Updates 622/887, Connect 961/1,438, Timeline 1,239/1,848 (was 3,284/5,911) at 1440/390; screenshots /tmp/live-ux92-panel-1440.png and /tmp/live-ux92-panel-390.png. 617 unit tests, fmt/clippy, layout-gate 0 failures. The channel delivered 0.3.286 to the 9VH; the console door carries SummriseSetup-0.3.287.exe. The review render (100 calls) measures Build y 4,870→463 at 1440 and 7,908→598 at 390, Updates 5,065→658 / 8,234→924, the timeline 3,506→891 / 6,187→1,445, and the fold opens all 100 rows (nothing dropped). 617 unit tests, fmt/clippy, layout-gate 0 failures. The 9VH is still 0.3.285 (the channel's interval); the manual `--check-update` fetched 0.3.286 but the launcher refused: `staged artifact C:\Program Files\Summriseersions .3.286 is missing` — recorded as item 8 below.)* The panel's card order
   puts Build at y 4,636 (1440) / 7,625 (390) and Updates at 4,830/7,951; the timeline is
   3,284/5,911 px. **Cost:** the operator scrolls 4–8k px past the call log to see the build or the
   last check. **Fix:** fold the timeline after e.g. 20 rows, or move Build/Updates/Connect above
   it. **S/M.** (Device-side panel release.)

5. **The shared root's unreadable state keeps the write controls live.** *(Done 2026-11-06, 917cfa9, live 0.3.273-ux91: after the root's first listing fails, Create and Upload are disabled (Refresh and the Root select stay live) with the hint `New folder and Upload a file are unavailable until this root can be listed; Refresh tries again.`; a successful listing re-enables them, a readable root is unchanged, and a disabled submit cannot repeat the refusal. 790 tests, gate PASS 51 steps, a11y 0/3, `build-web --check`. Live: the Shared root shows the no-shared-root fact, the hint, Upload/Create disabled and Refresh enabled; screenshot /tmp/live-ux91-shared.png.)* With `no shared root is
   configured on this device`, `Upload a file`/`Create` stay enabled; an upload repeats the same
   refusal. **Cost:** the page offers an action that can only fail and repeats the error. **Fix:**
   disable write controls after a failed first listing (keep Refresh). **S.**

6. **The Runs record's Started is an age, not an instant.** *(Done 2026-11-06, 6fadbf7, live 0.3.273-ux91: the record's Started is a `<time>` with the local instant (`2026-11-06 22:19`), the exact UTC in its `title` and datetime, and the age beside it (`· 38s ago`); the instant does not move on refresh, only the age. 791 tests (+1), gate PASS 52 steps (the fixture asserts dateTime/title/text before and after a 1.2 s refresh), a11y 0/3, `build-web --check`. Live: datetime 2026-11-06T22:19:08.893Z, the title's exact UTC and the local text; screenshot /tmp/live-ux91-started.png.)* `21m 04s ago` becomes `21m 11s ago` six
   seconds later; the `<dd>` has no title/datetime. **Cost:** the record's start cannot be matched
   to a log or another record and changes while read. **Fix:** show the local instant with the
   exact title, age beside it. **S.**

7. **The rotate dialog omits what happens to the expiry.** *(Done 2026-11-06, eb92f9b, live 0.3.273-ux92: the dialog says `The new token's expiry is a fresh period of the same length, counted from this rotation — or never, if this token never expires.`; the result panel names the rotate response's own `expires_at_ms` (`This token expires <time>…` with the exact title, or `This token never expires.`), and the list's date-only cell is unchanged. 793 tests (+2), gate PASS 52 steps, a11y 0/3, `build-web --check`. Live: the Tokens sheet's Rotate dialog carries the sentence; screenshot /tmp/live-ux92-rotate.png.)*
8. **The manual update check cannot install the fetched release.** *(Implemented 2026-11-06, a057dc4: `--apply` now receives the staged file (`<install>\versions\<v>\summrise-device.exe`) instead of the version directory, and a missing artifact is reported by name without invoking the launcher or moving `current.txt`; 619 unit tests, fmt/clippy. 0.3.287 is published and its installer is on the console door. The live 9VH proof is blocked: a silent installer run stopped `SummriseLaunch` and the device went offline at ~23:02 UTC; the operator was mailed the exact state and fix (`sc start SummriseLaunch` or a reboot). The installer's silent start is item 9.)*

9. **The silent installer leaves SummriseLaunch stopped.** *(Done 2026-11-07, 8d4f554 + 0.3.289. The killing step was measured, not guessed: `sc stop SummriseLaunch` makes the **installed 0.3.279 launcher** exit, and its kill-on-close child job kills both the setup waiting in the stop wait and the app hosting the caller's `runs.exec` (the 502). The setup log stops at `stop: sc stop: exit 0`; the next StopRuntime line never lands. `SILENT_BREAKAWAY_OK` could not help: it lives in the launcher being installed, and an upgrade is stopped by the launcher it replaces. The relay (`SummriseSetupRelay`, onstart/SYSTEM, runs the same setup `/S /RELAYED` outside the job, then exit 3) is the fix, and the repair/shell task registrations' quoting was also fixed (they had never worked: `Command=C:\Program` + `Arguments=Files\Summrise\repair.cmd`, 0x80070002). LIVE on the 84T 0.3.280→0.3.289: `runs.exec` returned `installer-exit=3` in ~1 s (service RUNNING, no 502), the relay installed, `sc.exe query` is `STATE: 4 RUNNING`, the log ends `service: SummriseLaunch is RUNNING at the end of the install; exit 0`, and the 84T is online 0.3.289.)*

10. **A `/S` install still disconnects the device it runs through.** *(Fixed 2026-11-07, 8d4f554 + 0.3.289, proven live on the 84T: the installed launcher's kill-on-close job was the killer; the relay runs the setup outside the job and returns exit 3, the install completes, and the service ends RUNNING. A residual is item 11. The 9VH host still runs 0.3.288 and will take 0.3.289 by its channel; it was not `/S`'d again.)*

11. **The relayed install's shell task still fails to start.** *(Measured 2026-11-07 on the 84T 0.3.289: the relayed `/S` install logs `shell task run: 1` and "the system cannot find the file specified", so the shell-start task the installer registers is still not runnable after the quoting fix. **Fix:** check the task's actual Command/Arguments and the schtasks invocation in the relayed path, or drop the shell task if the launcher's own autostart covers it. **S.** (installer.)*

## P0 — the seventeenth list (opened 2026-11-06, from the audit of 0.3.273-ux92)

Measured on the live console at 1440×960 and 390×844, Asia/Shanghai, selected machine
DESKTOP-84T3LIN (0.3.280) for the device pages, with the 9VH 0.3.286 offline and 14R/CPH offline,
and the 84T's panel at 127.0.0.1:18090. Ranked by the outage's timeline first, then the only online
machine's update story, then the controls.

1. **The last start is dropped when it repeats the header's version.** *(Live 0.3.273-ux93: the 9VH's Machine tab and fleet row both read `started 0.3.288 · 6m ago` with the exact title `2026-11-07 06:31:57 (…Z)`. Implemented 2026-11-06, 5315dea: `startedStory()` dates a start verdict from `host.restarts[0].at_ms` even when the version repeats the chip; the fleet row and the Machine tab draw `started 0.3.286 · 24m ago` with the exact title, and a `last_update` older than the start is labelled `last recorded outcome before this start`. 798 tests, gate 53 steps, a11y 0/3, `build-web --check`. The live check is blocked: `console-deploy.sh` fetches and swaps the console binary through `runs.exec` on the host device (the 9VH), and that device is offline, so ux93 cannot reach the console until the operator restarts `SummriseLaunch`; the code is committed and the console is ux92. Recorded as item 7 below.)*

2. **The 84T's update story says "channel not reported" and never names its verdict or 14 console retries.** *(Implemented 2026-11-07, b8bad5d: `updateChannel()` reads `host.restarts[0].channel` (falling back to `update.current.channel`), the Machine line says `verdict ok · started 0.3.280 · 41m ago` when `last_update` is absent, the device's `console_attempt` (last_attempt) is drawn and `/fleet` serves `channel_activity {joined,left,window}` from the audit as the fallback, and the Machine tab names the panel's `Check for updates now` as the local check. 806 tests, gate 55 steps, a11y 0/3, `build-web --check`, console Rust tests. The live check is deferred: the console deploy is blocked by the 9VH's outage (item 7).)*

3. **Screen capture is refused by the helper's disconnected session, but the page predicts only "locked and disconnected" in general.** *(Live 2026-11-07 on the 84T: the helper's session 2 is disconnected, Capture screen is disabled, and the sentence names session 2 and the way out. Implemented 4a84d46.)* The helper is registered in session 2; #2 is
   disconnected; `Capture screen` is live and answers `screen.capture: the capture thread could not
   attach to the input desktop: 拒绝访问。 (os error 5)`. The facts are on the Machine tab as
   `Helper session 2` and `#2 disconnected`. **Cost:** a daily action is invited into a refusal the
   page already knows; the error has no next step. **Fix:** pass the helper's session/state and say
   it before the press; disable or annotate the button. **S/M.**

4. **The Terminal session card says "No keystroke recorded" after a reload.** *(Live 0.3.273-ux93 with a fresh pty: a typed `echo audit40-terminal` draws `Last input echo audit40-terminal 3s ago` (the split joined whole), and a reload says `No keystroke recorded since this page opened.` — the scoped sentence. Implemented 5605033.)* At 390 the card reads
   `session-3 / opened 9m 14s ago / No keystroke recorded for this session yet`, while the xterm
   shows `echo audit37-terminal`; at 1440 the card's LAST INPUT is the one-character-short fragment
   `cho audit37-terminal`. The card reads only the live 200-line feed. **Cost:** the first block on
   the Terminal page is false after a reload; the audit has the evidence. **Fix:** read the
   session's audit evidence, or say "since this page opened"; accumulate the line. **S.**

5. **The fleet card says "calls" when it counts client calls only.** *(Live 0.3.273-ux93: the fleet row reads `146 client calls · 9 failed`. Implemented 2f5763a.)* The 9VH reads
   `141 calls · 4 failed` beside `Console reads · 238`; the 84T's hub reads `Today 3 client calls`
   beside `Console reads · 37`; the hub's day line says `client calls`. **Cost:** the row undercounts
   the machine's work and makes the hub's client count look contradictory. **Fix:** `141 client
   calls`. **S.**

6. **The offline hub tells the operator to open the machine's panel, which it neither links nor addresses.** *(Live 0.3.273-ux93: the 14R hub's notice carries the `Device → Machine` link (`#device/machine`) and `http://127.0.0.1:18090/` as code text, Retry intact. Implemented 2026-11-07, ab7db34: the offline notice says `The console keeps trying; Retry works from here. If it stays offline, open Device → Machine in the console for the machine's last facts. The machine's local panel is http://127.0.0.1:18090/ — it opens only at the machine.`; Device → Machine is a real `#device/machine` anchor (the gate clicks it and lands on the Machine tab), the panel URL is code text, and both branches carry the advice. 821 tests, gate 58 steps, a11y 0/3, `build-web --check` exit 0 after the commit. The live check waits for the 9VH.)*

7. **The console's own deploy needs its host device's shell.** *(Live 2026-11-07: the deploy printed `the host device's door is open: DESKTOP-9VH92BJ is connected; the fetch and the swap will run through its shell`; the swap's answer was lost in the console restart and the script polled to 0.3.273-ux93. Implemented 2026-11-07, a4ee2f4: `console-deploy.sh` preflights the host device's door (/health + a read-only state.get through /mcp) before building or fetching; a 503 stops before anything is built and reads `the host device's door is closed; nothing was built or fetched: a console deploy fetches and swaps through DESKTOP-9VH92BJ's shell ... /mcp answered 503 device reconnecting, retry in 1000 ms`. The script POSTs /mcp itself so the error body survives; a fetch/digest/swap refusal after preflight reuses the words; a swap refusal after the stop reads `outcome unknown, poll`. A loopback harness covers reconnecting/open/console-down and is wired into CI; `bash -n`, the harness and the yaml check pass. No local/service path exists: the console exposes no command/file route and the Linux box's only shell on the host is the device channel. The open-door live path waits for the 9VH.)*

8. **The hour after midnight breaks the last-hour window.** *(Fixed 2026-11-07, d42b9cb: the production `presetRange("hour")` was already instant-based (6e37747); the three failures were the checks/fixture — the App test and the gate asserted a single-date sentence, and the HomePage `todayCalls` fixture sat at 01:00 (in the future until then). The checks now assert the requested range (the gate mirrors `rangeText`), the fixture is anchored to `now` and compressed toward midnight, and `auditTime.test.ts` pins 00:30 (23:30–00:30 spans both days), 12:00 and Today's 00:00 boundary. 824 tests, gate 58 steps, a11y 0/3, `build-web --check` exit 0; a 00:30 harness run passes App+HomePage (114 tests) and the gate.)*

9. **The first seconds after midnight turn relative fixtures into yesterday.** *(Fixed 2026-11-07, e84cadf: the three HomePage fixtures now pin Date.now() to a fixed local 2026-11-05 14:45 with try/finally; production code and assertions unchanged; a harness clock at 00:00 reproduced 3 failed | 50 passed before and 824/824 at 00:00, 00:30 and 12:00 after; typecheck, 824 tests, gate 58 steps, a11y 0/3, `build-web --check` exit 0, dist unchanged.)*

## P0 — the eighteenth list (opened 2026-11-07, from the audit of 0.3.273-ux92)

Measured 2026-11-07 03:22–05:15 UTC on the live console at 1440×960 and 390×844, Asia/Shanghai, with
the 84T3LIN 0.3.280 online and the 9VH 0.3.286 / CPH 0.3.285 / 14R 0.3.275 offline; the 9VH's last
frame is 2026-11-06T22:40:26.841Z. Eight evidence-backed findings; the audit-37 items already carried
are not repeated.

1. **A pending or failed /fleet reads "No machines yet".** *(Live 0.3.273-ux93: a route-aborted /fleet draws `The console could not read the machine list.` + Retry, never `No machines yet` or the enrollment CTA. Implemented e670c7e.)* A slow or failed `/fleet` is drawn as
   `0 machines · 0 online` / `No machines yet` with the enrollment CTA; Dismiss leaves a clean empty
   page. **Cost:** the setup action is offered to a fleet the console simply could not read.
   **Fix:** pending says it is reading, failed says it could not read with the reason and a retry;
   `No machines yet` only for a confirmed empty fleet. **S/M.**

2. **The global error banner reads "The console could not read Failed to fetch".** *(Live 0.3.273-ux93: the same failure's banner names the machine list, no raw `Failed to fetch`. Implemented 03a57ee.)* Raw fetch text,
   no subject, no retry, and it can duplicate. **Cost:** the operator cannot tell what failed or
   what to do. **Fix:** name the failed read and offer retry; one banner per failure. **S.**

3. **The offline hub's Retry has no visible effect.** *(Live 0.3.273-ux93: the 14R Retry is disabled as `Retrying…`, then the same notice states `Still offline · last checked HH:MM.`; one press is one read. The first probe read a 160/220-char prefix and missed the appended outcome; the missing button-to-hook seam is now tested by HomePage.retry.test.tsx (523c9fc).)* The 9VH hub's Retry refetches but is
   byte-identical before and after (4,208 chars), with no spinner or result, indistinguishable from
   the 5 s poll. **Cost:** the one action on an outage page looks dead. **Fix:** show the attempt and
   its outcome (same result counts). **S.**

4. **The MCP door's 503 and the console's own tool read disagree.** *(Live 0.3.273-ux94: `/fleet/<14R>/tools` answers `503 device not connected` with `this machine is not connected since 1d 23h ago; connect it, then try again`; the online 9VH's tools read is 200. The MCP door now dates the silence the same way; the wrong-door 401 is item 5. Implemented 492a642.)* `/mcp` answers 503
   `retry-after: 1` / `{"error":"device reconnecting","retry_after_ms":1000}` for the 9VH after 5 h,
   while `/fleet/<id>/tools` says `this machine is not connected; connect it, then try again`.
   **Cost:** two surfaces describe one state differently; "reconnecting" cannot be true after 5 h.
   **Fix:** one sentence derived from the device's last frame, with the age. **S/M.**

5. **Missing, bogus and revoked tokens get bare 401s with no message or WWW-Authenticate;** *(Live 0.3.273-ux94: no token → 401 naming `/mcp/all` and the bearer shape, with `WWW-Authenticate: Bearer realm="summrise-console"`; a fleet token at a device door names the wrong door; `/mcp/deadbeef` → 404 `unknown_device` with the Machines-page hint; an out-of-scope device → 403 -32001 "not in the token's fleet. Mint a token that includes it". Implemented 492a642; the sign-in page's sentence is item 9.)* a valid
   token at `/mcp/deadbeef` gets "another machine's endpoint"; an unknown device's `state.get` gets
   -32001 "device not permitted by token"; bad operator sign-in shows "401 unauthorized". **Cost:**
   an AI client cannot tell a wrong door from a revoked secret from a typo. **Fix:** name the door and
   the reason; keep one shape. **S/M.**

6. **Activity claims it is following a failed audit.** *(Live 0.3.273-ux94: a route-aborted `/audit` draws `could not read the fleet's audit: Failed to fetch. This timeline shows only calls that arrive live.` + Retry, no "Following the console's audit". Implemented aca00df.)* When `/audit` failed, the page still says
   "Following the console's audit as it is written" (live-only rows, no counts, no hub-style notice);
   a slow `/audit` labels known machines `unknown machine · <hex>` and drops the selected header.
   **Cost:** the audit view lies about its source and loses the machine's identity while loading.
   **Fix:** say when the audit read failed; keep the selected header through the slow read. **S/M.**

7. **The offline Machine tab's last-frame facts read present tense.** *(Live 0.3.273-ux95: the 14R's Machine page says `Last seen 2d 0h ago. Nothing on this page can reach it until it reconnects; the console keeps trying on its own.` — dated from `last_seen_ms`. The 14R carries no `host` facts, so the fact block honestly reads "no machine facts from this device yet". Implemented 2662bd5.)* The 9VH's Machine tab shows
   `Sessions … #3 connected` and `Helper not registered this boot` from the 22:40 frame. **Cost:** a
   five-hour-old frame is read as now. **Fix:** date the fact block with the frame's age. **S.**

8. **An unknown selected id strands Activity.** *(Live 0.3.273-ux95: the fallback sentence and Clear work, but the timeline's chips still draw `unknown machine · deadbeefdead` and `unknown machine · fe209fc2d86a` — line-derived ids the fleet no longer lists. The goneId skip covers only the selected id; the broader line-derived chip is item 10. Partial: 2662bd5.)* A selected id not in `/fleet` leaves Activity on
   `unknown machine · deadbeefdead / No activity on deadbeefdead…` while Home/Device fall back; the
   fleet row menu's Remove is the real path. **Cost:** the page looks broken until the id is
   cleared. **Fix:** fall back like Home/Device and say the machine is not in the fleet. **S.**

9. **The sign-in page ignores the server's 401 sentence.** *(Implemented 2026-11-07, 7ee26f8, but the LIVE IS UNVERIFIED: the sign-in panel renders only when `session.authenticated` is false (the `!session.authenticated` branch); `#signin` is not a route, and the proxied console never drops the session, so no reachable path rendered the panel — `#signin` and a wrong token both left the page authenticated. The unit tests cover both 401 shapes (a message shown, a bare `error` falling back).)* *(Measured 2026-11-07: /auth's 401 now says "that token was not accepted; the operator token is the 64 hex characters in the file named by --operator-token-file", but App.tsx still draws "The console could not sign you in." with "401 unauthorized" in its title. **Fix:** surface the server's message. **S.** (web.)*

10. **Activity's line-derived chips label old ids as unknown machines.** *(Live 0.3.273-ux96: the chips read `All machines / DESKTOP-9VH92BJ / DESKTOP-14RJCR8 / DESKTOP-CPH4KB2 / DESKTOP-84T3LIN / old id · deadbeefdead… · 5h ago / old id · fe209fc2d86a… · 10h ago`; `unknown machine` is gone. Implemented 1a9f24f.)* *(Live 0.3.273-ux95: with the selected id deadbeefdead, the timeline's chips draw `unknown machine · deadbeefdead` and `unknown machine · fe209fc2d86a`; machineOptions' goneId skips only the selected id, and every line-named id the fleet no longer lists is drawn as "unknown machine". The audit's F8 wanted known machines not labelled unknown; the line-derived chip needs a history shape (a dated old id, not a claim of unknown machine). **Fix:** for line-derived ids not in the fleet, label them as retired/old (with the age of their newest line) or drop them from the chips; the selected gone id gets none. **S.** (web; Activity.)*

## P0 — the operator's direction (2026-11-07): 直观清晰一目了然

The operator, looking at the live ux96 console: **"改了这么久我怎么感觉页面没有什么变化啊，Home和Activity两个长得差不多啊"**
and **"我喜欢页面直观清晰一目了然"**. Measured on the live page the same day: Home is `Machines` + "4 machines ·
3 online" + an installer card + four tall rows whose right side is `199 client calls · 11 failed`; Activity is
`Activity` + five counters in one line + the same machine chips + search + a timeline. Both are gray page, white
card, 13 px text, a list of machines and a string of counters — the shape is the same, and neither answers "what
needs me?" before the eye reads four rows.

This outranks the copy items below. The two pages must become two different tools:

1. **Home is the verdict and the next action.** One large sentence at the top (e.g. "3 of 4 online · 1 needs
   attention"), a `Needs attention` block with one row per problem (offline since when, behind which version)
   and its action, then the fleet as a compact table (machine, status, version, started). The per-row `client
   calls · failed` counters leave Home — that is Activity's noun. The installer stops being the first card.
2. **Activity is the record.** One readable sentence for the day's counters (not five), the machine filter
   labelled as a filter, bigger day headings, a timeline rail that reads as time. No installer, no verdict.
3. **The visible shape must change, not just the words.** Type scale, spacing, status colour and section order
   are part of the fix; the live check shows the before/after screenshots to the operator.

**S.** (web; Home first, then Activity.)

## P0 — the nineteenth list (opened 2026-11-07, from the audit of 0.3.273-ux96)

Measured 2026-11-07 on the live console and the 0.3.289 devices (9VH moved to 0.3.289 at 10:04,
CPH at 09:53, 84T at 09:55; 14R offline at 0.3.275). Seven findings; the previous lists' verified
items are not repeated.

1. **The update story names an apply older than the running version.** *(Live 0.3.273-ux99 on 0.3.290: the 9VH's Machine line reads `last outcome applied to 0.3.290 on 2026-11-07 16:22`. Implemented bc1af1e, release 0.3.290.)* The 9VH reads `last recorded
   outcome before this start: applied to 0.3.273 on 2026-11-05 03:11` while on 0.3.289 (its restarts
   show 0.3.284→0.3.289 since); the CPH says `applied to 0.3.275 on 2026-11-05 15:21` while on
   0.3.289 (panel `Last target 0.3.275`); the 84T has no outcome/check at all after
   0.3.280→0.3.289. **Cost:** the update story cannot be read as history. **Fix:** record each
   applied version (the device's update record or the console's rendering of the restart journal's
   versions), so `applied 0.3.289` follows the install. **M.**

2. **An "old id" chip calls a live machine "not in the fleet".** *(Live 0.3.273-ux99: the 84T's chip is its fleet name; a 64-hex old id keeps its dated chip. Implemented b3773b7.)* The timeline draws
   `old id · DESKTOP-84T3… · 37m ago` with title `not in the fleet: DESKTOP-84T3LIN` beside the live
   `DESKTOP-84T3LIN` chip; the audit row carries `device_id: "DESKTOP-84T3LIN"` — a name from a 403
   `device_not_permitted`. **Cost:** the chip contradicts the row it sits on. **Fix:** only 64-hex
   `device_id`s get the retired-id shape; a name is matched to the fleet or left as the deny's own.
   **S/M.**

3. **A failed machine list hides enrollment and the installer.** *(Implemented b750c49; local pending/failed screenshots show the doors.)* Pending and failed `/fleet` states
   draw only `Tokens` — no Add a machine, Install the device runtime, or download. **Cost:** a
   console that could not read the fleet also removes every way to add one. **Fix:** keep the
   enrollment/installer doors reachable with the failure stated. **S.**

4. **One failed read is said three times and "Failed to fetch" is back in the page sentence.** *(Live 0.3.273-ux99: `Failed to fetch` exists only in titles. Implemented ca775dc.)*
   Banner + empty-state line + `The console could not read …: Failed to fetch.` + Retry. **Cost:**
   the page repeats one failure three ways and leaks the raw text the 18th list's fix removed.
   **Fix:** one sentence per failure; the technical text in a title. **S.**

5. **Activity counts client calls twice over two windows.** *(Fixed with the Activity redesign: one sentence, `client calls` once, `Clients only` labelled `rows in this window`. Implemented be4e510.)* `Today 199 client calls` beside
   `Clients only · 400` (the fetched window); pressing draws 397 rows and the header keeps 199.
   **Cost:** two numbers for the same noun; neither says which window it counts. **Fix:** one window
   per count, labelled, or a single number reconciled with the rows. **S.**

6. **The refused sign-in is a lowercase fragment in a plain paragraph, and the panel has no
   address.** *(Live 0.3.273-ux99: `#signin` renders `Sign in to the console` with `Back to the console`; a refused token is capitalized and `role=alert`. Implemented ca775dc.)* `that token was not accepted; …` sits in `<p class="error" title="401 unauthorized">`
   (no `role=alert`); `#signin` lands on `#home`; the panel is reachable only through Sign out.
   **Cos
t:** the sentence is unannounced and unfindable. **Fix:** a sentence, `role=alert`, and a `/signin`
   route (or the sign-out path linked). **S.**

7. **The channel trace counts joins/leaves without the window's span.** *(Implemented b739d09: `… in its newest 1,000 lines, spanning 17 h`; the 2000-vs-1000 claim is gone.)* `54 joins and 20 leaves … in
   its newest 2000 lines` while the 1,000-line window spans 17 h. **Cost:** the rate is unreadable.
   **Fix:** state the window's span (and the lines actually read). **S.**

## P0 — the twentieth list (opened 2026-11-07, from the audit of ux99)

Measured 2026-11-07 17:52–19:27 UTC on live 0.3.273-ux99; fleet 9VH 0.3.290, 84T 0.3.290, CPH 0.3.289
offline, 14R 0.3.275 offline. The nineteenth list is live; these are the shape problems the operator's eye
still finds.

1. **Home has two identities.** *(Live 0.3.273-ux100 with a machine selected: Home keeps `2 of 4 online · 2 need attention`, the grouped attention rows, its `machine-summary` section and the table; the old hub moved to `Device → Overview`. Implemented 139d7a1.)* With a machine selected, Home drops the verdict / Needs attention / table and
   becomes the old live timeline that still looks like Activity; the 14R/CPH problems vanish. **Fix:** Home keeps
   the verdict and the attention rows for the whole fleet, with the selected machine's own summary as a section —
   never a second Activity. **M.**

2. **The console host's repair task is still broken on 0.3.290.** *(Implemented 16d6e98; **live on 0.3.291/84T**: the relayed `/S` succeeded and `SummriseRepair`'s XML is `<Command>"E:\Program Files\Summrise\repair.cmd"</Command>` (registered 2026-11-08T04:46:13), the service RUNNING and the 84T online 0.3.291. The 9VH's stale task still needs a real setup run — the channel cannot refresh it.)* DESKTOP-9VH92BJ's `SummriseRepair` is
   `C:\Program` + `Files\Summrise\repair.cmd`, last result 0x80070002, every five minutes; the 84T's fixed task
   returns 0. **Fix:** find why the 9VH's 0.3.290 install did not re-register the task (the installer skips the
   re-registration on the "E:\Program Files\Summrise is already present; not written" path?) and repair it.
   **M.**

3. **The Machine tab's update story is a 12 px grey paragraph after the facts.** *(Live 0.3.273-ux100: the `Updates` card reads Applied 0.3.290 · Last outcome applied · Last result 2026-11-07 16:22 · Last check 2026-11-07 20:40 · Channel canary. Implemented c5e7359.)* The version is repeated three
   times, the applied clause is last, and the bare `online` is an orphan; the device panel's UPDATES grid is the
   clearer model. **Fix:** the UPDATES card's shape — target, result, when, applied — with one version per line.
   **M.**

4. **A failed read is still repeated.** *(Implemented 01d448e: one failure sentence with one Retry per failed read; the banner lost its Retry; the verdict no longer repeats it.)* Machines says it three times (banner, list stub, retry block) and Activity
   twice, with two Retry buttons; the raw `Failed to fetch` is gone, the repetition is not. **Fix:** one sentence
   and one Retry per failure. **S.**

5. **At 390 Activity's chrome pushes the first record to y=600.** *(Implemented 01d448e: retired-id chips collapse below 640 px except an active scope; 390 first record y=536.)* The filter chrome plus two old-id chips take two
   rows; the retired ids are already in the Table's device filter. **Fix:** at 390 the old-id chips collapse into
   the device filter; the first record starts above the fold. **S.**

6. **At 390 Home's attention rows push the table to y=644.** *(Implemented 01d448e: attention grouped per machine with one door; 390 fixture fleet card y=489.)* Four rows for two machines, four identical
   `Open machine` buttons. **Fix:** group by machine (one row per machine, its problems listed) and compact the
   actions. **S.**

7. **Device with no machine is headed `Terminal`** *(Implemented 01d448e: `Choose a machine`, no tab strip, no Connect, title follows.)* and shows the tab strip above `Choose a machine`. **Fix:** title
   the picker, hide the tabs until a machine is chosen. **S.**

8. **`#signin` while signed in leads with the token form** *(Implemented 01d448e: `You are signed in`, primary Back, token form behind a closed disclosure.)* although the sentence says "already signed in". **Fix:**
   lead with Back, put the token form behind a disclosure. **S.**

9. **`— 2 from the console` has no noun.** *(Implemented 01d448e: `2 console failures`.)* The 2 is console failures, not the 59 console calls. **Fix:** name it.
   **S.**

10. **`the console has 0.3.290` names the installer/release as the console** *(Implemented 01d448e: `the release is 0.3.290`.)* (the console build is 0.3.273-ux99).
    **Fix:** say the release, not the console. **S.**

## P0 — the twenty-first list (opened 2026-11-07, from the audit of ux100)

Measured 2026-11-07 20:47–21:22Z on live 0.3.273-ux100 at 1440 and 390; the 9VH applied 0.3.291 at
20:55:49Z during the tour, the 84T is 0.3.291, CPH 0.3.289 offline 11h09m, 14R 0.3.275 offline
2d12h. The twentieth list's items are live except its two residuals (this list's 5 and 6).

1. **Device → Overview is Activity a second time.** *(Implemented c149319; the band is 95 px with no rows, the timeline holds the calls once.)* Its Now band's newest five calls are the first five
   rows of its own Today timeline — the same ActivityTimeline rows under the same Today heading. **Fix:**
   Overview keeps the machine's state and its one next action; the calls live only in the timeline below.
   **M.**

2. **At 390 the Device tab strip hides Machine.** *(Implemented 0e3fd6b: the strip is 358/447, scrollLeft 87, active Machine at 302..374 in 16..374.)* clientWidth 358 vs scrollWidth 447; the active
   `Machine` sits at x=389–461 with scrollLeft 0, so `#device/machine` shows no selected tab. **Fix:** the
   strip scrolls to the active tab (or wraps) so the selected tab is always visible. **M.**

3. **At 390 the Connect sheet clips its instructions.** *(Implemented 0e3fd6b: the sheet is 390 px, body 389/389, endpoint sentence 349/349.)* Body 389 wide, content 440–596; the Tokens tab is
   cut to "Toke", the endpoint paragraph cut mid-sentence. **Fix:** the sheet fits 390 with no sideways
   clipping. **S.**

4. **Activity → Table is an 8-control filter form.** *(Implemented 49164b7: one 60 px band with one Export menu; `When` is the timeline's clock + age; at 390 a 43 px filter row and the first card above the rail; the 87k px page is gone.)* At 1440 there are two Export buttons in two rows and
   a relative-only `When` against the Timeline's clock; at 390 it is a 5-column grid in 320 px with its
   first row at y=746 cut by the tab bar (page 87,476 px). **Fix:** one filter row, one Export, the same
   time vocabulary as the timeline, a 390 layout that starts above the fold. **M.**

5. **The 9VH's repair task is still the old split-command registration on 0.3.291.** *(Live 2026-11-08: the 9VH took 0.3.292 by channel and the app re-registered the task at start; `schtasks /Query /TN SummriseRepair /XML` now reads `<Command>"C:\Program Files\Summrise\repair.cmd"</Command>`. Implemented cc09af7.)* `SummriseRepair` is
   `C:\Program` + `Files\Summrise\repair.cmd`, lastResult 0x80070002 at 21:15:01Z, every five minutes,
   registered 2026-11-06T01:45:52 — the machine took 0.3.291 by channel, which never runs the setup, so
   the task was never re-registered. The console shows no row for the task. **Fix:** re-register it on
   the host by a real setup run (or make the launcher repair a stale task on boot), and show the task's
   health on the Machine tab. **M.**

6. **The 84T's update card says "Last check not recorded"** *(Implemented 3815230: no launcher-written check exists for that apply, so the card scopes the absence as "not recorded before this update", dates the 9VH's real check/apply to the second, and drops the duplicate `Last outcome applied` row.)* while the 9VH's shows the check six seconds
   before its apply (20:55:43Z vs 20:55:49Z); "Last outcome applied" also repeats "Applied 0.3.291".
   **Fix:** read the check the launcher writes, and drop the duplicate clause. **S/M.**

7. **Every updated machine states one age twice:** *(Implemented 3815230: one age.)* `started 0.3.291 · 7m ago (updated 7m ago)` on Home and
   every device tab except Machine; `last_update.at_ms == restarts[0].at_ms` on both online machines.
   **Fix:** say it once. **S.**

8. **The offline Overview's advice contradicts its own card.** *(Implemented 3815230: the absence is dated via `/health` `uptime_s`.)* It says to open `Device → Machine` "for
   the machine's last facts", and that card says "The console has no machine facts from this device yet"
   (14R/CPH) — true only of the console process, restarted ~20:40Z. **Fix:** date the absence, or keep the
   last frame. **S/M.**

9. **Activity's "Today: 18 client calls" sits above "486 rows in this window"** *(Implemented 3815230: the window's span is labelled.)* (a 24h37m window whose span
   is stated nowhere) — 27× apart, same noun. **Fix:** label the window's span, or count the same window.
   **S.**

10. **The empty console says "Run the command" before the installer** *(Implemented 3815230: one zero state, the first-run/enrollment doors labelled and first.)*, shows the command last in an
    unlabeled panel, and says `No machines yet` + `0 machines · 0 online`. **Fix:** the enrollment/installer
    doors read as the next step, labelled; the zero state reads as one sentence. **S.**

11. **At 390 the Files "Filter this folder" floats on its own row at x=101** *(Implemented 3815230: aligned with the other controls.)* while every other control
    starts at x=33. **Fix:** align the control. **S.**

12. **Screen offers an enabled Capture screen on a machine whose helper it says has not registered** *(Implemented 3815230: Capture is disabled with the reason.)* and
    "a capture will be refused". **Fix:** disable it with the reason (as the 84T's disconnected-session case
    already does). **S.**

## P0 — the twenty-second list (opened 2026-11-07, from the audit of ux101)

Measured 2026-11-07 22:27–23:07Z on live 0.3.273-ux101 at 1440 and 390; 9VH 0.3.292, 84T 0.3.291, CPH
0.3.289 offline since 11-07T09:53Z, 14R 0.3.275 offline since 11-05T08:06Z. All twelve twenty-first-list
items verified live.

1. **Overview still draws Activity's day record.** *(Live 0.3.273-ux102: one `Today 29 client calls · 1 failed` day line + `All activity →`; 0 call rows. Implemented 76876e4.)* Under its Today heading it repeats the same rows
   (05:44:54 / 05:44:37 / `2 calls · 17 s`) the Activity timeline already holds; a 29-row card. **Fix:** one
   day line plus an All-activity link; the rows live in Activity. **M.**

2. **The version is printed twice on Home and missing where it matters.** *(Implemented 9592d54: Home's STARTED is the age only; the Machine start line is `started 0.3.292 · 2h ago`; the 390 topbar keeps the version.)* Home shows `VERSION v0.3.292` and
   `started 0.3.292 · 46m ago`; the Machine tab's start line has no version; at 390 the offline 14R/CPH Device
   pages have no version anywhere (the topbar collapses to the name). **Fix:** one version per page, present at
   390 and on the Machine tab. **S.**

3. **The same Connect action has two names and two places.** *(Implemented 9592d54: one `Connect a client` in the shell head on every tab.)* `Connect` on the six Device tabs, `Connect a
   client` on Overview and the Account menu, and it moves from the content to the shell header. **Fix:** one
   name, one home. **S.**

4. **The Table's line count is the window's, not the filter's.** *(Implemented 9592d54: the range sentence appends `N of the M lines match the filters`.)* `Showing the newest 993 lines` stays 993 with
   44 failed rows, and `29 lines` sits directly above `No calls match` at 390. **Fix:** say what the count
   counts, and that the filter found nothing. **S.**

5. **The tools read has no pending state.** *(Implemented 69c0eaf: reading / failed (one notice, one Retry) / ready in Overview, Tools and Connect; no zero from an unanswered read.)* A fresh 84T Overview shows `0 tools · 0 capabilities` for a
   46-tool machine; a failed read is two identical notices with two Retries on Tools, and Overview says
   `counts may be stale` while drawing none. **Fix:** reading / failed / ready states, one notice, one Retry.
   **S/M.**

6. **The installer strip's sha256 is truncated with no title or copy.** *(Implemented 9592d54: the full digest in the title + `Copy digest`.)* `sha256 0adcbd7c…`; the full digest is
   only at `/installer` (the downloaded file's sha matches it). **Fix:** a title with the full digest, or copy.
   **S.**

7. **The 84T has no Repair task row and no "not reported".** *(Implemented 7e1b0ca: `not reported by this build (v0.3.291)` for an online machine whose frame has no fact.)* The 9VH (0.3.292) shows it healthy; the 84T
   (0.3.291) shows nothing, which reads as no task rather than an older build that does not report. **Fix:** say
   the build does not report it. **S.**

8. **Runs' Started is relative-only.** *(Implemented 9592d54: `<time>` HH:MM:SS + age with the exact moment in the title.)* `31m 11s ago` with no title, while the Table uses clock + age for the
   same class of run. **Fix:** the same vocabulary. **S.**

9. **The Device picker's presence is colour-only.** *(Implemented 9592d54: the word visible, the version `v`-prefixed.)* Online/offline are a coloured dot with the words
   `.sr-only`; its version lacks the `v`. **Fix:** the word visible, the `v` kept. **S.**

10. **#signin's tab title says "Sign in" while the panel says "You are signed in".** *(Implemented 9592d54: `Signed in · Summrise console`.)* **Fix:** the title follows
    the panel. **S.**

## P0 — the twenty-third list (opened 2026-11-08, from the audit of ux102)

Measured 2026-11-08 00:15–00:57Z on live 0.3.273-ux102 at 1440/390. The twenty-second list is live; its
residuals are this list's 6, 7 and 8.

1. **`Open machine` only selects; the page stays Home.** *(Live 0.3.273-ux103: the attention door opens `#device/machine`, the fleet row opens `#device/overview`. Implemented 1ddaa30.)* Home's attention `Open machine`, every fleet row's
   `Open DESKTOP-…` and the machine-menu rows only select — the page stays `#home` at both widths; the only
   working door is the summary's `Open Device` → `#device/overview`. **Fix:** every door opens the machine.
   **S.**

2. **Home draws the selected machine three or four times with three vocabularies.** *(Implemented 1ddaa30: one presence phrase, the selected row highlighted, the summary repeats no identity.)* Topbar, attention,
   summary and fleet row; `Running` vs `online`; three age casings (`Offline — last seen`, `Offline · last
   seen`, `offline · last seen`); `0.3.291` appears four times on the page. **Fix:** one row per machine, one
   vocabulary, the selected machine highlighted not repeated. **M.**

3. **At 390 the fleet card's STARTED age has no label.** *(Implemented e0dd21e: a visible `.fleet-started-label`.)* The thead is `display:none`, so `online · seen 10s
   ago` and `2h 32m ago` stack 26 px apart as two bare ages — a regression of the twenty-second list's
   age-only change. **Fix:** a visible label at 390. **S.**

4. **Device with no selection says "No devices are enrolled yet. Add one from Home." while /fleet is still *(Implemented e0dd21e: reading / failed / empty states.)*
   reading or has failed.** A false empty fleet. **Fix:** reading / failed / empty states. **S.**

5. **The offline Overview says `Now · Quiet · no calls in the last minute` and gives no last-seen age**, *(Implemented e0dd21e: the offline Overview is offline and dated.)* while
   the same machine's Machine tab says `This machine is offline. Last seen 14h 31m ago.` **Fix:** the offline
   Overview is offline, dated. **S.**

6. **Activity counts one 9VH window three ways.** *(Implemented bc54d64: one count/noun per window.)* `353 rows in this window` (client-call rows), `891 lines of
   1040` (timeline footer), `994 lines of 1040` (Table, one tab away). **Fix:** one noun and one count, or say
   what each counts. **S/M.**

7. **Machine prints the version three times and never says whether it is current.** *(Implemented bc54d64: one version + `behind the release 0.3.280` / current.)* Topbar chip, start line,
   `Applied`; the 84T at the release 0.3.291 looks the same as the 9VH one ahead. **Fix:** one version + a
   current/behind/up-to-date answer. **S/M.**

8. **The clock is three clocks.** *(Implemented d06fcad: `Clock`/`Moment`, one spacing, titles everywhere, the Runs phone value cell.)* Table `08:46:18· 8s ago` with no space and no `title`; Timeline `08:14` at
   390 with no `title`; Runs `· 32m 39s ago` falls into the label column at 390 and `ago` strands at 1440 —
   the twenty-second list's own fix broke the phone grid. **Fix:** one clock format, one spacing, titles
   everywhere, the phone grid fixed. **M.**

9. **A tools failure is stated twice on Overview and three times on Tools.** *(Implemented e0dd21e: one notice, one Retry where it can work.)* Notice + line on Overview;
   `No tool list`, notice with the raw error `boom`, and the Plugins line on Tools; offline Tools adds a Retry
   it cannot use. **Fix:** one notice, one Retry only where it can work. **S.**

10. **The topbar machine menu is a second fleet table.** *(Implemented bc54d64: a switcher — name, state, build.)* It carries Activity's `29 client calls · 1 failed` and
    an unlabelled drive (`I: 23.9 GiB free`) where Home's rows dropped the counters and say `online`. **Fix:** a
    switcher, not a board. **S/M.**

11. **The Connect sheet prints the endpoint twice.** *(Implemented e0dd21e: one endpoint, one copy.)* The full 90-char endpoint in the code box and again in its
    paragraph (and `/mcp/all` likewise); at 390 that is eight lines of URL. **Fix:** one endpoint, one copy.
    **S.**

## P0 — the twenty-fourth list (opened 2026-11-08, from the audit of ux103)

Measured 2026-11-08 01:54–02:56Z on live 0.3.273-ux103. The CPH recovered to 0.3.292 during the tour (its ~15h
outage ended 00:51Z), so the fleet is 3 of 4 online. The twenty-third list is live; its residuals are this
list's 1-3.

1. **Every Device tab opens with `started … ago` before its answer** — on Overview it precedes the machine's
   name (y=129 vs 164). **Fix:** the start line belongs to the machine head, and only where it is the answer
   (Overview/Machine), not above every tab. **M.**

2. **At 390 the Device tab strip hides its first tabs with no affordance.** On Machine `scrollLeft=87`,
   `Overview` at x=-69, `Terminal` clipped at x=7. **Fix:** the strip shows a scroll affordance (or wraps) so
   the first tabs are reachable and visibly there. **S.**

3. **The failure banner covers the topbar** (banner 352,12,736,52 over a 52 px topbar), and the page states
   the same failure again with the only Retry. **Fix:** the banner below the topbar, one statement. **S/M.**
   *(Live 0.3.273-ux104: the failure banner is a grid row below the topbar — 0 `.alert-banner`s, one `.notice` + one Retry on a failed `/fleet`; the topbar keeps y=0 h=52 at 1440 and h=45 at 390)*

4. **Screen's disabled Capture gives its actual reason in paragraph 2**, after a locked-session note. **Fix:**
   the reason first. **S.**
   *(Live 0.3.273-ux104: `Capture is off: the per-user helper has not registered…` is the sentence beside the disabled button, `.screen-capture-group`; the locked-session legend stays after it)*

5. **The offline reason is stated twice on Screen/Files/Runs** (shell notice + tab sentence). **Fix:** once.
   **S.**
   *(Live 0.3.273-ux104: the tab lines name only what is missing (`No capture yet.`); the shell's orange notice owns the offline reason once)*

6. **The offline Machine dates its absence from the console's restart** (`since this console started at 09:52`)
   while its own notice says `Last seen 2d 18h ago`. **Fix:** the machine's own last-seen. **S.**
   *(Live 0.3.273-ux104: `The console restarted at 2026-11-09 12:27 and has not heard from this machine since; it was last seen 3d 20h ago.` — both dates, each labelled)*

7. **At 390 with a populated fleet, Add a machine opens the enrollment panel at y=1045 below the fold** and the
   press looks dead (visible at y=521 on the empty first-run page). **Fix:** scroll it into view. **S.**
   *(Live 0.3.273-ux104: at 390 with a full fleet the enrollment heading is focused at y=419 in an 844 px viewport (was y=1045, unfocused))*

8. **The Machine page never prints the OS/CPU/arch the fleet frame carries** (`Windows 11 · x86_64 · 12
   cores`). **Fix:** show them. **S.**
   *(Live 0.3.273-ux104: `Platform · Windows 11 · x86_64 · 12 cores` is the first row above Memory)*

9. **One action, four names** — `Open machine`, `Open Device`, `Open DESKTOP-…`, `Use`. **Fix:** one name.
   **S.**
   *(Live 0.3.273-ux104: the attention door and the summary read `Open`; the fleet rows keep `DESKTOP-… ›` with the accessible `Open DESKTOP-…`)*

10. **Activity's provenance line mixes audit.jsonl's 10-22 lifetime with the newest-1000 window**
    (`1000/1040/19 repeats/5.7 MiB/since 10-22`). **Fix:** separate the file's lifetime from the window's.
    **S.**
   *(Live 0.3.273-ux105: `Reading audit.jsonl — 5.8 MiB, lines since 10-22 18:55. This window: 11-07 14:41 → 11-09 12:40.` — the file's lifetime and the drawn window are two labelled facts, no shared arithmetic.)*

11. **`last result 0` on the Repair task row is a code, not a verdict**; the 84T's `not reported by this build`
    shows the row can speak English. **Fix:** `ok`/the reason. **S.**
   *(Live 0.3.273-ux104: `last result 0 (success)` · next 2026-11-09 12:33)*

12. **The CPH's ~15 h absence leaves no trace after recovery** — `started 27m ago`, `Applied 0.3.292`, a
    13-attempt retry note, but no `back after 14h 58m`. **Fix:** the recovery line. **M.**
   *(Partly shipped 0.3.273-ux105, and NOT closed: `RecoveryLine` draws `back after 41 m` from the console's own
   channel pair and the browser gate proves it on the fixture, but the live fleet serves `last_outage: null` on
   all four machines — the newest 1,000 audit lines carry 51 `device.joined` and 11 `device.left`, and every
   rejoin after a console restart is join→join with no close to pair, so the ~15 h absence the item measures is
   still unwitnessed. The remaining half is the DEVICE's own gap: the device knows when its channel dropped, so
   its next `device.joined` should carry `channel_gap_ms` and the console should record it. Device-side, ships
   with a device release.)*

## P0 — the twenty-fifth list (opened 2026-11-09, from the audit of ux105)

Measured 2026-11-09 04:26–04:52Z on live `0.3.273-ux105` at 1440 and 390. The twenty-fourth list is
live: 3–9 and 11 are ticked with their measurements, 10 is closed on ux105, and 12 is open with its
witnessed half named (the console can pair a close it saw; it cannot invent one it did not). The
four items below were each measured on the live page in this pass, and the two suspicions the audit
carried but did not confirm were **deleted from the list rather than written down as findings**:
both `Add a machine` doors mint **one** code (`POST /enroll` once; both doors show the same
`a572d1d9…`), and the account menu carries doors (`Connect a client · Tokens · Copy console URL ·
Documentation · Sign out`), not a second copy of the session.

1. **Home with a machine selected draws a card that never names the machine.** Measured at 1440
   with 9VH selected: `NEEDS ATTENTION` (14R, offline and behind the release) at **y=165**,
   `SELECTED MACHINE` at **y=334** — and that card's whole text is `46 tools · 11 capabilities ·
   Open · Connect a client`. The machine's name is only in the topbar and in the highlighted fleet
   row below it, so the card asks the reader to remember what they clicked. **Fix:** the card names
   the machine and states its presence in one line (`DESKTOP-9VH92BJ · online · v0.3.292`), and the
   counts follow. **S.**
   *(Live 0.3.273-ux107: `.machine-summary-who` reads `DESKTOP-9VH92BJ · online · 0.3.292` above
   `46 tools · 11 capabilities`; an offline machine keeps `offline · last seen 3d 21h ago`, because
   the card's next sentence is about the counts that silence withholds. `404b37e`. The age is
   deliberately still the row's, so the card names its subject and the row keeps the clock.)*

2. **Activity draws today as one 69-foot wall.** Measured: **888 rows** on the page, **692** of them
   under the `Today` label, `main` **73,740 px** tall at 1440, and four day labels
   (`Today / Yesterday / Saturday 11/7 / Friday 11/6`) under a heading that says
   `Today: 27 client calls, 3 failed.` The cap sentence — `newest 913 of 1040 (87 channel events, 27
   repeats not drawn)` — and the provenance line are **only reachable at the bottom, 70,000 px
   down**. **Fix:** a day is drawn as today's rows plus a count-and-door for the rest (`692 calls ·
   11:52 → 12:43 — open the day`), the cap sentence sits beside the count it qualifies at the top,
   and the full list stays behind the day's own expansion or the Table. **M.**
   *(Live 0.3.273-ux108: **180 rows drawn** (three days × 60), `main` **16,435 px** at 1440 — was
   888 rows / 73,740 px. Three doors, each carrying its day's own count and clock: `Show the rest of
   today (309 more)` · `369 calls · 10:02 → 14:42`, and the same for yesterday and Saturday. The cap
   sentence is the first thing in the record (`Showing the newest 937 lines of 1040 fetched …`) and
   `capAboveFirstDay` measured true. Pressing today's door draws 489 rows and leaves two doors.
   `d64c20f`.)*

3. **One credential list, seven rows that read the same.** Measured: seven tokens; four of them are
   `zss-harness-*` differing only in their device list and creation date; every row repeats
   `39 tools` and `destructive allowed` in the same position; the scope cell lists three names then
   `and 1 more`; and `Create a token` — with its full tool-scope form — sits **above** `Existing
   tokens`. The operator's question is *which credential do I hand out, and what does it reach?*
   **Fix:** the list first, one line per token (`name · scope count · last used`), details on the
   row's own disclosure; the mint form behind `New token`. **S/M.**
   *(Half shipped 0.3.273-ux109: the sheet opens on `Existing tokens` with the seven rows and a
   `New token` door; the form is one press behind it (`Create a token` appears only after the
   press), and an empty registry still opens on the form. **NOT DONE:** the rows are unchanged —
   each still prints `39 tools`, `destructive allowed`, three device names and `and 1 more`. The
   row's one-line summary and its own disclosure are the remaining half of this item. `4ee214d`.)*
   *(**The row is one line now, live 0.3.273-ux110** (`10bdb5d`): measured at 1280, seven summary
   rows of **89-97 px** (was 156-164) whose whole face is `⌄ zss-harness-20261109-fleet4 · 4 machines
   · used 2026-11-09 · fleet · /mcp/all · Rotate Revoke`; the table is 659 px where it was 1,141.
   The chevron opens the record, and that record names ALL four machines, the tool count with its
   destructive word, the id with Copy, and Created / Expires / Last used — `Last used 2026-11-09`
   beside `Expires never`, the two facts that used to read as one repeated word. `and 1 more`
   appears nowhere on the page (`andMore: false`). This item is complete.)*

4. **The rail says `Home`; the page it opens says `Machines`, and so does the window title.**
   Measured: `document.title` `Machines · Summrise console`, h1 `Machines`, rail `Home` — on the
   page the operator asked to be a verdict rather than a table. **Fix:** one name, `Home`, in all
   three. **S.**
   *(Live 0.3.273-ux106: `document.title` `Home · Summrise console`, h1 `Home`, rail `Home`
   (`aria-current="page"`); the h1 reads `PAGE_LABELS.home`, the rail's own map, so the three
   cannot drift apart again. `1cb2fd9`.)*

## The operator's own question, recorded (2026-11-09)

**"现在 console 为什么有多个 token，不能统一成一个 token 吗" — answered with the live measurement,
not with a design opinion.**

The console holds **seven tokens** (read from `/tokens` on ux105): four `zss-harness-*` fleet tokens
created 2026-11-04, -06 and -09; `dsh-aggregate` (2026-10-27, 3 machines, last used 2026-11-09
12:13); `e2e-connect` (created 2026-10-27, **never used**); and `test` (a single-machine token on
9VH, last used 2026-11-02). `dsh-aggregate` is the one this session's MCP client actually uses.

**Why the model has two kinds, and why that is not the defect.** A device token is the bearer at
`/mcp/<machine>`; a fleet token is the bearer at `/mcp/all` and carries a **fixed list** of machines
(`device_ids`), enforced where the device is chosen. The scope is recorded because the endpoint is
recorded: a token that only ever opened one machine's door must not silently become a fleet
credential. That is the same reason `TokenKind` exists at all — the 401 must name the door instead
of re-deriving it.

**What IS the defect, and it is the operator's actual friction.** Seven rows where four are the same
harness token minted again on a new date, one has never been used, and nothing on the page says
which one the AI is using right now. The list answers "what exists", not "which do I keep". The fix
is the fifth list's item 3 (list first, one line per token, `last used` beside the name) **plus**
one fact the list does not yet print: **the token this console is being read from**, when the
operator's own client is one of them. A credential list with "last used" and a footer row naming the
in-use credential is the shape that makes seven rows answer the question; a single unified token
would answer it by removing the choices the operator needs (rotation, per-machine blast radius).

**Cloudflare Access session vs Fleet token vs Device token** — three credentials, and they are not
interchangeable: Access is the *operator's browser* door (the page answers `via: proxy` here), the
fleet token is the *AI client's* door to every machine it was minted for, the device token is the AI
client's door to one machine. None can substitute for another; the console refuses each at the
wrong door by name.

## P0 — the twenty-sixth list (opened 2026-11-09, the twenty design rules, measured)

The operator's instruction: design by Refactoring UI's twenty rules, not by taste. This is the
measurement of each rule against the console as it is, so the list starts with what is **enforced**
(a named gate, so the rule cannot drift back), then what is **measured off** with its number.

**Already enforced — the gate is the rule, and it is `device/console/tests/tokens.rs`:**

| Rule | What holds it |
|---|---|
| 1 a system, not one-off values | `generated_tokens_match_the_canonical_sheet` (the sheet is generated from `device/ui`, so a hand-edit fails) |
| 3 fewer font sizes | `seven_type_sizes_and_a_thirteen_pixel_body` — 7 sizes, 10/11/12/13/14/17/22 |
| 4 fewer font weights | `every_weight_is_one_of_four_tokens` — 400/500/600/700, no literal `font-weight` in `app.css` |
| 5 hierarchy from size/weight/colour | `every_type_size_comes_from_the_seven_step_scale` (no 0.95rem escapes) |
| 13 readable text (contrast) | `every_text_pair_reads_on_the_surface_it_sits_on` — 15 named ink/surface pairs, AA measured, both themes |
| 13 readable text (terminal) | `the_evidence_output_reads_on_its_terminal_surface` |
| 18 motion respects the preference | five `prefers-reduced-motion` blocks, one of them a blanket; all four keyframes are named by their state change |
| 20 rhythm | `spacing_uses_the_rhythm_when_the_value_is_already_on_it` + `spacing_is_the_rhythm_or_a_documented_exception` (rem only) |
| 11 limited radii | `one_spacing_rhythm_and_the_canonical_radii` — the five canonical radii are asserted |
| 15 no pure black on light surfaces | `--ink:#18181b`, `--canvas` is a mix of ink, and the dark block is restated rather than inherited (`the_dark_surfaces_are_restated_not_inherited`) |

**Also held, not by a Rust gate: rule 19.** Empty / loading / failed states are asserted one by one
in `browser-gate.mjs` (the failed fleet read, the failed tools read, the offline machine, the empty
registry) and in the jsdom suites, because a state is a sentence and a sentence is measured where it
is drawn, not in the token sheet.

Measured gaps below. Each is the rule, its number, and the one shape that fixes it.

**A gate that was RED, and that nobody had run.** Before measuring anything else: `cargo test -p
summrise-console --test tokens` fails on this branch at
`spacing_is_the_rhythm_or_a_documented_exception` — a `2.25rem` in the phone audit card arrived with
`49164b7` and the file was never run after it, because CI runs `cargo test` on `main` only and this
round's gates were the web ones. **Fixed in this round** (`5ea5f18`): the length is a structural
measurement (measured at 390: the card is 320 px, the disclosure ends 29 px in, the clock's text
starts at 36), so it is now a documented exception with that number in its comment, and the file
passes 10/10. **The lesson to keep:** every round's gate list includes the Rust token gate, because
an untended gate is worse than none.


1. **The rhythm gate is blind to `px`, and 37 literals are already off the rhythm or already a
   token.** `spacing_uses_the_rhythm_when_the_value_is_already_on_it` scans **rem only**, so
   `padding: 2px 12px` passes while `padding: 0.125rem 0.75rem` fails — the same number, one
   spelling. Measured on `app.css`: **155 px spacing literals, 118 of them equal a `--sp-*` step,
   37 off the rhythm** (18px × 7, 7px × 5, 3px × 4, 9px × 4, 15px × 4, 5px × 2, 22px, 26px, 44px,
   68px, 220px, 26rem…). The 15px/18px pair is the timeline row's own padding, so the rows do not
   sit on the rhythm the rest of the page keeps. **Fix:** the gate learns px (a px literal that
   equals a step must be the token; an off-rhythm px is a documented exception or it moves), then
   the 15px and 18px are put on referenced steps. **M.** *(A gate change, so it lands with its
   mutation.)*
   *(**Done, live 0.3.273-ux112** (`7040079`): the test reads both spellings now. 92 declarations
   moved their on-step literals onto the tokens; the 121 on-step px literals went to zero, and the
   42 that remain are the documented structural lengths, each with its reason in the test comment
   (the timeline's 15/8 row padding and 18 px stretch gap, the 7/9/3 px control insets, the page's
   26/28/44 padding, the evidence's 22 px indent, 19/20/30/34 alignments, the sr-only `-1px` clip, the
   rail's `-16px` overlap, plus `calc()` arithmetic which is skipped). Both halves mutated: `8px`
   back panics naming `--sp-2`, a new `13px` panics as undocumented; restored, 13/13. The live
   timeline still measures 74 px rows and 120 rows — the conversion was pixel-identical.)*

2. **Eight of the fifteen border radii are literals, and `7px` is not even on the scale.** Rule 11's
   gate asserts the five canonical tokens exist; it does not stop a literal beside a token. Measured
   in `app.css`: 15 distinct values, of which seven are the scale's five steps and eight are
   literals — `7px` ×4 (the tab strip, the segmented control, the evidence door), `8px` ×3, `6px`
   ×1, `10px` ×1 (each equal to a step the token already names, so one radius has two spellings),
   `999px` ×3 (the canonical pill) and `1px` ×1. **Only `7px` is on neither the scale nor a token**,
   and it is the inset of a tray that is `--radius-sm2`. **Fix:** every `border-radius` is a
   `var(--radius-*)` or `0`, the segments take `--radius-xs`, and the gate asserts it. **S.**
   *(**Done, live 0.3.273-ux111** (`997fc17`): the eight literals are the tokens they already
   equalled and `7px` is `--radius-xs`; measured on the live page at 1440 the rendered radii are
   exactly `6px`, `8px`, `10px`, `14px` and `999px` — the five steps, in both the Home and the
   Activity trees. `every_border_radius_is_a_radius_token_or_zero` fails on the next literal, proved
   by re-adding `border-radius: 7px` and watching it panic with that line.)*

3. **Fifteen shadows where the system names six.** Rule 10. `tokens.css` declares `--shadow`,
   `--shadow-lg`, `--shadow-1`, `--shadow-2`; `app.css` adds `--elev-1`, `--elev-2` and `--ring`
   in a second `:root` block, and `--elev-1` is **used zero times** while `--elev-2` is used twice.
   Measured: **15 distinct `box-shadow` values**, of which three are literal
   `0 1px 2px rgba(0,0,0,0.1)` (the raised segment) and the rest are tokens. A shadow token nobody
   uses is a second palette waiting for its first use. **Fix:** one elevation family in one place
   (the generated sheet), the dead `--elev-1` deleted, the raised-segment shadow a token, and the
   gate asserts that every `box-shadow` is a token or `none` (`--ring` included). **S.**
   *(**Done, live 0.3.273-ux111** (`997fc17`): `--elev-1` deleted, the segment's shadow became
   `--elev-segment` (3 uses), and `every_box_shadow_is_an_elevation_token_or_none` now fails on a
   bare literal — proved by restoring `0 1px 2px rgba(0, 0, 0, 0.1)` and watching it panic. The two
   shapes it admits besides an elevation are a ring (any `var(--…)` colour at a length) and the
   keyframe's transparent outline, each named in the test's comment.)*

4. **Two literal colours outside the palette, and they are the terminal's own.** Rule 2/9. Measured
   in `app.css`: `#14161a` on `#d6dae0` for `.evidence-output` — an ANSI-dark surface and ink that
   the token sheet already has a family for (`--term-canvas`, `--term-white`), written as literals
   so they cannot change with the theme (they are meant not to) but also cannot be checked by any
   gate. The rest of the sheet's literals are `#000`/`#fff` inside `color-mix` (a ratio, not a
   colour) plus one fallback `var(--line, #e3e1dc)` that never fires in the React page. **Fix:**
   the two evidence colours become named tokens (`--evidence-bg`, `--evidence-ink`) declared beside
   the terminal family, and the contrast gate reads them instead of two literals it was given by
   hand. **S.**
   *(**Done on the gate, 0.3.273-ux116** (`8f50bc8`): both are tokens in `app.css`'s local block, the
   contrast test follows them (12.4:1), and a new `no_colour_literal_sits_in_a_colour_property`
   fails on any hex sitting as a colour property's value — proved by putting the two literals back
   (`a colour literal sits as a property value: background: #14161a; color: #d6dae0;`). **Honest
   boundary:** the LIVE check could not photograph the panel. All 115 openable rows of Activity's
   first day were opened on ux116 and **none drew `.evidence-output`** — the console's real traffic
   carries no text-output evidence in this window, so the pair is proved by the token gate and the
   browser gate's fixture, not by a screenshot of the operator's page. This item is complete as a
   gate, not as a picture.)*

5. **Nine letter-spacing literals beside the four tracking tokens.** Rule 14. `tokens.css` names
   `--tracking-display/-tight/-label/-eyebrow`; `app.css` also writes `-0.004em` (body), `-0.022em`
   (h1), `-0.014em` (h2), `-0.012em`, `-0.01em` ×4, `0.01em`, `0.02em`, `0.04em`, `0`. The h1's
   `-0.022em` is the *display* step's job done at the call site with a different number
   (`--tracking-display` is `-0.011em`, so the h1 is twice as tight as the token that names it).
   **Fix:** the heading tracking is the token's, the rest are the two literals a body and a small
   caps label genuinely need — or they become tokens. **S.**
   *(**Done, live 0.3.273-ux111** (`997fc17`): the h1 is `--tracking-display`, the h2 and five
   name/heading sites are `--tracking-tight`, `.files-kind`'s `0.01em` is `--tracking-label`, and the
   two documented exceptions are the body's `-0.004em` plus the small-caps `0.02em`/`0.04em`.
   `the_tracking_literals_are_the_tokens_or_a_documented_exception` fails on `-0.022em` put back,
   proved the same way.)*

6. **One opened timeline row draws three radii, two of them by hand.** Rule 11 with rule 7:
   `.evidence` writes `border-radius: 10px` and the row's own line writes `6px` — both literals at
   values the scale names — inside a sheet that is 14px. **Fix:** the nested box takes
   `--radius-sm2` (the step inside a 14px sheet) and the row's line `--radius-xs`, so a reader sees
   a sheet, a box in it, and a line rather than three unrelated corners. **S.**
   *(**Done with item 2** (`997fc17`): both were literals at named steps, so they became those
   tokens; nothing about the geometry changed and the live page's radii are the scale's five.)*

7. **The block that renders in the jsdom suite is not the block the operator meets.** Not a design
   rule but the reason several of the above went unseen: `device/console/web/src/tokens.css` is
   generated and gated, but `app.css`'s 15px timeline padding, 7px segments and 10px evidence are
   only visible in a browser. `browser-gate.mjs` measures layout where it already looks (the fleet,
   the sheet, the fold) and not the type/space/radius of the surfaces it visits. **Fix:** a
   measured pass in the browser gate that asserts the radius and shadow families **on the built
   page** at 1440 and 390, so the gate and the operator see the same numbers. **M.**
   *(**Done, live 0.3.273-ux115** (`89eba15`): the browser gate now collects every rendered
   `border-radius`, every text size and weight, and the distinct spacing lengths on Home and
   Activity at 1440 and at 390, and fails on a radius outside `6/8/10/14/20/999px`, a size outside
   `10-22px`, a weight outside `400/500/600/700`, or more than six shadow shapes. Measured: Home
   `10/14/6/8/999px`, 7 sizes, 4 weights, 2 shadows, 20 spacing values; Activity `14/6/8/999px`,
   6 sizes, 1 shadow; Home at 390 `14/6/8/999px`, 6 sizes, 14 spacing values. **Mutated**: a
   `border-radius: 13px` on `.fleet-machine` made it fail with `Home at 1440 draws radii outside the
   scale: ["13px"]`, and restoring it passes.)*

## P0 — the twenty-seventh list (opened 2026-11-09, from the page sweep at 1440 and 390)

A read of every page the console has, looking for what a reader sees that is not a fact. Measured
against the live console (`0.3.273-ux109`) at 1440.

### Two suspicions, disproved before they became items

**"The rail holds seven doors and only three are destinations."** The DOM says `nav.rail` contains
three `.rail-btn` plus `.account` and seven `button` elements, which looked like the account
popup's items leaking into the rail. Measured properly: the popup's four buttons are inside
`<details class="account">` with `open === false`; `document.body.innerText` does not contain
`Copy console URL`; hit-testing the centre of the (closed) `Tokens` button returns `NAV.rail`, not
the button; and forty sequential Tab presses never land inside `.account-pop`. The popup is
correctly closed, unpainted and unfocusable — **not a defect, not written as one.**

**"Device with no machine says `Choose a machine` and offers nothing."** The earlier snapshot read
only `p`/`.hint` paragraphs and found none. The page's `main.innerText` is: `Choose a machine` then
four rows — `DESKTOP-9VH92BJ online v0.3.292 Open`, the same for the other three — one `ul` and
four `Open` buttons. It does offer the choice. **Not a defect, not written as one.** (The rule this
round keeps: a suspicion is measured before it is written, and a disproved one is recorded here so
the next round does not re-run it.)

### The items the measurement supports

1. **The machine menu lists the machine it is already showing.** Measured on the device page at
   1440: the `.machine-menu` summary reads `DESKTOP-9VH92BJ online · v0.3.292`, and opening it
   draws five 34 px rows — `All machines`, then `DESKTOP-9VH92BJ online · v0.3.292` (**word for
   word the summary above it**), `DESKTOP-14RJCR8 offline · v0.3.275`, `DESKTOP-CPH4KB2 online ·
   v0.3.292`, `DESKTOP-84T3LIN online · v0.3.291`. A picker that offers the current item as a
   choice is one row too long, and it makes the operator wonder whether picking it would do
   something. **Fix:** the summary keeps naming the current machine; the list holds `All machines`
   and the *other* machines, and the current one is marked (`aria-current="true"`) if it stays for
   completeness. **S.**
   *(**Done, live 0.3.273-ux113** (`dad519a`): measured on the live page with 9VH in force, the
   summary reads `DESKTOP-9VH92BJ online · v0.3.292` and the rows are `All machines`,
   `DESKTOP-14RJCR8 offline · v0.3.275`, `DESKTOP-CPH4KB2 online · v0.3.292`,
   `DESKTOP-84T3LIN online · v0.3.291`, `Add a machine` — **five rows, four choices, no echo**. With
   no machine chosen every machine is still listed, because then every one is a choice. The browser
   gate fails if any row repeats the summary's machine and prints the rows it saw.)*

2. **Activity's record draws 150 borders for 8,270 characters of text.** Measured in `main` at
   1440 on the Timeline: 1,771 elements, **150 carrying a border**, 121 with a shadow, and the two
   largest contributors are `.timeline-row` (**118 `border-top`**) and `.timeline-caller`
   (**25 bordered pills**). Each row is a box with a rule above it and a pill inside it, so the
   record of what happened reads as a form to be filled in. Rule 7 of the design rules (spacing and
   grouping instead of borders and boxes) says the day's rows need one rhythm, not 118 rules.
   **Fix:** the day's rows lose their per-row rule and gain the existing 30 px group gap plus the
   stretch labels' rules; the caller pills stop being bordered chips (`.timeline-caller` already
   has the type). Measure the same three numbers afterwards — bordered, shadowed, and `main`'s
   height — and record them. **M.**
   *(**Done, live 0.3.273-ux114** (`b05e446`): measured on the same page at 1440, **150 bordered
   elements → 7** for 8,360 characters (the 118 row rules and the 25 caller pills are both gone; what
   remains is the search field, the stream bar, the two day-doors, the footer rule and a button).
   The caller is now `CONSOLE` in the page's small-caps type — uppercase, not colour alone, is what
   tells it from prose. `main` 10,921 → 10,816 px, a 105 px saving that is NOT the point: the point
   is that a 8,000-character record no longer reads as a form. The browser gate fails on
   `rowRules !== 0` or `callerPills !== 0` from now on.)*

## P0 — the twenty-eighth list (opened 2026-11-09, the fresh audit at 1440 and 390)

Both previous lists are closed, so this one starts from a measured read of every page the console
has: Home, Activity's Timeline at both widths, and the seven machine tabs (Overview, Terminal,
Screen, Files, Runs, Tools, Machine), with `main`'s height, line count, duplicate lines, clipped text
and button counts. Live `0.3.273-ux116`. **No console errors on any page at either width, and no
sideways scroll anywhere.** What the numbers did find:

1. ~~**The Devices bar stacks every tab's heading at the top of the tab strip.**~~ **DISPROVED before
   it was worked on, and the disproving is the finding.** My audit script collected
   `main.querySelectorAll('h2, h3')`, which returns hidden elements too. The heading is real in the
   DOM but it sits inside `<div class="page terminal-page" hidden>` whose computed `display` is
   `none`, its box is **0 x 0**, and `getClientRects().length` is 0 — so it is in neither the paint
   nor the accessibility tree. Measured per tab at 1440: Overview `visible:false,
   hiddenAncestor:"page terminal-page"`, the same on Files, Runs and Machine; on Terminal it is
   `visible:true` because there the section IS the page. **No defect. Nothing to fix.** The lesson is
   the audit's own: a DOM query is not a measurement; a heading counts as present only when it has a
   box.

2. **Two-thirds of the tool catalogue's descriptions are cut off with no way to read them.**
   Measured on Tools: **46 tools, 46 descriptions, 12 truncated at 1440 and 44 truncated at 390**
   (`text-overflow: ellipsis` + one line), losing **5,936 px of text at 1440 and 22,898 px at 390** —
   the longest description is 2,672 px wide in a 315 px box. `terminal.write`'s own text, which says
   what the tool can change, is unreadable on the phone. A catalogue whose entries can't be read is a
   list of names. **Fix:** the row's description gets two lines (a clamp, not an ellipsis) and the
   full sentence lives in the tool's own sheet, which already exists. **S.**
   *(**Done, live 0.3.273-ux117** (`cc43b9d`): measured on the operator's 46-tool page after the
   change, **cutToOne 0 at both widths** — 12 rows use their second line at 1440 and **44 of 46 use
   it at 390**, exactly the rows that were previously cut mid-sentence. `main` grew 3,848 → 4,064 px
   at 1440 and 4,072 → 4,864 px at 390: 216 and 792 px of text that used to be lost, not new
   content. The browser gate counts descriptions that collapsed to one line while their text still
   overflows and fails on any.)*

3. **The Machine tab draws 222 `Kill` buttons in a 235-row process table.** Measured at 1440:
   `Machine` has **236 buttons**, 222 of them `Kill` at 33 × 28 px, one per process row — the
   destructive action on every line of a list nobody reads (235 processes, `0.0 MB` for most). The
   button is 33 px wide, below the 44 px touch target the phone needs. A page whose most repeated
   control is "kill this process" tells the operator that killing processes is the point of the
   Machine page. **Fix:** one action per row is not the shape; a search finds the process you mean
   and the row's `⋯` menu holds Kill behind a confirmation naming the process. **M.**
   *(**Done, live 0.3.273-ux118** (`f4aeecb`): measured on the same page, 235 rows and **0 bare Kill
   buttons**, 222 row menus and 13 `Protected` rows — the count of killable rows is unchanged, the
   shape is not. `main` stays 1,970 px, so this bought no height: it bought the removal of 222
   buttons a reader could land on by accident, each of which opened a confirmation for a process
   nobody had chosen. The gate measures the same three numbers on its own eight-row fixture and fails
   on a bare button.)*

4. **Home's "Needs attention" says the release twice.** Measured: the block reads `The release is
   0.3.291. DESKTOP-14RJCR8 offline · last seen 4d 2h ago Behind the release.` — the version appears
   in the sentence and the condition is repeated as a second line, and the fleet row below says
   `v0.3.275 · offline · last seen 4d 2h ago` again. The operator's standing direction is that Home
   is the verdict and the next action: a block that states a fact three times and offers one `Open`
   is not a verdict. **Fix:** the block names the machine, the one condition and the one action; the
   release number appears once, in the install strip where the download is. **S.**
   *(**Done, live 0.3.273-ux119** (`6ca34b5`): the block now reads `The release is 0.3.291; the
   machines below are not on it.` and each row carries the machine's own distance:
   `DESKTOP-14RJCR8 · offline · last seen 4d 3h ago · On v0.3.275 · behind 0.3.291.` The build that
   was 445 px below in the table is now at y=275, in the sentence about it. When a machine reports no
   version the clause stays `Behind the release.` — nothing is invented for a machine that told the
   console nothing. The block is 211 px tall at 1440 (was 147): the 64 px is the number the reader
   came for.)*

5. **`App.test.tsx`'s "opens Runs on boot" fails intermittently under the full parallel run.**
   Measured: it passed alone three consecutive times, passed in four suite pairs, and failed in
   roughly half of the full runs. A test that fails one run in two is the same lesson as a red gate.
   **Fix:** find the shared state it depends on (the `beforeEach` history reset, or a module-level
   boot flag) and make it order-independent, or state why it cannot be. **S.**
   *(**Done, live 0.3.273-ux120** (`6e5dc4c`) — and the flake was a DEFECT, not a test. Measured
   first: five full runs failed one; eight more failed three, always in the same case. The cause was
   in `App.tsx`: the surface effect kept a run link's address only while `runsFocus` was set, and
   `RunsPage` clears that the instant it has opened the record (`onFocusConsumed`), so
   `#run/<device>/<run>` was rewritten to `#device/runs` whenever the read won the race — **a run
   link survived a reload only by accident of ordering.** The guard is the run link now. The new
   regression test fails on the old guard with the flake's own message
   (`expected '#device/runs' to be '#run/dev-workshop-0001/r-1'`) and passes on the fix; live on
   ux120 a `#run/<9VH>/r-nonexistent` link printed `h1 Runs` at boot and still held that address
   after the read settled. Two more tests that asserted at the wrong moment (TerminalWorkspace's
   attach, the gate's new process-table leg) wait for their promise now. **After: 6 of 6 full runs
   green, 1,042 tests.**)*

## P0 — the twenty-ninth list (opened 2026-11-09, the phone's targets and the shape of a row)

The twenty-eighth list is closed, so this one starts from a second audit on different axes: what
the phone can actually be tapped, what each state renders before and after a read, and whether the
rules a live region promises are kept where they matter. Live `0.3.273-ux120`, measured at 1440 and
390. **Three checks came back clean and are recorded as clean:** no console errors on any page at
either width; no sideways scroll anywhere (the audit table's wrapper is 320 px in a 390 px viewport,
`scrollWidth === clientWidth`); and `prefers-reduced-motion` is honoured — under `reducedMotion:
'reduce'` every transition resolves to `1e-05s` (192 elements), and the stylesheet carries the media
query.

1. **Every control in the phone's main area is under the 44 px touch target.** Measured at 390 on
   Home: **25 of 25** targets in `main` are shorter than 44 px, in four buckets — 26 px ×4 (the fleet
   row's name door), 28 px ×1, **30 px ×15** (Tokens, Add a machine, `Open …`, Copy/Rename/Remove,
   the account menu's items), 32 px ×5 (the `⋯` menus). The smallest is 24 × 28 px (the audit table's
   per-row opener) and the row's name door **156 × 26 px**. A thumb misses a 26 px line; the page
   even ships a 44 px `topbar-inner` rule and then never applies the same measure to the controls
   below it. **Fix:** the phone's main controls get a 44 px minimum (a `min-height` on the shared
   button/summary rules inside the phone query, not a per-component nudge), and the gate measures
   the count. **M.**
   *(**Done, live 0.3.273-ux123** (`98b1f4b` + `dd98bb7` + `c748985`): measured on the same phone
   after the rule, **Home 25 targets / 0 under, Activity's timeline 111 / 0, the Table 1,004 / 0, and
   the Connect sheet 1,030 / 0.** It took three commits because the cascade fought back: `.fleet-open…`
   zeroes `min-height` and `button { min-height: 28px }` is more specific than `main button`, so the
   rule is now the sheet's LAST statement and names each family that owns its own box (`.device-tab`,
   `.tab-main`, `.fleet-open`, `.seg button`, the two audit summaries, and `[role="dialog"]`'s
   controls). Measured while getting it wrong: `Tokens` stayed 30 px, the tabs 35 px, and a probe
   reported `min-height: 0px` — that is why the position is documented in the sheet.)*

2. **Home's fleet row is 103 px tall and only 11% of it is a door.** Measured at 390: each row is
   358 × 103 px; the name door inside it is 156 × 26 px (4,005 px² of 36,695), and tapping the row
   body at its centre hits `.fleet-status` — a **span** — so nothing happens (`#home` before and
   after; only the 26 px name navigates). On a phone the row is a card and the whole card looks
   tappable, but only a strip of its first line is. **Fix:** the row's surface is the door —
   the name button covers the card's body or the card itself is the button — and the `⋯` stays its
   own target. **S.**
   *(**Done, live 0.3.273-ux124** (`04e425f`): measured at 390 on the live page, every card's door
   covers **97%** of the card and the centre of **all four** lands on it; tapping a card's body at
   60% of its height lands on `#device/overview`, where before it stayed `#home`. The card is **77 px**
   tall (was 103): the absolute door collapses the cell's own box, so the phone's Home page got
   shorter as well as tappable. The `⋯` keeps its own 44 px. The gate fails on coverage under 95%,
   on a centre that misses, or on a menu that lost its own box.)*

3. **The audit table's phone rows are 26-57 px and their only opener is 24 × 28 px.** Measured at
   390 with the Table open: 1,004 targets in `main`, **1,004 under 44 px**; the rows are 57 px tall
   and the record's opener — `Show the record for runs.list` — is **24 × 28 px**, 999 of them. The
   table is the page's densest surface and its one per-row control is below every touch guideline
   in existence. **Fix:** with item 1's shared minimum the opener becomes 44 px; the row's height
   must then be re-measured so the table does not double. **M.**
   *(**Done with item 1** (`c748985`): the 24 x 28 px opener is 44 px now and the Table at 390 reads
   **1,004 targets, 0 under 44 px**; the row did not double — the table's total height moved
   65,533 → 65,559 px (26 px for the whole 1,001-row page, because the opener sits inside a cell that
   was already 57 px tall).)*

## P0 — the thirtieth list (opened 2026-11-09, one vocabulary and three sentences about one refusal)

The twenty-ninth list is closed. This one starts from three reads on axes the earlier audits did not
touch: **contrast measured per rendered text node** (not per token pair), **the product's own
vocabulary**, and **the Screen tab, which is the one place the console says "this will be refused"
before a press**. Live `0.3.273-ux125` at 1440 and 390. Two results were clean and are recorded as
clean: after the chip fix, **every text node on Home, Activity and the open token sheet meets AA**,
in both themes (58 / 657 / 62 / 51 nodes, 0 failures); and the machine band's phone overflow already
has its two named nudges (`Show later device views`, enabled only when there is something to show).

1. **The Screen tab says one refusal three times, in three vocabularies.** Measured at 1440 with the
   helper unregistered: `A capture of the signed-in user's desktop; locked and disconnected sessions
   refuse.` then `Capture is off: the per-user helper has not registered, so a capture will be
   refused. It starts in the signed-in user's desktop session; the Machine tab shows its state.` then
   `The picture comes from the per-user helper's session, so a locked or disconnected session is the
   device's refusal, not a black frame.` then `No capture yet. Capture screen takes one when you ask;
   nothing is captured on a timer.` — **591 characters for one page whose only control is a disabled
   button**, and the third sentence explains a failure mode the second already names. **Fix:** the
   page states the refusal once (the `Capture is off:` sentence, which carries the reason and the way
   out), the empty state says what the surface is for in one line, and the standing explanation
   moves under the picture as a caption rather than above the button. **S.**

2. **The product calls the same thing a `device` and a `machine`, in the same panel.** Measured
   across the seven machine tabs and both fleet pages: `machine` 53 uses and `device` 27, and the
   pairs are not different things — the Machine tab's own heading is `Device app` directly under the
   tab called `Machine`, its sentence says `Restart this device's app` and the process table says
   `236 processes on the device, by pid.`, while the tab strip, the fleet table and Home all say
   `machine`. In the tool catalogue it is worse than inconsistent: **11 of 46 descriptions say
   `device`, 1 says `machine`, and 33 say neither** — the catalogue's own per-tool sheet is the place
   a reader learns what the tool acts on. One noun, chosen, and the other retired. **M.**

## How a round goes

1. Read this file; pick the first item that is not done.
2. Implement it in the smallest shippable shape; write the tests that prove it.
3. `npm test`, `npm run typecheck`, `scripts/build-web.sh --check`, the token gate, the browser gate.
4. Build the console with a new `-uxNN`, swap it on the host, run the live check, screenshot.
5. Commit with the measurement and a `VERIFIED:` list. Tick the item here in the same commit.

## A decision C4 forced, recorded here

**Per-call approval of destructive tools does not exist in this console, and C4 deleted its last
vestige.** The `/next/approvals/*` routes were the only approval surface; the pending map nothing
populated, so no call was ever held. Destructive tools are allowed per grant (`allow_destructive`),
and the audit records the refusal when a grant does not carry it. If per-call approval is wanted, it
is a new feature with a real holding mechanism — not a page to restore.

## Not on this list, and why

- **A fleet-wide board.** The toolbar's picker is the model; the operator rejected a second
  navigation.
- **Decision events.** The console sees actions; the AI's reasoning is the client's to publish.
- **The cube.** A renderer for a page that does not exist yet; revisit after P0.

## P0 — the third list (audited 2026-11-05, live 0.3.273-ux43; items done as noted)

Measured on `https://console.saisi.online` with Playwright at 1440×960 and 390×844, 05:00–05:20
UTC. `/health` read `{"devices":3,"online":3,...,"version":"0.3.273-ux43"}` and `/session`
answered through the proxy. Every number below is from the page or its own `/audit?limit=1000`
read; nothing here is a suspicion, and the two lists above are not re-opened.

1. **The day count drops the calls its own token proves.** *(Done 2026-11-05, 327ea3a, live ux44: a line is a client call when the caller says so, or when the caller is absent and it carries a token — 14RJCR8 reads 363, not 62.)* Of the 1000-line window, 30 rows say
   `caller: "console"`, 62 say `"client"`, and **908 say nothing**; **376 rows carry a `token_id`**
   and **0 of the 30 console rows do**. Of the 908, 314 carry a token (client calls) and 594 carry
   neither (unattributed). Today `DESKTOP-14RJCR8`'s window has **940 rows and 363 client calls**
   (62 marked plus 301 token-only), but its hub reads **"Today 62 client calls · last 04:43"** —
   **5.9× low; 301 calls (82.9%) missing**. **Cost:** the one number the operator reads in the
   morning is wrong on the machine that worked, and 594 rows cannot be read as the AI's or the
   console's. **Fix:** a `token_id` is the client proof, absence of both caller and token is
   "unattributed", used by the day line, the row mark and the Activity filter; no new server field.
   **M.**

2. **The machine's day is 51,620 px and its capability line is at the bottom.** *(Done 2026-11-05, 9f17e51: capability line under the Now band; newest 60 rows with a real "Show the rest of today (N more)"; 51,127 px -> 3,731 at 1440, 51,135 -> 3,739 at 390.)* On
   `DESKTOP-14RJCR8` at 1440 the hub draws **935 rows** (one day) in a **51,620 px** `main`, and
   "46 tools · 11 capabilities · 0 plugins" sits at **51,599** — 56 screens down; at 390 it is 936
   rows, 51,684 px, the counts line at 51,669. Even the quiet `DESKTOP-9VH92BJ` spends 4,433 px on
   64 console rows at 390. **Cost:** the answer to "what can this machine do now" costs a 50k px
   scroll; the day cannot be scanned. **Fix:** move the counts line into the header and cap the
   hub's timeline (newest ~100, "All activity" for the rest). **M.**

3. **The run form is 2,940 px below the Tools tab.** *(Done 2026-11-05, 2dfb35f: the form precedes the catalogue — y 3,912 -> 115 at 1440, 4,093 -> 149 at 390; the catalogue keeps its order and search.)* Tools is **3,848 px** tall in a 908 px
   viewport; "Run a command on this machine" starts at **3,773** and the only path is past the
   46-tool catalogue (at 390: 4,070 px, 739 px viewport, 3,331 px of scroll). `runs.exec` is 139 of
   the 1000 lines. **Cost:** two clicks and three screens for the most-used write; Runs, the
   history, has no run control. **Fix:** put the form at the top of Tools, above the catalogue. **S.**

4. **The cap sentence disappears at exactly 1000.** *(Done 2026-11-05, c704084: "Showing the newest 1000 lines — the window's limit." in both views, from the fetched length; absent at 999.)* `/audit?limit=1000` returns **1000** lines; the
   page draws **997** (the merge drops 3 duplicates), sets `capped` false, and neither view says
   "Showing the newest 1000 lines; older ones are not on this page." The oldest row is 16:18
   yesterday. **Cost:** a 997-row slice reads as the whole record; the lines behind the server's cap
   are silently gone. **Fix:** set `capped` from the fetched window's length (`>= ACTIVITY_LIMIT`),
   not from the merged list. **S.**

5. **The biggest stretch is 470 rows and its summary arrives after them.** *(Done 2026-11-05, 7a2cb68: a stretch longer than 10 rows states itself above the rows; a one-call stretch has no label — 666 labels -> 333 in the 1000-row harness, 333 are single-call noise.)* Activity draws **997
   rows** and **64** stretch labels (05:12); **28 say "1 call"** and one says "2 calls · 0 s". The
   largest is **"470 calls · 51 m 30 s"**: 470 rows, 25,850 px, 46% of the 56,143 px timeline, and
   **360 of the 470 are "Read a terminal's output"**; the label is drawn after the 470 rows.
   **Cost:** the only summary of half the page cannot be used to skip the work it describes, and 28
   labels mark a single row. **Fix:** draw the summary at the top of the stretch, name the dominant
   tool, and fold a stretch above 50 rows from its label. **M.**

6. **The fleet row counts the attempts and hides the reason.** *(Done 2026-11-05, a8e67d7: "Reconnected after 3 attempts — HTTP error: 502 Bad Gateway", the device's words verbatim, one line with the full text in the title; absent when there is no reason.)* Two of the three rows read
   **"Reconnected after 3 attempts"** (`DESKTOP-9VH92BJ`) and **"Reconnected after 4 attempts"**
   (`DESKTOP-CPH4KB2`); the fleet read's `last_attempt.reason` is **"HTTP error: 502 Bad Gateway"**
   and **"the console accepted the channel and then dropped it"**, and neither string is on the
   page. **Cost:** a proxy 502 and a channel drop read as the same healthy reconnect; the operator
   cannot tell whether to look at the edge or the machine. **Fix:** append the reason to the muted
   line when the fleet carries one. **S.**

7. **Activity has no day count and no way to narrow to the AI.** *(Done 2026-11-05, c33296d: "Today 363 client calls · 12 failed · 587 unattributed" under the heading, and a "Clients only · N" chip; console rows have their own bucket and are never folded into unattributed.)* It draws **997 rows (980 today)** —
   363 client-proven, 30 console-marked, 587 unattributed, and 12 failed (the 12 are in the
   unattributed set; the 62 marked client calls are all 200) — and its header says only
   "Following the console's audit as it is written." The only client filter is Table → Token
   (`dsh-aggregate`): two clicks and a select; the timeline's four machine chips cannot separate the
   AI from the console. The newest row on the page is a console read (its own `plugins.list`).
   **Cost:** the page's stated job — what the AI did — takes a view switch and a guess. **Fix:** one
   "Today 363 client calls · 12 failed · 30 console reads · 587 unattributed" line and a Client
   calls chip using the same rule as 1. **S/M.**

### Cuts

- **C6. The Terminal page says "no session" twice.** *(Done 2026-11-05, 7fcbf21: the top empty card is gone; the sentence next to the session list stays.)* "No session is open on this machine." at
  y=228 and "No session is open on this device. New session opens a shell; it appears here once it
  is running." at y=336, 108 px apart; the second carries the action, the first is the page's
  duplicate. Delete the first. **S.**
- **C7. The hub's subtitle duplicates the account menu's console version.** *(Done 2026-11-05, bdc0775: the subtitle says Running/Offline; Account keeps the console build.)* "Running · console
  v0.3.273-ux43" is the page's one subtitle; the Account menu prints "Console 0.3.273-ux43". At 390
  the header chip drops the device's "v0.3.274", so the one build on the page is the console's,
  which the menu already has. Delete the version from the subtitle; keep the menu's. **S.**

## Updates, 2026-11-05: what is now true and what is not yet proven

**True, with evidence.** Three machines run 0.3.275 (9VH92BJ, 14RJCR8, CPH4KB2) and 14R's
`C:\SummriseInstall\current.txt` names `0.3.275` with `versions\0.3.275\summrise-device.exe` present —
the versioned layout is live, not planned. The canary channel is re-signed to 0.3.275 and
`scripts/verify-release.sh` passes (artifact digest, installer served, sha256 a693ab2a…). The
launcher's own `--migrate-layout` reports "already on the versioned layout", and the device's
`state.json` records `update {phase, to, outcome, lastResultAtMs}`, `lastVerdict`, a
`baseline.sha256`, and a `rolledBack` history — so checking, verdicts and rollbacks are not
theoretical.

**Not yet proven, and the next rehearsal.** No machine has yet walked from 0.3.275 to a newer
version *by itself* in front of us: that needs a 0.3.276 build published to the channel and the
device's own scheduled check to stage it, the launcher to probe it on a scratch port, the pointer to
flip, and `current.txt` to follow — with the old directory left behind for the rollback. Until that
run is watched, "zero-human update" is structurally true and experimentally unverified; this note is
here so nobody upgrades the claim without the run.

### The rehearsal, as far as it got (2026-11-05 08:21 UTC)

0.3.276 was published (installer served, channel pointer 0.3.276 read back from
`release-canary.json`). `SummriseRepair` was triggered on DESKTOP-14RJCR8; within the same minute the
machine left the tailnet (`tailscale status`: "offline, last seen 3m ago"; `tailscale ping` timed
out), so it is asleep or off the network, not a dead service — a dead service leaves the peer online.
**Unknown until it wakes:** whether the pointer flipped to 0.3.276 (the rehearsal passed) or stayed
at 0.3.275 (the probe refused). Both outcomes are safe by design: a refused candidate leaves the
pointer untouched. When the machine is back, read `C:\SummriseInstall\current.txt`, the `versions\`
directories, and `state.json`'s `update`/`lastVerdict`; that is the evidence, not the version in the
fleet alone.

### Why an installed 0.3.276 did not become current (measured 2026-11-05)

On DESKTOP-9VH92BJ after installing 0.3.276 (installer exit 0, health still 0.3.275):

```
C:\Program Files\Summrise\versions = 0.3.275, 0.3.276
C:\Program Files\Summrise\current.txt = 0.3.275
C:\Program Files\Summrise\versions\0.3.276\summrise-device.exe exists = True
```

The installer **did** stage the new version; the **pointer never moved**. The NSIS script runs
`summrise-launch --migrate-layout`, which on an already-migrated install answers "already on the
versioned layout" and does nothing — so every upgrade after the first migration installs files that
nothing points at. The installer understands the first migration and not the second upgrade.

**The fix:** after the files land, the installer must call the launcher's upgrade path —
`--install-version <versions\V> <sidecar>` (probe, then atomic pointer write) or `--apply` — and the
page must show the result. Until then, "installer exit 0" means "files copied", never "running".

### The real reason a staged version is never made current (measured 2026-11-05, host)

Running the launcher's own upgrade path by hand on DESKTOP-9VH92BJ:

```
summrise-launch.exe --install-version "…\versions\0.3.276" "…\versions\0.3.276\summrise-device.json" …
launch: version 0.3.276 failed its health check and was not made current:
cannot copy C:\ProgramData\Summrise\device.lock: another program has locked a part of the file
(os error 33)
EXIT=1
```

The probe copies the whole data directory to run the candidate on a scratch port, and the **running
app holds `device.lock`**. So the candidate can never start, the probe always fails, and the pointer
never moves — on every machine, every time. The NSIS script calling `--migrate-layout` is the second
bug; this is the first and it alone would block every upgrade.

**The fix:** the probe's copy must exclude the files the running app owns — `device.lock`,
`device.pid`, `device.port`, `restarts.jsonl`'s open handle — and give the candidate its own lock and
port file. A test must run a probe while the data dir contains a locked `device.lock` and still pass.

### The last gap after 182a064 (2026-11-05) — closed

The probe now survives a locked `device.lock`, the installer calls `--install-version` on a migrated
install, and it accepts the launcher-signed sidecar — so a staged version can be verified and the
pointer moved. But `--install-version` is **pointer-only by design**: after it returns, the running
service still serves the old version until its next start, and the setup does not restart it. So an
install would still end with `current.txt` naming the new version while the old process answers
`/api/health` — the same visible symptom as before, for a different reason.

**Closed by `installer: after the pointer moves, the process follows and the version is proven`.**
After a successful `--install-version`, `summrise.nsi` stops and starts `SummriseLaunch` with a
bounded wait, then polls `http://127.0.0.1:<port>/api/health` for up to 30 s until the body reports
`"version":"<setup version>"`. The launcher also accepts `--install-version --restart`, which
commits `current.txt`, restarts the owner through the same seam `--migrate-layout` uses, and
requires the restarted process to answer with the installed version. A restart or version mismatch
is non-zero; `current.txt` keeps naming the new version, `previous.txt` keeps naming the old one, and
the old version's directory is untouched — so the operator's rollback is one pointer write.

**The contract, in one sentence:** setup exit 0 ⇒ `/api/health` reports `${VERSION}`; setup exit
non-zero ⇒ the old version still serves (the launcher's start watch rolls `current.txt` back to
`previous.txt` after two failed starts, and the pointers are left in place for the operator's own
rollback).

## Updates, 2026-11-05 09:45 UTC: the upgrade path is proven on two machines

Two machines walked 0.3.275 → 0.3.277 through the fixed chain (stage `versions\<V>\`, probe the
candidate on a scratch port while the live `device.lock` stays locked, write the pointer atomically,
restart, require `/api/health` to report the new version):

- DESKTOP-9VH92BJ: 0.3.275 → **0.3.277**, online, and `/fleet` carries its own words —
  `update.current = 0.3.277`, `update.verdict = {outcome: ok, detail: started 0.3.277}`.
- DESKTOP-CPH4KB2: 0.3.275 → **0.3.277**, online.
- DESKTOP-14RJCR8: still 0.3.275 and **off the tailnet** (asleep since 08:21). It is not a failed
  update: a failed service leaves the peer online. Upgraded when it wakes.

The three bugs that made every earlier install a no-op are fixed and tested: the probe copied a
locked `device.lock` (os error 33), the installer only ever ran `--migrate-layout`, and a moved
pointer did not restart the process. The contract now holds: **setup exit 0 ⇒ `/api/health` reports
the installed version; non-zero ⇒ the old version still serves and the pointers are left for a
one-write rollback.**

## P0 — the fifth list (audited 2026-11-05, live 0.3.273-ux49)

Measured on `https://console.saisi.online` with Playwright at 1440×960 and 390×844, 09:46–10:15 UTC.
`/health` read `{"devices":3,"online":2,...,"version":"0.3.273-ux49"}`; `/fleet` had
DESKTOP-9VH92BJ and DESKTOP-CPH4KB2 on 0.3.277 and DESKTOP-14RJCR8 offline on 0.3.275, which
carries no `update` section. Every number below is from the page or from its own `/audit` answer. The folding
holds: 60 rows, then 108 after “Show the rest of today (47 more)”, then 112 after four live lines,
with the control gone; the capability line is at 410 px at 1440 and 426 at 390; a paused page says
“Paused” in the live chip and froze 949 rows for 11 s. The offline device with no `update` section
draws no update line — correctly: it said nothing.

1. **A tool's own refusal is written as a success.** *(Done 2026-11-05: the device answers a
   failed `runs.exec` with MCP's tool-level error — `{"result":{"content":[{"type":"text","text":
   "runs.exec: program not found"}],"isError":true}}` — and the console now reads that `isError`
   where it actually is: inside the device's JSON-RPC `result`, not at the top of the forwarded
   body. The previous unit and console tests built a bare `{content,isError}`, a shape no device
   emits, so they were green while `/audit` stayed 200 with no detail. A device that predates the
   rule and answers a `tools/call` with a JSON-RPC error envelope is a refusal too: 424 with the
   bounded envelope message, while ping/initialize/tools.list keep 200. The two console tests now
   send the device's real envelope and record the measured call as
   `{"tool":"runs.exec","status":424,"detail":"runs.exec: program not found"}`; the device tests
   pin the missing-program, exit-2 and timeout results and keep a malformed request an error
   envelope.)* At 10:00:17 on DESKTOP-9VH92BJ the Tools form
   ran `echo audit5-live` and answered **“Could not run the command: runs.exec: program not found”**
   in 4 ms; the same call is `/audit`'s 10:00:17 line — `runs.exec`, **status 200**, sentence “Ran a
   command”, no `detail` — and the timeline draws it “Ran `echo audit5-live` … console 4 ms” with no
   *failed* word. The browser already treats `result.isError` as a refusal (`terminal.ts:87-89`); the
   proxy treats every `Reply::Result` as `ok` (`http.rs:3113-3121`), and `run_detail` returns `None`
   for a result that is not a run, so nothing in the file records the refusal. **Cost:** the record
   cannot answer which of today's runs failed: the operator's failed run is a green row, and a
   client's `runs.exec` that returns `isError` is a successful client call that never reaches the
   failed count. **Fix:** when a `tools/call` result carries `isError: true`, write the device's
   message as `detail` and `ok:false` on the audit line and the live event (`AuditLine.ok` already
   exists), and let `auditFailed` read it. **M.**

2. **Activity's “M failed” counts every caller, beside a client-call count.** At 09:52 the header
   read **“Today 301 client calls · 55 failed · 21 unattributed”**; the page's own
   `/audit?limit=1000` answer (1000 lines, 949 drawn after 34 `device.left`/`device.joined` lines and
   17 duplicate `(ts,tool,status)` keys) holds **54 non-200 rows: 37 client, 15 console, 2
   unattributed** — and the header read 55, one failed row more than the fetched window held.
   **Cost:** 18 of the 55 failed rows (33%) are not the AI's calls; the sentence's grammar (“301
   client calls · 55 failed”) reads as a failure rate for the AI that the record does not support
   (37/301 = 12.3%, not 18.3%), and the hub's own line counts client failures only (“Today 283
   client calls · 38 failed” on 14RJCR8), so the word means two things on two pages. **Fix:** count
   the failures per bucket on the hub's rule and put the client's first — “Today 301 client calls ·
   37 failed · 21 unattributed · 15 console reads failed”. **S.**
   *(Done 2026-11-05: the sentence leads with the client's calls and the client's failures, names the
   console's failures as their own clause and states the failures among the unattributed rows in a
   parenthetical — “Today 301 client calls · 37 failed · 15 console failures · 21 unattributed (2
   failed)”. With `Clients only` on, the header is the hub's shape — “Today 301 client calls · 37
   failed” — and the chip's two caller counts are the two rows the list draws. Measured on the
   fixture: `Today 2 client calls · 1 failed · 1 console failure · 2 unattributed (1 failed)`, then
   `Today 2 client calls · 1 failed` with only the two client rows drawn; `npx vitest run` 602 passed
   across 51 files; typecheck clean; build clean; `./scripts/build-web.sh --check` clean.)*

3. **The machine that is down has no age, and the only absence it dates says it came back.**
   DESKTOP-14RJCR8's newest `device.left` is **08:18:06** (1 h 49 m at 10:07); `/fleet` serves
   `online:false`, no `last_frame_ms`, `last_seen_ms` 08:06:04 and `last_outage {away_ms:1009,
   back_at_ms:08:06:04}`. The fourth list's pair derivation (8449fc8) only closes an absence on a
   later join, and this one has no join after the 08:18 close, so `/fleet` serves the previous pair
   instead. The fleet row at 1440 and 390 reads **“DESKTOP-14RJCR8 offline v0.3.275”** with no age
   (45 px at 1440, 87 at 390); the device notice reads “This machine is offline. Nothing on this page
   can reach it until it reconnects…” with no age; the machine menu reads **“Offline · v0.3.275 back
   2h ago after 1s”**. **Cost:** the one absence the console witnessed (the close at 08:18) is on no
   surface, while the only line that dates an absence says a one-second one ended two hours ago. A
   machine down for two hours reads like one that just dropped, and the compare line says it is
   back. **Fix:** serve the newest `device.left` with no join after it (`audit.rs` already names that
   case “a device that is still away”) as the silence's start, age it on the row, the notice and the
   menu (“offline · since 08:18, 1 h 49 m”), and draw `last_outage`'s “back…” line only for an online
   machine. **S.**
   *(Done 2026-11-05: `/fleet` serves `last_left_ms` — the newest `device.left` that no
   `device.joined` closed, from the same newest-2,000-line read as `last_outage`, omitted when absent.
   The row, the notice and the terminal's notice age an offline machine from the newest evidence the
   console has: `last_frame_ms`, then that close, then the registry's `last_seen_ms`; the minutes are
   stated, so 1 h 49 m cannot read as “2h ago”. The measured sentence is **“offline · last seen 1h 49m
   ago”** at both 1440 and 390 against a fixture with no `last_frame_ms`. The machine menu draws the
   pair's “back …” line only when no later close has started a silence: the measured stale pair now
   reads **“away since 08:18”** (with the disk: “C: 312 GiB free · away since 08:18”), while a current
   return still reads **“back 2h ago after 41m”**. Measured: `cargo test -p summrise-console` 365
   passed / 0 failed / 2 ignored (1 new audit unit test, 1 new fleet integration test); clippy
   `--all-targets -- -D warnings` clean; `cargo fmt --check` clean; `npx vitest run` 613 passed across
   51 files (11 new); typecheck and build clean; `./scripts/build-web.sh --check` clean; browser gate
   PASS 19 steps (the offline row read at 1440 and 390, and the chip-on header); `a11y-audit` 0
   findings across 16 page states.)*

4. **The console's own reads are most of both timelines.** Activity at 1440 draws **949 rows, 627 of
   them console-marked (66%)**, in **55,973 px**. The “Clients only” chip (c33296d) works — 301
   rows, 18,236 px — but it is off by default, and the hub has no equivalent. DESKTOP-9VH92BJ's hub
   draws **60 rows, 56 console (93%)**, above **“Today 4 client calls · last 08:55”**; at 390 its
   first screen is two “Listed the plugins … console” rows. (14RJCR8's hub is the other way — 13
   console of 60 — because it is offline and the console cannot poll it.) **Cost:** the 60-row fold,
   added to make the day scannable, buys four rows of the AI's work on the busiest machine; the
   page's own count and Now band already decide the console's reads are not the AI's, and the rows
   still are the list. **Fix:** draw client-proven rows on both timelines and fold the
   console's own reads behind one counted line (“56 console reads today”), keeping them in the Table.
   **M.**
   *(Done 2026-11-05: the hub's timeline now draws what its day count counts — a line is the
   AI's call when its caller says so, or when no caller is recorded and it carries a token — and the
   console's own reads are folded behind one `Console reads · N` chip, off by default and per view,
   one click to add and one to fold. The chip counts the day's console rows, the rows stay in
   Activity and the Table, and an AI-empty hub says “No client calls yet; the console's own reads
   are hidden.” with the chip still offered. Activity is the audit and keeps every caller by default:
   its existing “Clients only” chip is the narrowing, and the hub's chip is not rendered there. The
   day fold follows the drawn list, so it counts the AI's rows rather than the console's polling.
   Measured in the browser-gate harness with the measured shape in the fixture (4 client calls + 73
   console reads; the newest 60 hold 56 console): at 1440 the hub went from **60 rows / 3,969 px to
   4 rows / 908 px**, and at 390 from **60 / 3,977 to 4 / 812**; the chip read `Console reads · 73`
   and one click restored 60 rows / 4,005 px (77 after “Show the rest of today”; the 60-row/56-console
   case pins 4 rows and `Console reads · 56` in jsdom). Measured: `npx vitest run` 620 passed across
   51 files (4 new); typecheck and build clean; `./scripts/build-web.sh --check` clean; browser gate
   PASS 21 steps; `a11y-audit` 0 findings across 16 page states.)*

5. **The update line repeats the running version and dates nothing.** On the 1440 fleet row,
   `v0.3.277` sits at x=1233 and **“started 0.3.277 (updated)”** at x=350, 22 px apart; at 390 the
   same card spends two of its lines on `v0.3.277` and `started 0.3.277 (updated)`. Every device tab
   at 1440 shows `Running · v0.3.277` in the header and the same started line at y=129, and the
   Machine tab adds `build 0.3.277 · online` — three copies. `/fleet`'s `update.last_update` is
   `{to: "0.3.273", applied, 2026-11-04 19:11}` on 9VH92BJ (14 h 30 m before its 09:41 start of
   0.3.277) and `{to: "0.3.275", applied, 07:21}` on CPH4KB2 (2 h 22 m before its 09:43 start), so
   “(updated)” is attached to a start it predates and names neither the version it moved to nor
   when. **Cost:** the story's one question — “did the 0.3.277 update take?” — cannot be answered
   from the line, while the running version is printed twice on the row and three times on the
   Machine tab. **Fix:** draw the version once and, when `last_update.to` is not `update.current`,
   print the update's own target and age (“last update to 0.3.273, 15 h ago”) or nothing. **S.**
   *(Done 2026-11-05: the line now states only the fact the chips cannot. The newest entry in the
   host frame's `restarts` journal is when the running process started; an updater's answer with
   `at_ms` before that moment is history — the start it produced proves the update took — so the
   row draws no update sentence for the measured 9VH92BJ/CPH4KB2 shapes at all. An answer inside
   the current start is drawn as `updated 12m ago` (applied), `update to 0.3.276 refused`, or the
   rollback sentence, never as `(updated)` attached to the start. The version is no longer
   repeated: the fleet row's own chip and the header's `Running · v…` each keep one copy, a start
   sentence whose only content is that version draws no line, and a start naming a version the
   surface does not show is kept as the contradiction it is. The Machine tab's line is now
   `online`/`offline` with no `build X`. Measured: the gate's online row read `update to 0.3.43
   refused` with its version exactly once; the Machine tab carried no `build `; `npx vitest run`
   631 passed across 51 files (11 new); typecheck and build clean; `./scripts/build-web.sh
   --check` clean with dist committed; browser gate PASS 23 steps; `a11y-audit` 0 findings across
   16 page states.)*

6. **The cap sentence's 1000 is the fetch's, not the list's.** The sentence landed in c704084; its
   number is the fetched window's, not the drawn list's. DESKTOP-14RJCR8's hub at 09:57 says
   **“Showing the newest 1000 lines; older ones are not on this page.”** and, 60 rows later, **“Show
   the rest of today (917 more)”** — 977 rows. Its `/audit?device=` (the same window, whose newest
   line is 09:23:43) has 1000 lines, 6 `device.left`/`device.joined` and 17 duplicate keys: 977
   drawn, 23 not. Activity at 1440 draws **949** rows under **“Showing the newest 1000 lines — the
   window's limit.”**; its 1000-line window has 34 boundaries and 17 duplicates: 51 not drawn.
   **Cost:** the page's two counts cannot both be true (1000 and 60 + 917), and the 23/51 rows are
   hidden by rules the sentence does not name; the cap sentence is the only place a cut is
   explained. **Fix:** say what was read and what is drawn (“1000 read · 977 shown; 23 channel facts
   and repeated lines are not drawn”), from the same list the fold counts. **S.**
   *(Done 2026-11-05: the sentence is built from the counts the merge already makes.
   `mergeActivityReport` returns the repeated keys it dropped, `mergeActivityWindow` adds the two
   reads' length and the channel boundaries in the window it holds, and `capSentence` names the
   drawn/fetched pair and only the kinds that were withheld: **“Showing the newest 949 lines of 1000
   fetched (34 channel events, 17 repeats not drawn).”** With nothing withheld it keeps the simple
   form. The timeline and the hub use the timeline's counts (boundaries dropped); the audit Table is
   the audit and draws the boundaries, so it reads **“Showing the newest 983 lines of 1000 fetched (17
   repeats not drawn).”** A paused page freezes the counts with its rows: the measured fixture held
   949 of 1000 while a live line arrived, then read 950 of 1001 on Resume. Measured on the measured
   fixtures: Activity 949/34/17; the hub 977/6/17 above “Show the rest of today (917 more)”.
   `npx vitest run` 642 passed across 51 files (11 new); typecheck and build clean;
   `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 23 steps;
   `a11y-audit` 0 findings across 16 page states.)*

### Cuts

- **C8. The `echo hello` example.** *(Done 2026-11-05: replaced, not deleted. The field names the
  program the device's own `host.os.name` says it has — `cmd.exe /c echo hello` on Windows,
  `uname -a` elsewhere — and says `program and arguments`, with no example, when the fleet has no
  OS fact (an offline machine, or a console that restarted since the last frame). Measured on
  DESKTOP-9VH92BJ with the form's exact argv (`command: "cmd.exe"`, `args: ["/c", "echo",
  "hello"]`): exit 0, stdout `hello`, 40 ms (run-1793879410419-7), where the old placeholder's
  shape answered `runs.exec: program not found` in 4 ms; `uname` is `/usr/bin/uname` on the Linux
  host. The shell-mode placeholder keeps
  `echo hello && echo bye`, because there `echo` is the shell's own command.)* The run form's
  placeholder is `echo hello` at both widths and the
  help says “Split on whitespace; no quoting.” On the two machines that answer — both Windows 11 —
  `echo` is a shell builtin, not a program: running the placeholder's own shape (`echo audit5-live`)
  answered “runs.exec: program not found” in 4 ms and wrote the green line in item 1. Delete the
  placeholder, or name a program the machines have; the help already says how the field splits. **S.**
- **C9. The fleet row's “Reconnected after 1 attempt — …” subline.** *(Done 2026-11-05: deleted from
  the row, not dated. `/fleet` serves `last_attempt` as a count and a reason with no moment of its
  own, and the row's only moment — the last frame — is after the reconnect, not at it, so any date
  there would state a different fact. No fleet row draws a reconnect line now: an online row reads
  “online · seen 12s ago”, its version, and the device's own “started 0.3.277 (updated)” when it
  sent an update story. The attempt stays in Copy diagnostics, below the ISO moment the console
  last heard the machine.)* Both online rows carry it (17 px
  each), both carry `last_attempt {attempts: 1, reason: "the console accepted the channel and then
  dropped it"}`, and neither dates it; the 0.3.277 restarts at 09:41 and 09:43 are the one failed
  attempt, and at 10:07 the sentence still reads as a fault. Delete it from the row (Copy
  diagnostics already carries the device record), or date it in the machine menu. **S.**

## P0 — the sixth list (audited 2026-11-05, live 0.3.273-ux55)

Measured on `https://console.saisi.online` with Playwright at 1440×960 and 390×844, 21:07–21:35
+08:00 (13:07–13:35 UTC), browser timezone `Asia/Shanghai` — the console host's own (`Get-TimeZone`
on DESKTOP-9VH92BJ: `China Standard Time`). `/health` read
`{"devices":3,"online":3,...,"version":"0.3.273-ux55"}`; DESKTOP-14RJCR8 read offline from 13:24
(its Terminal said `Last seen 3m ago`) and `offline · last seen 6m ago` at 13:33. Visited at both
widths: the fleet, the machine menu, all three hubs, Activity's timeline, table and an opened
record, the device tabs, the Connect sheet's Endpoint and Tokens sections, and a run's record. The
record's own facts are from
`C:\SummriseConsole\audit.jsonl` on the console host: **28,056 lines, 4,995,461 bytes**, first line
2026-10-22 10:55:58 UTC, last 2026-11-05 13:14:06 UTC (14.1 days). It is not unbounded: the console
rotates it at 8 MiB to one `audit.1.jsonl` (`AUDIT_MAX_BYTES`), which does not exist yet, so the
whole record is ~16 MiB at most.

What holds, measured: a refusal is a **424 with its reason** (`runs.exec: program not found` at
11:08:38, `device reconnecting` at 09:23:43, the 504 sentence at 17:23:41); an offline machine's age
is the newest evidence (`offline · last seen 6m ago` on the fleet row, `Last seen 3m ago` on its
Terminal); the console's own reads are folded behind a counted chip (`Console reads · 552`); the
update line no longer repeats the running version; the run form's placeholder is
`cmd.exe /c echo hello`; and no fleet row draws a reconnect sentence.

1. **There is no route to an answer older than the window.** *(Superseded 2026-11-05 by the seventh
   list's item 2: `GET /audit?from=&to=` plus the Time control is the real read, and the item's own
   "make the Time select a real read or delete it" is answered — the select now issues the range and
   the URL carries it. No separate commit; the seventh item's done note is the record.)* `/audit`
   takes `limit` and `device` and nothing else, and `limit` is clamped to **1000** (`MAX_LOG_LIMIT`, `http.rs:50`). The fleet's
   window is **8.5 h** (04:35:49–13:15:28 UTC); DESKTOP-14RJCR8's 1,000 lines are **10.3%** of its
   9,747 (9.7 h), DESKTOP-9VH92BJ's **5.7%** of 17,606 (22.8 h). The Table's Time control (All time /
   Last hour / Last day / Last week) flips to `Last week` with **0 requests** and the same **985
   rows**; no control on Activity or a hub fetches an older answer — the only "more" is the hub's
   `Show the rest of today (223 more)`, which expands the list in hand. The Files tab offers
   Workspace and Shared; its Workspace root lists 54 deploy/scratch files and not `audit.jsonl`, so
   the 27,000 lines behind the window are reachable only by a shell on the console host reading
   `C:\SummriseConsole\audit.jsonl`. **Cost:** "what happened at 09:00 yesterday" costs a remote
   shell and a text search, and the one control that reads as a time query is a filter over eight and
   a half hours. **Fix:** add `GET /audit?before=<ts>` (scan from the cursor, return the previous
   1,000) with one older control in Activity and on the hub, and make the Time select a real read or
   delete it. **M.**

2. **"Today" is the window's today, and the pages disagree about it.** At 21:23 the Activity header
   read `Today 269 client calls · 39 failed · 16 console failures · 0 unattributed`; the same local
   day in the audit file holds **649** client calls and **74** failures (2,017 rows from 00:00).
   DESKTOP-14RJCR8's hub read `Today 283 client calls · 38 failed · last 17:23` against **554** client
   calls and **53** failures in the file — **271 calls (49%) and 15 failures (28%) are older than its
   window**, which starts at 11:23 local. The three hubs read **11 + 283 + 84 = 378** client calls,
   so Activity's 269 and the hubs' 378 disagree with each other and both are under the file's 649
   (Activity is 41% of the day; 380 calls missing). The fifth list fixed *who* the count counts; it
   still counts only the lines the window holds. **Cost:** the morning number is half the busiest
   machine's day, its failure count is 28% short, and two surfaces of the same day print different
   totals. **Fix:** when the window's oldest line is after local midnight, count the day from the
   file (a bounded scan of today's lines) or state the hours the number covers, with one rule for the
   hub and Activity. **S/M.**
   *(Done 2026-11-05: `GET /audit/day` scans `audit.jsonl` backwards from the newest line, stops at
   local midnight, and counts the page's four buckets; `/fleet`'s `today` and the hub's day sentence
   read the same day, and Activity's header reads the route while its cap sentence keeps describing
   the fetched window. A file that starts after midnight says `since HH:MM`; the 50,000-line bound
   says `bounded` and names the bound. Tests first: a fixture whose window holds 3 of the day's 10
   client calls reports 10 for today and 3 for the window; a count taken twice as the window slides
   does not decrease; a file starting mid-day reports `since`.)*

3. **The hub's summary counts lines it cannot show and never names the rows it hides.**
   DESKTOP-14RJCR8's hub draws **60** rows under `Showing the newest 974 lines of 1004 fetched (9
   channel events, 21 repeats not drawn); older ones are not on this page.`, with `Show the rest of
   today (223 more)` and `Console reads · 552`; expanding the rest draws **283**, adding the console's
   rows draws **835**, and the sentence still says **974** — the lines beyond both controls are the
   rows with no caller, which the same machine's Activity header counts as `120 unattributed (1
   failed)` and the hub offers no row, chip or count for. The fifth list's cap-sentence fix counts
   the merged day list; on the hub that list is 974, and what is drawn is 60 before either control.
   DESKTOP-9VH92BJ's hub draws **11** rows and `Console reads · 122` under `Today 11 client calls ·
   1 failed`, while its Activity header counts `111 unattributed (7 failed)`; there is no cap
   sentence because that window is not cut, so the 111 rows get no sentence at all. **Cost:** the
   one sentence that explains a cut claims 139 more lines than the page can show, and a day with
   111–140 rows the console cannot attribute reads as 11 client calls and nothing else. **Fix:**
   build the hub's day line and cap sentence from the three buckets it can draw, name the
   unattributed rows with Activity's count, and replace the single "showing" number with what the
   controls hold (`60 drawn · 223 more · 552 console reads`). **S.**
   *(Done 2026-11-05: the hub's window sentence counts the three buckets its fold and two chips can
   draw — client calls, console reads, unattributed — and no longer prints the fetched total. The live
   before sentence was `Showing the newest 974 lines of 1004 fetched (9 channel events, 21 repeats not
   drawn); older ones are not on this page.` while expanding and the console chip reached **835** and
   the rows with no caller (Activity counted `120 unattributed` for that day) had no control. For the
   shape this item pins — 974 drawable, 1004 fetched, 60 folded, 111 unattributed — the sentence is
   now `Showing the newest 60 of 974 lines (283 client calls, 580 console reads, 111 unattributed; 9
   channel events, 21 repeats not drawn); older ones are not on this page.`, and `Showing all 974
   lines (...)` once the fold and both chips are on. The third fold `Unattributed · 111` is the bucket
   Activity's header already counted; it is drawn whenever it has rows, so DESKTOP-9VH92BJ's uncut
   window (19 client calls, 145 console reads, 111 unattributed) gets the chip even without a cap
   sentence. Polls are not reads on this page; they, the channel boundaries and the repeats are named
   "not drawn". One rule, stated in the comment: the same `isClientCall`/`isConsoleRead`/`isPoll`/
   `isUnattributed` predicates Activity's header uses, so the sentence's 283 client calls is the day
   line's 283. A test parses every number in the sentence and pins it ≤ the 974 rows the controls
   draw and expanding reaches the last claimed row (`Unattributed 110`). Measured on the gate's own
   974-row window (the same 283/580/111 shape): at 390, 60 rows drawn and 974 reachable after the two
   chips (fold label 914 more); at 1440, the fold draws 974 and the sentence says all 974; no fetched
   number appears. `npx vitest run` 666 passed across 51 files; typecheck and build clean;
   `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 29 steps;
   `a11y-audit` 0 findings across 16 page states.)*

4. **The fleet compares presence and disk; nothing compares work.** The three fleet rows are 45 px
   of dot, name, `online · seen 44s ago`, version and `›` — no calls. The machine menu's three rows
   add the disk and nothing else: `I: 24.3 GiB free`, `C: 19.7 GiB free`, `E: 30.1 GiB free`. The
   day counts are **11 / 283 / 84** client calls, one per hub; Activity's machine chips can produce
   them one at a time (measured **11 / 290 / 91**) in three chip clicks, and the chips carry no
   counts. The menu's silence is also incomplete: at 390 DESKTOP-14RJCR8, offline by frame silence,
   read `Offline · v0.3.275` and its disk line with no age, while the fleet row behind it read
   `offline · last seen 6m ago`. **Cost:** "which machine is doing the work / failing / quiet" is
   three screens, the one screen whose job is comparing compares disk, and the machine that went
   quiet without a close loses the one fact its row has. **Fix:** serve a per-device day summary from
   `/fleet` (client calls, failures, last call, from the audit's today scan) as one muted line on the
   fleet row, and read the menu's silence from the same `lastHeardMs` evidence as the row. **M.**
   *(Done 2026-11-05: `/fleet` serves `today {client_calls, failed}` per device from the one
   newest-2,000-line read that already feeds the absences — `caller: "client"`, or a token with no
   caller; never a failed poll, the console's own read, or a channel boundary. It is omitted for a
   machine the window names nowhere, so a row draws nothing rather than a zero; the fleet list and
   the rename's single-row answer read the same window once. The row states it on its presence line
   (`283 calls · 38 failed`, `· failed` gone at zero) and the machine menu draws it beside the disk
   (`C: 19.7 GiB free · 283 calls · 38 failed`). Measured on the gate's fixture: at 1440 the three
   rows read `283 calls · 38 failed`, `11 calls`, `84 calls · 2 failed`, the two without a verdict
   line measured 45 px and 44 px (one line; the live console's 45 px, the browser's font rounding),
   and at 390 they read the same three strings, the counts wrapped to their own line under the
   state with the name keeping the row above — 109 px and 108 px plain, 130 px with the update
   verdict — and no sideways scroll. `npx vitest run` 653 passed across 51 files; typecheck and
   build clean; `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 25
   steps (three rows' counts read at both widths); `a11y-audit` 0 findings across 16 page states.)*

5. **At 390 the audit table spends three lines on the machine's name.** The table is **320 px** wide:
   When **64**, Device **67**, Token **48**, What happened **96**, Status **45**; `DESKTOP-9VH92BJ
   console` in the 67 px Device cell breaks as `DESKTO / P- / 9VH92BJ` and the row is **91 px** tall
   (1440: Device 297 px, row 41 px). The same width clips the offline notice's reason: `the console
   accepted the channel and then dro…` — 283 px of text in 265, the rest only in a `title`, which a
   phone cannot hover. **Cost:** every row of the phone's audit spends two extra lines on the one
   column that identifies the machine, and the sentence that says why a machine is down loses its
   last word. **Fix:** at ≤639 fold the machine into the What happened cell and drop the mostly-empty
   Token column, and let the notice's reason wrap to two lines. **S.**
   *(Done 2026-11-05: at ≤639 the Device cell draws the name without the fleet's shared prefix —
   `9VH92BJ`, `14RJCR8`, `CPH4KB2` — with the full name as its accessible name and in its `title`;
   the cell stays its own column rather than folding into What happened. `DESKTOP-9VH92BJ console`
   is two lines instead of three, and the row measures **61 px**, not **91** (cell 67×61). The
   offline notice's reason is inline and wraps: the measured 283 px sentence is whole in two lines
   (258 px) with no mid-word `…`. `npx vitest run` 658 passed across 51 files; typecheck and build
   clean; `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 27 steps
   (the 390 device tail, row height and reason read); `a11y-audit` 0 findings across 16 page
   states.)*

### Cuts

- **C10. The Activity page's Refresh button.** It is the same read as the app's five-second poll —
  one click fired `/fleet`, `/audit?limit=1000&device=…` and `/tokens` — and it cannot show its
  answer while the page is paused: with the page paused at **955** rows the click left **955** rows,
  and only `Resume` shows what arrived. The poll and `Resume` own the two jobs; delete it. **S.**
  *(Done 2026-11-05: `Refresh` is deleted, not replaced. The five-second poll is the page's read and
  `Resume` is the paused page's only way back — the two jobs this cut named. The Activity header
  renders `Pause`/`Resume` alone; a unit test holds the audit read count across a Resume click, so
  the control reveals what the poll fetched instead of fetching again. The gate's 390 leg asserts
  the header offers no `Refresh` and that `Pause` then `Resume` returns the page. `npx vitest run`
  659 passed across 51 files; typecheck and build clean; `./scripts/build-web.sh --check` clean with
  dist committed; browser gate PASS 28 steps; `a11y-audit` 0 findings across 16 page states.)*

## P0 — the seventh list (opened 2026-11-05, after the sixth audit's questions)

The sixth audit asked two questions and answered them: **there is no route past the window** (the only
audit query is `limit` + `device`, clamped to 1,000; the Table's `Last week` fires no request; the
older ~27,000 lines of a 28,056-line / 14.1-day file are reachable only by a shell on the console
host) and **there is now a way to compare machines** (the fleet carries each machine's day). This
list is what is left of the page's sense of time, measured.

1. **Nothing on the page says what file it is reading.** *(Done 2026-11-05: `GET /audit/source`
   reports the current `file`, `bytes`, `oldest_ts_ms` and `newest_ts_ms`, the configured
   `max_bytes`/`rotations`, and `rotated` only when the predecessor exists (its `oldest_ts_ms` only
   when readable); Activity draws one muted line beside the cap sentence — the measured live shape
   `Reading audit.jsonl — 8.4 MiB, lines since 11-02 09:12; rotated once, older lines start
   11-02 03:10.` — and draws nothing when the route does not answer.)* The audit rotates at 8 MiB to
   one `audit.1.jsonl`; a day's read now says `since HH:MM` when its start is gone, but nowhere does
   the operator see the file's size, its retention, or that a rotation happened. The page should
   state the source it reads and its bounds — one line, in the place that already explains the
   window. **S.**
2. **A moment older than the window still has no route.** `/audit/day` reads a whole day, but a
   *chosen* day or a time range is not addressable: no query takes `from`/`to`, and nothing in the
   UI asks for one. The shape: `GET /audit?from=&to=&device=` bounded by the same scan rules, and a
   time field in the Activity Table's filter that actually issues it (today it filters in the
   browser and fires nothing). **M.**
   *(Done 2026-11-05: `GET /audit?from=<ms>&to=<ms>&device=&limit=` answers `{lines, lines_read,
   since_ms?, bounded?, scan_limit?, at_limit?}` — the lines whose `ts_ms` is in `[from, to)`,
   newest first, from the day read's same backwards scan of the current file (50,000-line bound).
   `bounded` is omitted unless the bound cut the scan; `since_ms` is present when the range starts
   before the file; `at_limit` when the range held more lines than the page's window. A request with
   neither `from` nor `to` keeps the bare array this route has always served. The Table's Time
   control is the request: `Newest window` (the default, and what the page has always drawn),
   `Last hour`, `Today`, `Yesterday`, `Last week`, and a custom `From`/`To`. A chosen range is
   `#activity?from=<ms>&to=<ms>`, so a link restores both the range and the Table view, and the
   sentence above the rows names what it drew — the gate measured `Showing 18:08–19:08 of 11-05, 2
   lines` over 2 rows. A range that starts before the file says `since 10-22 10:55`; a range the
   scan bound cut says `the 50,000-line scan bound cut it`; a range the 1,000-line window cut says
   `the window's limit`. The 50,001-line route fixture measured `bounded: true`, `scan_limit:
   50,000`, `since_ms` at the oldest line the bound read, and `at_limit: true` for the newest 1,000.
   Until a range is chosen the control says `Newest window` and the five-second poll is unchanged.
   `cargo test -p summrise-console` 390 passed (148 unit, 1 main, 231 console, 10 tokens; 2
   ignored); clippy and fmt clean. `npx vitest run` 696 passed across 53 files; typecheck and build
   clean; `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 31 steps
   (the preset fires `/audit?from=&to=`, the sentence counts the rows, the fragment carries the
   range); `a11y-audit` 0 findings across 16 page states.)*
3. **A day cannot leave the product.** *(Done 2026-11-06: `GET /audit/export?from=&to=&device=`
   serves a chosen range as `application/x-ndjson` — the range query's own lines, oldest first, one
   audit line per line and no header row — with `Content-Disposition` naming the requested bounds
   and the machine's short id, `X-Audit-Range`, and `X-Audit-Since` when the file starts after
   `from`; a range the same 50,000-line scan bound cannot prove is `413 range_too_large` with the
   console's sentence and no file. The Table's `Export JSONL` sits beside Time: a chosen range asks
   the route for the drawn bounds and device, the newest window is written in the browser, and the
   record ignores the Token/Tool/Status narrowing; the existing `Export CSV` still writes the rows
   on screen. The route fixture measured 2 lines at 1,001,000 and 1,002,999 ms oldest first with
   `summrise-audit-19700101T001641.000Z_to_19700101T001644.000Z_a1a1a1a1a1a1.ndjson`; the
   50,001-line fixture measured `413`, `x-audit-bounded: 50000` and no `Content-Disposition`; at 390
   the gate asked `/audit/export` for the drawn bounds and received the attachment. `cargo test -p
   summrise-console` 394 passed (148 unit, 1 main, 235 console, 10 tokens; 2 ignored), clippy and
   fmt clean; `npx vitest run` 701 passed across 53 files; typecheck and build clean;
   `./scripts/build-web.sh --check` clean with dist committed; browser gate PASS 32 steps;
   `a11y-audit` 0 findings across 16 page states. Live 2026-11-06 in `0.3.273-ux63`: on
   console.saisi.online at 1440, `Last hour` drew 20 lines and `Export JSONL` downloaded
   `summrise-audit-20261106T042044.933Z_to_20261106T052044.933Z.ndjson` — 20 lines, 4,788 bytes,
   oldest first, every line a JSON audit object — from the live `audit.jsonl` (4.8 MiB, lines since
   10-22 10:55); the button is on screen at 390 and the page raised no error.)*

Not items, measured: the day count is now the day's (14c4331, a wake check also landed in 2d4c7ec);
the hub's numbers are the rows its controls reach (d3c0503); a failed run is 424 with its reason
(live-verified).

## P0 — the eighth list (opened 2026-11-06, from the first real install)

Measured on 2026-11-06: `SummriseSetup-0.3.279.exe` was run on a Windows machine into
`E:\Program Files\Summrise`, and the installer aborted after writing the files. Its dialog said
**"The device runtime could not be installed (exit 1060)"** — the exit of the `sc start` attempted
*after* the launcher's `--install-service` had already failed — with `[SC] ... 指定的服务未安装`.
The launcher's own exit and message are written to `%TEMP%\summrise-setup.log`; the operator saw
neither, and no sentence said where to look. The product's own public doors were absent or 267 builds
behind: the console page (live `0.3.273-ux63`) carries **no installer URL** (`grep` of
`device/console/web/src`: only the data-export downloads), and the landing at `summrise.saisi.online`
sends "Download for Windows" to GitHub releases whose newest installer is **`v0.3.12`
(2026-10-23, `SummriseSetup-0.3.12.exe`)** while the fleet runs `0.3.279`. The only 0.3.279 installer
URL in use was the build box's tailnet mirror
`http://100.85.163.159:18100/SummriseSetup-0.3.279.exe`, which a normal user cannot reach.

1. **The console hands out the enrollment command but not the installer.** *(Done 2026-11-06, 9bed334 + 9bcab60, live 0.3.273-ux64: the fleet and the Add-a-machine panel draw `Install the device runtime` with the console's own filename, size and short sha — `SummriseSetup-0.3.280.exe · 6.3 MB · sha256 26d39c12…` — and a same-origin `/installer/download`; the live download was exactly 6,638,339 bytes with sha256 `26d39c1239f9…`; with no staged installer the sentence is true before a code exists and no link is drawn; 402 console tests, web 706, gate 34 steps, a11y 0 findings; screenshots /tmp/live-installer-1440.png and /tmp/live-installer-390.png.)* The Add-a-machine flow
   mints a 15-minute code and prints `summrise-device.exe --enroll <console> <code> --data …`, which
   assumes the binary is already installed; no control on the page offers the installer, and the
   only download actions in the page are data exports. **Cost:** the operator's first minute depends
   on a file from outside the product — in this round, a tailnet URL or a 0.3.12 release. **Fix:** the
   Add-a-machine flow offers the installer for the release the console serves (a console route for
   the setup the release carries, with its sha256), and says so plainly when the release has no
   installer. **M.**
2. **A failed install names the wrong error, and only the log knows the cause.** *(Done 2026-11-06, fe9617d: the dialog keeps the launcher's `$R0`/`$R1` across the `sc` answer, shows the launcher's exit and message, names `%TEMP%\summrise-setup.log`, and puts sc's answer on the last line; the shipped binaries are static-CRT so a clean Windows can start the launcher. Built into 0.3.280 (6,638,339 B, sha256 `26d39c12…`), which the operator ran on Windows; the failure dialog itself was not re-exercised.)* `summrise.nsi`
   overwrites the launcher's `$0`/`$1` with the `sc start` result before the MessageBox, so a
   launcher that refused before registering the service reads as "1060, service not installed"; the
   real exit and stack are one file away (`$TEMP\summrise-setup.log`, `install-service: exit N`).
   **Cost:** a missing runtime, a seed-sidecar refusal and an AV block are indistinguishable on the
   machine that has the problem. **Fix:** keep the launcher's exit and message in the dialog (and
   name the log path), put `sc`'s answer on a second line. **S.**
3. **The installer takes a working service down before it knows it can install.** *(Implemented 2026-11-06, 55f5480: the setup reads `HKLM\...\Uninstall\Summrise\InstallLocation` in the 64-bit view before anything else, refuses a different chosen root (page leave, `/S` and `/D=` all checked), stages `versions\<version>` and proves it with the launcher it carries — a 0.3.275/0.3.276 launcher's `--install-version` accepts only the release key — and only then stops the service, waits it out, replaces the flat images and starts it again with a version-aware health poll; a failed proof leaves the service running and `current.txt` untouched. Compiled with the repository's extracted NSIS: `SummriseSetup-0.3.281.exe` 159,413 B with stub payloads, exit 0. NOT YET RUN ON WINDOWS: the guard dialogs, the staged proof, the stop/wait and both upgrade paths are unexercised; the operator's next install is the first execution. Tick after that run.)* `.onInit` runs
   `sc stop SummriseLaunch` and taskkills the app before the section checks the path, seeds the
   baseline or touches the service; the directory page accepts any root, and nothing reads an
   existing install's `HKLM\...\InstallLocation`. **Cost:** a second-root attempt on a machine that
   works stops the working service first, and a failure leaves it stopped until `SummriseRepair`
   (≤5 min) or a manual start. **Fix:** detect an existing install, keep its root (upgrade in place)
   or refuse a different one with a sentence, and stop the service only once the new generation is
   staged and provable. **M.**
4. **The public download is 267 builds old.** *(Measured 2026-11-06: not a console-repo defect. The landing at `summrise.saisi.online` and the `SilasVale/summrise` releases it links to live in the other working tree `/home/zss/summrise` (`index/`); this repo has no landing source. This repo's own `.github/workflows/installer.yml` already attaches the installer to a `v*` tag release (`gh release upload`), so the console side of the pipeline exists — closing this needs the landing repo to point at the release the console serves, i.e. a product decision about which product the public door sells, made there. Left open, out of this repo's scope.)* `GET /repos/SilasVale/summrise/releases?per_page=1`
   answers `v0.3.12` (2026-10-23); the console is `0.3.273-ux63` and the fleet `0.3.279`. **Cost:** a
   person following the product's own download enrols a console-compatible device from a build 267
   device versions behind; the current installer lives on a tailnet address. **Fix:** publish the
   installer with every console-compatible release and point the landing at the release the console
   serves — the old product's setup block already knows to offer a download only when the manifest
   carries one. **M.**

### P1

5. **The export's refusal reads as a hint and outlives its range.** *(Done 2026-11-06, c10e37f + c170bda, live 0.3.273-ux65: the refusal is an inline `.alert-inline` with `role=alert` and Dismiss, carrying the range key it answered for; it clears on a range change, on Clear, on Dismiss and on a later success. jsdom 711 tests (+5), typecheck 0, `build-web --check` clean, a11y 0 findings, browser gate PASS 35 steps — the 413 is drawn and cleared in a real browser (screenshot /tmp/console-browser-gate/07f-export-refusal-390.png). A real 413 on the live console could not be produced: the audit holds fewer lines than the route's 50,000-line scan bound; the live ux65 serves the same bundle the gate exercised. Live Activity screenshot /tmp/live-activity-ux65-1440.png.)* `exportNotice` is a muted
   `.hint` paragraph with `role=alert`, and it stays after the Time control changes, so it can
   describe a range the page no longer shows. **Fix:** clear it when the range changes and draw it on
   the console's alert surface. **S.** *(From the seventh list's item 3 review: behaviour correct,
   surface not.)*

### Cuts

- **C11. The landing's GitHub-releases download.** It serves `v0.3.12` while the console runs
  `0.3.273-ux63`; replace it with the release the console serves — one door, not two. *(Item 4.)*
