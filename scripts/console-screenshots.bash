#!/usr/bin/env bash
# Render every console page and screenshot it, light and dark, into one directory with a gallery.
#
# The pages are server-rendered, so a reviewer needs the markup and a browser. The markup comes from
# the console's own renderer (`render_pages_for_review`, an ignored test: it is a tool for the eyes,
# not a gate), and the browser is whatever Chromium this machine has.
#
#   scripts/console-screenshots.bash --out /tmp/console-shots
#   scripts/console-screenshots.bash --out /tmp/console-shots --width 900 --dark-only
#
# The gallery is `<out>/index.html`; open it and every page is one click away. Screenshots are named
# `<page>.png` and `<page>-dark.png`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="/tmp/console-shots"
width=1280
height=900
dark_only=0
light_only=0

while [ $# -gt 0 ]; do
    case "$1" in
        --out) out="$2"; shift 2 ;;
        --width) width="$2"; shift 2 ;;
        --height) height="$2"; shift 2 ;;
        --dark-only) dark_only=1; shift ;;
        --light-only) light_only=1; shift ;;
        -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "console-screenshots: unknown argument $1" >&2; exit 2 ;;
    esac
done

browser=""
for candidate in google-chrome google-chrome-stable chromium chromium-browser; do
    if command -v "$candidate" >/dev/null 2>&1; then browser="$candidate"; break; fi
done
if [ -z "$browser" ]; then
    echo "console-screenshots: no Chromium found (google-chrome, chromium)" >&2
    exit 2
fi

pages="$out/pages"
mkdir -p "$pages"
export SUMMRISE_PAGE_DIR="$pages"

echo "console-screenshots: rendering the pages with the console's own renderer"
SUMMRISE_PAGE_DIR="$pages" cargo test --quiet \
    --manifest-path "$root/device/Cargo.toml" \
    -p summrise-console --lib -- --ignored render_pages_for_review >/dev/null

# The dark theme is a `prefers-color-scheme` media query, which headless Chrome does not switch on
# demand, so the same declarations are re-emitted as plain CSS for the dark pass.
dark_css="$out/dark.css"
python3 - "$root/device/console/src/http/page.rs" "$dark_css" <<'PY'
import sys

source = open(sys.argv[1]).read()
marker = "@media (prefers-color-scheme: dark)"
at = source.find(marker)
if at < 0:
    sys.exit("console-screenshots: the dark theme block was not found")
depth, start = 0, source.index("{", at)
end = start
for index in range(start, len(source)):
    if source[index] == "{":
        depth += 1
    elif source[index] == "}":
        depth -= 1
        if depth == 0:
            end = index
            break
block = source[start + 1:end]
root = block[block.index("{") + 1:block.index("}")]
open(sys.argv[2], "w").write(":root {" + root + "}\n")
PY

names=()
for html in "$pages"/*.html; do
    name="$(basename "$html" .html)"
    names+=("$name")
    if [ "$dark_only" -eq 0 ]; then
        "$browser" --headless --disable-gpu --no-sandbox --hide-scrollbars \
            --window-size="$width,$height" \
            --screenshot="$out/$name.png" "file://$html" >/dev/null 2>&1 || true
    fi
    if [ "$light_only" -eq 0 ]; then
        mkdir -p "$out/dark-pages"
        python3 - "$html" "$dark_css" "$out/dark-pages/$name.html" <<'PY'
import sys
html = open(sys.argv[1]).read()
dark = open(sys.argv[2]).read()
open(sys.argv[3], "w").write(html.replace("</head>", f"<style>{dark}</style></head>", 1))
PY
        "$browser" --headless --disable-gpu --no-sandbox --hide-scrollbars \
            --window-size="$width,$height" \
            --screenshot="$out/$name-dark.png" "file://$out/dark-pages/$name.html" >/dev/null 2>&1 || true
    fi
done

# A gallery, so a reviewer does not need a file manager.
{
    echo '<!doctype html><meta charset="utf-8"><title>Console pages</title>'
    echo '<style>body{font:14px/1.5 system-ui;margin:24px;background:#f6f6f7;color:#18181b}'
    echo 'h1{font-size:18px}figure{margin:0 0 28px}figcaption{margin:0 0 6px;font-weight:600}'
    echo 'img{max-width:100%;border:1px solid #e6e6e9;border-radius:10px;background:#fff}</style>'
    echo "<h1>Console pages — ${#names[@]} rendered, ${width}px wide</h1>"
    for name in "${names[@]}"; do
        echo "<figure><figcaption>$name</figcaption>"
        [ -f "$out/$name.png" ] && echo "<img src=\"$name.png\" alt=\"$name, light\">"
        [ -f "$out/$name-dark.png" ] && echo "<img src=\"$name-dark.png\" alt=\"$name, dark\">"
        echo "</figure>"
    done
} > "$out/index.html"

echo "console-screenshots: ${#names[@]} pages in $out (open $out/index.html)"
