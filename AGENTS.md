# Summrise

`device/` (the runtime) and `docs/superpowers/specs/` (the design record).

**THE REPOSITORY IS WRITTEN IN ENGLISH** — commits, comments, docs. **TALKING TO THE OPERATOR IS IN CHINESE.**

**READ THE EXIT CODE, NOT THE OUTPUT.** `cargo test` -> `test result: ok. N passed`. Grepping for the
wrong line returns nothing, which looks exactly like a suite that passed silently.

**RUN THE COMMAND THE OTHER END RUNS.** A local check that is not CI's check is not the same check.

**PREFER RUST.** The runtime is Rust; a change that adds another language needs a reason, not a habit.

**WINDOWS IS THE TARGET.** The lifecycle suite runs on `windows-latest` only. `cargo xwin check
--workspace --target x86_64-pc-windows-msvc` is the Linux-side compile check, not execution proof.

**THE DISK IS THE CONSTRAINT, NOT THE CPU.** `device/target` grows; clean it after a full run.

**THE CONSOLE PAGE IS RUST.** `/` redirects to `/next`, which the console renders from `maud`
templates; there is no npm, no committed bundle and no second build. A page change is a Rust change,
gated by `cargo test -p summrise-console` like everything else. The only JavaScript left is vendored
xterm and the two terminal pages' glue (`device/console/assets/`, `device/device/assets/`), and
`scripts/console-smoke.mjs --terminal` is its browser check.

**THE PAGE'S LOOK IS TESTED, NOT ARGUED.** Three tools answer "did this styling change break
something", and a change that skips them is a guess:

- `cargo test -p summrise-console` runs `every_text_colour_reads_on_the_surface_it_sits_on`, which
  measures eleven pairs in each theme (ink, muted, faint, accent, the three status colours, the
  rail's ink, and a primary button's label) against WCAG AA (4.5:1) — a colour that fails it is
  unreadable in one theme, which is not a matter of taste;
- `scripts/console-screenshots.bash --out DIR` renders every page and photographs it light and dark,
  writing `DIR/index.html` — the gallery is how a change gets looked at, not described;
- the same script with `--diff` (against the baseline `--update-baseline` wrote) lists the pages a
  change moved, with the share of pixels and a highlighted image per page. A change that moves a page
  nobody meant to touch is the bug this catches.

The page's shape is three columns — a 52px icon rail, a 244px context rail, a canvas, a status strip —
and the context rail holds the page's own subject (sessions, runs, processes, targets, entries,
disks), never a second copy of the navigation. Below 1000px the context rail goes first.
