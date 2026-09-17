#!/usr/bin/env bash
# design-sweep-ci.bash — run the DESIGN SWEEPS in CI, against what this repository builds.
#
# NOT IN scripts/test/: pack-chain runs EVERY script in that directory, and these need a browser that only the
# design job installs. Round 213 put the runner there first and pack-chain failed with "Cannot find package
# 'playwright-core'" — a script that needs a browser is not an artifact gate.
#
# WHY THIS EXISTS (rounds 204, 213, 219). The design suite has only ever run when the loop remembered to run it,
# on a DEVICE, against DELIVERED copies of the harness and the UIs. Everything rounds 184-213 found was a
# property of that delivery pipeline rather than of the product: a console bundle two generations old, a harness
# carrying a change that had been reverted, a probe reading a snapshot a day stale, and a guard that could not be
# satisfied after a rebuild. Run here, the same sweeps measure what the checkout BUILDS, from the repository's
# own stylesheet and assets, with nothing delivered in between.
#
# ALL THREE UIs. The panel renders a generated HARNESS; the console and the extension serve their own built
# files. Each sweep takes its paths from the environment, so neither knows where it is running.
#
# WHAT IT NEEDS: a browser. The job installs one with the PROJECT'S playwright-core — see ci.yml for why naming
# npx's playwright installs the wrong build number.
set -euo pipefail

# ONE LEVEL UP: this script is scripts/design-sweep-ci.bash. It lived in scripts/test/ until round 213 moved it,
# and the two-level `cd` came with it — which put every path OUTSIDE the repository
# ("Cannot find module '/home/runner/work/vale/agent/scripts/panel-render-audit.mjs'"). A moved file takes its
# relative paths with it; that is what the first CI run after the move reported.
cd "$(dirname "$0")/.."

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
HELPER="$PWD/agent/resources/panel-react/scripts/local-browser.mjs"

# THE FAILURE NAMES ITSELF. Round 213 parked the job on "MODULE_NOT_FOUND resolving playwright-core" with
# `requireStack: []`, which points at the FIRST require rather than the helper's own import and left two
# candidates to guess between. So print the facts a failure would depend on, BEFORE anything that can fail: an
# earlier version of this block ran after the harness step and printed nothing when that step died.
echo "── environment ──"
echo "helper:   $HELPER"
if [ -f "$HELPER" ]; then ls -l "$HELPER" | awk '{print "          " $5 " bytes"}'; else echo "          MISSING"; fi
echo "node:     $(node --version)"
( cd "$(dirname "$HELPER")" && node -e "console.log('resolves: ' + require.resolve('playwright-core'))" ) \
  || echo "          playwright-core DOES NOT RESOLVE from $(dirname "$HELPER")"

# THE PANEL: it renders a generated harness, so the harness has to exist first.
echo "── panel: generating the harness from this checkout ──"
# EXIT 2 IS THIS EMITTER'S SUCCESS CODE in emit mode — it prints its usage and the harness path and exits 2,
# which `set -e` reads as a failure. The fixture gate asserts exactly that distinction ("exited 1, not 2 — it
# did not finish"), so the runner has to know it too. Round 213's first CI run died here.
node agent/scripts/panel-render-audit.mjs >/dev/null || [ $? -eq 2 ]
cp /tmp/panel-render-audit/panel-harness.html "$TMP/panel-harness.html"
grep -o 'vale-harness-build" content="[^"]*"' "$TMP/panel-harness.html" || true

echo "── panel: emitting and running (all passes) ──"
node agent/scripts/panel-design-sweep.mjs --emit --passes=all > "$TMP/panel.js"
node --check "$TMP/panel.js"
VALE_PANEL_HARNESS="$TMP/panel-harness.html" \
VALE_SWEEP_REPORT="$TMP/panel-report.json" \
VALE_BROWSER_HELPER="$HELPER" \
  node "$TMP/panel.js"
if [ ! -s "$TMP/panel-report.json" ]; then
  echo "FAIL: the panel sweep wrote no report — a run that did not finish is not a pass" >&2
  exit 1
fi

# THE CONSOLE AND THE EXTENSION serve their own built files from the repository, so they need no harness —
# only a root that points at what this checkout produces.
for ui in console extension; do
  case "$ui" in
    console)   root="$PWD/gateway/public" ;;
    extension) root="$PWD/extension" ;;
  esac
  echo "── $ui: root $root ──"
  node "agent/scripts/$ui-design-sweep.mjs" --emit > "$TMP/$ui.js"
  node --check "$TMP/$ui.js"
  VALE_SWEEP_ROOT="$root" \
  VALE_SWEEP_REPORT="$TMP/$ui-report.json" \
  VALE_BROWSER_HELPER="$HELPER" \
    node "$TMP/$ui.js"
  if [ ! -s "$TMP/$ui-report.json" ]; then
    echo "FAIL: the $ui sweep wrote no report" >&2
    exit 1
  fi
done

# THE JUDGE IS THE VERDICT, for each UI in turn, reading the report that sweep just wrote. A stale harness and a
# partial pass set are both findings, so neither can pass as a clean run.
echo "── judging ──"
node agent/scripts/panel-design-sweep.mjs --judge "$TMP/panel-report.json"
for ui in console extension; do
  node "agent/scripts/$ui-design-sweep.mjs" --judge "$TMP/$ui-report.json"
done

echo "── all three design sweeps ran and judged clean ──"
