# Console polish: the working backlog

**Status:** running. The operator's instruction on 2026-11-05: find the problems myself, write them
down, solve them, and keep going — a goal that can run for hours without a question.

Each item ships on its own: the web gates, the console's Rust tests, a live check on
`console.saisi.online` with a screenshot, then one commit. An item is done when all four are true.

## P0 — the record of what the AI did

1. **Timeline + evidence** *(in flight)*. One row per action (time, sentence, machine, outcome,
   duration); a click opens the evidence (command, output tail, session, links). The audit table
   stays behind the `Table` toggle.
2. **Screenshots as evidence.** A `screen.capture` call keeps its last image in console memory
   (bounded per device) and the row opens it inline. The image is ephemeral: nothing is written to
   disk and a restart forgets it.
3. **The session card.** The Terminal page shows the session before the terminal: machine, shell,
   how long it has been open, the last command, the last few lines, and one button to attach. The
   raw terminal is what you get after that card, not the first thing.
4. **Home stops leading with the endpoint.** The hub becomes this machine's live page: Now (presence,
   rate, last steps), today's timeline, the session preview. The endpoint lives in the Connect sheet.

## P1 — the console stops feeling like a configuration tool

5. **Activity filters become chips.** Machine chips and a search field; the time-range select, the
   token column and the status select move into the Table view where they belong.
6. **Terminal tabs carry facts.** A tab says the shell and the age, not just `pty 1`.
7. **Connect has one token tab.** Per-device tokens and fleet tokens are the same idea at two
   scopes; one list with a scope column, not two places to mint a credential.
8. **Empty and offline states say what to do.** Every page that can be empty (timeline, runs, files,
   plugins) gets a sentence that names the next action, not just "none".

## P2 — consolidation, then polish

9. **Capabilities and Plugins become one Tools tab** under the machine. One place answers "what can
   this machine do", with plugins as a section of the same answer.
10. **The Runs page's exec form moves** under Tools; Runs becomes the history, not a second way to
    run a command.
11. **Accessibility sweep.** Every row that opens is a button with `aria-expanded`; every icon has a
    name; focus order follows reading order; the warning colour is checked against the AA pairs.
12. **Long timelines.** Measure a 1000-row day before optimising; virtualise only if the frame
    budget says so.
13. **Phone.** The timeline at 390 px: the time column narrows, the age drops, the evidence panel
    stacks (already true) and the day label sticks.

## How a round goes

1. Read this file; pick the first item that is not done.
2. Implement it in the smallest shippable shape; write the tests that prove it.
3. `npm test`, `npm run typecheck`, `scripts/build-web.sh --check`, the token gate, the browser gate.
4. Build the console with a new `-uxNN`, swap it on the host, run the live check, screenshot.
5. Commit with the measurement and a `VERIFIED:` list. Tick the item here in the same commit.

## Not on this list, and why

- **A fleet-wide board.** The toolbar's picker is the model; the operator rejected a second
  navigation.
- **Decision events.** The console sees actions; the AI's reasoning is the client's to publish.
- **The cube.** A renderer for a page that does not exist yet; revisit after P0.
