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
