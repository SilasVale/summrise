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

A gate that cannot fail is worse than no gate, and the only way to know is to break the thing it
guards and watch what happens. Every gate below was audited that way (rounds 65-68) — none of them is
assumed:

| gate | mutation that must fail it | result |
|---|---|---|
| `scripts/test/token-contract-check.mjs` | change a shared token's value on one side, or DELETE a spacing step from one of them | exit 1 both ways: "1 of 46 shared tokens DIVERGE" for a disagreeing value, and "console does not define --sp-3" for a deleted step — the second case is why the spacing block outlives the shared-name comparison, which by construction cannot see a name only one side still declares (round 243) |
| `cargo test --features terminal,keyring spec_snapshot` | add a parameter inside a device tool's `properties` | exit 101, snapshot diff |
| `scripts/test/panel-audit-skip-check.mjs` | make the audit `exit(0)` on a skip | exit 1, names the distinction |
| `agent/tests/fixtures/approval-grants.json` | rename a member the panel mirror reads | both sides fail |
| `agent/tests/fixtures/session-row.json` | rename `idle_ms` to `idleMs` | device + panel fail |
| `agent/tests/fixtures/embedded-bridge.json` | rename `fwd` to `forward` | shell + panel fail |
| `scripts/test/panel-design-sweep.bash` | plant a defect per axis in a report (51 checks: contrast, h1, skip, landmark, geometry, sliver, loud, mark-collision, name, title-only, reflow, focus, focus-empty, motion, motion-empty, type-floor, blind, theme-lie, harness-stale — plus the note assertions the axis loop cannot make) | one check per axis  **MARK-COLLISION (round 27): the silhouettes are now checked AS THE BROWSER PAINTS THEM.** The sheet-level unit tests cannot see a cascade override — round 25's `.plug-dot[error]` kept a stray halo through a test that passed — so the SURFACE probe reads the COMPUTED style of every state mark on every page, groups by family, and fails when two states of one mark paint identically. It cost no call sites: the probe every page already evaluates carries it. Verified on three rendered pages: four and five families each, ZERO collisions and no false positives |
| `scripts/test/release-lib.bash` | prune keeps 4 instead of 5 per minor | exit 1, actual/expected listed |
| `scripts/test/smoke-index.bash` | read the versioned installer instead of the versionless alias | exit 1 |
| `scripts/test/smoke-helpers.bash` | accept a truncated sha256 | exit 1, prints the offending value |
| `scripts/test/release-audit.bash` | stop recording mode drift | exit 1 |
| `scripts/test/publish-release.bash` | disable the stale-exe refusal | exit 1 — **after round 67 ADDED the case that does it** |
| `scripts/test/build-pins.bash` | bump rust-toolchain's channel alone | exit 1, names the workflow literal |
| `scripts/test/script-syntax.bash` | append an orphan `fi` to a shell script | exit 1, with file and line |
| `scripts/test/contrast-probe-check.mjs` | remove the probe's hex handling | exit 1, "both spellings must parse" |
| `scripts/test/e2e-only-check.mjs` | make the zero-selection guard exit 0 | exit 1, "reported success having run nothing" |
| `scripts/test/scan-dups-check.py` | stop recognising `*_test.rs` files | exit 1, names the file |
| `scripts/test/model-drift-check.mjs` | remove the normaliser's bracket-suffix strip | exit 1, prints the un-normalised id |
| `scripts/test/css-vars-check.mjs` | delete a `--token` definition its own sheet references, OR a token the CODE reads at runtime, OR (round 20) DECLARE a token nothing reads (`--token-nobody-reads`) | exit 1 either way. The runtime half was added in round 235 after both `particles.ts` files were found reading the retired `--aura-*` palette; its floors caught the first version of that scan reading ZERO references in 117 files. **IT ALSO CHECKS THE OTHER DIRECTION NOW** — a DECLARED token that nothing reads — which found twenty-one across the panel and the console (dead weight in the one block a reader consults to learn what the palette IS) and now reports "--token-nobody-reads" for a planted one. Its first run in that direction reported two tokens that exist only as PROSE in comments, because the `defined` set was built from raw text: comments are stripped first now, the same lesson `retired-colours-check` records from its own first run |
| `scripts/test/harness-fixture-check.mjs` | change the harness fixture's session-count default from 3 | exit 1, and its own self-test fails first if the pattern goes stale |
| `scripts/test/spacing-scale-check.mjs` | add a spacing declaration with an off-scale literal (a planted `13px`), or an ON-scale one where a token exists (a planted `8px`) | exit 1 either way: "off-scale spacing rose from 305 to 306", or "on-scale literals rose from 0 to 1 — a value that HAS a token was written out by hand, which is how 234 of them accumulated unnoticed". A token use replaced by a literal fails the third count |
| `scripts/test/landing-check.mjs` | lighten a label (the tertiary `#71717a` → `#9a9aa2`), take the heading away (the `<h1>` back to a `<div>`), or give a card a fixed width (`.card { width: 480px }`) | exit 1 for each: "2.68 on #fafafa, under the 4.5 AA wants for text" · "expected exactly 1 h1, found 0" · ".card { width: 480px } — wider than the 320px a 1.4.10 reflow test uses". It reads page.js's own values and BOTH themes (renamed from landing-contrast-check in round 242: a name covering a third of what it does is the kind that stops the next reader looking) |
| `scripts/test/feedback-check.mjs` | drop a press state the sheet had (`.tab:active`), or transition a layout property, or write `transition: all` | exit 1: "no :active for .tab:hover — a hover that answers and a press that does not is the feedback gap this checks for". It reads BOTH sheets — the panel's built sheet and the console's SOURCE sheet, each with its own floors (round 54) — skips reveal rules (`.tab:hover .tab-export` is not pressable), treats a press on a base as covering its variants (`.btn:active` answers `.btn.primary`), and its first run misread the sheet twice — a `:not(...)` suffix and a multi-line selector list — which is why it names what it could not read  **MEASURED AS RENDERED, WHERE ONE CONTROL HAD NO PRESS AT ALL (round 51).** This check proves an `:active` RULE EXISTS; it cannot see whether the press is VISIBLE, which is what the objective actually asks for. The first rendered measurement — `mouse.down()` on each control, comparing the computed transform / opacity / background — found every control answering EXCEPT the session tab, and only the ACTIVE one: `.tab.active` and `.tab:active` are both (0,2,0), so the later rule wins and the tab an operator presses to re-focus it did nothing at all. Fixed with `transform: translateY(1px)`, which a background rule cannot override, and it is what every other control already used. RENDERED AFTER: all three tabs report PRESS RENDERS. The measured set, for the next reader: `.rail-btn` 38x38, `.tab-close` 24x24, `.side-add` 30x21, `.btn` 54x26 and `.tab` 96x34 all answer — and a sheet-level check would have passed every one of them either way. **AND THE CLASS IS NOW AUTOMATIC (rounds 52-53): A PRESS WHOSE EVERY PROPERTY IS SHADOWED CANNOT BE SEEN.** The check groups every `:active` rule by the element it presses, collects the declarations the group makes, and fails when a LATER rule that names the same element at equal-or-higher specificity overrides every one of them — which is what killed the active tab's press. It found FOUR more, all rows whose own `:hover` rule sets a different background: `.side-row`, `.run-strip-head`, `.archive-row`, `.path-attention-row`. Hover ALWAYS co-occurs with active, so at equal specificity the hover rule won every time. Fixed with `transform: translateY(1px)`, the vocabulary every other press already uses; `.side-row` was measured DEAD in round 52 at 232x34 and reports `none -> matrix(1, 0, 0, 1, 0, 1)` now. IT SHED THREE CLASSES OF FALSE POSITIVE FIRST, and each is a way this kind of check lies: substring matching (`.btn-ghost` contains `.btn`, so `.btn`'s healthy press was reported shadowed), DESCENDANT selectors (`.cmd-btn.cmd-toggle.open svg` names the class while styling a chevron), and same-value rules (a later rule saying the same thing is not a shadow). And a fourth lie was MINE, not the check's: my first mutation removed a selector from a list that no longer had it, my `assert` looked for a string that was never expected to exist, and the gate rightly passed a sheet that had not changed — caught by grepping the BUILT sheet for the rule instead of trusting the edit. A floor of 20 presses and a count in the summary line  **AND THE CONSOLE HAD NO PRESS CHECK AT ALL UNTIL ROUND 54.** This gate had only ever read the panel's sheet. Pointed at the console's source it found 25 `:hover` selectors and ONE press rule: TEN controls — the rail button, the avatar, the logout item, the icon button, the language button, the auth tab, `.btn-dashed`, `.card-link`, `.dev-mini` and every link — acknowledged a hover and stayed completely still under a press. Fixed with a press layer that respects what each element IS: `transform: translateY(1px)` where it renders, `scale(0.96)` for the avatar whose hover is a scale, and OPACITY for `a` and `.card-link` because A TRANSFORM DOES NOT MOVE AN INLINE BOX. It also found six `transition: all var(--ds-dur)` rules the console had never been checked for — each now names the properties that element actually animates. AND THE VARIANT EXEMPTION IS MEASURED NOW, NOT ASSUMED. The panel writes `.btn.primary` (dot form) so a press on `.btn` provably covers it; the console writes `className="btn btn-primary"` (two classes, dash form), where the same is TRUE but no prefix rule can know it. The check reads the markup and grants the exemption only where both classes appear on one element — which is why `.btn-dashed`, used ALONE in this console, was reported as a real gap instead of being waved through with its siblings. Mutation: removing `.lang-btn:active` fails with "console: no :active for .lang-btn:hover". Console presses are verified at the SHEET level here; the rendered press measurement exists for the panel (round 51) and not yet for the console. |
| `scripts/test/mark-vocabulary-check.mjs` | stop the console's failure mark from being a diamond (`.dot.err` loses `rotate(45deg)`), or give the panel's `waiting` the `idle` ring | exit 1 both ways: ".dot.err means FAILURE and does not draw the diamond (transform: none) — the panel spells that state as a diamond", and for the panel "the panel's most urgent state draws \"ring\", not a diamond — the shape that means \"this wants you\" has moved" plus "the panel's four states share 1 silhouette(s)". It is the ONLY check that reads both surfaces at once — every other marks check is about one of them — and it reads the panel's vocabulary as DATA (`liveness.ts`) and the console's as PAINTED (its sheet, with the same signature rule `console-marks-check.mjs` uses). Measured 2026-09-18: panel `waiting`=diamond `working`=solid-halo `idle`=ring `off`=dashed-ring; console 11 marks, 4 distinct shapes, agreeing on attention=diamond, absent=ring, fine=fill |
| `scripts/test/console-marks-check.mjs` | give two console signal states the same silhouette (put `off` back to a plain circle), or put the failing ink back (`--text-faint` for `off`) | exit 1 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%|none|none) — strip the colour and they are one state", and ".sig-dot.off [light] var(--text-faint) on --bg = 2.46 (a dot is a graphic; 3 is the bar)". It reads the console's SOURCE sheets because `gateway/ui/dist` is a pruned build artifact, and its first run reported the fix as a defect because `.sig-dot.off` writes `border: 1.5px dashed …` — a SHORTHAND, which a `border-style` lookup cannot see (the panel's contrast test carries the same lesson). **IT COVERS THREE FAMILIES NOW, NOT ONE** (round 24): `.sig-dot`, the key LED `.ov-keyled` and the connection `.dot`, nine marks in all. Widening it found what one family hid — `.ov-keyled` and `.dot` were identical circles told apart by fill colour, and the Overview renders `.dot.ok` / `.dot.err` for every channel with **no rules at all**, so a healthy channel and a broken one drew the same decoration (the `.tab-wait` defect, on the other front end). Mutations: deleting a rendered state's rule trips the floor ("read 8/9 marks … so this proves nothing"); making `.dot.err` a plain fill collides it with `.dot.offline`. Distinctness is checked WITHIN a family, because a solid-with-halo means "on" in more than one place on purpose. **FIVE FAMILIES / THIRTEEN MARKS NOW** (round 26): the two LEDs joined, and they needed it — `.dev-led` had NO mark at all for "off" (ten invisible pixels where "the agent is down" is the row's whole message) and `.dev-mini-led` said it with a grey disc. Both are rings now, which is what the rest of that sheet already means by absent. Mutation: taking the ring out fails with ".dev-mini-led [light]: could not resolve transparent and --bg (a mark that cannot be measured is not a passing mark)"  **ELEVEN MARKS / FIVE FAMILIES, AND A NEW KIND (round 44).** The family list is the LIVE one: `.dot.online` had no producer (no view renders it — the Overview renders `ok`/`err`) and `.dot.offline` restated the base rule verbatim, so both were pruned. And the signature can no longer collapse a mark that is TWO things into one: a fill plus an inset shadow used to compute as `ring`, which is how a real defect could pass. `ring+fill` is now its own kind and a FAILURE — a mark that is both is neither. Mutation: deleting `.dot.err`'s `box-shadow: none` fails with ".dot.err is a FILL inside a RING — the vocabulary is solid / ring / halo / empty, and a mark that is two of them is neither." **AND THE WAY THAT RULE WAS FOUND IS WORTH THE LINE:** the first mutation I tried (`.dot.err` as a 50% circle) did NOT bite, and I read that as a hidden defect — but `.dot.err` was already correct (`box-shadow: none` plus `rotate(45deg)`, i.e. the diamond the vocabulary asks for) and my "fix" was a duplicate declaration, removed. A mutation that does not bite is evidence about the MUTATION first: it was aimed at `.dot.offline`, which this same round had pruned. Read the artefact before believing the instrument — the ninth time tonight, and the first time the instrument was right and I was wrong about the defect. |
| `scripts/test/state-colour-check.mjs` | paint a neutral element with a state colour (`.side-row { color: var(--state-fail) }` on the built panel sheet) | exit 1: "panel: .side-row paints with a state colour, and matches no purpose on the list". It judges BOTH sheets (panel built, console source) and its list is not a suppression file: each entry is a PURPOSE with a reason — a mark, a status surface, the approval gate, a destructive action, a lane, a dial's tone, the update card's availability. Its first run triaged nine rules whose names do not contain the word "state" (`.dial-arc[data-tone=crit]`, `.monitor-fact-down`, `.activity-row-timeout`, …), which is the case the list exists to make somebody write down. It deliberately does NOT judge `--accent`: an accent button is an ACTION, and dressing every primary button grey to satisfy a slogan would be a worse interface |
| `scripts/test/motion-check.mjs` | take a selector out of a `prefers-reduced-motion` block (`.mem-busy` from the panel's) | exit 1: "panel: .mem-busy runs \"mem-busy-spin 1s linear infini\" and no prefers-reduced-motion block stops it". It reads both sheets and accepts either idiom — a selector named in the block, or a global `*`. Written after the CONSOLE was found relying on `--ds-dur: 0s`, which reaches transitions and no `animation:` at all; the panel was the model for that fix and the gate then found SEVEN of its own twelve unsilenced. Verified on the rendered panel too: with reduced motion emulated, the only animation under `no-preference` is xterm's cursor and there are ZERO under `reduce` |
| `scripts/test/particles-check.mjs` | put the retired palette back (`colourOf('--aura-1', '#00ffff')` in the landing), or give a copy a fallback that is not a brand colour | exit 1 either way: "index/src/page.js reads the RETIRED --aura-1 — that is the palette the rebrand removed" and "falls back to #00ffff, which is not one of the brand's colours — a fallback is what people SEE when a token is missing". It holds all three copies (panel module, console module, landing inline script) to FIVE facts (the fifth added in round 48): brand tokens and no retired one, every hex fallback is a brand colour, the draw is `rgba(` from a triple rather than `hsla(` from a hue, and it refuses to run under `prefers-reduced-motion` — a canvas loop is motion the CSS-level `motion-check` cannot see. Verified on the device: with reduced motion emulated the field reports rAF 0/s, clears 0/s, fills 0/s and no canvas in the DOM. The fifth is that THE THREE COPIES DRAW THE SAME FIELD — the same cap, the same density and the same peak alpha — which nothing compared until round 48: the numbers happened to agree (90 / 5.5 per 100000 / MAX_ALPHA * twinkle * 0.35) and would have drifted apart in silence, since facts 1-4 are all about where the COLOUR comes from and three copies can agree on every one of them while drawing visibly different fields. The density needed care: the modules write `DENSITY = 0.55` and apply it as `(w * h / 100_000) * DENSITY * 10`, the landing writes `(w * h / 100000) * 5.5`, so the check resolves the named constant and compares the EFFECTIVE per-100000 value instead of the literals. Mutation: `MAX_MOTES = 140` in the landing fails with "index/src/page.js draws a different field: cap is 140 where the others are 90". Rendered evidence for the landing copy: mote cores measure 255,210,60 (= `#ffd43b` exactly), 243,156,0 and 232,87,12, with ZERO blue-dominant pixels |
| `scripts/test/exports-check.mjs` | export a name and use it only inside its own file (`export const URGENT_MS` in ApprovalGate), or export one used nowhere at all | exit 1 either way: "is exported but used only inside its own file — drop the `export`", or "is exported and used NOWHERE — delete it". It covers BOTH UIs, counts a test-only use as a SEAM rather than dead weight (34 in the panel, and a check that called those dead would be turned off within a round), and cannot see dynamic access (`mod[name]`) — nothing does that today. Its first fix run applied one remedy to both buckets and un-exported a type used nowhere, which only HID it; `noUnusedLocals` is already on in both tsconfigs, so the compiler is the second line of defence for what an un-export leaves behind |
| `scripts/test/retired-colours-check.mjs` | put a retired value back anywhere outside a comment (the accent `#d9480f` in the Rust status page) | exit 1, naming the file and the measurement that retired it. It strips comments FIRST, because its own first run failed on ten files that merely recorded the retirement — a gate that deletes its reasons is worse than no gate |
| `scripts/test/sweep-judges.bash` | plant a defect in a clean console report (an undersized target with no spacing, a theme lie, a stale delivered entry, an unreadable entry) | exit 1 per case — the CLEAN report, the spacing clause that must still PASS, and the current-entry note must all still work, so a judge that fails everything is caught too. Console-only since round 243: the extension's message-tone cases went with the extension, and its delivered-entry cases were TRANSFERRED to the console, which has the same `entryCheck` |

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

## Committing

**A pre-commit hook runs the emitters** (`scripts/hooks/pre-commit`, round 225). Five scripts build a
standalone script for the device inside a template literal, and a backtick anywhere inside that literal ends it
early — the emitted file then stops parsing in the middle of 30 KB. That has happened **34 times**, and in the
last six the failing check was already on screen: `emit=1`, and the commit made anyway. The hook runs the
emitters' OWN guards, so it cannot disagree with what it guards.

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
