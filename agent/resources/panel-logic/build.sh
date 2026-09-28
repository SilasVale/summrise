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
  # THE VERSION IS READ, NOT SPELLED — the line used to say "binaryen 132" as a literal, and this
  # box's `npm i binaryen` is 132 while the next one may not be. A build log that names the wrong
  # toolchain is worse than one that names none.
  echo "── wasm-opt -Oz (binaryen $("$WASM_OPT" --version 2>/dev/null | sed -nE 's/.*version ([0-9]+).*/\1/p' | head -1)) ──"
  "$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int "$BG_SRC" -o "$BG_SRC.opt"
  mv "$BG_SRC.opt" "$BG_SRC"
else
  # A MISSING OPTIMIZER IS A FAILURE, NOT A WARNING (2026-09-29), and the numbers are why. This
  # branch used to print one line to stderr and carry on, which shipped an UNOPTIMIZED artifact into
  # a COMMITTED binary — and the three files below are committed, so nothing downstream ever
  # re-derives it. MEASURED on this box, same source, same day:
  #
  #     with wasm-opt -Oz     54,700 bytes / 23,167 gzipped   <- the committed artifact, reproduced
  #     without it           109,391 bytes / 30,266 gzipped   <- +7,099 gz, +31% of the served asset
  #
  # The first build in this session did exactly that: it overwrote the committed wasm with the
  # unoptimized one, and `git status` was the only thing that said so. **A COST THAT APPEARS IN A
  # BINARY AND NOWHERE ELSE IS THE COST THIS REPOSITORY KEEPS PAYING** — the plan's own `{:.3}`
  # lesson is the same class, one formatter away from 8,879 gz. `wasm-pack` cannot fetch binaryen
  # here (its download is the 14-minute hang this file's header describes), but `npm i binaryen`
  # takes two seconds and gives a wasm-opt, which is why the message names it rather than the URL.
  echo "build.sh: NO wasm-opt, so this would commit an UNOPTIMIZED artifact — 109,391 bytes / 30,266 gz" >&2
  echo "          against the committed 54,700 / 23,167. The three files below are COMMITTED, so this is" >&2
  echo "          the only place the cost could be caught. Fix it with:" >&2
  echo "              npm i binaryen   &&   WASM_OPT=\$PWD/node_modules/.bin/wasm-opt ./build.sh" >&2
  exit 1
fi

mkdir -p "$GLUE_DST" "$(dirname "$ASSET_DST")"
cp "$GLUE_SRC" "$GLUE_DST/"
cp pkg/panel_logic.d.ts "$GLUE_DST/"
cp "$BG_SRC" "$ASSET_DST"

printf 'wasm (served)   : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$ASSET_DST")" "$(gzip -9 -c "$ASSET_DST" | wc -c)" "$ASSET_DST"
printf 'js glue         : %8s bytes / %8s gzipped   %s\n' \
  "$(stat -c%s "$GLUE_DST/panel_logic.js")" "$(gzip -9 -c "$GLUE_DST/panel_logic.js" | wc -c)" "$GLUE_DST/panel_logic.js"
