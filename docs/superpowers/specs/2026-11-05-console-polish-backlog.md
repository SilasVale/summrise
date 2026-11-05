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
3. **A machine's scope must read that machine's window.** Activity fetches the fleet's 1000 and
   filters in the page; `GET /audit?device=` exists. Fetch the machine's own window when a chip is
   selected, and let the cap sentence name whose window was cut. **S.**
4. **The hub's day must be reread, not remembered.** The machine window is fetched once per device;
   after a reconnect or a hiccup the day shows a hole with no cap sentence and the band says "Quiet"
   while rows exist. Refetch on the app's 5 s tick and on stream recovery; say when it was read. **M.**
5. **Stop claiming every call is in the record.** Both empty states say "Every call a client makes is
   written here"; a call straight to a machine's own endpoint is not. Name the boundary. **S.**
6. **Give every tool a sentence and gate the table against the registry.** *(Done 2026-11-05, a9d9dff: 16 templates added, 10 dead ones removed after a history measurement; `every_registry_tool_has_a_sentence` walks the registry and fails for a declared tool without a sentence.)* 16 of the 46 registry tools
   fall back to "Used `tool`" (`host.restart` among them) and eight sentences name tools the registry
   does not have. Write the missing templates and test the table by walking `registry.rs`. **S.**
7. **Say where one stretch ends.** The 60 s gap is computed and drawn as a hairline; a full day reads
   as one column. Label each stretch with what it held — "4 calls · 2 m 10 s" — inferring nothing. **S.**
8. **Delete the retired components and the calls nobody makes.** `TerminalPane.tsx` (159 lines, no
   importer), `ToolPicker.tsx` (42 + 35), `api.denials`, `api.meshStatus`/`meshReadiness` and their
   tests; the server routes stay. **S.**

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
