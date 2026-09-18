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

# ── 1b. THE PROBES REACH THE PAGE UNCHANGED ────────────────────────────────────────────────────
# Round 57 lost three rounds to this: the emitted script embeds its probes as TEMPLATE LITERALS, so every nesting
# level ate one backslash — `/^color\(/` in the file reached the browser as `/^color(/`, and `/rgba?\(…\)/` became a
# capture group whose parser returned NaN. Every guard against NaN is false, so the loud axis counted EVERY element
# over 400px2 for thirty-seven rounds while reporting a clean-looking six. The fix is structural (probes go out as
# JSON strings, the idiom the contrast probe has used since round 88) and this is the check that keeps it: every
# probe constant in the emitted script must be a JSON string whose VALUE equals the one the core produced.
cat > "$TMP/probe-check.mjs" <<'JS'
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const [, , root, emittedPath] = process.argv;
const core = await import(pathToFileURL(root + "/agent/scripts/lib/design-sweep.mjs").href);
const emitted = readFileSync(emittedPath, "utf8");
const intended = core.pageChecks("#root");
const jsonConst = (text, name) => {
  const m = new RegExp("const " + name + " = (\"(?:[^\"\\\\]|\\\\.)*\");").exec(text);
  return m ? m[1] : null;
};
const names = [...intended.matchAll(/const (\w+) = "/g)].map((m) => m[1]);
const problems = [];
if (names.length < 3) problems.push(`read ${names.length} probe constant(s) from the core, so this proves nothing`);
for (const name of names) {
  // a probe that reaches the page as a template literal loses one level of escaping on the way
  if (new RegExp("const " + name + " = `").test(emitted)) {
    problems.push(`${name} goes out as a TEMPLATE LITERAL — one level of escaping is lost before the page sees it`);
    continue;
  }
  const a = jsonConst(emitted, name);
  const b = jsonConst(intended, name);
  if (!a || !b) { problems.push(`${name} is not a JSON string on both sides`); continue; }
  if (JSON.parse(a) !== JSON.parse(b)) problems.push(`${name} reaches the page CHANGED (${JSON.parse(a).length} vs ${JSON.parse(b).length} chars)`);
}
for (const name of ["PROBE", "UNSTYLED", "TARGETS", "THEME"]) {
  if (new RegExp("const " + name + " = `").test(emitted)) problems.push(`${name} goes out as a template literal`);
}
// AND EVERY TEMPLATE LITERAL IN THE EMITTED SCRIPT, not just the probes: a backslash inside one is eaten when the
// file is evaluated, so `split(/\\s+/)` reaches the page as `/s+/`. An escaped backtick (`\``) is the one legitimate
// use. This is the general form of the bug that cost rounds 55-57 — it caught MOTION, which the probe check above
// could not.
{
  let inside = false, line = 1, i = 0;
  while (i < emitted.length) {
    const ch = emitted[i];
    if (ch === "\n") line++;
    if (ch === "\\" && inside) {
      let j = i;
      while (j < emitted.length && emitted[j] === "\\") j++;
      const run = j - i;
      const next = j < emitted.length ? emitted[j] : "";
      // an EVEN run is a literal backslash and an ODD run ending on a backtick is an escaped backtick; an odd run
      // ending anywhere else escapes the NEXT character at the template level and is eaten before the page sees it
      if (run % 2 === 1 && next !== "`") {
        problems.push(`line ${line}: a single backslash inside a template literal (\\${next}) is eaten before the page sees it`);
      }
      i = j; continue;
    }
    if (ch === "`") inside = !inside;
    i++;
  }
  if (inside) problems.push("the emitted script ends inside a template literal");
}
if (problems.length) { for (const x of problems) console.error("  " + x); process.exit(1); }
console.log("ok: " + names.length + " probe constant(s) reach the page byte-identical");
JS
if node "$TMP/probe-check.mjs" "$PWD" "$TMP/sweep.js" > "$TMP/probe-check.out" 2>&1; then
  ok "$(tail -1 "$TMP/probe-check.out")"
else
  bad "the emitted probes do not reach the page unchanged: $(head -4 "$TMP/probe-check.out")"
fi

# ── 1c. IDLE REPAINT, AND THE INSTRUMENT THAT MEASURES IT (round 64) ───────────────────────────
# The claim is exact: under the harness's STATIC fixtures nothing changes, so a settled panel should write nothing to
# the DOM. Two things are pinned here — the verdict, and the requirement that the observer saw its OWN probe mutation,
# because a blind observer reports a perfectly still panel forever and that is indistinguishable from a clean one.
cat > "$TMP/idle-check.mjs" <<'JS'
import { readFileSync } from "node:fs";
const [file, expect] = process.argv.slice(2);
const report = JSON.parse(readFileSync(file, "utf8"));
report.idle = [{
  density: "panel", theme: "light", page: "panel-Terminal", seconds: 6,
  byTarget: { __probe: 1, ...(process.env.VALE_IDLE_TARGET ? { "div.totals": 3 } : {}) },
  mutations: process.env.VALE_IDLE_TARGET ? 4 : 1,
  selfTest: true,
}];
if (process.env.VALE_IDLE_BLIND) report.idle[0].byTarget = {};
if (process.env.VALE_IDLE_BLIND) report.idle[0].mutations = 0;
const out = JSON.stringify(report);
const { writeFileSync } = await import("node:fs");
writeFileSync(expect, out);
JS
clean_idle() {
  cat > "$TMP/idle-clean.json" <<'JSON'
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
  "focus": [],
  "idle": [{"density":"panel","theme":"light","page":"Terminal","seconds":6,"byTarget":{"__probe":1},"mutations":1,"selfTest":true}]}
JSON
}
clean_idle
if node "$TOOL" --judge "$TMP/idle-clean.json" > "$TMP/idle-clean.out" 2>&1; then
  ok "a panel that writes nothing while idle passes the idle clause"
else
  bad "a still panel was rejected: $(head -3 "$TMP/idle-clean.out")"
fi
cat > "$TMP/idle-busy.json" <<'JSON'
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
  "focus": [],
  "idle": [{"density":"panel","theme":"light","page":"Terminal","seconds":6,"byTarget":{"__probe":1,"div.totals":3},"mutations":4,"selfTest":true}]}
JSON
if node "$TOOL" --judge "$TMP/idle-busy.json" > "$TMP/idle-busy.out" 2>&1; then
  bad "a panel that repainted three times while idle passed the idle clause"
else
  grep -q "DOM mutation(s) in 6s while idle" "$TMP/idle-busy.out" && ok "an idle repaint fails, and the clause names the target" || bad "the idle clause failed for the wrong reason: $(head -3 "$TMP/idle-busy.out")"
fi
cat > "$TMP/idle-blind.json" <<'JSON'
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
  "focus": [],
  "idle": [{"density":"panel","theme":"light","page":"Terminal","seconds":6,"byTarget":{},"mutations":0,"selfTest":false}]}
JSON
if node "$TOOL" --judge "$TMP/idle-blind.json" > "$TMP/idle-blind.out" 2>&1; then
  bad "a blind idle observer passed — a still panel and a blind instrument are indistinguishable"
else
  grep -q "did not see its own probe mutation" "$TMP/idle-blind.out" && ok "a blind idle observer fails, and the clause says why" || bad "the blind case failed for the wrong reason: $(head -3 "$TMP/idle-blind.out")"
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

# ── 2b. THE `twoloud` EXCEPTION EXCEPTS, AND ONLY WHERE IT IS NAMED ────────────────────────────
# Round 42 added 24 rail surfaces to the pages pass and TWO of them measure two loud elements: the rail button that
# says which page you are on, and the tab or new-session button. The ceiling of one is right about focal points and
# wrong about two indicators of where you are, so those two pages are named. This pins BOTH directions on the same
# planted reading — the named page must pass, and a page that is not named must still fail (that case lives in the
# axis loop as `loud-not-excepted`). Without the first, the pass would go red on a defect-free page; without the
# second, a wider exception would hide a page that really did have two focal points.
sed 's/"page": "Terminal"/"page": "panel-Terminal"/' "$TMP/clean.json" > "$TMP/twoloud.json"
python3 - "$TMP/twoloud.json" <<'PYEOF'
import json, sys
p = sys.argv[1]
d = json.load(open(p))
d["surfaces"][0]["loud"] = ["button.rail-btn 1444px2 rgb(154,52,18)", "div.tab 3254px2 rgb(154,52,18)"]
json.dump(d, open(p, "w"), indent=1)
PYEOF
if node "$TOOL" --judge "$TMP/twoloud.json" > "$TMP/twoloud.out" 2>&1; then
  ok "the pages whose second loud element is NAVIGATION pass, by name"
else
  bad "a named page was rejected: $(tail -3 "$TMP/twoloud.out")"
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
elif which == "mark-ringfill":
    # A MARK THAT IS A FILL **AND** A RING IS NEITHER — and it is NOT a collision, so the distinctness check passes
    # it. That is how four broken `.plug-dot` arms survived every sweep until round 45: the probe reduced the mark to
    # one kind and called a fill inside a ring a "ring".
    r["surfaces"][0]["marks"] = {"families": [".plug-dot[warn]"], "collisions": [], "ringFill": [".plug-dot[warn]"]}
elif which == "mark-collision":
    # TWO STATES OF ONE MARK PAINTING IDENTICALLY — the sheet can be right while the page is wrong (round 25's
    # `.plug-dot[error]` kept a stray halo through a unit test that passed).
    r["surfaces"][0]["marks"] = {"families": [".cmd-dot[fail,ok]"], "collisions": [".cmd-dot: ok and fail paint identically (50%/flat/solid)"]}
elif which == "loud":
    # TWO THINGS SHOUTING IS ONE FOCAL POINT TOO MANY. The probe counts a genuinely saturated fill of a certain
    # size (round 18); this plants a second one beside the first.
    r["surfaces"][0]["loud"] = ["button.approval-approve 1526px2 rgb(30,122,51)", "button.btn-primary 19680px2 rgb(176,58,10)"]
elif which == "loud-not-excepted":
    r["surfaces"][0]["page"] = "panel-Memory"
    r["surfaces"][0]["loud"] = ["button.rail-btn 1444px2 rgb(154,52,18)", "div.tab 3254px2 rgb(154,52,18)"]
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
elif which == "sheets-unreadable":
    # A CHECK WHOSE BASIS IS PARTIAL MUST NOT REPORT CLEAN. An unreadable sheet contributes no class names, so
    # every class it styles looks unstyled — the false-finding shape this suite has now fixed three times.
    r["unstyled"] = [{"page": "Terminal", "styledClasses": 2200, "sheetsUnreadable": 1, "classes": [], "tags": {}}]
elif which == "focus-unconfirmed":
    # A CHECK THAT COULD NOT LOOK MUST NOT READ AS A PASS. The pixel confirmation's screenshot can fail; the
    # first version turned that into "no focus indication", a false finding. It is now its own verdict and it
    # FAILS the run, because an unperformed measurement proves nothing either way.
    r["focus"] = [{"page": "Terminal", "pressed": 14, "landed": 14, "escaped": 0, "missing": 0, "unconfirmed": 2, "unconfirmedOn": ["button.btn", "a.link"], "paintFailed": "clip is outside the viewport"}]
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
for axis in contrast h1 skip landmark geometry sliver loud loud-not-excepted mark-collision mark-ringfill name title-only reflow focus focus-empty motion motion-empty type-floor blind theme-lie harness-stale focus-unconfirmed sheets-unreadable; do
  plant "$axis" "$axis"
  if node "$TOOL" --judge "$TMP/$axis.json" > "$TMP/$axis.out" 2>&1; then
    bad "the judge PASSED a report with a planted '$axis' defect"
  else
    ok "the judge fails a planted '$axis' defect"
  fi
done

# ── 3c. the DIAGNOSTIC HELPER survives the escaping layers ─────────────────────────────────────
# The sweep reports itself to the agent's diagnostic ring, and its first version emitted a regex with every
# backslash eaten by one of this file's three escaping layers — /tokens*:s*/ instead of /token\s*:\s*/ — so it
# never matched and returned silently. The helper now avoids both hazards (forward-slash path, line-prefix
# token read), and THIS asserts the emitted text, because the failure mode is invisible at runtime: the helper
# swallows its own errors by design.
if grep -q 'D:/Vale/etc/config.yaml' "$TMP/sweep.js" && grep -q 'indexOf("device_token")' "$TMP/sweep.js"; then
  ok "the emitted diagnostic helper reads the token without a regex"
else
  bad "the emitted diagnostic helper lost its token read (escaping layer)"
fi
# THE CODE FORM, not the comment: the helper's comment QUOTES the mangled regex to explain what went wrong,
# so a search for that text anywhere in the file matches the explanation. This looks at the call itself.
if grep -q 'readFileSync("D:\\' "$TMP/sweep.js"; then
  bad "the emitted diagnostic helper's path lost its slashes (readFileSync got a mangled path)"
else
  ok "and its config path survived the escaping layers"
fi

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

# ── 3d. the RAIL DOT's fill clears 3:1, with no margin to spare ─────────────────────────────────
# The sweep's DECORATIVE waiver says the working dot's halo is emphasis and "the dot's fill carries the
# state". That was an ASSERTION for fifty rounds: the probe prefers a ring over a fill when both exist, so the
# only row this mark produced measured the HALO at 2.33, and the fill was never measured at all. Round 202
# computed it: --accent #bf3a0a on the rail #1f1f1f is EXACTLY 3.00, the WCAG non-text threshold, with zero
# margin. That is worth a gate, because a one-step token change or a rail recolour puts it under.
TOKENS=agent/resources/panel-react/src/styles/tokens.css
if [ -f "$TOKENS" ]; then
  ACCENT="$(grep -m1 -oE '\-\-accent: *#[0-9a-fA-F]{3,8}' "$TOKENS" | grep -oE '#[0-9a-fA-F]{3,8}')"
  if [ -n "$ACCENT" ]; then
    RATIO="$(node --input-type=module -e "
      import { contrastRatio, parseColour } from './agent/scripts/lib/contrast-probe.mjs';
      console.log(contrastRatio(parseColour('$ACCENT'), parseColour('rgb(31, 31, 31)')).toFixed(2));
    " 2>/dev/null)"
    if [ -n "$RATIO" ] && node -e "process.exit(parseFloat('$RATIO') >= 3 ? 0 : 1)"; then
      ok "the rail dot's fill (--accent $ACCENT on the rail) measures $RATIO:1, at or above 3"
    else
      bad "the rail dot's fill measures $RATIO:1 — under the 3:1 a graphic needs (the waiver assumes it clears)"
    fi
  else
    bad "could not read --accent from $TOKENS"
  fi
else
  bad "tokens.css not found at $TOKENS"
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
for axis in contrast loud mark-collision name geometry focus; do
  python3 - "$TMP/console-clean.json" "$TMP/console-$axis.json" "$axis" <<'PY2'
import json, sys
src, dst, which = sys.argv[1], sys.argv[2], sys.argv[3]
r = json.load(open(src))
if which == "contrast": r["rows"][0]["cr"] = 2.1
elif which == "mark-collision": r["surfaces"][0]["marks"] = {"families": [".sig-dot[ok,err]"], "collisions": [".sig-dot: ok and err paint identically (50%/flat/solid)"]}
elif which == "loud": r["surfaces"][0]["loud"] = ["button.btn-primary 19680px2 rgb(176,58,10)", "button.pay 9000px2 rgb(217,72,15)"]
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

echo "panel-design-sweep: $PASS ok, $FAILED failed"
[ "$FAILED" -eq 0 ]
