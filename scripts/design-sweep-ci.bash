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
# EVERY UI THIS REPOSITORY SERVES, plus the landing. The panel renders a generated HARNESS; the console serves its own
# built files; the landing is RENDERED from the worker's own PAGE(). Each sweep takes its paths from the environment, so
# none of them knows where it is running. (The extension was a fourth arm until round 243 removed it.)
#
# ── WHAT THIS COSTS, MEASURED 2026-09-26 (run 36234166849, job 108382804074) ─────────────────────────────────────
#
# The `design` job is the LONG POLE of this repository's CI, and a push guard refuses a push while any run is in
# flight — so this step's duration IS the loop's push rate. The job is 823s of a ~14-minute run; the next longest job
# in that same run is pack-chain at 389s, so nothing else is close. Split by the timestamps the CI log already carries:
#
#     steps 1-8 (checkout, deps, browsers, assembler)   ~76s
#     step 9  · THIS SCRIPT                             747s
#       ├─ the panel sweep, `--passes=all`              464s
#       ├─ the console sweep                            241s
#       ├─ the landing sweep                             42s
#       └─ judging all three reports                    0.2s
#
# THREE browser-driven sweeps run in sequence (the extension was the fourth until round 243 removed it), and the panel's
# 464s is the half worth attacking first. `--passes=all` is FIVE passes — `pages · hover · motion · reflow · unstyled` —
# and `panel-design-sweep.mjs`'s own header names the cost: `pages` is "the heavy one" (1356 rows, 48 surfaces) while
# `unstyled` "completes in seconds".
#
# ── AND THE QUESTION THIS BLOCK USED TO ASK IS ANSWERED: THEY ARE NOT DUPLICATES ──────────────────────────────────
#
# It asked whether `hover`, `motion` and `reflow` are measured TWICE, because each has a standalone gate elsewhere in CI
# (`press-anchor-check`, `motion-check`, `feedback-check`) — and if they were, this step could run `pages,unstyled` and
# give back minutes. **They are not, and the difference is the one thing those gates cannot do: RENDER A PAGE.**
#
#   * `press-anchor-check` pins `pressDelta` as a PURE FUNCTION and asserts the emitted artifact reads the hovered
#     snapshot before `mouse.down()`. It renders nothing. The `hover` pass measures hover-state CONTRAST on ~31
#     interactive elements in a real browser. A wiring assertion is not a measurement of what the wiring found.
#   * `motion-check` READS BOTH SHEETS and asserts every selector that runs an animation is named in a
#     `prefers-reduced-motion` block. The `motion` pass renders with the preference EMULATED and asks the page what
#     still animates — which the gate's own header says a sheet cannot answer ("a media query adds no specificity, so
#     the answer depends on cascade order, selector scope and xterm's runtime-injected sheet"). Its "verified on the
#     rendered panel too" was a ONE-OFF measurement, not a CI job.
#   * `feedback-check` is about hover implying press, layout properties in transitions and the duration budget — ALL read
#     from sheets. It is not a reflow check at all, so the third pairing above was simply wrong: **the reflow axis has
#     no standalone counterpart anywhere in CI.** The two gates that do mention reflow (`landing-check`,
#     `spacing-scale-check`) check that a width is CAPPED in the source; whether the rendered page scrolls sideways at
#     320px is measured only here.
#
# The measurement behind that: NO gate in `scripts/test/` LAUNCHES a browser. The command, because the word alone is
# misleading — three files there mention it (a fixture's own text, the playwright BUNDLE's filename, a route path):
#
#     grep -rnE "require\(['\"]playwright|from ['\"]playwright-core|acquireBrowser|local-browser" scripts/test/
#     → exit 1, no matches
#
# So dropping these three passes would be a WEAKER gate, not a shorter one. The paragraph is kept as the record of a
# premise that looked reasonable and was false.
#
# ── AND THE COST IS NOT WHERE THE QUESTION EXPECTED IT (measured 2026-09-26, run 36237314418) ────────────────────
#
# The payload printed its split on the very next run, and it closes the question twice over:
#
#     pass pages     +439204ms   ← `pages` AND the focus/press/idle/targets/ack axes inside its loop
#     pass unstyled  +442791ms   →    3.6s
#     pass hover     +457637ms   →   14.8s
#     pass motion    +463848ms   →    6.2s
#     pass reflow    +467224ms   →    3.4s
#
# **THE FOUR CLEANLY DELIMITED PASSES ARE 28 OF THE SWEEP'S 467 SECONDS.** So running `pages,unstyled` — the change the
# paragraph above proposed — would have given back about **24 seconds of an 831-second job (2.9%)**, while removing the
# rendered hover, motion and reflow coverage that nothing else in CI provides. "Give back minutes" was wrong by an order
# of magnitude, and that is worth having on the record before anyone re-opens it.
#
# **THE COST IS THE `pages` BODY: 439s, 94% of the sweep, and it prints NOTHING for 7m20s** (10:58:15 → 11:05:35 in the
# same log — the first output after "emitting and running" is the `pass pages` mark itself). That is where a shorter
# design job has to be found, and the next instrument is marks INSIDE the density/theme/mode loop, one per iteration, so
# focus, press, idle and the pages probes can be told apart.
#
# The per-pass marks stay: they are what turned a 464-second silence into this table.
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
