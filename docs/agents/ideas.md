# Ideas inbox — the user's

Anything in here is the loop's TOP-PRIORITY work, and it is the anchor against
drift: round 100's "aurora" was the loop *interpreting* a request instead of
following one, and the user had to say plainly that they never wanted it.

Rules for the loop:

- Entries arrive here written by the user, or said in the session (append them
  here so they survive the round).
- Each entry gets either a round of its own, or an explicit reasoned verdict in
  the round log — with evidence — if the loop judges it not worth doing.
- Never dropped silently. Never reinterpreted into something else.
- `docs/CHARTER.md` is the user's alone; the loop may only propose changes.
- **THE PRE-RENAME NAMES STAY, AND THIS IS HOW TO READ THEM.** The product was `vale` before it was `summrise`.
  Rows written before the rename cite names that were REAL THEN and do not exist now: `vale status` is
  `summrise status`; `agent/vale-agent-npm/` is `agent/summrise-agent-npm/`; `index/public/vale-agent/` is
  `index/public/summrise-agent/`; and `/vale-agent/*.tgz` on the CDN is `/summrise-agent/*.tgz`. The rows are
  history and are not rewritten — a reader verifying one should read its paths through this note. (Added round
  189: two `done` rows cited `agent/vale-agent-npm/package.json`, which no longer exists, and a `done` row a
  reader cannot check is not evidence.)

| # | Idea (the user's words) | Status | Where it landed |
|---|---|---|---|
| 1 | The project should iterate and design itself; my creative direction enters through here | standing | `docs/CHARTER.md` (what counts as success) + `AGENTS.md` (how the work is done: build, test, the proven-gate ledger). The protocol document this row used to cite, `docs/agents/iteration-loop.md`, was pruned with the iteration journal in 2bb98183 — at your instruction, because the loop was spending its rounds writing markdown — so the row no longer points at it. |
| 2 | 我不是说了断舍离吗，这种没有用的功能加了干嘛啊 | done | The `monitor_note` tool, its route, its panel rendering and its CLI projection were DELETED (rounds 32-33), and so were `vale watch\|report\|monitor wait\|monitor note`. Verified in round 61: no `monitor_note` remains anywhere under `agent/src/`. |
| 3 | 加这个按键条干嘛 | done | The console key bar (`ConsoleKeys`), its lib, its tests and its CSS were removed; verified in round 61: zero references in the panel. |
| 4 | 可以做个新手引导之类的现在功能我都不知道怎么用 | done | `GettingStarted.tsx` + `lib/gettingStarted.ts`: three steps, reopenable from the rail's `?`, version-keyed dismissal. Verified present in round 61. |
| 5 | 有些多余的功能学会舍弃或者合并 | done | The Archive and Activity pages were merged behind one rail icon (round 31); the key bar and `monitor_note` deleted. |
| 6 | 不要经常使用sleep，使用后台任务监听不好吗 | done | Long waits are background jobs with `job_output`; sleeps are only used where a wall-clock boundary is the thing under test, and the one place that needed a longer wait (the panel's 30s poll) is measured by fake timers instead. |
| 7 | 你能全用英文吗，goal目标也同时改成英文 | standing, **CLARIFIED 2026-09-26** | **THE SCOPE IS THE ARTIFACTS, NOT THE CONVERSATION.** The operator clarified in session: *"我说的是改代码用英文"* — commit messages, code comments, the repository's own documents (`AGENTS.md`, `CONTEXT.md`, the specs) are ENGLISH. **Talking to the operator is in the operator's language**, which is Chinese. The row used to read "Session language is English", and an agent read that literally and answered in English — which is the misreading this note exists to prevent. The goal text is English (`goal-905e1213`, revision 28 at round 61). |
| 8 | goal加一个多花些心思在页面设计上面 | done | The design axes are measured, not asserted: contrast (rendered probes + a static pair sweep), hierarchy/landmarks, focus and hover states, geometry at 320-1440px, WCAG reflow, accessible names, boot/interaction timing — packaged as `agent/scripts/*-design-sweep.mjs` with gates. Rounds 39-60. |
| 9 | 跑完这轮先暂停 (said in this session, round 54) | done, then resumed | The goal was paused at revision 27 immediately after that round finished; it was resumed afterwards (revision 28), so the loop is running again. |
| 10 | (the loop's ask, recorded here because it needs YOU) | **done — 2026-09-21** | **d1 is current, twice over.** `vale status` on the device reports `release: 1.2.437 · this CLI: 1.2.437 · latest: 1.2.437 (this device is current)` with `update: none in flight`, and the panel it serves (660,231 bytes) carries this session's three new captions (`raw audit log`, `who ran each step`, `strip-label`) — verified by fetching `http://127.0.0.1:18080/panel/panel.js` from the device and counting the strings. The 1.2.403 note above was stale long before this row was written: the device had already moved to 1.2.435 by the time anyone read it. The restart closed the open sessions as warned; the sweep PTYs were reopened afterwards. |
| 11 | (the loop's ask, recorded here because it needs YOU) | waiting on you | `docs/CHARTER.md` cites `docs/agents/iteration-loop.md` in its opening note ("this file is the one thing the iteration loop may only PROPOSE — see ..."), and that file no longer exists: it was pruned with the iteration journal in 2bb98183. Two other citations were mine and are fixed (this inbox, and `agent/vale-agent-npm/test/cli.test.mjs`, which now states the promise instead of referencing it). CHARTER is yours alone, so the fix is yours too — either drop the parenthetical or point it at `AGENTS.md`. |
| 16 | answered in round 237 — one real defect, fixed; two kept with measurements | **The retired accent in two ink tokens.** Only ONE of the four survivors was a defect: the console's `--chrome-active-ink` is TEXT on `#ffefe5` and measured **3.83** (AA needs 4.5), now `#bf3a0a` = **4.90**; the panel's `--chrome-active-ink` inherited it through `var(--accent-ink)` and was fixed too. `--accent-ink` itself is a GRAPHIC use (4.12 light / 3.78 dark against a 3:1 need) and the terminal cursor measures 4.28 on its own `#131418` where the "correct" accent would give 3.35 — both kept, with the reasons in the retired-colours gate. |
| 15 | done — round 14 of the standing goal | **The particles draw the brand, not a retired palette.** Both `particles.ts` copies asked for `--aura-1/3/4` (removed in the rebrand, so they read EMPTY) and fell back to 190 cyan / 280 violet / 330 pink — the ambient layer behind every surface, in colours the brand had abandoned. They read `--brand-grad-a`, `--brand-grad-b` and `--accent` now, and **as colours rather than hues**: the old code turned a token into a hue and drew `hsla(h 90% 62%)`, so even a correct token would not have been the colour on screen. The console had NO `--brand-*` tokens at all, which is why it fell back — they are declared there now. The `css-vars-check` runtime waiver that held `--aura-*` open ("the replacement hue is a design decision") is REMOVED, so a regression fails like any other undefined token. Verified on the rendered panel by sampling the canvas: the mote cores cluster on `#c2410c` / `#9a3412` / `#bf3a0a`, and **not one painted pixel has blue above red** in either theme — the signature the cyan/violet/pink fallback could not have avoided. |
| 14 | closed — the subject was removed (round 243) | **Which accent is the brand?** — the extension that made this a question was removed in round 243 (it shipped nowhere, its feature was off by default, and the half it existed for had already been deleted). The evidence that settled the surrounding questions is still worth keeping: the logo is gold `#f59f00` on orange `#e8590c`, the panel and console use that orange family, and the extension was the only surface with no counterpart in the brand assets. No decision is needed. |
| 13 | (the loop's ask, recorded here because it needs YOU) | waiting on you | **One symlink would stop a mistake the loop has made 34 times.** Five scripts here build a standalone script for the device inside a template literal, and a backtick anywhere inside that literal ends it early — the emitted file stops parsing in the middle of 30 KB. That has happened **38 times** [ROUND 272: the count is unreliable — the hook said 34 and AGENTS.md said 52, and the three never agreed. The class itself is now GONE: rounds 266-271 moved all five payloads out of template literals into real modules that the assembler compiles, so no emitter can print a broken script any more. What the hook still buys is the only end-to-end assembly of all five artifacts in under a second. The symlink is still worth one line — it is cheap and it now catches a payload module that does not parse or a require the assembler cannot resolve — but it is no longer the difference between a red CI and a green one] — FOUR of them in the night of rounds 24-38 alone, and every one of those four was caught by the emitter's own guard BEFORE the commit, because this loop runs `scripts/hooks/pre-commit` by hand. That is the whole argument for the symlink, stated by measurement: the hook works, and the only thing standing between it and every commit is that a human-shaped loop has to remember to run it. There is now a pre-commit hook that runs the emitters' own guards (`scripts/hooks/pre-commit`, round 225). **On this machine it cannot run:** `~/.gitconfig` sets `core.hooksPath` to `~/.config/git/hooks`, and git ignores a repository's own `.git/hooks/` entirely when that is set — so a deliberately broken emitter was committed twice with the hook installed. That directory holds exactly one hook of yours (`post-commit`, tokensave auto-sync) and the **`pre-commit` slot is free**. The hook has been made safe to put there: it exits 0 immediately unless it is in this repository, so it is inert in every other repo you own — verified from a non-Vale directory and from your global hooks dir itself. The loop has not written into your global config. To install it: `ln -sf $PWD/scripts/hooks/pre-commit ~/.config/git/hooks/pre-commit`. **THE ARGUMENT IS NO LONGER HYPOTHETICAL — IT HAS COST A PUSH (round 93).** Commit `cd7aa961` carried a backtick in a comment inside the probe's template literal. The module stopped PARSING — not the emitted script, the module, at import — and **five of the ten CI jobs went red on the pushed commit**: ui, panel (vitest), gateway, design and pack-chain, everything that imports it. The hook refuses that exact commit in under a second, and so does `contrast-probe-check.mjs`, which CI already runs at `ci.yml:506`. The only thing that did not happen is the hook being invoked, because nothing invokes it unless the loop does. The next round then repeated the mistake inside its own new comment — and the hook, run by hand as the first thing after the edit, caught it in under a second (47th time). That is the whole difference the symlink makes: not a smarter loop, just a guard that runs. |
| 12 | (the loop's ask, recorded here because it needs YOU) | waiting on you | **Seventeen files cite design documents your prunes deleted.** Verified directly (round 73): `docs/solid-program.md`, `docs/adr/proposal-game-design.md`, `docs/superpowers/specs/*` and `agent/CLAUDE.md` do not exist, and the deletions were deliberate — `25507166` "delete the remaining design-doc apparatus" and `2bb98183` "delete the iteration journal". The citations survived: five Rust modules cite `solid-program.md` for a design sign-off, five panel modules cite the game-design ADR, six files cite a desktop-core spec. I fixed the one that mattered operationally (publish-release's usage message pointed at `agent/CLAUDE.md` for a step it already spells out). The rest are provenance pointers, and whether they should be reworded, left as history, or have a document restored is your call — restoring one would run against the instruction that produced the prune, which is why I did not.  **RE-MEASURED IN ROUND 154: twenty-two files, not seventeen.** The count moved because the repo did — and the breakdown matters more than the number, because only some of it is yours to decide about: SEVEN panel modules (`panel-react/src/`: App, PathView, SettingsPage, usePlugins, boot, path, recipe) and SEVEN Rust files (`paths.rs`, `terminal/tools/files.rs`, `terminal/mod.rs`, `summrise-command-core/src/lib.rs`, `tests/dep_surface.rs`), plus `gateway/src/plugins/translate.ts` AND its published mirror `gateway/public/code/files/summrise-gate/src/plugins/translate.ts` — that pair is one fact in two places, so a reword has to touch both — plus two of the repo's own documents (`inventory.md`, `gateway/DEVICE-INTEGRATION.md`). The remaining EIGHT are not repo source at all: five skill definitions under `.agents/skills/` and three historical reports under `.superpowers/sdd/`. Method: `grep -rlE 'solid-program\|proposal-game-design\|superpowers/specs\|CLAUDE\.md'` over the source extensions, excluding `agent/target`, the ledger and this file; the three documents are still absent. The call — reword, leave as history, or restore one — remains yours, and restoring one still runs against the instruction that produced the prune. |
| 17 | (the loop's question, recorded here because it needs YOU) | waiting on you | **Does one shape mean two things?** The console's device LEDs and the panel's `running` marks are both SOLID WITH A HALO, and the halo reads as "lit" on a device row and as "in flight" on a command row. They may be one idea — *something is happening* — or two, in which case one of them needs a different silhouette. Measured in round 26 while giving the verdict dots their shapes, and deliberately NOT changed: neither surface can be shown to be wrong, and the fix depends on which reading you want. The mark language is otherwise settled (shape first, colour second; ok solid, warn square, fail diamond, running haloed, absent a ring or a dash) and is enforced at three levels — the sheet, the rendered page, and the ink contrast — with mutations proving each. |
| 19 | Are these controls duplicated? A lot of it I cannot understand (round 68, two screenshots) | one fixed, three recorded | **MEASURED AGAINST THE CODE, AND THE THREE SESSION CONTROLS ARE NOT DUPLICATES.** `Take control` is `held_by_human` (`SessionControl.tsx`), `Ask before each command` is `approval_required` (`ApprovalGate.tsx`) and `+ Set a goal for this session` is `goal` (`GoalBar.tsx`) — three different facts off the wire. What makes them look like one thing is that all three wear the same pill and the state is only the small circle: the first two are TOGGLES whose label flips (`Hand back`, `Asking first`) and the third is an ACTION that opens an input. **`Trajectory` and `Path` are not duplicates either** — the component's own comment says it: one is "the raw audit timeline — every event, exactly as logged", the other "this session's work as steps, with a summary of how it went" — but the comment also says the two labels "are not self-explanatory next to each other", and the meaning is carried by `title` tooltips, which do nothing until hovered. That is why they read as two names for one screen. **FIXED THIS ROUND:** the Runs counter said `479 records with no run` beside `500 records`, which reads as a defect to investigate; it now says `479 of 500 records name no run` with the reason in its tooltip — a run exists only where a session DECLARED one, so records from a session that never set a goal are unattributed, which is the normal case. The test that pinned the old wording now pins the share and the reason. **THREE THINGS ARE YOURS TO DECIDE, because they are taste and not correctness:** (a) rename the two view tabs so the difference is visible without hovering — `Timeline` / `Steps` say what `Trajectory` / `Path` only imply; (b) whether History should keep `Sessions` and `Runs` as two tabs or become one page with a filter, since a run and a session are two readings of the same work; (c) whether the per-row `No records carry this run id.` should stay now that most runs in your list are empty ones — it is honest on one row and noise on five. |
| 18 | (the loop's ask, recorded here because it needs YOU) | **half done — round 156: the false sentence is fixed; the constant's home is still yours** |**One sentence in `vale-gate` is false, and it is the reason a constant moved here.** The worker's `cachedDeviceProbe` says its 30-second cache is justified because "the console polls every 30s already". The console polls every **60s** (`Overview.tsx` and `DevicesPanel.tsx`, each with its own literal until round 49). The behaviour is right — a 30s cache behind a 60s poll always answers fresh, so the cache serves a second view or a manual refresh and never the console's own request — but the stated reason is wrong, and a false reason is worse than none: it is what a future reader would trust when deciding whether the cache can grow. The console's side is one constant now (`CONSOLE_POLL_MS` in `lib/deviceState.ts`, whose comment records the discrepancy). The worker's sentence needs an edit in that repository. **FIXED IN ROUND 156.** The sentence said the console polls every 30s; it polls every 60s (`CONSOLE_POLL_MS = 60_000`, one constant both views read). It now states the MEASURED reason instead: a 30s cache behind a 60s poll is never what serves the console's own request, so the margin exists for a second view and a manual refresh, and any value comfortably under the poll would do — which is what a future reader needs when deciding whether the cache can grow. The remaining question — whether `CONSOLE_POLL_MS` should live somewhere both crates read — is unchanged and still yours. |
| 20 | asked in this session: "没有看到github release啊" (round 95) | **done — two releases cut and applied (2026-09-21)** | **Releases exist; the newest contains nothing from the last two days.** Measured through the API: the latest release is **v1.2.435, published 2026-09-18 15:47 UTC**, not a draft and not a prerelease, with the asset `vale-agent-1.2.435.tgz` (6,905,807 B, 1 download). Tags run to v1.2.435, `agent/vale-agent-npm/package.json` reads `1.2.435`, and the CDN pointer `index/public/vale-agent/version.json` agrees with a recorded sha256 — so nothing is version-behind and no release is missing. **What is unreleased is everything since that publish: 50 commits, 4 of them under `agent/resources/panel-react/`** — including the SVG measurement rule, the active tab's 1.99:1 fix, and the round-95 press fix. Two ways you could be looking at a repo that shows you none of this: this checkout's remote is the git PROXY (`v.saisi.online/api/git/SilasVale/vale.git`), whose web UI may not render GitHub's Releases at all; or you are looking for a release that carries the recent design work, and no such release has been cut. The loop has not cut one unasked, because publishing deploys to the CDN and moves your device's update path: `./scripts/publish-release.sh 1.2.N` → one commit → tag through the API → `release.yml` builds and attaches the asset. **Say go and it is one round's work.** |
| 21 | asked in this session: "为什么代码库里面有saisi.online的字样呢" (round 95) | answered — three options, none taken without you | **It is your own domain, and it is in 116 tracked files because this repository IS its infrastructure** — none of it is a third party. Grouped by hostname: `d1.agent.saisi.online` (109, plus d2/d9-d11/d1b/dfresh/ok1/pv1-3 — per-device tunnel hostnames, most of them TEST FIXTURES), `agent.saisi.online` (94 — the CDN and update channel, `/vale-agent/*.tgz` and `cloudflared.exe`), `api.saisi.online` (69 — console + relay upload), `ai.saisi.online` (69 — the MCP endpoint), `v.saisi.online` (31 — the git proxy), then `oracle/zen-us/zen-go/dsh/opencode/vscode` (your other services, in `proxies/` and deploy scripts) and a handful of DELIBERATELY INVALID hostnames in security tests (`evil.agent…`, `notagent…`, `renamed.agent…`, `moved.agent…`). By tree: `gateway/test` 20, `gateway/public` 12 (the console's built source browser), `gateway/ui` 10, `proxies/` 12, `agent/src` 9, `agent/deploy` 9, docs and fixtures the rest. **Why it is there:** the shipped agent must know where to fetch updates and `cloudflared` from and what to name its tunnel. Two of those are hard defaults rather than configuration — `CLOUDFLARED_PROXY_URL` in `agent/src/tunnel.rs` is a plain `const`, and `download_url`/`console_url` default in `agent/src/bootstrap.rs`; the rest is overridable config. The domain is not a secret (it already ships inside the exe and the npm tarball). **The options, scoped:** (a) leave it — it is product infrastructure; (b) make the two hardcoded ones configurable (small, no rename); (c) rename to a product domain — touches the update channel, the tunnel ingress, the console URL and the fixtures, and needs your go for the same reason a release does. |
| 22 | (the loop's question, and it now has a data path under it) | waiting on you | **What SHAPE should a failed session's mark have?** Round 96 made the device report `last_exit_code` per session, so the panel can now say "this session's last command FAILED" about every session — not only the one whose audit trail it has loaded — and it says it TODAY as a text chip (`exit 1`, measured 5.96 light / 5.65 dark as rendered). It does NOT say it as a mark, and that is deliberate: the mark language is one shape per LIVENESS state, and a failure is not liveness (a session can be idle and have failed). Giving it a fifth silhouette needs your call, because the two surfaces already disagree about one shape: the CONSOLE spells failure as a DIAMOND, and the panel spells WAITING as one, and `waiting` and `failed` would sit side by side in the same tab strip. The candidates, with what each costs: (a) **TRIANGLE** — the one shape neither surface uses for a state, and the console already uses it for warn in the boot mark, so it reads as "something is wrong" without being learned; (b) **SLASHED RING** — says "blocked/failed" and is distinct from the panel's `off` (a dashed ring) at a glance, but it is a new drawing idiom; (c) **reuse the DIAMOND with a different fill** — cheapest and wrong: shape is the first channel precisely so that colour is not load-bearing, and this would collapse two states into one silhouette. The loop's recommendation is (a). **ANSWERED BY THE LOOP IN ROUND 97, PENDING YOUR WORD: IT TOOK (a).** The failed state now has its own silhouette — a TRIANGLE, drawn with clip-path, radius 0, in the danger family's readable ink (`--danger-on-soft`; the first version used `--state-fail` and measured **2.87:1** on the dark ACTIVE session row, under the 3:1 a graphic needs, so the token was chosen by measuring the four surfaces a mark actually sits on). Precedence is off > waiting > working > failed > idle: a failure is about what already happened, so anything happening NOW outranks it, and it lingers until the device clears the code on the next command. The text chip is PRUNED — one fact, one focal point; the exit code is one hover away on the mark's title. Rendered and measured on the device: four states of one mark family on one page (`mark[waiting,working,failed,idle]`), **zero collisions**, light 165,29,29 / dark 255,135,135. If you would rather have (b) or (c), the change is one entry in `SILHOUETTE` plus one CSS rule — say so and it is a round's first five minutes. |
| 23 | the SAME question as row 21, asked again ("把saisi.online这些敏感信息从项目中去掉"), with the operator's blanket "you may decide the whole project" | **DECIDED — option (b) plus a gate; the rename still starts on your word (round 44 of the goal)** | **Re-measured before touching anything: 461 occurrences across 118 files** (row 21 counted 116 — the growth is this loop's own ledger and inventory notes, mine to clean). Three quarters of it is exactly what row 21 described: per-device tunnel hostnames in `gateway/test` — **145 occurrences, nearly all FIXTURES** — plus the two hard defaults at `agent/src/tunnel.rs:73` (`CLOUDFLARED_PROXY_URL`) and `agent/src/bootstrap.rs` (`download_url`, `console_url`). **TAKEN:** the two hard defaults become CONFIGURATION (the device already reads `download_url`/`console_url` from its config, so `cloudflared`'s URL joins them); the fixtures move to a documented test domain, which removes ~145 occurrences mechanically and costs nothing but churn in tests that were never about the hostname; what remains gets ONE declared place and a **gate** that fails a new production host outside it — the shape `retired-colours-check` already uses, so the rule is enforced rather than remembered; and this loop's own notes stop spelling the hosts. **NOT TAKEN, even with the delegation: the rename.** The strings already ship inside every exe and every tarball, so removing them from HEAD un-publishes nothing — while a rename touches the LIVE update channel, the tunnel ingress, the console URL and the device fleet, where a mistake does not fail a test, it stops real devices from updating. That one is a deliberate project with DNS in it, and row 21 already said the same thing: it needs your go for the reason a release does. |
| 24 | (the loop's question, rounds 160-161: a measurement with no verdict) | **answered — round 153: the repo had already decided it** | **Which device tools are DELIBERATELY not console-reachable, and which are forgotten?** `agent/spec-tools.json` lists 56 device tools; 19 are not named in `gateway/src/mcp-tools.ts`, the registry the console calls through. A tool may be absent on purpose (`mcp_client_*` would invite recursion, `memory_*` is the device's own store) or by oversight, and **nothing in the repository states which is which** — so no gate can tell them apart and I will not invent the intent. What IS gated: the routing partition ("registered but unrouted is a programming error", pinned in `mcp-handler.test.mjs`). One sentence from you turns the other half into a declared list with reasons, and a forgotten registration into a failure. | **THE ANSWER IS `gateway/test/mcp-handler.test.mjs`'s `NOT_EXPOSED`.** TWENTY-FOUR device tools are deliberately off the console MCP surface, each with a stated reason. Five of them are ALIASES whose canonical name IS registered (`sftp` = "legacy alias of terminal_sftp", and `terminal_secret_set`/`get`/`delete` = "alias of secret_set/get/delete (registered)"), which is why this row's own "19 not named in mcp-tools.ts" is right about the other question: 24 declared, 5 of them aliases, 19 both unregistered AND declared. The rest: the OS surface beyond the sanctioned transfer pair (four `system_file_*` — "`terminal_*` covers it"; three `system_process_*`/`system_net_test` — "a PTY away"); the device-local knowledge base (six `memory_*` — "panel surface"); the playwright-mcp bridge plumbing (four `mcp_client_*` — exposing it lets a client route around the `browser_*` tools); `agent_update` ("self-modifying — a CLI action, not an MCP call"); `page_view` ("legacy remote-page helper"); and `terminal_sftp` ("superseded by the relay pair; takes arbitrary SSH credentials"). The map carries its own rule — "each needs a reason, an unexplained absence is exactly the bug this map exists to prevent" — and round-554 is why it exists: 21 of the agent's 49 tools were once invisible AND uncalled with every gate green. Choosing WHICH of these to expose is a product decision and remains yours; the inventory of what is currently withheld, and why, is not.
| 26 | "你能优化下panel页面显示吗" (said in this session), plus "还有一点忘记说了任务管理器没有vale agent logo" | **done — round 265**, both asks; nothing published | The operator chose "measure first, fix what is objectively wrong" over naming a page, so the round measured the six live pages before it changed anything. **Found and fixed: prose had no measure.** At 1440px, in both densities, the Settings page rendered twelve paragraphs as SINGLE lines of 93-206 characters and the Plugins subtitle as one of 85 — while the History lede three blocks away has carried `max-width: 66ch` all along. Now 66ch on those surfaces: worst 206 -> 76 cpl, verified rendered in both densities, and a new sweep axis (`measure`, floor 90, plus a floor on the instrument itself) fails the next uncapped lede. **Two more defects were found by making the instruments run**: `panel-render-audit.mjs` had NEVER run anywhere and died on `PROBE is not defined` (fixed, gated at the source), and the audit and the sweep disagreed about one element — the waiver policy now lives in `lib/design-sweep.mjs` and both read it. A press gap in the sheet (`.settings-fold > summary`, RED at HEAD) was closed too. **The logo: `vale-agent.exe` carried no resource at all** — the brand mark was wired into the desktop app and the installer, never into the service binary. `build.rs` now compiles `brand/icon.ico` into the image (all four frames byte-present, no build path embedded, two re-links byte-identical) with `FileDescription "Vale Agent"` and the npm version, so Task Manager shows the mark and the name. **TO SEE IT, A RELEASE IS NEEDED** — the panel fix and the icon both ride the next `publish-release.sh` + `vale update` on d1, and publishing moves the update channel, so it waits for your word. · the round-265 commit (this row cited `docs/agents/design-ledger.md` until landing 4a retired it — a `done` row a reader cannot check is not evidence) · the sweep's `measure` axis + `panel-audit-skip-check` · `agent/build.rs` |
| 25 | the loop's measurement (round 222: DNS + HTTP, from the Linux box) | waiting on you | **Where is the landing page supposed to live?** `agent.saisi.online` and `api.saisi.online` resolve; **`saisi.online` and `www.saisi.online` do NOT resolve at all** (NO DNS), so `https://saisi.online/` cannot connect. The repo cannot answer this: `index/wrangler.jsonc` says the custom domain is added in the Cloudflare dashboard (the file carries the placeholder `<download-host>`), and no file names the public URL — that is the result of the earlier instruction to take those strings out of the project. So: if the landing is meant to be at the apex, it is unreachable right now and only you can see why; if it lives on another host, nothing is wrong and this row can be deleted. |
**AND ROUND 119 NAMED THE SERVICE ITSELF — `Dnscache`, THE WINDOWS DNS CLIENT — SO THE FIX IS ONE SAFE COMMAND.** Round 96 could only name the GROUP because `tasklist /svc /fi` needs quotes this shell strips; `wmic service where ProcessId=2312 get Name,DisplayName,State` needs none and answers `DisplayName=DNS Client`, `Name=Dnscache`, `State=Running`. **RESTARTING THE DNS CACHE IS LOW-RISK** — it re-populates — so: `net stop dnscache` then `net start dnscache` (admin), or a reboot, and the device is back above the threshold. The loop has NOT done it, because stopping a service on the operator's device is their call; what it has done is make the call deterministic. | 27 | the loop's measurement (rounds 58-81: the device, read from the device) | waiting on you | **The device's commit is exhausted by ONE WINDOWS SERVICE HOST, and it is what blocks the last two measurements.** `FreeVirtualMemory` has swung 184 -> 211 -> 389 -> 48 -> 55 MB across five readings, and a Chromium launch needs far more than the low end: the browser helper's node died with `Fatal process out of memory: Zone Allocation failed`, and every later browser attempt either timed out or half-rendered a page. The consumer is `svchost.exe` **PID 2312**, holding **24.6 GB of a 32 GB commit limit**, created at boot (round 96 named its GROUP: `wmic process where ProcessId=2312 get CommandLine` answers `svchost.exe -k NetworkService -p`, so the leaking service is one of the NetworkService group — `Dnscache`, `WinHttpAutoProxySvc` and `nsi` are the usual suspects for this leak. **RESTARTING THAT ONE SERVICE FREES THE COMMIT WITHOUT A REBOOT**; `tasklist /svc /fi "PID eq 2312"` would name it exactly, but this shell strips the quotes the filter needs, which is why the group is what could be read). **THE AGENT IS NOT THE CONSUMER**: `summrise-agent.exe` holds 12.4 MB of commit and 10.5 MB working set, flat across 30 hours of uptime (round 60). A reboot clears it. **AND NOTHING IS WAITING ON IT, WHICH CORRECTS WHAT THIS ROW FIRST SAID.** It claimed the terminal fit (round 30) and the ack first-press (round 54) were each one browser load away. **BOTH WERE ALREADY ANSWERED FROM CI, WHICH HAS A BROWSER THIS DEVICE'S MEMORY CANNOT ALWAYS AFFORD**: the terminal's grid is FITTED — 29 columns at a 6.21px cell is 180px inside a 198px container, measured by the design job (round 34) — and the ack note is INFORMATIONAL rather than a failure, because the pass presses twice by design and the row is counted (round 56). So the device's commit blocks CONFIRMATIONS, not answers, and a reboot is a convenience rather than a prerequisite. |
| 28 | the loop's measurement (rounds 83-85: releasing 1.2.475) | waiting on you | **1.2.475 SHIPPED, AND ITS TWO ARTIFACTS DO NOT MATCH — the P0 dual-builder invariant is violated.** The release itself succeeded: the CDN serves `version.json` 1.2.475 and the versioned tgz (both 200), npm's `latest` carries `summrise-agent@1.2.475`, the tag points at a CI-green commit, and `release.yml` built the GitHub release and its asset. **BUT THE BYTES DIFFER**: the GitHub asset is **6,735,310 bytes, sha256 2fa54a0d…**, and the CDN's copy is **6,735,330 bytes, sha256 90a9b760…** — 20 bytes apart, which is the class of difference (tar/gzip metadata between two builders) that the audit exists to catch rather than assume benign. **AND THE AUDIT COULD NOT CATCH IT**: `--audit-only 1.2.475` fails with `cannot download the GitHub asset, direct OR through the API` because github.com is unreachable from the Linux box — so the step that would have reported a MISMATCH instead reported a NETWORK failure. The mismatch was found by routing the download through d1 (`system_file_download`), which CAN reach github, and comparing hashes by hand. **AND THE 20 BYTES ARE PACKAGING, NOT CONTENT — MEASURED IN ROUND 87.** Listing both archives member by member shows NINE IDENTICAL MEMBERS with identical sizes: `summrise-agent.exe` 17,649,664 both sides, `bin/summrise.js` 198,635, `src/main.js` 66,158, `preload.js` 4,182, `url-policy.js` 8,036, `package.json` 2,232, `README.md` 3,245, `icon.ico` 16,870, `icon.png` 3,010. So the two builders produced **the same files** and differ only in container metadata (gzip/tar framing). **AND THE AUDIT ALREADY HANDLES EXACTLY THIS — CORRECTING WHAT THIS ROW FIRST CLAIMED (round 88).** `scripts/lib/release-audit.sh` says so in its own comment: "Whole-tarball hashes differ. **That is EXPECTED (exe toolchain)** — but only if every source-derived file still matches. **Compare file by file.**" So a 20-byte container difference is the ROUTINE case, not a violation, and the audit compares the extracted trees INCLUDING MODES — because a 600-vs-644 difference once "survived twenty consecutive releases under the words 'packaging metadata'". **SO THE OPEN QUESTION IS NOT content-versus-container; it is whether 1.2.475's file-by-file comparison PASSES** — which needs the audit to run, and it cannot from the Linux box (github.com unreachable). **AND ROUND 89 RAN THAT COMPARISON BY HAND — 1.2.475 PASSES IT.** Every member matches on NAME, MODE and SIZE: `summrise-agent.exe` `-rwxr-xr-x` 17,649,664 both sides; `bin/summrise.js` `-rw-r--r--` 198,635; `src/main.js` 66,158; `preload.js` 4,182; `url-policy.js` 8,036; `package.json` 2,232; `README.md` 3,245; `icon.ico` 16,870; `icon.png` 3,010 — nine members, no difference in any of the three fields the audit compares. So the 20 bytes are PURE CONTAINER FRAMING, the release is verified to the audit's own standard, and **NOTHING IS WAITING ON YOU FOR 1.2.475**. The device's inability to launch a browser and this box's inability to reach github.com are both recorded above and neither blocks the release. |
**CORRECTED IN ROUND 115: IT IS NOT SUSTAINED — IT IS VOLATILE ACROSS A WIDE RANGE.** The three readings that prompted "sustained" (5, 25, 22 MB) were followed by **207 MB in round 111, then 63 and 66 MB in rounds 114-115** — so the level moves between ~5 MB and ~207 MB within hours, and the earlier "it recovers" and the later "it is sustained" were both true of the rounds that measured them. **WHAT IS ACTUALLY RELIABLE IS THE ASYMMETRY**: a LOW reading predicts failure almost exactly (at 63 MB the probe's node heap was capped at ~50 MB), while a HIGH one does not predict success (123 MB completed a full measurement in round 107; 207 MB crashed in round 111). **AND IT HAS STAYED THERE FOR THREE ROUNDS — 5, 25 and 22 MB (rounds 108-110) — WHICH IS THE NEW PART.** Earlier readings RECOVERED within a round or two (48 MB in round 79, 123 MB by round 107, which is enough for the probe to complete a full measurement), so this leak had a floor it bounced off. Three consecutive rounds under 30 MB says the floor is now SUSTAINED, which changes what the measurement waits for: not a good moment, but the service restart. **AND IT IS WORTH SAYING WHAT IS NOT THE PROBLEM**: the probe, the read-only mode that removed the attached-view condition, and the page guard that stops it measuring the wrong page are all in place and PROVEN — round 107 produced 45 rows, 0 unmeasurable, no errors. The device's memory is the only variable left. | 29 | the loop's measurement (round 93: the device, while trying to clean up) | **WAITING ON YOU — AND NOW URGENT** | **THE DEVICE CAN NO LONGER SPAWN A PROCESS.** A terminal call to remove the stray CLI that round 90's mis-install left in `D:\Summrise` came back `Internal error: spawn failed: 页面文件太小，无法完成操作。 (os error 1455)` — "the paging file is too small for this operation to complete". That is row 27's commit exhaustion arriving at the point where it stops being about measurements: `svchost.exe` **PID 2312** holds **24.6 GB of a 32 GB commit limit**, and there is no longer enough headroom to start a program. **A REBOOT IS NO LONGER A CONVENIENCE.** It is also worth watching whether the agent survives it — `summrise status` answered `RUNNING` at 1.2.475 in round 90, so the running process is fine; what is failing is anything NEW starting. The agent itself holds 12.4 MB of commit, flat across 30 hours (round 60), so it is not the cause and is unlikely to be the casualty. **AND IT CLEARED ON ITS OWN THE NEXT ROUND, WHICH IS THE PART WORTH KNOWING (round 94)**: `summrise status` answered normally again and reported `release: 1.2.475`, so the headroom RECOVERS as well as collapses — 184, 211, 389, 48, 55, 210 MB and then spawnable again, all within hours. **AND IT REACHED 5 MB IN ROUND 108 — THE LOWEST YET, AND WORSE THAN THE 48 MB ALREADY RECORDED HERE** (`FreeVirtualMemory=5344` KB, against 184 MB in round 58 and 48 MB in round 79). At that level almost nothing starts: the live-panel probe died with `Fatal javascript OOM in MemoryChunk allocation failed during deserialization` and printed NOTHING, where at 123 MB in round 107 it had run to completion. **A REBOOT IS STILL THE FIX** (the leak does not stop), but the device is not down and this row is not a call to drop everything; it is a warning that anything new may fail at any moment, which is what the intermittent `fetch failed` responses look like from the outside. Round 94 also finished the cleanup row 93 could not: the stray CLI round 90's mis-install left in `D:\Summrise` — a package tree and two shims — is REMOVED and VERIFIED, and it took two calls that reported success without running before one that did. |
| 29 | 你要确保你提供的安装包能够完美安装，现在安装完后还要我输入一堆命令不太合理 · deepseek harness 不是也有 windows 桌面客户端吗，它是怎么实现的，你能参考下吗 | **evidence recorded 2026-10-12 — two decisions for you; a stopgap EXE is built and byte-verified (not published)** | **A real install of 1.2.511 on `desktop-cph4kb2` (Hangzhou, China Unicom, public 101.68.143.52) took roughly ten hand-typed commands, and the product's own tunnel path could not be provisioned at all.** Every number below was measured on that device, or from a box on the same LAN and WAN, tonight. **What the release host costs from that network.** The bundled-tgz smoke download reached **82,660 B in about 50 minutes** (27 B/s); the install script's cloudflared arm hit its 120 s timeout (the same URL measured 4 KB/s from this side); its playwright arm hit the 600 s timeout and the script then DISCARDS the partial file. The SAME pinned bytes arrive in seconds from domestic mirrors: electron v33.4.11, 115,028,145 B in **5.5 s** from npmmirror (sha256 `f64c8a5a…` equals the pin at `index/components.json:12-15`) and cloudflared 2026.8.3, 54,841,128 B in **5 s** through a GitHub proxy (sha256 `83e726ed…` equals the pin at `index/components.json:8-11`). The bytes are not the problem; the route is. **`summrise setup` fetches three components with no bounded timeout and no skip line** — playwright (32 MB) at `agent/summrise-cli/src/setup.rs:326`, cloudflared (54.8 MB) at `:355`, electron (115 MB) at `:386` — about 200 MB in one queue. At the rates above that is a night, and the operator's screen shows one line (运行 summrise setup ...) for hours with nothing to tell a stall from work. **P0 — the cloudflared path cannot be provisioned at all.** `cloudflared tunnel login --token` does not exist in the pinned cloudflared: `cloudflared-linux tunnel login --help` on **2026.8.3** lists only `--loginURL / --callbackURL / --fedramp / --help`, and the call itself answers `Incorrect Usage: flag provided but not defined: -token`. BOTH callers use it — `agent/src/tunnel.rs:252` and `agent/summrise-cli/src/tunnel.rs:130-141` — so `tunnel install` **spends the one-time registration key** and dies at `cloudflare login failed`, and the panel's Save and connect arm (`crate::tunnel::provision_tunnel`, `agent/src/web/mod.rs:3562`) leaves a gateway binding with no tunnel. The console then answers **530** for that device, which is what "registered but unreachable" looks like from outside. The sequence that DOES work, done by hand, is four steps: create the tunnel through the API with `config_src=cloudflare`; write the remote ingress `http://127.0.0.1:18080` (the shape `ingress_service` produces, `agent/src/tunnel.rs:575-577`); CNAME `<device>.agent.saisi.online` to `<id>.cfargotunnel.com`; `cloudflared service install <run-token>`. One environmental fact rides with it: **QUIC never registers on this device while `--protocol http2` connects in one second** (four edge connections, lax10 / lax13 / lax08) — the same UDP fragility that drops Tailscale to DERP(hkg) there. **The ACL that cost an hour was mine, not the product's.** `components\cloudflared.exe`, hand-copied, carried `Administrators:(R,W)` and `SYSTEM:(R,W)` — no execute bit for anyone, which is why even an elevated `--version` returned error 5; the file `setup` stages itself (`components\playwright\node.exe`) carries the correct inherited `Users:(I)(RX)`. `copy_file` staging is sound and no fix is owed there. **DeepSeek Harness, read from its checkout** (`@deepseek-ai/dsh@0.2.0-rc.2`): no Electron, no installer, no service, no tunnel. `bin: dsh`; `dsh web` serves the browser UI on loopback; remote access is the device dialling OUT (`dsh-client-connection`, "Authenticated RPC transport"; `dsh-api-gateway`, "Remote Host dispatcher", dependency `ws`). Its entire footprint is one npm package — and its client, like the panel here, is a browser tab. **This is field evidence on a decision the repo already framed.** `docs/research/remote-mcp-access.md` §6 ranks **Option B** ("Summrise Gate as the relay the agent dials out to" — the Portainer Edge Agent / SniTun / dev-tunnels shape) as the architecture every comparable product uses, with Option F as the part that cannot be deferred. What tonight adds is the COST of the alternative: the operator-run tunnel is not free — it is a 55 MB third-party binary in the distribution path, a provisioning control plane (reg-key, API token, DNS route, credentials), and a transport that needs UDP 7844 unless told `--protocol http2`. **And the relay is already built**: `agent/src/relay.rs` is the outbound client and keeps the rule that a relayed request goes through the SAME `axum::Router` as the local one, `relay/worker/` is a Rust Worker with a claim Durable Object and two test suites, and `agent/src/mcp/server.rs:258-266` spawns the client whenever `relay_url` is set — `docs/design/0010-distribution-and-relay.md` records d1 actually retrying it. What is open is the operator story for `relay_url` / `relay_token`, which is what decides whether Option B is a configuration change or a project. **Two corrections to my own first reading, recorded so they are not repeated.** (i) "inbound versus outbound" was too coarse: cloudflared is ALREADY outbound-only (§3.1 of that same research document), so the tunnel's cost is the binary, the control plane and UDP 7844, not a listening port. (ii) "ship the components as npm packages" was already considered and rejected in `0010`, with its reason stated (a `npx electron` postinstall is a GitHub-releases download, which this network drops); it is not re-proposed here. What tonight adds is that the host's own route needs a bounded fetch, and that the same pinned bytes can be hand-delivered from domestic mirrors. **Stopgap built and byte-verified, NOT published, repository untouched.** `SummriseAgent-Setup-1.2.511-OFFLINE-FULL.exe`, 174,352,320 B, sha256 `1ab624c6…`: the pinned tgz plus all three components embedded, and each payload was extracted from the built exe and hashed — `587f2c01…`, `83e726ed…`, `f64c8a5a…`, `952cd252…` — all four equal their pins. The only new code is a `foreach` in `summrise-online-setup.ps1` that copies each bundled file into `$pkgDir` BEFORE the existing checks, so the manifest sha256 still decides what is kept and a bad copy is discarded exactly as a bad download would be. A 27 MB cloudflared-only variant also exists. It has NOT been run on Windows — only the bytes are verified; the reachable Windows host is the operator's own device. **Two decisions, both yours.** (a) The cloudflared path: implement the four steps above (small, and it deletes the P0) or make the relay the default reachability path and stop shipping cloudflared — the evidence above argues for the second. (b) The stopgap: whether the offline variant is published beside the online installer, and whether `setup`'s three component fetches get a bounded timeout plus an explicit "skipped" line, so a fresh install can never look hung. |
| 30 | 为什么现在经常出现 zss.saisi.online:8080 会话中断，oracle tailscale 节点访问异常，你排查下 · 5分钟前就出现了 · 我需要通过 tailscale exit node 访问外网 | fixed; two open decisions | **THE DOOR WAS ON THE OVERLAY, AND THE FALLBACK WAS A PUBLIC RELAY — both are gone.** Measured: the operator's box reaches this box at 1ms over the LAN, yet its ONLY door was bound to the tailnet IP (`100.85.163.159:8080`) while `zss.saisi.online` resolved through MagicDNS — so a control-plane reset (6 in 60 minutes: `connection reset by peer` to `controlplane.tailscale.com`, netmap stale 2m6s) took the NAME and the transport together. `dsh-edge-proxy` now also listens on `192.168.102.50:8080` (PM2 `DSH_PROXY_LISTEN`, persisted; no hosts entry needed because the proxy rewrites Host itself). The exit node STAYS — it is required — so what changed is the RELAY: a self-hosted DERP (`derper` 1.102.4 built from source on the VPS, certbot DNS-01, region `900/phx` in the ACL `derpMap`, OmitDefaultRegions left false) now runs on the exit node itself. The slow episodes were never latency — `via DERP(sfo) in 201ms` against `via 132.226.90.175:41641 in 199ms` — they were a SHARED PUBLIC RELAY carrying the operator's entire internet. Verified: `desktop-cph4kb2` measures `phx: 203.2ms` out of 28 regions (fastest, ahead of sfo 221.4 and hkg 375.4), the VPS reports `.Self.Relay = phx`, and the direct path is 197-198ms. derper's own STUN does not answer on this box (not even on 127.0.0.1, on any port), so a 40-line RFC 5389 responder serves UDP 3478 — which needed ONE OCI ingress rule from the operator (added) because every other UDP port is filtered. Self-inflicted and recorded: a `127.0.0.1 derp.saisi.online` line in the VPS's `/etc/hosts`, added to dodge a NAT hairpin, LEAKED tailnet-wide through systemd-resolved and made the Windows box measure a fake 600µs; removed, and the hairpin turned out to work once 3478 was open. Runbook with all four traps: `scripts/DERP.md`. OPEN: `OmitDefaultRegions: true` was considered and deliberately NOT taken (it would force every relayed packet in the tailnet through Phoenix — a single point of failure, and a long way round for two China-side machines); the recommended next step is moving the tailnet's DNS onto a caching resolver on the VPS, since no global nameserver is configured today and lookups therefore go to the China-side router at `192.168.102.1`; and the Tailscale API key used for the ACL edit appeared in a chat transcript and should be rotated. **ROTATED 2026-10-13: the leaked key was revoked by the agent itself (it had been created for that session 20 minutes earlier, so nothing else used it) and verified dead — a read against `/acl` with it now answers `401`; the tailnet's device-registration key was not touched. The API cannot mint a replacement full-access token (`exactly one capability scope must be populated`), so the next one comes from the admin console.** **MEASURED 2026-10-13, WHY IT STILL STALLS: the relay removes the public hop, NOT the middlebox — derper's own log shows every node behind that Chinese NAT being cut on a timer (`client [6OlNm]/[CXUG9]/[lc2jO]: readFrameHeader ... connection timed out`), 125 route teardowns in 30 minutes for one peer against zero for the others, while the relay's TCP stayed healthy (`retrans:0/14` over 74 MB, RTT 200-250ms, `app_limited`). Each kill is a few seconds of stall, which is the "卡一下又好了" reported. Ruled out: bandwidth, loss, the VPS, derper. A nearer relay is NOT available — the Oracle tenancy allows no second free instance — so the only remaining lever is a transport that does not look like a long-lived flow to the middlebox.** **TESTED AND REFUTED 2026-10-13: the path does NOT reap long-lived flows — an idle SSH on tcp/22 (all keepalives off) and a keep-alive TLS session on tcp/443 to the VPS PUBLIC address both stayed ESTABLISHED for 41-43 minutes with zero drops, so the WebSocket/TLS transport idea (whose premise was that keeping the flow busy or moving it to 443 would help) is NOT worth building. The derper timeouts line up with the client own network changing instead, and none followed after the network settled.** |

## ROW 30, MEASURED — THE RELAY IS UP, AND ONE PEER IS TAKING IT (2026-10-16, round 92)

The question was *"为什么现在经常出现 zss.saisi.online:8080 会话中断，oracle tailscale 节点访问异常"*. Two commands, from
the Linux box that serves it:

| measurement | reading |
|---|---|
| `curl http://zss.saisi.online:8080/` | **200**, connect 1.73 s, total 1.81 s — **the service answers** |
| `tailscale status` | 6 peers; `desktop-9vh92bj` and `desktop-cph4kb2` **direct** (`192.168.102.x:41641`), `desktop-14rjcr8` **relay "phx"**, `oracle-phx` idle, offers exit node |
| `tailscale netcheck` | `Nearest DERP: Phoenix (self-hosted, saisi)`, `DERP latency: phx: 203.9ms`, `UDP: true`, `MappingVariesByDestIP: false` |

**SO THE RELAY IS NOT DOWN AND IT IS NOT THE PUBLIC FALLBACK** — it is OURS, and the peer that reports trouble is the one
peer using it. **That is the runbook's own distinction** (`scripts/DERP.md`): the latency of a relayed hop is nearly the
same as a direct one (201 vs 199 ms when it was measured), and what differs is that **a relayed hop carries the whole
conversation** — which is a session drop rather than a slow page.

**THE NEXT MEASUREMENT IS NOT MINE TO TAKE, AND THAT IS THE POINT**: whether `desktop-14rjcr8` can go direct is a
question about ITS network — `tailscale ping desktop-9vh92bj` from that machine says `via DERP(phx)` or `direct`, and
that single word decides whether this is a relay-capacity problem or a NAT problem on one Windows box. **Everything
above was taken from the server side; the symptom lives on the client side.**

### THE SECOND HALF, AND IT EXPLAINS BOTH HALVES (round 93, same box)

`tailscale ping` to the three peers, from the Linux box that serves the console:

| peer | path | RTT |
|---|---|---|
| `desktop-cph4kb2` (same LAN) | **direct** `192.168.102.99:41641` | **7 ms** |
| `oracle-phx` (the relay's OWN host) | **DERP(phx)**, four of four, then `direct connection not established` | **201–217 ms** |
| `desktop-14rjcr8` | **DERP(phx)**, four of four, `direct connection not established` | **384 ms** |

**`oracle-phx` IS THE HOST OF THE `phx` RELAY** (`netcheck`: *"Nearest DERP: Phoenix (self-hosted, saisi)"*, and
`status` shows `oracle-phx` offering the exit node). So this box's traffic to that node goes **me -> DERP(phx) ->
oracle-phx**: the relay is relaying **to itself**, because no direct path exists. `desktop-14rjcr8`'s 384 ms is the
same hop counted twice.

**SO "oracle 节点访问异常" IS NOT AN ANOMALY IN THE NODE — IT IS THE ABSENCE OF A DIRECT PATH TO IT**, and everything
that follows from that is the runbook's own warning: a relayed hop carries the whole conversation, and a peer 2 hops
away from the relay is where a session dies. **The readings are consistent with one cause rather than two.**

**WHAT IS LEFT TO CHECK, AND IT IS ONE PORT ON ONE BOX**: this side is not the problem (`UDP: true`,
`MappingVariesByDestIP: false`). `oracle-phx` is a cloud VM whose UDP/41641 (WireGuard) is not answering — a firewall
rule or a security-list entry — and the check is `ss -lunp | grep 41641` on that box plus a TCP/UDP reachability test
from outside. **Until then the tailnet is silently paying a round trip through Phoenix for a conversation with
Phoenix.**

**TWO MORE FACTS, SO THE NEXT ROUND DOES NOT RE-DERIVE THEM (round 94)**:

  * **STUN IS FINE, WHICH NARROWS IT TO ONE THING.** `netcheck` completed a STUN round trip and got this box's mapped
    address (`101.68.143.52:56646`), and the relay answers pings — so the DERP is reachable, its STUN port is
    reachable, and **what is missing is specifically the direct WireGuard path to `oracle-phx` (UDP 41641)**. That is
    the Oracle side's inbound rule, not a tailnet configuration and not this box.
  * **THIS CHECK CANNOT BE AUTOMATED FROM HERE, AND THAT WAS TRIED RATHER THAN ASSUMED**: `ssh ubuntu@132.226.90.175`
    and `ssh opc@132.226.90.175` both answer `Permission denied (publickey)` — no key for that host on this box. **So
    it is your command, and the runbook's own list is the recipe**: on `oracle-phx`, `ss -lunp | grep -E "41641|3478"`
    (is WireGuard listening at all) and then the Oracle console's security list / firewall for **UDP 41641** from the
    clients' addresses.

**AND THE RUNBOOK ALREADY PREDICTED ALL OF THIS**, which is why it is worth quoting rather than rediscovering: *"a
node picks its home DERP by measured latency, and a region whose latency cannot be measured is never selected. So a
DERP that is unreachable for STUN might as well not exist"* — the STUN half is healthy and the **direct** half is not,
which is precisely the state where every peer homes on the relay and none of them connects directly.

## ROW 29, PINPOINTED — THE UPDATE PATH ASKS THE HUMAN TO COMPUTE WHAT THE CLI ALREADY KNOWS (round 95)

**"安装完后还要我输入一堆命令不太合理"** — and the commands are not merely tedious, they are the ones the README says are
WRONG on a device that is already set up. `agent/summrise-agent-npm/README.md` measures the failure: plain
`npm i -g <url>` lands in npm's DEFAULT global prefix, which is not where `summrise` lives when the agent runs as
SYSTEM; npm printed success, `summrise update` then ran the OLD CLI from the other prefix and staged the OLD exe, and
**"the device quietly stayed on its previous release with no error anywhere."** The documented workaround is to type
`--prefix (Split-Path (Get-Command summrise).Source)` — asking the human to compute the running binary's own directory.

**AND `agent/summrise-cli/src/dispatch.rs` HANDS THAT SAME BARE COMMAND BACK AS THE REMEDY.** `update_decision` refuses
when the CLI is behind the channel (correct — it cannot deliver a version it does not carry) and then prints:

```
Install the new CLI first, then update again:
  npm i -g summrise-agent
  summrise update
```

**on a device where that command is the documented silent-failure path.** The function's own signature is the proof that
the fix is available: it already takes `layout: &Layout`, and the running CLI knows where it lives — **that is precisely
what `Split-Path (Get-Command summrise).Source` asks the machine for, computed by the process that IS that binary.**

**THE FIX, IN ONE SENTENCE**: the remedy must name the prefix the CLI is running from (or perform the install into it),
so that a device whose CLI is behind can be brought forward by the ONE command it is told to run — and the test that
already asserts this outcome's text is where the corrected sentence belongs.

## ROW 29's SECOND ASK, ANSWERED BY READING THE REFERENCE (round 100)

*"deepseek harness 不是也有 windows 桌面客户端吗，它是怎么实现的，你能参考下吗"* — the package is on this box, so
this is a reading rather than a guess:

```
@deepseek-ai/dsh 0.2.0-rc.2
  bin: { "dsh": "lib/bin.js" }
  scripts: (none)          optionalDependencies: (none)          one bundled lib/
```

**THE SHAPE IS THE ANSWER, AND IT IS ALMOST EMBARRASSINGLY PLAIN**: one npm package, ONE bin entry, **no install
scripts, no post-install steps, no per-architecture optional dependencies, no prefix arithmetic anywhere in the
documentation** — because the process that runs is the process that knows where it is. Nothing asks a human to compute
a path, so nothing can be computed wrongly.

**AND THAT IS WHY ROUND 98's FIX WAS THE RIGHT DIRECTION RATHER THAN A PATCH**: `summrise update` now prints the prefix
IT is running from instead of asking for `Split-Path (Get-Command summrise).Source`. **The reference does not have a
better trick; it never had the problem**, because its install never leaves the default prefix. Ours does (the agent runs
as SYSTEM, so `summrise` lives under the device's own tree — `D:\Summrise\components\npm-global` on the measured
device), and that is the ONE difference worth designing around.

**SO THE REMAINING WORK ON ROW 29 IS NOT ANOTHER MESSAGE — IT IS THE INSTALL LAYOUT**: a fresh install that lands where
the CLI will look (the current README already points at the tgz for exactly this reason), and an update that a device
can complete with the single command it is shown. **Anything still asking a person to type a path is a layout problem
wearing a documentation problem's clothes.**

## THE "MAKE THE HUMAN COMPUTE IT" CLASS, SWEPT (round 102) — ROW 29 WAS THE WHOLE OF IT

The question after rounds 98-101 was whether the same defect exists elsewhere in the CLI: **a message that asks a
person to supply something the process already knows** (a prefix, a path, a directory). Swept every string literal in
`agent/summrise-*/src/*.rs` for instructions naming a path, `npm`, `--prefix` or `Split-Path`:

| finding | verdict |
|---|---|
| `dispatch.rs:562` — `"THIS DEVICE IS BEHIND ... run 'summrise update'"` | **clean** — self-contained, and that command now prints its own prefix (round 98) |
| `psgen.rs:226,279,644` — `Split-Path $log`, `Join-Path (Split-Path $node -Parent) …` | **clean** — generated PowerShell, executed by the machine, not instructions to a person. **The same words in a script and in a message are different things, and the sweep has to tell them apart.** |
| `components.rs:495` — `"reinstall the package (npm i -g summrise-agent) to stage"` | **clean, and better than clean**: it is a NEGATIVE ASSERTION — `!STAGE_ADVICE.contains("…")` — a test that FORBIDS the CLI from advising a reinstall for a component the package does not carry (the oracle cited is `cli.test.mjs:3142`) |

**SO THE CLASS IS CLOSED**: the defect existed exactly where row 29 named it, and the CLI's remaining instructions are
either self-contained or forbidden by a test. **Nothing to fix, which is the answer a sweep should be allowed to
return** — inventing a change here would be manufacturing work to look busy.

## THE DESKTOP SHELL, MEASURED — ONE SECTION IS ALREADY A MODULE (round 103)

A read-only survey of the surfaces the objective names, by size, because that is where design debt concentrates:

| surface | source files | largest |
|---|---|---|
| `agent/resources/panel-react` | **231** | `SettingsPage.tsx` 41.8 KB, `useSessions.ts` 36.9 KB, `useDeviceRead.ts` 36.4 KB |
| `agent/summrise-desktop-electron` | 8 | **`main.ts` 87.0 KB** |
| `agent/summrise-command-core` | 6 | `sweep_judge.rs` 94.8 KB (a dev tool, not user-facing) |

**`main.ts` IS BIG BECAUSE IT HAS EIGHT JOBS, NOT BECAUSE IT REPEATS ITSELF** — its own header names three (probe the
agent's port, load the SPA or offer to start the agent, own window/tray/menu/CDP/sessions) plus a measured rule: *"No
spawn() of summrise-agent.exe from JS — that was the d1 Chrome-OOM root cause"*. I checked the two embedded-browser
sections for duplication and found the opposite: **the DSH view is two functions (`bootDshPort`, `dshTargetFor`) that
reuse the embedded view rather than reimplementing it.**

**AND THE EMBEDDED VIEW IS ALREADY A MODULE IN EVERYTHING BUT ITS FILE BOUNDARY.** Measured over its 208 lines
(447-655): **0 references to `tray`/`Tray`/`Menu`/`mainWindow`/`ipcMain`**, an eight-function API
(`embeddedViewEnsure`, `embeddedViewPlace`, `embeddedNavigate`, `embeddedGo`, `embeddedReload`, `embeddedState`,
`embeddedWireNavEvents`, `embeddedRecover`) and its whole state in four module-level bindings (`embeddedView`,
`embeddedVisible`, `embeddedUrl`, `embeddedBounds`). **A section with a coherent API and no cross-dependencies is a
file waiting for someone to say so** — which is the whole of what "确保各个模块完美" asks for here.

**ONE THING TO SETTLE FIRST, AND IT IS WHY THE EXTRACTION IS NOT IN THIS ROUND**: `src/` carries BOTH `.ts` and `.js`
for the same names (`main.ts` beside `main.js`, `preload.ts` beside `preload.js`), and the package declares
`"test": "node --test"` with a comment about the shipped artifact. **Whether the `.js` is generated or hand-kept decides
whether the extraction writes one file or two** — and that is a measurement, not a preference, so the next round takes
it before touching anything.

## A GUARD THAT COULD NOT FAIL, WALKED INTO ON PURPOSE (round 105)

Chaining a verification to a merge is a habit this repository encourages:

```bash
cargo test … | grep "test result: ok" && git checkout main && git merge --no-ff <branch> && git push
```

**IT DOES NOT GUARD ANYTHING.** A pipeline's exit status is the LAST command's — `head -3` here, `grep` in general —
so the `&&` fires whether the tests passed, failed, or never ran. This round it fired when `cargo` had not even found a
`Cargo.toml` (`error: could not find Cargo.toml in /home/zss/summrise-main`), and the merge and push went out with the
suite unverified. **The run afterwards was green by luck, not by discipline**: `cargo test -p summrise-cli` ->
**145 passed, 0 failed**.

**THE REPAIR IS ONE WORD, AND IT IS THE SAME REPAIR EVERY TIME**: `set -o pipefail` (or read the exit code instead of
the output, which is what `AGENTS.md` already says and what this round proves is not enough on its own — *"read the exit
code, not the output"* presupposes you are reading the exit code of the thing you think you are). **A guard whose
failure mode is silence is the exact class this repository writes gates against**, and here it was a shell idiom rather
than a missing gate: nothing to add to `ci.yml`, everything to remember at the prompt.

## THE THIRD EXTRACTION IS THE WRONG MOVE, AND THE MEASUREMENT SAYS WHY (round 110)

Having extracted `embedded-view` and `browser-sessions`, the obvious next step was to extract the remaining
state-bearing embedding — the DSH view — the same way. **Measuring it first says no**, and this is the measurement:

| `src/embedded-view.ts` (already extracted) | the DSH, still inside `main.ts` |
|---|---|
| `embeddedViewEnsure` | `dshViewEnsure` |
| `embeddedViewPlace` | `dshPlace` |
| `embeddedNavigate` | `dshNavigate` |
| `embeddedReload` | `dshReload` |
| `embeddedRecover` | `dshRecover` |
| `embeddedState` | `dshState` |
| (`embeddedGo`, `embeddedWireNavEvents`, `embeddedZoom`) | (`dshOpen`) |

**NINE FUNCTIONS, THE SAME SHAPE, THEIR OWN THREE STATE BINDINGS EACH** (`embeddedView`/`embeddedVisible`/
`embeddedBounds` and `dshView`/`dshVisible`/`dshBounds`), and the primitive counts match exactly: **11 occurrences of
`setVisible`/`setBounds`/`addChildView`/`removeChildView` in `main.ts` and 11 in `embedded-view.ts`**. The DSH's half of
`main.ts` was a near-copy of the module extracted two rounds ago — **and it is `src/dsh-view.ts` now** (round 112), so
this paragraph describes the state that MOTIVATED the extraction rather than the state of the tree.

**SO A THIRD EXTRACTION WOULD BE THE WRONG ACT** — it would move the copy into a third file and make the duplication
look like structure. **The right move is the opposite: ONE parametrised embedded-view slot, instantiated twice.** The
question "is this a module?" was the right one for the first two clusters; for this one the question is "are these two
the same module?", and the answer is measured rather than felt.

**AND THE UNIFICATION IS DELIBERATELY NOT IN THIS ROUND, BECAUSE IT IS THE FIRST CHANGE IN THIS SERIES THAT TOUCHES
BEHAVIOUR.** The two extractions were pure moves (bodies byte-identical modulo seam substitutions) and could be proven
by a diff; **merging two implementations means choosing one behaviour where they differ** — the DSH's policy is its own
(`dshPopupTarget`, `dshTarget`, `dshHome`, `harnessDoors`, `addDshPort`) and this round has not read enough of it to
say which differences are load-bearing. The 52 tests are the safety net; they are not a substitute for knowing what
the two modules disagree about, and finding that out is the next round's first measurement, not a guess to start from.

## THE TWO EMBEDDED SLOTS: UNIFY THE CORE, AND WRITE THE DSH'S TESTS FIRST (round 111)

A function-by-function comparison of `src/embedded-view.ts` against the DSH cluster in `main.ts` (nine paired
functions, mechanically diffed with identifiers normalised and comments stripped) found **four load-bearing differences
and five historical ones**, and the load-bearing ones are all parameterisable:

| what differs | why it cannot be merged away |
|---|---|
| **the door** — `loadTarget` (admits any http/https) vs `dshTargetFor` (its own loopback origin, or nothing) | forced by the crate split: shell-policy takes the *answer* of url-policy's predicate because the two crates must not depend on each other. **Merging them either exposes the internet to the DSH's `window.open` or confines the browser view to loopback.** |
| **the recovery target** — last decided URL (`about:blank` fallback) vs always `dshHome(dshBase())` | measured on each side: "recovering a crashed view must not make a request to a search engine to do it" against "a crash recovery that lands somewhere other than the harness is a view the operator did not ask for" |
| **the lifecycle** — created eagerly at startup, hidden, NEVER navigated (the attached AI always has an `about:blank` CDP target; it used to load `bing.com` on every launch) vs created lazily on the pane's `open()` and navigated home | these are CALLER behaviours, not parameters of `ensure()` |
| **the channel surface** — nav push + back/forward + zoom vs neither | pinned in FOUR places (the `ipc-door` channel-set equality, the policy crate's renderer-coercion test, the panel's bridge fixture, `embeddedBridge.ts`'s manifests) |

**AND ONE DIFFERENCE IS A SHARED DEFECT RATHER THAN A DIFFERENCE**: both `recover()`s contain a re-show block that
cannot run (`ensure()` clears the visible flag first). The browser's is pinned as-is by a test; **the DSH's is
compensated in its pane** (`reportBounds()` after `recover()`). **So unifying silently decides it for both**, and the
test must be flipped in the same change rather than discovered afterwards.

**THE RECOMMENDATION, WHICH IS THE PLAN**: one factory — `embeddedSlot({ win, door, popupTarget, recoverUrl,
goneChannel, navChannel? })` — instantiated twice, with `bootDshPort`, `dshTargetFor`, `loadHarnessDoors` and `dshOpen`
staying in `main.ts` and `go`/`zoom`/`wireNav` staying browser-only (adding them to the DSH would be uncalled surface
and would fail the preload/channel equality test).

**AND THE ORDER IS TESTS FIRST, BECAUSE THE DSH CLUSTER HAS NONE.** The only thing about it that is checked today is its
channel NAME set. Before any unification: a DSH-slot suite pinning **today's** behaviour (lazy create-then-home, the
popup drop, the `gone` payload, recover-to-home, recover leaves it hidden, no `canBack`/`canFwd`), plus the test nobody
has ever needed because there has only ever been one instance — **two-instance isolation**: `place`/`recover`/`gone` on
one slot must not touch the other's view, flags, bounds or channel.

**ONE MORE THING THE COMPARISON TURNED UP, WORTH KEEPING**: the browser slot's security posture
(`contextIsolation`, `nodeIntegration: false`, `sandbox`, no `preload`) is asserted by its suite, and **the DSH's
identical posture is asserted by nothing** — no test can import `main.ts`. Unification is the only move that gives the
DSH that assertion, and it is also the move that risks weakening both if a shared `webPreferences` is edited casually,
since adding a `preload` would then affect two views at once.

## A GREEN MAIN READ AS RED, BECAUSE THE "LATEST" RUN WAS YESTERDAY'S (round 115)

**WHAT I DID**: after three landings without a CI check, I asked the API for `runs?per_page=1`, took the first entry as
the tip's run, found `failure`, and reported a red `main` — then spent a round fetching that run's log and delegating a
fix for what it named.

**WHAT WAS TRUE**: the log's own timestamps were `2026-10-15T16:44`, a day old. That failure was fixed by `2266a548`
("fix: the tool-name list is NOT exported — and the exports gate is the one I keep not running"), which
`git merge-base --is-ancestor 2266a548 HEAD` confirms is an ancestor of `main`. **`main` was green the whole time**, as
re-measured on the current tree: `exports_check` -> `319 exports across both UIs, every one used outside the file that
declares it`; the contract freshness test -> a committed file that equals a fresh render, byte for byte; the panel ->
`116 files / 935 tests passed`.

**THE TRAP, NAMED SO IT IS NOT RE-ENTERED**: *the run list is not a guarantee about which COMMIT a run belongs to.* A
re-run, a scheduled run, or a run of an older push can be the newest entry — so **a run must be selected by `head_sha`,
and a log must be read with its own timestamps checked before it is believed.** This is the same shape as the two
mistakes this session already recorded (`cmd | grep | head` returning `head`'s status, and reading clippy's COUNT
instead of its names): **a check read at one remove from the thing it describes.** Here the removal was one API
listing; there it was a pipe.

**AND THE DELEGATION IS WHAT CAUGHT IT, WHICH IS THE PART WORTH KEEPING.** The brief said "the main branch is red right
now and this is the failure", and the first thing that came back was *"the premise is stale — here is the commit that
fixed it, and here is the ancestor check"*. **A subagent that verifies the premise it is handed is worth more than one
that executes it quickly**, and a brief that quotes its evidence (as this one did, verbatim from the log) is what made
the check possible at all.

**ONE MORE THING THAT ROUND TURNED UP, WORTH KNOWING BEFORE TRUSTING ANY "THE GATE GOES RED" EVIDENCE**:
`agent/build.rs` fails the build when `resources/panel-react/src` is NEWER than the built panel bundle — so a mutation
that touches a file's mtime can exit 101 **without the test you aimed at ever running**. The tool gate hiding the test
gate, by design and by mtime.

## THE PANEL, SURVEYED (round 116) — 231 FILES, THOUSANDS OF TESTS, AND THREE BIG FILES

The desktop-shell line is surveyed (rounds 103-115); this is the other large surface the objective names, measured the
same way:

| area | files | size | largest |
|---|---|---|---|
| `src/components` | **52** | 1076 KB | **`SettingsPage.tsx` 41.8 KB** |
| `src/lib` | 68 | 624 KB | (mostly `*.test.ts` — contrast and pairing suites) |
| `src/hooks` | 21 | 488 KB | **`useSessions.ts` 36.9 KB**, **`useDeviceRead.ts` 36.4 KB** |

**AND THE HEADLINE IS WHAT IS NOT THERE: THIS IS NOT AN UNTESTED MONOLITH.** The package runs **935 tests across 116
files** (`vitest run`), the largest files have neighbours in `__tests__/`, and the two biggest `lib` files ARE test
files. **A survey whose finding is "no emergency" is a legitimate finding**, and it is worth recording precisely because
the next reader's instinct — as mine was — is to assume a 231-file UI is where the debt is.

**THE NEXT CANDIDATE, WITH ITS REASON**: `src/hooks/useDeviceRead.ts` (36.4 KB) — a HOOK is stateful logic, which is
exactly the kind that decomposes into pure functions with the hook left as the wiring, **and it already has
`__tests__/useDeviceRead.test.tsx`**, so a change there is verifiable by running the package's own suite. That is the
same pairing the desktop shell work relied on: a module with tests around it can be moved safely; one without cannot be
moved at all — which is why the DSH slot had to become a module before it could be tested rather than the other way
round (round 112).

**NOT STARTED, AND NOT BECAUSE IT LOOKS HARD**: the same rule that governed `main.ts` applies here — measure the seam
before moving anything, and prefer unifying two things that are the same over extracting a third thing that is merely
large.

## THREE MEASUREMENTS THAT ONLY EVER LIVED IN A REPORT (rounds 119-122, written down here)

**1. THE MILLISECOND-STAMPED ID MAKES THE WINDOW CAP COUNT THE WRONG THING.** `browserId(Date.now())`
(`summrise-shell-policy/src/sessions.rs`) is a bare millisecond stamp — no counter, no suffix — so two `browserOpen`
calls inside one millisecond produce the SAME id, the second overwrites the first in the table, and the first window
becomes unreachable. Measured with `MAX_BROWSER_WINDOWS` = 8 and 11 distinct URLs:

| clock | table rows | live windows | alive after `browserCloseAll()` |
|---|---|---|---|
| frozen (one millisecond) | 1 | **11** | **10** |
| stepping | 8 | 8 | 0 |

**So the cap is enforced over TABLE ROWS while the resource is WINDOWS**: a same-millisecond burst builds past the cap
and the `before-quit` sweep leaks what it built. **And it corrects an earlier claim of mine** — the orphan is NOT caused
by `browserCloseAll` being "a plain `for…of` that deletes during iteration": deleting the current key while iterating a
`Map` is well defined and the sweep does reach every entry (8/8 under the step clock). **The orphan survives because it
is absent from the table.** Pinned by a test; not fixed, because a test fix must not carry a behaviour change.

**2. "FLAKE" WAS THE WRONG WORD, AND THE MESSAGE SAID SO.** The collision case asserted two ids were EQUAL while
running on the real clock — it required the wall clock to stand still between two calls. It does on a development box
and does not on a CI runner (`actual 'browser-…588'`, `expected 'browser-…589'` — one millisecond apart, printed by CI
in the message that was then called unattributable). **Reproduced: 2 failures in 20 full runs, 40/40 when the case runs
cold.** The repair makes the collision a fact (`clock: "frozen"`, an unknown mode throws) rather than a hope, and the
proof for a timing bug is ten consecutive runs, not one green one.

**3. TWO COUNTS IN THE ROUND-111 COMPARISON WERE WRONG, AND THE CORRECTIONS MATTER.** The two slots have **EIGHT**
twinned operations, not nine (the "nine" was the browser's nine OPERATIONS, a phrase inherited from its own suite), and
the browser slot owns **FOUR** per-instance bindings, not three — `embeddedUrl` is the last decided URL, which is
exactly the state behind the "recovery target" parameter. **So the unified slot owns four bindings per instance and
only the recovery target FUNCTION differs.** A count that is a descriptor in one file becomes a specification in
another, which is how both of these travelled.

## THE DESKTOP SHELL LINE, CLOSED (rounds 103-145) — FOUR THINGS WORTH KEEPING

**WHAT HAPPENED**: `main.ts` was 1368 lines with eight jobs and four state-bearing clusters inside it. Three were
extracted verbatim (`embedded-view` 20 tests, `browser-sessions` 18, `dsh-view` 18) and then the last two — which a
function-by-function comparison showed to be near-copies — were **unified into ONE factory instantiated twice**
(`embedded-slot`), landed as `b414c9ca` with CI green at 6.6 minutes. The package went from 14 tests to **75**.

**1. MEASURE THE SEAM WITH THE COMPILER, NOT WITH GREP.** Both of the first two extractions got this wrong once by
grepping for names. Cutting the cluster out and compiling it alone (`tsc --noEmit`) makes `Cannot find name` the
COMPLETE free-identifier set — and it catches what a grep cannot: `WebContentsView` and `Electron` appear only in TYPE
positions (`let dshView: WebContentsView | null`), while `fetch`, `AbortController`, `setTimeout` and `process.env` are
globals rather than dependencies. **A grep answers "where is this word"; the compiler answers "what does this code
need".**

**2. "IS THIS A MODULE?" AND "ARE THESE TWO THE SAME MODULE?" ARE DIFFERENT QUESTIONS.** The first two clusters were
modules; the DSH cluster measured as a near-copy of the one already extracted (nine paired functions, three state
bindings each, ELEVEN occurrences of the same four view primitives on each side). **Extracting it as a third module
would have turned duplication into structure.** The right act was the opposite one — and the same test applies to the
panel's 231 files: prefer unifying two things that are the same over extracting a third thing that is merely large.

**3. A GATE THAT READS THE INDEX CANNOT BE DRIVEN BY A WORKER FORBIDDEN TO TOUCH IT.** `js_boundary_inventory` and
`npm_package` discover their subjects through `git ls-files`, so the new files had to be `git add -N`-ed before either
could pass — and a subagent instructed "do not commit" cannot do that. **It failed on rules matching no tracked file,
which is a property of the instruction rather than of the code**, and the landing had to clear it.

**4. A BEHAVIOUR MERGE MUST NAME ITS CHOICES.** The three extractions were pure moves, provable by diff; the
unification was the first change in the series that touched behaviour, and it carried **four named changes** (the dead
re-show branch fixed for both slots with both pinned assertions flipped in the same commit; `render-process-gone`
hiding before reading the window; the DSH slot gaining the deny-by-default permission handlers; `state().visible` going
through the crate on both sides). **A behaviour change a commit message does not name is a behaviour change nobody can
find** — and the same round found two MORE differences that had never been in the plan (`stateUrl` as a sixth
parameter, and `embeddedUrl` as a fourth per-instance binding), which is what happens when a comparison is measured
rather than remembered.

## WHERE THE WORK STANDS, AFTER MEASURING RATHER THAN GUESSING (rounds 147-150)

**THE "FIND THE BIGGEST FILE" HEURISTIC IS EXHAUSTED IN THIS REPOSITORY, AND THAT IS A FINDING.** Four surfaces were
measured the same way — not by size but by WHERE THE LINES GO — and only the first was debt:

| measured | verdict |
|---|---|
| `panel-react` — 231 files, 935 tests | maintained: the biggest files are test files, the biggest hook is **220 lines of TYPES** inside 540, the biggest component has **five helpers already extracted above it** and a test of its own |
| the repository's largest single file — `sweep_judge.rs`, 2145 lines | large but ORDERED: **40 top-level items** (54 lines each, so no god-function), 17% comments, 13 in-file tests — and it is a DEV TOOL, not user-facing |
| `index/` — 13 + 15 source files, four test files | small and tested; the biggest numbers in it are `build/` and `dist/` ARTIFACTS |
| `main.ts` — 1368 lines, four state-bearing clusters | **the one real debt**, and it is done: three modules extracted, the last two unified into one factory (rounds 103-145) |

**SO "SIZE" SAID NOTHING UNTIL IT WAS SPLIT INTO TYPES / JSX / LOGIC / ALREADY-EXTRACTED HELPERS.** 36.4 KB of hook was
220 lines of type declarations; 41.8 KB of component already had five free functions above the component body. **A file
list sorted by bytes is an index of where to LOOK, never an answer about where the work is.**

**WHAT ACTUALLY REMAINS, IN THREE ITEMS**:

  * **WAITING ON THE OPERATOR** — the front-door cutover observation
    (`curl -si https://<console>/api/admin/public | grep -i x-summrise-front-door` -> `rust`). After it: delete
    `plugins/auth.ts`, `plugins/admin.ts`, `plugins/models-probe.ts` by the rehearsed seven-step recipe, and LOGIC goes
    48 -> 45.
  * **DECIDED AND FIXED (round 157, `aeb84b54`) — AND THIS PARAGRAPH IS KEPT AS THE DECISION'S RECORD, NOT AS AN OPEN ITEM.** It was recorded as a product decision because the two available fixes are not equivalent; the call was made and it was the ID rather than the counting: *an id that collides is not an id* — it fails the one job an identifier has — whereas counting live windows would have kept the broken identifier and made the sweeper chase windows it could no longer name. `browser_id` is now `browser-<ms>-<n>` (a process-local `AtomicU64`), three pinned tests were re-pointed to REQUIRE the fix rather than the defect, and **both defects closed together with no change to the sweep**, because rows ARE windows now. **A second-order defect closed with them, which no description of this bug had mentioned**: the `closed` handler closed over the shared id, so under a collision the ORPHAN's close event deleted the LIVE window's row. — `browserId(Date.now())` is a bare millisecond stamp, so two
    `browserOpen` calls in one millisecond collide, the second overwrites the first in the table, and **the cap counts
    TABLE ROWS while the resource is WINDOWS**: measured with a cap of 8 and 11 URLs, a frozen clock yields **11 live
    windows and 10 still alive after `browserCloseAll()`**. It is pinned by a test whose name says it is pre-existing.
    **Two ways to fix it, and they are not equivalent**: give `browserId` a counter or suffix so ids never collide (the
    table then tracks windows and the cap does what it says), or make the cap count live windows rather than rows (the
    collision stays, but the limit becomes true). **The first changes what a session id LOOKS like; the second changes
    what the limit MEANS.** That is the operator's call, not a refactor's.
  * **DONE** — the code-side migration: LOGIC 48/48.

## SIX SURFACES MEASURED, ONE REAL DEBT, AND A YARDSTICK THAT WAS WRONG TWICE (rounds 147-163)

**THE LEDGER**: `main.ts` (1368 lines, four state-bearing clusters) was the ONE real debt, and it is paid — three
modules extracted, the last two unified into one factory. The other five surfaces were all maintained:

| surface | what the measurement showed |
|---|---|
| `panel-react` (231 files) | the biggest files ARE test files; the biggest hook is 220 lines of TYPES inside 540; the biggest component already has five helpers above it, and a test |
| `sweep_judge.rs` (2145 lines) | 40 top-level items — **54 lines each** — 13 in-file tests, and a dev tool |
| `index/` (13 + 15 source files) | four test files; its biggest numbers are `build/`/`dist/` artifacts |
| `mail/` (2 source files) | a Rust worker whose TEST file (12.8 KB) is bigger than its implementation (12.1 KB + 8.4 KB) |
| `relay/` (7 source files) | differential tests; its biggest number is a build artifact |
| `proxies/` (11 Rust files) | **93 test attributes**, and the repository's largest single file — 2946 lines — is its MOST fine-grained: **79 items, 37 lines each** |

**THE YARDSTICK, FOR WHOEVER MEASURES THE NEXT ONE**:

  * **COUNT `#[test]` ATTRIBUTES, NOT TEST FILES.** `find -path "*/tests/*"` counts DIRECTORIES, and this repository
    puts tests IN the file — `sweep_judge.rs` has 13, `summrise-shell-policy/src/tests.rs` has 56 cases, and `proxies/`
    has **93 across 11 files** where a file count said "3". **That metric was wrong every time this session used it**,
    and it produced a false alarm about `proxies/` that a single corrected count dissolved.
  * **SIZE MEANS NOTHING UNTIL IT IS SPLIT**: lines versus TOP-LEVEL ITEMS (37 and 54 lines per item are both healthy;
    a file with a handful of enormous functions is not), types versus JSX versus logic, and how much is already
    extracted into free functions above the component. **36.4 KB of hook was 220 lines of type declarations; 41.8 KB of
    component had five helpers already pulled out.**
  * **AND THE FIRST MEASUREMENT IS THE ONE THAT FOUND THE DEBT** — a 1368-line file holding four state-bearing clusters
    — while five later ones found nothing. **"Which file is biggest" pointed at build artifacts and well-organised
    tools; "how many unrelated jobs does this file do" pointed at the real problem.** That is the question worth asking
    first.

## FOUR INSTRUMENTS THAT ANSWERED THE WRONG QUESTION (rounds 115-169)

**EVERY ONE OF THESE WAS A GREP OR A COUNT, AND EVERY ONE MATCHED THE SHAPE I EXPECTED RATHER THAN THE SUBJECT.** Each
was caught only by reading the output — a count never explains itself, and three of the four were "confirmed" twice
before anyone looked.

| instrument | the answer it gave | the truth |
|---|---|---|
| "the latest CI run" (`runs?per_page=1`) | **failure** — so I reported a red `main` | that run was **a day old**; `main` was green. **A run list is not a promise about which commit a run belongs to** — re-runs, scheduled runs and older pushes all appear. Select by `head_sha`, and read `updated_at`: `in_progress` with a frozen timestamp means QUEUED, not running |
| `find -path "*/tests/*"` | `proxies/` has **3 test files** — "thin" | **93 `#[test]`/`#[tokio::test]` attributes across 11 files.** This repository puts tests IN the file (`sweep_judge.rs` 13, `summrise-shell-policy/src/tests.rs` 56 cases). **Count test ATTRIBUTES; a file count measured the directory layout, not the coverage** |
| `grep '\|' ` over `run:` blocks | **50** steps with pipelines | it matched the `run: \|` **block markers themselves**. The command has to skip the marker and look at command lines |
| "pipelines without `pipefail`" | **8** in `ci.yml` | **`\|\|` is not a pipe.** Four of the five I read were `cmd \|\| true` / `cmd \|\| { … }`. The two real ones (`$(grep … \| awk …)`, `$(npm pack \| tail -n 1)`) are both already defended — one by a `test -n "$TOKEN" \|\| exit 1` on the very next line, one by a `tar` that fails loudly two lines later, and the workflow's own comment states the rule: *"it is never the last command in this step"* |

**A FIFTH WAS NOT A TOOL BUG BUT A SCOPE GAP, AND IT IS THE ONE THAT MATTERED MOST**: `find … -printf "%s %p" | sort -rn
| head` surveys only what it is pointed at, and I pointed it at the shell, the panel, `index/`, `mail/`, `relay/` and
`proxies/` — **never at `agent/src/`**, the largest Rust tree in the repository, where `web/mod.rs` is **9888 lines**.
Size-ordering would not have found it either: what found it was asking **"how many unrelated jobs does this file do"**
and measuring **lines per top-level item** (225 there, against 37 and 54 in the two large-but-healthy files).

**THE RULE FOR WHOEVER DRIVES THIS REPOSITORY WITH GREPS**: *when a count does not match what you expected, suspect the
instrument before the subject* — and then **read the lines it selected**, because every one of the four above was
obvious the moment its output was read, and invisible while its number was trusted.

## A CALIBRATED YARDSTICK, AND THE NINE SURFACES IT MEASURED (rounds 165-176)

**TWO QUESTIONS FOUND THE DEBT, AND NEITHER WAS "WHICH FILE IS BIGGEST"**:

  * **"how many unrelated jobs does this file do?"** — `main.ts` held four state-bearing clusters in 1368 lines, and
    that was the repository's one real architectural debt (now paid: three modules extracted, the last two unified into
    one factory);
  * **"are these two the same thing?"** — inside `agent/src/web/mod.rs`, a third of the JSON literals were the same two
    response envelopes written out 49 times (now `ok_body()`/`error_body()` in `src/web/envelope.rs`, 50 sites
    absorbed, and the OTHER 116 literals deliberately left alone because they are records, not duplicates).

**THE MEASUREMENT THAT WORKS, AND ITS SCALE, FROM NINE SURFACES**:

  * **lines per TOP-LEVEL ITEM** (`^(pub )?(async )?(fn|struct|enum|impl|const|static|type|trait|mod)`), because a big
    file of small things is healthy and a medium file of enormous functions is not;
  * the healthy band observed: **23** (`user_store.rs`) · **34** (`model_store.rs`) · **37** (`api-relay/lib.rs`) ·
    **54** (`sweep_judge.rs`) · **59** (`psgen.rs`) · **61** (`request_shape.rs`, `dispatch.rs`) · **87**
    (`mcp_tools.rs`, mostly data). The two files that came in at **154-225** were the ones worth opening — and **even
    then the composition decides**: `web/mod.rs`'s 225 is 34 HTTP handlers, so it does ONE job and was left alone;
  * **count `#[test]`/`#[tokio::test]` ATTRIBUTES, never test FILES.** `gateway/wasm` has **295** across 51 files and
    `summrise-cli` **145** across 24 — and 145 is exactly the number its own suite reports, which is the first time the
    instrument and reality agreed. A `find -path "*/tests/*"` count said a well-tested line had three tests when it had
    **93**.

**THE LEDGER**: nine surfaces measured — the desktop shell (the real debt, paid), `panel-react`, `index/`, `mail/`,
`relay/`, `proxies/`, `agent/src/web/mod.rs`, `gateway/wasm`, `summrise-cli` — **and every one but the first came back
maintained, with healthy per-item ratios and a substantial in-file test surface.** Not every survey should find
something, and **a survey that keeps finding nothing is itself the finding**: this repository's files are large because
their jobs are large, not because they repeat themselves.

**AND WHAT THAT MEANS FOR THE NEXT PERSON**: stop looking for the biggest file. Ask what a file DOES — how many
unrelated jobs, and whether two of its parts are the same thing — and only then measure its size.

## THE REAL BAR IS 61 GATES, AND "LOCAL GREEN" IS NOT "CI GREEN" (rounds 189-196)

**`scripts/test/all-gates.bash` RUNS EVERY GATE `ci.yml` INVOKES, AND ITS LIST IS DERIVED FROM THE WORKFLOW RATHER
THAN RESTATED** — which is the whole point, because AGENTS.md's table lists the PER-DIRECTORY test commands and NOT the
forty-odd gate scripts the `pack-chain` job runs one step at a time. Its header cites the cost that bought it: round 41
had `contract-vocabulary-check` fail CI over a test fixture **while every command in the table was green**. **This
session went ten rounds before running it**, which means every "all green" claim made from the table's commands was
made against a subset. First run: **60 ok, 1 failed of 61**.

**AND THE MIRROR OF ROUND 41 IS ALSO TRUE — THE TWO HALVES PROVE DIFFERENT THINGS.** That one failed gate was
`cargo test --manifest-path proxies/zen-go-proxy/worker/Cargo.toml`, whose
`the_port_is_byte_identical_to_the_recorded_shipping_answers` **fails HERE and passes in CI**: a crate this session
never touched, with every pushed tip green. **So "local green" is not evidence of "CI green", and "CI green" is not
evidence of "local green"** — the check is only the same check when it runs on the same tree with the same command. The
other thing the first run proved is narrower and more useful: **`cargo fmt --check` prints NOTHING when the tree is
formatted**, and the script's rule is that *a gate which exits 0 having printed nothing has proved nothing* — so its
exception list had two hand-written entries, and a third fmt step (`gateway/wasm`) had gone into `ci.yml` without one.
**A list of exceptions maintained BY HAND, checked against a list DERIVED from the workflow, is always one crate behind
the workflow** — the exact drift the file's header refuses for its gate list. The fix is one pattern
(`cargo:…cargo fmt …-- --check`), and the same reasoning applies to the next exception anyone is tempted to write.

**AND THREE TIMES IN SIX ROUNDS I MUTED A COMMAND THAT WAS TRYING TO TELL ME SOMETHING** — each time while holding the
rule "read the exit code, not the output" as if it were the whole discipline:

  * `git commit --amend … 2>/dev/null` hid that it was rewriting an existing MERGE commit, and put a twin of it on
    `main` (the pre-commit hook had correctly refused the first attempt, and named the `main-only-by-merge` rule — the
    repository's check worked while mine was silenced);
  * `bash all-gates.bash | tail -6` threw away the NAME of the one failing gate, leaving a number to guess at;
  * `bash all-gates.bash | grep …` without `--line-buffered` turned a per-gate progress stream into one lump at the end.

**THE EXECUTABLE SHAPE**: when you need per-line state, **do not pipe the command that produces it**; if you must
filter, `grep --line-buffered` (or `stdbuf -oL`); and **discarding stderr is almost always wrong** — a tool that fails
tells you why on the stream people delete first. **Read the exit code AND the output, and never wrap a talking command
in something that stops it talking.**

## TWINS ARE COMPARED ON ONE THING, IN THE SAME COLUMN (rounds 203-207)

**"THE SAME DEFECT, FIXED ONCE AND LEFT STANDING IN THE TWIN" IS REAL HERE, AND COMPARING TWINS IS HOW YOU FIND IT.**
`agent/summrise-command-core`'s judge (package `summrise-agent-core`) and `agent/sweep-judge`'s are the same seventeen
clauses over the same report shape, and the SECOND one had already solved a defect the first still carried: its
outcomes returned `report_notes` and its example printed them, while the other wrote two clock notes into a `Vec` that
`judge_report` created and dropped — with a doc comment claiming they "land in `notes`". **Fixed once, left standing.**
The convergence commit (`38205d4a`'s parent) made the two judges' output agree instead of inventing a third
convention, and the golden pin moved ON ITS OWN (405 committed lines against 413 rendered, no test edited), which is
what proved the wiring was real rather than assumed.

**FIVE COMPARISONS, AND ONLY ONE DIVERGENCE — WHICH IS THE USEFUL SHAPE OF THIS RESULT**: the judges' clause NAMES are
identical (18 = 18, `diff` clean); both proxy workers sit inside the healthy band (69 and 27 lines per item); both
worker crates are covered (20 test attributes in-file against 13 in `tests/` plus 638 lines of them). **So the
repository's twins are maintained in step, and the one place they were not had a single, findable cause.** Compare
twins on THE THING THEY SHARE — a clause list, a channel, a contract — and never on size, which differs legitimately
when the two sides serve different upstreams.

**AND THE INSTRUMENT THAT MEASURES COVERAGE WAS WRONG TWICE, IN THE SAME WAY BOTH TIMES**:

  * `find -path "*/tests/*"` said `proxies/` had **3 test files**; it has **93 test attributes across 11 files**,
    because this repository writes tests IN the file;
  * counting `#[test]` in `src/` only said `relay/worker` had **ZERO tests**; it has **13 attributes and 638 lines**
    of them in `tests/`, including a 330-line differential harness.

**Both times the instrument measured WHERE THE TESTS ARE WRITTEN and I read it as WHETHER THERE ARE TESTS** — and the
second reading would have sent someone to write tests for a module that already had them. **Count `src/` AND `tests/`,
count ATTRIBUTES rather than files, and when a count disagrees with what you expect, suspect the instrument before the
subject.**

## A REAL TWIN IS FORCED TO AGREE BY A CONTRACT; A FALSE ONE ONLY LOOKS ALIKE (rounds 209-210)

**THE DISCRIMINATOR, AND IT COSTS A MINUTE TO APPLY**: before comparing two things, ask whether anything FORCES them to
agree. Seven comparisons now sit on both sides of that line:

  * **REAL TWINS — a shared contract**: the two judges judge the same report shape, and the wasm boundary is one
    contract with two sides. These are worth comparing, and comparing them found the only divergence of the seven (the
    notes channel, fixed once and left standing in the twin) and produced the hardest evidence in this file: **59 wasm
    exports against 59 JS consumers, zero dead surface, and a MODE OF THREE FILES per export — the `.ts`, the committed
    `.js`, and a test — so every one of those fifty-nine decisions is named by a test**;
  * **COMPARABLE, NOT BOUND — same shape, different upstreams**: the two proxy workers serve different providers and
    both sit inside the healthy band (69 and 27 lines per item), as do the two worker crates (20 test attributes
    in-file against 13 in `tests/` plus 638 lines). Nothing forces them together, so a difference is not a defect;
  * **FALSE TWINS — nothing shared at all**: `mail/worker` and `relay/worker` were assumed to share a path-secret
    convention. Measured: `mail` has a purpose-written `secret_eq` with its own reasoning comment (*"Unequal lengths
    answer immediately — the length is not the secret"*), **`relay` has no secret comparison anywhere because its
    routes are public DERP endpoints**, and no constant-time crate is used repository-wide. **The comparison produced
    nothing because there was nothing to compare — and that is a result worth having, because it takes a minute and it
    stops an afternoon of "harmonising" two things that were never the same.**

