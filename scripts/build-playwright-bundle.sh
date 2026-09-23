#!/usr/bin/env bash
# build-playwright-bundle.sh — produce summrise-playwright.zip, the boxed browser-tools bundle.
#
# WHY THIS EXISTS. The bundle is one of the three components `summrise setup` fetches, it is pinned
# by sha256 in index/components.json, and NOTHING IN THIS REPO BUILT IT — which is how its route came
# to answer 502 for a day after the rename with nobody to notice, and why the pin file had to say
# "a real producer script is still owed". This is that script. It runs on Linux; the device is not
# needed, and the versions below are the ones MEASURED in the working bundle on 2026-09-23.
#
# OUTPUT: a zip whose ROOT is the `playwright` folder, because `summrise setup` expands it with
# `Expand-Archive -DestinationPath <install>\components` and then looks for
# `components\playwright\node.exe`. Getting that root wrong stages a bundle the CLI cannot find.
#
# AFTER RUNNING: stage it and move the pin, both, or every fresh install fails closed (which is the
# point of the pin, but not of an afternoon):
#   wrangler r2 object put summrise-temp-files/summrise-playwright.zip --file=<out> --remote
#   # then update playwright.sha256 in index/components.json IN THE SAME COMMIT
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NODE_VER="${NODE_VER:-v20.18.0}"                        # the runtime the CLI spawns on Windows
MCP_VER="${MCP_VER:-0.0.79}"                            # @playwright/mcp
CORE_VER="${CORE_VER:-1.63.0-alpha-2026-08-05}"         # playwright-core, pinned beside it
OUT="${OUT:-$ROOT/index/public/summrise-agent/summrise-playwright.zip}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
B="$WORK/playwright"
mkdir -p "$B"

echo "== node $NODE_VER (win-x64): the runtime the CLI spawns on Windows =="
curl -fsSL -o "$WORK/node.zip" "https://nodejs.org/dist/$NODE_VER/node-$NODE_VER-win-x64.zip"
unzip -o -q "$WORK/node.zip" "node-$NODE_VER-win-x64/node.exe" -d "$WORK"
cp "$WORK/node-$NODE_VER-win-x64/node.exe" "$B/node.exe"
echo "   node.exe $(stat -c %s "$B/node.exe") bytes"

echo "== the two packages and their dependencies, at the pinned versions =="
# npm resolves the tree (mcp depends on a playwright-core; the pin names the exact one), so this is
# the honest way to reproduce it rather than unpacking two tarballs and hoping nothing else is needed.
( cd "$B" && npm install --silent --no-audit --no-fund --omit=dev \
    "@playwright/mcp@$MCP_VER" "playwright-core@$CORE_VER" >/dev/null )
echo "   @playwright/mcp  $(node -p "require('$B/node_modules/@playwright/mcp/package.json').version")"
echo "   playwright-core  $(node -p "require('$B/node_modules/playwright-core/package.json').version")"

echo "== the root of the archive must be the playwright folder =="
# npm leaves a project behind (package.json + a lockfile); the boxed bundle is a RUNTIME tree, and
# the working copy on the device has neither. Drop them so the archive is what setup expects.
rm -f "$B/package.json" "$B/package-lock.json"
rm -f "$OUT"
( cd "$WORK" && zip -qr "$OUT" playwright )
echo "   wrote $OUT"
LIST="$(unzip -l "$OUT")"
echo "$LIST" | awk 'NR>3 && $4 ~ /^playwright\/[^/]*$/ {print "   root entry:", $4}'
# HERE-STRINGS, NOT `unzip … | grep -q`: under `set -o pipefail` an early grep -q exit SIGPIPEs the
# producer and the pipeline reports failure EVEN ON A MATCH — the round-288 lesson, written in
# release-audit.sh, and re-learned here the hard way (one of these two checks passed only because
# the listing happened to fit in the pipe buffer, which is the worst kind of flake).
grep -q 'playwright/node\.exe' <<< "$LIST" \
  || { echo "::error::the archive has no playwright/node.exe — setup would stage nothing" >&2; exit 1; }
grep -q 'playwright/node_modules/@playwright/mcp/package.json' <<< "$LIST" \
  || { echo "::error::the archive has no @playwright/mcp — the browser tools would not start" >&2; exit 1; }
grep -q 'playwright/node_modules/playwright-core/package.json' <<< "$LIST" \
  || { echo "::error::the archive has no playwright-core — the browser tools would not start" >&2; exit 1; }

printf '%s  %s\n' "$(sha256sum "$OUT" | cut -d' ' -f1)" "$(basename "$OUT")"
echo "NEXT: stage it in R2, then set playwright.sha256 in index/components.json to that value."
