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
