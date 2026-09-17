#!/usr/bin/env bash
# panel-design-ci.bash — run the PANEL DESIGN SWEEP in CI, against the repository's own build.
#
# WHY THIS EXISTS (round 213). The design suite has only ever run when the loop remembered to run it, on a
# DEVICE, against DELIVERED copies of the harness and the panel. Rounds 184-212 between them found: a console
# bundle two generations old, a harness carrying a change that had been reverted, a probe reading a snapshot a
# day stale, and a guard that could not be satisfied after a rebuild. Every one of those is a property of the
# delivery pipeline rather than of the product, and every one was invisible until something measured it.
#
# Run HERE, the sweep measures what the repository builds, from the repository's own stylesheet, with nothing
# delivered in between — so the numbers describe the code under review rather than a copy of it.
#
# WHAT IT NEEDS: a browser. The job installs one with `npx playwright install --with-deps chromium`; on a
# machine whose cache already holds a different build than playwright-core resolves, VALE_CHROMIUM_PATH points
# at it (see the helper). Everything else — the harness, the probe, the judge — comes from this checkout.
set -euo pipefail
cd "$(dirname "$0")/../.."

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

HELPER="$PWD/agent/resources/panel-react/scripts/local-browser.mjs"
HARNESS_SRC="/tmp/panel-render-audit/panel-harness.html"

echo "── design sweep: generating the harness from this checkout ──"
# EXIT 2 IS THIS EMITTER'S SUCCESS CODE in emit mode — it prints its usage and the harness path and exits 2,
# which `set -e` reads as a failure. The fixture gate asserts exactly that distinction ("exited 1, not 2 — it
# did not finish"), so the runner has to know it too. Round 213's first CI run died here.
node agent/scripts/panel-render-audit.mjs >/dev/null || [ $? -eq 2 ]
if [ ! -s "$HARNESS_SRC" ]; then
  echo "FAIL: the harness was not written to $HARNESS_SRC" >&2
  exit 1
fi
cp "$HARNESS_SRC" "$TMP/panel-harness.html"
grep -o 'vale-harness-build" content="[^"]*"' "$TMP/panel-harness.html" || true

echo "── emitting the sweep (all passes) ──"
node agent/scripts/panel-design-sweep.mjs --emit --passes=all > "$TMP/sweep.js"
node --check "$TMP/sweep.js"

# The sweep takes its paths and its browser from the environment, so it needs no knowledge of where it runs.
echo "── running ──"
VALE_PANEL_HARNESS="$TMP/panel-harness.html" \
VALE_SWEEP_REPORT="$TMP/design-sweep.json" \
VALE_BROWSER_HELPER="$HELPER" \
  node "$TMP/sweep.js"

if [ ! -s "$TMP/design-sweep.json" ]; then
  echo "FAIL: the sweep wrote no report — it did not finish, and a run that did not finish is not a pass" >&2
  exit 1
fi

# The judge is the verdict, and it reads the report the sweep just wrote. A stale harness and a partial pass
# set are both findings, so neither can pass as a clean run.
echo "── judging ──"
node agent/scripts/panel-design-sweep.mjs --judge "$TMP/design-sweep.json"
