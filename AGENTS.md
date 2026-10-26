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

**THE CONSOLE PAGE HAS ITS OWN GATE.** `device/console/web/dist/index.html` is committed because the
binary embeds it with `include_str!`, so a stale page still compiles and still ships. `ci.yml`'s
`console` job runs `npm ci`, `npm run typecheck`, `npm test` (vitest), `scripts/build-web.sh` and then
fails on a bundle that is not the one this source builds. Run those commands before a page commit; the
page's tests are real tests, not a build smoke.
