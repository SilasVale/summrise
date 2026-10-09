#!/usr/bin/env bash
# Build the Electron shell's URL/origin/certificate policy to wasm and install it where the shell reads it.
#
# WHY wasm AND NOT A napi NATIVE ADDON (the decision this landing implements, taken before it started):
# the Windows cross-build the release already runs would produce a napi addon for free — and it could not
# be EXECUTED on the Linux box this repository is developed on, or in CI, so its correctness could only be
# discovered on the device, after a release. wasm runs everywhere, which is the property that makes this
# port's own tests run here: `cargo test -p summrise-url-policy` (the 11 ported cases) and
# `agent/summrise-desktop-electron/test/url-policy-wasm.test.mjs` (which loads the COMMITTED artifact in
# Node and calls every export). The cost is the artifact, and it is paid deliberately: see SIZE below.
#
# WHY `--target nodejs`: the shell's main process is Node behind Electron, so the glue is CommonJS and
# loads the module SYNCHRONOUSLY — `fs.readFileSync` plus a compiled `WebAssembly.Module` — and the first
# predicate may run immediately. `--target web` (what `agent/resources/panel-logic` uses) would need an
# async `init()` awaited before any decision, which is the constraint the panel's migration had to design
# around.
#
# WHY `--no-opt`: wasm-pack optimizes with wasm-opt by default and fetches that binary from a GitHub
# release; on this box that download does not finish (P0 measured a 20 MB asset still crawling after 25
# minutes, and wasm-pack's own fetch sat on a lock file for 14 minutes). wasm-pack 0.15.0 has no
# environment variable for a system wasm-opt, so the step is skipped (`--no-opt`) and NOT replaced: see
# SIZE for the measurement that says what it would have bought.
#
# SIZE, MEASURED, because "a wasm artifact in the shipped package" is this landing's named cost. Every
# row is a build of THIS crate on this box, 2026-10-09, and the LAST COLUMN is the one that ships: an npm
# tarball is gzipped, so what a device downloads is the `npm pack` size, measured by swapping each module
# into the real package and packing it (`npm pack --pack-destination /tmp/...` in agent/summrise-agent-npm).
#
#     cargo's release defaults                                  1,562,964 bytes
#     + opt-level=z, lto, cgu=1, panic=abort                    1,551,194 bytes   tgz 562,365
#     + strip=symbols              <- THIS BUILD                1,108,704 bytes   tgz 509,242
#     opt-level=s instead of z, same flags                      1,133,494 bytes     (LARGER — not used)
#     the winner, through wasm-opt -Oz                          1,026,664 bytes   tgz 522,504
#
# **THE LEVER IS `strip`, NOT THE OPTIMIZATION LEVEL, AND THAT IS WHY IT WAS MEASURED RATHER THAN
# ASSUMED.** opt-level=z + LTO + codegen-units=1 buys 0.7% — the artifact is DATA, not code: the ICU4X
# tables behind `idna`, which is what makes the host parser agree with Chromium (`url` depends on `idna`
# unconditionally, and that agreement is the whole reason this crate uses the WHATWG implementation
# instead of a regex). Removing the name section buys 29% RAW and **53,123 bytes on the wire (-9.4%)**.
# The cost of stripping is a wasm trap whose stack trace names `wasm-function[41]` instead of a Rust
# symbol; the module is 18 small predicates whose behaviour is pinned by `cargo test -p summrise-url-policy`
# and by the Node suite that loads these bytes, which is a better regression net than a symbol name.
#
# **AND wasm-opt IS A NET LOSS HERE, WHICH IS THE MEASUREMENT THAT SETTLES `--no-opt`.** binaryen 132
# (`-Oz --enable-bulk-memory --enable-nontrapping-float-to-int`, 3.1 s) takes the stripped module from
# 1,108,704 bytes to 1,026,664 — and takes the TARBALL from 509,242 bytes to **522,504, i.e. +13,262
# bytes on the wire**: it removes bytes and makes the data less compressible, and the tarball is what a
# device downloads. It is also on this box already (`~/.cache/worker-build/wasm-opt-x86_64-linux-132`,
# which `worker-build` caches), so this is a decision about the artifact rather than about a download.
# The panel's and the console's build.sh DO hard-fail without wasm-opt, for a reason that does not apply
# here: their wasm is FETCHED BY A BROWSER on every page load (panel-logic's build.sh records 7,099 gz as
# the difference between building with it and without). This module is read once from disk by an Electron
# main process. So no optimizer is required, and the artifact stays a plain function of the Rust
# toolchain — which is also what lets the freshness gate run anywhere `cargo` and `wasm-pack` are.
#
# WHY THE PROFILE IS SET IN THE ENVIRONMENT RATHER THAN IN Cargo.toml: this crate is a member of the
# `agent` workspace, and cargo IGNORES a member's `[profile.release]` table while building with the
# ROOT's (it prints a warning and uses the root's). Setting it at the root would change
# `summrise-agent.exe`'s bytes, which release.yml's dual-builder audit compares. The overrides below are
# the profile this crate asks for, named in the one place that builds it — and the freshness gate runs
# THIS script, so the bytes it compares are produced by the same command.
#
# WHERE THE OUTPUT GOES:
#
#   pkg/summrise_url_policy.js            -> src/   the CommonJS glue the shell's main.js requires
#   pkg/summrise_url_policy.d.ts          -> src/   its types, so a tsc run resolves the import
#   pkg/summrise_url_policy_bg.wasm       -> src/   THE MODULE
#   pkg/summrise_url_policy_bg.wasm.d.ts  NOT installed — nothing imports it, and the panel's build.sh
#                                                  installs the same two of its four pkg files
#
# THREE OF THE FOUR ARE COMMITTED (the glue, its types, the module — the fourth, `_bg.wasm.d.ts`,
# nothing reads and nothing ships), the way this repository treats built artifacts: a checkout has no
# wasm-pack, and every job builds from the committed file. The npm package's copy of the TWO it ships is
# produced by that package's own build (`npm run build` in agent/summrise-agent-npm), so there is ONE
# producer per tree.
#
# HOW IT IS INVOKED, and the second form is the one a gate uses:
#
#   ./build.sh          build and INSTALL the three files into the shell's src/
#   ./build.sh --check  build into a scratch directory, CMP all three against the committed ones, and CMP
#                       the shipped pair against the npm package's copy — exit 1 on drift, naming each
#                       file and the command that refreshes it. `scripts/test/url-policy-wasm-freshness.bash`
#                       calls THIS, so the bytes a gate compares are produced by the same command as the
#                       bytes a build installs; a second copy of the wasm-pack line is how the two would
#                       come to disagree.
#
# AND `--check` EXISTS BECAUSE THE ARTIFACTS ARE COMMITTED: a Rust edit that nobody rebuilt ships the OLD
# policy, and nothing else in this repository can see it. The shell's own suite does not: it loads the
# committed bytes and passes on a stale artifact.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

DST=../summrise-desktop-electron/src
NPM_DST=../summrise-agent-npm/summrise-desktop-electron/src
OUT_NAME=summrise_url_policy
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

sizes() { # $1 = the directory to report
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
    # THE SHIPPED COPY FIRST, because it is the one the DEVICE reads and the one no compiler checks.
    for f in "${SHIPPED[@]}"; do
      if ! cmp -s "$NPM_DST/$f" "$DST/$f"; then
        echo "::error::agent/summrise-agent-npm/summrise-desktop-electron/src/$f differs from the shell's copy — run agent/summrise-agent-npm 'npm run build' (it copies the pair) and commit both" >&2
        fail=1
      fi
    done
    for f in "${FILES[@]}"; do
      if ! cmp -s "$OUT/$f" "$DST/$f"; then
        echo "::error::$DST/$f is STALE — the crate changed without a rebuild. Fix it with: bash agent/summrise-url-policy/build.sh && (cd agent/summrise-agent-npm && npm run build), then commit" >&2
        fail=1
      fi
    done
    [ "$fail" -eq 0 ] || exit 1
    echo "url-policy wasm freshness OK (${#FILES[@]} files rebuilt and compared byte for byte, ${#SHIPPED[@]} of them against the shipped copy)"
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
