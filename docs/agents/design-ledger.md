# The design ledger — the long form

`AGENTS.md` holds the RULES. This file holds the ROUNDS: what was measured, what it cost, and what was learned on the
way. It was split out in round 67 because AGENTS.md had grown past the workspace instruction budget (65,366 bytes) and
the harness began TRUNCATING it — the tail, which is the Release and layout sections, was being dropped silently. The
organising rule is that an instruction file has to stay small enough to be read whole; evidence does not.

READ THIS WHEN: you are about to re-measure something (the numbers here may already exist), you want the story behind
a rule in `AGENTS.md`, or you are wondering whether a failure you just saw has happened before. It has.

The gate table — which mutation fails which gate — stayed in `AGENTS.md`, because it is the thing consulted most
often. Everything below is in the order the rounds were WRITTEN, which is roughly chronological and NOT sorted by round number — the ordinals run 45 55 66 65 64 40 70 … because sections were added as they were learned. To find a round, grep for it; do not compute an offset. (This said "in round order, oldest first" until round 183, and the file has never been in that order.)

**AND A ROUND NUMBER IS NOT A KEY.** The ledger carries **three numbering epochs**: an early run that climbed to
103, a second that restarted at 15, and the newest sections numbered 265-272. So `round 73` names two unrelated
sections, and a reader who greps it gets both with nothing to tell them apart. **Grep a title or a quoted phrase.**
Every section title is unique; no round number is.

## What is in here

THE FILE IS LAYERED BY ERA, and the eras are runs of sections in the order they were written. Find yours by its
opening title, then read forward; nothing below reorders them.

| era | what it holds | opens with the section titled |
|---|---|---|
| the first sweeps | the panel and the four surfaces, measured as rendered — contrast, geometry, reflow, focus, the loud axis, type floor | `FOUR LIVE DEFECTS THE PANEL'S OWN SHAPE CHECK COULD NOT SEE` |
| the sweeps find their feet | the design job sweeping the wrong artifact, a stale entry, the first idle measurements, all four surfaces green | `THE INSTRUCTION FILE WAS BEING TRUNCATED` |
| the instruments | gates and probes turning out to be wrong before their subject — the SVG root's fill, the press pass measuring hovers, the waiver that was for the ratio, an instrument taken back | `AN SVG ROOT'S FILL IS NOT ALWAYS A COLOUR` |
| epoch two | wire contracts and clauses: the runs and monitors payloads, what a list cannot answer, exemptions that answer for themselves | `THE MYSTERY WAS IN THE OUTPUT I HAD ALREADY COLLECTED` |
| silhouettes and the queue | the marks vocabulary, the state families, the surfaces queue emptying, the live panel re-measured | `HALF THE SILHOUETTES WERE UNVERIFIED` |
| the gate discipline | auditing the gates themselves, which gates read comments, how this project serves a second configuration, whether failure messages tell a reader what to do | `AUDITING THE TEN GATES` |
| the emit seam | six slices that turned five emitted payloads into modules and pinned each one with a gate | `"CAN YOU OPTIMIZE THE PANEL DISPLAY?"` |
| the subsystem explorations | one dated `##` section per architecture pass: the release pipeline, the desktop command, the tool layer, the HTTP surface, the CLI, the proxies, the landing, the installer, the core crate | `2026-09-23 — the release pipeline audited its own author` |

## Looking for one thing

The three triggers in `AGENTS.md` — *about to re-measure something*, *the story behind a rule*, *has this failed
before* — map onto this file like so:

- **A number you are about to re-measure** (a contrast ratio, a silhouette count, a press latency, an off-scale spacing
  count): it may already exist, with the measurement that produced it. The sections state their own date and round.
- **The story behind a rule in `AGENTS.md`**: `AGENTS.md` names the section where it kept one — search that name.
- **A failure you just saw**: the recurring shapes all have a section, and the shape is usually in the title — *an
  instrument wrong before its subject*, *a number in prose*, *a check that misread the artefact*, *a rule and a
  comment*.

---

### FOUR LIVE DEFECTS THE PANEL'S OWN SHAPE CHECK COULD NOT SEE (round 45)

Round 44 taught the CONSOLE's marks check that a mark which is a fill AND a ring is neither. The panel has the same
expression in `statePalette.test.ts`, with the same hole — `inset` won outright, so a fill plus an inset shadow
computed as `ring`, distinct from a solid and therefore passing. Mirroring the rule there found FOUR rendered
defects, all on the Plugins page, all in both themes, confirmed by reading the browser's computed styles:

    .plug-dot[data-state="success"]   a green fill inside the base's grey inset ring
    .plug-dot[data-state="warn"]      an amber SQUARE inside the same ring
    .plug-dot[data-state="error"]     a red rotated square (the diamond) inside it
    .plug-dot[data-state="muted"]     a grey fill inside it

The base `.plug-dot` became a RING in round 25 (its "nothing to report" state), and every state arm only set a
BACKGROUND — so every one of them inherited the ring. Fixed by saying what each arm is (`box-shadow: none`), and by
DELETING the `.plug-dot[data-state="muted"]` rule, which contradicted round 25's decision that an unqualified
`.plug-dot` already IS the muted ring.

    RENDERED AFTER THE FIX: 8 success=solid + 1 warn=solid, in light and in dark. Zero ring+fill.

TWO WRONG CASCADE MODELS ON THE WAY, both worth recording because a shape check that disagrees with the cascade
reports on a stylesheet nobody is looking at:

  * taking the FIRST declaration hid the arm entirely (the base rule sits later in the sheet);
  * taking the LAST TEXTUAL declaration hid the base — it read `.plug-dot`'s `background: transparent` over the
    arm's fill and reported all four arms as clean rings, which is the opposite of the truth.

The model that is right is SPECIFICITY: `.plug-dot[data-state=...]` is (0,2,0) against the base's (0,1,0), so the arm
wins wherever it sits. The check now says so in the helper.

AND TWO PROCESS TRAPS SPRUNG IN THE SAME ROUND: `git checkout <file>` used as a "restore the mutation" shortcut
reverted the whole uncommitted round (the work was re-applied and backed up to /tmp first thereafter), and the
rendered check first read a STALE HARNESS — the panel had been rebuilt but `panel-harness.html` is a snapshot, so
the browser kept showing the old sheet and the fix looked absent.

THE RENDERED AXIS HAD THE SAME HOLE, WHICH IS WHY IT WATCHED (round 46). The core's `marks` probe — the one the
surfaces pass carries on every page — reduced each mark to ONE kind too, so it reported those four arms as clean
rings. It now computes `ring+fill`, and the judge FAILS on it, which needed a clause of its own: a fill inside a
ring is NOT a collision (no other state shares it), so the distinctness check that already existed passed it by
construction.

    --passes=pages  probe: 8 families, 0 collisions, 0 ringFill, light and dark, across all six rail pages
    self-test:      the judge fails a planted 'mark-ringfill' defect (46 ok, up from 45)

WHY THE SAME RULE LIVES IN FOUR PLACES, for the next reader who wonders: each one reads a DIFFERENT artefact. The
console gate reads its SOURCE sheet (`gateway/ui/dist` is a pruned build artifact), the panel's unit test reads the
BUILT sheet, and the core probe reads the COMPUTED styles of a RENDERED page. A rule in only one of them would have
missed two of the three copies of this defect — which is what happened: the console found it by mutation (round 44),
the panel's unit test by mirroring the rule (round 45), and the rendered axis not at all until the probe was fixed
(round 46).

### TWO INSTRUMENT BUGS THE PRESS PASS EXPOSED (round 55) — ONE FIXED, ONE OPEN

Adding a press pass meant running the emitted sweep for the first time in a while, and it died immediately:

    FATAL page.evaluate: ReferenceError: inset is not defined

THE FIRST BUG WAS MINE AND NINE ROUNDS OLD. Round 46 rewrote the marks probe's `kind` expression to use `inset`
(`inset && filled ? 'ring+fill' : inset ? 'ring' : …`) and never declared it — the line above defines `shadow`, not
`inset`. Every sweep run since would have thrown the moment the marks probe executed, so the pages pass would have
died in CI on the next push. It survived because round 46 verified the RULE with a reimplementation on the device
instead of running THIS probe, and the judge's self-test feeds the judge a synthetic report rather than the probe's
output. **A reimplementation is not a test of the original.** Fixed by declaring it.

THE SECOND IS OPEN, AND IT IS BIGGER: THE CORE'S `PAGE_CHECKS_TEMPLATE` UNDER-ESCAPES ITS REGEXES. The template is a
JS template literal, so a single backslash before a non-escape character is DROPPED — `/\(/` written as `/\(/`
becomes `/(/`, i.e. a capture group. In the emitted script the loud probe's parser reads

    const parse = (c) => { const m = /rgba?(([^)]+))/.exec(c); … split(/[s,/]+/) … .replace(/s/g, '') };

and on `rgba(252, 251, 250, 0.92)` that yields `r: NaN` — after which EVERY guard is false, because every comparison
against NaN is false. Reproduced directly:

    rgba(252, 251, 250, 0.92)   r=NaN  l=NaN  sat=NaN  skipped=false   <<< COUNTED AS LOUD
    rgb(19, 20, 24)             r=NaN  l=NaN  sat=NaN  skipped=false   <<< COUNTED AS LOUD

So in any EMITTED sweep the loud axis counts every element over 400px2 with alpha >= 0.5, capped by `slice(0, 6)` —
which is why a report from the device shows exactly 6 "loud" elements on all 24 surfaces, including pale containers
like `div#app-shell rgba(252,251,250,0.92)`. The hand-run measurements recorded in this ledger (rounds 18, 40, 42)
were written directly in browser scripts and are NOT affected; the axis they were checking is the one that is broken.
The next round fixes the escaping across the template and re-measures.

FIXED IN ROUND 56, AND THE AXIS IS REPAIRED — WITH ONE THING STILL UNEXPLAINED. Eight backslashes in
`PAGE_CHECKS_TEMPLATE` were doubled (the six replacement sites cover eight escapes: `/rgba?\(…\)/` and
`/rgba\(0, 0, 0, 0\)/` each carry two). A scan of the template now reports **zero** odd-backslash runs, and the
emitted script carries the real regexes:

    emitted parse:  /rgba?\(([^)]+)\)/    and   split(/[\s,/]+/)    and   .replace(/\s/g, '')
    evaluated on the emitted code:  rgba(252,251,250,0.92) → r=252 l=0.984 sat=0.250 → SKIPPED
                                    rgb(19,20,24)          → r=19  l=0.084 sat=0.116 → SKIPPED
                                    rgb(217,72,15)         → r=217 l=0.455 sat=0.871 → counted (correctly)

and the SURFACE probe EXTRACTED FROM THE EMITTED SCRIPT and run against the delivered harness returns
`loud: []` on the panel's Terminal page — which is what the hand measurements in this ledger always said.

WHAT IS STILL UNEXPLAINED: a full run of that same emitted script writes a report whose surfaces still carry SIX
loud entries each, pale containers included (`div#app-shell rgba(252,251,250,0.92)`), and the arrays are
theme-correct so they were computed on the page. Ruled out this round, each by measurement: the report is written
unconditionally at the end of the emitted script; there is exactly ONE `loud` producer in the emitted text and ONE
`SURFACE` definition; the contrast probe has no `loud` at all; the harness is read from the same file both times; a
run pointed at a private report path via `VALE_SWEEP_REPORT` produces the same arrays, so no other writer is involved;
and the same probe evaluated by hand on the same page returns `[]`. The next round starts by making the emitted script
print its own loud array for one surface at evaluation time, which will say whether the difference is the page state or
the code that ran.

SOLVED IN ROUND 57, AND THE CAUSE WAS THE NESTING ITSELF. The probes are embedded in the emitted file as TEMPLATE
LITERALS, so every level eats a backslash: the core's `\(` becomes `\(` in the emitted text (correct for a plain
string) and is then evaluated ONCE MORE by the emitted file's own template literal, reaching the page as `(`. Measured:

    raw emitted text:     const srgb = /^color\(/.test(c);
    value the page gets:  const srgb = /^color(/.test(c);      -> "Unterminated group"
    and the same mechanism turned \s into s, and /rgba?\(…\)/ into a capture group whose parser returned NaN

    THE FIX IS STRUCTURAL, NOT ANOTHER ESCAPE LEVEL: `pageChecks()` now re-embeds every probe as a JSON STRING — the
    idiom the contrast probe has used since round 88 — so there are no levels left to lose. Both sweeps emit and parse,
    and the console sweep gets the same repair from the shared core.

WITH THAT, THE LOUD AXIS READS CORRECTLY ON A REAL RUN, and it agrees with the hand measurements this ledger has
carried since round 40:

    loud histogram over 24 surfaces: {0: 11, 1: 11, 2: 2}
    panel-Terminal light  loud = []                                     unreadable = 1
    panel-Terminal dark   loud = [button.rail-btn.active 1444px2, div.tab 3254px2]   <- the round-42 exception
    judge: "panel design sweep OK: nothing above found a defect"

THREE MORE THINGS THE SAME ROUND HAD TO FIX TO GET THERE: the parser now reads FOUR colour syntaxes (rgb/rgba with
commas, `rgb(r g b / a)` with spaces, and `color(srgb …)` whose components are 0-1 floats); a colour it still cannot
read is COUNTED and SKIPPED rather than passing every comparison (that is how a NaN became "loud"); and the surfaces
of the main page loop are named by DENSITY (`panel-Terminal`, `desktop-Terminal`) so a surface name identifies one
page and the `twoloud` exception matches the rows it was written for. Six backgrounds across 24 surfaces remain in a
syntax the parser cannot read; they are reported as NOTES, not failures, and the count rides in the summary.

AND THE CLASS IS NOW GUARDED (round 58), in the emitted artefact rather than in anyone's memory. Two assertions run
in `panel-design-sweep.bash` on the script `--emit` just produced:

    EVERY PROBE CONSTANT MUST BE A JSON STRING WHOSE VALUE EQUALS THE CORE'S. A probe that goes out as a template
    literal loses one level of escaping before the page sees it (`/^color\(/` → `/^color(/`), which is the bug that
    cost rounds 55-57; comparing PARSED VALUES catches it rather than comparing text.

    NO TEMPLATE LITERAL IN THE EMITTED SCRIPT MAY CONTAIN A SINGLE BACKSLASH. An even run is a literal backslash and
    an odd run ending on a backtick is an escaped backtick; an odd run ending anywhere else escapes the next character
    at the template level and is eaten. THIS ONE FIRED WITHIN SECONDS OF BEING WRITTEN: `MOTION` carried
    `split(/\s+/)`, so the motion pass split class names on the letter "s" — the fourth instance of the same bug this
    session, found by the guard rather than by a symptom.

Mutation: reverting `pageChecks()` to template-literal probes fails with "read 0 probe constant(s) from the core, so
this proves nothing" (47 ok / 0 failed with the fix in place).

AND THE LOUD AXIS IS NOW FULLY SIGHTED (round 59). The six backgrounds it could not read were all ONE syntax —
`color(srgb 0.956863 0.956863 0.960784 / 0.88)` — and the reason was the colour-space NAME: the parser split the
argument list and took `p[0]` as `r`, which is `srgb`, so every component became NaN. It keeps only finite numbers now,
which reads all four syntaxes with one rule. Measured on the device:

    loud histogram over 24 surfaces: {0: 11, 1: 11, 2: 2}   loudUnreadable: 0   (was 6)
    panel-Terminal light  loud = []          panel-Terminal dark  loud = [button.rail-btn.active, div.tab]
    judge: "panel design sweep OK: nothing above found a defect"      unreadable notes: 0

The three unreadable colours a surface does report are now NAMED in the note rather than counted, so the next syntax
the browser invents arrives as its own string instead of a number.

AND THE CONSOLE'S PRESSES ARE MEASURED AS RENDERED TOO (round 60), which is the gap round 54 recorded in its own
words: "Console presses are verified at the SHEET level here; the rendered press measurement exists for the panel and
not yet for the console." The shared `pressPass` is now wired into the console sweep and the judge's clause applies to
its report unchanged:

    6 surfaces · 21 controls pressed · DEAD 0
    overview  measured=5  .rail-btn 40x40 · .btn 48x28 · .card-link 67x20 · .dev-mini 192x46 · .rail-avatar 36x36
    models    measured=4  .rail-btn · .btn 96x28 · .btn-dashed 1052x42 · .rail-avatar
    devices/keys/routes/users: 3 each, every one answering
    judge: "console design sweep OK: nothing above found a defect"

THREE THINGS THAT HAD TO CHANGE TO GET THERE, each worth its line:

  * THE CONSOLE SWEEP HAD NO PASS SELECTION. Every run measured every axis, and the run takes 95-118 s — past the tool
    call's cap, so a single axis could only be checked by paying for all of them (and the first attempt died at the
    timeout with the report unwritten). It has `--passes=` now, baked INTO the emitted script: the first version put
    the helper at node level and the device failed with "wants is not defined", which is the same class of mistake as
    a probe that does not reach the page.
  * THE BUILD IS SHIPPED AS A TAR, which its own header has documented since round 55 — 112 KB, one transfer, extracted
    to `C:\ProgramData\Vale\pwout\console` on the device. Twelve files would have been twelve transfers.
  * THE CLAUSE IS PINNED IN BOTH DIRECTIONS AND FOR THE EMPTY CASE (`sweep-judges.bash`, 11 ok → 14): a pressed control
    that answers passes, a control that renders NOTHING when pressed fails, and a pass that measured zero controls
    fails — because a clause that fails everything is as useless as one that fails nothing.

Console press targets are `.rail-btn`, `.btn`, `.icon-btn`, `.lang-btn`, `.auth-tab`, `.btn-dashed`, `.card-link`,
`.dev-mini`, `.rail-avatar`, `.user-pop-logout`; a target a page does not render is a NOTE, and `measured` keeps a
pass that pressed nothing from reading as clean.

### THE RUNS PAYLOAD — THE PANEL'S MOST-READ WIRE DATA — GETS ITS CONTRACT (round 66)

`.runs` is read more than any other field in the panel (42 reads), and `/api/operation` merges TWO feeds — terminal
events and browser actions — onto one row shape that the panel groups into runs, plans and goals. The device's own
comment names the risk: "this mapping is an ALLOWLIST, so a field missing here is dropped silently with every other
test still green". `run_id` was the first field to make that trip and it had a test of its own; this is the same
contract for all of them, on BOTH feeds.

    agent/tests/fixtures/run-event.json   NEW — two key lists, two `required_by_panel` lists, and one example per feed
    agent/src/operation.rs                both mappings became PURE functions, and the device asserts it sends them
    lib/runs.test.ts                      NEW — the panel groups the device's own example into ONE run

THE EXTRACTION HAD TO BE EXACT, and the first attempt was not: I wrote `text` and `status` into the browser row while
"extracting" it, and the diff against HEAD caught two fields the feed had never sent — a behaviour change dressed as a
refactor. Reverted, and the absence is now written down where it matters: the panel's row parser asks EVERY row for
`text` and `status`, the browser feed carries neither, and its own default turns the `undefined` into null. A row
parser that NEEDED them for a browser row would be reading a fact no feed promises.

    MUTATIONS, one per end:
      the device renames `plan_step` → `planStep`   → the row reads Null where the fixture says 2, and the key sets
                                                      "have drifted apart"
      the panel stops reading `plan_step`           → "expected null to be 2"

AND ROUND 65 LEFT A DEFECT THAT CI WOULD HAVE CAUGHT AND I HAD NOT: its new test was inserted INSIDE `monitor::tests`,
so the module ended up with TWO `use super::*;` and the second was an unused import. `cargo clippy --all-targets
--features terminal,keyring -- -D warnings` — the documented gate — fails on that. Found by re-running the tests and
reading the warning rather than only the exit code; `clippy` is clean now, which it was not an hour ago.

### THE MONITORS PAYLOAD HAD ONE END OF ITS CONTRACT (round 65)

The objective asks for facts "crossing the wire as an explicit contract (agent → API → panel)". The mechanism exists
and is good: eight payload fixtures carry a `required_by_panel` list that the DEVICE's test asserts it sends and the
PANEL's test asserts it reads. Monitors had only the second half — and the panel's own payload test recorded what that
costs, in its own words: "the shape is the contract, nothing validates it, and a wrong guess looks like data". A
hand-written transition shape (`{ts_ms, ok, reason}`) looked plausible, was wrong (`{at_ms, up, lasted_ms}`), and a
transition the parser cannot place in time is DROPPED — so a card drew with no transitions and no error.

    agent/tests/fixtures/monitor-row.json   NEW — the field list READ OFF THE PARSER, not from memory
    agent/src/monitor.rs                    the device asserts it SENDS every promised key
    useMonitors payload test                the panel asserts it READS them, value for value

Two details worth their lines. `summary()` was refactored into a tested core — `summary_of(&[Probe])` with the IO as
one line above it, the shape this ledger calls "a glue wrapper around a tested core" — because the wire shape could
not otherwise be asserted without a data dir. And `serde_json`'s object is a BTreeMap, so the WIRE order is
alphabetical while the fixture keeps the panel's READING order: the test compares SETS, because failing on a
difference nobody can observe is how a contract test earns a reputation for lying.

    MUTATIONS, one per end:
      the device renames `up_pct` → `upPct`   → "the summary the device builds and the fixture promises have drifted
                                                 apart" (left: … upPct …), and the panel would have shown a silently
                                                 absent percentage
      the panel stops reading `last_status`   → "expected null to be 503" — the HTTP target's whole point

ANOTHER GATE FIRED MID-ROUND, unsought: `agent/build.rs` REFUSED the cargo test because the panel source was newer
than `resources/panel/` — "panel.js is embedded at compile time, so this build would ship the OLD panel". Restoring a
file during a mutation is enough to trigger it, which is exactly the class of mistake it exists to catch.

### IDLE REPAINT IS A MEASUREMENT NOW, AND ZERO IS FALSIFIABLE (round 64)

The objective lists idle repaint among the things a claim is "verified by measurement" by, and nothing measured it.
Round 62 added the CONTRACT for one clock (`useNow` schedules nothing while inactive) and that is a unit test about
one hook, not a measurement of the panel. `--passes=idle` is the measurement: a MutationObserver on `#root` counts DOM
writes for six seconds on a settled page, and the harness's fixtures are STATIC — so nothing can change, React writes
only when the rendered output differs, and the bar is exact rather than a threshold.

    panel-Terminal light idle -> total 1 (probe 1, panel 0) in 6s
    panel-Terminal dark  idle -> total 1 (probe 1, panel 0) in 6s
    desktop-Terminal light idle -> total 1 (probe 1, panel 0) in 6s
    desktop-Terminal dark  idle -> total 1 (probe 1, panel 0) in 6s

THE "probe 1" IS THE POINT, and it is why this measurement can be believed. Zero is otherwise unfalsifiable: an
observer attached to the wrong node, or a filter matching nothing, reports a perfectly still panel forever — the
"a scan that read nothing is not a clean scan" trap this suite keeps catching. So the window opens with ONE
deliberate mutation of the panel's own root; the judge REQUIRES that the observer saw it, subtracts it, and fails on
any remaining write with the target named.

    judge clause, three planted cases in `panel-design-sweep.bash` (47 ok -> 50):
      a still panel                          → passes
      three writes to div.totals while idle   → "3 DOM mutation(s) in 6s while idle — nothing changed under static
                                                 fixtures, so this is a repaint of unchanged output (div.totals x3)"
      an observer that saw nothing at all     → "the idle observer did not see its own probe mutation — a blind
                                                 instrument reports a still panel forever, so this measurement proves
                                                 nothing"

TWO MISTAKES ON THE WAY, both recorded because both are the kind this session keeps making: the first version read
`label` for the page name in a scope that has no such binding (the rail walk that defines it is further down), and the
device answered "label is not defined"; and the self-test flag was written inverted (`=== undefined`, i.e. true when
the probe was MISSING), which the printed data caught before the judge ever ran — the clause reads the DATA
(`byTarget.__probe`) rather than the narration, which is the same lesson as round 61. A 44th stray backtick inside the
emitted template ended one `--emit` mid-round, and this time the guard stopped it before the file was delivered.

### A RUNNING COMMAND'S DURATION FROZE WHENEVER IT WENT QUIET (measured and fixed, round 62)

Five components rendered an elapsed time as `Date.now() - startedAt` and re-rendered only when their props changed —
and for a running command the props change when SSE delivers new OUTPUT. A silent command produces none, so the
number froze. MEASURED FIRST, in `CommandCard.test.tsx`:

    × advances while the command is still running, with no new output to trigger a render
      AssertionError: expected '517ms' not to be '517ms'

Five seconds of clock, the same label. The case is not exotic: the ledger's own `sessionActive` comment names it —
"a long command that prints nothing for a while" — and a flash, a probe or a serial write that prints one final line
all behave that way.

THE FIX IS ONE CLOCK, `hooks/useNow.ts`, and the `active` FLAG IS HALF ITS CONTRACT rather than an optimisation. It
ticks once a second and ONLY while the thing it measures is still moving, because a panel that repaints every second
with nothing running is the idle repaint this objective forbids. `useNow.test.ts` pins both halves, and the idle half
is measured as NO TIMER EXISTS (`vi.getTimerCount() === 0`) rather than as "the value did not change" — a timer that
fires and sets the same value is still a repaint:

    now active     → advances with the clock        (3 s of fake time, a timer exists)
    now inactive   → value unchanged, 0 timers      (10 s of fake time, nothing scheduled)
    false → true   → reads fresh on activation      (the first paint after "running" is not a second stale)

Wired into the three renderers of a live command duration (`CommandCard`, `DetailsPanel`, `TrajectoryView`, the last
ticking only while a visible row is running). Two deliberate non-customers: `PluginsPage`'s uptime is POLL-driven —
`started_at` is a wire fact the device owns, refreshed each poll, and a 1 s ticker for an uptime would BE the idle
repaint — and `ContextRail` already had a 30 s ticker of its own.

AND THE GATES CAUGHT THE CHANGE ITSELF: `exports-check.mjs` refused the round because `useNow.ts` exported
`NOW_INTERVAL_MS` that nothing outside the file reads — "an export is a PROMISE that somebody outside needs this".
Dropped. That is the second time this session a gate has reviewed my own work before a human could.

Panel 784/784 (five new tests).

### The DESKTOP density is swept as ONE page, and that is how a two-loud surface stayed invisible (round 40)

Found by probing all of the panel's rail pages at a 1440px viewport — which is the DESKTOP density — in both themes.
Sixteen surfaces, and every other axis was clean everywhere:

    marks       3-5 families per page, ZERO collisions          (the round-27 axis, on every page)
    geometry    0 overflow · 0 slivers · 0 sideways scroll       at 1440px and in both themes
    names       0 unnamed controls · exactly 1 h1 per page
    LOUD        light 0 on six pages, 1-2 on two · dark 1 on seven, 2 on one

It has since been CLOSED (round 41) and this section is corrected where round 40 got it wrong.

TWO CORRECTIONS TO WHAT ROUND 40 FIRST RECORDED, both found while closing it:

  * the desktop density had **two** swept surfaces, not one (`Desktop-empty` and `Desktop-settings-busy`) — the gap
    is that SIX OF THE EIGHT RAIL PAGES were measured in NEITHER density, not that everything but the empty state
    was;
  * the "light Settings loud=2" row was **DARK**. The hand-run loop clicked the rail's EIGHTH BUTTON, which is the
    THEME TOGGLE, and every surface after it was measured in the other theme while being labelled light. That is
    the same failure the sweep's own `theme-lie` axis exists to catch, committed by a probe that had copied every
    OTHER threshold from the core and did not think about which buttons are pages.

    A RAIL BUTTON IS NOT ALWAYS A DESTINATION. Measured: the rail holds EIGHT buttons and SIX are pages; the
    seventh is the theme toggle and the eighth opens the getting-started guide. The first version of the round-41
    sweep clicked all eight, so the toggle would have flipped the theme mid-pass — the fix reads the ACTIVE rail
    button back after each click and, when it has not moved, clicks once more to UNDO the action and skips it.

BOTH NAMES ARE NAVIGATION STATE — the rail button says which page you are on and the tab says which session — and
the loud axis, by its own words, exists to catch two competing FOCAL POINTS rather than two indicators of where you
are. `twoloud` is a supported option that NO CALLER PASSES, and whether they belong in it is the same
reading-dependent question as row 17 of the inbox.

`--passes=pages` now walks the rail in BOTH densities (`mode: 'rail'`, named by the label the app reports, with
actions undone) — 6 pages x 2 densities x 2 themes on top of the 48 panel surfaces, so the six rail pages are
rendered 24 times instead of never. A click that fails to move the active button is skipped rather than counted,
because a report of eight clean surfaces measured on one page is the "a scan that read nothing" trap wearing a
progress bar.

AND ADDING 24 SURFACES TO A PASS IS A CHANGE THAT CAN TURN IT RED, so it was measured before it could (round 42,
with the sweep's OWN theme and loud readings — the ninth probe-lie of this arc was a reinvented theme read). Light is
0 loud on all six panel pages and 0-1 in the desktop. DARK reaches two, on the Terminal page only, in both densities:

    panel    button.rail-btn 1444px2 (which page you are on) + div.tab 3254px2 (which session)
    desktop  button.desktop-rail-btn 1600px2 + button.btn-new 1915px2 (the primary ACTION, which
             state-colour-check deliberately protects: an accent button is an action, not a state)

The loud axis catches two competing FOCAL POINTS, and nothing on that page is about the rail or the tab strip — the
terminal canvas behind them is not loud at all. So the two pages are NAMED (`twoloud`), which is the mechanism the
judge documents for exactly this, and the reason says plainly that the NAME IS A PREFIX: it also covers
`panel-Terminal-16-sessions`, measured at 1 loud today, so it is not hiding a known defect — but a change that made
THAT page shout would pass because of this entry.

`panel-design-sweep.bash` pins both directions on the same planted reading: the named page passes (`2b`), and a page
that is NOT named still fails (`loud-not-excepted` in the axis loop). 45 ok / 0 failed.

AND THE REMAINING AXES WERE MEASURED TOO (round 43), because `loud` is one of eight things the judge and the
contrast probe decide. The sweep's OWN contrast probe (`lib/contrast-probe.mjs`, delivered to the device) ran over
the same 24 surfaces: **980 rows**, and the only two readings under their threshold were

    button.plug-btn.danger  2.26 light / 2.47 dark < 4.5   "Stop"                    Plugins
    button.btn.btn-ghost    2.11 light / 2.44 dark < 4.5   "Notifications unav…"     Settings

both DISABLED controls, which WCAG 1.4.3 exempts and which `failures()` waives with `!r.inactive`. So the verdict
on the new surfaces is 0 violations: contrast, type floor and target size are clean on all 24.

THE WAY THAT WAS ALMOST REPORTED WRONG IS THE POINT. My checker filtered rows by hand with `r.cr < r.need` and
produced eight "violations" — and `contrast-probe.mjs` line 369 already carries the warning, written when the same
mistake was made before: "ghost button after filtering rows by hand with `r.cr < r.need` — which drops the `inactive`
waiver". The eighth time tonight one of my own instruments disagreed with the rule it was copying, and the first
time the rule was ALREADY WRITTEN in the file being copied. Both the round-42 theme read and this one came from
rebuilding a rule instead of calling it: the fix in each case is `failures(rows)`, not a fresh filter.

### The LANDING has no rendered sweep, and that is a decision (measured round 37 of the standing goal)

It is the one surface of the four that no sweep visits, and the reason is not an oversight: `landing-check.mjs` reads
`page.js`'s own values statically and holds the three properties that carry the most risk on a static page — contrast
in both themes, exactly one `h1`, and no card wider than the 320px a reflow test uses. What a rendered pass adds on
top of that was measured by hand, at 1280px and at 320px:

    h1 1 · first-is-h1 · 0 skipped heading levels · 1 main landmark · no nav (a landing page has none by design)
    overflow [] · clipping [] · slivers [] · unnamed controls [] · no sideways scroll at 320px
    LOUD 1 — `a.btn-primary`, rgb(176,58,10), the download CTA. One focal point, the same one round 18 measured.
    4 controls, 2 of them under 24x24, both PASSING BY SPACING (nearest other target 473.8px and 261.5px away)

THAT LAST LINE IS WHY THE SWEEP'S CLAUSE EXISTS. A bare "is it 24x24" check flagged both links and was wrong: WCAG
2.5.8 asks for a 24px CIRCLE OF CLEAR SPACE, which a small inline link in prose has by a wide margin. It is the third
time this session a probe of mine reported a defect the real rule does not have — after the chip's "negative gap"
(round 6, the label was a text node) and the `off` mark at "1.00:1" (round 10, a transition caught mid-flight). A
measurement is not a verdict until the rule behind it is the rule being applied.

If the landing ever grows interaction — a form, a theme switch, anything with state — it needs the sweep the other
three have. Until then this is the coverage, and it is written down so it is a decision rather than a gap nobody saw.

### What the sweeps report when they are clean (measured round 31 of the standing goal)

Worth having, because "clean" without numbers is what makes the next person re-run it. Run on the DEVICE against a
freshly delivered harness, after the mark-language work of rounds 24-27 and the `command_running` wire change of
rounds 28-30:

    --passes=unstyled   panel 1126 styled classes · 0 unreadable sheets · the only unstyled names reported are
                        xterm.js's own DOM (`xterm-viewport`, `xterm-screen`, …), which the sheet injects at runtime
    --passes=motion     panel   normal 14 · reduced 0 · stillAnimating []
                        desktop normal 11 · reduced 0 · stillAnimating []
    --judge             "panel design sweep OK: nothing above found a defect" for both

`--passes=motion` reports TWO numbers per density and the pair is the point: 14 animations exist, and ZERO of them
run for a user who asked for reduced motion. A single number here would be unreadable — 0 with 0 animations is a
pass for the wrong reason, which is exactly the "a scan that read nothing is not a clean scan" trap.

AND `--passes=pages` DOES NOT FIT IN ONE MCP CALL. It is the heavy pass (1356 rows, 48 surfaces) and it exceeded the
caller's cap twice before finishing; the report on the device was still the previous run's. The pass selection DOES
reach the emitted script (`passes: "unstyled"` + a `wants()` gate per pass), so the small passes are the way to check
a change interactively — the heavy one belongs in CI, where it already runs.

The whole RELEASE PATH is now proven, which is the part where a toothless guard ships a broken
release: the prune, the version.json writer, the installer-alias arm and the sha256 gate all fail
when their subject breaks. (Failure messages differ in usefulness: `release-lib` prints actual vs
expected and `smoke-helpers` prints the value it rejected, while `smoke-index` says only "an
advertised installer passes" — accurate, and less use to whoever hits it. Left alone deliberately:
a terse message is not a defect, and churning it buys nothing measurable.)

**Every gate in `scripts/test/` is now audited** (rounds 65-68). A new one should be added to this
table with the mutation that proves it — an unaudited gate is an assumption, and this table is where
that stops being invisible.

TWO THINGS THE AUDIT TAUGHT ABOUT AUDITING (round 67):
  * a gate that asserts a CLEAN WORKTREE rejects a mutation before it can prove anything — so
    `publish-release.bash` can only be mutation-tested by committing the mutation temporarily and
    resetting afterwards. My first attempt read its failure ("the refusal modified the tree") as a
    verdict about the refusal; it was the cleanliness check, and only the committed-mutation run
    showed the truth: the gate passed with the refusal disabled, i.e. the check had NO coverage;
  * a mutation can have SIDE EFFECTS. With the refusal disabled the script walked past it and packed
    a tgz, which the gate's own side-effect check caught. The artifacts were removed; the lesson is
    that "break the guard and see" can also break something, so look for what the run left behind.

THE METHOD HAS A TRAP, and it caught THREE mutations:

  * round 65: `probe_param` inserted at the top level of a tool's JSON instead of inside `properties`
    changed nothing the snapshot reads, so the gate "passed" and proved nothing about the gate;
  * round 68, twice: the zero-selection guard's MESSAGE was deleted while its `process.exit(2)`
    stayed, so the guard still fired and the gate rightly passed; and a "remove `.toLowerCase()`"
    edit matched nothing because the normaliser has no `toLowerCase`. Changing the narration — or
    changing nothing at all — is not changing the behaviour.

Every time the fix was the same: prove the mutation altered the thing under test before drawing a
conclusion about the guard. A "toothless gate" finding is a claim about the gate, and it is worth
exactly as much as the mutation behind it.

### An "untested surface" scan that found nothing (round 70)

Worth recording so it is not re-run: scanning the agent crate for `pub fn`s whose names never appear
in test code lists 65, and every one I checked is a GLUE wrapper around a tested core —
`retention_sweep` → `retention_sweep_in`, `close_abandoned_runs` → `runs::abandon_open_runs`. The
wrappers resolve real `DataDir` paths, so calling them from tests would mutate the developer's own
data; the files say so explicitly ("Deliberately NOT wired into `AppState::new`... those are
constructed by tests"). The apparent gap is deliberate testability design, and the scan's method was
also unreliable in the other direction: its `#[test]` extraction missed real call sites, so its count
is an upper bound, not a finding. Read the module before believing the count.

### A dangling-citation scan is harder than it looks (round 73)

Trying to find every cited path that does not exist, mechanically, took seven attempts and every one
of them lied in a different direction:

  * a GREEDY character class (`[\w./-]*`) let one match swallow the next citation on the same line,
    so the scan reported ZERO dead citations while `docs/CHARTER.md` plainly had one;
  * resolving paths only from the repository root turned every cwd-relative citation into a false
    positive (`scripts/e2e/e2e.js` is `agent/scripts/e2e/e2e.js` when the step runs with
    `working-directory: agent`; `scripts/build-css.mjs` is beside its `package.json`);
  * tokenizing on whitespace then checking BOTH the root and the file's own directory still left
    false positives, and package.json scripts I had run successfully minutes earlier appeared "dead".

Each time, the direct check settled it: `ls` the path, run the command. The scan eventually produced a
usable list, but only after it was made to demonstrate the one case known to be dead — do that FIRST,
before believing any count it prints. What it found is recorded in `docs/agents/ideas.md` row 12.

### THE INSTRUCTION FILE WAS BEING TRUNCATED — SO THE LEDGER MOVED OUT (round 67)

MEASURED FIRST, because the size is the whole story: `AGENTS.md` had reached 65,366 bytes and the workspace harness
truncates an instruction file at 65,536. It was truncating THIS one — the note said so plainly ("truncated AGENTS.md
from 66314 to 65244 bytes") and the bytes it dropped were the TAIL: the Release steps and the Agent-layout map, the
two sections that say how to ship and where the code lives. Nothing failed. Nothing warned, beyond a line in a system
reminder, and an operator's own instructions were being silently cut off at the end of the file.

WHERE IT WENT: the operational sections stay (build, test, the gate table, committing, release, layout) and the
round-by-round narratives moved here, VERBATIM — 11 sections, 38 KB, nothing deleted. The instruction file is 29,046
bytes now, and it loads whole again, which is the only proof that matters: the next workspace-reminder after the split
carried the entire file, Release and layout included, with no truncation note.

    the split, measured:   AGENTS.md 64,972 -> 29,046 bytes · design-ledger.md 37,839 -> 38,097 with this section
    the rule, in AGENTS.md: "If a sentence does not change what you would DO, it belongs in the ledger."

A GATE NOW HOLDS IT, because the failure mode is invisible by construction — a truncated file looks like a file:
`scripts/test/ledger-budget-check.mjs` fails when AGENTS.md passes 48,000 bytes (naming the REAL 65,536), when a
`###` narrative grows back into it, or when the archive is missing or thin — because "pruned" has to mean MOVED, not
deleted. Both mutations were run: an appended narrative section fails with "that belongs in docs/agents/design-ledger.md",
and padding the file past the ceiling fails with the harness's own number in the message.

AND THE FIRST ATTEMPT AT THE SPLIT WAS WRONG in the way this session keeps being wrong: my rebuild concatenated the
table and the narratives and ADDED the pointer, so the file grew to 66,314 — past the truncation point, with the
pointer itself in danger of being cut. Rebuilt from `git show HEAD:AGENTS.md` with the measured spans, and the size
printed at every step rather than assumed.

### CI'S FIRST RENDERED VERDICT ON THIS SESSION'S WORK (round 68)

The release was published and the design job was the first place any of it met a rendered probe. Its first run raised
three findings; the round fixed the shape of all three and left two of them measured-but-not-yet-explained, which is
recorded here rather than guessed at.

**THE IDLE PASS COULD NOT SAY WHAT CHANGED.** Its first CI report read `6 DOM mutation(s) in 6s ... (#text x6)` — one
per second, a live duration ticking and not a repaint, but a text node has no identity, so the finding was
undiagnosable by construction. The probe now names the PARENT of a characterData mutation, and the next run said it
plainly: **`span.approval-left x6`** — the approval countdown, a clock doing its job. That is the whole argument for
naming the parent: the same sentence went from unactionable to diagnosed in one run. The clause still fails every
mutation; what it needs next is the stated exemption for a value that is SUPPOSED to change with time.

**THE LOUD EXCEPTION NAMED PAGES.** It excepted two page names, so CI's other Terminal surfaces —
`Terminal-16-sessions`, `Desktop-16-sessions`, `Desktop-Terminal-fail-dark`, `Desktop-empty` — were not covered while
measuring the same rail button. It is an element allowlist now (rail button, active tab, primary action), and the
self-test proves the property that matters: a page whose two loud elements are BOTH navigation passes, and a page with
navigation PLUS a real second focal point fails. 50 ok / 0 failed.

**THE RAIL DOT: FIXED AT THE TOKEN, STILL 2.33 ON HOVER.** `--rail-ink` feeds `--mark-ink` in exactly two rules and
never any text, so the 3:1 graphic bar applies. It sat at a hard-coded `#9a9aa0` while the DARK theme already used
`var(--chrome-ink-dim)`; the same reference measures **4.40** on the light rail and **6.89** on the active button's
`--accent-soft`, which is what the sheet's own round-126 note had already listed as `.rail-dot 2.63`. After the fix
CI STILL reports 2.33 while hovered, so the surface it measures is not one of those two — the next round runs the
contrast probe against the live rail on the device and finds out which, rather than picking a third token to try.

**AND MY MUTATION DID NOT BITE — THE THIRD TIME THIS SESSION.** I reverted a `--rail-ink` declaration to prove the
unit test would catch it, the test passed, and I read that as a gap in the test. Instrumenting the real test showed
the light map resolving the FIXED value (7.03): my edit had hit a different declaration than the light one. The
ledger's own rule held again — a mutation that does not bite is evidence about the MUTATION first — and the honest
outcome is that the static pair sweep may well have been able to see this all along. What did improve: its surface
choice is a TABLE now (`.rail-dot` and the desktop rail status measured against the rail they land on) instead of a
ternary with one hand-written exception for `.tab.active`.

### TEN FINDINGS BECAME EIGHT, AND ALL EIGHT ARE ONE ELEMENT (round 69)

The exclusions this round added were earned by the run before it, and the design job's list says so:

    10 findings -> 8:  the four idle "repaints" (span.approval-left, a countdown) and the loud findings are gone
    8 findings, all ONE element:  div.mark.rail-dot at 2.33, on six light pages, plus the hover case

**A CLOCK IS NOT A REPAINT.** The idle clause now carries a CLOCKS table — the approval countdown and the three
elements that render the panel's live command duration — each with its reason. Clock changes are reported as NOTES so
the number stays visible, and only the remaining mutations are findings. A false finding is worse than none: it
teaches the next reader to ignore the axis.

**THE APPROVAL GATE IS A PURPOSE.** `button.approval-approve` joined the loud allowlist with the reason
`state-colour-check` already gives it: while a command is held, that control IS the page's primary action.

**AND THE RAIL DOT IS STILL 2.33 — WITH THE FIX PROVABLY IN THE TREE.** This is the open thread, stated exactly:
`--rail-ink` feeds `--mark-ink` in exactly two rules and no other declaration anywhere sets it; the built sheet the
harness reads has the fixed value at line 149 (`var(--chrome-ink-dim)`, 4.40 on the light rail); the LIVE panel on d1
measures its `working` dot at **7.47**. CI still prints 2.33 after the fix, on six pages, which matches
`--chrome-ink-faint` (#a1a1aa, ~2.4 on the rail) — the base `.rail-dot` background — and that rule should lose to the
`data-live` arms on specificity. So the answer is in the HARNESS CI builds, which is the one artefact I have not been
able to inspect from here. Next round: regenerate the harness locally, deliver it to the device, and measure the dot
per state (idle / working / waiting / off / no attribute) — five measurements that will name the winning rule instead
of a sixth hypothesis. The live panel measuring 7.47 while CI's harness measures 2.33 is itself the finding: the two
are not the same artefact, and the difference is worth knowing.

### THE RAIL DOT'S 2.33 IS THE ACTIVE ITEM'S DOT IN THE DARK THEME (round 70), AND THE INSTRUMENT WAS THE PROBLEM

CI's eight findings were one element, and my first four explanations were all wrong. What settled it was measurement,
on the device, with the harness CI builds — and the correction to my own claim first: **the harness was never
uninspectable.** CI's script generates it from this checkout (`panel-render-audit.mjs`) and both stamps read
`235435-cc159cc40dcf`, so CI and I were measuring the same artefact the whole time.

    five forced states, panel density, light:  working / idle / waiting / off / no attribute ALL 7.47
    walking the rail and clicking each button: all dots ok … until the SIXTH, the theme toggle:
        live="working"  ink=rgb(82,82,91)  --mark-ink=#9a9aa0  surface=rgb(26,27,32)  ratio=2.23

That is the state nothing had ever measured: the harness starts LIGHT, and every forced-state test stayed there. Click
the theme toggle and the dot sits on the rail in the DARK theme with the LIGHT ink painted, at **2.23** — CI's number,
reproduced independently.

**WHY MY FIX DID NOT TOUCH IT.** I added `.rail-btn.active .rail-dot { --mark-ink: var(--chrome-active-text) }`,
copying the active tab's precedent — and the measurement came back identical because the rule CANNOT MATCH. The dot is
rendered as a SIBLING:
`<button className="rail-btn active">…</button> … <div className="mark rail-dot" …>`, and the theme and guide buttons
sit between them, so neither a descendant selector nor `.rail-btn.active + .rail-dot` reaches it. CSS also cannot scope
it per item: the item is a React FRAGMENT, so the dot's parent is the RAIL. The fix needs markup — a wrapper carrying
the active state, or `:has()` on one — and that is the next round's work, stated here rather than left as a rule that
silently matches nothing. The dead rule was reverted, because dead CSS is exactly what `stylesheet-hygiene` exists to
catch.

### THE RAIL DOT: FIXED AND MEASURED, AND CI'S PROBE STILL DISAGREES (round 71)

The cause was found by experiment, not by reading. `.rail-dot` carried the only `transition` any mark has
(`background 0.2s ease, box-shadow 0.2s ease`), and on a theme flip every custom property updated at once while the
PAINTED background never did — for three seconds and beyond. Removing the transition in the live page changed the
outcome, which is the proof a cause needs:

    as shipped:              light -> dark:  painted stays rgb(82,82,91)   (2.23)
    with `transition: none`: light -> dark:  painted becomes #9a9aa0      (6.17)

Removed in the sheet, rebuilt, and verified on the same harness in both directions: **light 7.47 · dark 6.17 · back
to light 7.47**. The removal is also what the objective asks for — the chrome is neutral and still, and this is a
state MARK whose motion channel is the WORKING HALO (an animation on the state arms), not a colour cross-fade.

**CI HAS NOT CONFIRMED IT, AND THE DISAGREEMENT IS NOW NARROW.** The design job reports the same eight findings
(`div.mark.rail-dot` 2.33 on six pages, 2.13 on the fail page) with the fix in the tree, while the same harness —
the stamps match — measures 7.47 in light and 6.17 in dark on the device. The six pages are RAIL-WALK surfaces, and
the rail walk clicks the theme toggle and undoes it, so the finding is consistent with a dot measured in the other
theme than its label claims. That is a hypothesis about CI's PROBE, not about the sheet, and it is the next
measurement: run the sweep's own contrast probe against the delivered harness on the device — the same instrument, the
same pages — instead of a hand-written one. My probe reading 7.47 while CI's reads 2.33 on the same artefact is the
same lesson this arc keeps teaching: the instrument that disagrees is the one to interrogate.

TWO SELF-INFLICTED FAULTS FOUND ON THE WAY, both recorded because both were invisible to the gates:
  * round 70's "revert" removed a comment and left HALF a rule: cutting from the comment to the next `.rail-dot {`
    found the brace inside the rule being removed, so `.rail-dot { --mark-ink: var(--chrome-active-text) }` survived
    as a real duplicate. Dead in intent, alive in the sheet, and `stylesheet-hygiene` passed it.
  * a comment written with `//` built fine and reached the sheet as text a browser reads as declarations. CSS
    comments only; rewritten.

### CI'S OWN PROBE SAYS THE SHEET IS CLEAN — SO THE 2.33 LIVES IN CI'S FULL RUN (round 72)

Round 71 ended with a disagreement: the sheet measured clean on the device while CI reported the same eight
`div.mark.rail-dot` findings. This round removed every difference except one.

    CI's harness stamp on a6013f5a:  236375-c729b637b9c6
    my harness stamp:                236375-c729b637b9c6   <- identical artefact

    CI'S OWN PROBE (contrast-probe.mjs, run by me on the device, every rail page, both themes):
        light  paint=rgb(255,239,229) (ring)  surface=rgb(31,31,31)   cr=14.71  need=3
        dark   paint=rgb(255,239,229) (ring)  surface=rgb(3,3,4)      cr=18.36  need=3
    twelve measurements (6 pages x 2 themes), all far ABOVE the bar.

So the shell is not what CI is measuring, and the difference is now down to HOW the full sweep drives the page: my
runs load a page and click a rail button, while CI runs `--passes=all` — which includes the HOVER pass, and the
ledger already records that class of artefact ("a pointer leak left by the dark-hover pass", round 182). The last CI
finding is literally the hover case; the six page findings are consistent with the same leaked pointer.

THE FULL SWEEP IS NOW RUNNING ON THE DEVICE in the background (`Start-Process node … --passes=all`, report to
`C:\ProgramData\Vale\pwout\full72.json`), which is the one experiment that reproduces CI's circumstances exactly.
Next round reads that report: if the dot appears there, its pass (hover vs pages) names the mechanism; if it does not,
CI's own environment is the remaining variable and the honest next step is to run the sweep there with its report
echoed, rather than reasoning about it from here.

WORTH KEEPING FROM THIS ROUND: the probe is a LIBRARY, not a script. `contrast-probe.mjs` exports `PROBE_SOURCE`
(the string the sweep evaluates in the page), `failures`, `contrastRatio`, `parseColour` and more — so "run CI's own
instrument against this artefact" is an import and a `page.evaluate`, not a reimplementation. That is how the twelve
measurements above were taken, and it is the difference between a number I can compare with CI's and one I cannot.

### ONE LINE OF DIAGNOSIS ENDED THREE ROUNDS OF GUESSING (round 73)

Making the finding self-describing paid for itself on the first CI run that carried it:

    2.13 panel/Terminal-fail-light div.mark.rail-dot — painted rgb(82,82,91) (border) on rgb(31,31,31), 10px graphic
    2.33 panel/Settings        div.mark.rail-dot — painted rgb(146,64,14) (background) on rgb(31,31,31), 11.31px
    … six pages, the same two facts

Read it: `rgb(146,64,14)` is **`#92400e` = `--warn-ink` IN THE LIGHT THEME** (the dark one is `#ffc078`), and
**11.31px** is an 8px square ROTATED — the WAITING silhouette, the diamond. And the surface is `rgb(31,31,31)`: a
DARK backdrop under a page the label calls `panel/Settings`. So the dot is not the defect and neither was its
transition. What CI is measuring is **a page rendered in the DARK theme and labelled light** — round 40's own defect,
recorded in this ledger as "the hand-run loop clicked the rail's EIGHTH BUTTON, which is the THEME TOGGLE, and every
surface after it was measured in the other theme while being labelled light" — alive in the sweep's rail walk today,
and it explains every remaining finding at once: the light ink on the dark rail, the dark `rgba(217,72,15,0.9)` loud
entries from earlier runs, and the hover case.

**THE LESSON IS THE ONE THIS ARC KEEPS TEACHING, AND IT TOOK THREE ROUNDS TO APPLY IT AGAIN.** I chased the element
(a transition, a token, a selector, an active state) while the finding said only "2.33". The idle pass had already
shown the way in round 69 — naming the PARENT turned "6 mutations (#text x6)" into "span.approval-left x6" and made a
one-line decision out of a hunt — and the contrast axis needed the same treatment: `paint`, `surface`, `kind`, `size`.

    before:  2.33 panel/Settings div.mark.rail-dot ""
    after:   2.33 panel/Settings div.mark.rail-dot "" — painted rgb(146,64,14) (background) on rgb(31,31,31),
             11.31px graphic, needs 3

NEXT ROUND: fix the rail walk's theme handling — either undo the flip reliably, or label each surface by what the page
REPORTS rather than by what the loop intended, which is what the `theme-lie` axis exists to enforce and evidently does
not reach these surfaces. That is a defect in the INSTRUMENT, and it has been reporting six false contrast findings
and a set of false loud findings for as long as the walk has run.

### THE APP DISAGREES WITH ITSELF: data-theme="light" OVER DARK PIXELS (round 74)

Two fixes went in this round and the finding stayed identical — which is itself the result, because the finding now
carries the theme read from the element the app actually writes:

    attr is read from <body> now   (it read <html> until round 74, so it was "(none)" on every surface ever swept,
                                    the theme-lie axis had two sides only by accident, and the rail walk's first
                                    labelling fix silently fell back to the loop's intention)
    the label is a READING          (rows, surfaces and names take the page's own attribute, not the loop's hope)

And the six surfaces still come back as `panel/<page>` with a DARK surface: `<body data-theme="light">` while the
pixels are `rgb(31,31,31)`. On the panel density, flipping the theme on the device and flipping it back is perfect —
shell, dot and `--warn-ink` all follow, and the attribute tracks them:

    start        attr=(none, on <html>)  body=light  shellBg=rgba(252,251,250,0.92)  warnInk=#92400e
    after flip 1 body=dark               shellBg=rgba(26,27,32,0.94)                warnInk=#ffc078
    after flip 2 body=light              shellBg=rgba(252,251,250,0.92)             warnInk=#92400e

So the disagreement is produced by the RAIL WALK's sequence, not by a single flip: the walk clicks every rail button,
the theme toggle among them, and then clicks it again at the same INDEX to undo it. If the button list shifts under it
— and it does, because navigating can add or remove rail controls — the undo lands elsewhere, and what remains is an
app whose attribute says one thing and whose paint says another. That is the objective's own subject, in the product
rather than the instrument: "the interface carry its state".

NEXT ROUND, ONE IDEA: SUPPRESS TRANSITIONS ACROSS A THEME FLIP. Round 71 proved one element (`.rail-dot`) froze
because it transitioned its own colour while the custom properties moved; the shell's backdrop is the same mechanism
at page scale, and the standard remedy is a class applied for one frame around the swap (`transition: none` on
everything, removed after a reflow). That fixes the class rather than the instance — and the sweep's rail walk should
also address its buttons by LABEL rather than by index, which is how the undo came to land somewhere else.

AND ONE MORE SELF-INFLICTED FAULT, caught by reading the emitted VALUE rather than trusting the edit: my first
attempt at the comment replaced the `const attr = ...` line along with the prose, so the probe returned
`attr: undefined` and the commit that claimed to fix the read changed nothing. The self-test was green through all of
it. Reading the artefact back — the emitted string, not the file I edited — is what caught it, and it is the same
lesson as round 57 and round 71.

### SOLVED: ONE MISSING COLOUR SYNTAX, AND THE PANEL GOES GREEN (round 75)

Five rounds chased an 8px dot. The defect was in the PROBE's colour parser, and the arithmetic is the proof:

    getComputedStyle(nav#icon-rail).backgroundColor   =  color(srgb 0.956863 0.956863 0.960784 / 0.88)
    parseColour, before:   the numeric scrape read those 0-1 floats as channels -> rgb(1,1,1) at 0.88 alpha
    composited over the shell:   0.88 x 0.956863 + 0.12 x 252  =  31   ->  rgb(31,31,31)
    parseColour, after:    r=244 g=244 b=245 a=0.88 -> composited rgb(245,245,246) -> the dot measures 7.09

`rgb(31,31,31)` is EXACTLY what CI reported as the surface under every `div.mark.rail-dot`. Every mark on the rail was
being judged against a near-black backdrop, and the six findings were the probe's arithmetic, not the interface. The
ancestor chain — printed from the live page — is what ended it: the dot's parent is `nav#icon-rail`, whose background
is that srgb form, and the surface was never dark at all.

THE SAME BUG THE LOUD AXIS FIXED IN ROUND 59. That round taught the loud parser four colour syntaxes and kept only the
finite numbers; nobody carried the rule across to the contrast axis, which had no srgb case in its own gate — 20 green
checks that proved nothing about the form getComputedStyle actually returns for a token declared with a colour
function. It has 21 now, asserting the channels AND that a genuine 0-255 triple is not scaled, and the gate also caught
the first version of my comment (backticks inside a function that is inlined into a template literal — the 47th time
this session) before it could reach a page.

WITH THAT, THE DESIGN JOB FAILS ONLY ON THE CONSOLE, and the remaining findings are real and different:

    devices@1440/900/720/640/320 + devices-dark:  dev-led[off] is a FILL inside a RING — the vocabulary is
        solid / ring / halo / empty, and a mark that is two of them is neither
    users-dark@1440px:  2 loud elements — a.rail-btn active 1600px2 and span.badge.badge-info 1218px2 rgb(26,58,92)

Both come from the RENDERED probe and neither is visible to the console's sheet-level gate, which is the same
sheet-versus-cascade lesson rounds 44 and 45 recorded on the other surface. That is the next round's work, and it is
the first time the console's rendered axis has had anything to say.

### THE DESIGN JOB WAS SWEEPING THE LAST PUBLISHED CONSOLE (round 76)

Eight findings became three this round, and the mechanism is worth more than the count. The six `dev-led[off]`
findings survived TWO correct fixes — the fill arm deleted, the transparency spelling taught — and a fresh build of
the same source measured clean with the sweep's own marks probe:

    devices @1440  dev-led[on,off]   ringFill: []   collisions: []

The reason was the ROOT. The console arm of `design-sweep-ci.bash` served `gateway/public`, which the RELEASE flow
copies a console into, so the job's verdict was a statement about the artefact from the LAST PUBLISHED RELEASE while
its own header says it runs "against what this repository builds". The findings were neither stale nor false: they
were TRUE OF THE ARTEFACT CI MEASURED. Every instrument was right and one was pointed at the wrong thing — the panel's
rail dot, one level up.

    the script now builds:  ( cd gateway/ui && npx vite build --outDir "$TMP/console-build" )  and serves THAT
    the job now installs:   gateway/ui's dependencies, which that build needs

    eight findings -> three, and each one is specific:
      users-dark@1440px: 2 loud — a.rail-btn active and span.badge.badge-info 1218px2 rgb(26,58,92)
      the delivered entry is 1375 bytes / sha 19186df2 but this sweep was emitted against … 2ab1c505 — a stale build
      class name(s) with no matching rule on overview: online — on screen, matched by nothing

THE THREE ARE THE NEXT ROUND'S WORK, and the first is the same idea as the panel's: the console's loud clause has no
element exception, so the RAIL BUTTON — navigation state, which the panel's clause excepts with a reason — counts as a
competing focal point. The second is the emit's own root: `EXPECTED_ENTRY` is baked when the sweep is emitted, from
`gateway/public`, so a run against a fresh build always reports its entry as stale. The third is a real one: a class
the console renders that no rule matches.

### ONE ROOT, BOTH ENDS: THE STALE-ENTRY FINDING WAS THE EMIT READING A DIFFERENT ARTEFACT (round 77)

Three findings became two, and the one that went was the sweep accusing itself. The stamp is baked when the sweep is
emitted and compared at run time, and baking it by hand showed the two ends were reading different consoles:

    baked:  {bytes: 1375, sha: 2ab1c505f928}      <- gateway/public, the last PUBLISHED console
    actual: {bytes: 1375, sha: 19186df2454b}      <- the console this checkout builds

`ENTRY_STAMP` read a hard-coded `gateway/public` while the run serves `VALE_SWEEP_ROOT`; and the CI script set that
variable for the run and not for the emit. Both halves now read one root, with the default unchanged so a DEVICE run
still compares the delivered copy against the copy it was emitted for. Verified both ways: with the root set the stamp
matches the fresh build; with no environment at all it still resolves to `gateway/public`.

The check itself is not wrong — it is the guard for the device pipeline, where a script emitted here is delivered
there, and round 184 found that directory holding eight files from four generations. What was wrong was pointing its
two ends at two artefacts and reading the disagreement as a fact about a commit. Third time in four rounds: the
instrument was right about the wrong thing.

WHAT IS LEFT, both real and both small:
    users-dark@1440px: 2 loud — a.rail-btn active (navigation: the core's NAV_LOUD already excepts it with a reason,
        which is how the clause works — EVERY loud element must be navigation) and span.badge.badge-info 1218px2
        rgb(26,58,92), a saturated navy fill on chrome. The objective says the chrome is NEUTRAL and colour belongs to
        the state layer; a role badge is not a state. That is the next round's first idea.
    class name(s) with no matching rule on overview: online — on screen, matched by nothing. A class the console
        renders that no rule styles.

### CI IS GREEN, ALL TEN JOBS (round 78)

    index · agent e2e · agent xwin check · pack-chain · DESIGN · ui · gateway · proxies · panel · agent cargo

The design job is green for the first time in this arc. Five rounds of instrument work got it there, and the ORDER
matters more than the list: the panel's six false findings hid the console's one real finding, the console's finding
was measured against a stale artifact, and the stale artifact came from a root that two ends read differently.

    1. `color(srgb ...)` is 0-1 floats (round 75) — six false findings against the panel vanished.
    2. CI swept the LAST PUBLISHED console (round 76) — build it from the checkout.
    3. The entry stamp read a different root than the run (round 77) — one root, both ends.
    4. A status surface is a tint, and ONE function decides it (round 78): the dark info chip was a saturated block
       while its light sibling was a pale tint the rule skips by design, `--success-bg` was already a tint, and the
       new sheet-level case asks the question of every state surface in both UIs and both themes.
    5. `online` was a class on a Link that no rule matched — a second name for a fact the LED already carries.
       Pruned, and the prune broke the build because a JSX comment sat at the top of a `return`, and I called tsc
       clean because I read `$?` after a pipes — the table in AGENTS.md, read too late. Again.

WHAT THE RELEASE TAUGHT, and it is worth writing down because the number will come up again: `release.yml`'s first
real step is "Gate on tag-commit CI status", so a release is gated by the CI of the commit the TAG points at. The tag
`v1.2.434` points at `cc7ccfac`, whose tree is genuinely broken — the design job failed on it for the srgb parser the
panel findings came from — so that release's asset can never be built, and re-running the workflow re-tests the same
broken tree. The gate is right; the release was pointed at the wrong commit. 1.2.435 is published from this green main
instead, which is the first time the pipeline runs end to end on a tree whose CI is green.

### THE CONSOLE'S FIRST IDLE MEASUREMENT: CLEAN (round 79)

The panel has had an idle pass since round 64; the console had **never** been measured for it, and its views poll twice
a second — so "the page is settled and writing nothing" was exactly the claim its live views could break. The
objective asks for idle repaint to be verified on both ends, and this was the end with no instrument.

    every console page, both themes, a six-second window each: 36 seconds for the whole axis.

THE JUDGE ALREADY KNEW HOW TO READ IT — the idle clause lives in the shared judge, so the console's rows are judged by
the rule the panel's are, including the CLOCKS exemption (round 69: a countdown is not a repaint) and the parent-naming
that turned "6 mutations (#text x6)" into "span.approval-left x6". The round added the instrument and nothing else.

MEASURED ON THE FIRST CI RUN CARRYING IT: no idle findings on any console page in either theme. The console polls
`/api/*` on its own schedule and still paints nothing it did not have to — which is the property, now verified rather
than assumed. (The panel's own axis reports the same, with its live command duration exempt by name.)

ONE PROCESS NOTE WORTH KEEPING: the idle pass takes longer than one MCP browser call tolerates, and a backgrounded
run on the device produced no report I could read before this round ended. CI is the instrument that settled it — a
sweep committed to the repository is measured on every push, which is the cheapest possible way to learn whether a new
axis finds anything. Reach for the device when a finding needs a human-scale look, not to discover a first result.

### THE OBJECTIVE, CLAUSE BY CLAUSE, AND THE ONE SURFACE WITH NO ARM (round 80)

Round 80 went looking for the next defect and found four clauses already implemented and gated. That is worth a table
rather than another round of hunting, because the next 900 rounds need to know where the coverage ENDS.

| the objective says | the gate that owns it | measured, as of round 80 |
|---|---|---|
| one source of truth per fact, crossing the wire as an explicit contract | `agent/tests/fixtures/*.json` + their `required_by_panel` assertions on BOTH sides (session-row, sse-frames, status, settings, vitals-series, boot-history, approval-grants, embedded-bridge, monitor-row, run-event) | 10 wire facts, each asserted by a device test AND a panel test |
| every state has its own SILHOUETTE — shape first | `mark-vocabulary-check` (panel vs console, read as data and as paint), `console-marks-check` (11 marks, 5 families, `ring+fill` is its own FAILING kind) | panel 4 states, console 11 marks, both agree: attention=diamond, absent=ring, fine=fill |
| the chrome neutral and still | `chrome-stillness-check` (15 animations, each with a declared purpose), `state-colour-check` (152 state-colour uses on purpose; every state SURFACE ≤ the loud bar — `loudnessOf`, shared with the rendered probe) | 8 STATE / 5 ENTRANCE / 2 ATTENTION; the dark info chip fixed this arc |
| immediate feedback on every input, transform/opacity only, inside a stated budget | `feedback-check` — three rules: a press exists for every pressable hover, no press is fully shadowed, and **no transition above 240 ms**; plus `pressPass` rendered on BOTH UIs | sheets sit at 120/150/200 (panel), 180 (console), 200 (landing) — all under the stated 240 |
| one focal point per surface | the rendered loud axis (shared rule) + `NAV_LOUD` element exceptions with reasons | panel and console both green |
| idle repaint | `idlePass` on the panel (6 pages, 2 densities) and — since round 79 — on every console page in both themes | both clean; clocks exempt BY NAME with reasons |
| reduced-motion | `motion-check` (no animation escapes the block), `particles-check` (the canvas refuses to run), rendered verification on the device | panel, console, landing |
| whatever stops earning its place is pruned | `exports-check`, `css-vars-check` (both directions), `unstyled` axis, `retired-colours-check` | the `online` class pruned this arc |

**AND THE ONE SURFACE WITH NO RENDERED ARM IS THE LANDING.** `landing-check` is STATIC — it reads `page.js`'s own
values and asserts contrast, exactly one `h1`, and a 320px reflow — and `particles-check` holds its canvas to five
facts including the reduced-motion refusal, verified on the device once. But no browser renders the landing in any
gate: the panel has a harness arm and the console an arm that builds from the checkout, and the landing has neither.
Everything a static read cannot see is therefore unmeasured there — the cascade as painted, the `hint` that replaces
the installer button when a release publishes none, focus order, and the rendered press. That is the next round's
work, and the objective's "all four surfaces green" is the reason it matters: three of the four are measured as
rendered and the fourth is measured as text.

### THE FOURTH SURFACE IS RENDERED, AND IT FOUND THREE THINGS ON ITS FIRST DAYS (round 81)

Three of four surfaces were measured as rendered; the landing was measured as TEXT — `landing-check` reads
`page.js`'s own values — and a static read cannot see the cascade as painted, a press, or a state swap.
`agent/scripts/landing-design-sweep.mjs` serves the page the WORKER serves, in BOTH installer states and both colour
schemes, by calling `PAGE()` — the same entry point `index/src/index.js` serves and the index tests call, so there is
no second renderer to drift. Its first CI run measured **98 text nodes, 4 surfaces, 4 name checks**.

    FINDING 1 — FIXED THE SAME ROUND: "a (a) renders NOTHING when pressed — before and during are identical
    (79x17)", in BOTH schemes. The page has two link families with a hover each and no press at all. The console's
    anchor and card-link recorded this class first: A TRANSFORM DOES NOT MOVE AN INLINE BOX, and the remedy there was
    opacity. Same remedy here, placed AFTER both hover rules because hover always co-occurs with active and at equal
    specificity the later one wins. The next run does not contain the finding.

    FINDING 2 — NAMED, NOT YET FIXED: the press pass measures ONE control on the landing, and the shared clause says
    a pass that pressed nothing proves nothing. A page with one pressable family is not the panel's page; the honest
    answer is either a selector list that covers what the landing actually renders in BOTH installer states, or a
    stated floor for a page this small. Not decided yet, and not guessed at.

    FINDING 3 — NAMED, NOT YET FIXED: "reduced motion (landing): 5 element(s) still animate — body trans=0.2s,
    .theme-toggle trans=0.2s, a trans=0.2s". The landing has NO prefers-reduced-motion block, and motion-check reads
    the panel's and the console's sheets — not this one. So the surface that talks about respecting a person's
    settings keeps its transitions when they ask for less motion.

AND THE ROUND'S OWN MISTAKE, the forty-ninth of its kind: my comment used backticks inside page.js's template
literal, the emit exited 1, and the rendered pages carried no `a:active` at all — the grep of the RENDERED html is
what caught it, twice, because the sheet is a string until PAGE() builds it.

### ALL FOUR SURFACES, MEASURED AS RENDERED, GREEN (round 82)

The landing's arm took three CI runs to judge clean, and every finding on the way was the instrument rather than the
page — which is the same lesson as the console's arm three rounds earlier, arriving on a smaller surface.

    1. "a renders NOTHING when pressed", both schemes — a REAL defect, fixed with the console's remedy (opacity,
       after both hover rules, because a transform does not move an inline box).
    2. "reduced motion (landing): 5 element(s) still animate" — a REAL defect: the canvas already refused under
       reduce and the CSS was never asked, so the theme toggle, every link and the body kept their 200 ms transitions.
       motion-check reads the panel's and the console's sheets and had never read this one. Fixed with the block all
       three surfaces now share, 0s rather than a shorter duration.
    3. "the press pass measured 1 control(s)" — MY LIST was stale: it named a class the page has never had and missed
       the theme toggle. The shared floor of two exists to make exactly that loud, and it did its job.
    4. "the delivered entry is 29707 … but this sweep was emitted against -1 / (unreadable)" — MY ORDER: the stamp was
       computed at module load, before --emit had rendered anything. Read after the render now, verified byte for byte.
    5. "only 22 styled classes found (floor 100)" — the shared floor is the PANEL's scale; the landing is one page
       with 22. The floor's job is to make a broken collector loud, so the landing states 15.

TWO OF THE FIVE WERE REAL DEFECTS IN THE PAGE, FOUND WITHIN TWO RUNS OF THE ARM EXISTING. That is the argument for a
rendered fourth surface in one line: the static check had read `page.js`'s own values for as long as the file existed,
and neither the press nor the reduced-motion block was visible to it.

The verified state: panel OK, console OK, landing OK, and CI green on every job.

### THE IDLE AXIS ON THE FOURTH SURFACE: NOTHING AT ALL (round 83)

The panel has had idle repaint since round 64, the console since round 79, the landing never. Wired this round, four
windows of six seconds — both installer states, both colour schemes — and the verdict is the strongest of the three:

    landing design sweep OK: nothing above found a defect

NOT EVEN A CLOCK. The panel's axis needed an exemption for its live command duration (round 69's CLOCKS table, declared
by name with a reason); the landing's footer clock is a single `textContent` assignment at load rather than a timer,
and its canvas paints without touching the DOM — so a settled landing writes NOTHING, and the shared table stayed
empty for it. That is the right order: declare an exemption when the measurement names one, never before, because a
table that grows in advance is a table nobody reads.

All three surfaces now measure the objective's idle clause, each with the rule inlined from the same source and the
same exemption list, so the axis cannot drift between them.

    panel (6 pages x 2 densities) clean · console (every page, both themes) clean · landing (4 windows) clean

### THE LANDING'S FOCUS PASS: CLEAN, WITH ONE RULE IN THE WHOLE PAGE (round 84)

The panel and the console have walked their controls with Tab for many rounds; the landing never had, and it is the
surface with the least focus styling — exactly ONE `.btn-primary:focus-visible` rule against a theme toggle and two or
three links. Fourteen Tab presses per surface, once for each of the four (two installer states, two colour schemes),
and the verdict is clean: focus is visible on every control the walk reaches, and Tab does not escape the page.

That the pass found nothing is the useful part: the page's single focus rule is on the PRIMARY ACTION, and the
browser's own default ring covers the rest — which is what a page with no framework reset gets for free and what the
panel had to earn with its own sheet.

Wired with the pass inlined from the shared source and judged by the shared clause, like every other axis: the landing
now measures contrast, marks, names, unstyled, targets, press, idle, reduced motion AND focus. The one axis left on a
STATIC proxy is reflow — `landing-check` reads `page.js`'s own values for the 320px question, while the panel and the
console measure the rendered document's scrollWidth. That is the next round's work.

### THE LANDING MEASURES REFLOW AS RENDERED — AND THE FOUR SURFACES NOW COVER THE SAME AXES (round 85)

The last axis on a static proxy. `landing-check` answered the 320px question by reading page.js's own values — a proxy
for the measurement the panel and the console take from the rendered document — and WCAG 1.4.10 is a claim about what
the BROWSER lays out. Wired this round: both widths the standard names, in BOTH installer states, because the npm-only
state swaps a button for a sentence and that is exactly the change that pushes a line past the viewport.

    landing design sweep OK: nothing above found a defect

FOUR MEASUREMENTS, and the page passes at 640 and 320 in both states. The note the panel produces about toolbar
scrollers belongs to the panel's own tab strip and does not appear for the landing — which is the right shape: the
shared judge prints it only where it happens.

THE COVERAGE TABLE FROM ROUND 80, COMPLETED. Every axis the objective names is now measured AS RENDERED on all four
surfaces, with each pass inlined from one shared source and each clause judged by the shared judge:

    axis              panel   console   landing
    contrast            yes      yes       yes
    marks/silhouette    yes      yes       yes
    names               yes      yes       yes
    unstyled            yes      yes       yes
    targets             yes      yes       yes
    press               yes      yes       yes
    idle repaint        yes      yes       yes
    reduced motion      yes      yes       yes
    focus               yes      yes       yes
    reflow              yes      yes       yes
    loud/focal point    yes      yes       yes

`landing-check` keeps its job — it reads page.js's own values, which is how a STATIC claim about the palette and the
heading structure is checked without a browser — and it is no longer the only thing standing behind the landing's
reflow claim.

### A NOTE THAT SAID THE OPPOSITE OF THE CODE, AND THE CHECK THAT REPLACED IT (round 86)

The panel harness carried this paragraph for sixty rounds:

    "WHAT THE HARNESS STILL DOES NOT DO, measured round 115 ... window.EventSource below is a NO-OP, so the panel's
     SSE stream NEVER opens. Every SSE-driven surface therefore renders its 'Connection lost - reconnecting' state,
     and has in every sweep this harness has ever produced."

It was true when written. Round 156 added the `/api/events/term` branch — one empty frame, then close, which is what
flips the app to connected — and the note BESIDE that branch says so: "round 156 made the panel render CONNECTED with
this exact shape". Two paragraphs in one file, contradicting each other, and the stale one is the one a reader meets
first. It very nearly made me "fix" a working fixture.

MEASURED, NOT ARGUED: the harness now publishes `window.__sse = {opened, fail}` — a fact only it knows, because the
app fetches a stream rather than using EventSource — the panel sweep reads it per surface, and the shared judge fails
any surface that should be connected and was not. CI's next run, green:

    4142 rows · 90 panel surfaces · every reporting surface opened: true

So the harness does deliver the push and the panel does render connected, and the sentence that said otherwise is
gone. The failure surfaces are exempt on purpose (`?fail=1` rejects every /api/ call, so they are SUPPOSED to read as
reconnecting) and the harness reports that flag beside the other rather than leaving the judge to guess from a label.

Proven both ways before shipping, on synthetic reports: `opened:false, fail:false` fails with "the harness never opened
the SSE stream, so this surface was measured in the RECONNECTING state — the fixture failed, not the panel";
`opened:false, fail:true` passes; the clean case passes.

THE LESSON IS NOT "A COMMENT WAS STALE". It is that a claim about an INSTRUMENT sat in prose for sixty rounds with
nothing checking it, while every other claim in this suite has a clause. A sentence about what a fixture does belongs
in the same place as a sentence about what the product does: next to a check that fails when it stops being true.

### TWO WITNESSES FOR ONE FACT, AND THE SECOND STALE PARAGRAPH (round 87)

Round 86 corrected the note that said the panel's stream never opens. Directly below it sat another, stale for the same
reason:

    "AN ATTEMPT TO OPEN THAT STREAM WAS MADE AND REVERTED ... with the stream shut, every panel measurement this
     harness has ever produced was taken in a reconnecting state."

Round 154's attempt was reverted; round 156 put it back and it has served the connected state since. The history is
kept — the symptom (one empty frame put the app on the CONNECT SCREEN) is what a future reader needs to recognise —
and the CONCLUSION is marked as belonging to round 154, because a reader who takes it for the present tense distrusts a
fixture that works. That is the second time in two rounds that a sentence about an INSTRUMENT was the defect.

AND THE FACT HAS TWO WITNESSES NOW, because one flag is one place to lie. The harness reports `{opened, fail}`; the
judge ALSO reads the rendered text. "Sessions unavailable" is the panel's own sentence for a push that never arrived, so
on a surface whose harness did not report the failure fixture, seeing it means the measurement describes a screen the
operator never sees. The failure surfaces report their own flag rather than the judge guessing from a page name —
`?fail=1` rejects every /api/ call, so they are SUPPOSED to say it.

    MEASURED, CI green: panel 4142 text nodes · 90 surfaces · NOT ONE of them renders "Sessions unavailable" on a
    surface whose harness did not report the failure fixture.

So the harness delivers the push, the panel renders connected, and two independent clauses now say so — one about what
the fixture did, one about what the panel concluded from it. Proven three ways on synthetic reports before shipping:
clean passes, a missing push fails on BOTH witnesses, and the failure fixture passes.

### THE FOURTH SILHOUETTE'S FIRST PHOTOGRAPH, AND EVERYTHING IT EXPOSED (round 88)

The harness's own note had said for many rounds that the page sweep, "which photographs pages and never presses", has
never photographed `off` — a closed session is CLIENT state, so no URL parameter can produce it. It is reachable by
PRESSING, and the recipe was already written down (click the tab's x, click the confirm's Close, read after the 0.15s
background transition). Round 245 made it stable. The surface now exists in both densities and both themes, with
`mode=pending` so the waiting diamond is on screen too — and it is the only surface where ALL FOUR STATES can be
compared at once.

IT FOUND FOUR THINGS, IN A CHAIN, AND THREE OF THEM WERE OLDER THAN THE SURFACE:

1. **THE MARKS PROBE COULD NOT SEE A BORDER.** `kind` read FILLS and INSET SHADOWS only:

       const kind = inset && filled ? 'ring+fill' : inset ? 'ring' : filled && shadow !== 'none' ? 'halo' : filled ? 'solid' : 'empty';

   The panel draws BOTH of its rings with a border — idle is `1.5px solid`, off is `1.5px dashed` — so both computed
   as 'empty', and the first surface to put them side by side reported "idle and off paint identically
   (50%/flat/empty)". A border is a ring now, a dashed or dotted one is 'dashed-ring' (its own kind, because the dash
   IS the design decision the state encodes), and a border that PAINTS NOTHING is not a ring at all — that last
   refinement came from the probe immediately reporting three more collisions, which were transparent borders until
   proven otherwise.

2. **THREE REAL DEFECTS, EXPOSED BY (1).** `mark[idle]` on Desktop-empty and `mark[off]` on both desktop failure
   surfaces were FILL inside a RING. The cause was specificity and order: `.mark[data-live="idle"]` (0,2,0) sets
   `background: transparent` and a border, `.desktop-rail-status .dot` (0,2,0) sets `background: var(--warn)` and an
   amber glow LATER in the sheet — equal specificity, later wins, so the state's own fill and halo never applied on
   the desktop rail. The comment beside it already said what it was for ("kept as the base so a missing data-state
   degrades to 'not sure'"); scoping it to `:not([data-live])` is what that comment describes.

3. **A TEXT FINDING COULD NOT SAY WHAT IT MEASURED.** Round 73 added paint/surface/kind to the judge's message so a
   finding would explain itself; the GRAPHIC rows had carried those fields since round 125 and the TEXT rows never
   had, so the axis that produces most of the findings printed "painted undefined on undefined, 12px undefined".

4. **AND THAT MADE A REAL CONTRAST DEFECT ACTIONABLE.** With the evidence in place the finding read
   `rgb(177,177,181) on rgb(255,255,255)` — 2.13:1 for the closed tab's own name, because `.tab.closed { opacity:
   0.45 }` dimmed the whole tab on top of the dimmed ink beside it. The panel had already decided this one component
   over, in the sidebar's words: "the ink stays legible and the shape says closed, which is the rule the whole mark
   language follows". The tab follows it now: 7.73:1 light, 6.47:1 dark, with the dashed ring, the line-through and
   `cursor: default` still saying closed.

AND ONE FAILURE THAT ONLY CI COULD SEE, worth its line because no local gate can: the text rows began using `rgbStr`,
which was declared BELOW them — a const in the temporal dead zone. `node --check` passed, the emit passed, every gate
was green, and the browser threw "Cannot access 'rgbStr' before initialization". The probe is a SCRIPT THAT RUNS IN A
BROWSER; the gates around it check its SHAPE, not its execution. The file's own round-46 note records the same class
from the other end.

### THE HELD SESSION'S FIRST PHOTOGRAPH, AND THE SECOND STATE IN TWO ROUNDS WITH NO TEST (round 89)

Round 88's lesson was that a state no surface renders is a state no surface measures; round 89 went looking for the
next one and found it in a wire fact the panel is most careful about. `held_by_human` is SERVER-OWNED — SessionControl
reads it off the session record and will not flip its button before the server agrees, "because another client taking
the session shows up here too" — and EVERY seed this harness has ever built set it false. So:

    .sc-dot[data-state="human"]   a solid fill        NEVER RENDERED
    .sc-dot[data-state="ai"]      an inset ring       rendered on every page
    #session-control.held         the button variant  NEVER RENDERED

`?held=1` holds the FIRST session and leaves the rest, so both states of the family are on one page — the marks probe
compares states WITHIN a family and can only see a collision between two that are both on screen.

WHAT THE FIRST PHOTOGRAPH FOUND:

    panel/Held-dark [dark] span.sc-dot — painted rgb(191,58,10) (background) on rgb(61,40,23), 7px graphic, needs 3

The dot that says a PERSON holds this session's keyboard sits on the held button, whose background is `--accent-soft`,
and it named `--accent` — the accent for a SOLID BUTTON — which measures **2.53** there in dark. The token whose own
comment names this exact job is `--accent-ink`: "the accent for CHROME — icons, DOTS, borders — and it is too light to
read as small text: measured against its own soft background it gives 3.83 in light and 3.23 in dark". Both clear the
graphic bar. The vocabulary was already written down; the dot was using the wrong half of it.

AND IT HAD NO TEST, for the same reason the sweep had never seen it: this mark does not route through `--mark-ink`, so
the panel's contrast contract — which walks the SOURCES of that channel — could not see it either. The contract has a
case now: the held dot's ink against `--accent-soft` in both themes.

THE MUTATION LESSON, AGAIN, AND THIS TIME IT COST THREE CALLS: my first attempt to prove that case bites replaced the
first occurrence of the rule's text in the file, which was a DIFFERENT rule — the sheet kept `--accent-ink`, the build
was green, and the test passed for the right reason while I read it as a hole in the test. Aimed at the actual line, it
fails. A mutation that does not bite is evidence about the MUTATION first; the ledger has said so since round 44 and it
has now been right five times.

### THE UNSET GOAL'S FIRST PHOTOGRAPH: CLEAN, AND THAT IS THE RESULT (round 90)

Third round running on the same method — find a state the vocabulary names, the sheet draws, and no fixture renders —
and the third state found by it. GoalBar's own comment states both states and which one is normal:

    "nothing stated -> a QUIET affordance. An unset goal is normal (most sessions ...)"

The fixture gave EVERY session a goal, so the affordance had never been on screen while the state it replaces has been
measured on every page since the harness was written. They are different ELEMENTS, not two colours of one: the unset
button's only content is a bare text node, with no `.goal-text` inside it — and the harness's own REQUIRED list names
`.goal-text` as a governance element it expects to find.

    ?goal=none, both densities, both themes: NO FINDINGS.

WHICH IS THE USEFUL ANSWER, and worth stating as plainly as a defect would be. The two previous rounds' surfaces each
found a real defect within two runs (the tombstone's mark collided with idle because the probe could not see borders;
the held dot measured 2.53 in dark because it used the solid-button token). This one found nothing: the dashed border,
the dim ink and the target size were already right, and the sheet's choices survive being rendered.

The difference between those outcomes is not the method's value — it is that "the sheet looks right" became "the screen
is right" for the state MOST sessions are in. Three rounds, three states that no gate had ever seen, two defects and
one clean bill.

THE RUNNING LIST OF STATES THIS METHOD HAS ADDED, for the next round to extend rather than rediscover:
    off / the tombstone          round 88  (found: a mark collision, then three FILL-inside-a-RING defects)
    human / the held session     round 89  (found: 2.53 on the graphic bar in dark)
    unset / the goal affordance  round 90  (clean)

### A PRUNE THAT EXPIRED, AND THE CLAUSE THAT MADE RE-ADDING IT SAFE (round 91)

Round 153 pruned the panel density's empty-state surface with a reason worth keeping:

    Round 152 added this for the PANEL density and measured it "clean". Round 153 looked at what that surface actually
    rendered and found it was not the empty state at all: the rail said "Sessions unavailable — reconnecting…",
    because in the panel harness the rail receives connected=false, while the DESKTOP harness renders the real thing.
    The contrast was clean in both cases, which is exactly why the label mattered — A SURFACE THAT MEASURES THE WRONG
    STATE PASSES FOR THE BEST REASON.

That reason has expired, and rounds 86-87 are what expired it: this harness DOES open the SSE stream, the panel renders
connected, and the judge now fails any surface that regresses — by the harness's own `{opened, fail}` flag AND by the
rendered sentence. So the block is back, and the clause that proved the harness connected is also what makes re-adding
it safe rather than hopeful: if the panel renders the wrong state there again, the surface fails instead of passing.

    MEASURED, CI green: the panel sweep now reports 104 surfaces, up from 98 — the held, no-goal and empty states, two
    themes each — and every one of them is clean, with the empty state's harness flag travelling with its reading.

THE RUNNING LIST, now four states and one re-added surface:
    off / the tombstone          round 88  found: a mark collision, then three FILL-inside-a-RING defects
    human / the held session     round 89  found: 2.53 on the graphic bar in dark
    unset / the goal affordance  round 90  clean
    empty / the panel density    round 91  re-added after its prune expired; clean, and the wrong-state failure it
                                          was pruned for is now a clause rather than a caveat

### THE NEW-SESSION MENU, AND A LIMIT THAT HAD NEVER BEEN NAMED WHERE IT BITES (round 92)

Fifth state added by the same method. The menu lives in DesktopShell — a `.btn-new` button with aria-expanded and a
popover of role=menuitem buttons, each with an `.nm-ico` span carrying data-kind — and the sweep has PRESSED that
button for many rounds (it is in the press targets) without ever photographing what it opens.

WHAT IT FOUND, AND WHY IT IS NOT A DEFECT:

    span.nm-ico 1.10 light / 1.05 dark — a 22px chip whose background is a subtle surface behind a coloured glyph

A chip's background delimits; it does not inform. So it is waived — with a reason that does NOT claim more than it
knows, and that is the part worth keeping:

    "the icon chip's background delimits a coloured glyph at 1.05/1.10; the GLYPH ITSELF IS NOT MEASURED — the probe
     excludes SVG by design, so the lane colour it carries has no row in this suite"

THE LIMIT IS REAL AND NOW NAMED WHERE IT BITES. The graphic loop skips `el instanceof SVGElement || el.closest('svg')`
for a good reason ("an icon's path inherits fill: black and its real colour comes from the svg above it, so every
decorative glyph reported cr ~1"), and the consequence had never been written next to a finding it produced: the
per-kind colour that carries THIS menu's meaning — --lane-ds for ssh, --lane-or for serial, the lane vocabulary the
rest of the panel uses — produces no row anywhere. A waiver that silences a decorative background is honest only if it
says the signal was not judged, and this one does.

AND PROVING THE WAIVER NARROW ENOUGH TURNED UP A FALSE CLAIM BESIDE IT. The rail-dot entry said "only 2.33 is set
aside, so a DIFFERENT ratio on the same element — in rows or on hover — is still a finding". The consumer is
`DECORATIVE.find((d) => d.match.test(String(r.sel)))`: the pattern is tested against THE SELECTOR ALONE, so that entry
waives EVERY ratio on div.rail-dot in the rows path. The sentence is true of the hover path (which reports through
`ignore` with the value in its pattern) and false of this one. Corrected, with the reason a selector-wide waiver is
survivable there — the dot's FILL, the channel that carries the state, is held at 3.00:1 by the panel gate — and the
value-anchoring work recorded as its own.

FOURTH ROUND RUNNING WHERE THE DEFECT WAS A SENTENCE ABOUT THE INSTRUMENT. The list is now: the SSE note that said the
stream never opens (86), the paragraph that said the attempt was reverted (87), the entry-stamp root (77), and this
value claim. Every one was found by using the thing the sentence described.

RUNNING LIST OF STATES THIS METHOD HAS ADDED:
    off / the tombstone          round 88  found: a mark collision, then three FILL-inside-a-RING defects
    human / the held session     round 89  found: 2.53 on the graphic bar in dark
    unset / the goal affordance  round 90  clean
    empty / the panel density    round 91  re-added after its prune expired; clean
    menu / the new-session popover round 92 clean, and a limit named (icons are not measured at all)

### THE 46TH BACKTICK REACHED A COMMIT, AND IT TOOK FIVE CI JOBS WITH IT (round 93)

Every previous one of these was caught by an emitter's own guard before the commit. This one was not, and the
difference is worth writing down precisely, because the guard exists and works:

    cd7aa961  fix(probe): an SVG element's className is not a string — ten findings named `svg ""`

The commit explained its fix with an example inside the probe's template literal — `` `svg ""` `` — so the module
stopped PARSING. Not the emitted script: the module itself, at import:

    SyntaxError: Unexpected identifier 'svg'   (contrast-probe.mjs:206)

Five of the ten CI jobs on the pushed commit went red — ui, panel (vitest), gateway, design and pack-chain — because
each of them imports the probe, directly or through the shared judge. `RolldownError: Parse failure` in panel-react's
pairContrast/themeContrast tests; `not ok 9 - test/gradient-text-contrast.test.mjs` in the UI job. The two Rust jobs
stayed green, which is the shape that reads as "only the front end noticed".

THE GUARD WAS ALREADY THERE. `scripts/hooks/pre-commit` refuses exactly this commit (exit 1, all three checks), and
`contrast-probe-check.mjs` — wired into CI at ci.yml:437 — fails it too. Proven on HEAD's own blob rather than on a
paraphrase of it: stash the fix, run either one, watch it fail; restore, watch 22 checks pass.

WHAT WAS MISSING IS THAT NOTHING RUNS THE HOOK. `core.hooksPath` is global (`~/.config/git/hooks`, which holds the
operator's `post-commit` and a free `pre-commit` slot), so `.git/hooks/` is ignored entirely and the only thing that
invokes the guard is the loop remembering to. The loop's inbox has carried that symlink request since round 226 as a
convenience — "the hook works, and the only thing standing between it and every commit is that a human-shaped loop has
to remember to run it". This commit is that sentence's first receipt: the hook was not run, and the mistake landed.

THE ROUND'S OWN FIX THEN REPEATED IT, which is the part that makes this a ledger entry rather than an anecdote: the
comment written for the SVG rule contained an `<svg>` and an `svgRootPaints` in backticks, and the hook — run by hand
this time, as the FIRST thing after the edit — refused it in under a second. 47th time, caught, because the guard was
invoked. The lesson is not "be careful with backticks"; it is that a guard nobody runs is a guard that is not there,
and the fix for that is one symlink the loop cannot make for itself.

### AN SVG ROOT'S FILL IS NOT ALWAYS A COLOUR (round 94)

Round 93 let the svg ROOT through the graphic loop on a true observation — the root is the element that knows an
icon's colour (fill: none, stroke: currentColor) — and then read the root's OWN computed fill and stroke. An `<svg>`
root draws nothing: fill and stroke are INHERITED properties, so a root whose shapes each declare their own paint
contributes the initial value, rgb(0,0,0), and CI filed ten rows for it:

    1.18 panel/Memory [dark] svg "" — painted rgb(0, 0, 0) (fill) on rgb(23, 24, 29), 22px graphic, needs 3
    1.19 panel/Browser [dark] svg "" — painted rgb(0, 0, 0) (fill) on rgb(23, 24, 29), 20px graphic, needs 3

The 20px one is the rail's BrandMark (shapes: url(#vale-sky), #fff8e1, #ffffff) and the 22px one the vitals dial
(circles painting --chrome-line and the tone's state colour). Nothing paints black in either.

MEASURED FIRST, ON THE DEVICE, on both idioms, with the bundled Playwright — the two lines that decide the rule:

    Icon   root fill=none stroke=rgb(162,163,172)  -> the polyline and line compute EXACTLY that
    brand  root fill=rgb(0,0,0)                    -> its rect/circle/path compute url(#sky), #fff8e1, #ffffff

So a root's value counts only when a shape below it computes the same paint. `svgRootPaints` is a PURE function —
which is the only reason the rule is testable at all, since the DOM loop around it is not — and it is embedded in the
probe the same way the other helpers are, so the browser and the test run one implementation. Mutation: delete the
`painted.has(...)` guard and the check fails with the brand mark's black; restored, 23 checks pass. The emitted
artifact was checked too, not only the module.

AND THE NOISE WAS NOT FREE. It failed the design job, and it pushed a REAL defect off the end of the report: the
judge prints ten findings plus coverage, and the ten were all this. Which is the case for treating a false finding as
a defect in the instrument — it does not merely annoy, it occupies the space where the true one would have been.

What the rule still does not measure, unchanged and stated where it bites: a shape that declares its OWN paint (the
sparkline's paths, the brand mark's discs) produces no row, because the root's value is not its colour.

### THE ACTIVE TAB'S OWN CONTROLS WERE 1.99:1 (round 94)

The row the false findings were hiding, from the same CI run, in the hover pass:

    svg "" 1.99<3 painted rgb(162,163,172) (stroke) on rgb(198,67,16), 12px graphic

rgb(198,67,16) is `rgba(217,72,15,0.9)` over `--chrome-bg` #17181d — an ACTIVE TAB. `.tab-export` and `.tab-close`
rest at `--chrome-ink-dim`, a CHROME ink, and the active tab is not chrome. It is also the one tab that shows both
controls WITHOUT a hover (`visibility: visible`), so the defect is on screen whenever the tab is.

Every ink that rule reaches fails there in one theme or the other — `--chrome-ink-dim` 1.99 dark, `--chrome-active-ink`
1.10 light (it is #bf3a0a there, and it measures 4.12 on the CHROME, which is where it was chosen), `--danger-on-soft`
2.16 dark — so it is one rule, not three point fixes.

THE TWIN RULE ALREADY DID IT RIGHT, which is how the fix was chosen rather than invented: the desktop density's
`button.dtab-close` inherits its tab's colour and `.dtab.active` sets `--chrome-active-text`. Both panel controls take
that token now — 4.99 dark, 4.90 light — measured as rendered on the device:

    dark   .tab.active .tab-export svg  stroke rgb(255,255,255) on rgba(217,72,15,0.9)   (was rgb(162,163,172))
    light  .tab.active .tab-export svg  stroke rgb(156,58,10)   on rgb(255,239,229)

The ✕ keeps its SHAPE, the mark language's first channel, and the danger hue returns the moment the tab is not the
active one. The test reads the RULE out of the BUILT sheet — selector, declared colour, and the surface composited the
way the probe composites it — so repointing it at another surface, renaming its ink, or deleting it fails. Mutation
verified: put `--chrome-ink-dim` back, rebuild, and it fails with CI's own number, "1.99 on the active tab's fill".

THE WHOLE PANEL SWEEP, RE-RUN ON THE DEVICE against the same harness: 106 surfaces, 5038 rows, 24 findings — every one
of them `span.approval-grant` or `span.nm-ico`, both already waived with reasons — and ZERO under AA in the hover pass
of all four density/theme combinations. The ten false rows are gone and no new SVG row took their place: the icons'
inherited strokes all clear 3:1, which is the thing round 93 was trying to find out.

### THE PRESS PASS WAS MEASURING HOVERS (round 95)

`pressPass` is the only instrument that can see whether a press reaches the screen. `feedback-check.mjs` proves an
`:active` RULE exists; the judge fails a row whose before and during snapshots are identical. It took its "before"
snapshot with the pointer PARKED AWAY from the control, so a sheet that answered `:hover` and had no press rule at all
still produced a difference — the hover made it — and the row read `changed: true`.

FOUND BY ASKING THE LANDING THE QUESTION DIRECTLY, because the landing is where it had to be asked: `.theme-toggle`
has a `:hover` rule and NO `:active` rule. Measured on the device, both schemes:

    light .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING
    dark  .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING

while `.btn-primary` (transform) and every link (opacity) DO add something beyond their hover on the same page. So the
toggle was the one input on that surface that answered a hover and ignored a press, and BOTH gates missed it: the
rendered pass for the reason above, and the sheet check because the landing's stylesheet is INLINE IN
`index/src/page.js` — a third kind of file that `feedback-check.mjs` had never opened.

THE FIX IN THE INSTRUMENT IS AN ORDER AND A BASELINE: hover, let the transition settle (260ms; the old pass waited
80ms), read the baseline, press. The verdict is `pressDelta(hovered, pressed)` — pure, so it has a test, the same
split `svgRootPaints` uses. It refuses layout properties, because a press that moves `width` re-lays-out the page every
frame and counting it would let exactly the press `feedback-check.mjs` bans pass here.

AND THE FIRST VERSION OF THE ASSERTION DID NOT BITE. It checked "hovered is read before mouse.down()" — which a probe
that reads the baseline BEFORE the hover also satisfies, so the mutation restoring the resting anchor PASSED. That is
the fifth time this ledger has recorded "a mutation that does not bite is evidence about the MUTATION first", and the
first time the fix was to make the assertion STRONGER rather than to re-aim it: it now requires move < read < press,
and fails with "the baseline is read BEFORE the hover (move@54245, hovered@54205) — that is the resting anchor this
check exists for".

RENDERED AFTER, ALL FOUR SURFACES, with the stricter anchor — nothing was hiding behind the old one except the toggle:

    panel    4 surfaces   12 controls   DEAD 0   .rail-btn 38x38 · .tab 96x34 · .side-row 232x34 · .side-add 30x21
    desktop  (same run)                        .desktop-rail-btn 40x40 · .dtab 90x36            all [transform]
    console  6 surfaces   21 controls   DEAD 0   .rail-btn [transform,opacity] · .btn · .card-link [opacity] ·
                                               .dev-mini · .btn-dashed 1052x42 · .rail-avatar  [transform]
    landing  2 schemes     3 controls   the toggle now reports [transform]; .btn-primary and a unchanged

THE THIRD SHEET IS NOW READ, and that is what makes the gap un-reopenable: `crop` takes the `<style>` block out of the
module, a missing block is FATAL rather than a silent zero, and `.step` — a plain div with no handler, whose links are
the real controls — is listed as not-pressable with that reason. The markup walk also reads `class="…"` in `.js` now,
not only `className="…"` in `.tsx`, because the landing builds HTML strings and every landing variant was invisible to
the variant exemption.

Mutation: delete the landing's `:active` rule and the check fails with "landing: no :active for .theme-toggle:hover".

### A WAIVER IS FOR THE RATIO, NOT THE ELEMENT (round 95)

Round 92 found this entry claiming something false about itself:

    "only 2.33 is set aside, so a DIFFERENT ratio on the same element — in rows or on hover — is still a finding"

The consumer tested the pattern against THE SELECTOR ALONE, so `div.rail-dot` set aside every ratio that element could
ever produce. The sentence was true of the hover path (whose patterns carry the value) and false of the rows path, and
the ledger recorded the value-anchoring work as its own. It is done now: each entry carries the band it was measured
in, and a row outside every band is a finding that names the band that refused it.

THE BANDS ARE MEASUREMENTS, taken from the device report this suite produced the same round:

    div.rail-dot        2.25-2.45   the working dot's halo (2.33 on the rail)
    span.approval-grant 1.10-1.35   FOUR surfaces, four ratios — 1.19, 1.20, 1.25, 1.27 — the outline composites
                                    over a different surface on each
    span.nm-ico         1.00-1.15   1.05 light / 1.10 dark, the chip's own background

PINNED IN BOTH DIRECTIONS, which is the part that keeps a judge honest (50 → 52 checks in
`panel-design-sweep.bash`):

  * a new axis, `decorative-drift`, plants the rail dot at 1.50 and requires a FAILURE. Mutation — make the consumer
    match the selector alone again — reports "the judge PASSED a report with a planted 'decorative-drift' defect".
  * and a positive case the axis loop cannot make: the same element at 2.33 must still pass AND the waiver must still
    be printed with its reason. A judge that fails everything is as useless as one that passes everything, and a
    suppression nobody can see is one nobody can review. Both planted rows carry `kind: "graphic"`; without it the
    type floor rejects them first and the case proves nothing about the waiver — a lesson the first version of the
    positive case paid for.

AND ONE OF THE REASONS IN THAT LIST HAD GONE STALE, found by reading it while banding it: the `nm-ico` entry still
said "the GLYPH ITSELF IS NOT MEASURED — the probe excludes SVG by design". Rounds 93-94 changed exactly that, so the
lane colour it named as unmeasurable HAS a row now and clears 3:1 everywhere the sweep renders. The entry says what it
silences (the background) and what is judged elsewhere. That is the sixth round in a row where a sentence about the
instrument, not the instrument, was the defect.

VERIFIED AGAINST THE REAL REPORT: the 5,038-row device report still judges clean with the bands in place — every
waived row lands inside its band — and its only finding is the expected INCOMPLETE REPORT clause for a run that
measured `pages,hover`.

### THE DEVICE REPORTS THE OUTCOME, AND THE ROW WEARS IT (round 96)

The gap was named by `liveness.ts` in its own comment, which is the objective's first clause written as a limitation:

    THE VOCABULARY IS ONLY WHAT THE DEVICE CAN ACTUALLY REPORT ... "Failed" is not here because no field
    reports it per session — inventing a state would put a shape on the screen that nothing can ever mean.

The panel CAN read a failure out of the audit trail (`cardState` maps an exit code to ok/fail) but only for the
session whose trail it has loaded, so the rail and every other tab could not say it — exactly the blindness
`command_running` had before round 28, and the same answer: the device has known all along (`marker_code` at the end
of the execute wait loop), so it says it on the list every client already polls.

`TermSessionInfo.last_exit_code: Option<i32>`, omitted when absent, pinned from both ends by the shared fixture (the
Rust serializer against it, the panel's mapper through it). THREE STATES, and the third is the design decision:

    Some(0)   succeeded
    Some(n)   failed, and the row says WHICH code
    ABSENT    NO CODE WAS OBSERVED — no command yet, a wait that ended in a timeout or a partial read, or an
              ssh/serial session, which has no marker injection at all

It is CLEARED when a new command is written (at the point the command really reached the shell — the write-failure
path returns before it) and SET only from the shell's own marker, the same value the audit line and the background
job registry get. So a stale code can never be read as the new command's outcome, and a command whose fate is
unknown reports nothing rather than guessing. A mark that lies is worse than a mark that is absent.

THE PANEL WEARS IT AS A CHIP, NOT AS A MARK, AND THAT IS THE ROUND'S OTHER DECISION. One shape per LIVENESS state is
the mark language's rule; a failure is not liveness — a session can be idle and have failed. A fifth silhouette would
also collide with a question the operator has not answered yet (the console spells failure as a diamond and the panel
spells WAITING as one — inbox item 17), so the shape decision stays open and the row says `exit 1` in the danger pair
the sheet already uses for danger text. No new tokens, no new vocabulary, nothing pre-empted.

MEASURED AS RENDERED, through a surface built for it: the harness grew `?exitfail=1` (and `?exitok=1` for the audit),
the sweep grew a `LastFail-{light,dark}` surface, and the chip reports

    light  span.side-exit  5.96  rgb(165, 29, 29) on rgb(252, 223, 214)   (AA needs 4.5)
    dark   span.side-exit  5.65  rgb(255, 135, 135) on rgb(74, 40, 24)

— 108 surfaces and 5,136 rows, up from 106 and 5,038, judged clean (the only finding is the expected INCOMPLETE
REPORT clause for a `pages`-only run). The states that render IDENTICALLY are pinned by tests rather than pixels:
exit 0 and absent both draw nothing, and that difference lives in `ContextRail.test.tsx`.

THE SIXTH STATE THIS METHOD HAS ADDED, and the first one that needed the DEVICE to change first:

    off / the tombstone           round 88  found: a mark collision, then three FILL-inside-a-RING defects
    human / the held session      round 89  found: 2.53 on the graphic bar in dark
    unset / the goal affordance   round 90  clean
    empty / the panel density     round 91  re-added after its prune expired; clean
    menu / the new-session popover round 92 clean, and a limit named (icons are not measured)
    exitfail / the failed last command round 96  NEEDED A WIRE FIELD: no surface could render it, so none could
                                                 measure it; now 5.96/5.65 rendered

### THE e2e ASSERTION THAT WAS ABOUT THE RUNNER'S SHELL (round 96, continued)

The wire field needed a drive through the real binary, and the first version of that check demanded a NUMBER. Running
it locally (agent on loopback, `--only governance,terminal`) is what stopped it reaching CI, because the answer on
this box is:

    gov: the approved command ran  -- state=partial
    terminal session execute       -- state=partial exit=null

No shell marker fires on a bash PTY, so the caller is told NOTHING and the row carries NOTHING — which is the correct
answer (`None` means "no code was observed"), not a missing feature. A check that required an exit code would have
been a check about the runner's shell.

SO THE ASSERTION IS AGREEMENT, IN BOTH DIRECTIONS, and it is a helper both sections share:

    the caller was told a code  ->  the row carries the SAME code
    the caller was told nothing ->  the row carries NOTHING

That is the invariant that can break — one audience reporting a code the other never saw — and it holds on a runner
with markers and without. The number itself is pinned where it can be: the device's unit test sets, replaces and
clears it, and asserts the OMISSION, which is the difference between "nothing to say" and "exit 0".

AND IT IS IN TWO SECTIONS ON PURPOSE: CI runs `--only governance,runs`, so a check written only in the terminal
section is a check CI never invokes — the "test nobody runs" shape this repository has paid for before.

WHAT IS NOT CLAIMED, because it would be easy to read the above as more than it is: no marker code has been observed
on a real session BY THIS LOOP. This box's shells never produce one, so the exec.rs call site is verified by reading
it (the value it records is the same `marker_code` the audit line and the job registry already get) and by the
unit test on the setter, not by a live end-to-end run.

### THE FIFTH SILHOUETTE, AND THE STRAY CLOSE THAT COST A REGION OF THE SHEET (round 97)

`liveness.ts` had said for rounds why this state did not exist — "no field reports it per session" — and round 96
made the device report it. The row wore a text chip while the shape decision waited on the operator; this round gave
it the shape, because "every state has its own silhouette" is the objective's own clause and a chip is not one.

A TRIANGLE, and it is the only shape neither surface has spent: the panel spends the diamond on WAITING, the console
spends it on FAILURE, and the console's rest are a solid fill (ok), a square (warn), a halo (running) and a ring
(absent). That matters because `waiting` and `failed` sit side by side in the same tab strip. `clip-path` rather than
a border trick — the probe resolves a BORDER as a ring, and this mark is a FILL whose outline is cut — with
`border-radius: 0` so the signature's radius channel does not contradict the shape it describes.

THE PRECEDENCE IS THE DESIGN: off > waiting > working > failed > idle. A failure describes what ALREADY HAPPENED, so
anything happening now outranks it; it outranks idle because "quiet, and the last thing here broke" is more than
"quiet"; and it LINGERS by design, because the device clears the code when the next command is written — the state
ends when the session does something else, not on a timer the panel invents. `deviceLiveness` deliberately does not
pass it: a device-wide failure would have to pick which session to blame and say nothing about which, and an AI's
ordinary `grep` exiting 1 would leave the rail shouting about a machine that is fine.

AND THE INK WAS CHOSEN BY MEASURING, which is the one number worth keeping:

    --state-fail on the dark ACTIVE session row (#3d2817)   2.87:1   FAILS the 3:1 a graphic needs
    --danger-on-soft, worst over the four mark surfaces      6.71 light / 5.99 dark

The first version used `--state-fail` and the failure was found the way these are always found — by measuring the
surface the mark actually sits on, which is the dark active row, the same place the waiting diamond already lives.

THE PROBE HAD TO LEARN THE SHAPE CHANNEL. The signature was radius / rotation / KIND, so a clipped fill and a plain
fill computed identically and the collision check would have called two different marks equal. It is radius /
rotation / CLIP / kind now: every channel a mark can be drawn with is in the data two states are told apart by.

RENDERED, on a surface built to hold four of the five states at once (`?exitfail=1` on the quiet ssh seed, under
`mode=pending`, so waiting + working + failed + idle share one page — the only place a collapsed failed/idle or
failed/waiting can be caught at all):

    LastFail-light  families=["mark[waiting,working,failed,idle]", ...]  collisions=[]  ringFill=[]
    LastFail-dark   families=["mark[waiting,working,failed,idle]", ...]  collisions=[]  ringFill=[]
    light  .side-dot[failed]  8x8  bg rgb(165,29,29)   radius 0  clip-path polygon(50% 0, 100% 100%, 0 100%)
    dark   .side-dot[failed]  8x8  bg rgb(255,135,135) radius 0  clip-path …

AND THEN THE SHEET BROKE IN A WAY NO GATE COULD SEE. The comment written for the new mark closed early — one `*/`
too many — so the paragraph after it became raw CSS. The symptom was a broken PAGE, not a broken rule: every mark
lost its size (0x0), the session rows lost `display: flex` and their padding, and the rendered sweep reported
`families=[]` — no marks at all — while every gate stayed green and `stylesheet-hygiene` reported "comments are
balanced and none of them reads as code". It was right about what it checked and blind to the shape that mattered:
the walk finds `/*` and takes the NEXT `*/`, so a stray close is skipped as ordinary text, while THE BROWSER reads
it as a parse error and discards rules until the next `}`.

The gate checks the gaps between the comment spans now, with a line number, and the mutation is planting one. The
deeper lesson is the one this ledger keeps relearning from the other side: a sheet-level gate and a rendered
measurement disagreeing is not noise — the rendered one was right, and the sheet-level one had a hole exactly the
shape of the mistake I had just made.

### THE SURFACE THE FLAG NEVER RENDERED (round 98)

Round 97 chose the failed mark's ink by measuring the four surfaces a mark sits on, and stated the number with
confidence. It had missed one: the **accent-filled active tab**, found by forcing the state onto the active tab in the
browser rather than waiting for a fixture to produce it:

    panel/dark  .tab.active .tab-dot[failed]   rgb(255,135,135) on rgb(198,67,16)   2.16:1   (needs 3.0)

Light was 6.71 and both active SIDE rows 5.99 — only the tab paints the accent FILL, which is why a table of "the
surfaces a mark sits on" written from the row-like ones missed it.

THE FIX NEEDED A THIRD VALUE, and that is the point of the entry. `--state-fail` was 2.87 on the dark active row;
`--danger-on-soft` fixed that and is 2.16 here; the value that clears everything is Open Color red-3 (#ffc9c9) in
dark, worst 3.44 on the fill and 9.5-11.2 elsewhere. So the failure mark owns its ink — `--state-fail-ink` — and it is
DECOUPLED from `--danger-on-soft` even though the light values are identical, because that is exactly the case where
two tokens with one job in one theme need different values in the other. Round 237 learned this for the accent family.

AND THE COMBINATION HAS A SURFACE NOW, or the fix would be one edit away from regressing in the dark: the rule this
repository keeps relearning is that a state no fixture renders is a state no sweep measures. `?exitfail=active` puts
the failure on the ACTIVE session — and it must ALSO quiet it, because every seed reports `idle_ms: 900`, which reads
as WORKING, which outranks a failure by design. The first run of the flag rendered `mark[working,idle]` and no failed
mark at all: the state model was right and the fixture was lying about what it expressed. 60 seconds of silence fixed
it, and `harness-fixture-check` pins both halves.

    rendered after: dark .tab-dot[failed] 3.44 on rgb(198,67,16), light 6.71; families mark[working,failed,idle],
                    collisions=[] in both themes; 110 surfaces, report clean.

The shipped test measures the token against every surface a mark lands on — including the COMPOSITED active fill that
no single token names — and fails with the device's own number when the old value goes back.

### THE PANEL BLAMED THE DEVICE FOR A QUESTION IT NEVER ASKED (round 99)

The unrendered-state method — enumerate states the sheet draws, find one no fixture produces — turned up something
different this time: a state that WAS rendered, on eight surfaces, saying something false.

    Settings   "The device did not answer, so its restart history could not be read."

`useBootHistory` reads `/api/boots` and requires `ok === true`; anything else is a FAILED read. The device sends the
envelope, the panel's hook test sends it, and THE HARNESS DID NOT — so the Restarts card rendered its failure branch
on every Settings surface the suite has ever photographed, judged clean every time, while its real content (the
24-hour summary, the crash rows, the "first start" row, the uptime facts) was measured by NOTHING. It is the lesson
the monitors stub three lines below already records from round 100, one endpoint over and one step worse: not an
empty card, but a card blaming the device.

FOUND BY READING THE DOM RATHER THAN THE REPORT, which is the part worth keeping. `window.__calls` showed no
`/api/boots` request had ever been counted, and the card's section held only its `<h2>` and that sentence. The report
had shown an `<h2>Restarts</h2>` with no rows after it and nothing looked wrong, because THE INSTRUMENT HAD HIDDEN
THE ROW: the text-row dedupe keys on class + the first sixteen characters, and the vitals card says "The device did
not answer, so its vitals could not be read" — same class, same sixteen characters, so the restart card's sentence
was dropped. A probe that collapses two paragraphs into one cannot report what the second one said.

Both ends fixed, and the first photograph of a restarts card this suite has ever taken came out clean:

    p.restart-summary "3 restarts in the last 24h — one of them a crash"   16.27 of 4.5
    span.restart-row-kind.is-crash "previous run crashed"                  7.27  rgb(165,29,29)
    span.restart-row-kind "replaced by a restart" / "first start"          16.27
    span.restart-row-fact "ran 1h 30m" / "ran 0s"                          7.47
    li.restart-row (the crash row's edge, a graphic)                       4.85 of 3
    "did not answer" rows across all 110 surfaces                                     0

AND THE DEDUPE KEY IS HONEST ABOUT WHAT IT IS: class + sixteen characters + TOTAL LENGTH. It can still collide for
two distinct strings of equal length sharing a prefix, and that is stated rather than claimed away; what it no longer
does is collide on the pair that mattered. The same run reports **5,508 rows where it reported 5,250** — every one of
them a distinct sentence the instrument had been dropping.

The gate: `harness-fixture-check` asserts the stub carries `ok: true`, with the mutation that strips it, because the
failure mode of THIS fixture is not an empty card — it is the panel asserting something false about a device that
answered.

### TWO ROUNDS OF FINDING THE SAME DEFECT BY HAND, AND THE CLAUSE THAT ENDS THAT (round 100)

Rounds 99 and 100 both found the same shape by hand: the panel saying **"the device did not answer"** about a device
that answered, because a harness stub omitted the `ok` envelope its reader requires — the restart history, then the
monitors. Two rounds, two endpoints, no gate, because a sentence is not a contrast ratio and every instrument in this
suite was reading numbers.

**THE ENVELOPE IS STRUCTURAL NOW.** `J()` — the harness's only JSON responder — merges `ok: true` into every object
body that does not bring its own. The rule had been written in that file since round 110 ("EVERY FIXTURE BELOW NEEDS
ok:true … the omission has cost three rounds") and the fourth still happened, because a note is not a mechanism. A
reader that requires `ok` gets it; a reader that ignores it is unaffected; a stub that MEANS to express a failure
passes `ok: false` and is left alone.

**AND THE CLAIM ITSELF IS JUDGED.** The surface probe collects read-failure claims — "could not be read", "did not
answer, so", "unavailable — reconnecting" — and the judge fails any surface that makes one WHILE THE FIXTURE ANSWERED
EVERY CALL. The excuse is the fixture's own answer, not a list in the judge: `report.sse.fail` marks the `?fail=1`
pages, where the claim is true, and those are printed with their reason.

IT FIRED ON ITS FIRST RUN, ON 22 SURFACES:

    "The device did not answer, so its logs could not be read."

The DeviceLogsCard, on every Settings page since it existed. `/api/logs` was stubbed by NOTHING: the device serves it
(`api_logs`), the card reads it, and the harness had never answered it — so the card drew its failure branch on every
sweep ever run, and its real content (the update VERDICT from `updateDiagnosis`'s four-way table, the receipt, the
directory, one row per file with an ABSENT file named as absent) had been measured by nothing. THE FOURTH CARD IN THIS
FAMILY: update, monitors, restarts, logs.

RENDERING IT FOUND A REAL DEFECT, which is the whole argument for the exercise:

    p.device-logs-verdict-ok   3.33:1   rgb(47,158,68) on rgb(252,251,250)   13px text, AA wants 4.5

`--state-ok` is a MARK colour, and `tokens.css` already names the readable weight for text — "--success-text: Status
ink for SMALL TEXT … --success is tuned for marks (dots, borders, bars); as 11px text they measure 4.27/3.45 in light
and 3.63/5.34 in dark". The card used the wrong half of the vocabulary. After the fix:

    ok    5.22 light / 9.89 dark        warn   7.27 light / 7.46 dark        need 4.5

AND THE SECOND TONE HAS A SURFACE, because one payload can only render one verdict: `?logs=warn` carries a receipt
with NO start line (the CLI reached the device and the swap never launched — the state an operator investigating a
stalled update actually sees), and the sweep grew `LogsWarn-{light,dark}`.

    112 surfaces, 5,916 rows, judged clean. Read-failure claims on non-failing surfaces: ZERO.

A NOTE ON THE INSTRUMENT, since the claim clause is new and a clause that cannot see a defect is worse than none:
the axis is planted in `panel-design-sweep.bash` as `false-claim`, and the OTHER direction is planted beside it — the
same sentence on a `?fail=1` surface must PASS and must print its reason, because a judge that fails everything is as
useless as one that fails nothing.

### THE TWO VIEWS NOBODY HAD PHOTOGRAPHED, AND THE FIXTURE BUG THAT HID THEM (round 101)

The per-session view switch has three tabs — Terminal, Trajectory (the raw audit timeline) and Path (the same work as
steps, with a summary) — and every surface in this suite left it on Terminal. The harness has served the events all
along: a goal, an approval armed/approved/granted, two commands with exit 0 and exit 1, their output. Nothing ever
clicked the tab. So the panel's two most information-dense views, and every style in them — the round dots and their
states, the exit badges, the governance chips, the plan rows, the step tags, the attention rows — had been measured by
nothing at all.

ADDING THE CLICK WAS NOT ENOUGH, and the reason is the fourth instance of one defect class:

    if (u.indexOf('/api/sessions') >= 0) { ... }        the ARCHIVE stub

also matches `/api/sessions/<sid>`, so the PER-SESSION stub below it — the one that serves the audit trail — was
UNREACHABLE. Every request for a session's events got the archive's body, and with the envelope now in place the
reader treated that as a SUCCESSFUL EMPTY READ: both views drew "No commands in this session yet" and "No path yet",
and the first photographs were of an empty page that looked correct. A BROAD MATCH SHADOWING A SPECIFIC ONE joins the
family this ledger has been collecting: a fixture answering a question nobody asked. It is matched as a PATH now
(no regex, no backslashes — the diag helper in that same file records why), and `harness-fixture-check` pins it with
the mutation that widens it back.

THE FIRST REAL PHOTOGRAPH, ON BOTH DENSITIES AND BOTH THEMES (120 surfaces, 6,412 rows), and the judge failed it at
once:

    2.56  panel/Trajectory-light              span.cmd-dot  rgb(161,161,170) (ring) on white, 8px graphic, needs 3
    2.56  desktop/Desktop-Trajectory-light    the same mark, the same number

THE MUTED RING WAS UNDER THE GRAPHIC BAR IN LIGHT, and nothing could see it from either side: `statePalette.test.ts`
asserts those dots agree on COLOUR across components, and the contrast contract walks `--mark-ink` — which these rings
do not use. `--state-muted` is the INK OF A RING (three rules draw `inset 0 0 0 1.5px var(--state-muted)`) and it was
`var(--faint)`, a token whose own doc calls it a mark colour. Fixed to `var(--muted)` — 4.83 on that surface, 6.71 in
dark — and restated in the dark block so each theme uses its own grey rather than the light one computed at :root.

    after:  Trajectory-light .cmd-dot (ring) 4.83 | Trajectory-dark 6.71 | need 3
            every other dot on the new surfaces: ok 3.42-4.88, fail 3.48-4.83, all passing

The two views also render their real content now: three rounds with durations, `exit 1` badges in the failure ink, the
governance chips, the plan steps with their tags and reasons, and the "why was this interrupted" notes — so the next
round has something to measure that no round before it could.

A NOTE FOR THE NEXT READER: the History page's OTHER half is still unphotographed. `HistoryPage` has two scopes —
Sessions (the archive, which the sweep does render, EMPTY) and Runs (the activity view, behind a click) — and
`/api/sessions` answers an empty list, so the archive's list rows and the trail inside an archived session have never
been drawn either.

### THE RECORD PAGE'S THREE UNMEASURED STATES, AND THE FLAG NOBODY PASSED (round 102)

The History page has two scopes and the archive two faces. The sweep had photographed exactly one of the four: an
EMPTY archive. This round closed the other three, and the reasons they were open are three different lessons.

  * THE FLAG WAS ADDED, MEASURED BY HAND, AND WRITTEN DOWN AS PROSE. The harness has served a populated archive behind
    `?rows=N` since round 69 — the measurement exists in this file's own header ("window 50 rows … the DOM stays flat
    (395 nodes) whatever the archive holds") — and NO SURFACE ever passed the flag. An ad-hoc measurement is not a
    gate, which is the rule round 150 wrote for `?sessions=N` and this round re-learned for its twin.
  * THE SECOND FACE WAS ONE CLICK AWAY. A row opens the TRAIL of a recorded session — the only place an operator can
    read a session that is over — and no surface had ever clicked one.
  * THE SECOND SCOPE'S ROUTE WAS NEVER SERVED. `Runs` reads `/api/operation`; no stub answered it, the generic branch
    replied `{ok:true}`, `events` was not an array, and the view drew its empty state everywhere it appeared. The
    fourth round in a row where an unserved route turned out to be an unmeasured page (envelope, archive shadowing,
    logs, operation) — and the stub now mirrors the client's own types (`lib/OperationEvent`, `RunBoundary`) rather
    than a shape invented here.

THE MEASUREMENT (126 surfaces, 6,854 rows) came back CLEAN, and that is the result worth recording:

    archive    "50 recorded sessions" · "showing 50 of 50" · rows with identity, kind, state "archived"    4.67+
    trail      a recorded session's trajectory: 3 rounds, exit 1 badges, durations, the session badge
    runs       "1 run" · "8 records" · "7 of 8 records not attributed" · run-row-state "unregistered" /
               "unattributed" · terminal + browser rows with intent and the branches not taken           4.63-16.12

Nothing to fix — and three states that can now REGRESS LOUDLY instead of silently, which is the whole difference
between a photographed state and an unphotographed one. The running list of states this method has added:

    off / the tombstone            round 88   found: a mark collision, then three FILL-inside-a-RING defects
    human / the held session       round 89   found: 2.53 on the graphic bar in dark
    unset / the goal affordance    round 90   clean
    empty / the panel density      round 91   re-added after its prune expired; clean
    menu / the new-session popover round 92   clean, and a limit named (icons are not measured)
    exitfail / the failed last command round 96  NEEDED A WIRE FIELD; now 5.96/5.65 rendered
    failed / the fifth silhouette  round 97   the shape, then the ink chosen by measuring the active tab (round 98)
    trajectory + path              round 101  found: a 2.56 muted ring, unreachable behind a shadowing stub
    archive + trail + runs         round 102  clean — and the third one needed a route that no stub answered

### THE INSTRUMENT REPORTED ITS OWN BLIND SPOT AS A DEFECT (round 103)

This round's finding was produced by the measuring tool, and the page was innocent.

`.device-logs-toggle` — the button that opens a log file's tail, drawn for the first time in round 100 — came back
PRESS-ADDS-NOTHING from a hand-run press measurement. It reads as a control with no `:active` rule. What had actually
happened: the pass took the element's rect AS IT FOUND IT and pressed that coordinate, and **the toggle sits at
y=1582 in an 860px viewport**. `document.elementFromPoint` at its centre returned null, the mouse moved to a point
outside the page, nothing was hovered, nothing was pressed — and the row said the control adds nothing on press.

THE PAGE DID HAVE A DEFECT, and reading the sheet is what established it (not the measurement): `cursor: pointer`,
a real action, NO hover rule and NO press rule. `feedback-check` demands a press only where a HOVER exists, so a
control with neither is invisible to the sheet gate — the same shape that hid the landing's theme toggle in round 95,
one level down. Fixed with the panel's own vocabulary — `:hover` moves the border to `--chrome-ink-faint`, `:active`
is the `translateY(1px)` every other control uses — and re-measured with the corrected instrument:

    light 720x28  rest border rgb(244,244,245)   hovered rgb(161,161,170)   pressed none -> matrix(1,0,0,1,0,1)
    dark  720x28  rest border rgba(255,255,255,0.07)  hovered rgb(111,112,122)  pressed none -> matrix(1,0,0,1,0,1)

THE INSTRUMENT, fixed in its own commit:

  * SCROLL INTO VIEW FIRST, then read the box. An element that will not fit is reported as exactly that — "could not
    be scrolled into the viewport … NOT pressed, and that is not evidence about its press" — because silence about a
    press is not evidence of a missing one.
  * HIT-TEST THE POINT. `document.elementFromPoint` decides whether the pointer reached the element; when it did
    not, the row carries a note instead of a verdict. A COVERED ELEMENT AND A STILL ELEMENT ARE DIFFERENT FACTS.
  * ASK THE DOM FOR THE CONTROLS. `pressPass` takes `discover: N` — visible buttons, links and `[role=button|tab]`,
    deduped by class+size (fifty archive rows cost one press) and capped — and the rail walk uses it. The control
    that started this round had never been pressed because no curated list named it; a list can only contain what
    somebody thought of.

All four rules are pinned in `press-anchor-check` (9 checks), with the mutation proved: deleting the `scrollIntoView`
line fails with "a control below the fold will be 'pressed' at a coordinate outside the page and reported as still".

AND THE TWO AXES THE NEW RECORD VIEWS HAD NEVER BEEN CHECKED ON, measured as rendered while I was there:

    reduced motion   Trajectory 17 -> 0 · Path 11 -> 0 · Settings 13 -> 0 · Archive 13 -> 0 · Runs 13 -> 0
    press            .view-switch button, .traj-collapse, .archive-row, .side-add — all answer in both themes

THE LESSON, for the third time this session and in its sharpest form yet: a measurement is a claim about the
INSTRUMENT as much as about the page. The previous two were a dedupe key that hid a row (round 99) and a fixture
that answered a question nobody asked (round 100). This one is worse, because the instrument did not stay silent —
it ACCUSED.

### THE LAST THREE ROUNDS' WORST FAILURE MODE, IN ONE MORE COSTUME (round 103, part two)

CI's design job died with `FATAL discoverPressTargets is not defined`, two minutes in — and every local gate had
passed, including the pre-commit hook.

The emitter BORROWS helpers by name (`const pressPass = ${pressPass.toString()};`), and the new DOM-discovery helper
was called from `pressPass` without being added to that list. The emitted file **parses** — it is valid JavaScript
that throws when the function is reached — so:

    the emitter's guard      compiles the HARNESS, not the sweep          passed
    press-anchor-check       reads the emitted text for rules it knows    passed
    panel-design-sweep.bash  plants defects in a REPORT and judges it     passed
    sweep-judges.bash        same                                          passed
    the pre-commit hook      runs the emitters (which emitted fine)        passed
    CI's design job          RUNS the sweep on a real browser              DIED

THE GATES ARE JUDGES, AND A JUDGE CANNOT SEE A RUN THAT NEVER REACHED IT. That is the sentence to keep: five checks
read the artifact's TEXT, and the one that would have caught this is the one that EXECUTES it — which is exactly
what the gate table has said about the rendered pass since round 51, applied one level up.

THE EMITTER CHECKS ITS OWN OUTPUT NOW, because it is the only thing that knows which helpers are borrowed: every
name it embeds must be defined in the text it produces. Mutation: replace the embed line with an unused binding —
`--emit` exits 1 with "the emitted sweep CALLS discoverPressTargets but does not define it — the run would die on the
device with 'is not defined'". The same commit embeds the helper, so the sweep runs again.

A NOTE ON THE TWO FAILURES OF THIS ROUND, since they are the same failure: the press pass ACCUSED a control from a
coordinate outside the page, and the sweep CALLED a function it did not carry. Both are instruments making claims
they had not earned, and both were caught by going one level down — `elementFromPoint` for the first, executing the
artifact for the second.

### AN INSTRUMENT THAT HAD TO BE TAKEN BACK (round 103, part three)

The finding was real: `.device-logs-toggle` — a button that opens a log file's tail — carried `cursor: pointer` and
NEITHER a hover nor a press. Neither gate could see that shape: `feedback-check` demands a press only where a HOVER
exists, and the rendered press pass ran a curated list that did not name this card. Both rules were added and
measured on the device:

    light 720x28  rest border rgb(244,244,245)  hovered rgb(161,161,170)  pressed none -> matrix(1,0,0,1,0,1)
    dark  720x28  rest border rgba(255,255,255,0.07)  hovered rgb(111,112,122)  pressed none -> matrix(1,0,0,1,0,1)

THE INSTRUMENT THAT FOUND IT WAS TAKEN BACK, and the three shapes of its failure are the useful part:

    1. THE EMITTED SWEEP CALLED A HELPER IT DID NOT DEFINE. `pressPass` gained a DOM-discovery helper; the emitter
       borrows helpers BY NAME, and the new one was not on the list. The file PARSED — it throws when reached — so
       the emitter's guard, `press-anchor-check`, `panel-design-sweep.bash`, `sweep-judges.bash` and the pre-commit
       hook all passed, and CI died: "FATAL discoverPressTargets is not defined". THE GATES ARE JUDGES, AND A JUDGE
       CANNOT SEE A RUN THAT NEVER REACHED IT.
    2. CENTRING EVERY ELEMENT SCROLLED THE PANEL OUT FROM UNDER THE PASS. `scrollIntoView({block:'center'})` moves a
       control that was already on screen inside a container that scrolls, and the FLOOR reported it: "the press pass
       measured 1 control(s)". An instrument may move the page to REACH a control; it may not rearrange the page it
       is measuring.
    3. REFUSING WHAT IT COULD NOT REACH EMPTIED THE PASS. Scoping the refusal to "the pointer never arrived" still
       cost the four CI press surfaces, because the pass had never had to ask that question before — and a pass that
       presses nothing proves nothing.

WHAT SHIPPED: the toggle's hover and press (measured), and the EMITTER GUARD — every helper the sweep borrows must be
DEFINED in the text it produces, with the mutation proved. WHAT WAS REVERTED: `pressPass`, the rail walk's discovered
targets and the judge's `reached` clause, because a change that reddens CI three times without a verified benefit has
not earned its place yet.

THE UNEXPLAINED FACT, written down so the next attempt does not re-derive it: **the device measures those four press
surfaces at 4 (panel) and 2 (desktop) controls; CI measures 1**, and the five missing rows are all "not rendered on
this page" — so CI is pressing a page where only the rail button matches the curated list. The next things to print
are the URL the mode loop is on when it presses, and the MODE in the press label (the judge prints density/theme, so
a finding does not even say which iteration produced it).

### THE MYSTERY WAS IN THE OUTPUT I HAD ALREADY COLLECTED (round 15)

Round 103's instrument was taken back after CI failed three times, and the ledger recorded an "unexplained fact": the
device measured four press surfaces at 4 controls and CI at 1. Round 15 found the answer by RE-READING MY OWN DEVICE
OUTPUT, where it had been sitting in plain sight:

    PRESS panel/light mode=rail page=panel-Browser measured=1 rows=1

IT WAS NEVER THE MODE LOOP. CI printed `panel/light … measured 1 control(s)` once per density+theme, and I read that
as the mode loop; the rail walk produces entries with the same density/theme label, and the harness's Browser page is
an EXPLANATION PAGE with exactly one control (`BrowserPage` mounts its pane behind `window.valeEmbedded`, so a plain
browser never sees the evidence drawer). A discovered pass that presses the one control such a page has is a COMPLETE
pass. The floor of two — written for curated selectors on Terminal pages — called it vacuous.

TWO LESSONS, both already in this ledger wearing other clothes:

  * THE LABEL MUST NAME THE ITERATION. `density/theme` alone made a rail-walk finding indistinguishable from a
    mode-loop one, and that ambiguity cost a round. The label carries `mode` and `page` now.
  * THE FLOOR MUST BE SIZED TO THE PAGE. A discovered pass reports `found`; the floor is `min(2, found)`, and a page
    with no content controls at all is carried as `found: 0` rather than as an empty set that reads like a vacuous
    pass. "A pass that pressed nothing proves nothing" stays true — it just no longer accuses a page that had one
    thing to press.

WHAT THE RE-LANDED INSTRUMENT DOES, and each clause is the answer to a way it lied before:

    scroll only if not fully visible, by the MINIMUM      (centring everything scrolled a panel out from under its rail)
    press the VISIBLE part, carry whether the pointer     (the toggle at y=1582 in an 860px viewport was "pressed"
      ARRIVED, never drop the row                          at a coordinate outside the page and read as still)
    ask the DOM, deduped by class+size, SKIP THE CHROME   (without the skip the cap went to the rail: Settings has
                                                            sixteen distinct controls and the first five were rail buttons)
    report `found`, floor = min(2, found)                 (one control pressed on a one-control page is complete)

MEASURED, FIRST RUN WITH FULL COVERAGE:

    press: answering=158   dead=0
    RAIL panel-Terminal 11/11 · panel-Settings 16/16 · panel-Browser 0/0 · desktop-Terminal 12/12
    log-toggle: panel-Settings changed=true ["transform"] · desktop-Settings changed=true ["transform"]

THE CONTROL THAT STARTED IT — `.device-logs-toggle`, which had no hover and no press until round 103 — is now
discovered and held by the suite, not merely hand-verified once. That is the difference this round buys.

AND THE EMITTERS NOW ASSERT WHAT THEY BORROW. `assertEmbedded` is shared by all three, because the failure that
reached CI (a helper called but not embedded) is invisible to every local gate: they read the artifact's TEXT or judge
a planted report, and none of them RUNS it.

### A FINDING THAT WAS TRUE AND STILL AN ACCIDENT (round 16)

Round 15 fixed a real 2.5.8 failure — two 22x22 session-row action buttons whose centres were 22px apart — and it was
found by LUCK: the press pass parked the pointer over a session row, the next navigation re-applied the hover, and the
target probe read a state it had never been asked to measure. `.side-actions` is `display: none` until `.side-row:hover`,
so every sweep before that read 0x0 and skipped it.

A SUITE MAY NOT DEPEND ON WHERE THE LAST PASS LEFT THE MOUSE. Two rules, one for each half:

    the pass parks the pointer at (2,2) when it finishes     the accidental finding cannot recur AS an accident
    revealPass hovers a row, measures, and parks again       …and cannot be LOST as one either

MEASURED, first run with the deliberate reveal:

    TARGETS panel panel-Terminal reveal   checked=32   undersized=10
            .side-action  22x22  nearest=26px  passes=true      (the round-15 fix, now held by the suite)
    TARGETS panel rest                    checked=29   undersized=6
    TARGETS desktop rest                  checked=27   undersized=5
    press: answering=158  dead=0

TWO MORE TARGETS ARE SEEN IN THE REVEALED STATE THAN AT REST. That is the whole argument for measuring it: a control an
operator can hit, and that no instrument had ever looked at.

AND THE FINDING NOW SAYS WHERE. This axis measures two states per page, so `target size (panel)` became
`target size (panel panel-Terminal reveal)` — the same lesson the press rows learned one round earlier, when
`panel/light` made a rail-walk finding indistinguishable from a mode-loop one. A finding that cannot be reproduced is
half a finding.

### THE FACT THE PANEL WAS THROWING AWAY (round 17)

`update_status` has reported `checked_at` since it was written — the moment the device last asked its release channel —
and `grep checked_at` across the panel source returned NOTHING. The card said "1.2.433 available" with no age on it,
while the device answers from a 30-second cache, so an update applied a minute ago still reads as available until the
next check lands. A version claim with no time on it is one the reader cannot weigh; the vitals window and the restart
list have carried their span for rounds.

WHAT SHIPPED:

    UpdateStatus.checkedAt      a NUMBER (a string is not epoch ms), finite and positive — absent, never zero
    checkedAge(checkedAt, now)  seconds while an update may be in flight, minutes when idle, hours beyond
    the line                   rendered only where the device reported a time; the clock time rides in the title
    nowMs                      injected, the way the vitals and monitor cards take it, so the age is testable

MEASURED AS RENDERED, on the device:

    light   .update-checked  "checked 2m ago"  12px  tabular-nums  rgb(113,113,122)  4.67 of 4.5
    dark    .update-checked  "checked 2m ago"  12px  tabular-nums  rgb(162,163,172)  6.88 of 4.5
    desktop light 4.79 · desktop dark 6.54 · Settings-busy 4.67 · LogsWarn 4.67 / 6.88

AND THE FIXTURE CHANGED WITH IT, because a state no fixture renders is a state no sweep measures — the rule rounds
96-101 kept re-learning. `/api/update` serves a FIXED `checked_at` two minutes back: fixed rather than relative,
because a page whose numbers move between the harness and the report would make two runs of the same surface
disagree. The judge's stale-stamp clause fired on the first pair I ran (harness and sweep emitted either side of the
fixture change) — a caveat I chose to remove by re-running from one generation rather than read through.

WHAT THIS ROUND DID NOT DO, and why it is the next device-end step: the update verdict an operator actually reads is
still a four-way READING OF `vale-update.log` — a text file written by TWO programs (the CLI writes the
`update requested` receipt, the generated swap script writes the stages). The device knows the fact that decides
whether to re-run an update — DID THE SWAP LAUNCH — at the moment it hands the script to WMI
(`update_from_tgz`'s `ReturnValue == 0` branch), and it could record it as a structured `last_attempt` for
`/api/update` to report. That is a device + wire + panel change with a real migration question (older devices report
the field as absent, so the log reading has to stay as the fallback), and it deserves its own round rather than the
tail of this one.

### THE MOMENT THE DEVICE KNOWS, AND THE TWO PROGRAMS THAT WERE NARRATING IT (round 18)

The question an operator has after pressing Update is "did the swap actually start". The answer was a four-way
READING OF A LOG FILE — `vale-update.log`, written by TWO programs (the CLI writes the `update requested` receipt,
the generated PowerShell swap script writes `update start`, `copy ok` and the restart line). The device knows the
answer at exactly one moment: when it hands the script to WMI and `Win32_Process.Create` returns 0.

    record_update_attempt(from, to, launched)   written in that branch, to logs/update-attempt.json
    last_update_attempt()                      read back; NULL for a body it cannot use
    /api/update.last_attempt                   reported in BOTH arms, including the no-channel one

ABSENT, NEVER AN EMPTY OBJECT. A body without a positive `at_ms` is not a record, and the panel must be able to tell
"this device has never launched an update" from "this device said something this build cannot read" — an empty object
renders as the first. The panel refuses the same shapes again (`parseAttempt`), because the two ends of a wire
contract drift independently: half a record renders as "updated from ? to ? at Invalid Date". A record with only a
`from` IS accepted — it is the half an operator needs to know what they are running now.

WHAT IT LOOKS LIKE, measured as rendered on 126 surfaces (6,920 rows, `stale=false`):

    p.update-attempt        "Last update launched on this device: 1.2.433 → 1.2.435,"   16.27 light / 14.74 dark
    span.update-attempt-age "10m ago"                                                  4.67 light / 6.88 dark
    desktop density 16.69 / 14.00 · Settings-busy and LogsWarn included · need 4.5

TWO AGES, TWO WORDS. `checkedAge` is the age of a READING ("checked 2m ago"); `attemptAge` is the age of an ACT
("10m ago"), and the sentence around it already says what happened. Rendering the act with the reading's verb would
describe the wrong event — which is why the formatter is separate rather than a boolean argument.

AND THE LOGS CARD IS UNTOUCHED, deliberately: its verdict remains a reading of the log, which is the narration and
the fallback for a device whose record is absent. The next step, if the two ever disagree in practice, is to let the
wire fact OVERRIDE the `cli-only`/`cli-swap-launched` arm — the receipt says the CLI reached the device, the record
says the swap started, and those are the two facts that arm is trying to guess from timestamps in text.

### THE CLAUSE NOBODY HAD MEASURED (round 19)

"Pressed and acknowledged states that fire on the EVENT, not on the network, inside a stated budget." The panel has the
mechanism — `useAck` sets its busy key in the same tick as the click — and a unit test pinning the hook's SHAPE. What
did not exist was a measurement of the claim as rendered, which is the only place it can be false: a handler that
awaits anything before calling `run` looks identical in the source and answers a full network round trip late.

WHAT THE INSTRUMENT IS: the fixture can delay every `/api/*` reply (`?slowms=N`, SSE exempt), and `ackPass` times the
gap between a real press and the first visible acknowledgement (`data-busy` / `aria-busy` / `disabled` / a painted
change) against a STATED budget of 100ms — about six frames, and far under any round trip. `msToClear` is reported
and not judged: that one is the network.

IT FOUND TWO CONTROLS WITH NO FEEDBACK AT ALL. `MonitorsCard` wrapped its ADD button in `run`/`ack` and left the row's
`check now` and `remove` calling their props directly. With every reply delayed 900ms, `watch` acknowledged in 5-7ms
and those two showed NOTHING for the whole round trip — invisible in the source, and the first thing an operator
notices on a slow device. Both go through the hook now, with per-row keys so the pressed control wears the ring and
its sibling steps back.

    after:  .monitor-btn acked=true via=data-busy msToAck=5   .monitor-add .btn acked=true msToAck=5
            (8 controls, both densities, both themes; every row 4-5ms against a 900ms delay)

AND THE INSTRUMENT WAS WRONG TWICE BEFORE IT WAS RIGHT — which is the part worth keeping, because both mistakes are
already written in this ledger under other rounds:

  * IT ACCUSED FROM A COORDINATE THE CONTROL DID NOT OCCUPY. The first run clicked `.monitor-btn` and the monitor
    form's button at y=1200-1430 in an 860px viewport, measured nothing, and reported "no acknowledgement" for two
    buttons it had never touched. The press pass's three rules — minimal scroll, clamp to the visible part, hit-test
    the point — are now asserted for this pass too. (Rounds 15-16, same shape, third occurrence.)
  * IT READ ITS BASELINE WITH THE POINTER PARKED AWAY, so `.monitor-btn`'s own hover rule looked like an
    acknowledgement: "acked via=paint, 6ms" for a button whose only response was the pointer being over it. THE
    BASELINE IS THE HOVER — the round-95 rule, now asserted here as well.

A FINDING THAT SURVIVES BOTH CORRECTIONS IS THE ONE WORTH REPORTING, and this round is the argument for measuring a
clause a doc comment already claims: the hook's own header says "it fires on the event", and two of its three call
sites in one card did not.

### WHAT A LIST CANNOT ANSWER, AND WHAT A COUNTER CANNOT PROVE (round 20)

Round 19 measured a curated pair of controls and found two with no acknowledgement at all. The obvious next question —
how many others are there? — is one a list cannot answer, because the controls that answer nothing are exactly the
ones nobody thought to name. So the pass now asks the DOM (chrome skipped, deduped by class+size, capped) and measures
a second page: Memory, whose buttons write and delete device-local records.

MEASURED, AND CLEAN — 14 rows, both densities, both themes, every reply delayed 900ms against a 100ms budget:

    Settings  .monitor-btn 6/6/11/5/6ms via=data-busy    .monitor-add .btn 5/5/5/6ms via=data-busy
    Memory    button.btn 4-9ms via=disabled              (discovered, not listed)

AND ITS FIRST VERSION ACCUSED THREE INNOCENT CONTROLS. Settings renders the CONNECT FORM ahead of everything else, so
a cap of eight spent itself on three tabs and three unnamed buttons — and the first tab is ALREADY ACTIVE, so clicking
it has nothing to do. Three "never acknowledged" findings, three controls that had been asked to do nothing.

THE OBVIOUS EXCUSE DOES NOT WORK. The pass counts `/api/` requests (`window.__calls`) to tell "nothing to wait for"
from "late" — and on a page that POLLS (update, monitors, vitals, restarts, logs) that delta is unattributable: a
background request lands in almost any window. `asked: false` still means something (no request at all in the window);
`asked: true` does NOT mean this control asked. So the finding text says only what it can ("never acknowledged the
press"), the row-level distinction is pinned in both directions, and the judge may still fail such a report on the
floor — a pass whose only row asked nothing proves nothing about feedback, which is what the floor is for.

THE SCOPE IS THE LESSON: a curated list stays where the work is known, discovery goes where every button does
something, and a signal that another process can produce is not evidence about the control you pressed. That is the
fourth time this session an added signal turned out to have a second producer (a request counter on a polling page, a
hover rule mistaken for an acknowledgement, a floor calibrated for curated selectors, a stub answering a route nobody
asked for) — and each time the fix was the same: name what the signal can and cannot support.

### A WAIVER NOTHING HAD USED FOR ROUNDS (round 21)

"Whatever stops earning its place pruned" is the one clause of the objective that has no instrument until somebody
looks. The panel's DECORATIVE list holds the exemptions from the 3:1 graphic bar, each with the band it was MEASURED
at — and one of them had matched nothing since the mark language changed:

    /^div\.rail-dot$/   band 2.25-2.45   MATCHES 0 of 6,876 ROWS ACROSS 126 SURFACES

The working dot's halo was waived when the rows path reported that mark as `div.rail-dot`. `data-live` arrived, the
selector became `div.mark.rail-dot`, and the pattern has matched nothing since — while the ELEMENT is measured and
fine: 68 rows name it, all above their bar (6.50 light / 10.99 dark against 3), and its fill is held at exactly 3.00
by the panel gate, which fails on a mutation to #8a2a07 (1.90). Nothing needed the exemption.

    PRUNED, with the measurement, the band, and where the exemption lives now written where the entry was
    REPORTED NOW: the judge prints how many DECORATIVE entries matched no row, naming them, with the row and
    surface counts so a reader can tell a stale entry from a partial run

A NOTE AND NOT A FINDING, deliberately: this list is judged per run, and a run that measures one axis (the ack pass
alone) has rows from nothing else — every entry would look stale. The counts are what make the difference visible.

PINNED IN THREE DIRECTIONS: a waiver the run used is not called stale; a waiver no row matched is named with its row
count; and THE PRUNE ITSELF — the row the deleted entry used to excuse is now a finding, because a prune that still
excuses something is a prune in name only. One gate case had to move for this: it planted `div.rail-dot 2.33` to pin
"a waiver at its measured ratio still passes", which was pinning the entry being removed. It plants
`span.approval-grant 1.19` now, whose entry exists — and the case is more honest for it, because the rule was never
about that one dot.

WHERE TO LOOK NEXT for weight of this kind, recorded rather than remembered: the `ignore` list beside DECORATIVE (the
hover path's exemptions, which the same note does not cover), the panel's `UNSTYLED` waivers, and the `DECORATIVE`
bands themselves — a band that no run has landed inside for several rounds is a number nobody has checked.

### AN EXEMPTION MAY ANSWER FOR ITSELF (round 22)

Round 21 pruned a waiver nothing had matched and recorded where to look next: the `ignore` list beside `DECORATIVE`,
whose entries are consulted against FINDINGS rather than rows. Asked the same question, one of its two entries turned
out to be used and one turned out to be a GUARD — and "prune it or say why it stays" is only actionable if there is
somewhere to say it.

THE MEASUREMENT, from a run with the hover and reflow axes on the device (126 surfaces, 6,920 rows, stale=false):

    hover    4 surfaces · 14/14 and 11/11 interactive elements · underAA EMPTY
    reflow   @640 a toolbar scroller (a note by design) · @320 THE DOCUMENT SCROLLS (506 > 320), and the harness
             artifact entry SET IT ASIDE — the entry is used, and the note never mentions it

WHY THE HOVER ENTRY IS A GUARD AND NOT WEIGHT — and this is the distinction round 21's prune turned on: its test is
`/div\.rail-dot/`, UNANCHORED, so it still matches the row's sel (`div.mark.rail-dot` since the mark language gained
`data-live`). The pruned DECORATIVE entry was `/^div\.rail-dot$/`, anchored, and matched nothing at all — a pattern
that cannot fire is weight; a pattern that can fire, for a state that currently passes, is a guard. The entry now says
so in one line, with its numbers, and the note reports it as `dormant as declared` instead of asking for a prune.

THE FIXTURE LESSON, third time this session: the gate case for this first judged a report with NO reflow row — where
the reflow entry is legitimately unused and undeclared, and the judge was RIGHT to ask about it. The instrument was
fine and the fixture was wrong; it now mirrors a real clean run (a 320px finding whose scrollers are all tab children,
an SSE record with `opened: true`, and the `docScrollsSideways` flag the clause actually reads — not the raw numbers,
which is how the first attempt produced no finding to exempt at all).

WHERE TO LOOK NEXT, unchanged from round 21 and now one shorter: the panel's `UNSTYLED` waivers, and the DECORATIVE
bands themselves — a band no run has landed inside for several rounds is a number nobody has checked.

### A BAND WIDER THAN ITS EVIDENCE (round 23)

A DECORATIVE entry waives a RATIO, not an element (round 95), so the band is what decides: a row inside it is set
aside, a row outside it is a finding. The failure that leaves no trace is the other direction — a band WIDER than its
evidence excuses a drift nobody measured, and nothing in the suite was asking.

MEASURED: the grant chip's entry carried **1.10-1.35** while its own reason claimed "the band covers what was measured
and nothing else". The four ratios this suite has ever seen on that element are **1.19, 1.20, 1.25, 1.27** — 32 rows
across 126 surfaces. That is ~0.09 of unearned margin on each side: a move to 1.12 or 1.33, real movement toward the
3:1 bar, would have been waived in silence. The icon chip's band had 0.05 on each side for a 1.05-1.10 observation.

    grant chip  1.10-1.35  ->  1.17-1.29   (observed 1.19-1.27)
    nm-ico      1.00-1.15  ->  1.03-1.12   (observed 1.05-1.10)
    each declares slack: 0.02 — probe rounding only, because ratios print to two decimals and a band written at the
    printed value would refuse a true 1.185

AND THE INSTRUMENT'S FIRST VERSION HAD THE SAME HOLE IT WAS BUILT TO FIND. It computed `Math.min(lo - bandLo,
bandHi - hi)`, so a run that saw a single 1.19 inside a 1.17-1.29 band reported NO margin at all while the upper side
carried a tenth nobody had measured. The margin is per side now, the note names both with the observation counts
("1 distinct over 1 row(s)") so a partial run is visibly partial, and the gate's case for the far side (`0.10 above`)
is one the min-of-both-sides version could not produce.

TWO ARITHMETIC NOTES worth keeping, because both produced a false reading before the numbers were looked at:

  * 1.29 - 1.27 is 0.020000000000000018 in binary floating point, so an EXACT band (observed range plus exactly the
    declared slack) reported itself as over its own slack. The comparison carries an epsilon; the alternative was a
    band written 0.001 tighter than its justification, which is the same defect in the other direction.
  * the ratios are printed to two decimals, which is why a slack exists at all rather than an equality. A tolerance
    invented to hide a drift is a hole; a tolerance that covers the READER's precision is honest, and the entry says
    which it is.

The full 126-surface report judges clean with no band note. The panel's remaining known weight is the `UNSTYLED`
waivers, which is where the next round of this kind should look.

### THE THIRD LIST, AND THE STALE ENTRY IT FOUND ON ITS FIRST RUN (round 24)

Rounds 21-23 asked "is this exemption still earning its place?" of two lists: `DECORATIVE` (waived against ROWS) and
`ignore` (waived against FINDINGS). The third is `implicitStates`, which waives classes that are ON SCREEN WITH NO
MATCHING RULE — and it had no way to say an entry was unused either. An entry stops being used the moment its class
leaves the markup OR gains a rule; either way the reason attached to it is kept for nothing.

The instrument found one immediately. The unstyled pass (4 seconds, 1129 styled classes per density over two pages)
reports ELEVEN unstyled names — the eight xterm ones, `composition-view`, `terminal`, `warn` — against a declared
count of TWELVE:

    serial: "a session-kind modifier; the element is painted by its [data-kind] rule"   <- NEVER SEEN

The class no longer exists: the mark language's TabBar emits `className="mark tab-dot" data-kind={s.kind}` and the
sheet paints the kind with `.tab-dot[data-kind="serial"]`, so nothing puts a bare `serial` on screen. The paragraph
above the list still counted "eleven such names ... the last two are ours" from when it was written; the last two are
`terminal` and `warn` now, which is what the same run says. Pruned, with that paragraph's correction recorded where
the entry was.

    A run that saw every declared class says NOTHING (verified against the real eleven)
    A run that saw none names them all, with the reason each is kept, and the counts that make a partial run visible

ALL THREE LISTS ANSWER THE SAME QUESTION NOW, and the three answers are worth keeping side by side because they are
three different verdicts on the same kind of entry:

    DECORATIVE      one waived ratio nothing had matched since a selector changed   PRUNED (round 21)
    ignore          one used, one a GUARD whose pattern can still fire             DECLARED DORMANT (round 22)
    implicitStates  one stale class name from pre-mark-language markup             PRUNED (round 24)

THE COMMON SHAPE, for the next list somebody adds: an exemption is a promise about the future written from evidence
about the past, and nothing in a suite naturally expires it. The expiry has to be asked for — and the cheapest place
to ask is where the evidence is counted anyway, with the SIZES printed so a partial run cannot be mistaken for a
stale entry. The panel's own instruction file now carries all three notes in its gate table.

### A DECLARATION IS EXERCISED BY THE PASS THAT ASKS (round 25)

Round 24's instrument reported, in CI's own log, that the console declares ONE unstyled-by-design class and the run
saw NONE:

    note: 1 of 1 declared unstyled-by-design class(es) were not seen in this run (0 unstyled name(s) over 6 pages)
      stat-off — the Overview's default tone: the base .stat-card::before already paints the faint bar that off means

The declaration was correct and could still fire. The Overview builds a stat-card plus a stat-<tone> class, the sheet
has rules for ok/warn/info only, and THREE of its four stats take the `off` tone when there is nothing to report — no
device online, no channels, no keys. The empty-fleet fixture already existed and visited only `#/devices` and `#/keys`,
so the one state that needs the declaration had never been rendered by anything.

    FIX 1  #/ joins the empty-fleet surfaces: 54 -> 56 surfaces, 1632 -> 1732 rows, 30 -> 32 name checks, every axis
           clean. THE NOTE DID NOT MOVE — because the CENSUS has its own page loop and walks the six POPULATED pages.
           The state was rendered; the pass that asks the question was not looking at it.
    FIX 2  the census visits the empty fleet too, and the note becomes what it should have been all along:
             note: stat-off is unstyled by design — the Overview's default tone: (…)     (once per theme)

THE LESSON IS NARROWER THAN "ADD A SURFACE", and it is the reason the first fix looked finished: A SUITE CAN RENDER A
STATE AND STILL NOT MEASURE IT, and the summary line is what hides that — 56 surfaces and 1732 rows read as success
whether or not the census that asks about a class ever saw the page. The rendered pass and the census are two
instruments with two loops, and a state has to reach the one whose question it answers.

FOUR PLACEMENTS WERE REJECTED BEFORE ONE LANDED, which is worth keeping because three were caught by machinery rather
than by reading:

  * a backtick in the comment (`stat-off`) — the emitter's own guard, 54th time this session;
  * an interpolation-looking `stat-<tone>` written with a dollar-brace — inside these template literals that is CODE,
    and it threw `tone is not defined` at emit time;
  * the block at MODULE level, where `await` is a syntax error the emitted-script parse check caught immediately;
  * the block inside the per-page loop, where it would have left the hover pass measuring a page it never navigated to
    — caught by reading the loop rather than by a gate.

PINNED: `press-anchor-check` now requires the emitted console sweep to carry the Overview in the empty-fleet loop, and
the mutation (dropping `['overview-empty', '#/']`) fails it with that sentence. The remaining unexercised declaration
in either UI is none today — the panel's eleven are all seen, the console's one is now seen in both themes.

### HALF THE SILHOUETTES WERE UNVERIFIED (round 26)

The mark-collision check compares the states a surface HAPPENS to render. A family with six declared states and three
rendered therefore has half its shapes unverified — and a collision among the unrendered half cannot be seen at all.
That is the round-96 rule ("a state with no surface cannot be measured") applied to the whole mark vocabulary, which
is the one place the objective asks for a silhouette PER STATE.

WHAT THE MEASUREMENT SAYS, over 126 surfaces — the states NOTHING rendered:

    cmd-dot        declares 6, rendered 3 (fail, muted, ok)     -> warn, running, bg      UNVERIFIED
    traj-ev-dot    declares 6, rendered 1 (fail)                -> ok, warn, running, muted, bg
    plug-dot       declares 4, rendered 2 (success, warn)       -> error, ongoing
    ag-dot         declares 2, rendered 1 (armed)               -> off
    monitor-mark   declares 2, rendered 1 (is-flapping)         -> is-up
    boot-mark      declares 1, rendered 1 (warn)                -> info
    mark           declares 5, rendered 5                       -> COMPLETE (round 96's exit-code flag did its job)
    sc-dot, monitor-dot                                         -> COMPLETE

The declared states are read from the BUILT SHEET with comments stripped first (prose about a selector is not a
selector — the lesson `css-vars-check` and `retired-colours-check` each record from their own first run), and the note
carries the row/surface counts so a single-axis run cannot be mistaken for a vocabulary that is entirely unrendered.

ONE STATE GAINED A SURFACE: an unterminated `command/start` in the operation fixture is the panel's own definition of
a LIVE card (useCommandEvents: "surface it as a LIVE card"), so cmd-dot's `running` is now painted by the row that
means it. The rest stay NAMED rather than forced.

AND ONE ATTEMPT WAS REVERTED, which is the part worth remembering: setting `playwright.running` in the plugins stub is
the obvious one-line way to render plug-dot's `ongoing` — the hook maps that field straight to the state — and it HUNG
the sweep. The plugins page polls while a browser is running, the page never settles, and the run produced no report at
all; the device showed a live node process and no report file after four minutes. A FIXTURE THAT CHANGES WHAT THE PAGE
DOES IS NOT A FIXTURE THAT CHANGES WHAT IT SHOWS. The failed attempt and its symptom are written where the fixture is,
so the next reader does not spend a sweep rediscovering it.

NEXT, in the order the note names them: `traj-ev-dot` is the same vocabulary as `cmd-dot` rendered in a second place,
so whatever renders one should render the other; `plug-dot`'s `ongoing` needs a fixture that does not start a poll;
`ag-dot`'s `off`, `monitor-mark`'s `is-up` and `boot-mark`'s `info` are each one state of a two-state family.

CORRECTION TO THE SECTION ABOVE, MADE AN HOUR LATER BY THE INSTRUMENT IT DESCRIBES. "ONE STATE GAINED A SURFACE" was
wrong. CI's own log, on the commit that claimed it:

    panel: 6938 text nodes · 126 surface(s)              (+18 rows, so the new event DID render content)
    note: mark family cmd-dot declares 6 state(s) and this run rendered 3 (fail, muted, ok) — NO SURFACE RENDERED
          data-state=warn, data-state=running, data-state=bg

The unterminated `command/start` feeds `/api/operation`, which is the History page's RUNS scope; the command CARDS come
from a session's audit events, which is a different stub. The row arrived and the state did not. So the note did not
just measure the gap — it FALSIFIED THE ROUND'S OWN CLAIM, in the same log line as the evidence for the claim, and the
fix is a corrected sentence where the claim was made rather than a quietly reworded summary here.

ZERO of the six families moved this round; the count of unverified silhouettes is unchanged, and the note now says so
on every run. That is the honest state of this thread, and it is why the note is a note: the loop's next attempt at
`running` should start from the session audit stub, not from the operation feed.

### THE CHROME THAT DID NOT SAY WHAT IT WAS (round 27)

An operator looking at the desktop window asked three questions about chrome no instrument in this suite had ever been
pointed at — which is itself the finding, because all three were answerable from the code and none of them from the
screen:

    "(2) Vale Agent"                        what is the 2?
    "Trajectory" beside "Path"              are these the same thing twice?
    "Take control" / "+ Set a goal…" /      what do these DO?
    "Ask before each command"

1. THE 2 IS BY DESIGN, and the design is right for a browser TAB: `titleFor` puts the count first because a tab
   truncates from the right ("a title that ends in `…` before the number is a title that told nobody anything"), and
   it reads `(N) [⚠] Vale Agent`. The two items are the two monitors the chip at the bottom left names. Nothing
   changed; the question is recorded here because the next person will ask it in a WINDOW, where that rationale does
   not apply and the bare number is still cryptic.

2. THE TWO VIEWS ARE NOT THE SAME, AND NOTHING VISIBLE SAID SO. `Trajectory` is the RAW audit log — every event as
   it happened, status events included. `Path` is the same work SUMMARISED, with the declared plan, the reasoning
   recorded per step, and who ran each step (from the `control` audit events). The difference lived in a `title`
   attribute: one hover, one wait and one mouse away from invisible. Each view now carries a caption on its own row —
   "raw audit log — every event as it happened" and "plan, reasoning and who ran each step".

3. THE THREE CONTROLS WEAR THE SAME PILL AND ARE THREE DIFFERENT KINDS OF THING: an ACTION (take this session's
   keyboard), a PROMPT for a missing value (set a goal), and a POLICY toggle (ask before each command). All three
   explanations were `title` attributes. The group is labelled "Session" now, so the row reads as settings of one
   thing; the tooltips still carry the detail.

MEASURED AFTER: the design job's panel summary moved 6938 -> 6990 text nodes over the same 126 surfaces, and no row
naming `traj-sub`, `path-sub` or `strip-label` appears among its findings — the three captions clear AA on every
surface they render on. All three are chrome, so they are quiet and still: 11px, `--muted` (TEXT, so AA wants 4.5),
no transition and no animation.

THE LESSON, which is the same one this ledger has recorded about the marks and the wire: A MEANING THAT ONLY APPEARS
ON HOVER IS A MEANING THE SCREEN DOES NOT CARRY. Three tooltips were written, tested and correct; the operator still
had to ask. Where a label cannot say it, a caption on the row can — and the test that pins it renders the component,
which is the only way to notice a sentence nobody can see.

### THE STATE THAT GAINED A SURFACE, AND THE SIX FAMILIES THAT NEVER HAD ONE (round 27)

Round 26 claimed cmd-dot's `running` had gained a surface. The mark-coverage note said otherwise in the same CI log —
"rendered 3 (fail, muted, ok)" — because the event was added to the OPERATION feed (the History page's Runs scope)
while the command CARDS are built from the session's AUDIT TRAIL. The trail is where it belongs, and one trailing
`command/start` is the panel's own definition of a live card:

    MEASURED ON THE DEVICE, 126 surfaces / 7014 rows / stale=false
      cmd-dot   rendered: fail, muted, ok, RUNNING          (was: fail, muted, ok)

AND THAT RUN EXPOSED A BLIND SPOT IN THE NOTE ITSELF, which is the part worth keeping. It enumerated the families the
REPORT contained, so a family that renders NOWHERE simply did not appear — and `traj-ev-dot` left the list instead of
being reported at zero. The families now come from the SHEET, by the probe's own naming rule, and the count went from
six to TWELVE:

    cmd-dot       6 declared, 4 rendered   warn, bg                    unverified
    traj-ev-dot   6 declared, 0 RENDERED   every state                 unverified
    plug-dot      4 declared, 2 rendered   error, ongoing              unverified
    ag-dot        2 declared, 1 rendered   off                         unverified
    monitor-mark  2 declared, 1 rendered   is-up                       unverified
    boot-mark     1 declared, 1 rendered   info                        unverified
    side-dot      2 declared, 0 RENDERED   data-kind ssh/serial        unverified
    tab-dot       2 declared, 0 RENDERED   data-kind ssh/serial        unverified
    update-state  2 declared, 0 RENDERED   is-error, is-ok             unverified
    monitor-state 2 declared, 0 RENDERED   up, down                    unverified
    monitor-chip  1 declared, 0 RENDERED   is-flapping                 unverified
    notify-state  2 declared, 0 RENDERED   is-granted, is-denied       unverified
    mark          5 declared, 5 rendered   COMPLETE
    sc-dot, monitor-dot                    COMPLETE

SIX FAMILIES WITH NO SURFACE AT ALL is the round-96 rule one level up: a family nothing paints is as unmeasured as a
state nothing paints, and the collision check — which compares the states of one family — can say nothing about either.

TWO GATES CAUGHT TWO PATTERNS RATHER THAN TWO DEFECTS, and both are the same mistake in different clothes:
`harness-fixture-check`'s mutation was written against the SOURCE form of the trail while the harness EMITS it as JSON
on one line (its own self-test refused to run, which is why that self-test exists), and the predicate then matched an
intent string that round 26 had ALREADY put on the operation feed — so it passed with the audit entry deleted. A check
on a phrase that exists twice cannot tell you which one you deleted.

NEXT, in the order the note names them: the six zero-rendered families are the cheapest wins (each is a fixture state,
not a code change), and `traj-ev-dot` — the same vocabulary as cmd-dot rendered in a second place — should follow its
sibling automatically once its view is measured with a card in it.

### ON SCREEN IS NOT UNRENDERED (round 28)

Round 27's sheet-enumerated note counted TWELVE mark families and reported six with zero painted states. Three of the
six are not gaps, and the device run this round measured why:

    ON SCREEN — every state-mark class the probe saw, over 126 surfaces:
      ag-dot, boot-mark, cmd-dot, dot, dtab-dot, mark, monitor-dot, monitor-mark, monitor-state, path-step-dot,
      path-step-why-mark, plug-dot, rail-dot, sc-dot, side-dot, tab-dot, waiting-mark

    tab-dot, side-dot   the class is on every tab, and the probe attributes it to `mark`: the state hangs off
                        `data-live`, and the family is then the FIRST class. It IS measured — under another name.
    monitor-state       on screen, and its elements are larger than the 40px the mark probe measures — a chip, not a
                        mark. A sizing question, not a missing surface.

The probe reports the classes it saw now (`marks.present`), and the note separates the two verdicts. THE REAL GAPS,
which keep their numbers: `ag-dot off` · `boot-mark info` · `monitor-mark is-up` · `plug-dot error, ongoing` ·
`cmd-dot warn, bg` · `update-state is-error/is-ok` · `monitor-chip is-flapping` · `notify-state is-granted/is-denied`.

THREE PATTERN SLIPS IN TWO ROUNDS, all one shape — A PATTERN IS NOT A NAME UNTIL IT IS ANCHORED:

  * the fixture mutation was written against the SOURCE form of a trail the harness EMITS as JSON on one line;
  * the fixture predicate matched an intent string that round 26 had already put on a DIFFERENT feed, so it passed
    with the audit entry deleted;
  * this round's gate case grepped `tab-dot declares … NO SURFACE RENDERED` and matched `dtab-dot declares …`.

Every one was caught by a gate rather than by reading, which is the only reason they cost minutes instead of a release.
The rule for the next one: when a check greps for a NAME, put the context in the pattern (`mark family tab-dot
declares`), and when it matches TEXT a program emitted, take the pattern from the emitted file rather than from the
source that produced it.

### THE AXIS THE DEFECT LIVED IN, MEASURED WHERE IT LIVES (round 30)

Round 29 closed a loop with the live probe, and the loop taught one thing worth acting on: the defect it found
(`ag-dot[off]` at 2.56:1) was in the MARK axis, while the probe measured only contrast — and the sweeps' own
mark-coverage note counts six families the HARNESS never paints. The device's real states are not the harness's states,
so the mark axis belongs on the live panel.

    families  mark[working] · sc-dot[ai] · ag-dot[off]        ← measured on d1, both densities
    collisions []   ringFill []   textFailing []   graphicFailing []   verdict { ok: true } → exit 0

ONE IMPLEMENTATION, WHICH COST TWO FAILURES TO GET RIGHT. The IIFE was extracted into `marksSource(rootSelector)` so the
sweeps and the probe cannot drift — and it then died twice before it ran:

  * as a bare const it evaluated with `ROOT_SEL` undefined — the same failure the comment above `pageChecks` records
    from the extension sweep, which is why the placeholder substitution exists at all;
  * as an interpolation inside `PAGE_CHECKS_TEMPLATE` it read `rootSelector` at MODULE LOAD, where that name does not
    exist ("Cannot access 'MARKS_SOURCE' before initialization" was the first symptom, "rootSelector is not defined" the
    second). The template gets a placeholder of its own now and `pageChecks` substitutes it at call time.

AND THE READING IS A GUARD: four lists must be empty, and a non-empty one is a non-zero exit. What that exit MEANS is
written beside the step in `AGENTS.md`, because the live panel's state varies with what the device is doing — two
sessions today, fourteen another day — so it says "look at this", not "the build is broken".

### TWO STATES OF SIX, AND THE RULE THAT KEEPS FINDING THEM (round 31)

The mark-coverage note has reported `cmd-dot` as "rendered 4 (fail, muted, ok, running)" since it was written. Two of
the six states had no surface anywhere in the suite — and one of them is the state round 29 was about: `bg` is what
`stateFromEnd` maps a BACKGROUNDED command to, and what the trajectory's private copy used to rename to `warn`. The
fixture needed two pairs of events, because the reason travels on `command/end`, which is the field the derivation
switches on:

    {"seq":14,…,"kind":"command/end","reason":"backgrounded","duration_ms":400}
    {"seq":16,…,"kind":"command/end","reason":"interrupted","duration_ms":1500}

    MEASURED ON THE DEVICE (128 surfaces, 7189 rows, stale=false):
      cmd-dot   bg, fail, muted, ok, running, warn      was: fail, muted, ok, running
      ag-dot    armed, off                              round 29's disarmed ring

THIS IS THE THIRD TIME THE SAME RULE HAS ADDED A SURFACE, and the pattern is now cheap enough to state: `?exitfail=1`
(round 96, the failure mark), `?appr=off` (round 29, the disarmed ring), and this (round 31, the last two command
endings). Each was a state the vocabulary declared, that no fixture could produce, and that therefore no gate could see —
and each is pinned in `harness-fixture-check`, the check whose subject IS that rule, with a mutation that removes the
ability to render it.

WHAT IS STILL WITHOUT A SURFACE, from the note's own list, so the next round does not have to rediscover it:
`traj-ev-dot` (all six states — the trajectory view renders no dot in the pages pass), `plug-dot` (error, ongoing),
`monitor-mark` (is-up), `boot-mark` (info), `update-state` (is-error, is-ok), `monitor-chip` (is-flapping),
`notify-state` (is-granted, is-denied). [`ag-dot` and `cmd-dot` are complete as of this round.]

### THE SURFACE FOUND TWO DEFECTS ON ITS FIRST RUN (round 31, continued)

Giving `cmd-dot`'s last two states a surface did not merely complete a count. CI's design job failed on the next run
with two defects that had been invisible for exactly as long as `bg` was unrendered:

    Trajectory-light/dark · Desktop-Trajectory-light/dark
      states of one mark paint identically — cmd-dot: muted and bg paint identically (50%/flat/-/ring)
    Path-dark · Desktop-Path-dark
      span.path-step-tag.s-bg "backgrounded" — painted rgb(0, 0, 0) on rgb(28, 29, 34), 11px text, needs 4.5   → 1.25:1

1. THERE WAS NO `.cmd-dot[data-state="bg"]` RULE AT ALL. The state fell to the base class and computed as `muted`'s
   hollow ring, so two of this vocabulary's six states were the same shape — the objective's "every state has its own
   silhouette" broken precisely where nothing was looking. It is a RING THAT IS NOT ROUND now (radius 2px, the channel
   `warn` and `fail` already use), and `designScale.test.ts` refused the raw radius until the selector joined the SHAPE
   exemption list with its reason — which is the gate doing its job: an exemption is by selector so a stray `2px`
   cannot ride along on it.

2. THE PATH'S STEP TAG HAD NO INK FOR `bg`. Every other tone was listed (`s-ok`, `s-fail`, `s-warn`, `s-running`,
   `s-muted`) and `s-bg` was not, so the tag inherited the page colour — black in the dark theme, on the one view whose
   job is to say how each step ended. Invisible, in both densities, since the tone was added.

BOTH WERE FOUND BY THE SAME ACT, WITHIN A MINUTE OF EACH OTHER: adding a surface for a state nothing rendered. That is
now the third time (`?exitfail=1` → the failure mark's ink, `?appr=off` → the disarmed ring's 2.56, this → a collision
and an invisible label), and the hit rate suggests the remaining list is not housekeeping but a queue of defects:

    traj-ev-dot (all six) · plug-dot (error, ongoing) · monitor-mark (is-up) · boot-mark (info)

THAT LIST WAS WRONG WHEN FIRST WRITTEN, IN THE WAY THIS LEDGER KEEPS RECORDING (round 32). It named nine entries; three
(`update-state`, `notify-state`, `monitor-state`) are TEXT LINES — a mono paragraph, a card's sentence, a word beside a
mark — whose is-error/is-ok/is-granted variants are ink on words, where the silhouette question does not apply because
the word IS the state. Two more (`run-row-state`, `archive-state`) are full-width ROWS. They were reported as gaps
because the judge's family rule is a NAME rule (`*-state` matches) and because `present` — the set that answers "is this
class on screen at all" — was collected only from elements small enough to be marks. The rule now carries three declared
text classes with reasons (a heuristic would also hide the day one of them becomes a real mark), `present` is collected
BEFORE the size filter, and the queue is four entries — each a state of a family that really is a mark. Eight surfaces
of false queue cost a round; the fix cost twenty lines.

### ONE CLICK, FOUR DEFECTS, AND A PRUNE THE GATES REFUSED (round 33)

`traj-ev-dot` measured **0 of its 6 states across 128 surfaces** while the view rendered perfectly — six round headers,
no event rows. The cause was the click this suite never made: only the NEWEST round is open by default, and the newest
round is the fixture's live `reboot`, a command with no output yet, so its body was `(no output yet)` and every event
row lived inside a COLLAPSED round. The suite opened the TAB in round 101; it opens the ROUNDS now. The note only began
reporting 0 after round 31 added that trailing command — a fixture change can hide a family as easily as reveal one.

WHAT THE CLICK THEN EXPOSED, in the order it arrived:

  1. THE MUTED EVENT DOT WAS INVISIBLE. `--ds-neutral-300` is rgb(212,212,216): **1.42:1** on the light surface where a
     graphic needs 3. The sheet's defence — "a quiet FILLED dot: texture, not a marker" — is an argument about WEIGHT,
     and an invisible dot carries no state at all. `--state-muted` (4.83 light / 6.5 dark) is what its three sibling
     rings already used. Verified on the device: 8 rows, **BELOW BAR: 0**.
  2. MUTED AND OK DREW THE SAME SILHOUETTE. With the fill corrected, the collision check fired on all four trajectory
     surfaces: `traj-ev-dot: muted and ok paint identically (50%/flat/-/solid)`. Two states of one mark, told apart by
     colour alone — forbidden by the objective, and TRUE SINCE THE ASYMMETRY WAS WRITTEN. It was invisible only because
     the family rendered nothing. The asymmetry is gone: the rail's muted is the same hollow ring as the command card's,
     and `statePalette.test.ts` now compares the two rules with their tokens masked, so colour can never again be what
     tells two muted dots apart.
  3. AND A PRUNE THE GATES REFUSED, WHICH IS THE PART WORTH KEEPING. `.traj-ev-dot[data-state="running"]` looked
     unreachable — `eventDotState` maps every event through `stateFromEnd(true, …)`, so it returns verdicts and never
     `running` — and I deleted it. Four tests in `statePalette.test.ts` refused: "EVERY state has a visual channel in
     the BUILT sheet", "the four VERDICT states agree across both renderers", "running and ok differ WITHOUT relying on
     animation", "no two states collapse onto one colour". Their rule is the better one and it is now written down:

         An unreachable STATE is the DERIVER's fact. The CHANNEL is the SHEET's obligation.

     Every state in the vocabulary must have a channel in every renderer, so a state that becomes reachable later — a
     new deriver, a new branch — cannot render as an invisible dot. Deleting the arm would have removed the guard
     against the very defect this round fixed two lines away. The arm, its `PURPOSES` entry and the test loop are
     restored, each carrying the story of the attempt; the mark-coverage note's "no surface rendered running" is a true
     statement about the DERIVER.

FOUR DEFECTS FROM ONE CLICK, three of them in code that had been there for rounds, and one of them mine. The pattern
from the last three rounds holds: a state with no surface hides whatever is wrong with it, and the hit rate has not
dropped yet.

### THE SURFACES QUEUE, CONTINUED: A BOOT TONE, A DEVICE PUSH, AND A SKIP THAT READ AS A PASS (rounds 34-41)

Four more states had declared silhouettes and no surface anywhere. Each got one, and the pins are the point:

  * **`boot-mark info`** (rounds 34/36). The chip is a mark with two tones and only the fault was ever painted:
    `.boot-mark` is the base rule (a warn TRIANGLE) and `.boot-mark.info` the exception (a DOT for "just restarted").
    `info` needs BOTH `last_boot_kind: 'replaced'` AND an uptime under `REPLACED_NOTICE_SECS` — a restart the operator is
    meant to notice because it just happened — so `?boot=replaced` moves both fields together, and the sweep gained
    `BootReplaced-<theme>`.
  * **The alert strip's two tones** (round 39). `monitor-mark` declares two states and only the flapping one inside the
    chip was ever painted; the OTHER two uses live in the alert strip (`.monitor-mark.is-up` for a recovery, the base
    rule for an outage) and that strip reads `monitor-change` frames off the SSE stream — the panel's ONE channel for
    device-initiated frames (`useSSE` dispatches every frame as `vale-<ev>`). The stub served exactly one EMPTY frame, so
    a state the device genuinely pushes had no surface. `?monitorchange=up|down` builds the device's own frame shape (the
    fields `monitor.rs` emits, so `parseMonitorChange` accepts it) and enqueues it BEFORE the empty frame, in the same
    `start()` — the stream still closes after its frames, the shape round 156 proved renders CONNECTED. This is the
    opposite of round 194's prune: that fixture's consumer could not be rendered at all; this frame has a consumer on
    screen and simply nothing to deliver it.
  * **A SKIP THAT READ AS A PASS** (round 40), which is this repository's oldest recurring defect, found this time in CI
    itself rather than in a surface. The installer-integrity step ran on a conditional: without pwsh it printed
    `::warning::…the LOGIC was NOT tested here` and **exited 0** — so the one step that tests the installer's SHA-256
    verification could decide at run time to test nothing and the run would still pass. A `::warning::` is a green. It
    exits 1 now, and `build-pins.bash` holds it: that gate already enforced round 146's rule ("every scripts/test file is
    invoked from ci.yml"), and this is that rule's sibling one step further in — a step the workflow DOES invoke that can
    still test nothing. Mutation: restoring the warning-and-continue branch fails with "FAIL: a missing pwsh FAILS the
    step" (measured: mutated rc=1, restored rc=0).

    AND THE FIXTURE PINS CORRECTED A CLAIM. Round 29's commit message said the fixture check pinned `?appr=off`; it did
    not — the case added that round was for the command endings. `harness-fixture-check` now holds `?appr=off` AND
    `?boot=replaced` with three mutations, including the one that reproduces that flag's own first bug (mutating the
    SEED instead of the list entries copied from it, which changed nothing on screen while looking correct).

### AND A MEASUREMENT I INVALIDATED MYSELF: THE DESIGN VERDICT I KEPT REPORTING AS "IN PROGRESS" (round 41)

`ci.yml` carries `concurrency: {group: ci-${{ github.ref }}, cancel-in-progress: true}`. Every push therefore CANCELS the
run still in flight — including its design job. Three times this session I committed the next round's work while the
design job was measuring the previous round's surfaces, watched the run sit at "in progress", and reported it as such;
the run's actual conclusion was `cancelled`, and the surfaces I thought were being measured never were.

The rule that follows is cheap and worth stating: **a push is not free while a run is measuring — either wait for the
design job, or accept that only the last run's verdict exists.** The two surfaces above (boot tone, alert strip) are in
the same tree now, so one run covers both; that is the shape to aim for deliberately rather than by accident.

### THE THIRD COLLISION, AND WHY THE SURFACES KEEP PAYING (rounds 42-43)

Adding the alert strip's fixture found the third instance of one defect class in a week:

    MonitorUp-light/dark: states of one mark paint identically
      monitor-mark: is-up and is-flapping paint identically (6px/rotated/-/ring)

`.monitor-alert .monitor-mark.is-up` was `transparent` plus a green border — a HOLLOW DIAMOND — which is exactly what
`.monitor-mark.is-flapping` draws in the status-bar chip. Two states of one mark, told apart by colour alone. The three
instances in order, and what each was hiding:

    cmd-dot        bg and MUTED painted identically        — a rule that did not exist at all, in a family rendering 4/6
    traj-ev-dot    muted and OK painted identically        — an asymmetry kept for "texture", in a family rendering 0/6
    monitor-mark   is-up and IS-FLAPPING painted identically — a hue swap, in a family rendering 1/2

Every one was invisible until something rendered the state, and none of them was a regression: they had been true since
the rules were written. That is the argument for the surfaces queue, stated three times in one week by three different
marks.

WHAT EACH SILHOUETTE MEANS is now written beside the rule (solid diamond = down, hollow diamond = unstable, solid circle
= back up) so the next edit does not re-collide them by taste.

AND THE PLUGIN DOT'S ERROR STATE IS REACHABLE AFTER ALL (round 42). Round 26 recorded that `plug-dot[data-state="ongoing"]`
hangs a sweep — playwright actually RUNNING starts the poll loop — and left `error` looking equally out of reach. It is
not: `error` comes from the card's Start/Stop POST FAILING, and a failed start never spawns a browser. `?pwstart=fail`
answers that POST with a **500** carrying the agent's own message, because `callApi` throws only on an HTTP error status
— a fixture answering 200 with `ok:false` would have rendered nothing and looked like a fixture that works. The sweep
clicks Start against it, and the surface carries both the error silhouette and the device's words.

ONE PUSH, THREE MEASUREMENTS, ON PURPOSE: the fix above, the alert-strip surfaces that found the collision, and the new
plugin-fail surface all ship together, so a single design job reports on all three. Round 41's lesson — a push cancels
the run that is measuring — is cheapest to obey by batching than by waiting.

### ONE DUPLICATE KEY, FIVE DEFECTS, TEN ROUNDS: THE MODELS PAGE (rounds 57-66)

The console's mark-coverage queue (round 50) named ten families; the worst was `prov-dot` — **2 states declared, ZERO
rendered in 56 surfaces**, which is worse than a missing state because every axis is blind to a family that never
appears. What it took to see it, in order, each layer hidden by the one above it:

  1. **the fixture key `/api/admin/public` was in the sweep's table TWICE** (round 59). In a JS object literal the LAST
     key wins, so the body added in round 57 never reached the page, `Models.tsx` set `failed`, and it rendered its
     "could not be read" banner — for three rounds, with every other check green.
  2. **the duplicate-key gate** (round 60) — written because of that, mutation-proven, wired into ci.yml.
  3. with the page rendering: **`lane-def` on screen and matched by nothing**, and **`span.prov-ident 72<161` at 320px**
     (round 61) — a page nothing had ever looked at, paying out immediately.
  4. the health fixture was keyed **`prefix:`** where the page reads **`h.id`** (round 64), so every lookup missed,
     `h?.ok !== false` was true, and `missing` could not render however many failing channels the fixture carried.
  5. the rule added in (3) painted the fallback lane with `--text-faint` — **2.54:1**, the token the console's own marks
     check already recorded as under the bar (round 64).
  6. and with BOTH states finally on screen together for the first time: **`prov-dot: ok and missing paint identically
     (50%/flat/-/halo)`** (round 65) — the **fourth** collision of this class, after `cmd-dot` bg/muted, `traj-ev-dot`
     muted/ok and `monitor-mark` is-up/is-flapping. Two of those four had been wrong since the day they were written.

CLOSED BY MEASUREMENT (round 66): the design job is green, `prov-dot` no longer appears in any note, and the two lane
findings are gone. The console's queue is down to its naming/sizing notes, which are statements about the probe rather
than about the page.

WHAT THIS COST AND WHAT IT BOUGHT: ten rounds for one dot, and five real defects — a page that rendered nothing, a class
with no rule, a 320px reflow overflow, an ink under 3:1, and a colour-only pair of states. Every one of them was
invisible for exactly as long as the surface did not exist, which is the argument the coverage queue makes in one line:
**a state with no surface is not an untested state, it is an unknown one.**

### THE SURFACES QUEUE IS EMPTY (round 81): TWENTY-FIVE ROUNDS, NINE DEFECTS, THREE INTERFACES

Measured, on a green design job: `plug-dot` appears in NO coverage note — all four of its states render, `ongoing`
included — and neither does `boot-mark`, whose two tones both render. The panel's queue is empty. The console's has been
empty since round 66. The landing's was empty by construction (it declares no state-mark families at all, verified).

WHAT THE THREAD COST AND BOUGHT, as one list, because the shape is the argument:

    the approval gate's disarmed ring     2.56:1, invisible, found by the LIVE probe (the only instrument that could)
    cmd-dot's bg                           NO RULE AT ALL — it computed as muted, two of six states identical
    the Path step tag for bg               black on black in dark (1.25:1), an invisible label
    traj-ev-dot muted                      1.42:1 invisible, and identical to ok
    the console's Models page              rendered NOTHING — a duplicate fixture key, three rounds to see
    lane-def                               on screen, matched by nothing; then its ink at 2.54:1
    models at 320px                        reflow overflow, the first time the page rendered at all
    prov-dot ok/missing                    painted identically — the FOURTH silhouette collision
    boot-mark info                         unreachable for seven rounds: six suspects eliminated, two wrong
                                           conclusions withdrawn, and the cause a query parsed in the wrong document

NINE DEFECTS, and every one was invisible for exactly as long as its surface did not exist. Three of the nine were
colour-only silhouette collisions, a class whose other instances had been wrong since the day they were written.

AND THE INSTRUMENTS THAT FOUND THEM ARE NOW LOAD-BEARING: the coverage queue runs on all three interfaces (rounds 50-51),
the harness serves a state per flag and re-reads its flags per request (round 73), the checks that guard the fixtures and
the derivation are seven gates in ci.yml, and the live probe measures the panel the device actually serves.

THE METHOD THAT CLOSED THE HARD ONES, stated because it beat inference three times: **ask the page.** Six eliminations
and two wrong conclusions got `boot-mark info` nowhere; one device probe that read the URL, the response and the DOM
produced the cause in a single call. The corollary is the mistake this session named: **an absence is not evidence until
the instrument is shown to see** — a truncated grep, a log without surface names, and a judge's output read instead of its
exit code were the same error in three costumes.

### THE GATE THAT PRODUCED ITS OWN COVERAGE ITEM (rounds 82-85)

`wire-field-check` compares the fields the panel's parsers read against everything the other end speaks, and on its FIRST
run it read 26 fields and found exactly one with no source: **`first_seq`** — which the device sends
(`agent/src/web/mod.rs:1030`) and no fixture carried, so `TrajectoryView`'s `.traj-trimmed` branch ("this trail was
TRIMMED", a fact only the device can state) had never rendered in any surface. Round 83 gave it one; round 85 measured the
result on a green design job: **surfaces 136 → 142, rows 7772 → 8266, zero findings**, the six new ones being the two
trimmed-trail tabs in both themes plus the plugin-running surface from round 77.

AND THE ROUND AFTER THAT REMOVED THE CLASS RATHER THAN THE INSTANCE. Two flags had each been fixed by hand for the same
reason — `boot` cost seven rounds, `pwrun` repeated it inside the round that added it — both because
`var P = new URLSearchParams(location.search)` was parsed ONCE, in a document that is not the one the assertion sees. The
parse is a function now (`P.get`/`P.has` re-read on every call), the two hand-fixed flags went back to the ordinary
spelling, and `harness-fixture-check` pins the re-parsing `P` itself: its mutation restores the once-parsed form and names
the fixture whose state stops rendering.

A gate that finds a defect on its first run and whose sibling then removes the whole class is the shape this session was
building toward: the measurement apparatus is not overhead next to the work, it is where the work comes from.

### THE LIVE PANEL, RE-MEASURED AFTER FORTY ROUNDS (round 86)

The live probe is the one instrument that measures the panel the DEVICE actually serves — the harness renders a stubbed
device, and every sweep in this suite points at that. It found the `ag-dot` ring at 2.56:1 in round 29, it shipped in
1.2.438, and it has not been run since. Re-run on d1 (1.2.438), both densities, exit 0:

    panel    61 rows (50 text, 11 graphics)  textFailing []  graphicFailing []  unmeasurable 0
             marks: mark[working] · sc-dot[ai] · ag-dot[off]   collisions []   ringFill []
    desktop  28 rows (17 text, 11 graphics)  textFailing []  graphicFailing []  unmeasurable 0
             same three families, same empty verdicts
    verdict  { ok: true }

WHY IT IS WORTH A ROUND TO SAY THIS PLAINLY: forty rounds of work went past this panel — a derivation unified, four
silhouette collisions fixed, a generated contract wired through, seven gates added — and the only way to know the OPERATOR's
surface is still clean is to measure the operator's surface. The suite's 142 harness surfaces and this one probe answer
different questions, and the probe is the one that answers "is what you are looking at true".

### THE LOCAL GATES, ALL THIRTY-NINE, AFTER A REFACTOR THAT REBUILT THE ARTIFACT THEY READ (round 102)

The objective's own bar says "the existing design gates on all four surfaces green", and this round was spent checking
that bar locally rather than assuming it — because the rounds just before it changed panel SOURCE and REBUILT the bundle,
and seven of these gates read the BUILT panel sheet rather than the sources:

    feedback-check · chrome-stillness-check · spacing-scale-check · state-colour-check · motion-check ·
    css-vars-check · mark-vocabulary-check

A refactor that moved the session-row mapping out of the hook body (`wireFields`, rounds 96-97) and regenerated
`panel.js`/`panel.css` is exactly the kind of change that can leave those seven reading a sheet nobody meant to change.

MEASURED: 39 of 39 green, each by its exit code, listed by name — 30 `.mjs` gates (the ten the standing objective added
among them) and 9 `.bash` gates. The panel suite is 806 in 103 files, the gateway 917/0, and CI green on the same commit.

WORTH A ROUND, WITH NO CODE CHANGED, because of what it rules out: a gate that reads an artifact is only as good as the
last time somebody ran it against the artifact the current sources build, and seven of them had not been run since the
rebuild.

### AUDITING THE TEN GATES, AND TWO FALSE ALARMS THAT WERE MINE (round 104)

This repository's standard is that no gate is ASSUMED to bite — each is proven by breaking the thing it guards. The ten the
standing objective added were each proven when written, but rounds 93-97 then refactored two of their subjects (the
gateway's stamping sites, and `useSessions`), so six were re-audited with their breaks planted again:

    session-row          a device field read outside the mapping section          rc=1  BITES
    one-derivation       a second ending→state mapping                            rc=1  BITES
    wire-field           a field the other end never speaks                       rc=1  BITES
    gateway-device-field the gateway forwards a field the device never spells      rc=1  BITES
    harness-fixture      …first attempt: a COMMENT appended to the P definition    rc=0  did not bite
    console-wire-field   …first attempt: `(st as any)?.last_boot_verdict`          rc=0  did not bite

**BOTH ALARMS WERE MY MUTATIONS, NOT THE GATES**, and each for a reason worth keeping:

  * `harness-fixture` pins the query as a re-parsing `P` by PATTERN, so appending `// noop` to that definition leaves the
    pattern matching and the gate correctly green. Re-planted as the real thing — `var P = new URLSearchParams(location.search)`
    — it fails with the fixture whose state stops rendering, rc=1. The gate is sound; my mutation was cosmetic.
  * `console-wire-field` matches `\w{1,8}\??\.field`, and `(st as any)?.last_boot_verdict` has `)` for a receiver, so the
    scan cannot see it. The plain `st?.last_boot_verdict` that round 101 proved the gate with bites, rc=1.

THIS IS THE FIFTH TIME THIS SESSION THAT A NON-BITING MUTATION WAS ABOUT THE MUTATION (AGENTS.md records the same lesson
from `.dot.err`: "a mutation that does not bite is evidence about the MUTATION first"). The operational consequence is
specific: **an audit must use the mutation the gate was PROVEN with, not an equivalent-looking one** — which is why
`harness-fixture-check` keeps its mutations IN the file, where they cannot drift from the predicate they are paired with,
and why the other five should eventually do the same.

### THE AUDIT IS NOW AN ARTIFACT: EIGHT GATES BROKEN ON PURPOSE, ON EVERY PUSH (round 105)

Round 104 re-audited six gates by hand and two came back "DOES NOT BITE" — both because of MY mutations, not the gates.
The lesson was written down; this round made it structural. `scripts/test/gate-mutations-check.mjs` holds eight mutations in
ONE place, each paired with the gate it must fail, the file it edits and the reason, and it runs in CI:

    ok  session-row          a device field read outside the mapping section
    ok  one-derivation       a second mapping from an ending to a state
    ok  wire-field           a field the device harness and every fixture never speak
    ok  console-wire-field   a console field nobody produces (the round-100 shape, in the spelling the scan can see)
    ok  gateway-device-field the gateway forwards a field the device never spells
    ok  device-verdict       the console stops reading the device's own verdict
    ok  harness-fixture      the query is parsed ONCE again
    ok  contract-vocabulary  one artifact's vocabulary diverges from the other's
    → 8 broken on purpose, every one bit

THREE PROPERTIES MAKE IT SAFE TO RUN UNATTENDED, and each was a decision rather than a default: it REFUSES to start on a
dirty tree, so a mutation can never be mistaken for somebody's work; it restores every file in a `finally` and re-asserts
the tree is clean when it finishes; and a case whose recorded mutation no longer matches its file is a FINDING rather than a
skip, because that is exactly how the mutation and the predicate it is paired with drift apart.

AND IT FOUND A BUG IN ITSELF ON THE FIRST RUN, which is the same lesson one level up: the harness emitter exits **2 on
success** ("rc=2 (2=ok)", the convention every caller of it prints), so reading its exit code as pass/fail killed the audit
on the single case that has to re-emit. The wrapper accepts 2 now.

ROUND 106 EXTENDED IT TO TEN — the declared-hosts list (planting the deployment's host back into the file that just came
off it, which is the regression a shrinking list can suffer) and `build-pins` (deleting a gate's invocation from ci.yml) —
and running it found TWO MORE bugs of mine, both in the audit rather than in any gate:

  * a placeholder object in the CASE list was an entry the loop cannot run (no file), and it crashed the run;
  * every `gate` was fed to `node`, so `build-pins.bash` came back red BEFORE any mutation and the audit printed "the tree
    must be green" — a true sentence about a false premise. The runner follows the file's extension now.

TEN OF TEN BITE. And the shape of this round is worth naming: an audit that runs on every push keeps finding bugs — in
itself, so far — because it is the only artifact in the suite whose subject is the OTHER artifacts.

ROUND 107 PAID THE ONE DEBT IT RECORDED, AND THE DEBT WAS IMAGINARY. `sweep-fixture-dupes-check` was left out because its
mutation "needs a two-key shape this list cannot express" — and the anchor that made it look that way was the multi-line
`/api/health` block. A fixture key that sits on ONE line (`'/api/version': { version: '1.0.106' },`) duplicates in place with
a plain from/to replacement, and the gate bites:

    ok  sweep-fixture-dupes  a sweep's fixture table answers one endpoint TWICE,
                             so the last key silently wins and a page renders what nobody meant it to
    → 11 broken on purpose, every one bit

That is round 104's lesson one step EARLIER in the process. There, a mutation that did not bite was aimed at the wrong
text; here, the wrong text made me conclude the TOOL could not express a mutation it expresses perfectly. Both are the same
act — deciding something about an artefact without reading it — and both cost a round.

ROUND 108 TIGHTENED THE RULE AND ROUND 109 PROVED IT. `wire-field-check` used to accept a field that "the harness or a
fixture speaks", which is the TEST side — and a field that exists only there is exactly the `prov-dot`/`verdict:` class this
session paid for twice. It now requires the name in something that SENDS it (the agent's Rust or the gateway), and measured:
all 26 fields the panel's five parsers read have producers, so none of them renders only in a stub.

That new branch was owed one round of proof rather than assumed, for a stated reason: exercising it needs a field planted in
a hook AND in the harness while absent from every Rust and TypeScript source, and the audit's mutations edited one file
each. Round 109 gave a case an optional `also` list — applied with the primary, restored with it in the same `finally` —
which cost four lines of runner and closed the debt:

    ok  wire-field  a field a fixture carries and NO producer sends — it renders in the harness
                    and would be undefined on a device
    → 12 gate/branch pairs broken on purpose, every one bit

### THE WIRE-FIELD RULE REACHES THE PRODUCER ON BOTH FRONT ENDS (rounds 108, 122)

`wire-field-check` (the panel) accepted a field that "the harness or a fixture speaks" until round 108, when the rule was
tightened to require a PRODUCER — the agent's Rust or the gateway. A field that exists only in a stub is the `prov-dot` /
`verdict:` class this objective paid for twice, and after the tightening all 26 fields the panel's five parsers read are
spelled by something that SENDS them.

Round 122 did the same to the console, whose version accepted "the gateway or a fixture" — and its fixtures are the render
smokes and the console sweep, which is exactly where a field can look real and answer `undefined` in production. Measured
after: all 7 fields the console's twenty modules read come from `gateway/src` itself.

AND THE TIGHTENING IS PROVEN, which the old mutation could not do: the audit's original console case planted a field
NOTHING carries, so both the loose and the strict rule caught it. The new case plants one in the SWEEP's `/api/devices`
payload and reads it in `DevicesPanel` — the fixture carries it, no producer does — and that case passes under the old rule
and fails under this one. It needed the audit's multi-file `also`, added in round 109 for exactly this kind of debt.

    → 13 gate/branch pairs broken on purpose, every one bit

### THE PRODUCER RULE, THREE ENDS, AND A FULL RE-VERIFICATION (round 124)

ONE RULE, at every layer where a name crosses: **a field an interface reads must be spelled by something that SENDS it.**
Three gates hold it now, each with a mutation that must fail it:

    wire-field-check             the panel's 26 fields   → the agent's Rust or the gateway    (rounds 82, 108)
    console-wire-field-check     the console's 7 fields  → the gateway ITSELF, not a fixture  (rounds 100, 122)
    gateway-device-field-check   the gateway's 5 fields  → the agent's Rust or a fixture      (rounds 101, 123)

TWO OF THE THREE WERE TIGHTENED AFTER BEING WRITTEN, and in both cases the tightening needed a NEW mutation to be provable:
the original cases planted fields NOTHING carried, which the loose and the strict rule both catch. The cases that prove the
tightening plant a field a FIXTURE carries and no producer does — the exact defect the rule exists for (a page that renders
in a stub and answers `undefined` in production).

AND A FULL RE-VERIFICATION AFTER THE WORK, because rounds 103-123 changed eleven gateway test files and four gates: every
local gate re-run by exit code — NONE RED — and the audit reporting THIRTEEN gate/branch pairs broken on purpose, every one
bit. The suites: panel 806 in 103 files, gateway 917/0.

The one mistake in that stretch was caught by the audit alone: a careless edit removed two lines from
`gateway-device-field-check` and pushed a commit where it threw `ReferenceError` on every run. The pre-commit hook guards
the emitted scripts' template literals, not that file's syntax; CI had not run yet; the audit refused to start on a dirty
tree and then reported "1 gate(s) unproven". **An artifact whose subject is the other artifacts is the only one that notices
when an artifact stops working.**

### THE FAILURE FACT, TRACED END TO END (round 125): ONE SOURCE, ONE DERIVATION, THREE CONSUMERS

The device learned to report `last_exit_code` per session in round 96, and the objective's spine says a surface may PROJECT
that fact and never recompute it. Traced, not assumed:

    device row            last_exit_code
      → wireFields        the ONE place a device field is read in useSessions.ts   (guarded by session-row-check)
      → sessionFailed     the ONE derivation, with its rule written down:
                          "ABSENT IS NOT FAILURE and not success: null means the device did not say"
      → sessionLiveness   composes it with closed / pendingApproval / idleMs
      → THREE consumers   TabBar · ContextRail · DesktopShell — all read `data-live`, none recompute

and the tests pin the whole matrix that rule implies: absent, `0`, `130`, closed, approval-pending, idle. The fifth
silhouette the objective asked about is not open: `failed` is one of the FIVE panel states `mark-vocabulary-check` has held
since round 97, deliberately NOT the diamond the console spends on failure, because the panel spends that on a QUESTION.

SO THIS LINE IS VERIFIED RATHER THAN CHANGED, which is the honest outcome for a round that goes looking for a second
derivation and finds none: the check existed, the rule was written at the point of derivation, and the three surfaces
consume one value. Nothing to fix; one thing to record, because "we looked and it is clean" is a fact the next round should
not have to re-establish.

### WHICH GATES READ COMMENTS, AND WHICH DO SO ON PURPOSE (round 136)

Round 135 found that all three field gates matched raw text, so a COMMENT could satisfy them — measured with a field carried
by the harness, read by a hook, and mentioned in one `//` line of a Rust source: rc=0. That direction is a FALSE NEGATIVE
(a requirement met by prose), and it is the dangerous one. This round measured the same property across the rest:

    gate                        strips comments?   the direction of the risk, and the decision
    wire-field family           YES (decomment)   was a false negative — fixed in round 135
    console-wire-field family   YES (decomment)   same
    gateway-device-field        YES (decomment)   same
    one-derivation (endings)    NOW               a comment quoting the pattern was a FALSE POSITIVE; fixed here
    one-derivation (mark states) already did      the clause was written with the skip
    session-row                 no                flags a comment as an offender (noise) and can pad its own floor.
                                                   Low risk in both directions; left, and recorded rather than assumed
    production-host             NO, ON PURPOSE   a comment that spells the deployment's host is EXACTLY what this gate
                                                   should flag: the rule applies to the record about the rule. Proven on
                                                   this session's own inventory, which the gate refused until its notes
                                                   stopped quoting the hosts
    contract-vocabulary         no                counts occurrences in generated artifacts and Rust; a comment there
                                                   would inflate a count rather than satisfy a check

The lesson is the same one this objective keeps re-learning one layer down: a rule that matches TEXT is not yet a rule about
the thing the text describes, and the fix is to decide, per gate and in writing, which direction of error is tolerable.

### THE THREE DISCIPLINES FOR A GATE THAT MATCHES TEXT (rounds 104-139, closing note)

Sixteen gate/branch pairs are broken on purpose on every push, and six of them match SOURCE TEXT rather than running
anything. Those six cost this stretch four rounds of correction, and the corrections reduce to three rules. They are worth
stating together because each was learned by the gate being wrong in a way nobody would have predicted from its name:

  1. **A MATCHING RULE IS NOT YET A RULE ABOUT THE THING IT MATCHES.** `gateway-device-field-check` widened to every
     gateway source reported 35 defects; reading them showed the vocabulary of the UPSTREAM LLM PROVIDERS (`media_type`,
     `prompt_tokens`, `cache_read_input_tokens`), not of the agent. `one-derivation`'s first run and the mark clause's first
     run did the same thing with different words. A broad pattern does not become a rule by matching more — and the fix each
     time was to keep the rule, declare the exception, and write the reason down.
  2. **STRUCTURE, NOT PRESENCE.** `device-verdict-check` proved the console READS the device's verdict and could not see
     whether its own comparison was still GUARDED by it; hoisting the comparison passed every pattern while making the
     console answer for devices it cannot speak about. What a gate asserts must be the shape it cares about, not a word that
     happens to appear.
  3. **A COMMENT IS NOT A PRODUCER, AND WHICH DIRECTION THAT ERRORS IS A DECISION.** For the field gates a comment that
     satisfied them was a FALSE NEGATIVE — a deleted producer kept alive by prose, measured with rc=0 — while for the
     derivation gate a comment quoting the pattern was a FALSE POSITIVE. `production-host-check` refuses the strip on
     purpose, because a comment spelling the deployment's host is exactly what it must flag; that is not a theory, it is what
     this session's own inventory note was refused for. `scripts/test/lib/decomment.mjs` is the one definition, and each gate
     says which direction it tolerates.

AND THE GATES FELL FOUL OF THE OBJECTIVE'S OWN RULE ONCE: rounds 135 and 136 each wrote that strip into the gate that needed
it, making three copies of one rule — the defect this objective removes — until round 137 made it a module. The gates are
subject to the spine too.

### TWO MEASUREMENTS THAT FOUND NOTHING, RECORDED SO THEY ARE NOT REPEATED (round 141)

  1. **"The documentation claims a consumer that does not exist"** was worth a scan after round 140 found one — a docstring in
     `Models.tsx` asserting its lane mapping was "the same mapping the Routes page uses", with no second copy anywhere. A
     grep of both front ends for comments claiming another file shares a mapping / vocabulary / rule / list returns ten
     hits, and NONE is that defect: nine explain a rule by analogy ("the same shape as ADOPT losing…"), and the tenth is the
     comment round 140 corrected. The family is one instance wide, and it is fixed.
  2. **The provider dot's states**: the sheet declares exactly two (`.prov-dot.ok`, `.prov-dot.missing`) plus the base, and
     `Models.tsx` emits exactly those two (`ready ? " ok" : " missing"`). Aligned — and the sheet's own comment records the
     round that made them distinguishable, which is why this took one grep to confirm rather than a rendered sweep.

Both are the shape of result this objective should produce more of: a question worth asking, answered in two commands, and
written down so the answer survives the round that asked it.

### THE LAST CLAUSE I HAD NOT VERIFIED MYSELF: ONE FOCAL POINT PER SURFACE (round 142)

Every other clause of this objective has been measured in front of me at some point this session — contrast, silhouette
coverage, transition budgets, idle repaint, reduced motion, the wire contract. "One focal point per surface" had not, so I
went looking for its instrument rather than assuming it had none. It has one, and it is older than the objective:

    design-sweep.mjs 184   the loudness probe: every element's saturation and lightness, reported per surface (max 6)
    panel-design-sweep.mjs 1288   "The loud axis exists to catch a page with two competing FOCAL POINTS"

and it carries what a good axis carries: a MEASURED exemption rather than a tolerance — the rail pages legitimately show a
second loud element because navigation is not a competing focal point, and the comment records the reading that settled it
("light is 0 loud on six of six pages in both densities"). The axis is judged too: `panel-design-sweep.bash` plants a defect
for it among the others.

MY OWN ATTEMPT TO MEASURE THE DISTRIBUTION FAILED, for a reason worth one line: I grepped the stored CI logs for `"loud"` and
found nothing, because the field is not spelled that way in the emitted rows. The instrument was there all along; the record
of this round is that I checked instead of asserting, and that the check cost one grep once I asked the right file.

THAT CLOSES §12's TABLE: every clause of the objective now has an instrument, a gate, or both, and the two that had only a
measurement (this one and the live panel) also have a verdict.

### OTHER CONFIGURATIONS NOBODY RUNS: A SCAN, AND FOUR NEGATIVE RESULTS (round 147)

Round 146 found that the agent's DEFAULT feature config had not compiled for a long time, because CI only ever built one
configuration. That is a class, not an instance, so this round asked where else a configuration is never exercised. Four
candidates, measured:

    cargo check -p vale-agent --features terminal          rc=0   green
    cargo check -p vale-agent --features keyring           rc=0   green
    cargo check -p vale-agent --features terminal,keyring  rc=0   green (and CI runs test+clippy on it)
    cargo check -p vale-agent                              rc=0   green — was rc=101 until round 146 fixed the stub

    release.yml          no cargo test/clippy/build step at all; it goes through scripts/build.sh, so there is no
                         second configuration to keep green
    panel desktop mode   NOT a build: `desktop` is a runtime density the harness serves by URL, and package.json has no
                         desktop script. Nothing to keep green

So the stub-versus-real split exists in exactly ONE configuration — the no-feature one — and that one is now built by CI.

THE METHOD IS THE PART THAT GENERALISES, and it is the same one that found the defect: for each configuration, ask what
would have to be true for a break to be invisible, then run it. The no-feature build was invisible because CI ran only the
full one; the single-feature builds were equally invisible and happen to be fine; the release pipeline has no cargo step to
be invisible in; and the desktop density is not a configuration at all.

### HOW THIS PROJECT SERVES A SECOND CONFIGURATION, AND WHY ONLY ONE HALF NEEDED A GATE (round 149)

Round 148 gated the stub's surface. The obvious next question is whether other feature splits hide the same hole, so this
round looked for them — and the answer is that `terminal` is served by TWO mechanisms, with very different safety:

  1. **a STUB MODULE** (`tools/terminal/stub.rs`) — the whole backend, in one file, chosen by `#[cfg]` at the module. Its
     surface is what round 148's gate now compares, because a hole in it compiles in one configuration and fails in the
     other. `term_note_exit_code` was exactly that.
  2. **per-call-site fallbacks** — `#[cfg(not(feature = "terminal"))]` appears in `connections.rs`, `files.rs` and `mod.rs`,
     and each body is an honest "nothing here": `Err(DeviceError::Internal { message: "terminal support not compiled
     in" })`, `Ok(tool_error(…))`, or an empty list. **These are compiled in the DEFAULT configuration**, so a fallback that
     disagrees with the real path is a TYPE ERROR, not a silent hole — the strongest check there is, and the reason they need
     no gate.

So the general rule this leaves behind: a second configuration is safe where the compiler sees BOTH sides of the split, and
needs a gate exactly where it does not — which is a module-level `#[cfg]` choosing a whole file, because no single
compilation unit ever contains both.

### ROUND 150: THE WHOLE SYSTEM, MEASURED THE WAY CI MEASURES IT

A milestone is a good place to answer the objective's last clause as a whole rather than clause by clause, so this round ran
every command CI runs, in the working directory CI runs it from, and read each exit code:

    panel   npm run build rc=0 · npm test rc=0 (806 in 103 files)
    gateway npm run typecheck rc=0 · npm test rc=0 (917 pass / 0 fail) · npm run lint rc=0 · npm run format:check rc=0
    ui      npm run build rc=0 · npm test rc=0
    agent   cargo fmt --check rc=0 · clippy (default) rc=0 · clippy (terminal,keyring) rc=0 ·
            test (default) rc=0 · test (terminal,keyring) rc=0, 13 test binaries ok

    40 local gates run by exit code: 0 red
    gate-mutations: 17 gate/branch pairs broken on purpose, every one bit

TWO THINGS THIS NUMBER DOES NOT SAY, and both are the point of the rounds that produced it. First, "green" here means green in
the configurations CI builds — which is only true since round 146 added the agent's DEFAULT config to CI, because before that
one of the four ends had a configuration nobody built and it was broken. Second, the 17 mutations matter more than the 40
gates: a gate that cannot fail is worse than no gate, and every one of those 17 is a defect this session actually met.

### A NUMBER IN PROSE: THREE TREATMENTS, AND HOW TO PICK (rounds 151-154)

Three rounds in a row caught a COUNT that had drifted, and the fixes were not the same fix. Written together because the
choice between them is the whole content of the rule:

  1. **MAKE IT CHECKABLE.** AGENTS.md carried a table of "the exact commands, by working directory", and it said the agent's
     checks run in "both feature sets" while the workflow ran one — the missing configuration was broken at the time.
     `ci-command-table-check.mjs` now compares the two in BOTH directions, and its own first run found three disagreements.
  2. **REMOVE IT.** The paragraph over the gate table said "TEN GATES"; a round later there were twelve.
     `numbered-claims-check.mjs` holds the checkable half (every wired gate is a NAMED gate) and the number is gone. The
     same treatment went to the migration note's "446 in 105; the declared list 52 → 42", which the computing gate printed
     as 374 in 102 and 41 — off by three files and one file, one round after it was written.
  3. **DATE IT AND WARN.** This one was already in the file, and finding it is what turned two options into three: the
     sweep's cell says "79 checks **as measured on 2026-09-21** — the axis list below is the part that stays accurate, the
     NUMBER is what drifts", and the animation and mark counts sit inside "Measured 2026-09-18". A measurement of a MOMENT
     is honest when it says which moment; a claim about NOW is not, because nothing recomputes it.

HOW TO PICK, and this is the sentence to keep: **is the number a claim about the present, or a record of a measurement?** A
claim about the present gets treatment 1 or 2 — because a reader will act on it — and a record of a measurement gets 3. The
failure mode is a number that LOOKS like the second and is read as the first, which is exactly what "TEN GATES" was.

### TWO BOOT KINDS HAVE ONE READER, AND THAT IS THE GOOD SHAPE (round 158)

Round 157's measurement showed three boot kinds read by the panel AND the gateway/console (`clean-exit`, `replaced`,
`crashed`) and two read by the panel alone (`first-run`, `machine-restart`). This round asked whether that is a defect or a
decision, and looked at the code that decides.

    gateway/src/plugins/mcp.ts:114   if (j.last_boot_kind === "crashed" && …)   → the crash row's payload
    gateway/src/plugins/mcp.ts:194   last_boot_kind: probe.lastBootKind          → forwarded as a plain string

It is a POSITIVE TEST for the one value the console acts on, not a WHITELIST of values that may pass. The difference matters
for exactly the reason this objective keeps meeting: a whitelist would silently drop a boot kind added later, so the next
vocabulary value would never reach the console and nothing would say so; a positive test lets it through and the console
declines to render it, which is the deliberate rule that page records ("ONLY A CRASH GETS A ROW", round 33).

So both single-reader values are DECISIONS rather than gaps, and the asymmetry is the design: the panel is the surface that
says "this device just started for the first time" or "the machine restarted", and the fleet view is for exceptions.

RECORDED RATHER THAN FIXED, which is the whole point of the round: the next reader who notices "two values have only one
reader" will find this paragraph and the two line numbers instead of a defect to repair.

### LOOKING FOR A FACT THE INTERFACE WANTS AND THE DEVICE DOES NOT REPORT (round 159)

`first_seq` was the shape worth hunting: the panel said a trail was TRIMMED only after the device learned to report where the
survivors begin. The same question asked of the panel's time-based logic:

    grep -E "Date\.now\(\) *- *[a-zA-Z_.]+ *[<>]"   →  nothing
    the thresholds that do exist:  bootNotice.ts:69   REPLACED_NOTICE_SECS = 300
                                   liveness.ts        (a quiet-session threshold, over the device's idle_ms)

Both are PRESENTATION WINDOWS over facts the device already states — `uptime_secs` and `idle_ms` — and that distinction is
the point: the device reports the fact, the surface decides how long to keep saying it, and a threshold is the right shape
for the second job and the wrong shape for the first. Nothing wants a fact the device is not reporting.

A NEGATIVE RESULT, recorded with the two commands, because this is the file a future round reads before hunting the same
thing. The `first_seq` case was found by a GATE (`wire-field-check`, round 82) rather than by looking, which is the argument
for the gates over the hunting: this round cost two commands and found nothing, and the one that mattered was found by a
program that never gets bored.

### THE TOOL-REACHABILITY QUESTION, MEASURED, AND WHY IT IS NOT YET A FINDING (round 160)

AGENTS.md carries a rule for a new MCP tool: it "must be registered in `gateway/src/mcp-tools.ts` AND matched by
`isDeviceDirectTool()`". Asked mechanically — the device's own list is `agent/spec-tools.json` (JSONC, comment-headed like the
vocabulary artifact) — the numbers are:

    56 device tools
    19 of them are NOT NAMED in mcp-tools.ts   (mcp_client_*, memory_*, system_*, terminal_secret_*)
    49 of them are NOT NAMED in mcp.ts         (the 7 that are named look like the explicit exceptions)

THE SECOND NUMBER IS AN ARTEFACT OF THE METHOD: `isDeviceDirectTool()` matches by PREFIX for whole families
(`terminal_*` and friends), so "not named" says nothing about reachability. And the first number cannot be read as a defect
either, because a tool may be deliberately absent — `mcp_client_*` would invite recursion, `memory_*` is the device's own
store — and NOTHING IN THE REPOSITORY STATES WHICH IS WHICH.

So this is a measurement without a verdict, recorded rather than dressed up. The checkable form it points at is the pattern
this repository uses everywhere else: a DECLARED list of the tools that are deliberately not console-reachable, each with its
reason, so that "absent" becomes a decision and a forgotten registration becomes a failure. That is a round of its own, and
it is written here rather than started at the end of one.

### THE MONITOR FAMILY, TRACED TO THE END (round 162)

Round 126 unified the monitor MARK's state (two components, three spellings); this round asked the same question of the rest
of the family, and the answer is that the duplicate was the only one:

    useMonitors.ts:268-283   the ALERTS list is built here — one hook, one place, expiring on its own
    MonitorAlerts.tsx        consumes it and renders; it computes nothing
    MonitorChip.tsx          consumes the down/flapping lists; same
    MonitorChip / Alerts     both call `monitorMarkClass` for the mark (round 126, and the container's `monitorModifier`)

So the family's facts — up/down/flapping, the alert list, and the mark's vocabulary — each have one home, and the one
duplicate was found by tracing rather than by reading the sheet.

RECORDED AS A VERIFICATION, which is what this round is: nothing changed, and a future round that wonders whether the monitor
family has a second derivation can read this instead of re-tracing it. Three families have now been walked this way
(session liveness in round 125, the monitor mark in 126, this) and each time the walking found either one duplicate or none —
which is itself the argument for walking them rather than assuming.

### THE FAMILIES WALKED FOR A SECOND DERIVATION, AND WHAT EACH ONE FOUND (rounds 125-163)

The spine's first clause is "no surface computing its own version of the same fact", and the way this session checked it was
to WALK a family: find the fact, follow it to every consumer, and look for a second computation. Five families so far, and
the index is here so the sixth round does not re-walk the first five:

| family | the fact | what the walk found |
|---|---|---|
| session liveness (r125) | `last_exit_code` → failed / idle / off | one read (`wireFields`), one derivation (`sessionFailed`), three consumers |
| the monitor MARK (r126) | up / down / flapping | TWO components, THREE spellings — unified into `monitorMarkClass` |
| the monitor family (r162) | the alerts list | one hook (`useMonitors.ts:268-283`), two consumers, nothing computed twice |
| the channel's health (r132) | ok / err per provider channel | ONE row rendered the same answer three times — unified into `channelSignal` + `channelLabel` |
| the dial's tone (r132) | some-but-not-all healthy | two tiles answering one question differently — unified into `healthTone` |
| the run family (r163) | the device's `run_begin`/`run_end` grouping | one module (`lib/runs.ts`), ONE consumer (`RunStrip.tsx`); a `"running"` comparison elsewhere belongs to the CONNECTION PROBE, a different fact |
| a control's acknowledgement (r186) | is this control busy, and did it answer? | ONE hook, NINE surfaces: `lib/useAck.ts` gives `{ busy, ack, run }` to GoalBar, SessionControl, MemoryPage, MonitorsCard, PathView, NotificationsCard, SettingsPage, ApprovalGate and ConnModal, with `ackMechanism.test.ts` beside it and the rendered ack pass timing it against the stated 100 ms budget (`via=disabled`, 4-9 ms on the device). This is the POSITIVE model for the objective's feedback clause — one mechanism, nine consumers, and a measurement of its latency rather than a claim |
| the trajectory's event dot (r176) | a `CommandEvent`'s state | NOT a duplicate any more, and its own doc records why: "This used to be a private copy, and the copy is where the defect lived". `eventDotState` is an ADAPTER over `stateFromEnd` + `terminalStatus` — the same shape as `cardState` — so the family that once wore two states for one backgrounded command now has one derivation and two adapters |
| the update verdict (r174) | is this device behind? | NOT parsed in the console at all: the DEVICE answers it, and the console's own string comparison survives only as a guarded fallback — which `device-verdict-check`'s structural clause holds in place (round 134). Searched for a version parser (`split(".")`, `localeCompare`, semver) and found none: the three hits are a JWT split, an access token and a hostname's second label |
| the activity timeline (r166) | each record's source / kind / session / seq / exit | the SAME module — `rowFromEvent` builds a row, and its doc says it in as many words: "Every field is the device's own value or `null`; nothing here is inferred from a neighbouring record". The two `kind` comparisons live in one `kindNote`, which is presentation over the row's own field rather than a second computation of a wire fact |

THE PATTERN WORTH KEEPING: three of the six found a real duplicate and three found none, and the ones that found something
found it in a place nobody would have guessed from the sheet — two components spelling one state three ways, one row
computing one answer three times, two tiles disagreeing about what "some are well" means. Walking is cheap (a grep and a
read); assuming is what costs rounds, in both directions.

### DO THE GATES' FAILURE MESSAGES TELL A READER WHAT TO DO? MEASURED, AND THE MEASUREMENT WAS THE PROBLEM (round 164)

The question is worth asking — a gate whose failure says only "FAILED" makes its reader open the file to learn what it wanted —
so it was measured: 34 gates, of which 10 have a message containing an actionable verb, 7 have no `console.error` at all, and
17 fail "without an instruction".

THE NUMBERS ARE THE MEASUREMENT'S FAULT, not the gates'. Reading three by eye:

  * `retired-colours-check` prints "FAILED — a value that was replaced for a measured reason is back:" and then, per hit,
    "retired because …". It tells the reader WHY it is wrong, which is what makes the next step obvious without an imperative.
  * `spacing-scale-check` has a terse header ("spacing scale: FAILED") and rows that carry the story ("off-scale spacing rose
    from 305 to 306"), which names both what changed and what is expected.
  * `stub-surface-check` — which my scan also listed — ends "Mirror it, or declare it here with the reason it cannot be".

The regex looked for imperative verbs in `console.error(...)` calls, and it missed multi-line templates, nested parentheses and
messages that carry a REASON instead of an instruction. That is the third time this session a text heuristic was mistaken for
a rule (the duplicate-key scan in round 116 reported 947 false positives; the gateway widening in round 123 reported 35), and
this time the count was checked by eye BEFORE anything was changed.

WHAT IS LEFT, honestly: the message QUALITY of the gates is good where it matters and no gate says only "FAILED" — and if a
round wants to improve one, the shape to aim for is `retired-colours-check`'s: say what is wrong, and why it is wrong, which
is more useful than an imperative verb.

### FIVE TIMES A TEXT CHECK MISREAD THE ARTEFACT, AND THE RULE THAT COMES OUT OF IT (round 165)

The question was whether each scanning gate states its own ERROR MODE where the next reader of that code will find it. The
grep for phrases like "false positive" reported `console-wire-field-check` at ZERO, so the file was opened — and it states
both modes plainly, with the measurement that established one of them ("passed `wire-field-check` with rc=0"). All five
scanning gates state theirs. The grep was wrong, for the fifth time this session:

    116  the duplicate-key heuristic          947 "candidates", essentially all two different objects at one indent
    123  the gateway widening                 35 "defects", all the upstream PROVIDERS' vocabulary
    135  a comment satisfying a field gate    measured right, but only because the plant was built by hand
    164  the gates' failure messages          17 "without an instruction", incl. ones ending "Mirror it, or declare it"
    165  this                                  0 hits for a file whose comment says it in as many words

Each time, the correction was to READ THE ARTEFACT, and each time the grep had looked plausible. So the rule, stated as the
thing to do rather than the thing to avoid: **a text scan is a way to CHOOSE WHAT TO READ, never a way to reach a verdict.**
Every one of the five was cheap to resolve that way — one `grep -B2 -A6` in this round — and every one would have been
expensive to resolve by acting on the count.

AND THE VERIFICATION ITSELF IS RECORDED: five scanning gates, five statements of their error mode, each written by the round
that was bitten by it. Nothing changed this round, which is what makes it worth a paragraph: the next reader who wonders
whether these gates document their failure modes has an answer instead of a grep.

### FOUR DATA ATTRIBUTES NOBODY READ, PRUNED — AND A SCAN THAT WAS WRONG TWICE FIRST (round 167)

The objective's last clause is "whatever stops earning its place pruned", so the panel's `data-*` attributes were asked the
same question the marks were: does anything READ you? 32 attributes are set; four are read by neither the built sheet, nor the
TypeScript, nor the tests, nor the sweeps or the render harness:

    data-drops    MonitorChip's flapping chip — its `title` already carries the drops and the window
    data-down     MonitorChip's down chip — the parent's class already says which state it is
    data-ready    EmbeddedBrowserPane — the `ready` flag drives five other things in that component
    data-crashes  RestartHistoryCard — the count is still read twice (the class and the label)

All four pruned, panel 806 green, tsc silent, bundle rebuilt, and the fields they were carrying are still read elsewhere, which
was checked rather than assumed.

THE SCAN WAS WRONG TWICE BEFORE IT WAS RIGHT, and both corrections are the round-165 rule in action. Its first version read
only `src/**/*.tsx` and `*.ts` — so `data-testid`-style readers in `*.test.tsx` and every probe in `agent/scripts` were
invisible, and 17 attributes looked dead. Its second version added the tests and still missed the sweeps. Only then was the
list read by hand, four at a time, which is what the rule says a scan is for.

### THE TWO FRONT ENDS SPELL STATE DIFFERENTLY, AND THAT EXPLAINS TWO GATES (round 168)

Round 167 pruned four unread `data-*` attributes from the panel. The same ruler on the console returns ZERO — and reading the
artefact, as the round-165 rule requires, shows why: the console's TSX contains `data-` exactly ONCE, in a comment about
`body[data-theme]`, which a theme script sets before the first paint. Its state marks are spelled with CLASSES

    Overview.tsx:338   <span className={`dot ${channelSignal(c.ok)}`} />        (round 131)
    Overview.tsx:341   <span className={`health-state ${channelSignal(c.ok)}`}>

So the two front ends answer "which state is this mark in?" with different mechanisms — the panel puts a `data-state` (and
friends) on the element and styles it by attribute, the console writes the state INTO the class list — and both are internally
consistent, which is why neither is a defect and why this is recorded rather than unified.

IT ALSO EXPLAINS AN ASYMMETRY THAT LOOKED ARBITRARY: `mark-vocabulary-check` reads the panel's vocabulary as DATA (a module)
while `console-marks-check` reads the console's as PAINTED (its sheet, matching class rules). That difference is not a
preference — it is the only way to read each end, because one keeps its vocabulary in TypeScript and the other in CSS class
names. Two gates that looked like they should be one had a reason, and now the reason is written down.

### SOMETHING WRITTEN THAT NOBODY READS: FOUR ROUNDS, FOUR OUTCOMES, AND THE ORDER THAT PRODUCES THEM (rounds 167-171)

The objective's last clause is "whatever stops earning its place pruned", and the question it implies is mechanical: this
attribute, this class — does anything READ it? Four rounds asked it of four targets, and the answers were not the same answer:

    167  the panel's data-* attributes        FOUR pruned   (data-drops, data-down, data-ready, data-crashes)
    168  the console's data-* attributes      NONE          — it does not use them at all; its marks put the state in the
                                                            CLASS list, which is also why `console-marks-check` reads a
                                                            sheet while `mark-vocabulary-check` reads a module
    169  the panel's class names              ONE pruned    (cmd-copy, while .cmd-btn carries the control)
    170  the console's class names            ONE KEPT AND RENAMED (topbar-avatar → user-pop-avatar)

THE ORDER THAT PRODUCES THESE, and it is the round-165 rule applied to a new subject:

  1. **FIND EVERY READER FIRST**, and widen the scope until the count stops moving. Both scans were wrong before they were
     right — twice for the panel's classes (fragments of template literals; readers in `*.test.tsx` and under `agent/scripts`
     invisible) and once here — and the wrong versions reported 17 and 39.
  2. **READ THE ARTEFACT BEFORE JUDGING.** In 167 that showed the fields were still read elsewhere; in 168 it explained a
     zero that looked like a bug; in 170 it changed the verdict twice, from "unstyled control" to "a slot the parent lays out,
     whose name is the only thing wrong".
  3. **THEN PRUNE, KEEP, OR RENAME** — and a KEEP is a result too, which is why the four outcomes differ. `user-pop-avatar`
     keeps its place because an avatar slot is a name worth having; `cmd-copy` loses its because `.cmd-btn` already names the
     control and the extra hook changed nothing.

VERIFIED AFTER: 42 local gates by exit code, none red, and the audit reporting 21 gate/branch pairs breaking on purpose and
every one biting.

### THE CONSOLE'S DERIVATION GATE, AND ITS FIRST DAY (round 173)

`console-derivation-check.mjs` is the console's side of "one fact, one derivation" — the panel has had its version since
round 78, the console had nothing, and round 172 found the prefix rule written nine times with two behaviours. Its first
clause: a trailing-slash strip outside `lib/lane.ts` is a second derivation, by file and line.

IT WAS CAUGHT TWICE ON THE DAY IT WAS ADDED, both times by gates this objective wrote:

  * `numbered-claims-check` (round 152) refused it: "console-derivation-check — invoked by the workflow, absent from AGENTS.md,
    the ledger and the inventory". A gate whose job is to notice an unnamed gate noticed its own successor, which is the
    clearest demonstration that family has had.
  * the audit reported it UNPROVEN, because the mutation's `to` string was over-escaped and produced `\\/` — a regex matching a
    literal backslash — rather than the hand-written strip the gate looks for. A mutation that does not bite is evidence about
    the mutation first (round 104), and this is the sixth time.

### A CONSOLE SOURCE CHANGE OWES ITS BUILT ASSETS, AND NOTHING ENFORCES IT (round 177)

A full re-verification caught a repository inconsistency that no gate had: rounds 172 and 173 changed console SOURCE
(`barePrefix` and its call sites), and the built assets under `gateway/public/assets/` — which are TRACKED, hashed, and
referenced by `gateway/public/index.html` — were left at the old revision. `git status` showed the shape plainly: one new
hashed bundle, one deleted, and a modified `index.html`.

The PANEL has a guard for exactly this, and it is the reason the two ends differ here: `agent/build.rs` refuses to compile when
`resources/panel-react/src` is newer than `resources/panel/`, with a message that says the bundle is embedded at compile time.
The console embeds nothing — its assets are served by the worker — so nothing compares them to the source, and CI's `ui` job
cannot see it either: it rebuilds the same new file, runs the tests, and never asks whether the tree it built from was
consistent.

ROUND 179 CHECKED THE OTHER TWO SURFACES and the gap is exactly one surface wide. The LANDING has no build output at all: `index/public/` holds static files (headers, icons, the installer scripts) and `index/src/page.js` is the page itself — inline, with `"scripts"` in its package.json holding only `test`, because that page has no bundler (round 95 recorded the same fact from the other direction). The INDEX WORKER serves those files; nothing is generated, so nothing can be stale. The PANEL is guarded by `build.rs`, the CONSOLE by this check, and the landing needs neither.

ROUND 180 CLOSED THE SURVEY, and the RELEASE CHAIN is the fourth answer: `agent/vale-agent-npm/vale-agent.exe` is NOT tracked (the package's gitignore covers it; `git ls-files` lists only the readme, the json, `bin/` and `src/`), so the repository cannot hold a stale exe — the release flow builds it fresh, `publish-release.bash` drives it, and `build-pins.bash` pins that the WORKFLOW never carries one. Four surfaces, four answers:

    panel          generates, TRACKS, and is guarded by `build.rs` (the bundle is embedded at compile time)
    console        generates, TRACKS, and is guarded by `console-assets-check.mjs` (round 178)
    landing/index  generates NOTHING to track — the page is inline and the worker serves static files (round 179)
    npm package    generates and does NOT track the exe — nothing can go stale (round 180)

RECORDED WITH THE TWO THINGS IT IS WORTH: the assets are committed (the inconsistency is gone), and the gap is named — a
freshly checked-out tree can serve a console built from older source, and the test that would notice is a comparison between
the built assets and a rebuild, which is a candidate rather than something started here.

### THE FIRST COMPLETE DESIGN MEASUREMENT SINCE ROUND 81, AND A NEW KIND OF COVERAGE NOTE (round 183)

`226a42f2` is the first run since round 81 whose design job finished, because every push before it cancelled the one in
flight — a discipline this session re-learned twice. It is GREEN, with 142 surfaces and 8266 rows, and it reports two things:

    note: mark family traj-ev-dot declares 6 state(s) and this run rendered 5 (bg, fail, muted, ok, warn)
          — the known one: `running` is a state the derivation carries and no surface paints.

    note: mark family X declares N state(s) and the CLASS IS ON SCREEN — the probe did not record it as a family of its own

NINE of the second kind: side-dot, tab-dot, monitor-chip, run-row-state, archive-state, dtab-dot, health-state, dev-sig-state and
one more. That note is new — it distinguishes "the class renders but the probe did not group it" from "nothing renders it", and
it is the coverage queue doing exactly what it was built for: naming, per run, the families whose silhouettes remain UNVERIFIED.

SO THE NEXT THREAD IS ALREADY WRITTEN: nine families the surface probe sees on screen but never measures as families, which
means their states' silhouettes and collisions are unproven — the same class of gap that hid the four collisions this objective
found by hand. Recorded here rather than started, because the round that takes it should begin by READING the probe's grouping
rule, not by assuming it.

### THE NINE "CLASS IS ON SCREEN" NOTES ARE RIGHT, AND READING THE PROBE SETTLED IT (round 184)

Round 183 saw a coverage note this repository had not produced before — nine families reported as "declares N state(s) and the
CLASS IS ON SCREEN — the probe did not record it as a family of its own" — and the honest next step was to read the rule rather
than to assume either way. The rule, in the probe's own template:

    const present = new Set();                       collected for EVERY visible element, before any filter
    for (const el of … ) { for (const c of cls) if (/(dot|dotcol|mark|led|chip|signal|state)$/.test(c)) present.add(c);
      if (r.width < 4 || r.height < 4 || r.width > 40 || r.height > 40) continue;        ← THE SIZE FILTER
      const state = el.getAttribute('data-state') || el.getAttribute('data-live');
      const base  = state ? cls[0] : cls.length > 1 ? cls.slice(0, -1).join('.') : null;

So a class is "on screen" if any visible element wears it, and it becomes a FAMILY only if the element is a MARK — between 4
and 40 pixels on both sides. The nine families are text lines and full-width rows (`health-state`, `dev-sig-state`,
`run-row-state`, `archive-state`, `monitor-chip`, `side-dot`, `tab-dot`, `dtab-dot`), which the size filter excludes on purpose:
round 32's comment says it in as many words — "a class on a full-width ROW is on screen and is not a mark, and collecting only
from small elements made the judge call those NO SURFACE RENDERED — a queue of false gaps, which is worse than no queue".

SO THE NINE NOTES ARE THE QUEUE WORKING, not a gap to close: those families are measured as TEXT by the contrast pass on every
run, and their silhouettes are not a question that applies to them. The note's wording says exactly that, and this paragraph is
here so the next round does not spend itself on nine families that are already covered — by the other instrument.

### CAN A GATE FAIL SILENTLY? MEASURED, AND THE SCAN WAS NARROW AGAIN (round 185)

The question is worth asking — an operator who sees a red X with nothing in the log learns nothing — so it was measured: of the
34 `.mjs` gates, ONE has `process.exit(1)` paths and no `console.error` at all, `harness-fixture-check.mjs`. Reading it settles
the matter: it reports through `console.log` (`FAIL: …` per check, and a summary line that says "N FAILED"), which is the
stdout/stderr distinction and not a silent failure — every CI runner shows both streams interleaved.

So NO GATE FAILS SILENTLY, and the scan was narrow in the same way as six before it: it looked for one spelling of "reports a
problem" and concluded from its absence. The rule this session keeps arriving at is the fix — a text scan chooses what to READ —
and the count here (one) was small enough that reading it cost a single command.

### THE CONSOLE'S ACKNOWLEDGEMENT: MEASURED, FAST, AND MECHANICALLY DIFFERENT (round 187)

Round 186 found the panel's ack to be a positive model — one hook, nine components, latency measured. The obvious next question
is whether the console has the same, and the answer is that it acknowledges just as well by DIFFERENT means:

    panel     lib/useAck.ts → `{ busy, ack, run }` for nine components, with ackMechanism.test.ts beside it
    console   per-view `useState`: DevicesPanel.tsx:603 `boolean`, Models.tsx:118 `string | null` (WHICH prefix is busy)

So the console has no shared hook, and its busy state has TWO SHAPES for one idea — but its feedback is `disabled`, which the
rendered ack pass recognises and times at 5-11 ms against the stated 100 ms budget, and that is the clause the objective asks
about. A shared hook is a PANEL convention, not a requirement; what would be a defect is a control that does not answer at all.

AND THAT IS THE OPEN QUESTION, named rather than assumed: the sweep's ack pass discovers its own controls where every button
does device work and keeps a CURATED pair elsewhere (Settings monitors), so the console's coverage is partial by construction.
The panel paid for the full treatment — a pass over every control it renders, with 158 presses and zero dead — and whether the
console needs the same is a measurement nobody has taken.

### DOES EVERY BUSY FLAG DRIVE SOMETHING VISIBLE? MEASURED, AND THE SCAN MISSED TEXT (round 188)

The follow-up to round 187's open question, asked statically first: of the console's many per-view busy states, do any guard
NOTHING — a flag that changes no pixel and so acknowledges nobody? A scan for `disabled={…busy…}`, `data-busy`, `aria-busy` and a
class interpolation reported three: `loading` in Models, Routes and AuthContext.

READING THEM SETTLES IT, and the answer is that all three acknowledge in a spelling the scan did not know:

    Models.tsx:573      <Badge tone="muted">{loading ? t("loading") : `${total} …`}</Badge>   a TEXT ack, in place of a count
    Routes.tsx:80       if (loading) { … <p className="muted">{t("loading")}</p> }            an early-return loading screen
    AuthContext.tsx:68  the flag is exposed to the whole app and consumed there               a session-level state

So NO busy flag is silent, and the scan was narrow in the familiar way — the eighth time this session that a text check was
mistaken for a rule. The console acknowledges with `disabled`, with TEXT, and with a whole SCREEN, and only a rendered
measurement can see all three.

WHICH IS EXACTLY WHY ROUND 187'S QUESTION IS THE RIGHT ONE TO LEAVE OPEN: the static question has now been asked twice and
answered twice in the same direction, and the only instrument that can ask it properly is the ack pass — which covers the
console only where it discovers controls or keeps its curated pair.

### A CORRECTION: THE CONSOLE HAS NO ACK PASS AT ALL (round 189, correcting round 187)

Round 187 recorded that "the console's ack pass keeps a curated pair elsewhere" and left the extension as an open question. That
sentence was WRONG, and the way it was wrong is this session's ninth misreading: the greps that produced it printed line numbers
from TWO files (the panel sweep at 980 and 1002, the console sweep at 545), and I attributed the panel's wiring to the console.

The measured state, verified three ways:

    console-design-sweep.mjs   0 references to ackPass / ACK_BUDGET / data-busy / acknowledg*
    panel-design-sweep.mjs     6 — a CURATED pair AND `discover: 4`, which asks the DOM for every visible control
    lib/design-sweep.mjs:611   `export async function ackPass(page, targets, budgetMs, label)` — SHARED, so the console can use it

So the console has NO acknowledgement measurement: its feedback is covered by the sheet-level `feedback-check` (an `:active`
rule exists) and by the rendered press pass (the press paints), and nothing times a control's acknowledgement against the stated
budget — while the panel measures 158 presses at 4-9 ms.

THE CANDIDATE IS NOW ONE CALL SITE, and it is written here rather than started at the end of a round: add `ackPass` to the
console sweep with `discover: N`, exactly as the panel does, and let the next design job measure what it finds. Its first run
may well find controls that answer nothing — which is what the panel's first run found, and what the instrument exists for.

### THE CONSOLE'S ACK PASS RUNS AND REPORTS NOTHING — AND THE IDLE FINDINGS ARE GONE (round 193)

`b24e84f6` is GREEN, and two facts come out of its design job:

  1. THE TWO IDLE FINDINGS ARE GONE. `console/light/keys` (1 mutation) and `console/light/routes` (4) disappeared once the ack
     pass moved after the idle window — which confirms the diagnosis: those mutations were the presses' own state updates, not a
     repaint of unchanged output.
  2. THE CONSOLE'S ACK PASS PRODUCED NO ROWS. The log carries twelve `note: ack panel Memory-ack-*` lines (`acked=true
     via=disabled ms=3-4 budget=100`) and ZERO `note: ack console` lines.

So the wiring is in place and the run is clean, and the open question is now narrower and answerable in one read: either
`wants('ack')` is false where the console calls it, or `ackPass` found no controls to press (its `discover` cap, the skip list,
or a page label), or the ROWS exist in `report.ack` and the note printer shows only some of them. The next round reads the note
printer first, because that is the cheapest of the three to rule out.

RECORDED RATHER THAN GUESSED, which is the habit this session has paid for repeatedly: the instrument ran, the run is green, and
what it did or did not find is a question with three candidate answers and a one-command test for each.

### THE EMBED GUARD IS SATISFIED BY THE CALL ITSELF (round 195)

Pointing the panel at the shared `ackNotes` looked like a one-line follow-up. It was not, and the reason is a weakness in the
guard round 103 added for exactly this class: `assertEmbedded(out, [... "ackNotes" ...])` checks that the NAME APPEARS in the
emitted text — and the CALL site supplies it. So the emitter exited 0 with the helper's definition missing, and the emitted
sweep would have thrown `ackNotes is not defined` at its first console page.

MEASURED: with the panel's import, embed line, call and assert all present, the emitted script contained `ackNotes` exactly
ONCE — which is the call, not the definition. The panel's copy of those three note lines therefore STAYS, and the revert is the
round's honest action rather than a half-wired sweep.

WHAT WOULD FIX IT, recorded for the round that takes it: the guard should ask for the DEFINITION, not the name — the emitted
embed is `const NAME = ` immediately followed by the function's own text, so `new RegExp("const " + name + " = ")` is the shape,
and it cannot be satisfied by a call.

### THE CONSOLE'S FIRST ACKNOWLEDGEMENT MEASUREMENT (round 197)

The instrument rounds 187-196 built finally answered, on a green run: 33 `note: ack console` rows over the console's pages.

    acked=true            27
    acked=false            3      and 6 rows carry via=none — the ones that asked the device NOTHING
    via=disabled          19
    via=paint             16
    via=data               8
    over the 100ms budget  0
    "the SECOND press"    35      almost every row

Three things worth keeping from it. The console's controls DO answer — 24-42 ms against a 100 ms budget, with `disabled` and
`paint` doing most of the work. The rows that asked nothing are notes and not findings, which is the rule this suite settled in
round 19 after it accused three innocent connect-tab controls: a control with no work to do cannot be judged for not
acknowledging. And the "acknowledged only on the SECOND press" note fires on 35 rows, which is a PATTERN rather than a defect:
the first sample after a fresh page paint sees nothing, and the second sees the change — exactly the timing artefact the note's
own wording describes, and the reason it is a note and not a finding.

The run is GREEN: nothing exceeded the budget and nothing is silently dead. What the console's feedback clause needed was the
measurement, and it now has one.

### "CAN YOU OPTIMIZE THE PANEL DISPLAY?" — THE ANSWER WAS A MEASURE (round 265)

Asked in the session, with a second ask arriving mid-round: "任务管理器没有vale agent logo". The operator chose
**measure first, fix what is objectively wrong** over naming a page, so nothing was changed until the six live pages had
been captured and probed (device, 1.2.449, 1280x800 and 1440x900, BOTH densities).

WHAT SURVIVED THE FIRST PASS, and what did not — worth writing down because three of the four candidates were the
instrument's fault, not the panel's:

  * the missing `<h1>`s were MY probe's: it filtered `.sr-only` out by size, and every page does name itself (the
    outline contract in `documentOutline.test.ts` is intact). The tenth time this suite has had to read the artefact
    before believing the instrument.
  * the 10px meta text is ON the declared floor (`designScale.test.ts` pins 10), so it is not a defect.
  * 19-23px controls pass WCAG 2.5.8's spacing clause, which is the criterion as written (undersized AND unspaced).

THE DEFECT: PROSE WITH NO MEASURE. At 1440px, in both densities, the Settings page rendered **twelve paragraphs as
single lines of 93-206 characters** (`.muted monitor-lede` 206, `.muted notify-hint` 193, `.muted restart-note` 188,
`.muted` 151 / 143 / 135 / 123 / 120 / 118 / 112 / 104 / 93) and the Plugins subtitle as one 85-character line. What
makes that a defect rather than taste is that **the sheet already answered the question three blocks away**:
`.archive-lede` and `.activity-lede` have carried `max-width: 66ch` since they were written and wrap at 73. The rule
existed; it had simply never been applied to these surfaces.

THE FIX, and the rendered numbers that say it worked (harness built from these bytes on the device, stamp
`260716-290d0a71a7f4`):

    panel/Settings      worst 206 -> 76 cpl   (every capped paragraph now 503.153px = 66ch at 13px, 16 blocks measured)
    desktop/Settings    worst 206 -> 76 cpl   (17 blocks)
    Plugins .plug-sub   85 on ONE line -> 43 over two (464.449px)
    audit (same build)  294 nodes, 0 failing, 2 waived — where the same page reported 206-char lines before

THE GATE, because a rule nobody measures is exactly how this shipped: the sweep's SURFACE probe now reports
`measure { measured, worst }` per surface and the panel judge fails any row over **`proseFloor: 90`**. The floor came
from the defect class, not from a preference: the twelve offenders start at 93 and the sheet's own capped ledes render
70-73. There is a floor on the INSTRUMENT too — a pages run that matched fewer than twelve blocks fails, because a run
that measured nothing reports "no long lines" and reads exactly like a clean one. The console and the landing carry the
same numbers in their reports and are NOT judged by this floor: neither has been measured on this axis, and a threshold
somebody else picked is not a finding about them. Mutations in `scripts/test/panel-design-sweep.bash`: `prose` (a
planted 206-character line) and `prose-none` (measured: 0) — both proven to fail the judge.

TWO MORE DEFECTS, FOUND BY MAKING THE INSTRUMENTS RUN:

  * **`panel-render-audit.mjs` HAD NEVER RUN.** It called a bare `PROBE` that moved to `lib/contrast-probe.mjs`, and
    nothing could see it: CI only takes the emit path (which exits 2 before that line), the skip gate pins that exit,
    and the only environment that reaches the call is a device — where it died on `PROBE is not defined` and read as
    "the audit is broken" rather than "an import is missing". Fixed by importing it, and by judging with the shared
    `failures()` / `unmeasurable()` instead of its own `cr < 4.5`, which asked text's 4.5 of GRAPHIC rows and counted a
    row nobody could read as a pass. Gate: `panel-audit-skip-check.mjs` now reads the SOURCE — every name handed to
    `page.evaluate()` must be imported or declared — and both mutations were run (delete the import; restore the local
    literal) and both bite.
  * **TWO INSTRUMENTS, ONE MEASUREMENT, TWO VERDICTS.** The audit's first working run reported `span.approval-grant`
    at 1.19 light / 1.25 dark as failures; the sweep waives that exact element in a measured band, because the grant
    pill's signal is its text (5.53 of 4.5) and its revoke control (5.33). A policy kept in one tool is a policy the
    other one contradicts, so the waiver list moved into `lib/design-sweep.mjs` and both read it — the audit now prints
    waived rows WITH the reason and keeps the judge's semantics (a waived selector at a ratio outside its band is still
    a failure).

AND THE SHEET HAD A PRESS GAP, RED AT HEAD: `.settings-fold > summary` carried a hover and no `:active`, and
`feedback-check` failed on it. Verified pre-existing by restoring the committed sheet and re-running (same failure),
then fixed with `transform: translateY(1px)` — the vocabulary every other press in this sheet uses.

THE SECOND ASK: THE EXE HAD NO ICON AT ALL. Task Manager drew a generic glyph because `vale-agent.exe` carried no
resource of any kind — the brand mark existed in `brand/icon.ico` and was wired into the desktop app and the installer,
never into the service binary the operator actually looks at. `agent/build.rs` now generates a `.rc`, compiles it with
LLVM's `llvm-rc` and hands the `.res` to the linker (lld-link accepts a `.res` as an input file — measured before the
rewrite). On the built exe: **6 resources, all four brand frames byte-present** (16/24/32/48), **no absolute path
embedded** (0 occurrences — the dual-builder audit's premise holds), and **two forced re-links are byte-identical**.

**AND THE FIRST VERSION OF THAT FIX WAS WRONG IN A WAY ONLY CI COULD SEE, which is the durable lesson of this round.**
It asked for `llvm-windres`, because windres can emit a COFF object directly and the box here has it (LLVM 18, reached by
resolving the `llvm-rc` symlink cargo-xwin creates in `~/.cache/cargo-xwin`). Every local measurement passed: the icon in
the bytes, the four frames present, two re-links identical. Then `agent (xwin check windows-msvc)` went red with
`cannot run llvm-windres` — that job installs APT's `llvm`, which ships `llvm-rc` and no such binary, and it never runs
the symlink step `release.yml` does. "It works here" had been mistaken for "it works", and the environment that decides
is the one that has none of this machine's conveniences. The fix is smaller than the bug: use `llvm-rc` (which every one
of these environments has), emit a `.res`, link that — and SEARCH for it in the four places it is actually installed
(PATH versioned and not, the cargo-xwin symlink, Ubuntu's `/usr/lib/llvm-*/bin`, MSVC's `rc.exe`) with a hard failure
naming `VALE_LLVM_RC` if none exists. A warning-and-continue would have restored the original defect in silence. The VERSIONINFO
carries the npm package's version (1.2.449) and FileDescription `Vale Agent`, because that file IS this binary's version
at release time (the release flow bumps it before building) while the crate's own 1.0.x is not — and a process with no
FileDescription is listed as `vale-agent.exe`, which is what the operator saw.

**AND THE AXIS HAD A BLIND SPOT OF ITS OWN, FOUND BY MEASURING THE DEVICE RATHER THAN THE HARNESS.** The live panel at
1440px (1.2.450, after the update) still reported two uncapped classes: `.update-note` at 93 characters per line — real,
and capped now — and `.plug-desc` at 129, which is NOT a defect: that row's description is `white-space: nowrap` with
`text-overflow: ellipsis` and the full text on its title, i.e. a LABEL, and a measure would truncate it sooner rather
than make it readable. The overflow axis has always treated that idiom as legitimate; the measure axis excludes it now.
THE BOUNDARY IS DELIBERATE AND NAMED: a paragraph could in principle be silenced by ellipsising it, and neither axis
would report it — the human-visible ellipsis is the detector for that, and building a third counter for a shape nobody
has produced was not worth the surface this round.

**AND THE LAST RED JOB WAS THE INSTRUMENT'S OWN BLIND SPOT, NOT THE PANEL'S.** With prose clean, the design job still
failed on twelve identical findings: `button.connect-tab (button.connect-tab.on) renders NOTHING when pressed`. The device
said otherwise — pressing it in a relaxed-mode surface reported `transform: none -> matrix(1, 0, 0, 1, 0, 1)` — and
reproducing CI's exact rail sequence reproduced the finding, with `hit: false` carried in the row. What the probe was
looking at: the connect tabs sit inside a CLOSED `<details>`, and **Chromium keeps LAYOUT BOXES for a closed details'
content** (49x24 measured) while `checkVisibility()` returns false and `elementFromPoint` over them returns the section
painted there. So discovery offered three controls the pointer could never reach, and `reached` — the flag that exists
precisely so that "a press the pointer never delivered is a note, never a dead-control finding" — was computed as
`!(box.movedPage && hovered.hit === false)`, which consults the hit test ONLY when the element had to be scrolled first.
Two fixes, both in the pass, both about the same sentence: `checkVisibility` now gates discovery AND both press probes
(a box is not a surface), and `reached` is `box.reaches !== false` — the hit test AT THE COORDINATE PRESSED. Rendered on
the device after: the rail pass discovers five real controls and ZERO connect tabs, and a curated press of the hidden one
reports `not rendered on this page` instead of accusing it. `press-anchor-check` pins both — and its first version of the
guard assertion matched the string ANYWHERE, which passed with the guard deleted from `discoverPressTargets` because the
same three lines live in the two press probes. Scoped to that function's own text, the mutation bites.

**AND THE OPERATOR ASKED ABOUT THE BOTTOM BAR, WHICH TURNED OUT TO BE A GEOMETRY DIFFERENCE BETWEEN THE TWO
DENSITIES** (his words: "最底下的一栏为什么不是左侧到头的"). Measured on his window, 1181x798: `nav.desktop-rail` was
0,0,57x798 — a FULL-HEIGHT column — while `.desktop-status` was 73,743,1092x39 with `border-radius: 0 0 14px 14px`,
i.e. the content card's footer, inset 16px from the right and bottom; `elementFromPoint(40, 790)` was the rail. That
was not an accident: `desktop.css` says "icon rail | canvas … a status strip folded into the content card footer —
surfaces, not bars". The PANEL density does the opposite (its `StatusBar` is a sibling of the row, so it spans the
window), and the same device-level facts — version, uptime, CPU/MEM, relay, watches — were therefore drawn in two
different geometries depending on which shell you were in. HIS CALL, TAKEN: make it a bar. `Shell`'s desktop branch
now renders the same shape the panel branch always had (`desktop-body` holding rail + main, then `{statusBar}`),
`DesktopShell` passes its strip through the new slot instead of folding it into the canvas, and the strip's own rule
loses the card's bottom rounding. Re-measured: `.desktop-status` 0,759,1181x39, radius 0, `elementFromPoint(6, 790)`
AND `(1175, 790)` both the bar, rail 759 tall. The test that pinned the old rule ("hides context rail and status bar")
now pins the new one and names why. This is also the round's clearest example of the loop's own rule working: the
operator's question was the SPEC, and the answer was a measurement rather than an opinion.

### THE EMIT SEAM, FIRST SLICE: THE LANDING'S PAYLOAD BECOMES A MODULE (round 266)

The architecture review's top candidate was the emit seam — five scripts that build their device-side program INSIDE A
TEMPLATE LITERAL in their own source, so the emitted text is a second grammar and six modules exist only to compensate
(four parse guards, `assertEmbedded`, a bash program that walks the emitted text counting backslash runs, a pre-commit
hook). 53 recorded incidents came from that one rule.

**DECIDED, AND TAKEN AS RECOMMENDED (the operator said "自动完成上面的这些" rather than answering the four questions):**
(1) the delivered artifact stays a single self-contained CommonJS file, so the device contract does not move; (2) the
seam stays where it is — the JSON report is the interface, the verdict stays host-side; (3) migrate ONE emitter first and
prove the seam before touching the other four; (4) equivalence is the bar, measured, not argued.

**WHY NOT A BUNDLER.** esbuild 0.21.5 IS in `agent/resources/panel-react/node_modules` (a vite transitive dep), but
`--emit` has to work where the emitters actually run: on the device, under its bundled node, with no node_modules. A
bundler that only exists on CI trades one broken environment for another. So `agent/scripts/lib/sweep-bundle.mjs` — about
100 lines, zero dependencies — resolves the payload's literal relative requires, refuses a cycle or a missing module BY
NAME at bundle time, wraps each module in a factory, and COMPILES what it is about to return.

**THE SLICE.** The landing's payload is now `agent/scripts/lib/sweep/landing-run.cjs`; the run-varying pieces (paths, the
baked stamp, the probe texts, the six passes) arrive as a generated module beside it, so nothing is escaped and the
"borrowed helper must also be embedded" list is not a list any more — `pressPass` calls `discoverPressTargets` because
they are declared in one module body. `landing-design-sweep.mjs --emit` assembles it. **AND THE LANDING EMITTER WAS THE
ONE WITH NO PARSE GUARD AT ALL** (verified: `new Function` appears twice in the panel emitter, once each in console and
audit, ZERO times in landing) — its `--emit` would print a 79 KB script to stdout without ever asking whether it parsed,
which is now impossible by construction.

**EQUIVALENCE, MEASURED ON THE DEVICE.** Same rendered landing, same pass subset (`contrast,names`), the old emitter's
script and the new bundle run one after the other on d1: 102 contrast rows each, 4 surfaces, 4 name checks, and
`surfaces`/`names`/`themeChecks`/`entryCheck` byte-identical. The four rows that differ are the landing's own footer
CLOCK (`12:17:16` vs `12:17:23`, seven seconds apart) — a timestamp, not a behaviour. The program pieces were also
compared directly before that: 12/12 identical (three probe texts raw, three page-check texts decoded, six pass sources).

**AND THE FIRST VERSION OF THE ASSEMBLER WAS BROKEN IN A WAY THE PIECE COMPARISON COULD NOT SEE.** Its loader resolved a
relative require to the module ID STRING instead of calling `__require`, so the first payload that touched a required
value died with `Cannot read properties of undefined (reading 'root')` — while every piece was byte-identical and the
emitted file parsed. Comparing text proves the text; a seam is proved by EXECUTING it. That is why
`scripts/test/sweep-bundle-check.mjs` (wired into the design job) assembles three-module payloads in memory, writes the
bundle into a temp directory that holds NOTHING else, and RUNS it: the exports-vs-id case fails loudly (`require
returned string`), as do a cycle, a missing module, an unparsable payload, and a payload whose comment carries a
backtick plus an interpolation plus a `\s` regex — the three shapes that used to end a host file mid-parse.
`gate-mutations-check.mjs` carries that lookup-without-the-call as its mutation; it exits 1 naming the case.

**WHAT REMAINS, in order:** migrate `panel-design-sweep.mjs` (the big one: 1012 of its 1631 lines are payload), then
console, then the harness emitter, then the live probe; convert `pageChecks`/`marksSource` from source-text generators
into functions that take the root selector as an argument (the landing's pieces module evaluates them once and says so);
and only then delete the four parse guards, `assertEmbedded`, the bash backslash walk and the hook arm — deleting them
before the last emitter is migrated would remove the guards from the emitters that still need them.

### THE EMIT SEAM, SECOND SLICE: THE PANEL'S 1012-LINE PAYLOAD BECOMES A MODULE (round 267)

The panel emitter was the big one: **1012 of its 1631 lines (62%) were the emitted program**, held inside a template
literal with nineteen interpolations — nine passes via `.toString()`, four probes and the page checks via
`JSON.stringify`, `${DIAG_SOURCE}` as code, `${MOTION}`/`${TIMING}` as snippets that themselves declared a const, and
`${JSON.stringify(HARNESS_STAMP)}`. Every backtick in any of that text ended the HOST file mid-parse (53 incidents),
the Windows defaults carried FOUR backslashes to survive one escaping level, and `assertEmbedded` existed to check by
substring that a borrowed helper had also been spliced.

**THE MOVE.** `agent/scripts/lib/sweep/panel-run.cjs` is the program as ordinary code — 956 of its 978 non-empty lines
appear verbatim in the old artifact, which is what "moved, not rewritten" looks like when you measure it. The emitter
generates the pieces module (passes, probes, checks, the diag helper, the two probe snippets, the run-varying config)
and `bundleSweep` assembles the two. Three things died on the way: `assertEmbedded` (the passes and the payload meet in
one module body, so a helper is in scope by construction), the hand-copied `new Function(script)` parse guard (the
assembler compiles what it returns), and the four-backslash defaults (the values are values now — the landing's first
migration caught the same over-escaping as a real path bug, and this one had `C:\\ProgramData` in its own defaults).

**EQUIVALENCE, MEASURED TWICE.** Structure first: **17/17 program pieces appear BYTE-IDENTICALLY in both artifacts**
(the four probes, the three page-check texts, the nine passes, the diag helper — each in the encoding it actually
crosses by). Then behaviour, on d1, same harness, `--passes=timing`: the old script and the new bundle report
**1138 rows, 24 surfaces, 24 name checks** each, and every report field is **identical** except `timing`, whose four
rows differ only in wall-clock milliseconds (427/435/336/344 nodes in both). The 355 KB reports differ by ONE byte.

**AND ONE GATE HAD PINNED THE OLD CARRYING CONVENTION.** `panel-design-sweep.bash`'s "the probes reach the page
unchanged" check looked for `const NAME = "<json>";` declarations — the spelling round 57 introduced. It reported
"SURFACE is not a JSON string on both sides" on a payload whose probes reach the page byte-identically. It now reads
the pieces module OUT OF THE ARTIFACT, evaluates it in a sandbox, and compares all seventeen values with what the core
produces; a string value must additionally cross as JSON (which is what makes it lossless), while a FUNCTION crosses as
code — the backslash walk covers that half, and it stays. Two mistakes in writing THAT check are worth the line: the
module's end was found with the first `\n};` (a factory body contains functions; it cut the module in half and the
failure read "Unexpected token ')'"), and the JSON assertion was first applied to every value including functions
(`.toString()` makes those look like strings). 83 ok, 0 failed.

**WHAT REMAINS:** console (484 payload lines), the harness emitter (646, HTML not a script), the live probe (68); then
`pageChecks`/`marksSource` become functions taking the root selector (the panel's pieces module evaluates them once and
says so); only then can the four remaining guards, the bash backslash walk and the hook arm be deleted.

### THE EMIT SEAM, THIRD SLICE: THE CONSOLE'S PAYLOAD, AND THREE GATES THAT PINNED A SPELLING (round 268)

The console emitter was next: 484 of its 652 lines (74%) were the emitted program, sixteen interpolations, and a parse
guard copied from the panel after a backtick in a COMMENT here had already cost two `node --check` cycles.
`agent/scripts/lib/sweep/console-run.cjs` is that program as ordinary code now, with the pieces module beside it and
`bundleSweep` assembling the two.

**IT ALSO CAUGHT A REAL ESCAPING BUG IN THE PANEL'S MIGRATION — and the way it was caught is the lesson.** I had been
copying the template literal's SOURCE TEXT into the new module, and a template literal's source is not its value: the
console's route handler is written `p.replace(/^\\//, '')` in the emitter (one escaping level for the literal), so the
module needs `/^\//` — the text the device actually ran. The panel and the landing had escaped this only by luck (the
panel's route serves one file and never strips a path; the landing's was hand-written from the ARTIFACT). The fix is a
check, not care: every payload line must appear VERBATIM in the artifact the old emitter produced, and it found exactly
six escape-residue lines in the console module (two of them comments carrying `C:\\ProgramData`) beside the nineteen
bindings that were meant to change.

**EQUIVALENCE.** Structure: 16/16 pieces byte-identical in both artifacts, 474/474 payload lines present. Behaviour: the
old and new console sweeps run against the same built console on d1 and their reports are compared field by field —
the numbers are in the commit message and, like the panel's, the only differences are the ones wall-clock produces.

**AND THREE GATES HAD PINNED THE SPELLING RATHER THAN THE RULE, each failing a payload that was right:**
  * `panel-design-sweep.bash` looked for `const SURFACE` / `const NAMES` in the emitted console script (the page checks
    now arrive as data) and for `assertEmbedded(out` in the landing (which now gets the assembler's compile instead).
    The probe check is now ONE program that reads whichever pieces module an artifact carries and compares every value
    the core produces — 17 for the panel, 16 for the console — in the encoding it actually crosses by.
  * `sweep-judges.bash` regexed `EXPECTED_ENTRY = ({...})` out of the console artifact to build its staleness fixtures;
    after the move the read came back EMPTY and the failure surfaced as a python `SyntaxError` in a fixture, which
    names neither the gate nor the cause. It reads the digest from the pieces module now, and says what it could not
    read.
  * `sweep-fixture-dupes-check.mjs` scanned the EMITTERS for `const API = {`; the console's fixture table now lives in
    its payload module, so the scan read ZERO tables and printed "0 fixture key(s) … no duplicates" — exit 0. The
    per-file floor could not see it (every path may legitimately `skip`), so the run now refuses to report success
    having read nothing, and the mutation that points the list back at the emitters fails it.

WHAT REMAINS: the harness emitter (`panel-render-audit.mjs`, 646 lines that produce an HTML harness rather than a
script — it has one guard that must stay, `/</script/`, because the stub is inline HTML), then the live probe (68);
then `pageChecks`/`marksSource` become functions taking the root selector; then the guards.

### THE EMIT SEAM, FOURTH SLICE: THE LIVE PROBE, AND THE CHECK THAT MAKES EACH MOVE SAFE (round 269)

`live-panel-probe.mjs` is the last of the five emitters that build a device script inside a template literal, and the
smallest: 68 payload lines, three interpolations (the probe, the marks source, the config paths it looks for a device
token in), and — since round 167 — a parse guard copied from the panel after a backtick in a comment would have shipped
a broken script "in the middle of 16 KB". `agent/scripts/lib/sweep/live-run.cjs` is that program as ordinary code; the
three values arrive as the pieces module beside it; `bundleSweep` assembles and compiles it.

**EQUIVALENCE, AND THIS TIME IT IS EXACT.** The probe's output is JSON on stdout, so the comparison is direct: the old
artifact and the new bundle run one after the other against the panel d1 is actually serving (1.2.451) and print **the
same bytes** — `configAt` the same, panel 58 rows (47 text / 11 graphics), desktop 28 (17 / 11), zero failing, zero
unmeasurable, the same four mark families (`mark[working]`, `sc-dot[ai]`, `ag-dot[off]`, `monitor-mark[is-down]`), no
collisions, no ring+fill, `errors: []`, `verdict.ok: true`. Structure first: 3/3 pieces byte-identical, and 64 of the 68
payload lines verbatim in the old artifact — the four that differ are the header and the three bindings that replaced an
interpolation.

**AND THE METHOD IS NOW THE POINT.** Three migrations in, the safe way to move a payload is mechanical and checkable, and
it is what the last two rounds converged on: (1) cut the literal's text into a module, (2) point each interpolation at
the pieces module, (3) **compare every payload line against the artifact the OLD emitter produced, and treat every
difference that is not a binding as a bug**. Step 3 is what caught the console's six escape-residue lines (round 268)
and this emitter's token regex — a template literal's SOURCE is not its VALUE, and copying one for the other is
invisible in a diff of the new code alone. It also gives the two measurements that go in the commit: pieces
byte-identical, and payload lines verbatim.

WHAT REMAINS: the harness emitter (`panel-render-audit.mjs`, 646 lines). It is a different shape — it emits an HTML
harness with an inline `<script>` stub rather than a CommonJS script, so the assembler needs a browser target (no
`require` fallback in the loader) and the `/</script/` guard stays, because the stub is inline HTML. Then the page
checks become functions that take the root selector, and only then can the guards go.

### THE EMIT SEAM, FIFTH SLICE: THE HARNESS — AND THE BUG ONLY A PAGE COULD SEE (round 270)

`panel-render-audit.mjs` was the last of the five and a different shape: 640 payload lines that produce an HTML HARNESS
with an inline `<script>` stub which runs in a PAGE. So it needed `bundleSweep`'s browser target (round 269): no
`require` in the preamble, a non-relative require refused at bundle time, the bundle wrapped in an IIFE. Three
interpolations (the session id, the session object, the event list) became the pieces module, and two of the three
guards went with the literal they guarded — the backtick search and the hand-copied compile. The third STAYS: a closing
script tag inside a payload that is inlined into an HTML script element cuts the fixture in half, and the page still
loads and still renders a panel with no sessions — which reads like a product bug.

**IT CAUGHT ITS OWN AUTHOR.** The first version of `panel-stub.cjs` had a header comment that spelled the closing-tag
sequence out (while explaining the guard), and the emitter refused to emit. The comment now says "a closing script
tag" and records why.

**AND THEN IT SHIPPED A DEFECT THAT EVERY LOCAL CHECK PASSED.** The stub binds the pieces module at the top of the
file — I called it `P` — and the stub has its own `var P` for the query flags, declared INSIDE the IIFE. `var` hoists
and shadows, so `P.sid`, `P.session` and `P.events` all read the params object and came back `undefined`. The page
rendered session IDS where labels belonged and lost its goal and approval rows (Terminal: 156 rows against 170).
Nothing local noticed: the pieces VALUES in the artifact were byte-identical to before, the stub's 639 lines were all
present verbatim, the boot check passed, and `harness-fixture-check` regexed the emitted HTML happily. Only running the
sweep against the harness on d1 showed it — 14 rows short, in rows whose TEXT said `term-audit-0` where the old run
said `d1`. The binding is `FIXTURE` now, and `sweep-bundle-check` scans every payload module for a second declaration
of the name it binds the pieces to (mutation: put `P` back and it fails, naming the file).

EQUIVALENCE, on d1, both harnesses emitted from this checkout and driven by the same sweep artifact: **1120 rows
each, zero rows present in only one, ZERO field differences**, and `surfaces` and `names` identical. The method that
caught the shadowing is the one worth keeping: the device run is not a formality on top of the structural checks, it
is a different instrument.

WHAT REMAINS: `pageChecks`/`marksSource` become functions that take the root selector (the pieces of the panel, the
console and the live probe each evaluate them once through `new Function` today, and each says so in a comment), and
then the guards that only existed for the template literals can go.

### THE EMIT SEAM, LAST SLICE: THE PROBES BECOME FUNCTIONS, AND THREE COLLAPSED REGEXES COME TO LIGHT (round 271)

The five emitters no longer build payloads from template literals, but the PAGE CHECKS still did: `pageChecks(rootSelector)`
returned 209 lines of source text with the selector substituted in (`replaceAll("ROOT_SEL", ...)`) and the mark axis spliced
into it at a `/* MARKS_PLACEHOLDER */` marker; `marksSource(rootSelector)` did the same for the live probe. That
substitution is what once put a Node-side identifier into page code and killed the extension sweep with `ROOT_SEL is not
defined`, and the splice was silent: renaming the marker would leave `marks: {}` with no error anywhere.

**THEY ARE FOUR REAL FUNCTIONS NOW** — `marksProbe(root)`, `surfaceProbe(root)`, `namesProbe(root)`, `reflowProbe(root)` —
and the payloads call `page.evaluate(probe, SELECTOR)`. Two consequences worth stating: the selector cannot leak as an
identifier because it is an argument, and the mark axis is evaluated alongside the surface probe and merged in Node
(the report's shape is unchanged; the JSON key order moved by one position, which is why `surfaces` compares unequal
while its length is byte-identical — a deep comparison shows ZERO differences).

**ONE COMPOSITION POINT REMAINS, AND IT IS NAMED.** `page.evaluate(fn)` serializes the function ALONE, so anything a probe
closes over is undefined in the page: `surfaceProbe` needs `loudnessOf`, which is also imported by
`state-colour-check.mjs`, so it cannot simply move inside. `withHelpers(body, helpers)` binds a module-level helper into a
probe's body once, at load, from its ONE definition — the job the old interpolation did, isolated to one documented
function instead of scattered through a template. Nothing else in the library composes source.

**EQUIVALENCE.** Locally, each new probe's body was compared with the string the old emitter shipped: NAMES, REFLOW and
MARKS byte-identical modulo the selector now being an argument, SURFACE identical except that its helper declaration
moved to the top of the function. On d1, the same sweep artifact emitted before and after: 1138 rows each, and `rows`,
`reflow`, `names`, `focus`, `motion`, `hover`, `unstyled`, `targets`, `themeChecks` and `sse` IDENTICAL; `surfaces`
deep-equal (the key-order note above); `timing` differs only in wall-clock milliseconds. The live probe reports
`verdict.ok` with the same families, collisions and ring+fill.

**AND THE CONVERSION EXPOSED THREE COLLAPSED REGEXES, which is the round-57 bug family surviving in the one place nobody
re-read:**
  * `namesProbe`: `own.replace(/\s+/g, ' ')` — the emitted probe has `s+`, so the claim text replaces runs of the LETTER
    "s" with spaces. This is the same collapse that once made the loud axis count every element for thirty-seven rounds.
  * `marksProbe`: `split(/[\s,/]+/)` — emitted as `[s,/]`, so class names are also split on the letter "s".
  * `marksProbe`: `/\\.(dot|dotcol|...)$/` — this one is OVER-escaped (the emitted regex wants a literal backslash).
They are preserved EXACTLY as they behave today by this round's conversion — fixing them changes what the probes measure,
so it belongs in its own round with the report diff in front of it, not folded into a refactor whose whole claim is
"nothing changed".

`FOCUS_SOURCE` (exported, imported by nothing) is deleted, and the "no backticks inside PAGE_CHECKS_TEMPLATE" warnings
now name the constraint that still exists: the SURFACE body is carried in the one template literal `withHelpers` reads.

**THE RULE HAS A GATE**: `sweep-bundle-check` scans every payload for a probe called without its selector (90 calls
across 5 modules; mutation: drop the argument and it fails, naming the file).

### THE EMIT SEAM, CLOSING THE REVIEW'S CRITERIA: ONE PIECES GENERATOR, AND THE WARNINGS THE LITERALS NEEDED (round 272)

The architecture review's candidate 1 said it precisely: *one module whose interface is "named browser-side pieces + a
JSON config in, a runnable script out": it owns quoting, splicing, which helpers travel, and refuses to print a script
that does not compile. The five emitters become adapters that supply only their genuinely local facts.* Rounds 266-271
built the assembler and emptied the literals; two of its stated success criteria were still open, and both are closed
here.

**"ESCAPING OWNED ONCE" WAS NOT YET TRUE.** Four emitters had each grown a `piecesSource()` spelling out the same two
rules — a function crosses as code (`fn.toString()`), everything else as JSON — and the live probe and the harness had
two more. `piecesModule(pieces)` in `sweep-bundle.mjs` generates that module now, from one implementation, and the five
emitters pass FACTS (a config object, probe texts, the passes, `diag`). The subtlety worth its line: a function is
declared at the top of the generated module and referenced by name, never inlined into the exports object, because the
passes CALL EACH OTHER by name (`pressPass` calls `discoverPressTargets`) and an inline function expression would leave
that name undefined at run time. `DIAG_SOURCE` — the last payload piece that was still source TEXT, kept regex-free and
backslash-free because of the escaping layers — is an ordinary `async function diag(line)` now, which is what the
review meant by guards evaporating: the rule that shaped it ("no regex, no backslashes, deliberately") was a
consequence of the seam, not of the job.

**AND 17 WARNINGS THAT ONLY THE LITERALS NEEDED ARE DELETED** — "(No backticks: emitted template literal.)" and its
siblings, scattered through the payload modules by a decade of rounds that had each been bitten by one. The one that
stays is the one that says the rule is gone.

**EQUIVALENCE, MEASURED ON THE ARTIFACTS.** Every piece was compared before and after by VALUE, not by text: the panel
exports 11 pieces and 8 compare identical, the three that differ being the intended shape changes (a probe is a
function now, a comment was rewritten) — `passes.pressPass` is byte-identical; the harness's three fixture pieces are
3/3 identical; the landing emits and parses; all five emit and `node --check` clean. The gate that compares every
probe/pass/check value against the core still reports 17 for the panel and 16 for the console, byte for byte.

**WHAT A REVIEW CRITERION IS WORTH.** "Escaping owned once" reads like a slogan until you count the implementations:
four, each a copy of the same two rules, none of them wrong — which is exactly the state that produces a fifth that is.
The criterion was measurable and it was measured.

WHAT IS NOT VERIFIED HERE: the sweep's `pages` pass was not run against this build. It does not fit the device runner's
per-call cap (round 253) and this box has no browser (nine missing shared libraries). CI's design job runs it on the
branch; every rendered number above comes from the device's own Playwright against a harness generated from these bytes.

---

## 2026-09-23 — the release pipeline audited its own author (1.2.453 → 1.2.455)

Six hours of shipping two releases produced five defects, and **every one of them was a step that reported success
without doing its job** — the class this repo already had a name for, found this time in the tooling rather than in the
product. AGENTS.md keeps only the rules; these are the transcripts behind them.

**1. `gh release create <tag> <tgz>` published a release and attached NOTHING.** Four seconds, no output, exit 0. Its
replacement, `gh release upload` plus a `gh release view --json assets` check, did the same on a release the API shows
has ZERO assets. The reason neither could see it: the release was a DRAFT, and **a tag MOVE demotes a release to a
draft** — so `GET /releases/tags/<tag>`, exactly the read the release audit performs, answers 404 while `gh` (which
does find drafts) is satisfied. "The step succeeded" and "the audit can see the asset" were two different questions.
release.yml now does it through the API: find the release for the tag INCLUDING drafts, PATCH `draft=false`, delete any
same-named asset, upload, then assert against `/releases/tags/<tag>` that the release is PUBLISHED and lists the name.

**2. The audit read a CACHED endpoint.** `scripts/lib/release-audit.sh` asked `/releases/tags/v<ver>` whether the tgz
was attached. An asset upload does not invalidate that endpoint: for minutes it answered `assets: []` while
`/releases/<id>/assets` listed the 6,694,727-byte tgz — so the audit reported "no asset" for a release that had one, the
reconcile debt stayed open, and the release suite failed two checks that belonged to a cache. It now takes the ID from
the tags read (stable) and asks the ASSETS sub-resource. The fixture reproduces the lie on purpose, and a second case
pins that a release whose assets endpoint really has no tgz still FAILS.

**3. The audit's own worse finding, once it could read.** With the cache out of the way it said:
`source-derived file drifted: ./bin/summrise.js … the two builders packaged DIFFERENT SOURCE — do not ship`. CORRECT,
and my doing: v1.2.453's pack was published from one tree and its TAG was later moved onto a commit carrying the
component-fetch CLI, so CI packaged a different artifact under the same version number. A tag must never move onto
different content; if CI has to go green again for an already-published version, put an EMPTY commit on the release
commit and tag that. 1.2.454 then passed the dual-builder audit byte-for-byte, exe included.

**4. `--npm` never reached the registry on a first publish.** The npm block sat BELOW the audit, and the audit cannot
pass on a first publish (the asset is built after the tag), so its failure branch exited first. Measured: the CDN
deployed, `/api/version` smoked green, `--npm` was passed, and npm still answered `latest: 1.2.453`. A step that must
happen, ordered after a step that is allowed to fail, is the whole bug; npm now publishes before the audit.

**5. A test failing for a state it does not own.** `publish-release.bash` sat at 8/10 while the CDN carried an
unreconciled version, because the reconcile gate fired first and the two cases read ITS refusal as their own failure.
Both now pass `--acknowledge-unreconciled` — the documented escape — and reach the assertions they are about. Not a
weakening: the assertions are unchanged, and the gate still fires for a real publish that has acknowledged nothing.

**AND THE TWO BREAKAGES THE 1.2.454 CYCLE FOUND IN PRODUCTION**, both from the rename that created a new worker and a
new bucket and carried over neither. `summrise-dist`'s `wrangler secret list` was `[]`, so `/api/upload` — which
compares the bearer against `env.UPLOAD_KEY` — accepted only an empty bearer and rejected the gateway's real key: the
file relay's upload leg, dead since the rename. And `summrise-playwright.zip` was in neither bucket, so that route had
answered 502 for a day; nobody noticed because every device already had the component expanded locally and nothing
fetched it through the route. Both fixed and verified the same day (a 32 MB upload whose sha256 matched on arrival, its
one-time URL correctly 404ing on the second fetch; the bundle rebuilt from the device's copy and staged in R2, the route
answering 200 with the right content-length). The class: **a resource recreated under a new name needs its secrets and
its bucket's contents — that is the cutover, not follow-up work.**

**AND WHAT THE RELAY CUTOVER COST IN TWO MISTAKES OF MINE, both caught by testing rather than reasoning.** A fresh
`UPLOAD_KEY` written to the relay but not to the gateway produced a 401 — the ADR's "a fresh shared secret is required"
means BOTH sides in one command. And the relay built its download URL from `request.url.origin`; called through a
SERVICE BINDING, that is `https://summrise-relay.internal/...`, a host nothing can resolve — the end-to-end transfer
caught it, and a unit test would not have. `PUBLIC_BASE` is a var on the relay now, with the route's own origin as the
fallback. The route (`agent.saisi.online/files/*`) takes precedence over summrise-dist's dashboard-managed Custom
Domain, which is what made a path-level handover possible at all.

---

## 2026-09-23 (later) — the architecture pass on the release pipeline

The operator asked whether releasing could be one `npx` command the way dsh installs; the honest answer
was NO, and the question was sharp enough to run the architecture process on the release pipeline
itself. Five candidates came back; this is the first, the cheapest, and the one that makes the others
smaller.

**THE INTERFACE IS AS WIDE AS THE IMPLEMENTATION, MEASURED.** `publish-release.sh` is 582 lines and
carries **30 refusal/usage branches** — a stale exe, uncommitted pack inputs, worktree permissions that
differ from a fresh checkout, a foreign `tsc` major, a missing one, pin drift between
`index/components.json` and `agent/src/tunnel.rs`, three credentials, and a CDN carrying a version with
no release to audit against. Every one is a PRECONDITION THE CALLER MUST ALREADY HAVE SATISFIED, and
none is expressible except by running the thing: the middle of the script (pack → stage → installer →
prune → deploy → smoke → npm → audit) has no seam, and its own suites say so in their headers. The
contrast is inside this repo: `release-audit.sh` HAS a real seam — its tests substitute a `curl` shell
function — which is what two adapters at one seam buys.

**C5, DONE FIRST: delete the residue.** Each item was checked before it was touched, and one of the
review's findings did not survive that check.

- `mode_drift` in `release-audit.sh` was initialised at :207 and branched on at :234, and **nothing
  appended to it** — the live mode check had moved to the tar-listing comparison above. Deleted. Its
  branch carried a better rationale than the check that actually fires ("the tgz IS the release, and two
  different tgz files are two different releases however identical their contents"), so the REASON moved
  to the firing check rather than dying with the dead code, and that check's message gained the
  Cause/Fix lines it never had. The audit suite is 27/27 and its mode-drift case still bites on the
  words FILE MODES.
- A four-line comment appeared twice, verbatim, twelve lines apart. Deduped.
- **THE SCRIPT'S OWN HEADER DESCRIBED THE WRONG ORDER.** It listed `5 commit → 6 deploy → 7 audit →
  8 npm`; the code runs `deploy → smoke → npm → audit`, because npm had to move before the audit when a
  first publish never reached the registry. This is the C2 problem in miniature — a release's order
  living in prose that drifted from the code — and the fix was to make the prose TRUE and to mark where
  the command ENDS and a human begins.
- The npm dist-tag was described three contradictory ways in one file ("alpha by default", the
  `NPM_TAG="latest"` assignment, and "defaults to `latest` here, NOT `alpha`") plus a fourth in the
  design doc. One statement each now.
- Four comments cited `publish-release.sh:344` and `:309`. The real call had moved to `:416`, twice in
  one night. **A line number in a comment is a claim with no owner; a function name is not.** All four
  now name the function.
- `release.yml`'s own failure message advised the bypass "move the tag" — the procedure 1.2.453
  disproved, and which `AGENTS.md` recorded the same evening. An error message is where an operator
  reads the procedure, so it now carries both halves: fix CI and re-run THIS job; a moved tag demotes
  the release to a DRAFT and makes CI package a different artifact under the same version.

**THE FINDING I REJECTED, because verification is not optional.** The review called
`grep -q "summrise-release"` a magic-string gate that proves nothing and should be deleted; it noted in
the same breath that the freshness `cmp` right after it is what bites. Both halves are true and neither
is a defect: the grep is a cheap "did you compile at all" pre-check with an accurate message, and
deleting it removes an early exit without concentrating complexity anywhere. Left alone. The same pass
caught a second over-claim: the review read a test's "the reconcile ledger was deleted 2026-09-14" as
contradicting the live reconcile gate. They are DIFFERENT LEDGERS — the test means the hand-satisfied
MARKDOWN one; the gate reads `docs/agents/release-reconcile.txt`, gitignored and mechanical. Not a
contradiction, but a trap for the next reader, who would conclude the gate is dead code (it refused a
publish that same day), so the test now says which ledger it means.

**AND THE LESSON THAT COST TWO CANCELLED RUNS.** The release rule already said "push, WAIT for that
commit's CI to go green, then tag it". What it did not say is that pushing the NEXT piece of work
cancels the run you are waiting for: GitHub supersedes it, the tag has no green CI to point at, and
`release.yml`'s own gate refuses. It happened on `94cb06fd` and again on `fe29a24d`, both times because
the loop kept working while waiting. AGENTS.md now says it, and this round obeyed it: the C5 commit was
held locally until 1.2.456's tag existed.

---

## 2026-09-23 (later still) — C2: the release sequence becomes data, and gets a dry run

The architecture review's top recommendation, and the piece that answers the operator's actual
question. Two changes, both load-bearing:

**1. `SEQUENCE="pack stage prune deploy smoke npm audit"`.** A release's order lived in four places
— the script's layout, its header comment, AGENTS.md, and cases in the suite that asserted it **by
line number**. The header had already drifted from the code once (it listed the audit before npm —
the order a FIRST publish never survives, because the audit cannot pass until the asset exists and
its failure branch exits). Now a checker walks the source's EFFECT banners and fails when they run
in a different order, and the case that proves the checker bites MOVES a banner line.

**THAT CASE PASSED FOR THE WRONG REASON FIRST, and the reason is the lesson.** The mutation renamed
a banner instead of moving it, so the check failed with "no banner for step 'deploy'" — the
PRESENCE rule, not the ordering rule — and the case was green while proving something else. A
mutation aimed at the wrong property is evidence about the mutation first; it now asserts that the
refusal NAMES the order. The checker had its own version of the same lesson one step earlier: its
first form matched SUBSTRINGS, and `deploy` matched the installer's banner ("installer
(self-contained, staged, no deploy yet)"), so it reported a CORRECT script as non-monotonic. Both
are the same rule this file has recorded before — read the artefact before believing the instrument
— applied to a check I had just written.

**2. `--dry-run` is the seam.** The 30 refusals ARE this module's real interface: a stale exe,
uncommitted pack inputs, worktree permissions that differ from a fresh checkout, a foreign `tsc`
major, pin drift, three credentials, a CDN carrying an unaudited version. Every one is a
precondition the CALLER must already have satisfied, the whole gate block needs no credential, and
it sat inline above the first effect — so no test could reach it, and `publish-release.bash` said so
in its own header ("the FIRST executable coverage … nothing ever RAN it"). `--dry-run` runs the
entire gate block — including the binary recompile-and-cmp freshness check, the electron copy sync
and the cloudflared pin cross-check — and stops before the first effect.

**MEASURED, on this machine.** It exits 0 with every gate reporting. Run against the real tree
BEFORE the reconcile escape it exits 1 through the reconcile gate, naming the 1.2.453 debt — a
precondition that previously cost a full release attempt to discover, now visible in two seconds
and with no credentials. And it says what it does NOT cover — the Cloudflare token check, which
lives inside its step — instead of implying it verified everything: a dry run that overclaimed is
the exact failure mode the rest of that file exists to prevent.

**ON CI THE DRY RUN MUST REFUSE** (no staged exe), so the suite's case asserts the invariant that
holds everywhere rather than the verdict that holds here: it either passed or refused BY A GATE,
and it left the worktree exactly as it found it. The suite is 15 checks (was 10).

**WHAT IS STILL SHALLOW, said plainly.** The sequence is DATA now, but the script is still a linear
582-line file: `--dry-run` is ONE adapter at the gate seam, and the effects themselves (pack →
deploy → npm → audit) are still reachable only by running the world. That is the next candidate,
and it is smaller than it was an hour ago.

---

## 2026-09-23 (the desktop command, and C3) — a command that had never met Windows

`summrise desktop` was designed, unit-tested (48/48), published in 1.2.456 and reported as done. It
had never been executed on Windows. It failed there three times, in three different ways, and every
one of them was invisible to the suite that passed:

1. **`powershell -File '<path>'` is not a valid invocation.** The single quotes came from `psq()`,
   which escapes for a PowerShell STRING; at the cmd layer they are literal characters, so
   PowerShell received a path with quotes around it and answered "unsupported path format" for a
   path that was perfectly valid. Double quotes are what `-File` wants. **The same mistake sat in
   `setup` and had never been noticed**, because the task on this device had been registered by
   hand — a command whose only producer is manual is a command nobody has run.
2. **It must not re-register the task in order to start it.** `Register-ScheduledTask` needs the
   interactive user's principal as `DOMAIN\user`, and a shell running as a service account has no
   such mapping: from a PTY running as `systemprofile` the device answered "No mapping between
   account names and security IDs was done … UserId" **while the task itself was Ready**. The task
   already carries the right principal — `setup` creates it — so starting it is all that is needed,
   and registering is reserved for the case where it is genuinely absent.
3. **THE ONE THAT EXPLAINED THE OTHER TWO: the command must not be a `-Command` string at all.**
   The CLI spawns with `shell: true`, so every command goes through `cmd.exe`, and **cmd splits a
   command on `&`**. The PowerShell call operator in the middle of the logic cut the command in
   half, so the `$t` assignment never ran, the guard fell through to registering, and that failed on
   the principal. Two failures that looked unrelated, one metacharacter — the same class as the
   `printf | grep -q` lesson and the `pgrep -f` lesson already in this file.

The fix is structural: the logic is written to `<install>\scripts\desktop-start.ps1` and run with
`powershell -File "<path>"` — nothing to mangle, one level of quoting, and **the operator can read
what the command does**. The test pins the WIRING as well as the builder: `bin/summrise.js` must
write the script to a file and must not spawn `desktopStartPs()` as a `-Command` string.
Mutation-verified: renaming the file in the built CLI turns it into "the CLI must write the start
script to a file" (1 failure), and the suite is 48/48 restored.

**AND THE FIRST RUN THAT WORKED PRINTED A WARNING WORTH KEEPING:** `[DEP0190] … Passing args to a
child process with shell option true can lead to security vulnerabilities, as the arguments are not
escaped, only concatenated.` That is Node saying `spawnSync(cmd, [...args], {shell:true})`
concatenates rather than escapes — the documented form is a single command string, and this command
needs no argument list. The operator should not have to read a security warning to get a window.

**C3, THE ARCHITECTURE ITERATION: the reconcile ledger is an INPUT, and the review was wrong about
it in a way worth recording.** The review called it "machine-local state deciding a publish" and
concluded the gate was dead code. Inspecting it corrected that twice: `RECONCILE_LEDGER` has been
overridable since the gate was written (`${RECONCILE_LEDGER:-docs/agents/release-reconcile.txt}`),
so the seam EXISTED — it was never FILLED. One adapter is a hypothetical seam; two is a real one.
The gate had only ever been observed against the real, gitignored, machine-local file, which is
precisely why a reader could mistake live code for dead. Three cases now drive the real script with
a fixture ledger, and the middle one is the control that makes the others evidence: a pending
version refuses AND names it; an EMPTY ledger does not refuse and the run reaches the NEXT gate; and
`--acknowledge-unreconciled` carries it past. Suite 15 -> 18, no credentials, no network.

**AND THE NETWORK, which is now a first-class actor in the release story.** The direct GitHub
release-asset download died twice at the 300 s cap (1.9 MB and 0.8 MB of 6.7 MB) while the API asset
URL served the same bytes immediately — so the audit now falls back to the API asset id it already
reads. The fallback is verified to FIRE (the log names the id and makes the request), and on the next
attempt the network defeated BOTH routes, which is a fact about this box and not about the audit. The
same degradation failed an `npm i -g` from the CDN mid-install. What stands regardless: 1.2.456 was
verified three ways by hand (CDN manifest, local staged tarball, API asset — the same sha256), and
1.2.457's release is published with its asset.

**C4, THE NEXT ARCHITECTURE ITERATION: one fact, one owner — and the copy that had teeth.** The
review listed five owners for "what artifact is this release"; the one that can actually hurt is the
list of files a packed tarball MUST contain, which stood verbatim in `scripts/publish-release.sh`
AND `.github/workflows/release.yml`. Two owners of one fact for as long as nobody edits it, and two
DIFFERENT release gates the moment somebody does — which is the failure the list exists to prevent
(round 278/282: a file missing from the tgz fails nothing, the device's update flow keeps the STALE
file, and the release looks fine).

It is now `agent/summrise-agent-npm/required-in-tgz.txt`, read by both builders.

**AND IT DELIBERATELY IS NOT DERIVED from `package.json`'s `files[]`, which looked like the obvious
owner.** That array names `bin/` as a DIRECTORY, and both gates check `bin/summrise.js` — so a gate
derived from `files[]` would pass while the CLI itself was missing, which is precisely the defect
class the list guards. The near-miss is worth the paragraph: "there is already an owner" is a claim
to CHECK, not to act on, and the check here took one look at the array.

Verified two-sidedly against a REAL pack (the 1.2.458 tarball): every required entry is present, and
removing `bin/summrise.js` from the listing makes the gate say exactly that. Neither builder carries
a restated copy any more, and two suite cases hold it — a source assertion of the honest kind, since
a second copy IS the defect and a behavioural test cannot see a copy that happens to agree today.
Suite 18 -> 20.

**CF_TOKEN: FOUR COPIES, ONE OWNER, AND A DISAGREEMENT NOBODY HAD NOTICED.** The review listed this
almost in passing ("`cf_token()` duplicated 4x, each with its own rationale") and it was the real
remaining thing: four byte-identical bodies in `build.sh`, `publish-release.sh`,
`build-installer.sh` and `publish-cdn-from-ci.sh`, whose COMMENTS had already begun to diverge —
which is how a fifth copy gets written with a fifth idea. Three of the four already sourced
`scripts/lib/release-lib.sh`; `build.sh` now does, at the site where its copy used to be.

The copies also disagreed with the rest of the system about TRIMMING. The npm token path learned on
2026-09-23 that a token file written with a trailing newline authenticates as nothing — "401 while
the file looks right" cost an hour — and the fix was `tr -d`. The Cloudflare copies still `cat`'d
the file raw. The one owner trims, and the reason sits where the code is.

**THE SEAM MADE THE TESTS REAL.** `cf_token` resolves `${CLOUDFLARE_API_TOKEN}` first and
`$HOME/.cloudflare-token` second, so a fixture home is a second adapter — the cases exercise the
resolution order instead of asserting source text: file, environment-beats-file, the trailing
newline, and no-token-prints-nothing. `release-lib.bash` 40 -> 44, release suite still 20, and all
four scripts resolve a token through the one owner when run with a fixture home.

**AND THE CASES DID NOT RUN AT FIRST, TWICE, IN TWO DIFFERENT WAYS.** The first version used the
`ok`/`bad` helpers from `publish-release.bash`, which this suite does not define; the second was
appended with `cat >>` AFTER the suite's summary line and `exit`, so it never executed and the
suite cheerfully reported 40. Neither mistake was visible in the output — the second one especially,
because this suite's `check` prints ONLY on failure, so "no output" is indistinguishable from "not
run" unless you are watching the count. Watching the count is what caught it.

**AND A NEGATIVE RESULT ON THE REVIEW'S TOP CANDIDATE.** C1 ("582 lines, no seam in the middle") does
not survive inspection: the middle is thin adapters around decisions that ALREADY have seams and
suites — `write_version_json`, `prune_last5_per_minor`, the packed-tgz gate, `smoke-index.sh` with its
stubbed curl, and `release-audit.sh` with 27 checks — and most of the 582 lines is prose (the header
plus the reason beside each gate). The part of C1 that was real was the ordering, and C2 addressed it
with `--dry-run` and `SEQUENCE=`. A review finding is a hypothesis; this one was worth checking
rather than acting on.

**AND A STALL NEEDS A SPEED BOUND, NOT A LONGER TIMEOUT.** The audit's fallback to the API asset
route (added the same evening) never fired: `-m 300 --retry 5` can spend THIRTY MINUTES proving a
route is stalled, and my own 25-minute cap killed the run before the API route was ever tried. The
measurements, four attempts in one run: 15,459 / 32,488 / 0 / 0 bytes of 6,698,935.

`--speed-limit 20000 --speed-time 20` is the instrument for this: abort a transfer that stays under
20 KB/s for 20 seconds, and the stall this host produces is under 200 B/s. Retries then cost seconds
instead of minutes, so they drop to 2, and the fallback carries the same bound. Measured after:
direct dies in ~20 s, the fallback fires and NAMES the asset id, and the whole audit resolves in
201 s with a message that says both routes were tried. Ten times faster to the same honest answer —
and when either route works, it passes.

The general shape is worth keeping: a timeout answers "how long am I willing to wait", which is the
wrong question for a transfer that is moving but not arriving. Throughput is the question, and curl
can be told to ask it.

**THE REVIEW MEASURED INTERFACE *SIZE*, AND SIZE IS NOT DEPTH.** Working through the architecture
review's candidates to the end produced one real defect and three negative results, and the pattern
in the negatives is the useful part:

| the review said | inspection found |
|---|---|
| `cf_token()` duplicated 4× | four byte-identical COPIES whose comments had begun to diverge — **real**, and fixed (one owner, and it trims) |
| `--check-modes-only` is "a 4-line entry point whose only job is to expose a function the chain made unreachable" | that IS its job. `--dry-run` refuses at the FIRST failing gate, so on CI (no staged exe) the mid-chain mode gate is unreachable through it — the narrow entry point is what makes a single gate testable on a tree where other gates fail |
| `assert_want_sha256` is "a 6-line regex wrapper, re-called by its own caller" | five call sites across three scripts, plus its own test file with several cases: a shared precondition check, not a wrapper |
| C1: "582 lines, no seam in the middle" | thin adapters around decisions that already have seams and suites; most of the file is prose |

The DELETION TEST is what separates them, and the skill states it precisely: deleting three copies of
`cf_token` CONCENTRATES the behaviour in one place; deleting the other three MOVES their job to their
callers and leaves it written once per caller. A small module with one job is not a shallow module —
depth is leverage per unit of interface, and a 4-line function whose interface is "the mode gate's
verdict" is deep by that measure. The review produced a good list of *small* things and correctly
found the *copied* one among them; three of its five candidates were size mistaken for shallowness.

It also means the candidate list is worked through: C1 (negative), C2 (`SEQUENCE=` + `--dry-run`),
C3 (the ledger as an input with a fixture adapter), C4 (one owner for the packed-tgz list), C5 (the
residue), and the shallow-module list (one real fix, three negatives). The next iteration should
start from a FRESH exploration rather than from this list — and it should scope itself to a different
subsystem, because the release pipeline has now been measured, corrected, and re-measured.

---

## 2026-09-23/24 — the tool layer: a live drift, and the artifact that makes prose checkable

A fresh exploration (the release pipeline had been worked through) went at the tool-registration
path, and found the thing every previous round kept finding in a new place: **one fact, two owners,
no gate.**

**THE DEVICE AND THE CONSOLE TOLD THE MODEL DIFFERENT CONTRACTS**, with all ten contract tests
green. `monitor_list`'s console copy had lost `last_expect_ok` while `drops`' explanation moved onto
`last_status`. Nothing could compare prose that existed in only one machine-readable place: the
snapshot pins NAMES and PARAMETER NAMES deliberately ("a type difference between the two sides is a
separate question this snapshot is not trying to answer"), and the code-viewer mirror gate compares
the console's copy **to itself**.

**SO THE FIRST HALF WAS TO MAKE THE DEVICE'S PROSE EXIST SOMEWHERE A CHECK CAN READ IT**: the
snapshot now carries all 56 descriptions. The measurement that followed turned one known drift into a
systemic one — of the 32 console-exposed tools, 5 were identical, 27 differed, and FIVE had lost a
field the device names (terminal_execute's session_id; terminal_read's offset/start/end;
terminal_resize's rows/cols; terminal_screen's lines; terminal_write's break_ms). Those five were
carried over MECHANICALLY from the spec, and they carry lessons: terminal_resize now tells a console
client that rows/cols are OPTIONAL and default to 24x80, knowledge from a schema that once claimed
otherwise.

**THE GATE IS FIELD PRESENCE, AND IT WAS PROVEN ON THE REAL DRIFT.** No console copy may drop a
backticked field the device names — the spelling both sides use — with a floor so a scan that reads
nothing fails. Proven by re-introducing tonight's exact loss: `console copies that drop a field the
device names: monitor_list: last_expect_ok`. THE FIRST MUTATION ATTEMPT DID NOT BITE, and that was
the third such lesson in one evening: my perl pattern guessed at the file's formatting, matched
nothing, and the gate rightly passed an UNCHANGED file. A mutation that does not bite is evidence
about the mutation first — so the second attempt used the same walker the gate uses.

**AND THE GATE ITSELF HAD THE BUG IT EXISTS TO CATCH.** Its walker assumed descriptions are written
with double quotes; prettier picks whichever quote needs fewer escapes, so `browser_run_script`'s
description (containing `"<SUMMRISE_RUN_ID>-*.png"`) is written with SINGLE quotes. The walker
returned 23 characters of a 1080-character text — a gate mis-reading its input reports violations
that are not there, or misses ones that are. Both quote styles now, and the measurement script was
corrected too, because its numbers are what this ledger records.

**STATE AFTER THE ROUND, MEASURED:** 11 identical, 2 containment (secret_set's merge keeps the
console's "Lives on the device agent — the browser extension is not involved."), 19 REWRITES — every
one still lossy (monitor_add 1122 device chars -> 788 console). The next iteration is a CONTAINMENT
rule (device text must appear in the console's), which catches both halves of the original drift —
a dropped field AND a moved explanation — and its work list is those 19, in the order of how much
each lost.

**AND CI CAUGHT WHAT NO LOCAL CHECK OF MINE HAD:** deleting the dead `NavItem` left
`summrise-command-core/src/lib.rs` unformatted. `cargo fmt --check` runs in CI and I had never run
it. The repo's own rule — run the command the other end runs — applied to the loop that wrote it.

**SIX QUOTING FAILURES IN ONE EVENING, AND THEY WERE ALL THE SAME MISTAKE.** Merging the two
description walkers into one took three attempts, and the two that failed are worth more than the fix:

1. A regex-based mover missed one of the two blocks, because prettier had reshaped the one I had not
   read verbatim — the same "read the artefact, not your memory of it" that this file records three
   times elsewhere. Located by LINE SCAN instead.
2. The replacement script itself died of escaping: a regex literal embedded in a JavaScript string
   embedded in a shell command. `node --check` reported the syntax error, and the file was untouched,
   so the suite passed and looked like success.
3. And then the commit message — written with single quotes, containing "reader's", "device's" and
   "doesn't" — ended its own string early and ran the rest of the sentence as shell commands.

Every one of the six came from writing CODE INSIDE A STRING: a perl pattern guessing at formatting, a
`python3 -c` with nested quotes, three node `-e` snippets, and a commit message with an apostrophe.
The tools that take text verbatim — the file editor, and `git commit -F - <<'MSG'` — failed none of
them, and have been used all evening without a single escaping problem. The rule this file keeps
arriving at in other forms applies to the loop as much as to the code: USE THE INTERFACE THAT TAKES
THE TEXT, NOT THE ONE THAT PARSES IT.

The consolidation itself is one definition and two call sites now, which is what the containment gate
needed: with two walkers, only one of them unescaped single-quoted literals, and the newer copy
reported a false violation on a description that was already verbatim the device's.

**A RED TREE REACHED MAIN BECAUSE A TEST AND A COMMIT WERE CHAINED WITHOUT `set -e`.** The gate
refused the commit — correctly, it had found a name the merge had paid — and the shell ran the commit
anyway: `npm test | grep …` on one line and `git commit` on the next, in one command, with no `set -e`
and no `&&`. A red tree sat on main for two minutes, and the CI run for that commit will be red in the
history forever. The repo already carries the rule for this shape — READ THE EXIT CODE, NOT THE
OUTPUT — written for a different failure mode one level down. This is the same rule applied to the
command line that does the committing: a step that must not run on failure has to BE conditional, not
merely preceded by a failure it happens to print.

The check itself deserved the last word, though: it named the exact entry (`system_file_upload`) that
had been paid and left in the list, which is what a self-cleaning list is for.

**THE TOOL-PROSE WORK IS FINISHED: 32 CONSOLE-EXPOSED TOOLS, 24 IDENTICAL, 8 WITH THE CONSOLE'S OWN
ADDITIONS KEPT, ZERO REWRITES, EMPTY DEBT LIST.** It started at five identical and twenty-two
rewrites, with one live contract drift that ten green contract tests had passed through.

What did the work, in order: the snapshot learned to carry descriptions (nothing could compare prose
that existed in one machine-readable place); a field-presence gate caught the dropped-field half and
was proven on the real drift; a containment gate caught the moved-explanation half, with a
self-cleaning debt list that twice refused a commit — once because entries were paid, once because I
left one behind; and then each of the 22 pairs was READ and judged. That last part is the part a
mechanical pass would have got wrong: the verdicts flipped twice. The device's text won most often,
but for `monitor_add`, `system_file_download` and the secret store's locator the CONSOLE knew facts the
device's text omits, and for `terminal_diag_write` the two sides disagreed about who calls the tool —
which I could not adjudicate without reading the callers, so the device's text won and the question is
named in the commit rather than papered over with an invented sentence.

TWO REAL DEFECTS CAME OUT OF THE COMPARISON ITSELF. `terminal_write`'s console copy showed a DOUBLED
escape, `\\n`, so a model reading the console was told to send a literal double-backslash where the
tool wants `\n` — visible only by comparing stored strings, not rendered prose. And the walker that
does the comparing needed THREE fixes: it knew only double quotes (prettier writes single ones when the
text contains a quote), then only unescaped the single-quoted style, then did not decode a
double-quoted literal as JSON at all — each fix made by comparing the artefact rather than trusting the
instrument. That walker is now defined once, which is the reason it only had to be fixed once at the
end.

The shape of the whole episode is the shape of every round in this file: one fact with two owners and
no gate. What is different here is that the fix was not a deletion or a merge but an ARTEFACT — putting
the device's prose somewhere a check could read it — and the gates fell out of that.

**A NEGATIVE RESULT WITH EVIDENCE: THE DEVICE-PROXY DOOR IS NOT A BACK DOOR.** The tool-registration
exploration ended by noticing that `gateway/src/plugins/device-proxy.ts` "forwards any /api/tools/<name>
with the device token, needing no registration at all" — accurate as a fact, and worth examining as a
question: two doors to the same device tools, with different rules. Reading it answers the question the
other way round.

The route's own header states its three auth paths (admin session cookie, paired plugin token, or the
per-device `summrise_pt_<name>` cookie minted by the `?token=` bootstrap) and the code enforces device
scoping on every one of them:

  * the per-device cookie is scoped BY CONSTRUCTION — a request for device `b` reads
    `summrise_pt_b`, so device `a`'s cookie is never even looked at;
  * the paired plugin token is checked explicitly: `if (link && link.device === deviceName)`;
  * `?token=` is accepted ONLY on a top-level navigation, because otherwise "a leaked URL
    (history/sync/screenshot/log) would otherwise grant full device terminal control via /proxy/* for
    the 30-day plugin-link TTL" — and `Sec-Fetch-Mode` is the signal, chosen because it cannot be
    spoofed cross-origin;
  * the cookie exists in a per-device form SPECIFICALLY to prevent cross-device hijack, which the
    comment names: "one origin-wide cookie would let a later-opened device's page steal an earlier
    device's terminal".

And the file carries three past fixes with their reasoning, each a real vulnerability found and closed
in place: round-103's proxy secret (`X-Summrise-Auth`, added because the R102 marker header "was
client-spoofable end-to-end (a direct curl could set it and read the token → /api/tools RCE)"),
round-124's 302 bootstrap (which kept the token out of the omnibox and minted the cookie even when the
panel failed to boot), and the stripping of inbound `x-summrise-auth` so a client cannot ride a
self-minted header through.

THE LESSON IS ABOUT THE QUESTION, NOT THE CODE: "needs no registration" describes a DIFFERENT property
than "needs no authorization", and the two are easy to conflate when one door has a registry and the
other has a policy. This one has a policy, it is written down where the code is, and it is enforced.

**AND IT IS TESTED — the third negative result in a row, and the one that redirects effort.** Having
verified the proxy's authorization by READING, the honest follow-up is whether reading is all there is.
It is not: `gateway/test/proxy-auth.test.mjs` walks the matrix.

  * paired plugin token → proxied, device Bearer injected server-side;
  * token bound to a DIFFERENT device → 401 and no upstream call — the exact cell that mattered;
  * no auth → 401; garbage token → 401; non-admin session without a plugin token → 401;
  * `?token=` on a non-navigation → plain 401; on a navigation → 302 with the token stripped and the
    cookie minted; an EXPIRED token on a navigation → the readable HTML page rather than JSON,
    because the panel's own recovery UI can never load if the bootstrap navigation 401s;
  * the minted per-device cookie authenticates, and a malformed value is absent (401);
  * an unknown device → 401 for the unauthenticated caller, which closes the device-name oracle;
  * metadata/unspec/mapped hostnames are refused WITHOUT dialing (the SSRF gate);
  * SSE responses pass through with the plugin token.

And `gateway/test/auth-gates.test.mjs` holds the audit P1 finding ("per-device summrise_pt_* cookies
are gated too"), while `device-proxy-rewrite.test.mjs` covers the body rewriting: the proxy mount for
live paths, removed table entries, double-prefix avoidance, template-interpolation close, and the
injected device token stripped in both quote styles.

So the answer to "the exploration flagged this door" is: the door has a documented policy, every auth
path is device-scoped in the code, and the policy is tested cell by cell — including the two security
subtleties (the name oracle and the SSRF gate) that a reviewer would have to think of before looking
for them. THE VALUE OF THIS RESULT IS WHERE IT POINTS: not at this file. A review finding is a
hypothesis, and this one was worth three reads and no edits.

**THE DEVICE-DOOR EXPLORATION FOUND ONE REAL GAP AND ONE FINDING I HAD TO CORRECT MYSELF ON.**

**THE REAL GAP: A CURATED CATALOGUE WITH A SECOND DOOR THAT DOES NOT KNOW IT EXISTS.** Door A, `/mcp`,
withholds tools from MCP clients by name, each with a reason, in `NOT_EXPOSED` — `terminal_sftp`
"takes arbitrary SSH credentials an MCP client should not be offered", `agent_update`
"self-modifying", `mcp_client_call` "internal bridge plumbing". Door B, the device proxy, enforces
none of it: `restPath` goes from the route straight into `deviceFetch` with no check, and the device
itself authorizes nothing per tool — `api_call_tool` does `find_tool` then `handler.call`, with no
caller identity and no allowlist. So the curation exists in exactly one place.

What makes it precise rather than alarming is the evidence, which took two greps: the PANEL needs some
of those names — `UpdateCard.tsx` calls `/api/tools/agent_update` through door B — so door A's reason
("self-modifying") is coherently about MCP clients rather than the operator's UI, and blocking the
catalogue at door B would break the panel. Of the withheld names, `terminal_sftp`, `mcp_client_call`
and `system_file_write` appear in NO UI file at all: reachable through door B with a 30-day plugin
cookie, needed by nobody. That is the escalation path, and whether a browser cookie may reach an
SSH-credential tool is a product decision, not a refactor — so it is asked, not assumed.

**AND THE FINDING I CORRECTED: "the event/capability surface is dead" SURVIVED, but not the way it was
argued.** `emit`, `on`, `requireApi`, `ctx.events` and `ctx.api` have no live consumers — but my first
grep said `ctx.api` had three and `ctx.events` five, and all eight were COMMENTS, one of them the old
code the policy replaced (`// the old (ctx.api?.translate as any)?.resolveAutoModel || null`). Live
framework use is `ctx.routes` (29) plus `ctx.user`, `ctx.model`, `ctx.generationId`. So the surface is
dead, and the deletion test says deleting it REMOVES complexity rather than moving it — but it is also
the IMPLEMENTATION of a policy adopted after an incident, kept beside the practice it guarded. The
honest move is therefore not a late-night deletion: the candidate is recorded with its reason intact,
because this file already knows what happens when code outlives the reason for it.

**THE CATALOGUE HAS ONE OWNER NOW, AND THE PROXY ENFORCES IT.** `/mcp` withheld device tools by name
with a reason each; the device proxy forwarded any `/api/tools/<name>` verbatim; and the device
authorizes nothing per tool. So the curation was one proxied POST away from being bypassed by anyone
holding an admin cookie or a 30-day plugin cookie. `gateway/src/tool-policy.ts` now records, per
withheld name, the reason AND the audience — whether the device proxy may relay it — and
`device-proxy.ts` refuses the rest with a 403 that carries the reason, before the device is dialed. A
403 and not a 401 because the console ejects a session on any 401.

**THE AUDIENCE WAS THE MISSING FACT, AND IT IS EVIDENCE.** Every call site in the panel goes through
`callTool()`/`callApi()` with a LITERAL name — the whole surface is `terminal_*`, the `memory_*`
family, `agent_update` and `terminal_saved_connections` — so `panel: true` is exactly what the panel
calls, and blanket enforcement would have broken the Update Card. That is also how a claim I made an
hour earlier was corrected: "three tools reachable, needed by nobody" came from a SAMPLE of the
catalogue, and the catalogue is ~22 names.

`gateway/test/mcp-handler.test.mjs` asserts every policy name is decided in `NOT_EXPOSED`, so a tool
cannot be withheld from one door and unknown to the other. The reverse is not required: the catalogue
carries names the panel legitimately uses.

**THREE MISTAKES OF MINE ON THE WAY, EVERY ONE CAUGHT BY A GATE RATHER THAN BY ME.** The test URL used
a host the route table does not key on (404 — and the fix was to READ A WORKING CASE rather than guess
at the shape); the refusal message is NESTED at `error.message`, because `jsonError` wraps it; and the
typecheck failed SILENTLY because I had put it in an `&&` chain — the same masking a pipe caused a
round earlier, which is why the repo's rule is about exit codes and not about reading output.

**AND A GATE I HAD NOT MET: THE ONE THAT CHECKS THE OTHER GATES ARE STILL PROVEN.** CI refused the
build with "its mutation's anchor is GONE from scripts/test/production-host-check.mjs — re-pair it, or
the gate is unproven". Bumping `MAX_ALLOWED` 41 -> 42 when `relay/` was declared removed the literal
its mutation flips, so the proof that the ratchet BITES had quietly stopped being a proof.
`gate-mutations-check.mjs` re-paired, and running it locally prints the line worth keeping: **26 gates
broken on purpose and every one of them bit.**

**THREE ROUNDS OF PROPOSING GAPS THE REPO HAD ALREADY CLOSED, AND THE POINT IS THE PROCESS.** Each of
these began as a finding I was about to act on, and each ended with the ledger — or a script named in
it — already carrying the answer. Written down because the alternative (acting first) would have added
a fourth copy of something that exists:

1. "The 13,000-line session/terminal layer has NO recorded history" — because its identifiers
   (`kill-on-close`, `idle sweeper`, `broadcast`, `serial pool`, `read_from`) appear ZERO times in the
   ledger. The layer is thoroughly covered: the record simply writes in its own vocabulary, not the
   code's — `pty` 53 mentions, `session` 116, `panel` 285, plus sections like "THE MOMENT THE DEVICE
   KNOWS" and "THE LIVE PANEL, RE-MEASURED AFTER FORTY ROUNDS". Searching for identifiers is searching
   for the wrong string.

2. "Merged is not live: the gateway deploy is manual and CI only dry-runs it" — TRUE, and it mattered:
   the catalogue enforcement from round 20 sat in main while production still forwarded any tool. Fixed
   by deploying (`Version ID 18554136…`, live surface verified: `/` 200, `/api/devices` and `/mcp` and
   the proxy guarded with 401).

3. "Nothing gates 'the deployed worker is N commits behind main'" — `gateway/scripts/check-live-parity.sh`
   has done exactly that since round 542, and its comment records why it exists: "the round-537 stale
   deploy proved green tests don't imply a fresh worker (19 src commits sat undeployed)". Round 546 then
   fixed its own false verdict (a fixed `sleep 8` compared against the PREVIOUS assets and reported a
   successful deploy as a failure — "the one verdict a deploy gate must not get wrong"), which is why
   the probe carries its own retry. Run after this round's deploy: `checked 45 files, 0 drifted`.

So the answer to "why read first" is not politeness: two of these three would have produced a duplicate
mechanism or a redundant list, and the third — the deploy — was a real gap that ONLY reading the build
script revealed (`wrangler deploy --dry-run` in CI, a manual deploy in production). The instruction at
the top of `AGENTS.md` — read the ledger before proposing anything is a defect — is doing measurable
work, which is the only kind of rule this file is interested in.

## 2026-09-24 (the agent's HTTP surface, a new gate, and a release that was blocked twice)

The ninth exploration went at `agent/src/web/` — 8,268 lines, the largest file in the repo, and the only
surface none of the eight previous passes had opened. It found the usual shape in new places, and one
thing the previous passes had not: **a security test whose own comment had stopped being true.**

**`every_dispatch_route_is_auth_gated` LISTED 24 ROUTES WHILE `dispatch` ANSWERED 32**, and its comment
promised "a new route added without auth fails HERE instead of shipping". The eight it did not test
included the two most dangerous on the surface: `/api/update` and `/api/run/mark-exit`. Nothing was
serving unauthenticated — all eight answer 401 once tested — so what had rotted was the SENTENCE, which
is the thing a security test must not lie about. The list is now paired with `dispatch`'s own literals,
and building that pairing taught three normalisations by failing on each: a `{PARAM}` template is a
prefix, a trailing `/` is a match prefix, and a prefix is covered by an EXAMPLE UNDER IT. Planting a
route in `dispatch` fails the test; that mutation is the proof.

**THE BRUTE-FORCE THROTTLE GUARDED `/mcp` AND NOT THE GATE THAT REACHES SYSTEM.** `auth_backoff_ms` and
its two neighbours were invoked from exactly two sites, both inside `TokenGate`; `check_auth` — whose own
comment calls the device token "the ONLY gate between an unauthenticated network caller and SYSTEM-level
device control" — recorded no failure and never slept. It was a TENSION rather than an oversight: the
throttle must sleep, so it lives in an async block and keys on a peer from `ConnectInfo`, while
`check_auth` took headers rather than the request on purpose so it stays usable from a `Send` future. It
is `async` and takes the peer now, with the penalty read FIRST so a caller inside its window never has
its guess compared, and one `deny` path for both failure branches. Eleven call sites, one of which the
report had not named (there are TWO streaming branches), and the peer is threaded rather than re-derived:
`peer_key` treats an absent peer as local, while the loopback bool treats absent as NOT loopback and
denies the token handout — one extension, two questions, and the auth decision nobody understood well
enough to fold together is the one that was left alone.

**A DOC PROMISED `None` AND THE CODE ALWAYS RETURNED `Some`.** `handle_browser_evidence` said "Returns
None when the path is not one of ours" and its two `if`s FELL THROUGH to the screenshot reader, so an
unknown path was read as a pwshot name and answered `400 "bad name"` — a wrong answer dressed as a right
one, which is also why the caller's `if let Some` looked permanently true. The defect lived in a SEAM
BETWEEN TWO LISTS: the handler enumerates its three routes, and `route_pre_dispatch` enumerates the paths
that reach it. Neither list could see the gap.

**THE `200 + {ok:false}` SHAPE IS A CONTRACT, NOT FRICTION.** The exploration listed three answer-shapes
for one failure and twelve readers in three dialects. The gateway settles it — `mcp.ts` records the shape
in a comment and compensates with `!ok || data.ok === false` — so answering a failed tool with a non-2xx
would break two clients. The readings were also three QUESTIONS, not one: `callApi`'s `res.ok` is the
HTTP status, and the `s?.ok` reads in the browser and evidence panes are about a NESTED object. What was
real is that eleven sites asked the first question in two dialects that DISAGREE about a body carrying no
`ok`; they now call one `deviceRefused`, strict, because the device always sends `ok: true` on success.

**AND ADDING THAT ONE EXPORT BROKE THREE TESTS INTO PASSING-LOOKING SILENCE.** Their mock was
`vi.mock("../../lib/api", () => ({ callApi: vi.fn() }))`, which replaces EVERY export: the hook called
`undefined`, threw into its own catch, and "a failed read must not update" quietly made no read update at
all. The three went red with an EMPTY VALUE rather than an error — including two whose fixtures carry
`ok: true`, because the throw happens before any fixture is read. `tsc` cannot see it; a `vi.mock`
factory is never checked against the module. All nineteen factories now spread the original, and a new
NAMED gate holds it: `panel-mock-spread-check.mjs`, with a floor (19 found, 15 required) so a renamed
idiom reports "this proves nothing", wired into the design job, and paired with a mutation that puts a
wholesale factory back. **ITS OWN FIRST RUN FAILED ON `lib/api.ts` ITSELF**, because the predicate's doc
QUOTES the idiom it warns about — the trap `exports-check` and `retired-colours-check` each recorded on
their own first run, which is why comments are stripped before the scan.

**THE RELEASE WAS BLOCKED BY TWO OF MY OWN MISTAKES, AND DIAGNOSING THEM WAS THE WORK.** `pack-chain`
went red because `console-smoke-check.mjs`, added four rounds earlier, runs the jsdom smokes with
`cwd: gateway/ui` in a job whose steps are checkout and setup-node — so `jsdom` could not resolve and all
four failed to LOAD. Reproducing CI's condition (hiding `node_modules` → exactly 0/4) found it; the log
could not have, because the wrapper printed only lines matching `✗|FAIL|not ok` and a smoke that dies
before its first check has none — it now prints the line that NAMES the error. `design` went red on the
`routes-fail` scene with "2 loud elements", which was my own round-51 fix firing BOTH a toast and the
persistent banner: one fact, two mechanisms, added by the pass whose whole subject is removing them.

The release itself then went through cleanly — 1.2.462 audited byte-for-byte against the GitHub asset and
installed on the device — and three earlier audit answers were the first-publish state, as they have been
since 1.2.454.

## 2026-09-24 (the operator's CLI, two new gates, and a compiler that moved)

The tenth exploration opened the operator's own front door — `summrise setup/update/status/rollback`, the
one surface every install passes through — and found the shape this stretch keeps meeting, now in its
purest form: **A SENTENCE THAT SOUNDS LIKE IT SHOULD WORK.**

**`summrise --version` FAILED THE CHECK THE INSTALLER TELLS OPERATORS TO RUN.** Step 4 of
`deploy/README-installer.md` BEGINS with it. It is not a verb, so it fell through to the usage branch,
printed the verb list and exited 1 — a fresh install that had worked reported a FAILURE in the one place
the operator is told to look. Fixed, and the test EXECUTES the CLI: the suite's own comment said every
documented verb mutates the machine, which is why only a bare invocation ran; `--version` mutates
nothing. That is the first real execution test in that file.

**A FAILED UPDATE NAMED THE VERSION THAT FAILED.** `releaseMarkerVerdict` is printed by TWO callers —
`rollback`, staging a release to PIN it, and `update`, staging one to INSTALL it — and it said
"rollback: … re-run `summrise rollback <want>`" for both. `want` is the version that just failed to
install, so the advice asked the device to pin a release it was not running. The verb decides the
sentence now, and the update path offers the version the device is ACTUALLY on.

**THE NO-OP GUARD EVAPORATED WHEN THE NETWORK DID.** The parity guard exists because of round 201's
measured defect (a CLI stamped the install with the version it already had, the swap installed the same
build, and `status` kept pointing at the command that could not help). It compares the CLI against the
RELEASE CHANNEL, so when the CDN is unreadable `latest` is empty and the whole check is skipped — a
network blip re-opens the defect. The same refusal now comes from two facts already on the machine.

**`setup` OVERWROTE A REMAPPED DataDir WITH THE LITERAL DEFAULT.** `resolveDataDir()` is registry-first
and `DATA_DIR` is its answer, but the registry write used `path.join(process.env.ProgramData, "Summrise")`
— while the tree was created at `DATA_DIR`. A remapped device got its new tree at one path and a registry
naming another, which is the path the AGENT reads: the data splits and nothing reports it. The comment two
lines above even said "DataDir defaults to %ProgramData%\Summrise" — a default applies when nothing is
set, not OVER A REMAP.

**THE BUSY-MARKER PATH HAD ONE OWNER IN PROSE AND FOUR IN CODE**, and the agent had already fixed the
same defect on its own side, with a comment that says why: "a drift between the two is invisible until an
update actually runs — and then the swap releases a file the agent never created, the marker survives,
and every later update is refused for up to an hour." The CLI's generated PowerShell carried three
hand-written copies; `busyMarkerPs()` derives the fourth from the one owner, and a contract test holds the
JS form and the PS form to the same file.

**AND TWO FINDINGS TURNED OUT TO BE NEGATIVES WORTH THE WRITING DOWN.** The exploration reported that the
two swap builders both write `update start` so "the receipt cannot say which ran" — the receipt is exactly
what separates them, and `diagnoseUpdate` already relies on it (`cli-swap-launched` vs `rust-swap`), which
is now said in the code. And the busy-marker path WAS consistent; what was false was the claim, not the
behaviour. **What is real behind the second one is behavioural, and is named rather than patched: the
agent's swap script re-stages the boxed components while the CLI's runs the migration gate — neither does
both, and both report success.**

**THEN TWO CI REDS, EACH DIAGNOSED RATHER THAN RETRIED.** The first was mine from four rounds earlier: the
jsdom console smokes were wired into `pack-chain`, a job whose steps are checkout and setup-node, so all
four failed to LOAD. Reproducing CI's condition (hiding `node_modules` → exactly 0/4) found it; the log
could not have, because the wrapper printed only lines matching `✗|FAIL|not ok` and a smoke that dies
before its first check has none — it now prints the line that NAMES the error. The second was not mine:
the freshness gate compiles `src/summrise.ts` with the binary left behind by the electron step ABOVE it,
and that step installed `typescript@5` — a MOVING major — while the committed bin came from 5.9.3. Two
tsc versions emit different JavaScript from identical TypeScript, so the gate failed a commit that was
CORRECT, in both workflows. Three places name that compiler; `build-pins.bash` — whose header already
existed for exactly this shape, "the build inputs that are HAND-COPIED, each with a single source of truth
and NOTHING comparing them" — holds them to one version, and its mutation is in the mutation list.

**THE STRETCH'S OWN GATES CAUGHT THE STRETCH'S OWN COMMITS.** The new mock gate found `lib/api.ts` itself
on its first run (the predicate's doc QUOTES the idiom it warns about — the trap `exports-check` and
`retired-colours-check` each recorded on their own first run); `ledger-budget-check` refused an AGENTS.md
edit of mine that was 72 bytes over its ceiling; and `gate-mutations-check` runs the two gates this
stretch added alongside the twenty-seven that were already there.

**AND THE DURABLE FINDING, after five separate instances:** the code and its instruments drift
independently, and only BREAKING one of them shows it. A security test whose comment had stopped being
true, a doc promising `None` where the code always returned `Some`, a wholesale mock that turned a
`TypeError` into passing silence, an SSE assertion on a header no consumer reads, a help test pinning
whitespace — each was green, and each was wrong about what it measured.

## 2026-09-24/25 (the proxies, four parity gates, and a test that had stopped being evidence)

The eleventh exploration went at `proxies/` — four workers, ~5,900 lines, no pass in eleven rounds. It
found the shape this stretch keeps meeting, and this time the repository had already written the remedy
down without applying it everywhere.

**FOUR PARITY GATES, BECAUSE THE FACTS THAT DRIFT HERE ARE THE ONES HELD IN SEVERAL FILES WITH A COMMENT
ASSERTING THEY AGREE.** The gateway EXPORTS a CORS allowlist (and lets config extend it) while four
proxies restate it; one documented 30s header budget is implemented three ways in seven sites across three
deployment units; five caller-selectable upstream hosts live in a map that only three tests asserted; and
the TypeScript version was named in three places with two values. Each is now a check that reads every end
and fails on disagreement, each is wired, named, and paired with a mutation. The pattern is not a
coincidence: a comment saying "these match" is the signal that nothing does.

**AND THE GUARD NAMED FOR ONE OF THEM READ NO OTHER FILE.** `proxies/api-relay/api/test/proxy-gate.test.mjs:53`
is titled "CORS matrix on the autonomous copy (drift guard vs gateway http.ts)" and imports `../proxy.js`
and no gateway file at all — the copy checking itself. That is the twelfth instance of this stretch's
recurring shape, and the first found by an exploration rather than by mutating something.

**TWO LEAKS, BOTH INVISIBLE UNTIL COUNTED.** `relay.mjs` registered `server.on("error", …)` INSIDE the
request handler, so every request added a listener to the process-lifetime server: Node warns at eleven,
the array grows unbounded, and one real error would print its line once per accumulated listener. It is
registered once in the factory now, and the test asserts the count is one before and after 25 requests.
Separately, `SUMMRISE_RELAY_HEADER_TIMEOUT_MS` is read by three production handlers at module load and was
set in exactly ONE place — a test, which never unset it, so anything imported later in that process would
silently get 120 ms instead of the documented 30 s.

**A TEST THAT HAD STOPPED BEING EVIDENCE.** The `summrise-relay` suite "did not finish in 120 s: 6 passed,
the 7th was cancelled". The cause was one line of harness: `stop` set a flag the poll loop only re-read
after the in-flight 25-second long poll resolved. Aborting that poll instead took the suite to THREE
SECONDS and 8/8 passing — and the seventh test, CANCELLED rather than failing, began running for the first
time. A cancelled test is not a passing test and not a failing one; it is nothing, and it sits in a suite
CI runs.

**A NEGATIVE THE EXPLORATION GOT WRONG, AND WHY THAT IS WORTH RECORDING.** It reported
`server/test/vercel-references.test.mjs` as "a test whose only subject is a DELETED file". It is the
opposite: a live gate asserting that no comment cites `vercel.json` as the authority for the code while the
file is missing, and its header records two lessons of its own — a trailing `\b` that "did not match
'DELETED'", and a round where the scan covered `server/*.mjs` only while the same stale sentence sat in the
README an operator reads. An exploration's SUMMARY can overstate what its evidence shows, exactly as a test
title can, and the remedy is the same: open the artifact.

**AND THE EDIT-TOOL LESSON, AFTER FOUR ROUNDS OF PAYING FOR IT.** Anchors copied from `sed`/`awk` output
that was piped through `sed 's/^/  /'` carry two extra spaces the file does not have. Rounds 99, 104 and
117 each lost an edit to that; round 120 lost SIX attempts to it, including two mutations that planted
nothing and one that broke the syntax. For an edit whose anchor is whitespace-sensitive, the tool that
shows exact text beats a script that decorates it — and a mutation is only evidence once it is proven to
have LANDED, which is why the last two rounds ran theirs before writing the commit message.

## 2026-09-25 (the landing and the CDN, and the difference between a rule and a comment)

The twelfth exploration went at `index/` — the landing page and the release channel every device's updater
depends on — and its first line corrected the premise I had been working from: `ai.saisi.online` is the
GATEWAY console, while `index` serves `agent.saisi.online`. One round earlier I had called the landing the
console's host in a commit message. The report was right and the brief was wrong.

**THE RELAY LIED ABOUT THE BODY IT SENT, AND MEASURING A LIBRARY IS WHAT FOUND IT.** `collectResponseHeaders`
copied every upstream header verbatim while `entry.mjs` relayed an independently-read body. Measured against
Node 24's undici: an upstream answering 10,000 bytes gzipped to 45 reports BOTH `content-encoding: gzip` and
`content-length: 45` in `response.headers` while `response.body` yields all 10,000 DECODED bytes — so the
relay answered 45 declared, 10,000 sent, and `gzip` claimed over bytes that are no longer compressed. A
client trusting either header truncates or fails to decode. Reachable, not theoretical: `forwardHeaders`
passes the CALLER's `accept-encoding` upstream, so a browser asking for gzip is exactly the case. Both
headers are dropped now, and only those two. The test drives a REAL gzip upstream, because the finding is
about what fetch does to a real one.

**A RULE WITH NO INSTRUMENT IS A COMMENT.** `index/public/_headers` states, for the whole
`/summrise-agent/*` prefix, "Device artifacts change under stable URLs — never let the edge serve a stale
body (a device would silently receive an old build)" — and NOTHING read the file. The cloudflared route
answered `public, max-age=3600` inside that very prefix, so the edge could hand out the previous binary for
an hour while `version.json`'s pin had already moved: fail-closed, because `summise setup` refuses the
mismatch, but the install then cannot proceed at all. What made it findable was a SIBLING: the electron test
is named "… and no-cache (not max-age)". The rule existed, was asserted one route over, and was not applied
beside it. Both are now asserted, and the file itself has a test that cross-checks the CODE against it — which
is the half that would have caught the defect the day it was written.

**TWO THINGS NOBODY HAD EVER EXERCISED.** The `CONSOLE_URL` fallback to the request's own origin — the
mechanism behind the header's promise "no production domain is hardcoded here" — was skipped by BOTH
fixtures, which set the var; substituting a production URL now fails with the promise restated as an
assertion. And the prune that keeps five releases per minor matched `summrise-agent-*` ONLY, so the six
installers published while the product was called `vale` were never candidates: measured against the live
CDN, all still answered HTTP 200, including a `-latest` alias pointing at 1.2.451. It sweeps a DECLARED list
of superseded names rather than a loose glob, because a `*-agent-*.tgz` sweep would delete a future
product's assets the day it shares the directory.

**THE LANDING TOLD EVERY VISITOR TO RUN A COMMAND THAT CAN INSTALL NOTHING.** Step 3 prescribed a bare
`npm i -g … summrise-agent`, which AGENTS.md records as able to print `changed 1 package` and leave the OLD
CLI in place — while step 2, on the same page, and both READMEs used the URL form. The paragraph even ended
"not with npm's exit code", so the class was known and the safe form was one line away. And the landing had
NO cache policy at all, unique among that worker's routes: heuristically cacheable, which would have kept
serving the old instruction after the fix. Both are fixed, and the page now revalidates with a body-derived
ETag so a match answers 304 rather than 30 KB.

**THREE DOCS SAID THE INSTALLER WAS RETIRED.** Two READMEs said it; `scripts/build.sh` had already worked out
that it is not, in a comment that says so in as many words — "THE NSIS INSTALLER IS **NOT** RETIRED, and this
comment said it was for long enough that a reader would have believed it". The correction existed in ONE
place and the READMEs never got it. `index/README.md` was wrong TWICE about the same route: it called the
Setup.exe route a redirect to the console (it is served as an asset), and it called the installer retired. It
is built ON DEMAND (`--with-installer`; the default prunes any staged exe and the manifest then carries no
`installer` field — a publication state, not a retirement), and when built it ships the
desktop task and the one-click install, and an absent `installer` field in the manifest is a PUBLICATION
STATE, not a retirement.

**AND A SYMBOL I ALMOST DELETED.** Running the deletion test on a 155-line block of upload helpers the D6
cutover left behind, `SHA256_RE` sat inside it and has THREE LIVE USES below. My scan reported "6 hits outside
the file" and I read them as other files' copies — they are the relay's own, and the symbol is used HERE.
Removing it would have thrown ReferenceError on every `/api/version` request carrying a pin, and `npm test`
would NOT have caught it: 43 tests passed before and after, because none of them carry a pin. A SCAN OF USES
IS NOT A SCAN OF DEFINITIONS.

**AND THE MUTATION RULE, WHICH THIS STRETCH KEPT RE-LEARNING.** A mutation is evidence only once it is
proven to have LANDED on the thing under test. Four variants cost rounds here: an anchor copied from `sed`
output decorated with two extra spaces; a plant that succeeded but landed AFTER an early return, so the test
never reached it; a plant that broke the syntax instead of the behaviour; and a replace that hit the FIRST of
three identical lines at two different indentations instead of the one at the route under test. Each time the
mutation "passed", and each time the mutation was the thing that was wrong.

## 2026-09-25 (the installer path: three claims that contradicted their own code)

The thirteenth exploration took `agent/deploy/` — the one subsystem thirteen rounds had never opened — and
its first finding was that the documentation disagreed with the code about the code's own subject. It is a
small path by line count and it holds three of the sharpest defects this stretch found, because everything
in it runs once, on a stranger's machine, with no CI able to reach it.

**THE COMPONENTS WERE MEASURED, NOT VERIFIED.** The installer downloads cloudflared (54 MB), the playwright
bundle (31 MB) and the electron runtime (115 MB) into the npm package directory, and `summrise setup` then
takes them BY PRESENCE — `resolveComponent` returns a local path on mere presence — so the manifest pins
those components carry were never consulted on this path at all. The acceptance was `Length -gt 1MB`, three
times, and for the electron zip the next step is `Expand-Archive` and a copy into `dist\`: 115 MB of
executable payload installed on a file size. AND THE TOOLS WERE ALREADY IN THE IMAGE — `SummriseIntegrity.ps1`
ships `Get-ManifestSha256`, `Get-FileSha256` and `Test-FileSha256`, the installer dot-sources it, and it
already verified the TGZ with them. The components simply never asked.

**THE FAILURE DIALOG SENT THE OPERATOR TO A FILE THAT NEVER EXISTS.** It said "look at
$INSTDIR\installer.log"; the transcript goes to `%ProgramData%\Summrise\logs\installer.log`, and the NSIS
script already knew the right root because it passes `-ResultFile "$3\Summrise\logs\install-result.txt"` on
the line above the run. Worse: `Start-Transcript`'s failure is swallowed (`catch { }`), so on a locked
profile the old text sent the operator to a log that had never been created, with no way to tell that from
"the log says nothing".

**A FAILED INSTALL COULD NOT BE REMOVED.** `WriteUninstaller` and the Add/Remove registration came AFTER the
`${If} $0 != 0` / Abort, so a failure left everything the setup script had already done — Machine PATH among
it — with no uninstaller and no entry in Add/Remove Programs. The dialog's advice (re-run; it is idempotent)
is true, but it is not the only thing an operator may want to do. Both are written before the step that can
fail now, and the uninstall section already tolerates a partial install.

**THE LOGON TASK HAD TWO OWNERS AND THE WEAKER ONE WON.** `summrise setup` registers `SummriseDesktop` with
an Interactive/Highest principal and settings (a 10-minute limit, `IgnoreNew`); the installer's step 6
registers it TOO, passes neither, and — because step 6 runs AFTER setup — its definition is the one that
survives. Every NSIS install silently downgraded the hardened definition. The comment above it claimed 形态抄
update 流的 hardened 版 ("the shape is copied from the hardened version"). It was not copied.

**A SCRIPT THAT COULD NOT RUN, AND TWO DOCUMENTS THAT STILL NAMED ITS TARGET.** `build-linux-xwin.sh` built
`--bin summrise-command`; `Cargo.toml` declares one bin, `summrise-agent`. Nothing live referred to the
script, which is exactly why nobody noticed it was broken — and `gateway/DEVICE-INTEGRATION.md` and
`cloudflared-config.example.yml` still described a `summrise-command` binary to operators and to the console.

**TWO RE-RUN COSTS.** The setup script probes `Get-Command node` — which sees only the CURRENT session's PATH
— and extends that PATH forty lines later, so a repair run deleted a working `components\node` and
re-downloaded ~30 MB. And the prune that keeps five releases per minor matched `summrise-agent-*` ONLY, so
the six installers published while the product was called `vale` were never candidates: measured against the
live CDN, all still answered HTTP 200.

**AND A REPORT'S SUMMARY IS NOT EVIDENCE — TWICE.** This exploration reported the NSIS installer as "built
every release" (it is OPT-IN: `WITH_INSTALLER=0`, and the default PRUNES any staged exe), and
`fix-tunnel.ps1` as having "no producer and no consumer" (the agent RUNS it, the CLI migrates it, and
`paths.rs` maps it — it is the legacy-tunnel repair path). Both summaries were directionally right and wrong
in the detail, and acting on either without checking would have made things worse: round 129 wrote a FALSE
claim while correcting a true one, and round 146 nearly deleted a live repair script. The rule that comes out
of it is the same one the mutation discipline teaches: verify the claim against the code that decides it,
not against the summary that describes it.

**AND THE HONEST LIMIT.** Every `runs-on` in `ci.yml` is `ubuntu-latest`. A 326-line PowerShell install path
is therefore exercised only through pure-function tests on pwsh and text-offset pins in
`agent/tests/installer_integrity.rs` — which now hold five separate ORDER guarantees, because order is what
these defects had in common: dot-source before manifest before verify before install; components verified
before they are kept; the uninstaller written before the step that can fail; the reuse check before the
download; and both task definitions agreeing on their values.

## 2026-09-25/26 (the installer and the core crate, and six report details that were wrong)

Seventeen rounds, two explorations, and one design pass. The pattern worth recording is not any single defect
but how the findings arrived: an exploration would report five things, and CHECKING EACH ONE before acting
found that roughly a third were wrong — always in the direction of worse than reality. Acting on the summary
would have made things worse twice and deleted a live repair path once.

**THE INSTALLER, WHERE EVERY LINE RUNS ONCE ON A STRANGER'S MACHINE AND NO CI CAN REACH IT.**

*Components were MEASURED, not verified.* The installer downloaded cloudflared (54 MB), the playwright bundle
(31 MB) and the electron runtime (115 MB) into the npm package dir, and `summrise setup` took them BY PRESENCE
— so the manifest pins those components carry were never consulted on that path. The acceptance was
`Length -gt 1MB`, three times, and for the electron zip the next step is `Expand-Archive` and a copy into
`dist\`. The tools to close it were ALREADY IN THE IMAGE: `SummriseIntegrity.ps1` ships the sha256 functions,
the installer dot-sources it, and the tgz was already verified with them.

*A failed install could not be removed.* `WriteUninstaller` and the Add/Remove registration came AFTER the
`${If} $0 != 0` / Abort, so a failure left Machine PATH written and no way to uninstall. The dialog's advice
(re-run; it is idempotent) is true but not the only thing an operator may want.

*The logon task had two owners and the weaker one won by ORDER.* `summrise setup` registers `SummriseDesktop`
with an Interactive/Highest principal and a 10-minute limit; the installer registered it too, passed neither,
and ran LAST. Every NSIS install silently downgraded the hardened definition, while the comment above it
claimed the shape was "copied from the hardened version".

*The CDN arm named a version it was not pinned to*, and the prune could not clean up after a rename: six
installers from the product's OLD name were still answering HTTP 200 months later, because the glob matched
the current name only.

**THE CORE CRATE, WHOSE INTERFACE EVERY PLUGIN AND THE CONSOLE DEPEND ON.**

*The boot warning called an operator's own key a typo.* `unknown_key_warnings` hand-lists every config key and
warns "IGNORED by the agent (typo? check docs; will never take effect)" — and `server` omitted `relay_url` and
`relay_token`, which the relay client READS, plus the legacy `auth_token` alias serde still honours. The test
NAMED after that property could not see it, because its fixture never wrote those keys.

*Two accessors disagreed about a shared name.* `find_tool` is last-wins through a HashMap; `plugin_tools` was
FIRST-wins through `.find()`; `all_tools` published both. A duplicate plugin name — which nothing warns about
either — would have made the listing and dispatch name different owners.

*`BrowserConfig` had no reader*, and deleting it needed three test assertions removed, a fixture in the
warning-coverage test removed, and a `SECTIONS` blessing removed. The compiler found the first, the test named
after the property found the second, and clippy found that the third test then asserted NOTHING after its
`unwrap()`.

**THE SWAP-SEAM DESIGN PASS REFUTED ITS OWN BRIEF.** I asked for two designs for "the CLI and the agent build
different swap scripts". The pass verified the claim first and found that the component staging it described
reads files out of a tgz that has never contained them — nine entries, neither file — so `tools.rs:805-806`
were dead lines and the comment above them named a guarantee the code could not deliver. The real divergences
were elsewhere: the CLI's migration gate, the desktop-shell swap, and the receipt. It also found a live hazard
the brief had not mentioned: the agent's swap repoints the boot task at `etc\config.yaml` and then kills the
agent, with no check that the file exists, where the CLI gates and aborts.

**AND THE SIX WRONG DETAILS, WHICH ARE THE POINT.** "The operator's file is gone" (it is quarantined, logged
to stderr and the startup log, and token-preserving across four rounds of hardening). "No producer and no
consumer" for `fix-tunnel.ps1` (the agent runs it, the CLI migrates it — it is the legacy repair path).
"The clamp is silent" (the response echoes the value it used). "Ten browser variants constructed nowhere"
(two are, in tests). `install-panel-url.txt` was a documented decision, not a duplication. "Built every
release" (it is opt-in, and the default PRUNES it). And "nothing reads the TS side" — `agent/tests/gateway_code_contract.rs`
exists for exactly that drift, states the problem in its own header, and parses BOTH files so the assertion
cannot agree with itself. That report read `error.rs` and `mcp-errors.ts` and never opened `agent/tests/`. Each was checked before acting; two would have become
new defects, one would have deleted a live path. AN EXPLORATION'S SUMMARY IS NOT EVIDENCE — check the claim
against the code that decides it, and expect a third of the details to need correcting.

**AND THE MECHANICAL LESSONS, WHICH KEPT RECURRING.** `cargo fmt --check` is a CI step and skipping it reddened
a run for five rounds. The shell mangled five one-liners (an apostrophe, a backslash, a backtick, a JS string
continuation) — write the script to a file and run that. A mutation is evidence only once it is proven to have
LANDED: my guards caught four that had not, and each time the "passing" mutation was the thing that was wrong.
And comments-are-not-code bit four times, twice inside the tests written to prevent exactly that.

## 2026-09-26 (the gate suite audited, and an instrument that cannot fail)

The fifteenth exploration did what none of the fourteen before it had: it audited THE INSTRUMENTS. Every
prior pass had asked whether the product was right; this one asked whether the gates could tell. It found ten
things, and the shape of them is the point — a suite built to catch vacuity had vacuity of its own, at every
level, from the scan that could read zero files to the checker that accepted a refusal as proof of a bite.

**THE SCAN THAT READ NOTHING AND SAID OK.** `script-syntax.bash` walks `git ls-files '*.sh' '*.bash'`. When git
refuses the tree — a dubious-ownership / safe.directory refusal in a container does exactly that — the list is
EMPTY, the loop runs zero times, FAILED stays 0, and the gate printed `ok: script-syntax 0 files parse` with
EXIT 0. `set -euo pipefail` cannot catch it, because the failure happens inside a process substitution, which
the shell does not check. It has a floor now, and the proof is the report's own: on a copy with `.git` removed
it exits 1 with "read only 0 file(s), expected at least 20 — the scan is reading the wrong thing".

**THE DERIVATION THAT COULD NOT SEE A GATE CI RUNS.** `all-gates.bash` promises its list "can never become a
second list that drifts from the first", and its pattern required the script path to start immediately after
the interpreter. Two gates escaped it from opposite sides: one invoked as
`node ${{ github.workspace }}/scripts/test/console-assets-check.mjs` (the prefix contains a SPACE, so widening
the character class did not help — the strip has to run first), and one living OUTSIDE `scripts/test/` entirely,
where `build-pins.bash`'s wiring check — which iterates that directory — never asked about it either. Both are
in the local run now, and the moved file needed exactly one line changed with it, its `ROOT`.

**THE RATCHET THAT HAD GONE SLACK.** `spacing-scale-check.mjs` is a one-way ratchet: it fires when off-scale
uses RISE above a baseline and when token uses FALL below it. The baselines were taken in rounds 220-222; the
counts had since IMPROVED to 299 off-scale and 397 token, so the gate silently tolerated SIX new off-scale
literals and twelve lost token uses. Tightened, and the mutation was run BOTH ways — one planted `13px` now
fails with "rose from 299 to 300", and restoring the old 305 makes the same tree exit 0. That is the six units,
reproduced.

**EXIT 2 WAS DOING TWO JOBS.** The runner documents 2 as "this host cannot run me" and maps it to `n/a` WITHOUT
counting a failure. Three gates spent it on "I ran and my patterns went stale, so I measured nothing" — and all
three PRINT "FAIL" first, so the runner printed `n/a` over a line that said FAIL and could exit 0 with three
gates having proved nothing. The missing verdict was the fourth one: it PASSED, it FAILED, it COULD NOT RUN
HERE, and it RAN AND COULD NOT MEASURE. That is 3, and it increments `fail`.

**AND THE CHECKER THAT PROVES THE OTHERS BITE.** `gate-mutations-check.mjs` tested `after === 0`, so ANY
non-zero counted as proof — a refusal (2) or a stale-pattern verdict (3) would have been recorded as a bite
without biting. Theoretical when the report found it, LIVE by the time it was fixed, because the round before
had introduced 3. It also printed `${CASES.length} gate(s) broken on purpose` — 31 — for 19 distinct gates.
Both fixed; the full run now reports "31 case(s) over 19 gate(s) … every one of them bit".

**TWO GATES THAT NOBODY RAN, ONE FROM EACH SIDE.** `console-assets-check` (prefix) and
`custom-prop-check` (wrong directory) were invoked by CI and invisible to the local runner. The second was
moved into `scripts/test/` rather than widening two globs, because the directory IS the convention and two
checks key on it.

**AND THE PANEL SHEET HAD NOBODY COMPARING IT TO ITS SOURCE.** Five gates read the COMMITTED
`agent/resources/panel/panel.css` — chrome-stillness, feedback, motion, state-colour, stylesheet-hygiene — and
`agent/build.rs`'s guard is an MTIME comparison, which misses a restored file or a same-second content drift;
CI's `panel` job runs `npm run build` and throws the result away. The console already had the right instrument:
`console-assets-check.mjs` rebuilds and asserts the tree does not move, and its own header names the asymmetry
("the PANEL has a guard for exactly this"). The panel has the same check now, copied deliberately, including
its refusal to run on a dirty tree — which it demonstrated immediately, refusing because the new file was
still untracked. Its failure message names the five readers and says the diff IS the fix.

**AND EVERY JOB WAS UNBOUNDED.** Nothing in `ci.yml` set `timeout-minutes`, so all eleven jobs ran against
GitHub's 360-minute default; a hung gate would spend six hours of runner and report a timeout instead of the
gate's name. All eleven are bounded at 45 — about 3x the longest run measured here (27 minutes, `pack-chain`,
which executes the gate scripts one at a time). A job that exceeds that is not slow, it is stuck.

**AND THE DEPLOY THAT HAD NOT HAPPENED.** Round 169 measured the LIVE landing and found it still serving the
bare `npm i -g` instruction round 128 had removed from the source, with no cache-control at all — three rounds
of committed fixes sitting undeployed. `./scripts/build.sh index` shipped them, and the live page was verified
afterwards rather than assumed: 0 occurrences of the bare name, the URL form prescribed, the warning present,
`cache-control: public, no-cache`. A committed fix is not a delivered one, and the only evidence is the live
response.

### Round 144's story, moved out of AGENTS.md (round 182)

AGENTS.md is capped at 48,000 bytes and sat ten bytes under it, so the row the table was missing — the
desktop shell's suite, which `all-gates.bash`'s own header names among the commands that runner does NOT
cover — could not be added without freeing space. The rule stays there (run the command the other end runs);
the story lives here, because a sentence that does not change what you would DO belongs in the long form.

WHAT IT COST. Six type errors passed a local `tsc --noEmit` in `gateway/ui` and failed CI, because the `ui`
job runs `npm run build` — `tsc -b && vite build && prune-stale-assets`, a PROJECT-GRAPH build rather than a
single-file check. The errors were real and CI was right; the local command was simply a different question.
That is why AGENTS.md carries the table it does, and why the table now lists five directories instead of four.

**AND SIX TEST STEPS NOW ASSERT THEY RAN SOMETHING (round 192).** `scripts/test/npm-test-floored.mjs` wraps `npm test`
for the six CI steps whose package uses `node --test`: measured, that runner prints `tests 0 / pass 0` and EXITS 0, so
renaming a test file would have silenced any of them. It reads `ℹ pass N` (Node 24), `# pass N` (Node 20) and
`Tests  N passed` (vitest), and a suite whose output carries NO count is a failure rather than an excuse. Mutation:
an empty package with `node --test` exits 1 with "the suite ran 0 test(s) and exited 0". The floors the gate scripts
already carried (all-gates FLOOR 40, script-syntax FLOOR 20, console-smoke floor 4) now exist at the step too.
**TWO GATES WERE NAMED ONLY IN THE INVENTORY UNTIL ROUND 200.** `proxy-cors-parity-check` (the gateway EXPORTS
one CORS allowlist and four proxies restate it, so a disagreement is a browser error on one surface and not
the other) and `proxy-timeout-parity-check` (the README states ONE 30s header budget while seven sites in three
deployment units implement it). Both are wired in `ci.yml` and were reachable by the name census only through
`docs/agents/inventory.md` — a checkpoint whose narrative is 70 KB and hundreds of rounds old, which is not
"somewhere an operator reads". They are named here because this section is where the mutations live.
## Which mutation must fail which gate

MOVED OUT OF `AGENTS.md` IN ROUND 187. It was 37 rows and 31 KB — **68% of the instruction file**,
whose own paragraph states the rule this broke: gate evidence belongs in the ledger, because
`AGENTS.md` is the file that gets truncated when it grows. The RULES stayed there; the evidence is here.

`scripts/test/gate-mutations-check.mjs` automates the mutations that can be automated (31 cases over 19
gates, run on every push — a build with any of them planted must fail with exit 1). The rows below are the
rest: each was performed by hand, once, and the result recorded. Read a row when you change the gate it
names — the mutation is how you find out whether the gate can still fail at all.

AND THE MOVE ITSELF WENT WRONG FIRST, which the lead-in above should record: the extraction stopped at the
first line not starting with `|`, and some cells contain embedded newlines, so it moved 16 rows and left 27
orphaned in `AGENTS.md` under no header at all. Every gate passed — none of them checks that a markdown
table is well-formed. The boundary is now 'to the last `|` line before the next `## ` heading'.

| gate | mutation that must fail it | result |
|---|---|---|
| `scripts/test/token-contract-check.mjs` | change a shared token's value on one side, or DELETE a spacing step from one of them | exit 1 both ways: "1 of 41 shared tokens DIVERGE" for a disagreeing value, and "console does not define --sp-3" for a deleted step — the second case is why the spacing block outlives the shared-name comparison, which by construction cannot see a name only one side still declares (round 243) |
| `cargo test --features terminal,keyring spec_snapshot` | add a parameter inside a device tool's `properties` | exit 101, snapshot diff |
| `scripts/test/panel-audit-skip-check.mjs` | make the audit `exit(0)` on a skip, OR delete the `PROBE_SOURCE` import its measuring half evaluates, OR put its old local `cr < 4.5` back | exit 1 each way: the first names the skip/pass distinction; the other two read the SOURCE, because that half runs only where a browser exists — and it had never run anywhere (CI takes the emit path, which exits 2 first) until round 265 imported the probe it was calling by a name that no longer existed |
| `agent/tests/fixtures/approval-grants.json` | rename a member the panel mirror reads | both sides fail |
| `agent/tests/fixtures/session-row.json` | rename `idle_ms` to `idleMs` | device + panel fail |
| `agent/src/plugins/update/tools.rs` (the launch record, round 18) | make `last_update_attempt` return an empty object for a body it cannot use, or drop `at_ms` from the record | `cargo test --features terminal,keyring` fails: the record is what tells the panel whether an update STARTED, and an empty object renders as "never launched" — the panel refuses the same shapes again (`parseAttempt`), because the two ends drift independently |
| `agent/tests/fixtures/embedded-bridge.json` | rename `fwd` to `forward` | shell + panel fail |
| `scripts/test/panel-design-sweep.bash` | plant a defect per axis in a report (82 checks as measured on 2026-09-22 — the axis list below is the part that stays accurate, the NUMBER is what drifts when a round adds a case without updating this cell: contrast, h1, skip, landmark, geometry, sliver, loud, mark-collision, name, title-only, reflow, focus, focus-empty, motion, motion-empty, type-floor, blind, theme-lie, harness-stale, **prose** — the newest, round 265: a planted 206-character line, plus `prose-none` for the instrument's own floor — plus the note assertions the axis loop cannot make) | one check per axis (the newest being `false-claim`: a surface claiming a read failed while the fixture answered everything — the defect rounds 99-100 found by hand, twice; it applies only to a report that DECLARES what its fixture served (`sse` records), because the console has no backend and its "could not be read" is true)  **MARK-COLLISION (round 27): the silhouettes are now checked AS THE BROWSER PAINTS THEM.** The sheet-level unit tests cannot see a cascade override — round 25's `.plug-dot[error]` kept a stray halo through a test that passed — so the SURFACE probe reads the COMPUTED style of every state mark on every page, groups by family, and fails when two states of one mark paint identically. It cost no call sites: the probe every page already evaluates carries it. Verified on three rendered pages: four and five families each, ZERO collisions and no false positives |
| `scripts/test/release-lib.bash` | prune keeps 4 instead of 5 per minor | exit 1, actual/expected listed |
| `scripts/test/smoke-index.bash` | read the versioned installer instead of the versionless alias | exit 1 |
| `scripts/test/smoke-helpers.bash` | accept a truncated sha256 | exit 1, prints the offending value |
| `scripts/test/release-audit.bash` | stop recording mode drift | exit 1 |
| `scripts/test/publish-release.bash` | disable the stale-exe refusal | exit 1 — **after round 67 ADDED the case that does it** |
| `scripts/test/build-pins.bash` | bump rust-toolchain's channel alone | exit 1, names the workflow literal |
| `scripts/test/script-syntax.bash` | append an orphan `fi` to a shell script | exit 1, with file and line |
| `scripts/test/press-anchor-check.mjs` | read the press baseline BEFORE the hover (put `const hovered = await styleOf(sel);` back above `page.mouse.move`), or add `width` to `pressDelta`'s key list | exit 1 both ways: "the baseline is read BEFORE the hover (move@…, hovered@…) — that is the resting anchor this check exists for", and "layout properties are not a press response". It pins the rule as a pure function AND the wiring in all three EMITTED artifacts, because a probe measured against rest calls every hover a press — that is how the landing's theme toggle passed for as long as it existed. The first version of the wiring assertion checked only "hovered before down" and PASSED the mutation (round 95). **AND IT REACHES EVERY CONTROL A PAGE RENDERS (round 15)**: it scrolls an element into view only when it is not
already fully visible (minimum movement), presses the VISIBLE part of the rect it clamped into the viewport, carries
`reached` (a press the pointer never delivered is a note, never a dead-control finding), and ASKS THE DOM for the
controls — deduped by class+size, chrome skipped, capped, with `found` reported so the judge's floor is
`min(2, found)` rather than a constant (the harness's Browser page is an explanation with ONE control). Measured on
the device: 158 presses, ZERO dead, panel-Settings 16/16, panel-Browser 0/0. `press-anchor-check` pins the emitted
pass; `panel-design-sweep.bash` plants both judge directions. **AND THE ACKNOWLEDGEMENT'S LATENCY IS MEASURED (rounds 19-20)**: the fixture can delay every reply (`?slowms=N`) and `ackPass` times the press-to-first-visible-acknowledgement gap against a stated 100ms budget, discovering its own controls where every button does device work (Memory: 6/6, 4-9ms via=disabled) and keeping the curated pair where the work is known (Settings monitors: 5-11ms via=data-busy). A row that asked the device NOTHING is a note, never a silent-control finding — the first survey accused three innocent connect-tab controls, and the request-count excuse does not work on a page that polls. **AND THE REVEALED STATE IS MEASURED ON PURPOSE (round 16)**: the pass parks the pointer at (2,2) when it finishes, and `revealPass(page, rowSel, targetsSource, label)` hovers a row, measures the targets, and parks again — so a control that exists only while its row is hovered is measured every run instead of once by luck. Measured: `panel-Terminal reveal` checks 32 targets against 29 at rest, and the row actions report `nearest=26px passes=true`. Target-size findings name the page AND the state (`panel panel-Terminal reveal`). THE HISTORY, kept because the shapes recur: an earlier attempt at this was REVERTED (it reddened CI three times — a helper called but not embedded, an unconditional centring scroll that moved the page it was measuring, and a refusal that emptied the pass), and the cause of that CI failure turned out to be the floor itself, not the pass: see `docs/agents/design-ledger.md` under "THE MYSTERY WAS IN THE OUTPUT I HAD ALREADY COLLECTED". **AND THE EMITTER NAMES WHAT IT BORROWS (round 103)**: every helper the sweep embeds by name must be DEFINED in the emitted text, or `--emit` exits 1 — because a new helper was called and not embedded, every local gate passed (they JUDGE a planted report and read the emitted TEXT; none RUNS the artifact) and CI died with "FATAL … is not defined". Mutation: add a name to the embedded list that is not defined and `--emit` exits 1. **AND THE SCROLL/REACH WORK WAS REVERTED ONCE, THEN RE-LANDED WITH THE CAUSE FOUND (rounds 103 and 15)**: the first attempt reddened CI three times (a helper called but not embedded; centring every element SCROLLED THE PANEL OUT FROM UNDER the pass; refusing unreachable controls EMPTIED it). The CI failure that ended it was the FLOOR, not the pass: it fired on the harness's Browser page — an explanation page with exactly ONE control — and the label `panel/light` hid which iteration produced it. Both are fixed (label names the iteration; floor is `min(2, found)`), and the finding the work started from stands: `.device-logs-toggle` had `cursor:pointer` and neither a hover nor a press, and the suite now discovers and holds it |
| `scripts/test/contrast-probe-check.mjs` | remove the probe's hex handling, or delete `svgRootPaints`'s `painted.has(...)` guard | exit 1 both ways: "both spellings must parse"; and the brand mark's `rgb(0,0,0)` — the INHERITED default on an SVG root whose shapes each declare their own paint — is reported as its colour again. Those rows were TEN of a red design job in round 94, and they pushed the one real defect in that run off the end of the report |
| `scripts/test/e2e-only-check.mjs` | make the zero-selection guard exit 0 | exit 1, "reported success having run nothing" |
| `scripts/test/scan-dups-check.py` | stop recognising `*_test.rs` files | exit 1, names the file |
| `scripts/test/model-drift-check.mjs` | remove the normaliser's bracket-suffix strip | exit 1, prints the un-normalised id |
| `scripts/test/css-vars-check.mjs` | delete a `--token` definition its own sheet references, OR a token the CODE reads at runtime, OR (round 20) DECLARE a token nothing reads (`--token-nobody-reads`) | exit 1 either way. The runtime half was added in round 235 after both `particles.ts` files were found reading the retired `--aura-*` palette; its floors caught the first version of that scan reading ZERO references in 117 files. **IT ALSO CHECKS THE OTHER DIRECTION NOW** — a DECLARED token that nothing reads — which found twenty-one across the panel and the console (dead weight in the one block a reader consults to learn what the palette IS) and now reports "--token-nobody-reads" for a planted one. Its first run in that direction reported two tokens that exist only as PROSE in comments, because the `defined` set was built from raw text: comments are stripped first now, the same lesson `retired-colours-check` records from its own first run |
| the three exemption lists (rounds 21-24) | stop recording usage, or delete the `dormant` support | the runner prints how many entries matched nothing, with the size of the search: `DECORATIVE` against rows, `ignore` against findings, `implicitStates` against on-screen class names. It found a stale waiver (`/^div\.rail-dot$/`, 0 of 6,876 rows since a selector changed) and a stale class declaration (`serial`, whose markup became `data-key`-driven) — both PRUNED; a third was a guard and declares itself `dormant` |
| `scripts/test/stylesheet-hygiene.mjs` | plant an ORPHANED `*/` outside any comment (round 97), or the round-87 shapes (a comment holding a rule-like selector line, or declaration-shaped prose) | exit 1: "ORPHANED '*/' outside any comment — the browser reads it as a parse error and drops rules until the next '}'". The walk finds `/*` and takes the NEXT `*/`, so a stray close used to be skipped as ordinary text while the BROWSER discarded everything up to the next `}` — one stray close silently deleted a large region of the panel's sheet (marks lost their size, session rows lost `display: flex`) and every gate stayed green. The clause checks the GAPS between the comment spans, which is exactly where the walk cannot look |
| `scripts/test/panel-sheet-freshness-check.mjs` | commit a sheet `agent/resources/panel-react` would not produce, or move its CI step to a job that installs no panel-react dependencies | exit 1 both ways: "a rebuild of the panel changed tracked files, so the COMMITTED sheet is not what the source produces" — and the diff it leaves IS the fix. Round 190's placement bug was the second case in reverse: the step sat in `ui`, which installs only gateway/ui's dependencies, and the check faithfully reported "the panel build itself failed" for a missing node_modules. Round 191 added exit 2 for a host that cannot rebuild at all, which is how `all-gates` runs it. **NAMED HERE IN ROUND 199 BECAUSE THE CENSUS COULD NOT SEE IT**: that census matched only an invocation written as `node scripts/test/...`, and this step carries the path-prefix form, so it was invisible — the same trap `all-gates.bash` hit before round 170 |
| `scripts/test/harness-fixture-check.mjs` | change the harness fixture's session-count default from 3, or stop `?exitfail=1`/`?exitok=1` from setting `last_exit_code` | exit 1, and its own self-test fails first if the pattern goes stale. The second pair is round 96's rule: A STATE WITH NO SURFACE CANNOT BE MEASURED — every seed reports no exit code, so without a flag the chip has no rendered surface anywhere, and the mutations are "the failure flag stopped setting the code" and "exit ZERO stopped being expressible". **AND THE `/api/boots` ENVELOPE (round 99)**: the stub omitted `ok: true`, which the hook requires, so the Restarts card rendered "The device did not answer" on eight surfaces while the device answered perfectly — a fixture whose failure mode is a FALSE CLAIM, not an empty card. Mutation: strip the envelope from the emitted stub — and, since round 101, WIDEN THE ARCHIVE STUB BACK to `u.indexOf('/api/sessions') >= 0`, which shadows the per-session stub below it (the audit trail) and makes the Trajectory and Path views read an empty archive as if it were the trail. Also since round 100 the envelope is STRUCTURAL (`J()` merges `ok: true` into every object body that does not bring its own, so a fixture cannot forget it), with its own mutation in the same gate: remove the merge |
| `scripts/test/spacing-scale-check.mjs` | add a spacing declaration with an off-scale literal (a planted `13px`), or an ON-scale one where a token exists (a planted `8px`) | exit 1 either way: "off-scale spacing rose from 299 to 300", or "on-scale literals rose from 0 to 1 — a value that HAS a token was written out by hand, which is how 234 of them accumulated unnoticed". A token use replaced by a literal fails the third count |
| `scripts/test/landing-check.mjs` | lighten a label (the tertiary `#71717a` → `#9a9aa2`), take the heading away (the `<h1>` back to a `<div>`), or give a card a fixed width (`.card { width: 480px }`) | exit 1 for each: "2.68 on #fafafa, under the 4.5 AA wants for text" · "expected exactly 1 h1, found 0" · ".card { width: 480px } — wider than the 320px a 1.4.10 reflow test uses". It reads page.js's own values and BOTH themes (renamed from landing-contrast-check in round 242: a name covering a third of what it does is the kind that stops the next reader looking) |
| `scripts/test/harness-fixture-check.mjs` (the slow-network flag, round 19) | stop applying the delay (`SLOW ? p.then(SLOWLY) : p` → `p`) | exit 1 — the fixture must be able to make every reply slow, or a control whose feedback waits for the network is indistinguishable from one that answers on the event |
| `scripts/test/feedback-check.mjs` | drop a press state the sheet had (`.tab:active`), or transition a layout property, or write `transition: all` | exit 1: "no :active for .tab:hover — a hover that answers and a press that does not is the feedback gap this checks for". It reads BOTH sheets — the panel's built sheet and the console's SOURCE sheet, each with its own floors (round 54) — skips reveal rules (`.tab:hover .tab-export` is not pressable), treats a press on a base as covering its variants (`.btn:active` answers `.btn.primary`), and its first run misread the sheet twice — a `:not(...)` suffix and a multi-line selector list — which is why it names what it could not read  **MEASURED AS RENDERED, WHERE ONE CONTROL HAD NO PRESS AT ALL (round 51).** This check proves an `:active` RULE EXISTS; it cannot see whether the press is VISIBLE, which is what the objective actually asks for. The first rendered measurement — `mouse.down()` on each control, comparing the computed transform / opacity / background — found every control answering EXCEPT the session tab, and only the ACTIVE one: `.tab.active` and `.tab:active` are both (0,2,0), so the later rule wins and the tab an operator presses to re-focus it did nothing at all. Fixed with `transform: translateY(1px)`, which a background rule cannot override, and it is what every other control already used. RENDERED AFTER: all three tabs report PRESS RENDERS. The measured set, for the next reader: `.rail-btn` 38x38, `.tab-close` 24x24, `.side-add` 30x21, `.btn` 54x26 and `.tab` 96x34 all answer — and a sheet-level check would have passed every one of them either way. **AND THE CLASS IS NOW AUTOMATIC (rounds 52-53): A PRESS WHOSE EVERY PROPERTY IS SHADOWED CANNOT BE SEEN.** The check groups every `:active` rule by the element it presses, collects the declarations the group makes, and fails when a LATER rule that names the same element at equal-or-higher specificity overrides every one of them — which is what killed the active tab's press. It found FOUR more, all rows whose own `:hover` rule sets a different background: `.side-row`, `.run-strip-head`, `.archive-row`, `.path-attention-row`. Hover ALWAYS co-occurs with active, so at equal specificity the hover rule won every time. Fixed with `transform: translateY(1px)`, the vocabulary every other press already uses; `.side-row` was measured DEAD in round 52 at 232x34 and reports `none -> matrix(1, 0, 0, 1, 0, 1)` now. IT SHED THREE CLASSES OF FALSE POSITIVE FIRST, and each is a way this kind of check lies: substring matching (`.btn-ghost` contains `.btn`, so `.btn`'s healthy press was reported shadowed), DESCENDANT selectors (`.cmd-btn.cmd-toggle.open svg` names the class while styling a chevron), and same-value rules (a later rule saying the same thing is not a shadow). And a fourth lie was MINE, not the check's: my first mutation removed a selector from a list that no longer had it, my `assert` looked for a string that was never expected to exist, and the gate rightly passed a sheet that had not changed — caught by grepping the BUILT sheet for the rule instead of trusting the edit. A floor of 20 presses and a count in the summary line  **AND THE CONSOLE HAD NO PRESS CHECK AT ALL UNTIL ROUND 54.** This gate had only ever read the panel's sheet. Pointed at the console's source it found 25 `:hover` selectors and ONE press rule: TEN controls — the rail button, the avatar, the logout item, the icon button, the language button, the auth tab, `.btn-dashed`, `.card-link`, `.dev-mini` and every link — acknowledged a hover and stayed completely still under a press. Fixed with a press layer that respects what each element IS: `transform: translateY(1px)` where it renders, `scale(0.96)` for the avatar whose hover is a scale, and OPACITY for `a` and `.card-link` because A TRANSFORM DOES NOT MOVE AN INLINE BOX. It also found six `transition: all var(--ds-dur)` rules the console had never been checked for — each now names the properties that element actually animates. AND THE VARIANT EXEMPTION IS MEASURED NOW, NOT ASSUMED. The panel writes `.btn.primary` (dot form) so a press on `.btn` provably covers it; the console writes `className="btn btn-primary"` (two classes, dash form), where the same is TRUE but no prefix rule can know it. The check reads the markup and grants the exemption only where both classes appear on one element — which is why `.btn-dashed`, used ALONE in this console, was reported as a real gap instead of being waved through with its siblings. Mutation: removing `.lang-btn:active` fails with "console: no :active for .lang-btn:hover". **AND THE LANDING IS THE THIRD SHEET, WHICH IS WHERE THE ROUND-95 DEFECT WAS HIDING (round 95).** It reads the panel's built sheet, the console's SOURCE sheet, and now the landing's `<style>` block CROPPED OUT OF `index/src/page.js` (a third kind of file this check had never opened; a missing block is FATAL, not a silent zero). That is how `.theme-toggle` — a hover rule and no `:active` at all — was found: the rendered pass could not see it either, because it measured the press against REST. Mutation: delete `.theme-toggle:active` and it fails with "landing: no :active for .theme-toggle:hover". It also reads `class="…"` in `.js`, not only `className="…"` in `.tsx`, because the landing builds HTML strings. |
| `scripts/test/chrome-stillness-check.mjs` | add an undeclared decorative animation (`.rail-brand { animation: logo-pulse 3s infinite }` on the built panel sheet), make a declared ENTRANCE run forever (`gs-in … infinite`), or delete an animation whose reason is still declared | exit 1 all three ways: "…animates (logo-pulse 3s ease-in-out infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer"; "…is declared ENTRANCE (the getting-started card arrives) and animates FOREVER — an entrance or an acknowledgement is a ONE-SHOT"; and "PURPOSES declares .new-menu and the sheet no longer animates it — a stale entry is a reason nobody is using". It is the gate for the objective's "the chrome neutral AND STILL": `motion-check` asks whether an animation is silenced under `prefers-reduced-motion`, which is a different question, so a decorative pulse would have been silenced for the users who ask for that and left running for everyone else. Measured 2026-09-18: 15 animations across both sheets — 8 STATE (may run forever, because the thing they describe has not stopped), 5 ENTRANCE, 2 ATTENTION. ITS FIRST VERSION CLASSIFIED BY SELECTOR NAME AND REPORTED FIVE DEFECTS THAT WERE NOT DEFECTS — `.mem-busy` and `.browser-ev-live` are state-bearing and the pattern could not see it, and the two `.flash` rules are attention states that decay; reading the markup settled it (`.browser-ai-dot` and `.browser-ev-live` render only while `aiActive` holds). The fourth time this session a probe reported a defect the real rule did not have, and the fix was the same: write the rule down instead of approximating it |
| `scripts/test/mark-vocabulary-check.mjs` | stop the console's failure mark from being a diamond (`.dot.err` loses `rotate(45deg)`), or give the panel's `waiting` the `idle` ring | exit 1 both ways: ".dot.err means FAILURE and does not draw the diamond (transform: none) — the panel spells that state as a diamond", and for the panel "the panel's most urgent state draws \"ring\", not a diamond — the shape that means \"this wants you\" has moved" plus "the panel's four states share 1 silhouette(s)". It is the ONLY check that reads both surfaces at once — every other marks check is about one of them — and it reads the panel's vocabulary as DATA (`liveness.ts`) and the console's as PAINTED (its sheet, with the same signature rule `console-marks-check.mjs` uses). Measured 2026-09-18 (**FIVE panel states since round 97**): panel `waiting`=diamond `working`=solid-halo `failed`=triangle `idle`=ring `off`=dashed-ring; console 11 marks, 4 distinct shapes, agreeing on attention=diamond, absent=ring, fine=fill. The failed silhouette arrived only when the DEVICE learned to report the fact (round 96, `last_exit_code`), and it is deliberately NOT the diamond: the panel spends that on a QUESTION and the console spends it on FAILURE, so a fifth state reusing it would put two meanings on one shape in the same tab strip. Mutations: folding `failed` onto the diamond fails with "the shape it already uses for a QUESTION", and dropping the state fails the floor ("read 4 panel state(s), expected at least 5") |
| `scripts/test/console-marks-check.mjs` | give two console signal states the same silhouette (put `off` back to a plain circle), or put the failing ink back (`--text-faint` for `off`) | exit 1 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%|none|none) — strip the colour and they are one state", and ".sig-dot.off [light] var(--text-faint) on --bg = 2.46 (a dot is a graphic; 3 is the bar)". It reads the console's SOURCE sheets because `gateway/ui/dist` is a pruned build artifact, and its first run reported the fix as a defect because `.sig-dot.off` writes `border: 1.5px dashed …` — a SHORTHAND, which a `border-style` lookup cannot see (the panel's contrast test carries the same lesson). **IT COVERS THREE FAMILIES NOW, NOT ONE** (round 24): `.sig-dot`, the key LED `.ov-keyled` and the connection `.dot`, nine marks in all. Widening it found what one family hid — `.ov-keyled` and `.dot` were identical circles told apart by fill colour, and the Overview renders `.dot.ok` / `.dot.err` for every channel with **no rules at all**, so a healthy channel and a broken one drew the same decoration (the `.tab-wait` defect, on the other front end). Mutations: deleting a rendered state's rule trips the floor ("read 8/9 marks … so this proves nothing"); making `.dot.err` a plain fill collides it with `.dot.offline`. Distinctness is checked WITHIN a family, because a solid-with-halo means "on" in more than one place on purpose. **FIVE FAMILIES / THIRTEEN MARKS NOW** (round 26): the two LEDs joined, and they needed it — `.dev-led` had NO mark at all for "off" (ten invisible pixels where "the agent is down" is the row's whole message) and `.dev-mini-led` said it with a grey disc. Both are rings now, which is what the rest of that sheet already means by absent. Mutation: taking the ring out fails with ".dev-mini-led [light]: could not resolve transparent and --bg (a mark that cannot be measured is not a passing mark)"  **ELEVEN MARKS / FIVE FAMILIES, AND A NEW KIND (round 44 — 11 today).** The family list is the LIVE one: `.dot.online` had no producer (no view renders it — the Overview renders `ok`/`err`) and `.dot.offline` restated the base rule verbatim, so both were pruned. And the signature can no longer collapse a mark that is TWO things into one: a fill plus an inset shadow used to compute as `ring`, which is how a real defect could pass. `ring+fill` is now its own kind and a FAILURE — a mark that is both is neither. Mutation: deleting `.dot.err`'s `box-shadow: none` fails with ".dot.err is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither." **AND THE WAY THAT RULE WAS FOUND IS WORTH THE LINE:** the first mutation I tried (`.dot.err` as a 50% circle) did NOT bite, and I read that as a hidden defect — but `.dot.err` was already correct (`box-shadow: none` plus `rotate(45deg)`, i.e. the diamond the vocabulary asks for) and my "fix" was a duplicate declaration, removed. A mutation that does not bite is evidence about the MUTATION first: it was aimed at `.dot.offline`, which this same round had pruned. Read the artefact before believing the instrument — the ninth time tonight, and the first time the instrument was right and I was wrong about the defect. |
| `scripts/test/state-colour-check.mjs` | paint a neutral element with a state colour (`.side-row { color: var(--state-fail) }` on the built panel sheet) | exit 1: "panel: .side-row paints with a state colour, and matches no purpose on the list". It judges BOTH sheets (panel built, console source) and its list is not a suppression file: each entry is a PURPOSE with a reason — a mark, a status surface, the approval gate, a destructive action, a lane, a dial's tone, the update card's availability. Its first run triaged nine rules whose names do not contain the word "state" (`.dial-arc[data-tone=crit]`, `.monitor-fact-down`, `.activity-row-timeout`, …), which is the case the list exists to make somebody write down. It deliberately does NOT judge `--accent`: an accent button is an ACTION, and dressing every primary button grey to satisfy a slogan would be a worse interface |
| `scripts/test/motion-check.mjs` | take a selector out of a `prefers-reduced-motion` block (`.mem-busy` from the panel's) | exit 1: "panel: .mem-busy runs \"mem-busy-spin 1s linear infini\" and no prefers-reduced-motion block stops it". It reads both sheets and accepts either idiom — a selector named in the block, or a global `*`. Written after the CONSOLE was found relying on `--ds-dur: 0s`, which reaches transitions and no `animation:` at all; the panel was the model for that fix and the gate then found SEVEN of its own twelve unsilenced. Verified on the rendered panel too: with reduced motion emulated, the only animation under `no-preference` is xterm's cursor and there are ZERO under `reduce` |
| `scripts/test/particles-check.mjs` | put the retired palette back (`colourOf('--aura-1', '#00ffff')` in the landing), or give a copy a fallback that is not a brand colour | exit 1 either way: "index/src/page.js reads the RETIRED --aura-1 — that is the palette the rebrand removed" and "falls back to #00ffff, which is not one of the brand's colours — a fallback is what people SEE when a token is missing". It holds all three copies (panel module, console module, landing inline script) to FIVE facts (the fifth added in round 48): brand tokens and no retired one, every hex fallback is a brand colour, the draw is `rgba(` from a triple rather than `hsla(` from a hue, and it refuses to run under `prefers-reduced-motion` — a canvas loop is motion the CSS-level `motion-check` cannot see. Verified on the device: with reduced motion emulated the field reports rAF 0/s, clears 0/s, fills 0/s and no canvas in the DOM. The fifth is that THE THREE COPIES DRAW THE SAME FIELD — the same cap, the same density and the same peak alpha — which nothing compared until round 48: the numbers happened to agree (90 / 5.5 per 100000 / MAX_ALPHA * twinkle * 0.35) and would have drifted apart in silence, since facts 1-4 are all about where the COLOUR comes from and three copies can agree on every one of them while drawing visibly different fields. The density needed care: the modules write `DENSITY = 0.55` and apply it as `(w * h / 100_000) * DENSITY * 10`, the landing writes `(w * h / 100000) * 5.5`, so the check resolves the named constant and compares the EFFECTIVE per-100000 value instead of the literals. Mutation: `MAX_MOTES = 140` in the landing fails with "index/src/page.js draws a different field: cap is 140 where the others are 90". Rendered evidence for the landing copy: mote cores measure 255,210,60 (= `#ffd43b` exactly), 243,156,0 and 232,87,12, with ZERO blue-dominant pixels |
| `scripts/test/exports-check.mjs` | export a name and use it only inside its own file (`export const URGENT_MS` in ApprovalGate), or export one used nowhere at all | exit 1 either way: "is exported but used only inside its own file — drop the `export`", or "is exported and used NOWHERE — delete it". It covers BOTH UIs, counts a test-only use as a SEAM rather than dead weight (34 in the panel, and a check that called those dead would be turned off within a round), and cannot see dynamic access (`mod[name]`) — nothing does that today. Its first fix run applied one remedy to both buckets and un-exported a type used nowhere, which only HID it; `noUnusedLocals` is already on in both tsconfigs, so the compiler is the second line of defence for what an un-export leaves behind |
| `scripts/test/retired-colours-check.mjs` | put a retired value back anywhere outside a comment (the accent `#d9480f` in the Rust status page) | exit 1, naming the file and the measurement that retired it. It strips comments FIRST, because its own first run failed on ten files that merely recorded the retirement — a gate that deletes its reasons is worse than no gate |
| `scripts/test/sweep-judges.bash` | plant a defect in a clean console report (an undersized target with no spacing, a theme lie, a stale delivered entry, an unreadable entry) | exit 1 per case — the CLEAN report, the spacing clause that must still PASS, and the current-entry note must all still work, so a judge that fails everything is caught too. Console-only since round 243: the extension's message-tone cases went with the extension, and its delivered-entry cases were TRANSFERRED to the console, which has the same `entryCheck` |

### THE INSTRUCTION FILE WAS 68% EVIDENCE, AND THE MOVE BROKE IT FIRST (rounds 186-187)

The sixteenth exploration — the first pass over the DOCUMENTATION SYSTEM rather than the product — measured the
instruction file's problem precisely: AGENTS.md sat **48 bytes** under its own 48,000-byte ceiling while **68% of it
was one table**. Thirty-seven rows of "this gate, this mutation, this result", the longest cell 5,329 characters.
And the file's own paragraph already stated the rule this broke: *the gate evidence belongs in the ledger, because
THIS is the file that gets truncated when it grows — the rule below, applied to itself.* The file knew. It had not
done it.

**IT WAS NOT A BYTE PROBLEM, IT WAS A HIERARCHY PROBLEM.** The cap was doing its job — it refused two of my edits in
round 175 and forced a decision about what a reader needs — but a cap cannot see a table whose shape is the problem.
Progressive disclosure says a document is steps and reference, and reference that only some branches reach belongs
behind a pointer. Thirteen of the table's rows are read when someone changes THAT gate; a reader loading the file to
learn how to build does not need them at all, every turn, forever.

So the RULES stayed and the EVIDENCE moved: 31,074 bytes out, a four-line pointer in its place, the ledger gaining
a section and the index gaining a row for it. AGENTS.md went 47,952 -> 17,270 bytes. Thirty kilobytes of headroom
where there were 48 bytes.

**AND THE MOVE BROKE THE FILE FIRST, IN A WAY EVERY GATE CALLED CLEAN.** The extraction stopped at the first line
that does not start with `|` — and **some cells contain embedded newlines**, so it moved 16 rows and left 27
orphaned in AGENTS.md under no header at all: table fragments sitting loose between two sections, the sweep-judges
row and five others floating in prose with nothing to say what they were. **Every gate passed.** None of them checks
that a markdown table is well-formed, and a truncated table still parses as prose. The only thing that caught it was
reading the file the edit produced instead of the script's summary — which is the rule this ledger has recorded
eleven times now, and the reason it is recorded rather than assumed.

The boundary is unambiguous and is now stated in the moving script: the table runs to the LAST `|`-starting line
before the next `## ` heading. And the verification asserts the OUTCOME structurally — how many rows are in the new
section, how many `|` rows remain outside the surviving tables — rather than trusting the tool that did the work.
That check then produced its own false positive (it knew about one table and reported the reporter table as
orphans), which reading the artefact settled in ten seconds.

**WHAT IT COST AND WHAT IT BOUGHT.** Two rounds, one bad edit, one revert from a backup taken before the script ran.
What it bought: an instruction file a reader can read whole, a budget that no longer binds, and the evidence in the
one place designed to hold it. The cap was never the problem; the file's shape was, and the cap is what made someone
look.

### THE WINDOWS TARGET WAS VALIDATED WITH A TOOLCHAIN THE RELEASE REJECTS (round 193)

The seventeenth exploration asked, for each job, whether it could go green while the thing it names is broken.
`agent-windows-target` was the sharpest yes: it ran `sudo apt-get install -y llvm` with the comment "llvm-lib
needed by ring's cc build", while `release.yml` refuses that exact package in as many words —

  LLVM 18.1.8 from the OFFICIAL release tarball. NOT apt.llvm.org and NOT the distro `llvm` package: those are
  per-distro builds, and the resulting lld laid .data out differently (measured on 1.2.317 — a 32-byte shift
  plus a 0x100 difference in where rust_panic landed).

**AND THE SAME JOB ALREADY PINNED ITS OTHER TOOL FOR THIS REASON.** One step below the apt install, cargo-xwin
carries a comment explaining the hazard precisely: "a bare install grabs whatever is newest that day, so this job
would validate the Windows target against a different cargo-xwin than the builder uses — a new xwin that rejects
the current invocation fails the RELEASE having passed CI, or the reverse." The job pinned one tool against build
drift and installed the other from the distribution. `build-pins.bash` keeps cargo-xwin, the toolchains and the
pins in step; it had never mentioned llvm at all.

**THE FIX COPIES RATHER THAN INVENTS.** `release.yml` already had the whole answer in three steps — the shared
cache key, the official tarball, the `libtinfo.so.5` compat library the 24.04 runner lacks, and the five symlinks
into `~/.cache/cargo-xwin` that make both builders invoke the same binaries. They are now in the CI job verbatim,
and the shared cache key means no extra download.

Verified as far as a file can be: zero occurrences of `apt-get install -y llvm` in either workflow, the same
cache key in both, and EVERY line of the release's LLVM block present in `ci.yml` (0 missing). What is NOT
verified is that the job runs — that needs a push, and the honest position is that the steps are a copy of a
working file rather than something new.

A GATE IS THE OBVIOUS NEXT STEP: `build-pins.bash` holds the cargo-xwin pin for exactly this class of drift, so
"CI and release resolve the same LLVM" belongs there — one place that fails when the two paths diverge again.

**AND THE DRIFT HAS A GATE NOW (round 194).** `build-pins.bash` exists for exactly this species — its own header
lists "the four build inputs that are HAND-COPIED, each with a single source of truth and NOTHING comparing them",
and its #2 is the cargo-xwin case with the same shape: CI installing whatever is newest while the release pinned a
version. The LLVM is #5. It reads the cache key and the LLVM release out of `release.yml` as the source of truth,
requires BOTH workflows to name both, and refuses a distribution install outright — because a version check cannot
see that drift (the distro package carries whatever the image ships), so the ban is the assertion.

Six checks where there were none; the gate went 24 -> 30. Three mutations, all exit 1: the apt install put back,
the cache key changed, the LLVM release changed. A gate that cannot fail is worse than no gate, and this one was
built by first finding the defect it would have caught.

**AND WRANGLER WAS THE FOURTH MOVING TOOL (round 195).** The seventeenth exploration measured it: the only
wrangler validation in either workflow installed `wrangler@4` — a bare major — so CI dry-ran every proxy with
whatever 4.x was newest that day, while the deploy box runs 4.127.0. Measured when the pin was written: **ten
minor versions apart**, with `npm view wrangler version` reporting 4.137.0. The repo already pins rust 1.98.1,
cargo-xwin 0.23.0 and typescript 5.9.3 — each with a comment naming the drift it prevents — and left the tool that
PUBLISHES the workers unpinned.

`build-pins` clause 6 reads the version out of ci.yml, requires `build.sh` to name the same one in its install
message, and refuses a bare major in either workflow or the build script. 30 -> 34 checks. Two mutations, both
exit 1: the bare major put back, and the two ends disagreeing.

**AND IT FIRED ON ITS OWN EXPLANATION FIRST.** The step's comment quotes the removed `wrangler@4` to say why it is
gone — the repo's house style, since "a gate that deletes its reasons is worse than no gate" — and the raw scan
read that quotation as the drift. Comments are stripped before the scan now, which is what `retired-colours-check`
and `css-vars-check` already do and for the same reason. Fifteenth time in this stretch that an instrument was
wrong before its subject, and the second time the fix was the rule those two gates had already written down.

### AN INSTRUMENT WRONG BEFORE ITS SUBJECT, AND THIS TIME IT WAS ME (round 197)

Verifying the seventeenth exploration's claims locally — work that needs no push — I ran a grep for
`build-installer` and read this:

  scripts/build.sh:138:  # that a reader would have believed it: round-320 deleted build-installer.sh

The file exists (11,349 bytes, executable) and three scripts call it, so I concluded the comment was false and
started to write a fix. Then I read the lines around it, which is the step I had skipped:

  build.sh:137-142:  THE NSIS INSTALLER IS **NOT** RETIRED, and this comment said it was for long enough that a
                     reader would have believed it: round-320 deleted build-installer.sh (182a0347), a later
                     round restored it for the online setup.exe (cf6b3383), and publish-release.sh calls it on
                     every release today. The manifest simply carries no `installer` field at the moment, which
                     is why the landing shows no Setup.exe button — a publication state, not a retirement.

The comment explains the whole history: the deletion, the restoration, the callers, and why the landing shows no
button. My grep showed one line of a paragraph and I read a stale negative into it. **A grep result is a line, not
a context**, and this is the sixteenth time in this stretch that an instrument was wrong before its subject — the
first time the instrument was a search I ran myself.

**AND THE REAL FINDING IS THE INVERSE.** `inventory.md` row 4.8 still asserts, of that same comment, that "the
comment is false" and that "either the script or the comment goes". It was written when that was true and the
comment has since been CORRECTED — so the stale entry is in the document whose entire job is to be a current
checkpoint. That is the eighteenth exploration's subject, and it is the first finding it gets handed.

### AN INSTRUMENT THAT PRINTS A FINGERPRINT OF SOMETHING ELSE (round 198)

The seventeenth exploration's §2 said `toolchain-fingerprint` "prints hashes and compares nothing". Verified: the
job is dispatch-only by design, its step is literally named "Print the fingerprints the release box compares
against", and the comparison happens on that box, by a human. That part is deliberate.

**WHAT IS NOT DELIBERATE IS THAT THREE OF ITS LINES COULD DESCRIBE THE WRONG THING.**

  ls "$HOME/llvm18/bin/clang-18" "$HOME/llvm18/bin/lld" 2>/dev/null || true
  sha256sum A B 2>/dev/null || sha256sum C D
  cargo install cargo-xwin --locked --version 0.23.0 --quiet 2>/dev/null || true
  cargo xwin --version | head -1

The first `|| true` made absence print exactly like presence. The second was the worse one: when the first pair of
files was missing, the fallback hashed two OTHER files under the same "clang / lld extracted from it" label — so a
human comparing CI's fingerprint against the release box could match a hash that described a different binary, and
conclude the toolchains agreed. The third let a failed install fall through to whatever the runner already had, so
the version printed afterwards described a toolchain this job never fetched. A fingerprint about the wrong tool is
worse than no fingerprint, because it is evidence.

Every line names its file now, an absence prints as `ABSENT:`, and a failed install fails the step.

**AND THE CHECK FOR IT CAUGHT ITS OWN EXPLANATION**, which is round 195's lesson arriving one round later: the
grep for remaining `|| true` matched the comment I had just written quoting the old form. The repo's rule — a gate
that deletes its reasons is worse than no gate — is why the quotation stays and why the scan for it has to know
what a comment is.

**AND THE CENSUS READ ONE WORKFLOW WHILE SAYING SO (round 199).** `numbered-claims-check.mjs` — the gate holding "a
gate that runs is a named gate" — read `ci.yml` alone. Its summary said "the workflow", singular, so it was honest
about its scope and still incomplete: `release.yml` invokes a gate too (`npm-test-floored.mjs`, wrapped there in
round 192), and it sat outside the census entirely. Found by wrapping that step, which is the shape of the miss: a
rule written for six steps did not notice the seventh.

It reads both now, 52 gates, and all are named. The first mutation tried did NOT bite — I removed a name for
`stylesheet-hygiene.mjs` and the census still passed, because the ledger names it (the mutation table moved there
in round 187 and took the names with it). The mutation was wrong, not the gate; a name that genuinely is not
documented does fail it. That is the lesson this ledger has recorded more often than any other, and it keeps
arriving.

### A "[measured]" THAT NAMED A PATH NOBODY HAS (round 201)

The eighteenth exploration's §3 was blunt: "§4.3 is wrong three ways ... Executing the delete list destroys it."
Verified before touching anything — and it is right:

  the entry says:   `scripts/live-panel-probe.mjs` (91 lines) · "zero referrers in the whole repository [measured]"
  the path:         does not exist
  the file:         `agent/scripts/live-panel-probe.mjs`, 135 lines
  the referrers:    ELEVEN — scripts/hooks/pre-commit, AGENTS.md, index/README.md, docs/BRAND.md, the ledger,
                    gateway/scripts/sync-code-viewer.sh, a mirror under gateway/public/code/, and §12 of the
                    SAME FILE, which cites it as the instrument verified against the panel the device runs

**A `[measured]` tag is a claim about method, and this one measured a path that was never there.** "Zero
referrers" was true of nothing: `grep -rl` over a filename that does not exist finds nothing, and nothing was
reported as a finding. That is the same failure this ledger has recorded for the `ci.yml:N` citation and the
stale comment — an instrument aimed at the wrong subject — with the extra weight that this one carried the word
`[measured]` and sat in a section titled DELETE.

The row is marked WITHDRAWN rather than removed: a checkpoint that silently drops a mistake teaches the next
reader nothing, and the reason it was wrong is the part worth keeping. The other DELETE rows survive the same
check — the exploration verified 4.1, 4.2 and 4.7 as still correct and 4.5, 4.6, 4.8 as fixed since.

### TWO SECTIONS OF ONE CHECKPOINT THAT CONTRADICTED EACH OTHER IN SILENCE (round 202)

The eighteenth exploration's §7 named the confusable pair, and verification made it sharper than the report put it:

  §9   "The production-host cleanup: what is left, measured, READY TO EXECUTE"
       proposes: 1. the gateway's production defaults become CONFIGURATION
                 2. THEN the fixtures move, and the gateway/test/ allowance comes off the list
  §13  "The deployment-host migration: how it ended, in one place"
       "Read this instead of the eleven rounds that produced it."
       "Eleven gateway test files now run on a TEST DOMAIN; four remain"

**Step 2 has already been executed for eleven of the files**, and §13 says so 350 lines below the section that
proposes it. Neither mentioned the other — zero cross-references in either direction — and the preamble at :8
tells a reader to start from §4/§5/§6, which point forward to nothing. A reader following §9 redoes finished work,
and the counts they would compare against (492) predate the move.

Both now carry the pointer, in opposite directions, and §9's banner says which of its halves is still live: the
MEASUREMENT is the evidence for §13's remaining four, the PLAN is history. That distinction is the whole reason
the section was worth keeping rather than deleting — a checkpoint that drops its own measurement loses the thing
the next round needs.

Same family as everything else this stretch has recorded: a document whose two halves disagreed, where the fix is
not to pick a winner but to make the disagreement visible at the point of use.

### MY TEST-COUNT WRAPPER FAILED ON THE REPORTER IT CLAIMED TO READ (round 203)

The CI run for `9bdd7ad7` came back **failure** — the first completed run since `26d0eb54`, and the first real
verdict on rounds 193-201. Two of the risky changes passed outright: `agent (xwin check windows-msvc)` is green
with the official LLVM tarball in place, and `proxies (node --test + wrangler dry-run)` is green with wrangler
pinned. The failure was `panel`, at step 6: Test.

  Test Files  107 passed (107)
  Tests       832 passed (832)
  FAIL …/agent/resources/panel-react: the suite exited 0 but its output carried no test count at all

The suite ran 832 tests and passed. **My wrapper could not read vitest's summary.** In the raw log the line is
`[2m Test Files [22m [1m [32m107 passed` — vitest COLORISES when it writes to a pipe, and the anchored
`/^\s*Tests\s+(\d+)/` never matched a line beginning with an escape. So the wrapper reported "no test count" for
a suite that had just run 832 of them, and turned a green job red.

**I tested it on two packages and both were `node --test`.** `index` and `gateway` pass, and I wrote "reads all
three reporters the repo uses" from the AGENTS.md reporter table rather than from a run. The third reporter was
never exercised — in the file whose entire purpose is to assert that a suite ran something.

ANSI is stripped before every match now, and the wrapper has been run against all three: vitest 832, node --test
45 and 921, plus the empty-package mutation at exit 1. The lesson is not "strip ANSI" — it is that a claim about
three formats needs three runs, and a table that documents a reporter is not a test of one.

### RE-MEASURING A NUMBERS TABLE, AND GETTING MY OWN INSTRUMENT WRONG TWICE (round 204)

The eighteenth exploration found every headline count in `inventory.md` §1 drifted. Re-measuring them produced a
lesson about MEASUREMENT rather than about arithmetic.

**FIRST MISTAKE: I did not use the cells' own commands.** Two numbers came out lower than the claims, which is
impossible for a count that only grows — a signal I nearly ignored. The cell for the CLI says
`agent/summrise-agent-npm/src/summrise.ts`; I measured `bin/summrise.js`. The panel cell says
`find …/src -type f`; I filtered to `*.ts`/`*.tsx`. With the cells' commands, both match the exploration exactly:
CLI 3,651, panel 40,674. A numbers table whose cells carry their commands is only useful if you RUN them.

**SECOND MISTAKE: three of my probes were broken and I nearly wrote their results down.** The CLI subcommand count
came back 5 against a claim of 12, and the gateway route registrations 0 against 61. Both are my grep failing on
syntax I did not read, not a finding. They are left as written, and the cells now say so, because "correcting" a
number from an instrument I already know is unreliable is how the table got wrong in the first place.

**WHAT LANDED:** ten counts updated (49,132 · 17,777/30 · 7,677/12 · 8,580/4 · 58 · 3,651 · 40,674 · 13,968/42 ·
8,313/25 · 2,756), the vintage re-stamped to 2026-09-24 at `c12e556c`, and a line naming what was NOT re-measured.
`plugins: 9 registered unconditionally` survived verification unchanged.

And one incidental discovery worth the line: **`agent/spec-tools.json` is not JSON.** `require()` refuses it —
"Unexpected token '/', // Device" — because it carries comments, so the cell's own evidence command cannot be run
the obvious way. Counting `"name"` occurrences gives 58, which matches.
