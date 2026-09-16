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
    r["reflow"][0]["docScrollsSideways"] = True
    r["reflow"][0]["docScrollWidth"] = 514
elif which == "focus":
    r["focus"] = [{"density": "panel", "theme": "light", "missing": 3}]
else:
    raise SystemExit("unknown axis " + which)
json.dump(r, open(dst, "w"))
PY
}
for axis in contrast h1 skip landmark geometry clipping sliver name title-only reflow focus; do
  plant "$axis" "$axis"
  if node "$TOOL" --judge "$TMP/$axis.json" > "$TMP/$axis.out" 2>&1; then
    bad "the judge PASSED a report with a planted '$axis' defect"
  else
    ok "the judge fails a planted '$axis' defect"
  fi
done

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
for helper in "const SURFACE" "const NAMES" "const SHIM" "\"body\" + ' *'"; do
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

echo "panel-design-sweep: $PASS ok, $FAILED failed"
[ "$FAILED" -eq 0 ]
