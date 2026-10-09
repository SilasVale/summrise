#!/usr/bin/env bash
# Build the Electron shell's remaining decisions to wasm and install it where the shell reads it.
#
# WHY wasm AND NOT A napi NATIVE ADDON — the decision `agent/summrise-url-policy/build.sh` records and this
# file inherits rather than re-argues: the Windows cross-build the release already runs would produce a
# napi addon for free, and it could not be EXECUTED on the Linux box this repository is developed on, or
# in CI, so its correctness could only be discovered on the device, after a release. wasm runs everywhere,
# which is the property that makes this port's own tests run here: `cargo test -p summrise-shell-policy`
# (the 55 cases in src/tests.rs) and `agent/summrise-desktop-electron/test/shell-policy-wasm.test.mjs`
# (which loads the COMMITTED artifact in Node and calls every export).
#
# WHY `--target nodejs`: the shell's main process is Node behind Electron, so the glue is CommonJS and
# loads the module SYNCHRONOUSLY — `fs.readFileSync` plus a compiled `WebAssembly.Module` — and the first
# decision may run immediately. THAT MATTERS MORE HERE THAN IT DID FOR THE URL POLICY: the very first
# thing `main.ts` does with this module is resolve the agent's port and pin the url-policy crate's
# predicates from it, before any window exists. `--target web` would need an async `init()` awaited before
# that, and there would be no correct place to put the await.
#
# WHY `--no-opt`: wasm-pack optimizes with wasm-opt by default and FETCHES that binary from a GitHub
# release; on this box that download does not finish (P0 measured a 20 MB asset still crawling after 25
# minutes). wasm-pack 0.15.0 has no environment variable for a system wasm-opt. The url-policy crate
# MEASURED that wasm-opt is a net loss for these artifacts anyway — it removed 82,040 raw bytes and ADDED
# 13,262 bytes to the tarball, because the module is ICU4X DATA behind `idna`/`url` and the optimizer
# makes data less compressible — so skipping it costs nothing and keeps the artifact a plain function of
# the Rust toolchain, which is also what lets the freshness gate run anywhere `cargo` and `wasm-pack` are.
#
# SIZE: `--check` prints the three files' sizes; the numbers belong in a commit message that measured
# them rather than in a comment that remembers them.
#
# WHY THE PROFILE IS SET IN THE ENVIRONMENT RATHER THAN IN Cargo.toml: this crate is a member of the
# `agent` workspace, and cargo IGNORES a member's `[profile.release]` table while building with the
# ROOT's. Setting it at the root would change `summrise-agent.exe`'s bytes, which release.yml's
# dual-builder audit compares. The overrides below are the profile this crate asks for, named in the one
# place that builds it — and the freshness gate runs THIS script, so the bytes it compares are produced
# by the same command. `strip=symbols` is the lever the url-policy crate measured (29% raw, 9.4% on the
# wire) and it is applied here for the same reason: what ships is a tarball.
#
# WHERE THE OUTPUT GOES:
#
#   pkg/summrise_shell_policy.js            -> src/   the CommonJS glue the shell's main.js requires
#   pkg/summrise_shell_policy.d.ts          -> src/   its types, so a tsc run resolves the import
#   pkg/summrise_shell_policy_bg.wasm       -> src/   THE MODULE
#   pkg/summrise_shell_policy_bg.wasm.d.ts  NOT installed — nothing imports it, exactly as the url
#                                                    policy's fourth pkg file is not installed.
#
# THREE OF THE FOUR ARE COMMITTED (the glue, its types, the module), the way this repository treats built
# artifacts: a checkout has no wasm-pack, and every job builds from the committed file. The npm package's
# copy of the TWO it ships is produced by that package's own build (`npm run build` in
# agent/summrise-agent-npm), so there is ONE producer per tree.
#
# HOW IT IS INVOKED, and the second form is the one a gate uses:
#
#   ./build.sh          build and INSTALL the three files into the shell's src/
#   ./build.sh --check  build into a scratch directory, CMP all three against the committed ones, and CMP
#                       the shipped pair against the npm package's copy — exit 1 on drift, naming each
#                       file and the command that refreshes it.
#                       `scripts/test/shell-policy-wasm-freshness.bash` calls THIS, so the bytes a gate
#                       compares are produced by the same command as the bytes a build installs; a second
#                       copy of the wasm-pack line is how the two would come to disagree.
#
# AND `--check` EXISTS BECAUSE THE ARTIFACTS ARE COMMITTED: a Rust edit that nobody rebuilt ships the OLD
# decisions, and nothing else in this repository can see it. The shell's own suite does not: it loads the
# committed bytes and passes on a stale artifact.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

DST=../summrise-desktop-electron/src
NPM_DST=../summrise-agent-npm/summrise-desktop-electron/src
OUT_NAME=summrise_shell_policy
# The three the shell's src/ holds, and the two of them the npm package ships.
FILES=("$OUT_NAME.js" "$OUT_NAME.d.ts" "${OUT_NAME}_bg.wasm")
SHIPPED=("$OUT_NAME.js" "${OUT_NAME}_bg.wasm")

build() { # $1 = the out-dir
  echo "── wasm-pack build --target nodejs --no-opt (opt-level=z, lto, cgu=1, panic=abort, strip) ──"
  CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
  CARGO_PROFILE_RELEASE_LTO=true \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  CARGO_PROFILE_RELEASE_PANIC=abort \
  CARGO_PROFILE_RELEASE_STRIP=symbols \
    wasm-pack build --target nodejs --no-opt --out-dir "$1" --out-name "$OUT_NAME"
}

sizes() { # $@ = the files to report
  local f
  for f in "$@"; do
    printf '  %8s bytes / %8s gzipped   %s\n' "$(stat -c%s "$f")" "$(gzip -9 -c "$f" | wc -c)" "$f"
  done
}

case "${1:-}" in
  --check)
    OUT="$(mktemp -d)"
    trap 'rm -rf "$OUT"' EXIT
    build "$OUT"
    fail=0
    # THE SHIPPED COPY FIRST, because it is the one the DEVICE reads and the one no compiler checks. A
    # device that received the new `main.js` and not the module would fail to start the shell AT ALL.
    for f in "${SHIPPED[@]}"; do
      if ! cmp -s "$NPM_DST/$f" "$DST/$f"; then
        echo "::error::agent/summrise-agent-npm/summrise-desktop-electron/src/$f differs from the shell's copy — run agent/summrise-agent-npm 'npm run build' (it copies the pair) and commit both" >&2
        fail=1
      fi
    done
    for f in "${FILES[@]}"; do
      if ! cmp -s "$OUT/$f" "$DST/$f"; then
        echo "::error::$DST/$f is STALE — the crate changed without a rebuild. Fix it with: bash agent/summrise-shell-policy/build.sh && (cd agent/summrise-agent-npm && npm run build), then commit" >&2
        fail=1
      fi
    done
    [ "$fail" -eq 0 ] || exit 1
    echo "shell-policy wasm freshness OK (${#FILES[@]} files rebuilt and compared byte for byte, ${#SHIPPED[@]} of them against the shipped copy)"
    ;;
  "")
    build pkg
    mkdir -p "$DST"
    local_f=
    for local_f in "${FILES[@]}"; do cp "pkg/$local_f" "$DST/"; done
    sizes "${FILES[@]/#/$DST/}"
    ;;
  *)
    echo "usage: $0 [--check]" >&2
    exit 2
    ;;
esac
