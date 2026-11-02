# Summrise

`device/` (the runtime) and `docs/superpowers/specs/` (the design record).

**THE PRODUCT STATEMENT IS `docs/PRODUCT.md`** — the one sentence, the four nouns, the seven
surfaces and the nine design rules. A UI change that has not read it is a guess. The README is the
build/run map; this file carries only the rules that are not discoverable from the tree.

**THE REPOSITORY IS WRITTEN IN ENGLISH** — commits, comments, docs. **TALKING TO THE OPERATOR IS IN CHINESE.**

**READ THE EXIT CODE, NOT THE OUTPUT.** `cargo test` -> `test result: ok. N passed`. Grepping for the
wrong line returns nothing, which looks exactly like a suite that passed silently.

**RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check.

**PREFER RUST.** The runtime is Rust; a change that adds another language needs a reason, not a habit.
The console page is the one deliberate exception — see below.

**WINDOWS IS THE TARGET.** The lifecycle suite runs on `windows-latest` only. `cargo xwin check
--workspace --target x86_64-pc-windows-msvc` is the Linux-side compile check, not execution proof.

**THE DISK IS THE CONSTRAINT, NOT THE CPU.** `device/target` grows; clean it after a full run.

**THE CONSOLE PAGE IS REACT 19, BUILT INTO ONE HTML FILE.** `device/console/web` is the operator's
page; Vite builds `dist/index.html`, `summrise-console` embeds it with `include_str!`, and there is
no Node at runtime. A page change is a change in that tree followed by `scripts/build-web.sh
--check` — CI's `console-web` job runs the same typecheck and fails on a stale committed bundle.
`/next` is the maud fallback that redeems a login code; it is not the operator's path. The vendored
xterm and the device's own pages are separate.

**THE PAGE'S LOOK IS TESTED, NOT ARGUED.** A styling change that skips these is a guess:

- `cd device/console/web && npm run typecheck && npm test && npm run build` — the page compiles, its
  component suite passes, and the bundle is rebuilt. CI gates typecheck and the freshness check; the
  jsdom suite is a local author's tool, not a browser proof.
- `node device/console/web/scripts/screenshot.mjs <tag>` — the rendered gallery in
  `/tmp/console-react-shot/<tag>/` (light, dark, 390). Look at it; a change that moves a page nobody
  meant to touch is the finding.
- `scripts/build-web.sh --check` — the committed page is the source; the binary cannot ship
  yesterday's UI silently.
- `cargo test -p summrise-console` — the console suite and the token contract
  (`tests/tokens.rs`: seven type sizes, one spacing rhythm, the radii, the dark surfaces). The maud
  fallback's ink pairs are measured by `every_text_colour_reads_on_the_surface_it_sits_on` in
  `src/http/page.rs`; **the React page's own colour pairs are not yet behind a gate**, so measure a
  new pair before it lands.
- `node scripts/layout-gate.mjs <pages-dir> <width…>` — the rendered Rust pages and the landing at
  every width; a page that scrolls sideways fails.

The page's shape is one compact icon rail with seven destinations — Devices, Terminal, Screen, Files,
Runs, Machine, Activity — a canvas, and a status strip. The selected machine's header sits above
every workspace page, so a rail click never loses the machine the calls will hit. There is no second
navigation rail inside the page.
