# Summrise

`gateway/` (worker) · `agent/` (Windows agent) · `index/` (CDN) · `proxies/` · `brand/` · `docs/`.
The operator's rules are `docs/CHARTER.md`; their inbox is `docs/agents/ideas.md`.

**THE COMMANDS, THE GATE LIST AND THE PER-DIRECTORY TEST MATRIX ARE `ci.yml` — read them there.** A copy here goes
stale; this file carries only what is not discoverable from the tree.

**THE REPOSITORY IS WRITTEN IN ENGLISH** — commits, comments, docs. **TALKING TO THE OPERATOR IS IN CHINESE.**

**READ THE EXIT CODE, NOT THE OUTPUT.** The suites do not share a reporter: `cargo test` -> `test result: ok. N passed`;
`npm test` in `gateway/` -> `ℹ pass N`; `npx vitest run` in `panel-react/` -> `Tests N passed`. Grepping for the wrong
line returns NOTHING, which looks exactly like a suite that passed silently.

**RUN THE COMMAND THE OTHER END RUNS** — a local check that is not CI's check is not the same check.

**PREFER RUST.** A change to a `.js`/`.mjs`/`.cjs` file needs a reason, not a habit: `agent/tests/js_boundary_inventory.rs`
classifies every tracked JS/TS file against `agent/tests/fixtures/js-boundaries.txt` and caps the LOGIC count.

**THE DISK IS THE CONSTRAINT, NOT THE CPU** (one HDD, measured 84% full on 2026-10-13). A full local run leaves ~15 GB of
`target/` behind, so `bash scripts/clean-build-space.bash` after one, and `--targets` for the crates not in flight.
Debug info is off machine-wide (`~/.cargo/config.toml`: `debug = 0`) and that is measured rather than cosmetic:
`gateway/wasm/target` went 2.9 GB -> 403 MB, `agent/target` 13 GB -> ~2 GB.

**THE RULES ARE ENFORCED, NOT WRITTEN DOWN**: `main_shape.rs` (main advances only by merge), the pre-commit and pre-push
hooks (the emitters; no push while CI runs), `publish-release.bash` (the whole-tree mode check), and every gate's own
`MUTATION:`/`RESULT:` header. **A sentence that can be enforced should be enforced instead of written down.**

**WHERE THE LONG FORM LIVES**: the commit message (the measurement and the `VERIFIED:` line — this is the record) ·
`CONTEXT.md` (a word and what it means) · the gate itself (its mutation proof) · `docs/superpowers/` (designs and
decisions, including the wrong ones) · `scripts/RELEASE.md` (the release runbook) · `scripts/DERP.md` (the tailnet's
self-hosted relay on the exit node, and the traps it cost to learn).
