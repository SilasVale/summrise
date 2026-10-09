#!/usr/bin/env bash
# ── THE MUTATION THAT MUST FAIL THIS GATE (run, 2026-10-09) ──────────────────────────────────────
# MUTATION: in agent/summrise-url-policy/src/lib.rs, `const BLANK: &str = "about:blank";` ->
#           `const BLANK: &str = "about:blankX";` — a change to the CODE. A COMMENT edit is NOT a
#           mutation: it rebuilds and produces the same bytes, and this gate is right to pass on that.
# RESULT:   exit 1 —
#             ::error::../summrise-desktop-electron/src/summrise_url_policy_bg.wasm is STALE — the crate
#             changed without a rebuild. Fix it with: bash agent/summrise-url-policy/build.sh && …
#           and exit 0 again the moment the source is restored (the gate builds into a scratch directory,
#           so it never writes the tree it is judging).
#
# url-policy-wasm-freshness — THE COMMITTED WASM POLICY MUST BE WHAT THE CRATE BUILDS.
#
# ── WHY THIS EXISTS, and it is the cost landing 6 was warned about ────────────────────────────────
#
# The desktop shell's URL/certificate policy is Rust compiled to wasm, and the glue + the module are
# COMMITTED (a checkout has no wasm-pack, and every job builds from the committed file — the same
# arrangement `agent/resources/panel-logic`'s three committed files have). So a Rust edit that nobody
# rebuilt ships the OLD POLICY, silently, on the next release. **NOTHING ELSE IN THIS REPOSITORY CAN SEE
# THAT**: `agent/summrise-desktop-electron/npm test` loads the committed bytes, so it passes on a stale
# artifact; `cargo test -p summrise-url-policy` tests the SOURCE; and the shell's `electron src freshness`
# step compares tsc output, which no longer includes this module at all.
#
# ── WHAT IT RUNS, AND WHY IT IS NOT A SECOND COPY OF THE BUILD ────────────────────────────────────
#
# `agent/summrise-url-policy/build.sh --check` — the SAME script and the same command a build uses, with
# the output going to a scratch directory instead of the tree. It rebuilds the module and compares bytes:
#
#   1. the npm package's shipped copy  vs  agent/summrise-desktop-electron/src/   (the pair a device reads)
#   2. a fresh build                   vs  agent/summrise-desktop-electron/src/   (glue, types, module)
#
# and it exits 1 naming each stale file with the command that refreshes it. A second wasm-pack line here
# is how a gate and a build would come to disagree about what "fresh" means.
#
# ── IT EXITS 2 WHEN IT CANNOT RUN AT ALL ─────────────────────────────────────────────────────────
#
# `cargo`, `wasm-pack` and the `wasm32-unknown-unknown` target are all required, and `all-gates.bash`
# reports exit 2 as "n/a": a host without them cannot make this judgement, and saying so is better than a
# green that means nothing (the convention `scripts/test/rust-byte-checks.bash` established). THE CHEAP
# QUESTIONS COME FIRST — a target check before a tool install, the lesson that script records.
#
# AND THE HALF THAT NEEDS NO TOOLCHAIN IS NOT LOST ON SUCH A HOST: the shipped-copy comparison above is
# also made by the release path, which every builder runs — `scripts/publish-release.sh` and the
# `electron src copies in sync` loops in `ci.yml` / `release.yml` compare the same files with `cmp`.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
export PATH="$HOME/.cargo/bin:$PATH"

# THE CHEAP QUESTIONS FIRST (measured lesson from rust-byte-checks.bash: a tool install in front of a
# target check spent minutes building a tool the host could not use).
if ! command -v cargo >/dev/null 2>&1; then
  echo "  n/a no cargo on PATH — the freshness check rebuilds the wasm module"
  exit 2
fi
if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "  n/a no wasm-pack on PATH — the freshness check rebuilds the wasm module"
  exit 2
fi
if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "  n/a the wasm32-unknown-unknown target is not installed, so the module cannot be rebuilt"
  exit 2
fi

# THE STATUS IS KEPT BY EXITING, NOT BY PRINTING (`cmd && echo ok || echo FAIL` throws it away), and the
# build's own output is not swallowed: a compile error must read as a compile error, not as a stale
# artifact. `--check` exits 1 with `::error::` lines that name the file and the fix.
if ! bash agent/summrise-url-policy/build.sh --check; then
  echo "url-policy-wasm-freshness: the committed wasm policy is NOT what agent/summrise-url-policy builds."
  echo "  The files are COMMITTED, so this is the only place the drift could be caught: rebuild, commit,"
  echo "  and re-run. If the crate did not change, the build is not reproducible — say so rather than"
  echo "  committing a different artifact."
  exit 1
fi
exit 0
