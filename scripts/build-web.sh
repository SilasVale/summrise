#!/usr/bin/env bash
# Build the console page — the one artifact the Rust binary embeds.
#
# Node is a BUILD dependency and nothing else: `npm ci && vite build` produces a single index.html that
# `console/src/http.rs` includes with `include_str!`, so the shipped runtime stays one executable with
# no Node and no external requests.
#
# Run this before `cargo build` whenever `web/src` changed. `--check` fails instead of writing when the
# committed artifact is older than the sources, which is the failure this script exists to prevent: a
# page that was edited in TypeScript but shipped as yesterday's JavaScript.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
web="$here/device/console/web"
dist="$web/dist/index.html"

if [[ "${1:-}" == "--check" ]]; then
    newest="$(find "$web/src" "$web/index.html" "$web/vite.config.ts" -type f -newer "$dist" -print -quit 2>/dev/null || true)"
    if [[ -n "$newest" ]]; then
        echo "build-web: $dist is older than $newest — run scripts/build-web.sh" >&2
        exit 1
    fi
    echo "build-web: $dist is current"
    exit 0
fi

cd "$web"
if [[ ! -d node_modules ]]; then
    npm ci --silent
fi
npx vite build
ls -l "$dist"
echo "build-web: $(( $(stat -c%s "$dist") / 1024 )) KB, gzipped ~$(( $(gzip -c "$dist" | wc -c) / 1024 )) KB"
