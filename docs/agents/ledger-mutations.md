<!-- LEDGER ARCHIVE 3 of 3 — the mutation table.
     Split out of docs/agents/ledger-appendix.md in round 107, whose 3,861 bytes of headroom were fewer than two
     mutation rows: this is the table rounds APPEND to whenever a gate earns a mutation, so it is the half that grows.
     The other two archives are docs/agents/design-ledger.md (the rounds) and docs/agents/ledger-appendix.md (the
     lookup tables). Titles still resolve across all three; ledger-budget-check.mjs carries the ceiling for each. -->

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
| `scripts/test/ledger-budget-check.mjs` (the third archive) | pad `docs/agents/ledger-mutations.md` past its 400,000-byte ceiling | exit 1, naming the file and both numbers: `is 461649 bytes and the ceiling is 400000 - a table that outgrows its own file is the round-49 problem again`. THE FIRST ATTEMPT WAS INCONCLUSIVE: padding to 303,249 B gave exit 0 because 303 KB is UNDER the ceiling, and a mutation that does not cross the boundary proves nothing (round 109) |
| `scripts/hooks/pre-commit` | append a syntax error to a payload module under `agent/scripts/lib/sweep/` (e.g. `panel-run.cjs`) | exit 1: "The emitted scripts are how the device runs every design check. Fix the emitter, then commit - or use --no-verify if you know the failure is unrelated, and say so in the commit." THE FIRST ATTEMPT PROVED NOTHING: it planted into `agent/scripts/lib/sweep/*.mjs` and the payloads are `.cjs`, so both runs -- mutated and restored -- were on an UNMUTATED tree and both exited 0 (round 120, the same inconclusive-test shape round 109 recorded) |
| `scripts/test/ci-command-table-check.mjs` (direction A2: the scoped instruction file) | name a command in `agent/AGENTS.md` that no step in `ci.yml` runs (e.g. swap `cargo test -p summrise-agent-core` for `cargo test --workspace --all-features`) | exit non-zero: "agent/AGENTS.md names `cargo test --workspace --all-features` and NO step in ci.yml runs it - a scoped instruction file inherits none of its parent's gates (round 123)" AND THE OTHER DIRECTION IS THE ONE THAT MATTERS HERE: the first version of this block REFUSED A CLEAN TREE because it kept each line's trailing `#` comment, so the command never matched CI's text -- a gate that fails on a clean tree blocks every commit until somebody weakens it (round 124). The shipped version strips the comment, and the proof is three runs: clean ok, mutated non-zero, restored ok |
| `gateway/test/instruments-mirror.test.mjs` (new, round 134) | append a byte to a mirrored instrument (`agent/scripts/live-panel-probe.mjs`) without re-syncing `gateway/public/code/files/instruments/` | exit 1: "gateway/public/code/files/instruments is out of date, so the Source Viewer serves instrument code that nobody runs - including the probe AGENTS.md tells a reader to point at a device. Re-sync with `bash gateway/scripts/sync-code-viewer.sh` and commit the mirror." BOTH DIRECTIONS RUN BY EXIT CODE: clean exit 0 (pass 922), mutated exit 1, restored exit 0 |

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

**AND §14 NAMED A FUNCTION THAT DOES NOT EXIST (round 205).** Its table of "one fact, one derivation" row for the
console's device signal read `deviceState.ts` (`deviceSignal`/`signalOf`). Verified: `deviceSignal` appears in ZERO
files of the repository — the name is `DeviceSignal`, a TYPE, used as the return type of two functions. `signalOf`
does exist, at `:86`, but it is NOT exported (no `export` keyword), and the row's own claim is "three views import
it". The exports that matter are `agentSignal:98` and `tunnelSignal:103`.

The row names them now. This is the fourth variant of the same shape this stretch has recorded — after a path that
did not exist, a section that contradicted its neighbour, and a measurement of a file nobody has — and it is the
mildest of the four: the cited FILE was right, the cited LINE was right, and only the symbol was a near-miss of the
type name it returns. A reader following it would have found `DeviceSignal` in the file and concluded they had
misread, which is exactly how a near-miss survives review.

Spot-checked the rest of §14's derivations while there: `path.ts:stateFromEnd`, `liveness.ts:sessionFailed` and
`monitorMark.ts:monitorMarkClass` all exist where named, and `vocabulary.rs` is in the core crate.

### I INTRODUCED A DRIFT WHILE FIXING ONE, AND THE NEXT EXPLORATION CAUGHT IT (round 213)

Round 207 corrected §1's tool count from 56 to 58 and README from 56 to 58, on the strength of
`grep -c '"name"' agent/spec-tools.json` returning 58. The nineteenth exploration measured the same file properly
and got **56**, confirmed by the agent's own prose at `src/mcp/server.rs:68` — "fifteen of the fifty-six tools".

**`grep -c '"name"'` counts PARAMETER names as well as tool names.** The snapshot is an array of tool objects and
every one of them declares a `name` in its input schema, so the count ran two high. Worse: I then wrote a breakdown
whose PARTS summed to 56 and labelled the total 58 — the arithmetic was checked by nothing, including me.

So the file's own warning about counts was demonstrated by the round that was fixing counts: a number was
"corrected" from a right value to a wrong one, in two files, under a commit message complaining about drift. The
fix took one round to land and was found by the next reader, which is the entire argument for measuring a claim
before trusting it — including when the claim is mine.

Both files say 56 again. The lesson is narrower than "verify": **a count of a structured file must come from the
structure, not from a text search over it.** `JSON.parse` after stripping the comments gives 56; grep gave 58; the
difference was two parameter names, and nothing in the pipeline could tell.

### TWO ACCESSORS AGREED ON A DUPLICATE AND THE THIRD DID NOT (round 215)

The nineteenth exploration's registry finding, and the code states the rule it breaks. `plugin_tools` was made
last-wins in round 163 and its own doc comment spells out what all three accessors owe each other:

  "`all_tools` would meanwhile have published both copies. Nothing registers a duplicate name today, and `register`
   now warns if one ever does, but the two accessors must not disagree about the answer even then."

`all_tools` still flat-mapped, so a tool name declared by two plugins was published TWICE — two identical entries in
`tools/list`, which the MCP spec forbids — while `find_tool` and `plugin_tools` both resolved it to the second. And
the asymmetry was visible in the tests: the PLUGIN-name collision has had a test since round 163, the TOOL-name
collision had only a `tracing::warn!`.

**WHAT LANDED.** `all_tools` is last-wins per name and order-preserving, so the generated spec snapshot is
untouched; a test now registers two plugins declaring the same tool and asserts `all_tools()` returns it once. No
duplicate name exists today, so the dedupe changes nothing that ships — it makes the three accessors agree about the
answer they would give if one ever did. 630 lib tests pass.

**AND MY TEST DID NOT COMPILE TWICE.** `E0716` — `reg.all_tools()` returns an owned `Vec`, so chaining `.iter()`
off it borrows a temporary that dies at the end of the statement; rustc printed the fix and I took it. Then
`cargo fmt --check` failed, because I had written the assertion message as a multi-line string literal that rustfmt
reflows differently. Both are the ordinary cost of writing Rust through a script rather than an editor, and both
were caught because the suite ran rather than the file being read.

**AND A SERIALIZATION FAILURE BECAME AN EMPTY SUCCESS, SILENTLY (round 216).** `to_value_or_empty` is read at 64
call sites and converts a failure into `json!([])` with nothing said:

  /// returning `[]` beats propagating a serialization panic.

The reasoning is sound as far as it goes — the alternative in a request handler is a panic — but `[]` is the right
SHAPE for a tool that returns a list and the wrong shape for one that returns an object, and nobody can tell which
happened: not the caller, not the panel, not the console. A wrong-type success is worse than an error because it
reads as an answer.

The empty value stays; the silence does not. It logs at `error!` now, and a test makes the branch fire with a map
whose key cannot be a JSON object key — a branch that had existed since the helper was written and had never been
exercised by anything.

**AND ONE PROBE ANSWERED TWO SHAPES DEPENDING ON THE DOOR (round 217).** The MCP tool `monitor_probe` returns
`{ok, probe, expect, summary}` and its own comment says why `expect` is load-bearing: "`expect_ok: false` is
unreadable without the text the probe wanted, so the target's own expectation travels with the result." The HTTP
route `/api/monitors/probe` — the panel's "check now" — returned `{ok, probe, summary}`, with no `expect`. Neither
shape was pinned by a fixture or a test.

The repo already knew what that costs, in `monitor-row.json`'s own `_why`: "the shape is the contract, nothing
validates it, and **a wrong guess looks like data**." The route carries the criterion now, derived the way the tool
derives it, and both arms of the split are visible side by side in `web/mod.rs`.

Still unpinned: neither envelope has a fixture. `monitor-row.json` pins `summary_of`'s key set (which both
embeddings include) but not the envelope around it — which is the gap the fixture's own paragraph describes.

**AND ONE PROBE'S ENVELOPE WAS A SIX-LINE BLOCK COPIED INTO TWO FILES (round 218).** Round 217 made the two doors
agree; this removes the possibility of their disagreeing, which is the repo's own rule — one source of truth per
fact. Both sites read, byte for byte:

  let expect = crate::monitor::targets().into_iter().find(|t| t.id == id).and_then(|t| t.expect);
  { ok: true, probe: probe, expect: expect, summary: summary(id) }

`probe_envelope(id, probe)` now lives in `monitor.rs` beside `targets()` and `summary()`, which are the two things
it composes, and both doors call it. The tool's block became one line; the route's became one line.

**AND THE TEST ASKS THE QUESTION THE COMMENT ASKS.** It pins the key set, and then asserts that an unwatched id
yields `expect: null` — because the field's own doc says `None` and `false` are different facts ("no answer" versus
"the answer did not say it") and must not collapse. That distinction is the reason `expect` travels with the probe
at all, so it is the thing worth pinning rather than the four key names alone.

### THE TRAIT CARRIED NO BEHAVIOUR AND NINE PLUGINS RESTATED IT (round 219)

The nineteenth exploration's first finding, and the code agrees: the whole `Plugin` trait is three `&'static str`s
plus a `Vec` defaulting to `vec![]` — no init, no shutdown, no health, no state handle. All nine plugins hand-wrote
the same four-method impl, and `runs/mod.rs:29-42` differed from `monitor/mod.rs:31-43` in exactly four literals.

The trait's own doc says where the substance is: "Tools are the single source of truth — MCP, Web API, and Tauri
commands all dispatch through the PluginRegistry." The tools ARE the point; the three strings are metadata that
feeds one JSON field.

**`simple_plugin!` now generates the impl**, so each plugin declares its four facts once:

    pub struct RunsPlugin;
    simple_plugin!(RunsPlugin, "runs", "Runs", "Run identity — …", tools::build);

`runs` and `monitor` are converted; the other seven are the same mechanical change and are next.

**AND THE CONVERSION BROKE FOUR THINGS, EACH CAUGHT BY A DIFFERENT INSTRUMENT** — which is the argument for having
them. `cargo check` found the macro needed importing and that I had removed an import the tests still needed.
The **duplicate-attribute warning** found that round 218's insertion had split an existing `#[test]` from its
function (I had inserted the new test between an attribute and the name it applied to), and my first repair deleted
the ORIGINAL's attribute instead of the stray one — visible only as "function is never used". And `clippy
--all-targets -D warnings` — the exact command CI runs — found that `Plugin` belongs in the TEST module, not at
file scope, because the macro implements the trait through `$crate` and the file itself never names it.

Four fixes, three instruments, none of them the compiler alone. The insertion bug is the second time this stretch
has recorded that a script editing a file must know what the lines around its anchor MEAN, not just where they are.

### A REFACTOR THAT WAS RIGHT AND NOT WORTH IT, AND THE RUST FACT THAT STOPPED IT (round 220)

Round 219 added `simple_plugin!` and converted `runs` and `monitor`. This round tried to convert the other seven and
**reverted the attempt**. The reasoning is the part worth keeping.

**THE MACRO FITS TWO OF NINE, AND THE REASON IS RUST'S MACRO HYGIENE.** The remaining plugins do not pass a bare
path — `memory` writes `tools::build(self.store.clone())`, `terminal` passes seven `&self.` fields across multiple
lines, and `design`/`mcp_client`/`update` return `vec![…]` expressions. Widening `$tools:path` to `$tools:expr` is
the obvious move and it fails with **E0424, "expected value, found module `self`"**: a call-site `self` cannot
resolve inside a `macro_rules!` expansion. That is hygiene, not a bug in the call.

The textbook fix is for the MACRO to bind the receiver and hand it in — `let plugin = self; ($tools)(plugin)` — with
each call site becoming `|me| tools::build(me.store.clone())`. That is correct, and it worked for six of the seven.

**AND THEN IT WAS NOT WORTH IT.** Three rounds of the nine-file rewrite produced: one round of compile errors from
removing an import the tests needed, one from a double semicolon my own replacement introduced, and one from my
argument splitter mishandling `terminal`'s multi-line call. At that point the honest accounting is that the GAIN is
cosmetic — nine impls become nine one-liners, with no behaviour changed and no defect fixed — while the RISK is nine
files in the one subsystem where a mistake reaches every tool the product exposes.

Reverted to round 219's state: `runs` and `monitor` converted, the other seven as written, clippy clean, 632 tests
passing. The macro stays because it is a genuine improvement where it applies and costs nothing where it does not;
the seven are left alone, with the E0424 constraint recorded here so a future attempt starts from the fact rather
than rediscovering it.

**THE LEDGER'S OWN RULE APPLIED TO A REFACTOR**: a change that does not alter what the system DOES has to be
justified by what it makes possible, not by how it reads.

### A REPORT FINDING REVERSED BY FOLLOWING THE DATA (round 222)

The nineteenth exploration's §1 said `display_name`/`description` "are read at exactly ONE site in the crate —
`web/mod.rs:2326-2327` (`api_spec`)" and that "18 hand-written strings exist for one JSON field", implying waste.

Both halves of the measurement are right: those two methods HAVE exactly one caller in the crate, and MCP
`tools/list` genuinely ignores them. What the finding did not follow is where the JSON goes. `api_spec` serves
`/api/spec`, the panel fetches it, and the panel RENDERS the value:

  agent/resources/panel-react/src/hooks/usePlugins.ts:240   displayName: p.displayName,
  agent/resources/panel-react/src/components/ContextRail.tsx:109   {r.displayName}
  agent/resources/panel-react/src/components/PluginsPage.tsx:31,100  search + label

So the 18 strings are the plugin list's LABELS, drawn in the side rail and matched by the plugins page's search.
Not dead weight — the opposite of what the finding implied, and the fourth report detail this stretch reversed by
checking rather than acting.

**THE METHOD THAT CAUGHT IT IS THE ONE WORTH KEEPING**: a finding that stops at "nothing reads this" has usually
stopped one hop too early. The right question is never "who reads the field" but "who reads what the field feeds",
and in a system with an HTTP surface between two languages that is exactly the hop a grep across one crate cannot
make.

**AND FOLLOWING THE PROBE'S DATA ONE MORE HOP SHARPENS WHY THE TWO DOORS MUST AGREE (round 223).** Round 217 made
the MCP tool and the HTTP route return the same envelope; round 218 gave them one source. Following the response to
its consumers shows neither surface UI actually reads it:

  panel  useMonitors.ts:200,231   await callApi("…/monitors/probe", …).catch(() => {}); await refresh();
  CLI    summrise.ts:2311         deviceApi("POST", "/api/monitors/probe", { id: t.id });   ← return unused

The panel refreshes and re-reads the monitor list; the CLI prints its own confirmation. Both are deliberate — the
route exists to TRIGGER a probe, and its body is a courtesy. Only the MCP tool hands the envelope to a consumer that
reads it, which is the AI.

That makes the round-217 fix sharper rather than redundant: an MCP client and an HTTP client asking the same
question should receive the same answer, and the one consumer that reads the answer reaches it through the MCP door.
A shape that differed by door would be a contract nobody could state.

### "NO MCP TOOL RESULT HAS A FIXTURE" IS TRUE AND MISLEADING (round 224)

The nineteenth exploration's §5 ended on that sentence, and the fixtures bear it out: all ten in `agent/tests/`
are HTTP-route, SSE or persisted-file shapes. But following the mechanism shows what it is FOR, and the sentence
implies a gap that is not one.

A fixture exists to pin a payload that a PARSER IN ANOTHER LANGUAGE reads — `required_by_panel` is, in
`monitor-row.json`'s own words, "the list read off the panel's parser", with the device asserting it SENDS every one
and the panel's test asserting it READS them. **No repo-side code parses an MCP tool result**; the AI does. So the
absence of a fixture for one is not an absence of a contract — it is the mechanism correctly not applying.

**AND MEASURING IT FOUND THE ONE PLACE IT DID APPLY AND WAS MISSING.** The panel parses ten routes, and every
payload-bearing one has a fixture: boot-history, monitor-row, session-row, status, vitals-series. `/api/spec` was
the exception — and it is exactly the payload round 222 followed to the panel, where `displayName` is drawn in the
side rail and matched by the plugins search. A rename there empties the plugin list's labels with nothing to see,
which is the failure that fixture family exists to prevent.

Closed: `agent/tests/fixtures/plugin-spec.json` lists what the panel's `SpecPlugin` interface reads, and
`plugin_spec_fixture_matches_the_payload` asserts the DEVICE builds every one of those keys — through
`spec_plugin_object`, a state-free function extracted from `api_spec` so the route and the test build the same
object rather than two that agree today.

**AND THE ATTRIBUTE-BELONGS-ABOVE-THE-DOCS LESSON COST THREE ATTEMPTS (round 224).** Adding the fixture's test put
`#[test]` between an existing doc comment and the `fn` it belonged to — the same insertion bug round 218 recorded.
Repairing it took three tries, and every one failed the same way: I checked the line DIRECTLY above the `fn`, which
is the last line of its doc block, so the attribute was never where I looked. The first repair deleted the wrong
attribute; the second added a duplicate to the function that already had one; the third added two more. The
diagnostic that finally settled it was counting attributes per function — `1` on mine, `3` on the original — which is
the general form of the check I should have written first.

The rule, for the next script that edits Rust: **an attribute goes above the doc block, not above the `fn`.** Any
check that anchors on `fn` must scan upward over `///` lines to find it.

### ONE RENAME, DECLARED FOUR TIMES (round 226)

The nineteenth exploration counted it: the console rewrites `terminal_execute`'s first parameter before forwarding
(`mcp.ts:238` — `body.command = body.input; delete body.input;`), and the test file declared that rename
**identically at three places**. Round 221's type check made it four.

All three were byte-identical, so the fix is a hoist: one `RENAMES` at module scope with the source line it mirrors
written above it. The first attempt put it where the first copy had been — inside a test — and two sibling tests
immediately failed with `ReferenceError: RENAMES is not defined`, which is the whole reason there were three copies:
each test needed its own. Module scope is the answer the copies were working around.

**WHAT IS NOT FIXED, AND IS THE REAL FRAGILITY**: nothing compares that map to the source. If `mcp.ts` renames a
second parameter, or renames this one differently, the test keeps asserting yesterday's contract and passes. A
source-text check would be the usual answer here and it would be the wrong one — the two are one `if` and one object
literal in different languages. The honest fix is to export the map FROM `mcp.ts` so the test imports the same
binding the code uses, which is a change to the source rather than to its copy.

### THE TWENTIETH EXPLORATION: THE TWO UIs, STRUCTURALLY (round 226-227)

Nineteen reviews had measured how the front ends PAINT. This one asked how they are STRUCTURED, and it reversed one
of my own suspicions on the way.

**MY SUSPICION WAS WRONG, AND THE REPORT SAYS SO.** I briefed it to look for a `lib/` that is "mostly browser-bound
and therefore untestable". Measured: 63 files, 32 modules, **25 of 32 pure**, and the split is deliberate —
`lib/attention.ts` is pure derivation while `hooks/useAttention.ts` holds the `document.title` and `Notification`
effects. The one wrinkle is that `vitest.config.ts:10` sets `environment: "jsdom"` globally, so nothing ENFORCES the
purity the layout already has.

**THE FINDINGS THAT MATTER, each with its evidence:**

- **`useSessions.ts` is 837 lines and accumulated, not inherent** — 284 of them comments, 16 `round-NN` markers, and
  it does DOM work (`document.createElement("a")` at :646) inside a session-state hook. It also carries the corpse
  of its own earlier design at :163-169, a `runtimes` Map that "was DECLARED, RETURNED from this hook and never
  written or read".
- **One session row's field names are declared in FIVE places inside one file** — `liveFields`, `wireFields`,
  `wireFieldsChanged` (a second hand-written list of the same nine names), the `map*` helpers, and the `Session`
  interface — and `lib/evicted.ts:42` reads `idle_ms` a sixth time, in another module, with its own coercion. The
  existing gate (`session-row-check.mjs`) disclaims exactly this hole at its own :20.
- **`ok:false` is handled in four dialects.** Five hooks call `deviceRefused`; three do not and default to `[]`, so a
  refusal renders as an empty timeline. And even among the five the OUTCOME differs: silent keep-last
  (`useAgentVitals`), a `failed` flag (`useMonitors`), or a thrown error (`usePlugins`). `api.ts:44-45` calls this
  deliberate-but-unfinished.
- **The console has NO request timeout at all** — no `AbortSignal` anywhere in `gateway/ui/src` — so a hung tunnel
  leaves its busy flag true forever. And its two views of one endpoint disagree about a bodyless response:
  `DevicesPanel.tsx:87` keeps the last good map, `Overview.tsx:103` writes `{}` and blanks the fleet.
- **Two copies of "which version field wins"**, `lib/agentVersion.ts:22-27` and `gateway/src/plugins/mcp.ts:105-107`,
  with the SAME defect fixed independently on each side and nothing comparing them. The panel's own comment names
  the cost: "Two copies of a rule is what let them disagree."
- **The console's `npm test` has zero render or behaviour tests** — all twelve files read source text or shell out.
- **`PanelApp.tsx` is 295 lines of real logic with no test** — six polled hooks, `localStorage`, `new Notification`,
  and a `as any` that casts the session type away.

**AND EDITING THE SOURCE BROKE A GATE I HAD NOT LOOKED FOR**: exporting the rename map from `gateway/src/mcp.ts`
failed `code-viewer-mirror.test.mjs`, because `gateway/public/code/` holds a TRACKED mirror of the source and
nothing had re-synced it. `gateway/scripts/sync-code-viewer.sh` is the other half of that contract; the failure
named the file (`differing: ['mcp.ts']`), which is what made it a one-command fix.

### A DISCLAIMER THAT NAMED A GATE WHICH WAS NOT LOOKING (round 228)

The twentieth exploration found the pair. `session-row-check.mjs` reads `useSessions.ts` and its own comment lists
what it does NOT check: "the panel's other hooks (their fields are the wire-field gate's business) ... or whether
`wireFields` is used everywhere it should be". `wire-field-check.mjs` holds a `PARSERS` list, and `useSessions.ts`
is on it — **`lib/evicted.ts` was not**. That file reads `r.idle_ms` with its own coercion, and the gate whose
business it was said so in the disclaimer of a DIFFERENT gate.

It is on the list now, and the count went from 5 parsers to 6: "30 field(s) read by 6 panel parser(s) — every one
spoken by the harness or a fixture AND by a producer".

**AND MY FIRST MUTATION DID NOT BITE, WHICH WAS THE USEFUL PART.** I renamed `r.idle_ms` to `r.idleMs` and the gate
passed — correctly, because the gate looks for SNAKE_CASE reads, so a camelCase rename does not introduce a bad read,
it REMOVES a wire read. The mutation that bites is the other direction: injecting a read of a field no producer
speaks gives "evicted.ts: reads \"invented_field_ms\", which neither the device harness nor any ...", exit 1.

That is the lesson this ledger has recorded more than any other, and it arrived again: **a mutation that does not
bite is evidence about the mutation first.** The gate guards an UNKNOWN read, not a renamed one, and knowing which
is what makes the next person able to test it.

### A GATE FOR THE SESSION ROW S CARRY LIST VERSUS ITS DETECT LIST (round 233)

The twentieth exploration counted one session row s field names in five places inside `useSessions.ts`. Following it
to the source found the file already says which two matter, and that the bug it describes WAS this disagreement:

  AND THE LIST IS WHERE THE BUG WAS. It omitted `idleMs` and `commandRunning` while `wireFields` did too, so a
  refresh neither carried them nor noticed them — a row kept its discovery values for life and three consumers
  read them as live (sessionActive, the rail s anyCommandRunning, idleSessions offer-to-close). CARRY AND DETECT
  ARE TWO LISTS; fixing the bug needed both.

They CANNOT be merged, and the comment records the attempt: `pendingApproval` is derived at map time, so comparing
the mapped objects reports a change on every poll and pulls the deadline earlier each time. So they stay explicit —
and until now **nothing compared them**. `session-row-check.mjs` guards the READ side (a device field may only be
read inside the mapping section); this guards the two lists agreeing with each other.

**Which mutation must fail it**: remove `idleMs` from `wireFieldsChanged`. It reports
`"idleMs: wireFields carries it, wireFieldsChanged never compares it"` — the historical bug verbatim. It also fails
when `wireFields` stops spreading `liveFields`, because then the CARRY side is two lists of its own and the
comparison can only see half.

**AND THE GATE THAT CAUGHT THIS ONE**: `numbered-claims-check.mjs` failed the round after it was wired into
`ci.yml`, because a gate invoked by the workflow and named NOWHERE an operator reads is a gate nobody can run by
hand. That is why this section exists.

**AND ITS OWN EXTRACTOR MADE THE MISTAKE THIS LEDGER HAS RECORDED FOUR TIMES.** The first version walked braces to
find each list; `wireFieldsChanged` is an EXPRESSION-bodied arrow with no brace at all, so it walked into an
unrelated block and reported every field as missing. Reading it as an expression fixed it.

### THE SIXTH REPORT DETAIL REVERSED BY READING THE SUBJECT (round 237)

The twentieth exploration's §6 ended with a list of "real logic, no test", headed by **`PanelApp.tsx` (295 lines,
six polled hooks, `localStorage`, `new Notification`)**. It is wrong, and the way it is wrong is the point.

`PanelApp` IS rendered — `src/components/__tests__/ActivityPage.test.tsx:418` mounts it through the real shell with
the full prop set, and that test says why in its own comment:

  it("is reachable from the rail with NO sessions open, and shows what ran", async () => {
    // The reachability half: the page must be on the rail and it must mount
    // through the real shell, or the feature is implemented and invisible.

Two more files assert against it by source (`documentOutline.test.ts:109-113` requires it to render both the shell
and the guide; `pageCoverage.test.ts:35` lists it as one of the two SHELLS). What the report actually found is that
`PanelApp` has no test FILE OF ITS OWN — which is a different claim from no test, and the one it wrote would have
sent me to write a second mount of a shell that already has one.

**THE RUNNING COUNT IS NOW SIX.** Across four explorations, six report details have not survived being checked:
§5.8's count, §12's vintages, and `completed:skipped` (18th); the metadata strings "read at one site" (19th); the
`ok:false` dialects in `useOperationRuns` (20th, twice — once wrong about the hook, once about my own guard); and
now `PanelApp`'s test coverage. Every one was caught the same way, and it is the method rather than the finding that
is worth keeping: **the claim and the thing it describes are different objects, and only one of them can be quoted.**

### THE TWENTIETH EXPLORATION, DISPOSITIONED (rounds 226-239)

Nineteen reviews had measured how the front ends PAINT. This one asked how they are STRUCTURED — where state
lives, what components know about the wire, whether two UIs rendering overlapping facts duplicate each other.
Every finding now has an outcome, and the outcomes are not all fixes.

**FIXED.**

- **One parameter rename, declared four times.** `terminal_execute`'s `input`→`command` rewrite sat byte-identically
  at three places in `test/mcp-handler.test.mjs`, and round 221's type check made it four. `PARAM_RENAMES` is now
  EXPORTED from `gateway/src/mcp.ts` and the test imports the binding the code uses (rounds 226-227).
- **The console had no request timeout anywhere.** `client.ts` was the console's only `fetch` and carried no
  `AbortSignal`; a stalled tunnel left every caller awaiting a promise that could not settle, and the views gating
  a spinner on it stayed busy for as long as the tab was open. It carries `AbortSignal.timeout(30_000)` now — the
  bound the panel has had since round 107 — translated to an `ApiError` so callers see it (round 230).
- **Two views of one endpoint disagreed.** `DevicesPanel` writes device status only when the reply carries it;
  `Overview` wrote `s.devices || {}`, so a bodyless reply blanked the fleet — while the comment one line below says
  "tiles keep their last value". Overview now does what its comment says (round 230).
- **DOM work inside a state hook.** `exportSession` built a Blob and clicked an anchor. `lib/download.ts` holds it
  now, the hook no longer names `createElement`, and the helper is testable without rendering — which is what
  `exportSession` never was (round 235).
- **CARRY and DETECT had nothing comparing them.** `useSessions.ts` says it about itself: the two lists cannot be
  merged (`pendingApproval` is derived at map time) and the bug WAS them disagreeing. `session-carry-detect-check`
  holds them together, proven by removing `idleMs` (round 233).
- **The device-version rule, implemented twice.** The panel and the console each decide `release` vs `version`,
  and BOTH halves broke independently. Each is a named function now, each names the other by path, and
  `device-version-rule-check` holds them to one rule (round 236).

**REVERSED — the report was wrong, or my reading of it was.**

- **"One session row's field names declared in five places"** is true and leads to the two lists above; the FIVE
  count is about declarations, and the file already documents which two matter.
- **"`ok:false` handled in four dialects, so a refusal renders as an empty timeline."** True of the DIALECTS;
  false for `useOperationRuns`, which MERGES (`mergeEvents(prev, [])` returns `prev`) and cannot blank. My guard
  was a no-op and its comment claimed otherwise — a mutation that removed it changed no test (round 232).
- **"`PanelApp.tsx` (295 lines): real logic, no test."** It IS rendered — `ActivityPage.test.tsx:418` mounts it
  through the real shell with the full prop set, deliberately ("or the feature is implemented and invisible"), and
  two more files assert against it by source. What it lacks is a test FILE of its own (round 237).
- **"The console's suite has zero render or behaviour tests."** Accurate about `npm test`, and the report says so —
  rendering is covered by four playwright smokes, run by `console-smoke-check.mjs`, which DISCOVERS them after the
  lesson its own comment records: "A smoke that runs nowhere is worse than no smoke, because its green is read as
  coverage." Not a reversal, but not a gap either (round 238).

**DECLINED, WITH THE MEASUREMENT.**

- **"`useSessions.ts` is 837 lines — accumulated, not inherent."** Measured: **835 lines, 286 of them comments
  (34%), 20 `round-NN` markers**, 549 code lines, largest single function ~42. The characterisation holds and the
  file's own history is why. It is a SIZE, not a defect: no symptom follows from it that round 235 did not already
  fix, and splitting it would be nine files of churn for readability in a file whose 835 tests are green. The
  ledger's rule decides it — a sentence that does not change what you would DO does not change the code either.

### A SHELL SCRIPT INSIDE A WORKFLOW IS STILL A SHELL SCRIPT (rounds 240-242)

`release.yml`'s "Publish the release and attach the tgz (API)" step had NEVER EXECUTED. Its script did not parse:

  read -r old old_digest <<<"$(curl -sf -H "$auth" "${api}/releases/${id}/assets?per_page=100" \
         | jq -r --arg a "$ASSET" '.[] | select(.name==$a) | "\(.id) \(.digest // "")"')

The `<<<"` opens a double-quoted string and nothing closes it. It swallowed the rest of that logical line, then the
`local_digest="sha256:..."` two lines below, and the parser gave up at the `jq -r` after that — which is why the
runner reported `line 42: syntax error near unexpected token ')'` for a line that is perfectly valid on its own.
**One character.** `bash -n` on the step, extracted through the YAML parser: exit 2 before, exit 0 after.

It cost a release: v1.2.463 published to the CDN and npm (sha256 74f353d6…, tarball byte-identical) with a GitHub
release carrying NO asset, so the dual-builder audit had nothing to reconcile. The step arrived with round 200's
tag-move guard, and the reason nobody noticed is in that guard's own design — only a SECOND publish reaches it.

**THREE WAYS I MISREAD IT BEFORE GETTING IT RIGHT**, all recorded because the method is the point:

1. **Line-number prefix bisection** (`head -1..N | bash -n`) is invalid for shell: a prefix of a multi-line
   construct is legitimately incomplete, so the FIRST failure is the first OPEN construct, not the break. It
   reported line 4.
2. **`bash -n`'s line number is where the parser GAVE UP**, not where the error is. It blamed a valid line.
3. **`cut`-ting the line for display** truncated it, which made a complete line look broken and sent me looking in
   the wrong place.

What worked: extract the step through the **YAML parser** (not text slicing), test prefixes at STATEMENT
boundaries, and count quote parity — seven double-quotes on one logical line is odd, and the eighth was missing.

**AND THE CLASS NOW HAS A GATE.** `workflow-shell-check.mjs` extracts every `run:` block from every workflow and
`bash -n`s it: **112 blocks across the workflows**, and it fails with the exact message this bug deserved —
"this `run:` block does not parse, so NO step after it in that job can run". Proven by removing the closing quote
again: exit 1, naming `release.yml:279`. Restored, exit 0.

A step whose script cannot run is worse than no step, because the job's green is read as coverage — the same shape
as the console's smoke that ran nowhere, and the gate that could not fail.

### THE DUAL-BUILDER AUDIT CANNOT RUN FROM THIS HOST, AND THE DIGEST ANSWERS IT BETTER (round 252)

`publish-release.sh --audit-only 1.2.463` downloads the GitHub asset and the CDN tarball and compares them byte for
byte. It cannot finish here, and the reason is the network rather than the release:

  curl: (28) Operation timed out after 300001 milliseconds with 2947668 out of 6695607 bytes received

Roughly 10 KB/s from GitHub Releases. The same limit is why `git push` of tags times out (AGENTS.md already says to
tag through the API instead) — this host is behind a link that throttles GitHub's binary endpoints specifically.

**THE QUESTION THE AUDIT ASKS IS ANSWERED BY A HASH, AND ALL THREE COPIES AGREE:**

  GitHub asset      size 6,695,607   digest sha256:74f353d6e2208d22d81f003cb54b6f05b9a2c27702fd63bcecf9317397d4cb8d
  local tarball     size 6,695,607   sha256:74f353d6e2208d22d81f003cb54b6f05b9a2c27702fd63bcecf9317397d4cb8d
  CDN version.json                   sha256:74f353d6e2208d22d81f003cb54b6f05b9a2c27702fd63bcecf9317397d4cb8d

That is a STRONGER statement than the byte comparison, not a weaker one: the release API carries the digest of what
GitHub stores, so this compares three independently-held copies rather than a download of one against a local file.
When the download is possible, run the script; when it is not, compare `assets[].digest` from
`GET /releases/tags/<tag>` against `version.json`'s sha256 and the local tarball, and say which you did.

**AND THE WHOLE CHAIN IS NOW CLOSED, EACH LINK VERIFIED SEPARATELY:**

  CI 11/11 green on 5007aea2  →  tag v1.2.463  →  GitHub release with the asset (digest above)
  →  CDN serving version.json 1.2.463  →  npm carries 1.2.463  →  **d1 reports release 1.2.463, this device is current**

The device line is the one that matters, and it took the whole stretch: 137 commits had shipped nothing when the
operator asked why the work was invisible.

### THE ARC OF THIS STRETCH: FROM "WHY CAN T I SEE PROGRESS" TO A DEVICE THAT IS CURRENT (rounds 213-255)

The operator asked, in the middle of it: **"你最近改了什么，我怎么看不出进展呢"** — what have you changed lately, and why
can I not see any progress. The question was correct and the answer was measurable, so it is recorded here rather
than in a report nobody re-reads.

**THE MEASUREMENT.** At that moment: **49 of the last 60 commits touched `docs`**, 24 touched `scripts` (the gate
suite), 22 touched `agent` (mostly tests and comments), 5 touched `gateway`. **137 commits had landed since
1.2.462 and nothing had been published.** Of those, roughly eighteen changed what the DEVICE does — the installer's
repair path, the update receipt, the boot warning, a SYSTEM-reachable route's brute-force penalty — and every one of
them existed only in this checkout.

So the work was real and the delivery was zero. That is the whole diagnosis, and the loop had been treating
"green gates" as the product.

**WHAT FOLLOWED.** Two releases, both verified end to end:

  1.2.463  the eighteen device fixes, plus the release that had never been possible: `release.yml`'s publish step
           had NEVER PARSED. A missing `"` after `<<<"$(curl …)` swallowed two lines; the runner blamed a line
           three below that is valid. It arrived with round 200's tag-move guard and was unreachable from the day
           it was written, because only a SECOND publish reaches it.
  1.2.464  three CLI commands that reported the wrong verdict, a panel that knew too much about the DOM, and the
           gate that came out of 1.2.463's own blockage (`workflow-shell-check`, 112 run blocks).

**EACH LINK VERIFIED SEPARATELY, and the last one is the point:** CI 11/11 green → tag → GitHub release with the
asset → CDN `version.json` → npm `latest` → and on d1:

  release: 1.2.463
  this CLI: 1.2.463
  latest: 1.2.463 (this device is current)

**WHAT THE LOOP LEARNED ABOUT ITSELF, in its own failures:**

- **A guard that searches for a string the change itself introduces is not a guard.** Mine skipped its own import
  because the comment it had just inserted named the file it was checking for.
- **`git checkout` prints "Aborting" and I read past it twice**, putting two empty commits on the wrong base.
  "READ THE EXIT CODE, NOT THE OUTPUT" is in `AGENTS.md` and I had to be reminded by the tool.
- **A mutation that does not bite is evidence about the MUTATION first.** Twice: the refusal guard in
  `useOperationRuns` was a no-op (the hook merges), and a precedence check measured declaration order rather than
  branch order.
- **Editing a file with a script requires knowing what the lines around the anchor MEAN.** Four times: an attribute
  split from its function, `ok: true` injected into a destructuring pattern, a brace walker sent into an unrelated
  block by an expression-bodied arrow, and a publish step sliced at a line number.
- **SIX report details did not survive being checked**, across four explorations, every one caught by reading the
  subject instead of the summary.

**WHAT REMAINS OPEN, for whoever continues:**

- The six rows in `docs/agents/ideas.md` are waiting on the OPERATOR, not on the loop (CHARTER citations, a mark
  shape, a landing host, the pre-commit hook symlink).
- `1.2.453` still sits on the CDN with no GitHub release to audit against. It is the residue of the `alpha`
  incident and is acknowledged in every publish rather than silently skipped.
- The dual-builder audit cannot run from this host — GitHub Releases are throttled to ~10 KB/s here. Compare
  `assets[].digest` against `version.json`'s sha256 instead, and say which you did.
- `status` exits 0 on an UNKNOWN verdict, deliberately, and the reason is now written at the command.
