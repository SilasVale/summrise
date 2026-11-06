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

5. **The device's own panel gets the same record.** The panel the device serves
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

5. **Activity filters become chips.** Machine chips and a search field; the time-range select, the
   token column and the status select move into the Table view where they belong.
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


## P0 — the ninth list (opened 2026-11-06, from the operator's first client and the token confusion)

Measured on 2026-11-06, after the first real install and the first outside MCP client: the console
printed an enrollment command the machine could not run, and the person looking for the token could
not find it. The console's own Tokens sheet holds five tokens with different, fixed device scopes;
nothing on the fleet says so.

1. **The enrollment command names a binary the installer never puts on PATH.** The Add-a-machine
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
3. **The enrollment command still depends on a PATH entry the installer never writes.** The console
   sentence (task 6) tells the operator to open the prompt in the install folder; the product's own
   command is still a bare `summrise-device.exe`, so a prompt opened anywhere else fails. The
   installer knows the root; the console does not. **Fix:** the installer appends `$INSTDIR` to the
   machine PATH (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`), only
   when absent, and the uninstaller removes exactly its entry; the Windows run is the first proof.
   **M.** (installer; measured on the E: install.)

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

1. **There is no route to an answer older than the window.** `/audit` takes `limit` and `device`
   and nothing else, and `limit` is clamped to **1000** (`MAX_LOG_LIMIT`, `http.rs:50`). The fleet's
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
