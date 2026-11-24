#!/usr/bin/env bash
# Render the device's local panel without a device and serve the pages.
#
# The panel is compiled into `summrise-device` and rendered by the running app, so a change to it can
# only be looked at on a machine that runs the binary — or after a signed release, which is the step
# that needs keys. `render_panel_for_review` writes the same pages to a directory instead; this script
# runs it and serves the result, so a panel change can be reviewed, screenshotted and diffed while it
# is still only a commit.
#
# Three pages, because they are three different screens: the panel with a console and sessions, the
# empty one, and the first run (no console, no history), which is the one an operator meets first.
#
# Usage: scripts/panel-preview.sh [port]     (default 8098; cargo must be on PATH)
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
port=${1:-8098}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

cd "$root/device"
SUMMRISE_PANEL_DIR="$dir" cargo test -p summrise-device --bin summrise-device -- --ignored \
  render_panel_for_review >/dev/null
mkdir -p "$dir/panel"
cp device/assets/* "$dir/panel/"

echo "panel preview on port $port:"
for page in panel.html panel-empty.html panel-fresh.html; do
  echo "  http://127.0.0.1:$port/$page"
done
echo "Ctrl-C stops the server and deletes the rendered pages."

( cd "$dir" && python3 -m http.server "$port" --bind 0.0.0.0 )
