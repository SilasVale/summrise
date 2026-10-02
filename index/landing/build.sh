#!/usr/bin/env bash
# build.sh — the whole landing page, one command, and the numbers printed at the end.
#
#   ./build.sh            build everything, verify fidelity, print the numbers
#   ./build.sh --check    the above (the fidelity check is not optional; see below)
#
# WHAT IT PRODUCES, and where each half goes:
#
#   ../src/landing/setup.html      the document, with the three URL slots the worker fills
#   ../src/landing/npm-only.html   the tgz-only arm: no installer door at all
#   ../public/summrise-agent/landing/summrise_landing.js + _bg.wasm
#                                  the two behaviours, fetched AFTER the page has painted
#
# The worker imports the two documents as TEXT (wrangler's default `**/*.html` rule),
# so there is no subrequest on the page's path and no way for the page to fail because
# an asset fetch failed. The wasm pair is served by the same ASSETS binding that serves
# the tgz — the loader names an absolute path under that prefix, not `./pkg/…`, because
# the page is served at `/` and at `/index.html` and a relative specifier would resolve
# differently at the two.
#
# THE FIDELITY CHECK IS NOT OPTIONAL AND IS NOT A GATE. verify.mjs renders the page
# with this branch's index/src/page.js, renders it with this renderer, normalises ONLY
# the two places the migration is about, and requires the rest BYTE-IDENTICAL. It is
# the reason "the same measurements" is a fact rather than a claim, so a build that
# skipped it would be a build that could not say what it produced.
#
# Requirements (measured on this box; none of them is a framework):
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

PKG=../public/summrise-agent/landing

echo "── 1/4  the document (runs on the HOST; every word is in the HTML) ──"
cargo build --release --bin gen -q
./target/release/gen slots ../src/landing

echo "── 2/4  the two behaviours (wasm, fetched after paint) ──"
cargo build --release --target wasm32-unknown-unknown --lib -q
# **BUILD INTO A SCRATCH DIRECTORY AND MOVE THE PAIR IN ONLY ON SUCCESS.** This step used to
# `rm -rf "$PKG"` and then call `wasm-bindgen` — so a box without that tool (or any failure between
# the two lines) DELETED the tracked pair that the worker serves, and the page lost its behaviours
# until someone rebuilt. Measured 2026-09-30: one run of this script on a box without wasm-bindgen
# left `index/public/summrise-agent/landing/` with two deleted files, and two gates failed on it.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
wasm-bindgen --target web --out-dir "$STAGE" --no-typescript \
  target/wasm32-unknown-unknown/release/summrise_landing.wasm
BG="$STAGE/summrise_landing_bg.wasm"
# Rust 1.98 emits bulk-memory and nontrapping-float-to-int; wasm-opt 132 refuses
# the module without being told they are allowed.
"$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG" -o "$BG.opt"
mv "$BG.opt" "$BG"
mkdir -p "$PKG"
mv "$STAGE/summrise_landing.js" "$STAGE/summrise_landing_bg.wasm" "$PKG/"

echo "── 3/4  fidelity: the Rust page against the page it replaces ──"
node verify.mjs

echo "── 4/4  THE NUMBERS ──"
# THE SERVED BYTES, not the module's. The page a reader gets is the document with
# the three URLs filled in, so measuring `../src/landing/setup.js` would measure the
# file the WORKER reads — slots and export statement included — and report a size no
# browser ever sees.
CONSOLE_URL="https://agent.saisi.online"
INSTALLER_URL="https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz"
SETUP_URL="https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe"
./target/release/gen render dist/served-setup.html "$CONSOLE_URL" "$INSTALLER_URL" "$SETUP_URL" >/dev/null

WASM_RAW=$(stat -c%s "$BG")
WASM_GZ=$(gzip -9 -c "$BG" | wc -c)
JS_GZ=$(gzip -9 -c "$PKG/summrise_landing.js" | wc -c)
HTML=$(stat -c%s dist/served-setup.html)
HTML_GZ=$(gzip -9 -c dist/served-setup.html | wc -c)
printf 'wasm after wasm-opt -Oz : %8s bytes\n' "$WASM_RAW"
printf 'wasm gzipped (-9)        : %8s bytes\n' "$WASM_GZ"
printf 'js glue gzipped          : %8s bytes\n' "$JS_GZ"
printf '  payload over the wire  : %8s bytes  (wasm+glue, gzipped)\n' "$((WASM_GZ + JS_GZ))"
printf 'html served (Rust)       : %8s bytes / %s gzipped\n' "$HTML" "$HTML_GZ"
printf '  page, total over wire  : %8s bytes\n' "$((HTML_GZ + WASM_GZ + JS_GZ))"
printf 'html served (page.js)    : %8s bytes / %s gzipped  (the page it replaces)\n' \
  "$(stat -c%s dist/pagejs-reference.html)" \
  "$(gzip -9 -c dist/pagejs-reference.html | wc -c)"
printf 'rust source lines        : %8s\n' "$(cat src/*.rs src/bin/*.rs | wc -l)"
