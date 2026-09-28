#!/usr/bin/env bash
# Build the CONSOLE's Rust logic to wasm and install it where the console reads it.
#
# THE SHAPE IS THE PANEL'S, DELIBERATELY (`agent/resources/panel-logic/build.sh`), including the two
# things that are not obvious: `--no-opt` because wasm-pack fetches wasm-opt from a GitHub release
# and that path does not finish on this box, and the two wasm-opt flags because they are a property
# of the CODE rather than of the toolchain (measured in P0: wasm-opt refuses the module without them
# and accepts its own minimal crate byte-identically, so they are always passed).
#
# WHERE THE OUTPUT GOES, and it is NOT the panel's arrangement:
#
#   pkg/ui_logic.js        -> ui/src/wasm/          the `--target web` glue (ESM)
#   pkg/ui_logic.d.ts      -> ui/src/wasm/          its types, for `tsc -b`
#   pkg/ui_logic_bg.wasm   -> ui/public/            THE SERVED ASSET
#
# `ui/public/` is vite's publicDir and `ui/vite.config.ts` sets `outDir: "../public"`, so a file
# placed there is copied VERBATIM into `gateway/public/` by the console's own build — which is what
# makes this a one-line install rather than a hashed-asset rewrite. The panel instead writes beside
# `panel.js` because the agent embeds and serves that directory by name; the console is served by
# the worker out of `gateway/public/`, so its asset belongs to vite.
#
# ALL THREE FILES ARE COMMITTED, for the same reason the panel's are: a checkout has no wasm-pack,
# and every CI job builds from the committed artifact.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
GLUE_SRC=pkg/ui_logic.js
BG_SRC=pkg/ui_logic_bg.wasm
GLUE_DST=../ui/src/wasm
ASSET_DST=../ui/public/ui_logic_bg.wasm
echo "── wasm-pack build --target web --no-opt ──"
wasm-pack build --target web --no-opt
if [ -n "$WASM_OPT" ]; then
  echo "── wasm-opt -Oz (binaryen $("$WASM_OPT" --version 2>/dev/null | sed -nE 's/.*version ([0-9]+).*/\1/p' | head -1)) ──"
  "$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG_SRC" -o "$BG_SRC.opt"
  mv "$BG_SRC.opt" "$BG_SRC"
else
  echo "build.sh: NO wasm-opt, so this would commit an UNOPTIMIZED artifact. The three files are" >&2
  echo "          COMMITTED, so this is the only place the cost could be caught. Fix it with:" >&2
  echo "              npm i binaryen   &&   WASM_OPT=\$PWD/node_modules/.bin/wasm-opt ./build.sh" >&2
  exit 1
fi
mkdir -p "$GLUE_DST" "$(dirname "$ASSET_DST")"
cp "$GLUE_SRC" "$GLUE_DST/"
cp pkg/ui_logic.d.ts "$GLUE_DST/"
cp "$BG_SRC" "$ASSET_DST"
printf 'wasm (served)   : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$ASSET_DST")" "$(gzip -9 -c "$ASSET_DST" | wc -c)" "$ASSET_DST"
printf 'js glue         : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$GLUE_DST/ui_logic.js")" "$(gzip -9 -c "$GLUE_DST/ui_logic.js" | wc -c)" "$GLUE_DST/ui_logic.js"
