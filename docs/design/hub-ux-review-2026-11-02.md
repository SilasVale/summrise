# Hub UX review: what still fails the Apple register

*Revision of the review committed at `3124259`; re-verified 2026-11-02 14:11 UTC on `hub/v1` after the
shell commit `3770e59` and the `Machines` naming decision `79bb784`. Since the committed copy, the
one-control header, the fleet rows, `Add a machine`, the 200 px rail with a 640–767 icon state, the
32 px padding and the global focus ring were fixed; those rows are removed. The gallery in
`/tmp/console-react-shot/hub-recon/` was still rendered at 13:37 from the pre-shell page, so no
screenshot shows the new shell — a visual finding cites it only where the CSS or the surface is
unchanged. Every row below was true at 14:11.*

## Verdict

The shell now matches the spec, and the finish items that were open at 13:59 are closed. What remains
is the record, the terminal, and three places where the spec still contradicts itself or its own
architecture. A calmer shell will make the old record and the double-framed terminal look worse, not
better, because everything around them now has air. Fix M2–M4 before the gallery is re-shot; M5–M7
are one-line decisions in the spec, not work.

## Must fix before deploy

| # | Finding | Screen / file | Why it matters | Exact fix |
|---|---|---|---|---|
| M1 | Between 640 and 767 px the rail is icons only and the label exists as a native `title`: it appears on a mouse hover after a delay and never on keyboard focus. §5.2 promises "labels on hover/focus". | `app.css:190-196`; `Rail.tsx` (`title={PAGE_LABELS[entry]}`) | The spec's whole argument for labels is that an icon column is a memory test. At the one width where it accepts icons, it drops the label for the operator who is not using a mouse. | Keep `.rail-label` in the DOM at that width: `.rail { overflow: visible }`, and on `.rail-btn:hover .rail-label, .rail-btn:focus-visible .rail-label` render it `position: absolute; left: calc(100% + var(--sp-2));` on a `--surface` chip with `--line` and `radius-xs`. Drop the native `title`. |
| M2 | Two accent fills on any page with a primary action: the current rail item is `background: var(--accent-solid)`, and the action is also a filled accent. §7 says a screen has one accent fill. | `app.css:114` (source) | The eye can only land on one thing. A 200 px block of `#b03a0a` on `#17181c` is the loudest object on screen and the same colour as the button the page exists for. | The current item is ink, not paint: `color: var(--rail-ink-hover)` + `box-shadow: inset 3px 0 0 var(--accent)` at ≥768; at 56 px `background: rgba(255,255,255,.06)` + `color: var(--accent)`. Reserve the fill for the action. |
| M3 | Activity still ships the old table: the expanded record is clipped by `.scroll{max-height:17rem}`, label and value sit ~900 px apart, `KIND` repeats `mcp`, headers are uppercase 11 px, and every row has a `Details` button. §6.4 now forbids all five. | `audit-detail.png`, `audit.png`, `audit-dark.png`; `app.css:442`, `:475`, `:484`; `AuditTable.tsx:321`, `:390` | This is the product's evidence — the one screen where a fact must read at a glance. It is also the surface the shell change did not touch, so it will look older beside the new pages. | Implement §6.4: no nested max-height; the whole row toggles its record with one chevron; `device.name` with the id in `title`; no KIND; sentence-case 12 px headers; record in `10rem minmax(0,1fr)`, max 42 rem, left-aligned values. |
| M4 | The terminal is still framed twice, and in dark the canvas (`#131418`) is the page's own colour, so the surface disappears. §6.6 now demands one frame and `#0f1014`. | `terminal-session.png`, `terminal-session-dark.png`; `app.css` `.term-host` | The xterm is the product's most important surface; a box in a box with an invisible canvas is the opposite of confidence. | `.terminal-panel { padding: 0; overflow: hidden }`; `.term-host { border: 0; border-radius: 0; padding: 0 }`; 40 px tab strip above. Change `--term-canvas` in `device/ui/src/lib.rs`, run `scripts/generate-tokens.sh`, and commit the regenerated `tokens.css` with the sheet. |
| M5 | The Add-plugin sheet validates the path on blur, but that needs a filesystem read on the *device*, and §8.3 defines no such tool (`plugins.list` takes no path; `plugins.add` starts the plugin). | spec §6.3 vs §8.3 | As written, "validated on blur" cannot be built without a route or a side effect — so the implementer will either fake it or press Add, which is what the preflight was meant to prevent. | Add `plugins.inspect {path} → {manifest, tools}` (read-only, does not spawn) to §8.3 and the plugin-host scope; the sheet calls it on blur. If that is too much for v1, delete "validated on blur" and keep the five inline errors on Add. |
| M6 | The sheet has two widths: §6.5 says "wide on desktop (720 px)"; §7 says the right drawer is 480 px. | spec §6.5 vs §7 | The tool sheet holds a form, a 40 vh result and a pinned footer; the token sheet holds a scope summary and a table. At 480 both break; at 720 the drawer and the 40 % backdrop leave no page context. | One number, stated once in §7 and referenced by §6.5 and §6.2: 600 px. Full-screen below 800, as §7 already says. |
| M7 | Home's status line and counts line carry the same numbers: `Running · v0.3.246 · 50 tools · 2 plugins`, then `50 tools · 8 capabilities · 2 plugins →`. | spec §6.1, §6.7 | Two lines, one fact, one paragraph apart — the duplication pattern the rest of this revision deletes. The status line is the machine's health; the counts line is the catalogue. | Status: `Running · v0.3.246`. Counts: `50 tools · 8 capabilities · 2 plugins →`. Update the copy-deck row "Home, running" to the status line only. |
| M8 | Deletion stopped at navigation. Nothing imports `MemoryPage`, `MonitorsPage`, `EventsPage`, `SecurityPage`, `MonitorChip`, `NotificationsCard` or the old `DevicePage` (1,719 source lines plus their tests), and `--tracking-micro` is declared but unused. | tree (source); spec §15.9 | A dead page is a second product nobody maintains; a dead token is a second vocabulary. The rail change made them unreachable. | Delete the seven files and their tests with the shell commit. Remove `--tracking-micro`; `--rail*` are now live in the dark rail, so keep them. |
| M9 | The screenshot harness covers five destinations at 1280 and 390, but not 360 or 800, and captures no sheet and no state (offline, stale catalogue, plugin failed, Try it running). §3.9 and acceptance 9 claim 360/390/800/1280 for every page. | `screenshot.mjs` PASSES; gallery `index.html` | 800 is where the sheet becomes a drawer and 640 where the tab bar appears — the two widths the shell is built around, and neither is rendered. A visual review cannot approve a state nobody photographed. | Add 360 and 800 passes for all five destinations; add Home offline/stale, Plugins empty/failed/old build, and a tool sheet idle/running/error/destructive. Re-shoot after M1–M4. |

## Should fix in v1.1

| # | Finding | Exact fix |
|---|---|---|
| S1 | Try it is silent to a screen reader: the spec defines the result visually but not how it is announced, and a focus move is the only way a keyboard user learns the call finished. | After Run, focus the result heading and wrap the summary in `role="status"`: `Done in 42 ms.` / `Failed.` Never steal focus while the operator is typing. |
| S2 | Capabilities search has no keyboard story: no shortcut, no live count, and the empty state's `Clear the search` is described as text in §6.2. | `/` focuses the field, Escape clears it, the count updates (`3 of 50 tools`), and `Clear the search` is a button beside the empty sentence. |
| S3 | Adding a plugin succeeds into silence: the sheet closes, a row appears, and the operator has to find it. | Close the sheet, toast `hello added · 2 tools`, with an action that opens Capabilities filtered to `hello`. That is the loop the product promises. |
| S4 | The Machine tab's process list is the full process table — hundreds of rows, no ranking, no limit. | Top 10 by resident memory with `Show all 214`; columns process, pid, memory; the rest behind the disclosure. |
| S5 | The terminal `⋯` menu carries four settings for one tab (Save a preset, Rename tab, Export log, Text size) and `×` closes a live pty with no confirmation. | Label it `Session`; drop Text size and Save a preset (saved connections exist); `×` asks `Close this session? The process keeps running on the machine.` Still current: `terminal-tools.png`. |
| S6 | Activity makes the operator expand a row to learn whether the call failed, and hides the run link at the bottom of the record. | Show the run's exit code inline (`exit 0`, `exit 1`) and put `Open in Runs` first in the record. |
| S7 | Home shows one door. The product's distance promise has two: the console endpoint and the machine's own panel. | Endpoint row: the console URL with Copy; a second quiet line `On this machine: http://127.0.0.1:18080` with Copy. No second card. |
| S8 | The plugin chips `Starting` and `Stopped` have no sentence, though `Failed` does. A ten-second start with a chip and no words is the state that gets refreshed by hand. | `Starting… this usually takes under ten seconds.` and `Stopped. Restart to use its tools again.` Add both to §6.3's state list. |

## What the spec gets right

- **One accent fill, and the instruction to delete the weaker candidate** rather than restyle it. That
  is the rule that keeps the new shell calm — and the rule the rail currently breaks (M2).
- **The shell**: one machine control, the 200 px rail from 768 with an argued icon state, a bottom bar
  that does not wrap, no status strip.
- **The record's reading rules** — no nested scroller, whole-row disclosure, machine names, no KIND,
  sentence-case headers, relative time with the date in `title`. Do not soften these (M3 is the CSS
  catching up).
- **Try it's state list**, now including blank-vs-omit, tri-state booleans, the Raw JSON round-trip,
  the 40 vh/400-line cap, the timeout sentence, the inline destructive confirmation and
  `Recorded in Activity.` This is the most complete interaction spec in the document.
- **Plugin trust stated consequence-first at the install**, a preflighted Add, and a failed row whose
  action is Restart. No sandbox theatre, no permission model.
- **Committed numbers** — sizes, spacing, radii — with "a new size is a token change and a test change
  in the same commit". That ends the argument.
- **Degradation as design**: no Add on an old build, no fake data offline, a stale catalogue labelled
  rather than hidden.
- **Deletion with names**, and acceptance lines a reviewer can check rather than a mood to agree with.

## Copy findings

| # | Current | Better | Where |
|---|---|---|---|
| C1 | `No audit lines yet.` | `No calls yet.` | `AuditTable.tsx:102` |
| C2 | `No audit lines match.` | `No call matches these filters.` + `Clear the filters` | `AuditTable.tsx:103` |
| C3 | `Mint a grant and show the config` / `Signing…` | `Create a session` / `Creating…` — do not carry "mint a grant" into the new sheet | `ConnectCard.tsx:284` |
| C4 | `A machine appears here after it enrols with this console.` beside `Enrollment command` | one language: `enrolls` (the code, `--enroll`, `api.enroll` and `Enrollment` are US) | `HomePage.tsx:76` vs `:47` |
| C5 | the empty state repeats `Add a machine` as a link under the page head's `Add a machine` | delete the link; the primary button in the page head is directly above | `HomePage.tsx:81` |
| C6 | `This code works once, for 15 minutes. On the Windows machine, run this in an elevated Command Prompt, then restart the SummriseLaunch service.` | `Run this on the machine in an elevated Command Prompt, then restart SummriseLaunch. The code works once and expires in 15 minutes.` | `HomePage.tsx:49-52` |
| C7 | `${count}, newest first. Click a row to read its record.` | `Recent runs` — the chevron and the row are the instruction | `RunsPage.tsx:586` |
| C8 | `Could not copy the command.` | `The browser refused the clipboard. Select the command and copy it.` | `HomePage.tsx:58` |
| C9 | `All machines` as the header trigger when nothing is chosen | `Choose a machine` as the control; `All machines` only as the menu item | `MachineMenu.tsx:36` |
| C10 | `Starting` / `Stopped` chips with no sentence | `Starting… this usually takes under ten seconds.` / `Stopped. Restart to use its tools again.` | spec §6.3 |
