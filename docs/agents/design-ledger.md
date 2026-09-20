# The design ledger — the long form

`AGENTS.md` holds the RULES. This file holds the ROUNDS: what was measured, what it cost, and what was learned on the
way. It was split out in round 67 because AGENTS.md had grown past the workspace instruction budget (65,366 bytes) and
the harness began TRUNCATING it — the tail, which is the Release and layout sections, was being dropped silently. The
organising rule is that an instruction file has to stay small enough to be read whole; evidence does not.

READ THIS WHEN: you are about to re-measure something (the numbers here may already exist), you want the story behind
a rule in `AGENTS.md`, or you are wondering whether a failure you just saw has happened before. It has.

The gate table — which mutation fails which gate — stayed in `AGENTS.md`, because it is the thing consulted most
often. Everything below is in round order, oldest first.

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
