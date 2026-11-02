#!/usr/bin/env bash
# Build the console's React page and, with --check, fail when the committed page is stale.
#
# The page is embedded in the console binary with include_str!, so a source change that does not
# update device/console/web/dist/index.html still compiles and still ships yesterday's UI. This is
# the gate CI runs after npm ci/typecheck/test; run it from the repository root.
set -euo pipefail
cd "$(dirname "$0")/.."
cd device/console/web
# THE WASM LOGIC IS PART OF THIS PAGE. A source edit that did not rebuild it is a stale artifact,
# and this check needs only Node, so it belongs here rather than in a Rust-only job.
node ../../../scripts/check-console-logic.mjs
npm run build
if [[ "${1:-}" == "--check" ]]; then
    git -C ../.. diff --exit-code -- device/console/web/dist/index.html
fi
