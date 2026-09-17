#!/usr/bin/env bash
# panel-design-sweep.bash — the design sweep tool must EMIT a valid script and JUDGE correctly.
#
# WHY THIS EXISTS (round 50). Rounds 36-50 built one measurement axis at a time, each in an ad-hoc
# browser script that was thrown away afterwards: contrast, hierarchy, landmarks, focus states,
# geometry at three widths, WCAG reflow, accessible names, boot and interaction timing. The findings
# became tests, but the MEASUREMENT was not reproducible without rewriting it. `panel-design-sweep.mjs`
# now packages all of it — and a tool that packages measurements has to be checked itself, in CI,
# where there is no browser:
#
#   1. `--emit` produces a script that PARSES (it is assembled from template literals, which is how
#      this project has three times shipped a stub that was silently cut short);
#   2. `--judge` passes a clean report;
#   3. `--judge` FAILS a report with a planted defect — for every axis, so a branch of the judge
#      cannot rot unnoticed. A judge that says OK to everything is worse than no judge.
set -euo pipefail
cd "$(dirname "$0")/../.."

TOOL=agent/scripts/panel-design-sweep.mjs
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAILED=0
ok() { PASS=$((PASS + 1)); echo "  ok: $1"; }
bad() { FAILED=$((FAILED + 1)); echo "  FAIL: $1" >&2; }

# ── 1. emit ───────────────────────────────────────────────────────────────────────────────────
if node "$TOOL" --emit > "$TMP/sweep.js" 2>"$TMP/emit.err"; then
  ok "--emit exits 0"
else
  bad "--emit exited non-zero: $(head -3 "$TMP/emit.err")"
fi
if [ -s "$TMP/sweep.js" ]; then
  ok "--emit wrote a non-empty script ($(wc -c < "$TMP/sweep.js") bytes)"
else
  bad "--emit wrote nothing"
fi
# `node --check` parses without executing: exactly the failure mode of a truncated template literal.
if node --check "$TMP/sweep.js" 2>"$TMP/check.err"; then
  ok "the emitted script parses"
else
  bad "the emitted script does not parse: $(head -3 "$TMP/check.err")"
fi
# The probe must be IN the emitted script, not merely referenced: a sweep that measures nothing
# because its probe is missing is the failure this whole family of checks exists to catch.
if grep -q "compositeStack" "$TMP/sweep.js" && grep -q "failures" "$TOOL"; then
  ok "the emitted script embeds the shared probe"
else
  bad "the emitted script does not embed the probe"
fi

# ── 2. a clean report passes ──────────────────────────────────────────────────────────────────
cat > "$TMP/clean.json" <<'JSON'
{
  "passes": "all",
  "rows": [
    {"cr": 7.03, "need": 4.5, "size": 11, "sel": "span.ok", "text": "hello", "density": "panel", "theme": "light", "page": "Terminal"}
  ],
  "surfaces": [
    {"density": "panel", "theme": "light", "page": "Terminal", "h1Count": 1, "firstIsH1": true, "skipped": 0, "mains": 1, "navs": 1, "over": [], "clipped": [], "slivers": []}
  ],
  "names": [
    {"density": "panel", "theme": "light", "page": "Terminal", "checked": 12, "unnamed": [], "titleOnly": []}
  ],
  "reflow": [
    {"width": 320, "docScrollWidth": 320, "viewport": 320, "docScrollsSideways": false, "sideScrollers": ["div.tabs 100<300"]}
  ],
  "timing": [],
  "focus": []
}
JSON
if node "$TOOL" --judge "$TMP/clean.json" > "$TMP/clean.out" 2>&1; then
  ok "a clean report passes"
else
  bad "a clean report was rejected: $(tail -3 "$TMP/clean.out")"
fi

# ── 3. every axis can FAIL ────────────────────────────────────────────────────────────────────
# One planted defect per axis, each in its own report, so no branch of the judge can rot unnoticed.
plant() { # plant <name> <jq-ish python edit>
  python3 - "$TMP/clean.json" "$TMP/$1.json" "$2" <<'PY'
import json, sys
src, dst, which = sys.argv[1], sys.argv[2], sys.argv[3]
r = json.load(open(src))
if which == "contrast":
    r["rows"][0]["cr"] = 2.5
elif which == "h1":
    r["surfaces"][0]["h1Count"] = 2
elif which == "skip":
    r["surfaces"][0]["skipped"] = 1
elif which == "landmark":
    r["surfaces"][0]["mains"] = 0
elif which == "geometry":
    r["surfaces"][0]["over"] = ["div.thing 100<200"]
elif which == "clipping":
    r["surfaces"][0]["clipped"] = ["span.label"]
elif which == "sliver":
    r["surfaces"][0]["slivers"] = ["span.goal w=30"]
elif which == "name":
    r["names"][0]["unnamed"] = ["input.mem-input"]
elif which == "title-only":
    r["names"][0]["titleOnly"] = ["button.search"]
elif which == "reflow":
    # NOT a tab scroller: the panel's exemption sets aside the 320px scroll only when every offending
    # scroller IS a tab child, so a genuine one must still fail. (Planting it on the fixture's tab
    # strip proved nothing — that is the artifact itself.)
    r["reflow"][0]["docScrollsSideways"] = True
    r["reflow"][0]["docScrollWidth"] = 514
    r["reflow"][0]["sideScrollers"] = ["div.card 100<300"]
elif which == "focus":
    r["focus"] = [{"density": "panel", "theme": "light", "missing": 3}]
elif which == "theme-lie":
    # A REPORT THAT DESCRIBES A PAGE IT DID NOT RENDER: navigated as dark, rendered light. This is exactly
    # what round 175 shipped by accident, and the check reads the theme off the PAGE to catch it.
    r["themeChecks"] = [{"page": "Terminal-fail-dark", "intended": "dark", "stored": "light", "attr": "", "bodyBackground": "rgb(250, 250, 250)"}]
elif which == "harness-stale":
    # A FIXTURE OLDER THAN THE BUILD INVALIDATES EVERY MEASUREMENT IN THE REPORT (round 189: a delivered
    # harness two generations old collapsed the tab strip to 17px and the sweep called it a live defect).
    r["harnessBuild"] = "100000-deadbeef0000"
    r["expectedHarnessBuild"] = "212274-0517495785a2"
    r["harnessStale"] = True
elif which == "paint-drift":
    # THE PIXELS OVERRULING THE STYLE CHECK must be VISIBLE, not silent. It was 18 of 18 in round 186 while
    # the style verdict called every console ring missing; the count is a note rather than a finding, so the
    # gate asserts the note is printed and carries both numbers.
    r["focus"] = [{"page": "Terminal", "pressed": 16, "landed": 16, "escaped": 0, "missing": 0, "paintConfirmed": 4}]
elif which == "blind":
    # A REPORT THAT COULD NOT MEASURE MOST OF ITS ROWS. Every entry with cr None is excluded from judgement,
    # so without a floor this exits 0 while having judged almost nothing.
    for row in r["rows"]:
        row["cr"] = None
elif which == "type-floor":
    # A ROW RENDERED BELOW THE SCALE'S FLOOR. designScale.test.ts pins the tokens; this is the claim that
    # the RENDERED page honours them, and a judge that could not fail here would not be checking it.
    r["rows"][0]["size"] = 9
elif which == "motion":
    # AN ELEMENT THAT STILL ANIMATES under the preference: the defect this pass exists for.
    r["motion"] = [{"density": "panel", "normal": 19, "reduced": 3, "stillAnimating": [".btn fade x1", ".chip slide x1", ".tab scale x1"]}]
elif which == "motion-empty":
    # NOTHING ANIMATED WITHOUT THE PREFERENCE EITHER: reduced==0 is then vacuous, not a pass.
    r["motion"] = [{"density": "panel", "normal": 0, "reduced": 0, "stillAnimating": []}]
elif which == "focus-empty":
    # A RUN THAT LANDED NOWHERE: 14 presses, every one escaping to the body. Clean by the old rule
    # (missing == 0) and must still be a finding.
    r["focus"] = [{"density": "panel", "theme": "light", "pressed": 14, "landed": 0, "escaped": 14, "missing": 0}]
else:
    raise SystemExit("unknown axis " + which)
json.dump(r, open(dst, "w"))
PY
}
for axis in contrast h1 skip landmark geometry sliver name title-only reflow focus focus-empty motion motion-empty type-floor blind theme-lie harness-stale; do
  plant "$axis" "$axis"
  if node "$TOOL" --judge "$TMP/$axis.json" > "$TMP/$axis.out" 2>&1; then
    bad "the judge PASSED a report with a planted '$axis' defect"
  else
    ok "the judge fails a planted '$axis' defect"
  fi
done

# A NOTE MUST BE PRINTED, WHICH THE LOOP ABOVE CANNOT CHECK — it only asserts that planted defects FAIL.
# The paint-confirmed count is deliberately NOT a finding (the pixels are the authority and they said the ring
# is painted), so a silent regression here would hide the one number that made round 186's six-round detour
# possible. Assert the note, and that it carries BOTH numbers.
plant paint-drift paint-drift
if node "$TOOL" --judge "$TMP/paint-drift.json" > "$TMP/paint-drift.out" 2>&1; then
  ok "a report of pixels overruling the style check still passes"
else
  bad "the judge FAILED a report whose only oddity is paint-confirmed focus verdicts"
fi
if grep -q "overruled the computed-style focus verdict 4 time(s) of 16 press(es)" "$TMP/paint-drift.out"; then
  ok "and the judge prints how often the pixels overruled it, with both numbers"
else
  bad "the drift note is missing or incomplete: $(grep -c overruled "$TMP/paint-drift.out") match(es)"
fi

# ── 4. the CONSOLE sweep, same contract ───────────────────────────────────────────────────────
# Its emitted script referenced helpers it never defined when it was first written (the placeholder
# was in the .replace() call and not in the template), which PARSES and cannot run — so presence is
# checked explicitly, not just syntax.
CONSOLE=agent/scripts/console-design-sweep.mjs
if node "$CONSOLE" --emit > "$TMP/csweep.js" 2>"$TMP/csweep.err" && node --check "$TMP/csweep.js"; then
  ok "console --emit writes a script that parses"
else
  bad "console --emit failed: $(head -3 "$TMP/csweep.err")"
fi
for helper in "const SURFACE" "const NAMES" "const API" "const PAGES"; do
  if grep -qF "$helper" "$TMP/csweep.js"; then
    ok "the console sweep defines: $helper"
  else
    bad "the console sweep USES but does not define: $helper (it would report nothing)"
  fi
done
cat > "$TMP/console-clean.json" <<'JSON'
{
  "rows": [{"cr": 7.0, "need": 4.5, "size": 13, "sel": "span.ok", "text": "x", "page": "overview"}],
  "surfaces": [{"page": "overview", "h1Count": 1, "firstIsH1": true, "skipped": 0, "mains": 1, "navs": 1, "over": [], "clipped": [], "slivers": []}],
  "names": [{"page": "overview", "checked": 5, "unnamed": [], "titleOnly": []}]
}
JSON
if node "$CONSOLE" --judge "$TMP/console-clean.json" >/dev/null 2>&1; then
  ok "a clean console report passes"
else
  bad "a clean console report was rejected"
fi
for axis in contrast name geometry focus; do
  python3 - "$TMP/console-clean.json" "$TMP/console-$axis.json" "$axis" <<'PY2'
import json, sys
src, dst, which = sys.argv[1], sys.argv[2], sys.argv[3]
r = json.load(open(src))
if which == "contrast": r["rows"][0]["cr"] = 2.1
elif which == "name": r["names"][0]["unnamed"] = ["input.form-input"]
elif which == "geometry": r["surfaces"][0]["over"] = ["div.card 100<200"]
elif which == "focus": r["focus"] = [{"page": "devices", "missing": 2}]
json.dump(r, open(dst, "w"))
PY2
  if node "$CONSOLE" --judge "$TMP/console-$axis.json" >/dev/null 2>&1; then
    bad "the console judge PASSED a report with a planted '$axis' defect"
  else
    ok "the console judge fails a planted '$axis' defect"
  fi
done

# ── 5. the EXTENSION sweep (third adapter of the same core) ───────────────────────────────────
EXT=agent/scripts/extension-design-sweep.mjs
if node "$EXT" --emit > "$TMP/esweep.js" 2>"$TMP/esweep.err" && [ -s "$TMP/esweep.js" ] && node --check "$TMP/esweep.js"; then
  ok "extension --emit writes a script that parses"
else
  bad "extension --emit failed: $(head -3 "$TMP/esweep.err")"
fi
# the selector is INLINED by pageChecks() rather than referenced, so the check is for the
# substituted form — asserting the identifier would pass while the page-side reference broke
# (which is exactly how the first version of the shared core shipped broken).
# the storage shim became a FUNCTION (so the fresh-install pass can flip it), hence "const shim"
# and the storage flag rather than the old SHIM constant.
for helper in "const SURFACE" "const NAMES" "const shim" "storage.empty" "\"body\" + ' *'"; do
  if grep -qF "$helper" "$TMP/esweep.js"; then
    ok "the extension sweep defines: $helper"
  else
    bad "the extension sweep USES but does not define: $helper"
  fi
done
cat > "$TMP/ext-clean.json" <<'JSON'
{
  "rows": [{"cr": 16.4, "need": 4.5, "size": 17, "sel": "h1", "text": "Vale Code Links", "page": "options", "width": 900}],
  "surfaces": [{"page": "options", "width": 900, "h1Count": 1, "firstIsH1": true, "skipped": 0, "mains": 1, "navs": 0, "over": [], "clipped": [], "slivers": []}],
  "names": [{"page": "options", "checked": 4, "unnamed": [], "titleOnly": []}]
}
JSON
if node "$EXT" --judge "$TMP/ext-clean.json" >/dev/null 2>&1; then
  ok "a clean extension report passes (no nav is correct for an options page)"
else
  bad "a clean extension report was rejected"
fi
python3 - "$TMP/ext-clean.json" "$TMP/ext-two-h1.json" <<'PY2'
import json, sys
r = json.load(open(sys.argv[1]))
r["surfaces"][0]["h1Count"] = 2
json.dump(r, open(sys.argv[2], "w"))
PY2
if node "$EXT" --judge "$TMP/ext-two-h1.json" >/dev/null 2>&1; then
  bad "the extension judge PASSED a two-h1 report — the exact defect it was written for"
else
  ok "the extension judge fails a two-h1 report"
fi

# …and the exemption itself is narrow: the SAME defect reported from a tab scroller is set aside,
# because that is the harness artifact the panel adapter documents.
python3 - "$TMP/clean.json" "$TMP/reflow-artifact.json" <<'PY3'
import json, sys
r = json.load(open(sys.argv[1]))
r["reflow"][0]["docScrollsSideways"] = True
r["reflow"][0]["docScrollWidth"] = 514
json.dump(r, open(sys.argv[2], "w"))
PY3
if node "$TOOL" --judge "$TMP/reflow-artifact.json" > "$TMP/reflow-artifact.out" 2>&1; then
  if grep -q "set aside" "$TMP/reflow-artifact.out"; then
    ok "the tab-scroller 320px artifact is set aside WITH its reason printed"
  else
    bad "the artifact was suppressed silently — an exemption nobody can see is a hidden failure"
  fi
else
  bad "the tab-scroller 320px artifact was treated as a defect (the exemption is too narrow now)"
fi

# REDUCED MOTION (round 77): a report showing anything still animating under the preference must
# fail. Planted rather than assumed — the rule lives in the shared judge, so a report is the only
# input needed to prove it bites.
python3 - "$TMP/clean.json" "$TMP/motion.json" <<'PY3'
import json, sys
r = json.load(open(sys.argv[1]))
r["motion"] = [{"density": "desktop", "animating": [".dtab trans=0.12s", ".xterm-cursor.xterm-cursor-blink anim=blink_block_1 xinfinite"]}]
json.dump(r, open(sys.argv[2], "w"))
PY3
if node "$TOOL" --judge "$TMP/motion.json" > "$TMP/motion.out" 2>&1; then
  bad "the judge PASSED a report with elements still animating under prefers-reduced-motion"
else
  ok "the judge fails a report that still animates under reduced motion"
fi

# HOVER (round 84): a report showing an element below AA WHILE HOVERED must fail. Hover was the last
# unmeasured state — no instrument looked at it before this round.
python3 - "$TMP/clean.json" "$TMP/hover.json" <<'PY3'
import json, sys
r = json.load(open(sys.argv[1]))
r["hover"] = [{"density": "panel", "theme": "light", "interactive": 31, "underAA": ['.btn-ghost "Cancel" 3.1<4.5']}]
json.dump(r, open(sys.argv[2], "w"))
PY3
if node "$TOOL" --judge "$TMP/hover.json" > "$TMP/hover.out" 2>&1; then
  bad "the judge PASSED a report with an element below AA while hovered"
else
  ok "the judge fails a report whose hovered element is below AA"
fi

# UNSTYLED CLASSES (rounds 88-89): the mirror of dead CSS. A report showing a class the page renders
# that no rule matches must fail — the failure a too-eager prune causes. And a report whose collector
# read almost no stylesheets must fail too: round 88's version reported 38 styled classes where the
# browser sees 221, and its silence looked exactly like a clean page.
python3 - "$TMP/clean.json" "$TMP/unstyled.json" "$TMP/thin.json" <<'PY3'
import json, sys
r = json.load(open(sys.argv[1]))
r["unstyled"] = [{"page": "devices", "styledClasses": 221, "classes": ["regkeys-clear"]}]
json.dump(r, open(sys.argv[2], "w"))
thin = json.load(open(sys.argv[1]))
thin["unstyled"] = [{"page": "devices", "styledClasses": 38, "classes": []}]
json.dump(thin, open(sys.argv[3], "w"))
PY3
if node "$TOOL" --judge "$TMP/unstyled.json" > "$TMP/unstyled.out" 2>&1; then
  bad "the judge PASSED a report with a class no rule styles"
else
  ok "the judge fails a report with an unstyled class"
fi
if node "$TOOL" --judge "$TMP/thin.json" > "$TMP/thin.out" 2>&1; then
  bad "the judge PASSED a report whose collector read almost nothing"
else
  ok "the judge fails an unstyled report that read almost no stylesheets"
fi

# PARTIAL REPORTS (round 91). The sweep outgrew its caller's timeout, so it runs in passes — and a
# partial run reporting "nothing found" would read exactly like a clean full one. Both directions are
# pinned: a partial report must FAIL against the default expectation, and must PASS when the caller
# declares that it only asked for those passes.
python3 - "$TMP/clean.json" "$TMP/partial.json" <<'PY3'
import json, sys
r = json.load(open(sys.argv[1]))
r["passes"] = "pages,hover"
json.dump(r, open(sys.argv[2], "w"))
PY3
if node "$TOOL" --judge "$TMP/partial.json" > "$TMP/partial.out" 2>&1; then
  bad "the judge PASSED a partial report against the default expectation"
else
  ok "the judge refuses a partial report that claims nothing was wrong"
fi
if node "$TOOL" --judge "$TMP/partial.json" --expect=pages > "$TMP/partial2.out" 2>&1; then
  ok "the judge accepts a partial report the caller declared"
else
  bad "the judge refused a partial report the caller declared: $(head -c 200 "$TMP/partial2.out")"
fi

echo "panel-design-sweep: $PASS ok, $FAILED failed"
[ "$FAILED" -eq 0 ]
