#!/usr/bin/env bash
# Build the panel's Rust logic to wasm and install it where the panel and the agent read it.
#
# WHY `--no-opt`: wasm-pack optimizes with wasm-opt by default, and it fetches that binary from a
# GitHub release. On this box GitHub releases are unusable (P0 measured a 20 MB asset still
# crawling after 25 minutes, and wasm-pack's own download sat on a lock file for 14 minutes with
# the crate already compiled). wasm-pack 0.15.0 has NO environment variable for a system wasm-opt —
# `strings` on the binary shows the hardcoded binaryen URL — so the step is skipped here and run
# below from a wasm-opt that is already on the box.
#
# WHY THE TWO wasm-opt FLAGS, UNCONDITIONALLY: they are a property of the CODE, not of the
# toolchain. P0 measured the same wasm-opt refusing the spike's module without them
# (`memory.copy operations require bulk memory operations`) and accepting its own minimal crate
# byte-identically. Whether they are needed changes with every edit, so they are always passed.
#
# WHERE THE OUTPUT GOES, and why it is copied rather than imported from `pkg/`:
#
#   pkg/panel_logic.js               -> panel-react/src/wasm/     the `--target web` glue (ESM)
#   pkg/panel_logic.d.ts             -> panel-react/src/wasm/     its types, for `tsc --noEmit`
#   pkg/panel_logic_bg.wasm          -> agent/resources/panel/    THE SERVED ASSET
#
# `agent/resources/panel/` is the panel's build-output directory (vite's outDir, which the agent
# embeds and serves by name), so the wasm lands beside panel.js and is fetched from the SAME
# directory the page was served from — `/panel/panel_logic_bg.wasm` or
# `/desktop/panel_logic_bg.wasm`, derived at runtime from panel.js's own URL. The name is the
# glue's own default, so the glue's no-argument `init()` resolves correctly too.
#
# ALL THREE FILES ARE COMMITTED. A checkout has no wasm-pack, and both CI jobs (`panel`, `agent`)
# build from the committed artifact — the same arrangement `resources/panel/panel.js` already has,
# and `agent/build.rs` is what refuses a build whose products are older than their sources.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
GLUE_SRC=pkg/panel_logic.js
BG_SRC=pkg/panel_logic_bg.wasm
GLUE_DST=../panel-react/src/wasm
ASSET_DST=../panel/panel_logic_bg.wasm

echo "── wasm-pack build --target web --no-opt ──"
wasm-pack build --target web --no-opt

if [ -n "$WASM_OPT" ]; then
  echo "── wasm-opt -Oz (binaryen 132) ──"
  "$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG_SRC" -o "$BG_SRC.opt"
  mv "$BG_SRC.opt" "$BG_SRC"
else
  echo "wasm-opt not found — set WASM_OPT=/path/to/wasm-opt (npm i binaryen gives one)" >&2
fi

mkdir -p "$GLUE_DST" "$(dirname "$ASSET_DST")"
cp "$GLUE_SRC" "$GLUE_DST/"
cp pkg/panel_logic.d.ts "$GLUE_DST/"
cp "$BG_SRC" "$ASSET_DST"

printf 'wasm (served)   : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$ASSET_DST")" "$(gzip -9 -c "$ASSET_DST" | wc -c)" "$ASSET_DST"
printf 'js glue         : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$GLUE_DST/panel_logic.js")" "$(gzip -9 -c "$GLUE_DST/panel_logic.js" | wc -c)" "$GLUE_DST/panel_logic.js"
