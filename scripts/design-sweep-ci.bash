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
# ── WHAT THIS COSTS, MEASURED 2026-09-26 (rounds 21-23 of the standing goal) ──────────────────────────────────────
#
# The `design` job is the LONG POLE of this repository's CI, and a push guard now refuses a push while any run is in
# flight — so this step's duration IS the loop's push rate. Measured from a real run (36233462701):
#
#     steps 1-8 (checkout, deps, browsers, assembler)   ~76s
#     step 9  · THIS SCRIPT                             749s and counting
#
# and inside it, FOUR browser-driven sweeps run in sequence: the panel with `--passes=all`, then the console and the
# second UI in a loop, then the landing. `--passes=all` is FIVE passes — `pages · hover · motion · reflow · unstyled` —
# and `panel-design-sweep.mjs`'s own header names the cost: `pages` is "the heavy one" (1356 rows, 48 surfaces) while
# `unstyled` "completes in seconds".
#
# **THE NEXT MEASUREMENT IS PER-PASS TIMING, AND THE QUESTION IT ANSWERS IS WHETHER THREE OF THE FIVE ARE DUPLICATES.**
# `hover`, `motion` and `reflow` each have a standalone gate that runs elsewhere in CI (`press-anchor-check`,
# `motion-check`, `feedback-check`) — so they may be measured TWICE. If they are, this step can run `pages,unstyled` and
# give back minutes, which is a shorter CI rather than a weaker gate. Nobody has measured it yet; the number above is
# where to start.
#
# WHAT IT NEEDS: a browser. The job installs one with the PROJECT'S playwright-core — see ci.yml for why naming
# npx's playwright installs the wrong build number.
set -euo pipefail

# ONE LEVEL UP: this script is scripts/design-sweep-ci.bash. It lived in scripts/test/ until round 213 moved it,
# and the two-level `cd` came with it — which put every path OUTSIDE the repository
# ("Cannot find module '/home/runner/work/summrise/agent/scripts/panel-render-audit.mjs'"). A moved file takes its
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
grep -o 'summrise-harness-build" content="[^"]*"' "$TMP/panel-harness.html" || true

echo "── panel: emitting and running (all passes) ──"
node agent/scripts/panel-design-sweep.mjs --emit --passes=all > "$TMP/panel.js"
node --check "$TMP/panel.js"
SUMMRISE_PANEL_HARNESS="$TMP/panel-harness.html" \
SUMMRISE_SWEEP_REPORT="$TMP/panel-report.json" \
SUMMRISE_BROWSER_HELPER="$HELPER" \
  node "$TMP/panel.js"
if [ ! -s "$TMP/panel-report.json" ]; then
  echo "FAIL: the panel sweep wrote no report — a run that did not finish is not a pass" >&2
  exit 1
fi

# THE CONSOLE serves its own built files from the repository, so it needs no harness — only a root that points
# at what this checkout produces. (The extension was a second arm here until round 243 removed it: it shipped
# nowhere, its default was off, and the half it existed for had already been deleted.)
for ui in console; do
  # BUILD IT, DO NOT SERVE THE PUBLISHED COPY (round 76). This arm pointed at `gateway/public`, which the RELEASE
  # flow copies a console into — so the design job was sweeping the console that was last published, not the one in
  # this checkout, in a script whose own header says it runs "against what this repository builds". Two console fixes
  # were reported as absent by CI while a fresh build of the same source measured clean (`ringFill: []`), which is
  # how the difference was found: the findings were real FOR THAT ARTEFACT and had nothing to do with the commit.
  ( cd gateway/ui && npx vite build --outDir "$TMP/console-build" --emptyOutDir ) >"$TMP/console-build.log" 2>&1 \
    || { echo "FAIL: the console did not build: $(tail -3 "$TMP/console-build.log")" >&2; exit 1; }
  root="$TMP/console-build"
  echo "── $ui: root $root (built from this checkout) ─"
  # THE EMIT MUST SEE THE SAME ROOT THE RUN SERVES (round 77). The sweep bakes the entry's size and sha when it is
  # emitted and compares them at run time — a guard for the DEVICE pipeline, where the script is emitted here and
  # delivered there. Pointing the run at a fresh build while the emit read `gateway/public` made every CI run report
  # its own entry as stale, which is a true statement about two artefacts and a useless one about a commit.
  SUMMRISE_SWEEP_ROOT="$root" \
    node "agent/scripts/$ui-design-sweep.mjs" --emit > "$TMP/$ui.js"
  node --check "$TMP/$ui.js"
  SUMMRISE_SWEEP_ROOT="$root" \
  SUMMRISE_SWEEP_REPORT="$TMP/$ui-report.json" \
  SUMMRISE_BROWSER_HELPER="$HELPER" \
    node "$TMP/$ui.js"
  if [ ! -s "$TMP/$ui-report.json" ]; then
    echo "FAIL: the $ui sweep wrote no report" >&2
    exit 1
  fi
done

# THE LANDING IS GENERATED, NOT BUILT, AND IT HAD NEVER BEEN RENDERED BY ANY GATE (round 81). `--emit` renders the page
# the WORKER serves — by calling PAGE(), the same entry point index/src/index.js serves and the index tests call — in
# BOTH installer states, into a directory this job then serves. The two states are the point: when a release publishes
# no installer the page swaps the primary button for a hint, a different element with a different contrast question,
# and only a browser can see it. Nothing is delivered in between, so the root is set for the emit AND the run.
echo "── landing: rendering the page the worker serves ──"
SUMMRISE_LANDING_OUT="$TMP/landing" node agent/scripts/landing-design-sweep.mjs --emit > "$TMP/landing.js" 2>"$TMP/landing-emit.log"
cat "$TMP/landing-emit.log"
node --check "$TMP/landing.js"
SUMMRISE_SWEEP_ROOT="$TMP/landing" \
SUMMRISE_SWEEP_REPORT="$TMP/landing-report.json" \
SUMMRISE_BROWSER_HELPER="$HELPER" \
  node "$TMP/landing.js"
if [ ! -s "$TMP/landing-report.json" ]; then
  echo "FAIL: the landing sweep wrote no report — a run that did not finish is not a pass" >&2
  exit 1
fi

# THE JUDGE IS THE VERDICT, for each UI in turn, reading the report that sweep just wrote. A stale harness and a
# partial pass set are both findings, so neither can pass as a clean run.
echo "── judging ──"
node agent/scripts/panel-design-sweep.mjs --judge "$TMP/panel-report.json"
for ui in console landing; do
  node "agent/scripts/$ui-design-sweep.mjs" --judge "$TMP/$ui-report.json"
done

echo "── all three design sweeps ran and judged clean ─"
