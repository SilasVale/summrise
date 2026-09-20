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
