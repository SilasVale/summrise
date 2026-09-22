# Vale

One repo, one front door: `gateway/` (Vale Gate worker), `agent/` (Vale Agent, Windows),
`index/` (dist + CDN worker), `proxies/` (satellite workers), `brand/`, `docs/`.
The operator's own rules are `docs/CHARTER.md`; their inbox is `docs/agents/ideas.md`.

## Build

```bash
./scripts/build.sh agent              # panel SPA (vite + vitest) then cargo xwin release
./scripts/build.sh gateway|index      # wrangler deploy the worker (needs CLOUDFLARE_API_TOKEN)
./scripts/build.sh proxies            # deploy the satellite proxy workers
./scripts/build.sh deploy             # agent + gateway + index + proxies

cargo xwin check --target x86_64-pc-windows-msvc --features terminal,keyring   # fast agent check
```

Panel-first: `panel.js` is embedded with `include_str!`, so a change under
`agent/resources/panel-react/` needs `npm run build` there (or `build.sh agent`, which does it).
The exe lands in `agent/target/x86_64-pc-windows-msvc/release/vale-agent.exe`.

## Test

```bash
cd agent/resources/panel-react && npm test       # panel (vitest)
cd agent && cargo test                           # agent
cd agent && cargo test --features terminal,keyring
cd agent && cargo clippy --all-targets -- -D warnings        # also with --features terminal,keyring
cd gateway && npm test                           # gateway (own prettier gate)
```

Green tests are the bar for a release.

**AND RUN THE COMMAND THE OTHER END RUNS.** Round 144 is what it costs to skip this: six type errors passed a local
`tsc --noEmit` in `gateway/ui` and failed CI, because the `ui` job runs `npm run build`, which is
`tsc -b && vite build && prune-stale-assets` — a project-graph build, not a single-file check. The exact commands, by working
directory, as of the run that verified them:

| where | CI runs | and NOT |
|---|---|---|
| `gateway/` | `npm run typecheck` (= `tsc --noEmit`) · `npm test` · `npm run lint` (= `eslint src/`) · `npm run format:check` | — |
| `gateway/ui/` | `npm run build` (= `tsc -b && vite build && prune-stale-assets`) · `npm test` | **not** `tsc --noEmit`, which is the check that missed them |
| `agent/resources/panel-react/` | `npm run build` · `npm test` | — |
| `agent/` | `cargo fmt --all -- --check` · `cargo clippy -p vale-agent --all-targets -- -D warnings` · `cargo clippy -p vale-agent --features terminal,keyring --all-targets -- -D warnings` · `cargo clippy -p vale-agent-core --all-targets -- -D warnings` · `cargo test -p vale-agent` · `cargo test -p vale-agent --features terminal,keyring` · `cargo test -p vale-agent-core` | — |

All four were run by hand on the commit that added this table and all were green; before that, `gateway`'s lint and typecheck
and the agent's `fmt`/`clippy` had not been run by this loop at all, and the panel's `npm test`, not `npx vitest run`, is what
CI invokes.


**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter, and grepping for the wrong
one returns NOTHING — which looks exactly like a suite that passed silently:

| command | the line to look for |
|---|---|
| `cargo test` (either config) | `test result: ok. N passed; 0 failed` |
| `npm test` in `gateway/` (Node 24) | `ℹ pass N` — on Node 20 it prints `# pass N` instead |
| `npx vitest run` in `panel-react/` | `Tests  N passed` |

Two rounds of this session were spent reading a silent grep as "the suite did not run" and then
re-running it by another route: once on the burst-gate test (round 144) and once on the gateway
(round 169). `exit 0` is the answer in every case; the line is a convenience.

### Which gates have been PROVEN to bite

**THE GATES THE STANDING OBJECTIVE ADDED ARE IN THE LEDGER, NOT HERE** (beginning with `contract-vocabulary-check`,
`one-derivation-check`, `session-row-check`, `wire-field-check`, `console-wire-field-check`, `gateway-device-field-check`,
`device-verdict-check`, `sweep-fixture-dupes-check` and `production-host-check`, plus the `harness-fixture-check` changes;
`stub-surface-check` and `ci-command-table-check` came later). Each carries the mutation that must fail it, and each was
written because the rule it holds had ALREADY cost a real defect. They live in `docs/agents/design-ledger.md` and
`inventory.md` §12 rather than in this table, because THIS is the file that gets truncated when it grows — the rule below,
applied to itself.

**NO COUNT IS GIVEN HERE ON PURPOSE.** It said "ten" while the objective was at ten, and a round later there were twelve:
the same drift this file records for the sweep's check count ("the NUMBER is what drifts when a round adds a case without
updating this cell"). `numbered-claims-check.mjs` now holds the claim that a wired gate is a NAMED gate, which is the part
that can be checked; the count is left to whoever wants to count.


A gate that cannot fail is worse than no gate, and the only way to know is to break the thing it
guards and watch what happens. Every gate below was audited that way (rounds 65-68) — none of them is
assumed:

| gate | mutation that must fail it | result |
|---|---|---|
| `scripts/test/token-contract-check.mjs` | change a shared token's value on one side, or DELETE a spacing step from one of them | exit 1 both ways: "1 of 46 shared tokens DIVERGE" for a disagreeing value, and "console does not define --sp-3" for a deleted step — the second case is why the spacing block outlives the shared-name comparison, which by construction cannot see a name only one side still declares (round 243) |
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
| `scripts/test/harness-fixture-check.mjs` | change the harness fixture's session-count default from 3, or stop `?exitfail=1`/`?exitok=1` from setting `last_exit_code` | exit 1, and its own self-test fails first if the pattern goes stale. The second pair is round 96's rule: A STATE WITH NO SURFACE CANNOT BE MEASURED — every seed reports no exit code, so without a flag the chip has no rendered surface anywhere, and the mutations are "the failure flag stopped setting the code" and "exit ZERO stopped being expressible". **AND THE `/api/boots` ENVELOPE (round 99)**: the stub omitted `ok: true`, which the hook requires, so the Restarts card rendered "The device did not answer" on eight surfaces while the device answered perfectly — a fixture whose failure mode is a FALSE CLAIM, not an empty card. Mutation: strip the envelope from the emitted stub — and, since round 101, WIDEN THE ARCHIVE STUB BACK to `u.indexOf('/api/sessions') >= 0`, which shadows the per-session stub below it (the audit trail) and makes the Trajectory and Path views read an empty archive as if it were the trail. Also since round 100 the envelope is STRUCTURAL (`J()` merges `ok: true` into every object body that does not bring its own, so a fixture cannot forget it), with its own mutation in the same gate: remove the merge |
| `scripts/test/spacing-scale-check.mjs` | add a spacing declaration with an off-scale literal (a planted `13px`), or an ON-scale one where a token exists (a planted `8px`) | exit 1 either way: "off-scale spacing rose from 305 to 306", or "on-scale literals rose from 0 to 1 — a value that HAS a token was written out by hand, which is how 234 of them accumulated unnoticed". A token use replaced by a literal fails the third count |
| `scripts/test/landing-check.mjs` | lighten a label (the tertiary `#71717a` → `#9a9aa2`), take the heading away (the `<h1>` back to a `<div>`), or give a card a fixed width (`.card { width: 480px }`) | exit 1 for each: "2.68 on #fafafa, under the 4.5 AA wants for text" · "expected exactly 1 h1, found 0" · ".card { width: 480px } — wider than the 320px a 1.4.10 reflow test uses". It reads page.js's own values and BOTH themes (renamed from landing-contrast-check in round 242: a name covering a third of what it does is the kind that stops the next reader looking) |
| `scripts/test/harness-fixture-check.mjs` (the slow-network flag, round 19) | stop applying the delay (`SLOW ? p.then(SLOWLY) : p` → `p`) | exit 1 — the fixture must be able to make every reply slow, or a control whose feedback waits for the network is indistinguishable from one that answers on the event |
| `scripts/test/feedback-check.mjs` | drop a press state the sheet had (`.tab:active`), or transition a layout property, or write `transition: all` | exit 1: "no :active for .tab:hover — a hover that answers and a press that does not is the feedback gap this checks for". It reads BOTH sheets — the panel's built sheet and the console's SOURCE sheet, each with its own floors (round 54) — skips reveal rules (`.tab:hover .tab-export` is not pressable), treats a press on a base as covering its variants (`.btn:active` answers `.btn.primary`), and its first run misread the sheet twice — a `:not(...)` suffix and a multi-line selector list — which is why it names what it could not read  **MEASURED AS RENDERED, WHERE ONE CONTROL HAD NO PRESS AT ALL (round 51).** This check proves an `:active` RULE EXISTS; it cannot see whether the press is VISIBLE, which is what the objective actually asks for. The first rendered measurement — `mouse.down()` on each control, comparing the computed transform / opacity / background — found every control answering EXCEPT the session tab, and only the ACTIVE one: `.tab.active` and `.tab:active` are both (0,2,0), so the later rule wins and the tab an operator presses to re-focus it did nothing at all. Fixed with `transform: translateY(1px)`, which a background rule cannot override, and it is what every other control already used. RENDERED AFTER: all three tabs report PRESS RENDERS. The measured set, for the next reader: `.rail-btn` 38x38, `.tab-close` 24x24, `.side-add` 30x21, `.btn` 54x26 and `.tab` 96x34 all answer — and a sheet-level check would have passed every one of them either way. **AND THE CLASS IS NOW AUTOMATIC (rounds 52-53): A PRESS WHOSE EVERY PROPERTY IS SHADOWED CANNOT BE SEEN.** The check groups every `:active` rule by the element it presses, collects the declarations the group makes, and fails when a LATER rule that names the same element at equal-or-higher specificity overrides every one of them — which is what killed the active tab's press. It found FOUR more, all rows whose own `:hover` rule sets a different background: `.side-row`, `.run-strip-head`, `.archive-row`, `.path-attention-row`. Hover ALWAYS co-occurs with active, so at equal specificity the hover rule won every time. Fixed with `transform: translateY(1px)`, the vocabulary every other press already uses; `.side-row` was measured DEAD in round 52 at 232x34 and reports `none -> matrix(1, 0, 0, 1, 0, 1)` now. IT SHED THREE CLASSES OF FALSE POSITIVE FIRST, and each is a way this kind of check lies: substring matching (`.btn-ghost` contains `.btn`, so `.btn`'s healthy press was reported shadowed), DESCENDANT selectors (`.cmd-btn.cmd-toggle.open svg` names the class while styling a chevron), and same-value rules (a later rule saying the same thing is not a shadow). And a fourth lie was MINE, not the check's: my first mutation removed a selector from a list that no longer had it, my `assert` looked for a string that was never expected to exist, and the gate rightly passed a sheet that had not changed — caught by grepping the BUILT sheet for the rule instead of trusting the edit. A floor of 20 presses and a count in the summary line  **AND THE CONSOLE HAD NO PRESS CHECK AT ALL UNTIL ROUND 54.** This gate had only ever read the panel's sheet. Pointed at the console's source it found 25 `:hover` selectors and ONE press rule: TEN controls — the rail button, the avatar, the logout item, the icon button, the language button, the auth tab, `.btn-dashed`, `.card-link`, `.dev-mini` and every link — acknowledged a hover and stayed completely still under a press. Fixed with a press layer that respects what each element IS: `transform: translateY(1px)` where it renders, `scale(0.96)` for the avatar whose hover is a scale, and OPACITY for `a` and `.card-link` because A TRANSFORM DOES NOT MOVE AN INLINE BOX. It also found six `transition: all var(--ds-dur)` rules the console had never been checked for — each now names the properties that element actually animates. AND THE VARIANT EXEMPTION IS MEASURED NOW, NOT ASSUMED. The panel writes `.btn.primary` (dot form) so a press on `.btn` provably covers it; the console writes `className="btn btn-primary"` (two classes, dash form), where the same is TRUE but no prefix rule can know it. The check reads the markup and grants the exemption only where both classes appear on one element — which is why `.btn-dashed`, used ALONE in this console, was reported as a real gap instead of being waved through with its siblings. Mutation: removing `.lang-btn:active` fails with "console: no :active for .lang-btn:hover". **AND THE LANDING IS THE THIRD SHEET, WHICH IS WHERE THE ROUND-95 DEFECT WAS HIDING (round 95).** It reads the panel's built sheet, the console's SOURCE sheet, and now the landing's `<style>` block CROPPED OUT OF `index/src/page.js` (a third kind of file this check had never opened; a missing block is FATAL, not a silent zero). That is how `.theme-toggle` — a hover rule and no `:active` at all — was found: the rendered pass could not see it either, because it measured the press against REST. Mutation: delete `.theme-toggle:active` and it fails with "landing: no :active for .theme-toggle:hover". It also reads `class="…"` in `.js`, not only `className="…"` in `.tsx`, because the landing builds HTML strings. |
| `scripts/test/chrome-stillness-check.mjs` | add an undeclared decorative animation (`.rail-brand { animation: logo-pulse 3s infinite }` on the built panel sheet), make a declared ENTRANCE run forever (`gs-in … infinite`), or delete an animation whose reason is still declared | exit 1 all three ways: "…animates (logo-pulse 3s ease-in-out infinite) and NO PURPOSE IS DECLARED — the chrome is still, and motion belongs to the state layer"; "…is declared ENTRANCE (the getting-started card arrives) and animates FOREVER — an entrance or an acknowledgement is a ONE-SHOT"; and "PURPOSES declares .new-menu and the sheet no longer animates it — a stale entry is a reason nobody is using". It is the gate for the objective's "the chrome neutral AND STILL": `motion-check` asks whether an animation is silenced under `prefers-reduced-motion`, which is a different question, so a decorative pulse would have been silenced for the users who ask for that and left running for everyone else. Measured 2026-09-18: 15 animations across both sheets — 8 STATE (may run forever, because the thing they describe has not stopped), 5 ENTRANCE, 2 ATTENTION. ITS FIRST VERSION CLASSIFIED BY SELECTOR NAME AND REPORTED FIVE DEFECTS THAT WERE NOT DEFECTS — `.mem-busy` and `.browser-ev-live` are state-bearing and the pattern could not see it, and the two `.flash` rules are attention states that decay; reading the markup settled it (`.browser-ai-dot` and `.browser-ev-live` render only while `aiActive` holds). The fourth time this session a probe reported a defect the real rule did not have, and the fix was the same: write the rule down instead of approximating it |
| `scripts/test/mark-vocabulary-check.mjs` | stop the console's failure mark from being a diamond (`.dot.err` loses `rotate(45deg)`), or give the panel's `waiting` the `idle` ring | exit 1 both ways: ".dot.err means FAILURE and does not draw the diamond (transform: none) — the panel spells that state as a diamond", and for the panel "the panel's most urgent state draws \"ring\", not a diamond — the shape that means \"this wants you\" has moved" plus "the panel's four states share 1 silhouette(s)". It is the ONLY check that reads both surfaces at once — every other marks check is about one of them — and it reads the panel's vocabulary as DATA (`liveness.ts`) and the console's as PAINTED (its sheet, with the same signature rule `console-marks-check.mjs` uses). Measured 2026-09-18 (**FIVE panel states since round 97**): panel `waiting`=diamond `working`=solid-halo `failed`=triangle `idle`=ring `off`=dashed-ring; console 11 marks, 4 distinct shapes, agreeing on attention=diamond, absent=ring, fine=fill. The failed silhouette arrived only when the DEVICE learned to report the fact (round 96, `last_exit_code`), and it is deliberately NOT the diamond: the panel spends that on a QUESTION and the console spends it on FAILURE, so a fifth state reusing it would put two meanings on one shape in the same tab strip. Mutations: folding `failed` onto the diamond fails with "the shape it already uses for a QUESTION", and dropping the state fails the floor ("read 4 panel state(s), expected at least 5") |
| `scripts/test/console-marks-check.mjs` | give two console signal states the same silhouette (put `off` back to a plain circle), or put the failing ink back (`--text-faint` for `off`) | exit 1 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%|none|none) — strip the colour and they are one state", and ".sig-dot.off [light] var(--text-faint) on --bg = 2.46 (a dot is a graphic; 3 is the bar)". It reads the console's SOURCE sheets because `gateway/ui/dist` is a pruned build artifact, and its first run reported the fix as a defect because `.sig-dot.off` writes `border: 1.5px dashed …` — a SHORTHAND, which a `border-style` lookup cannot see (the panel's contrast test carries the same lesson). **IT COVERS THREE FAMILIES NOW, NOT ONE** (round 24): `.sig-dot`, the key LED `.ov-keyled` and the connection `.dot`, nine marks in all. Widening it found what one family hid — `.ov-keyled` and `.dot` were identical circles told apart by fill colour, and the Overview renders `.dot.ok` / `.dot.err` for every channel with **no rules at all**, so a healthy channel and a broken one drew the same decoration (the `.tab-wait` defect, on the other front end). Mutations: deleting a rendered state's rule trips the floor ("read 8/9 marks … so this proves nothing"); making `.dot.err` a plain fill collides it with `.dot.offline`. Distinctness is checked WITHIN a family, because a solid-with-halo means "on" in more than one place on purpose. **FIVE FAMILIES / THIRTEEN MARKS NOW** (round 26): the two LEDs joined, and they needed it — `.dev-led` had NO mark at all for "off" (ten invisible pixels where "the agent is down" is the row's whole message) and `.dev-mini-led` said it with a grey disc. Both are rings now, which is what the rest of that sheet already means by absent. Mutation: taking the ring out fails with ".dev-mini-led [light]: could not resolve transparent and --bg (a mark that cannot be measured is not a passing mark)"  **ELEVEN MARKS / FIVE FAMILIES, AND A NEW KIND (round 44).** The family list is the LIVE one: `.dot.online` had no producer (no view renders it — the Overview renders `ok`/`err`) and `.dot.offline` restated the base rule verbatim, so both were pruned. And the signature can no longer collapse a mark that is TWO things into one: a fill plus an inset shadow used to compute as `ring`, which is how a real defect could pass. `ring+fill` is now its own kind and a FAILURE — a mark that is both is neither. Mutation: deleting `.dot.err`'s `box-shadow: none` fails with ".dot.err is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither." **AND THE WAY THAT RULE WAS FOUND IS WORTH THE LINE:** the first mutation I tried (`.dot.err` as a 50% circle) did NOT bite, and I read that as a hidden defect — but `.dot.err` was already correct (`box-shadow: none` plus `rotate(45deg)`, i.e. the diamond the vocabulary asks for) and my "fix" was a duplicate declaration, removed. A mutation that does not bite is evidence about the MUTATION first: it was aimed at `.dot.offline`, which this same round had pruned. Read the artefact before believing the instrument — the ninth time tonight, and the first time the instrument was right and I was wrong about the defect. |
| `scripts/test/state-colour-check.mjs` | paint a neutral element with a state colour (`.side-row { color: var(--state-fail) }` on the built panel sheet) | exit 1: "panel: .side-row paints with a state colour, and matches no purpose on the list". It judges BOTH sheets (panel built, console source) and its list is not a suppression file: each entry is a PURPOSE with a reason — a mark, a status surface, the approval gate, a destructive action, a lane, a dial's tone, the update card's availability. Its first run triaged nine rules whose names do not contain the word "state" (`.dial-arc[data-tone=crit]`, `.monitor-fact-down`, `.activity-row-timeout`, …), which is the case the list exists to make somebody write down. It deliberately does NOT judge `--accent`: an accent button is an ACTION, and dressing every primary button grey to satisfy a slogan would be a worse interface |
| `scripts/test/motion-check.mjs` | take a selector out of a `prefers-reduced-motion` block (`.mem-busy` from the panel's) | exit 1: "panel: .mem-busy runs \"mem-busy-spin 1s linear infini\" and no prefers-reduced-motion block stops it". It reads both sheets and accepts either idiom — a selector named in the block, or a global `*`. Written after the CONSOLE was found relying on `--ds-dur: 0s`, which reaches transitions and no `animation:` at all; the panel was the model for that fix and the gate then found SEVEN of its own twelve unsilenced. Verified on the rendered panel too: with reduced motion emulated, the only animation under `no-preference` is xterm's cursor and there are ZERO under `reduce` |
| `scripts/test/particles-check.mjs` | put the retired palette back (`colourOf('--aura-1', '#00ffff')` in the landing), or give a copy a fallback that is not a brand colour | exit 1 either way: "index/src/page.js reads the RETIRED --aura-1 — that is the palette the rebrand removed" and "falls back to #00ffff, which is not one of the brand's colours — a fallback is what people SEE when a token is missing". It holds all three copies (panel module, console module, landing inline script) to FIVE facts (the fifth added in round 48): brand tokens and no retired one, every hex fallback is a brand colour, the draw is `rgba(` from a triple rather than `hsla(` from a hue, and it refuses to run under `prefers-reduced-motion` — a canvas loop is motion the CSS-level `motion-check` cannot see. Verified on the device: with reduced motion emulated the field reports rAF 0/s, clears 0/s, fills 0/s and no canvas in the DOM. The fifth is that THE THREE COPIES DRAW THE SAME FIELD — the same cap, the same density and the same peak alpha — which nothing compared until round 48: the numbers happened to agree (90 / 5.5 per 100000 / MAX_ALPHA * twinkle * 0.35) and would have drifted apart in silence, since facts 1-4 are all about where the COLOUR comes from and three copies can agree on every one of them while drawing visibly different fields. The density needed care: the modules write `DENSITY = 0.55` and apply it as `(w * h / 100_000) * DENSITY * 10`, the landing writes `(w * h / 100000) * 5.5`, so the check resolves the named constant and compares the EFFECTIVE per-100000 value instead of the literals. Mutation: `MAX_MOTES = 140` in the landing fails with "index/src/page.js draws a different field: cap is 140 where the others are 90". Rendered evidence for the landing copy: mote cores measure 255,210,60 (= `#ffd43b` exactly), 243,156,0 and 232,87,12, with ZERO blue-dominant pixels |
| `scripts/test/exports-check.mjs` | export a name and use it only inside its own file (`export const URGENT_MS` in ApprovalGate), or export one used nowhere at all | exit 1 either way: "is exported but used only inside its own file — drop the `export`", or "is exported and used NOWHERE — delete it". It covers BOTH UIs, counts a test-only use as a SEAM rather than dead weight (34 in the panel, and a check that called those dead would be turned off within a round), and cannot see dynamic access (`mod[name]`) — nothing does that today. Its first fix run applied one remedy to both buckets and un-exported a type used nowhere, which only HID it; `noUnusedLocals` is already on in both tsconfigs, so the compiler is the second line of defence for what an un-export leaves behind |
| `scripts/test/retired-colours-check.mjs` | put a retired value back anywhere outside a comment (the accent `#d9480f` in the Rust status page) | exit 1, naming the file and the measurement that retired it. It strips comments FIRST, because its own first run failed on ten files that merely recorded the retirement — a gate that deletes its reasons is worse than no gate |
| `scripts/test/sweep-judges.bash` | plant a defect in a clean console report (an undersized target with no spacing, a theme lie, a stale delivered entry, an unreadable entry) | exit 1 per case — the CLEAN report, the spacing clause that must still PASS, and the current-entry note must all still work, so a judge that fails everything is caught too. Console-only since round 243: the extension's message-tone cases went with the extension, and its delivered-entry cases were TRANSFERRED to the console, which has the same `entryCheck` |

## The design ledger

THE LONG FORM LIVES IN `docs/agents/design-ledger.md` — one section per round: what was measured, what it cost, and
what was learned. It was split out in round 67 after this file grew past the workspace instruction budget and the
harness began TRUNCATING its tail (the Release and layout sections below were being dropped silently — an instruction
file that cannot be read whole is worse than a short one).

READ IT WHEN you are about to re-measure something, want the story behind a rule here, or wonder whether a failure has
happened before. The numbers for contrast, silhouettes, the loud axis, idle repaint, the press passes and the wire
contracts are all in there, with the mistakes that produced them.

What stays HERE: how to build, how to test, which mutation each gate must fail, how to commit, how to release, and
where the code lives. If a sentence does not change what you would DO, it belongs in the ledger — and a round that
learned something new adds a section THERE, not here.

## Committing

**A pre-commit hook runs the emitters** (`scripts/hooks/pre-commit`, round 225; rationale rewritten round 272).
It used to guard the backtick-in-a-template-literal accident — **and that class is gone**: all five emitters now hand
their payload to `agent/scripts/lib/sweep-bundle.mjs`, which resolves the payload's own requires and COMPILES what it
returns. (The incident count was quoted as 52 here, 34 in the hook and 38 in the operator's inbox; the three never
agreed, and they are history — the ledger holds the incidents.) What the hook still buys is the only end-to-end
assembly of all five artifacts in under a second: a payload module that does not parse, a require the assembler cannot
resolve, or an emitter that was renamed or deleted fails at the commit instead of in the design job.

**AND IT IS STILL NOT INSTALLED, WHICH HAS NOW COST A PUSH.** Round 93's commit carried a backtick in a comment,
the probe module stopped PARSING, and five of the ten CI jobs went red (ui, panel, gateway, design, pack-chain —
everything that imports it). The hook refuses that commit in under a second, and so does
`contrast-probe-check.mjs`, which CI runs at `ci.yml:460` — but nothing runs the hook unless the loop does it by
hand, because `core.hooksPath` is global. RUN IT BY HAND BEFORE EVERY COMMIT until the symlink below exists; the
story is in the ledger under "THE 46TH BACKTICK REACHED A COMMIT".

TWO THINGS ABOUT INSTALLING IT, both measured rather than assumed:

  * **`.git/hooks/pre-commit` will NOT run on this machine.** `core.hooksPath` is set globally in
    `~/.gitconfig` to `~/.config/git/hooks`, and git ignores the per-repo directory entirely when that is set.
    The first version of this hook was symlinked into `.git/hooks/` and a deliberately broken emitter was
    committed twice with it in place. Install it where the config actually looks:

        ln -sf "$PWD/scripts/hooks/pre-commit" ~/.config/git/hooks/pre-commit

    or set a repo-local path (which would SHADOW any global hooks, so read what is already there first):

        git config core.hooksPath scripts/hooks

  * **PROVE THE MUTATION, NOT THE HOOK.** The first attempt at proving it bit planted a backtick after
    `function browserScript() {` — inside the function body and OUTSIDE the template literal — so the emitter
    exited 0 and the test proved nothing about either. A trap only counts when it is inside the thing it traps.

## Release — npm is the only channel

```bash
# 1. bump agent/vale-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/vale-agent.exe agent/vale-agent-npm/vale-agent.exe
# 2. publish (pack + manifest + prune + deploy + smoke; it does NOT commit):
./scripts/publish-release.sh 1.2.N
# 3. ONE commit that includes agent/vale-agent-npm/package.json and index/public/vale-agent/version.json
git push origin main          # CI green on the pushed commit
# 4. tag through the API (git push of tags times out here) — this triggers release.yml:
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/vale/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte:
./scripts/publish-release.sh --audit-only 1.2.N
```

On the device (PowerShell) — the `--prefix` matters: without it npm installs elsewhere, reports
success, and `vale update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command vale).Source) https://agent.saisi.online/vale-agent/vale-agent-latest.tgz
vale update
vale status
```

**AND MEASURE THE PANEL THE DEVICE IS ACTUALLY RUNNING, not only the harness.** Every design sweep renders the
HARNESS (a stubbed device, this checkout's bundle); nothing measured the live panel until
`agent/scripts/live-panel-probe.mjs` was pointed at `127.0.0.1:18080` on d1 — and it found, on the first run,
`span.ag-dot 2.56<3`: the approval gate's DISARMED ring used `--faint`, the exact ink round 101 replaced in its three
sibling rings, in a rule the harness could not see because it only ever rendered the gate ARMED. The fix shipped in
1.2.438 and the same probe then reported `graphicFailing: []` on both densities. It needs a browser and a running
panel, so it cannot be a CI job: run it on the device after a `vale update` (emit with `--emit`, hand the script to the
device's node or to `browser_run_script`).

Two things that cost a device restart when ignored: **never launch a second `vale-agent.exe` from
an agent-hosted PTY** (it inherits the kill-on-close job and kills the running agent), and **never
kill/copy the exe inline over a PTY** — use the npm flow above.

## Agent layout

```
agent/src/main.rs        server binary (argv[1] = config path); Windows service via SCM
agent/src/lib.rs         crate root (DEFAULT_CONFIG_YAML embedded)
agent/src/paths.rs       registry-first install_dir()/data_dir() + layout-v2 subdirs
agent/src/state.rs       AppState (terminal_mgr, event_bus, plugin_registry, config)
agent/src/web/           HTTP surface: auth + dispatch + api_* handlers (hand-rolled Tower)
agent/src/metrics.rs     vitals + the 30 s sampler behind /api/vitals/history
agent/src/monitor.rs     reachability: persisted host:port targets, 15 s probes (TCP or HTTP)
agent/src/tools/         TerminalManager + backends (pty/ssh/serial), serial pool, ssh client
agent/src/plugins/       terminal, update, mcp_client, design, playwright, memory, system,
                         runs, monitor
agent/vale-command-core/ Plugin/ToolDef/Config/EventBus/DeviceError (vale_agent_core::)
agent/vale-agent-npm/    the npm package + the `vale` CLI (bin/vale.js)
agent/resources/panel-react/  the panel SPA (React + vitest); resources/panel/ is its build output
```

Features gate behind `terminal`/`keyring` with identical public paths across configs. A new MCP
tool is defined in its plugin's `tools.rs` (the registry caches at register time) and, to be
callable from the console, must be registered in `gateway/src/mcp-tools.ts` **and** matched by
`isDeviceDirectTool()`; after adding or removing a tool run
`VALE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`.
