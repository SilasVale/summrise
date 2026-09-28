#!/usr/bin/env bash
# The one command that builds the proof, and the sizes it prints.
#
# WHY `--no-opt`: wasm-pack optimizes with wasm-opt by default, and it fetches that binary from a
# GitHub release. On this box GitHub releases are unusable (measured 2026-09-28: a 20 MB asset was
# still crawling after 25 minutes, and wasm-pack's own download sat on a lock file for 14 minutes
# with the crate already compiled). wasm-pack 0.15.0 has NO environment variable for a system
# wasm-opt — `strings` on the binary shows the hardcoded binaryen URL — so the step is skipped here
# and run below from a wasm-opt that is already on the box.
#
# WHY THE TWO FLAGS ON wasm-opt: see README.md. They are harmless when the module does not need
# them (measured: byte-identical output) and required when it does.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"

echo "── wasm-pack build --target web --no-opt ──"
wasm-pack build --target web --no-opt

BG=pkg/wasm_pack_proof_bg.wasm
GLUE=pkg/wasm_pack_proof.js
printf 'wasm as emitted : %8s bytes / %8s gzipped\n' "$(stat -c%s "$BG")" "$(gzip -9 -c "$BG" | wc -c)"
printf 'js glue         : %8s bytes / %8s gzipped\n' "$(stat -c%s "$GLUE")" "$(gzip -9 -c "$GLUE" | wc -c)"

if [ -n "$WASM_OPT" ]; then
  echo "── wasm-opt -Oz (binaryen 132) ──"
  "$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG" -o "$BG.opt"
  mv "$BG.opt" "$BG"
  printf 'wasm optimized  : %8s bytes / %8s gzipped\n' "$(stat -c%s "$BG")" "$(gzip -9 -c "$BG" | wc -c)"
else
  echo "wasm-opt not found — set WASM_OPT=/path/to/wasm-opt (npm i binaryen gives one)" >&2
fi
