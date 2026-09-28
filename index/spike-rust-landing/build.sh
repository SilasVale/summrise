#!/usr/bin/env bash
# build.sh — the whole spike, one command, and the three numbers printed at the end.
#
#   ./build.sh            build dist/ and print the numbers
#   ./build.sh --check    also verify the Rust HTML against index/src/page.js, byte for byte
#
# Requirements (all measured on this box; none of them is a framework):
#   rustup target add --toolchain 1.98.1-x86_64-unknown-linux-gnu wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.129      # must match the crate
#   wasm-opt from binaryen (npm i binaryen)               # WASM_OPT=/path/to/wasm-opt
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
if [ -z "$WASM_OPT" ]; then
  echo "wasm-opt not found. Set WASM_OPT=/path/to/wasm-opt (npm i binaryen gives one)." >&2
  exit 1
fi

echo "── 1/4  HTML (runs on the HOST; every word is in the document) ──"
cargo build --release --bin gen -q
./target/release/gen dist/index.html setup
./target/release/gen dist/npm-only.html no-setup

echo "── 2/4  wasm (the two behaviours) ──"
cargo build --release --target wasm32-unknown-unknown --lib -q
rm -rf dist/pkg
wasm-bindgen --target web --out-dir dist/pkg --no-typescript \
  target/wasm32-unknown-unknown/release/spike_rust_landing.wasm
BG=dist/pkg/spike_rust_landing_bg.wasm
# Rust 1.98 emits bulk-memory and nontrapping-float-to-int; wasm-opt 132 refuses
# the module without being told they are allowed.
"$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG" -o "$BG.opt"
mv "$BG.opt" "$BG"

echo "── 3/4  fidelity: the Rust page against the page it replaces ──"
node verify.mjs

echo "── 4/4  THE THREE NUMBERS ──"
WASM_RAW=$(stat -c%s "$BG")
WASM_GZ=$(gzip -9 -c "$BG" | wc -c)
JS_GZ=$(gzip -9 -c dist/pkg/spike_rust_landing.js | wc -c)
HTML=$(stat -c%s dist/index.html)
HTML_GZ=$(gzip -9 -c dist/index.html | wc -c)
printf 'wasm after wasm-opt -Oz : %8s bytes\n' "$WASM_RAW"
printf 'wasm gzipped (-9)        : %8s bytes\n' "$WASM_GZ"
printf 'js glue gzipped          : %8s bytes\n' "$JS_GZ"
printf '  payload over the wire  : %8s bytes  (wasm+glue, gzipped)\n' "$((WASM_GZ + JS_GZ))"
printf 'html (Rust)              : %8s bytes / %s gzipped\n' "$HTML" "$HTML_GZ"
printf 'html (page.js, for ref)  : %8s bytes / %s gzipped\n' \
  "$(stat -c%s dist/pagejs-reference.html)" \
  "$(gzip -9 -c dist/pagejs-reference.html | wc -c)"
printf 'rust source lines        : %8s\n' "$(cat src/*.rs src/bin/*.rs | wc -l)"
printf 'page.js lines (replaced) : %8s\n' "$(wc -l < ../src/page.js)"
