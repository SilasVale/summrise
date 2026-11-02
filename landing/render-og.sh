#!/usr/bin/env bash
# Render the social card from `og.html` at the exact size the platforms crop to.
#   ./render-og.sh          # writes public/assets/og.png
#   CHROME=chromium ./render-og.sh
set -euo pipefail
cd "$(dirname "$0")"
chrome="${CHROME:-google-chrome}"
command -v "$chrome" >/dev/null || { echo "render-og: $chrome not found (set CHROME=)" >&2; exit 2; }
"$chrome" --headless=new --disable-gpu --no-sandbox --hide-scrollbars \
    --force-device-scale-factor=1 --window-size=1200,630 \
    --screenshot="$PWD/public/assets/og.png" "file://$PWD/og.html" >/dev/null 2>&1
echo "render-og: wrote public/assets/og.png"
