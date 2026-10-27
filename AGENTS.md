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
xterm and the terminal page's glue (`device/console/assets/`), and `scripts/console-smoke.mjs
--terminal` is its browser check.
