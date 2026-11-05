# Console polish: the working backlog

**Status:** running. The operator's instruction on 2026-11-05: find the problems myself, write them
down, solve them, and keep going — a goal that can run for hours without a question.

Each item ships on its own: the web gates, the console's Rust tests, a live check on
`console.saisi.online` with a screenshot, then one commit. An item is done when all four are true.

## P0 — the record of what the AI did

1. **Timeline + evidence** *(code complete 2026-11-04; deployment blocked by the device outage —
   see 2026-11-04-device-outage.md; ship the console as soon as a host answers).* One row per action (time, sentence, machine, outcome,
   duration); a click opens the evidence (command, output tail, session, links). The audit table
   stays behind the `Table` toggle.
2. **Screenshots as evidence.** *(Code complete 2026-11-04: 4 per device, 32 total, LRU, memory only; the row opens the frame with its capture time; the route is authenticated and a restart forgets it.)* A `screen.capture` call keeps its last image in console memory
   (bounded per device) and the row opens it inline. The image is ephemeral: nothing is written to
   disk and a restart forgets it.
3. **The session card.** *(Code complete 2026-11-04, queued behind the outage: the card leads the page — machine, session, kind, age, last keystroke from the live evidence.)* The Terminal page shows the session before the terminal: machine, shell,
   how long it has been open, the last command, the last few lines, and one button to attach. The
   raw terminal is what you get after that card, not the first thing.
4. **Home stops leading with the endpoint** *(code complete 2026-11-04, queued behind the outage:
   the hub is the machine — Now band, today's timeline, one `Connect a client` action; the endpoint
   lives only in the Connect sheet).* The hub becomes this machine's live page: Now (presence,
   rate, last steps), today's timeline, the session preview. The endpoint lives in the Connect sheet.

## Blocked on 2026-11-04

The device fleet is offline after canary `0.3.272`; the console is healthy but every deploy needs a
host's `runs.exec`. Code-only work continues; nothing ships until a device answers. The console build
`0.3.272-ux37` (timeline + evidence + review fixes) is green and queued.

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
2. **Name the token on the newest rows.** A live event carries no `token_id` (`http.rs:455-467,
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

## P0 — the third list (audited 2026-11-05, live 0.3.273-ux43)

Measured on `https://console.saisi.online` with Playwright at 1440×960 and 390×844, 05:00–05:20
UTC. `/health` read `{"devices":3,"online":3,...,"version":"0.3.273-ux43"}` and `/session`
answered through the proxy. Every number below is from the page or its own `/audit?limit=1000`
read; nothing here is a suspicion, and the two lists above are not re-opened.

1. **The day count drops the calls its own token proves.** Of the 1000-line window, 30 rows say
   `caller: "console"`, 62 say `"client"`, and **908 say nothing**; **376 rows carry a `token_id`**
   and **0 of the 30 console rows do**. Of the 908, 314 carry a token (client calls) and 594 carry
   neither (unattributed). Today `DESKTOP-14RJCR8`'s window has **940 rows and 363 client calls**
   (62 marked plus 301 token-only), but its hub reads **"Today 62 client calls · last 04:43"** —
   **5.9× low; 301 calls (82.9%) missing**. **Cost:** the one number the operator reads in the
   morning is wrong on the machine that worked, and 594 rows cannot be read as the AI's or the
   console's. **Fix:** a `token_id` is the client proof, absence of both caller and token is
   "unattributed", used by the day line, the row mark and the Activity filter; no new server field.
   **M.**

2. **The machine's day is 51,620 px and its capability line is at the bottom.** On
   `DESKTOP-14RJCR8` at 1440 the hub draws **935 rows** (one day) in a **51,620 px** `main`, and
   "46 tools · 11 capabilities · 0 plugins" sits at **51,599** — 56 screens down; at 390 it is 936
   rows, 51,684 px, the counts line at 51,669. Even the quiet `DESKTOP-9VH92BJ` spends 4,433 px on
   64 console rows at 390. **Cost:** the answer to "what can this machine do now" costs a 50k px
   scroll; the day cannot be scanned. **Fix:** move the counts line into the header and cap the
   hub's timeline (newest ~100, "All activity" for the rest). **M.**

3. **The run form is 2,940 px below the Tools tab.** Tools is **3,848 px** tall in a 908 px
   viewport; "Run a command on this machine" starts at **3,773** and the only path is past the
   46-tool catalogue (at 390: 4,070 px, 739 px viewport, 3,331 px of scroll). `runs.exec` is 139 of
   the 1000 lines. **Cost:** two clicks and three screens for the most-used write; Runs, the
   history, has no run control. **Fix:** put the form at the top of Tools, above the catalogue. **S.**

4. **The cap sentence disappears at exactly 1000.** `/audit?limit=1000` returns **1000** lines; the
   page draws **997** (the merge drops 3 duplicates), sets `capped` false, and neither view says
   "Showing the newest 1000 lines; older ones are not on this page." The oldest row is 16:18
   yesterday. **Cost:** a 997-row slice reads as the whole record; the lines behind the server's cap
   are silently gone. **Fix:** set `capped` from the fetched window's length (`>= ACTIVITY_LIMIT`),
   not from the merged list. **S.**

5. **The biggest stretch is 470 rows and its summary arrives after them.** Activity draws **997
   rows** and **64** stretch labels (05:12); **28 say "1 call"** and one says "2 calls · 0 s". The
   largest is **"470 calls · 51 m 30 s"**: 470 rows, 25,850 px, 46% of the 56,143 px timeline, and
   **360 of the 470 are "Read a terminal's output"**; the label is drawn after the 470 rows.
   **Cost:** the only summary of half the page cannot be used to skip the work it describes, and 28
   labels mark a single row. **Fix:** draw the summary at the top of the stretch, name the dominant
   tool, and fold a stretch above 50 rows from its label. **M.**

6. **The fleet row counts the attempts and hides the reason.** Two of the three rows read
   **"Reconnected after 3 attempts"** (`DESKTOP-9VH92BJ`) and **"Reconnected after 4 attempts"**
   (`DESKTOP-CPH4KB2`); the fleet read's `last_attempt.reason` is **"HTTP error: 502 Bad Gateway"**
   and **"the console accepted the channel and then dropped it"**, and neither string is on the
   page. **Cost:** a proxy 502 and a channel drop read as the same healthy reconnect; the operator
   cannot tell whether to look at the edge or the machine. **Fix:** append the reason to the muted
   line when the fleet carries one. **S.**

7. **Activity has no day count and no way to narrow to the AI.** It draws **997 rows (980 today)** —
   363 client-proven, 30 console-marked, 587 unattributed, and 12 failed (the 12 are in the
   unattributed set; the 62 marked client calls are all 200) — and its header says only
   "Following the console's audit as it is written." The only client filter is Table → Token
   (`dsh-aggregate`): two clicks and a select; the timeline's four machine chips cannot separate the
   AI from the console. The newest row on the page is a console read (its own `plugins.list`).
   **Cost:** the page's stated job — what the AI did — takes a view switch and a guess. **Fix:** one
   "Today 363 client calls · 12 failed · 30 console reads · 587 unattributed" line and a Client
   calls chip using the same rule as 1. **S/M.**

### Cuts

- **C6. The Terminal page says "no session" twice.** "No session is open on this machine." at
  y=228 and "No session is open on this device. New session opens a shell; it appears here once it
  is running." at y=336, 108 px apart; the second carries the action, the first is the page's
  duplicate. Delete the first. **S.**
- **C7. The hub's subtitle duplicates the account menu's console version.** "Running · console
  v0.3.273-ux43" is the page's one subtitle; the Account menu prints "Console 0.3.273-ux43". At 390
  the header chip drops the device's "v0.3.274", so the one build on the page is the console's,
  which the menu already has. Delete the version from the subtitle; keep the menu's. **S.**
