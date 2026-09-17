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
| `scripts/test/feedback-check.mjs` | drop a press state the sheet had (`.tab:active`), or transition a layout property, or write `transition: all` | exit 1: "no :active for .tab:hover — a hover that answers and a press that does not is the feedback gap this checks for". It reads the BUILT sheet, skips reveal rules (`.tab:hover .tab-export` is not pressable), treats a press on a base as covering its variants (`.btn:active` answers `.btn.primary`), and its first run misread the sheet twice — a `:not(...)` suffix and a multi-line selector list — which is why it names what it could not read |
| `scripts/test/console-marks-check.mjs` | give two console signal states the same silhouette (put `off` back to a plain circle), or put the failing ink back (`--text-faint` for `off`) | exit 1 either way: ".sig-dot.off draws the same shape as .sig-dot.ok (50%|none|none) — strip the colour and they are one state", and ".sig-dot.off [light] var(--text-faint) on --bg = 2.46 (a dot is a graphic; 3 is the bar)". It reads the console's SOURCE sheets because `gateway/ui/dist` is a pruned build artifact, and its first run reported the fix as a defect because `.sig-dot.off` writes `border: 1.5px dashed …` — a SHORTHAND, which a `border-style` lookup cannot see (the panel's contrast test carries the same lesson). **IT COVERS THREE FAMILIES NOW, NOT ONE** (round 24): `.sig-dot`, the key LED `.ov-keyled` and the connection `.dot`, nine marks in all. Widening it found what one family hid — `.ov-keyled` and `.dot` were identical circles told apart by fill colour, and the Overview renders `.dot.ok` / `.dot.err` for every channel with **no rules at all**, so a healthy channel and a broken one drew the same decoration (the `.tab-wait` defect, on the other front end). Mutations: deleting a rendered state's rule trips the floor ("read 8/9 marks … so this proves nothing"); making `.dot.err` a plain fill collides it with `.dot.offline`. Distinctness is checked WITHIN a family, because a solid-with-halo means "on" in more than one place on purpose. **FIVE FAMILIES / THIRTEEN MARKS NOW** (round 26): the two LEDs joined, and they needed it — `.dev-led` had NO mark at all for "off" (ten invisible pixels where "the agent is down" is the row's whole message) and `.dev-mini-led` said it with a grey disc. Both are rings now, which is what the rest of that sheet already means by absent. Mutation: taking the ring out fails with ".dev-mini-led [light]: could not resolve transparent and --bg (a mark that cannot be measured is not a passing mark)" |
| `scripts/test/state-colour-check.mjs` | paint a neutral element with a state colour (`.side-row { color: var(--state-fail) }` on the built panel sheet) | exit 1: "panel: .side-row paints with a state colour, and matches no purpose on the list". It judges BOTH sheets (panel built, console source) and its list is not a suppression file: each entry is a PURPOSE with a reason — a mark, a status surface, the approval gate, a destructive action, a lane, a dial's tone, the update card's availability. Its first run triaged nine rules whose names do not contain the word "state" (`.dial-arc[data-tone=crit]`, `.monitor-fact-down`, `.activity-row-timeout`, …), which is the case the list exists to make somebody write down. It deliberately does NOT judge `--accent`: an accent button is an ACTION, and dressing every primary button grey to satisfy a slogan would be a worse interface |
| `scripts/test/motion-check.mjs` | take a selector out of a `prefers-reduced-motion` block (`.mem-busy` from the panel's) | exit 1: "panel: .mem-busy runs \"mem-busy-spin 1s linear infini\" and no prefers-reduced-motion block stops it". It reads both sheets and accepts either idiom — a selector named in the block, or a global `*`. Written after the CONSOLE was found relying on `--ds-dur: 0s`, which reaches transitions and no `animation:` at all; the panel was the model for that fix and the gate then found SEVEN of its own twelve unsilenced. Verified on the rendered panel too: with reduced motion emulated, the only animation under `no-preference` is xterm's cursor and there are ZERO under `reduce` |
| `scripts/test/particles-check.mjs` | put the retired palette back (`colourOf('--aura-1', '#00ffff')` in the landing), or give a copy a fallback that is not a brand colour | exit 1 either way: "index/src/page.js reads the RETIRED --aura-1 — that is the palette the rebrand removed" and "falls back to #00ffff, which is not one of the brand's colours — a fallback is what people SEE when a token is missing". It holds all three copies (panel module, console module, landing inline script) to FOUR facts: brand tokens and no retired one, every hex fallback is a brand colour, the draw is `rgba(` from a triple rather than `hsla(` from a hue, and it refuses to run under `prefers-reduced-motion` — a canvas loop is motion the CSS-level `motion-check` cannot see. Verified on the device: with reduced motion emulated the field reports rAF 0/s, clears 0/s, fills 0/s and no canvas in the DOM. Rendered evidence for the landing copy: mote cores measure 255,210,60 (= `#ffd43b` exactly), 243,156,0 and 232,87,12, with ZERO blue-dominant pixels |
| `scripts/test/exports-check.mjs` | export a name and use it only inside its own file (`export const URGENT_MS` in ApprovalGate), or export one used nowhere at all | exit 1 either way: "is exported but used only inside its own file — drop the `export`", or "is exported and used NOWHERE — delete it". It covers BOTH UIs, counts a test-only use as a SEAM rather than dead weight (34 in the panel, and a check that called those dead would be turned off within a round), and cannot see dynamic access (`mod[name]`) — nothing does that today. Its first fix run applied one remedy to both buckets and un-exported a type used nowhere, which only HID it; `noUnusedLocals` is already on in both tsconfigs, so the compiler is the second line of defence for what an un-export leaves behind |
| `scripts/test/retired-colours-check.mjs` | put a retired value back anywhere outside a comment (the accent `#d9480f` in the Rust status page) | exit 1, naming the file and the measurement that retired it. It strips comments FIRST, because its own first run failed on ten files that merely recorded the retirement — a gate that deletes its reasons is worse than no gate |
| `scripts/test/sweep-judges.bash` | plant a defect in a clean console report (an undersized target with no spacing, a theme lie, a stale delivered entry, an unreadable entry) | exit 1 per case — the CLEAN report, the spacing clause that must still PASS, and the current-entry note must all still work, so a judge that fails everything is caught too. Console-only since round 243: the extension's message-tone cases went with the extension, and its delivered-entry cases were TRANSFERRED to the console, which has the same `entryCheck` |

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
