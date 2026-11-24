# Console and panel polish: what changed, 2026-11-04

> **Status: record, 2026-11-04.** One pass over the console's page and the device's local panel, judged
> against one question — does this screen tell the truth about the machine, and does it read as one
> thing. The console half is deployed (`0.3.271-ux31`). The panel half is **eight commits that have not
> shipped**: it goes out through a signed release whose key is not on this box.
>
> Every row below was checked in a browser against the real device, not only by a test. Where a number
> is quoted it was measured there.

---

## The console

| commit | what was wrong | what it does now |
|---|---|---|
| `ae3cf67` | a tab's `×` killed the shell while the dialog said the process kept running | Detach (primary), End session (danger), Cancel; the detached id is remembered |
| `06527dc` | the Terminal attached to the oldest session on the machine | it attaches to the newest |
| `c8ca2be` | a tab said "pty 1" and nothing else | the tab carries the device session's short id |
| `fcc394c` `a66f31c` | Find needed Enter, and had no marks to land on | it searches 120 ms after the last keystroke and paints an overview ruler |
| `f35d491` | Find threw `allowProposedApi` and never scrolled | it scrolls to the match |
| `78851ef` | the Sessions list did not say when a shell was opened | it says "opened 12m ago" |
| `6cad93c` | Screen offered a helper it could not reach | the helper's absence is named before the click |
| `2f8dc4a` | a proxy sign-out landed on the token form | "Signed out of this tab" and a reload, no token offered |
| `9e875c3` `b84b265` | a 49-entry folder could only be read by scrolling | a filter, and it sits with the count above the list rather than at the panel's edge |
| `52261b0` | "Root" and "Workspace" read as one phrase | a muted label, a 6 px gap, the value in ink |
| `39fed19` | the search field floated above the summary it sits beside | centred in the heading block (80–137 vs 90–127) |
| `4276391` | Home repeated the machine chip's own text | it names the console's build instead |
| `c3ecf41` | Export CSV was the only item in a `⋯` menu | a visible button at the end of the filter row |
| `f40bb8d` | the URL was written and never read — Back, Forward and pasted links did nothing | every fragment change is applied; an unreadable one changes nothing |
| `e776d89` | a `#run/…` link that arrived while Runs was open loaded nothing | it loads the record |
| `fda4f70` | opening a tool focused "Raw JSON" | it focuses the first field |

Two refactors ride along: the console and the panel now share one tracking scale
(`--tracking-eyebrow/label/tight`, `f40bb8d`) and the panel names the shared type steps by token
(`4940f9f`) and spacing steps by token (`3c23fbf`).

## The panel (not shipped)

| commit | what was wrong |
|---|---|
| `d5d3cc0` | the top bar was padded from the window edge while the cards sit on a centred column (brand x 24 → 217) |
| `79522bf` | five controls read as five equal actions; Close sat beside New serial |
| `93fbb76` | an empty terminal canvas read as a broken terminal; the hint pointed the wrong way |
| `2d0cb89` | an unlinked card described a link that does not exist, above the form that is its content |
| `3e06822` | "not linked" was stated three times on one screen |

`scripts/panel-preview.sh` renders and serves the three screens (with a console, empty, first run) so
these can be reviewed without a device or a release key.

## Audited and found clean

Do not re-audit these blind; the measurements are in the round reports.

- **Contrast.** Every text run in `main` on Activity (46 rows), Files (49 rows) and Machine passes WCAG
  AA for its size — computed from the live colours and the nearest opaque background.
- **Horizontal overflow.** No element exceeds its box on Runs, Files or Machines.
- **Dialogs.** Escape closes, Tab is trapped, and focus returns to the trigger when it had focus. The
  skip link's `#main` target exists.
- **Machine page.** The vitals chart already names its latest values in text; its flat start is "not
  enough history", not a divider.
- **Files rows.** Hover underlines the name in the accent colour; directories are semibold.

## Not verified

- **Dark theme.** The token set has dark values, but this browser cannot emulate
  `prefers-color-scheme` and nothing here simulates it.
- **Narrow viewports.** The controlled browser is fixed at 1280×800; the `@media` blocks are read, not
  measured.

## Open

- One `App.test.tsx` case ("legacy alias") failed once in a full run and never again in six. It now
  allows three seconds (`6d6adc6`); that is a mitigation, not a diagnosis.
- Console deploys since `0.3.271-ux17` use the interim path in `docs/console-deploy.md`, which skips
  the page check, the dirty-tree refusal and the hash check before the swap.
