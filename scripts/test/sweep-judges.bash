#!/usr/bin/env bash
# sweep-judges.bash — the CONSOLE judge must fail what it exists to catch.
#
# WHY THIS EXISTS (round 179). `panel-design-sweep.bash` has gated the panel's judge since round 50, with a
# planted defect per axis. The other adapters had NO gate at all: their adapter-specific rules were
# mutation-proven by hand when written and by nothing since. The shared judge they call is covered through the
# panel's gate, which is exactly why the gap was easy to miss: most of the machinery IS tested, and the parts
# that are not are the parts each adapter wrote for itself.
#
# IT COVERED TWO ADAPTERS UNTIL ROUND 243, when the EXTENSION was removed. What moved rather than disappeared:
# the delivered-entry provenance checks (section 3) ran against the extension because it was the adapter whose
# entry was easiest to stage; the CONSOLE has the same `entryCheck` and inherits them here, so the coverage
# survives the component. What genuinely went with the extension was its message-tone rules (round 149's
# refusal-versus-confirmation defect) — the console collects no `messages`, so there is nothing left to guard.
#
# Each case below plants ONE defect in an otherwise clean report and requires the judge to fail. A judge that
# says OK to everything is worse than no judge, and the only way to know is to break the thing it guards.
set -euo pipefail
cd "$(dirname "$0")/../.."

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAILED=0
ok() { PASS=$((PASS + 1)); echo "  ok: $1"; }
bad() { FAILED=$((FAILED + 1)); echo "  FAIL: $1" >&2; }

# A clean report: the minimum the judge needs to reach the rule under test. Anything absent is absent on
# purpose — the judge treats missing sections as nothing to say, which is the behaviour a fixture for ONE rule
# wants.
write_con() { # write_con <path> <python-mutation>
  python3 - "$1" <<PY
import json, sys
r = {
  "rows": [{"sel": "body", "text": "hello", "size": 14, "weight": "400", "need": 4.5, "cr": 15.0, "density": "console", "page": "overview", "width": 1440, "theme": "light"}],
  "surfaces": [], "names": [], "focus": [], "unstyled": [], "targets": [], "themeChecks": [],
}
$2
json.dump(r, open(sys.argv[1], "w"))
PY
}

judge() { # judge <tool> <report> -> rc
  local rc=0
  node "$1" --judge "$2" >/dev/null 2>&1 || rc=$?
  echo "$rc"
}

CON=agent/scripts/console-design-sweep.mjs

# ── 1. the tool still emits a script that parses ────────────────────────────────────────────────
if node "$CON" --emit > "$TMP/console-design-sweep.js" 2>"$TMP/con.err"; then
  ok "console-design-sweep --emit exits 0"
else
  bad "console-design-sweep --emit exited non-zero: $(head -3 "$TMP/con.err")"
fi
if node --check "$TMP/console-design-sweep.js" 2>/dev/null; then
  ok "console-design-sweep --emit script parses ($(wc -c < "$TMP/console-design-sweep.js") bytes)"
else
  bad "console-design-sweep --emit script does not parse"
fi

# ── 2. a clean report passes ────────────────────────────────────────────────────────────────────
write_con "$TMP/con-clean.json" "pass"
[ "$(judge "$CON" "$TMP/con-clean.json")" = "0" ] && ok "console: a clean report passes" || bad "console: a clean report was rejected"

# ── 2b. THE RENDERED PRESS AXIS (round 60) ──────────────────────────────────────────────────────
# The console had press feedback checked at SHEET level only (feedback-check.mjs proves an :active RULE exists);
# round 54 recorded that the rendered measurement existed for the panel and not here. The sweep now presses 21
# controls across 6 console pages and the judge fails any control whose computed style is identical during the
# press. Both directions are pinned, because a clause that fails everything is as useless as one that fails
# nothing.
write_con "$TMP/con-press-clean.json" '
r["press"] = [{"density": "console", "theme": "light", "page": "overview", "measured": 3, "rows": [
  {"sel": ".rail-btn", "where": "button.rail-btn", "size": "40x40", "changed": True, "props": ["transform"]},
  {"sel": ".btn", "where": "button.btn", "size": "48x28", "changed": True, "props": ["transform"]},
  {"sel": ".rail-avatar", "where": "button.rail-avatar", "size": "36x36", "changed": True, "props": ["transform"]},
]}]
'
[ "$(judge "$CON" "$TMP/con-press-clean.json")" = "0" ] && ok "console: pressed controls that answer pass" || bad "console: a clean press pass was rejected"

write_con "$TMP/con-press-dead.json" '
r["press"] = [{"density": "console", "theme": "light", "page": "overview", "measured": 3, "rows": [
  {"sel": ".rail-btn", "where": "button.rail-btn", "size": "40x40", "changed": True, "props": ["transform"]},
  {"sel": ".btn", "where": "button.btn", "size": "48x28", "changed": False, "props": []},
  {"sel": ".rail-avatar", "where": "button.rail-avatar", "size": "36x36", "changed": True, "props": ["transform"]},
]}]
'
if [ "$(judge "$CON" "$TMP/con-press-dead.json")" != "0" ]; then
  ok "console: a control that renders NOTHING when pressed fails"
else
  bad "console: a dead press passed the judge"
fi

write_con "$TMP/con-press-empty.json" '
r["press"] = [{"density": "console", "theme": "light", "page": "devices", "measured": 0, "rows": [
  {"sel": ".dtab", "note": "not rendered on this page"},
]}]
'
if [ "$(judge "$CON" "$TMP/con-press-empty.json")" != "0" ]; then
  ok "console: a press pass that pressed nothing fails"
else
  bad "console: a press pass that measured nothing passed the judge"
fi

# ── 3. a DELIVERED COPY OLDER THAN THE BUILD ────────────────────────────────────────────────────
# Round 184 found the console's directory holding eight files from four generations; round 189 lost an
# afternoon to a stale panel harness. The entry's digest is baked at emit time and checked at run time; these
# plant the mismatch, and the digest is read FROM the emitted script rather than copied here, so a rebuild
# cannot make this fixture agree with a build it was not written for.
read -r BYTES SHA < <(node -e '
// THE STAMP IS READ FROM THE ARTIFACT AS DATA (round 268). This used to regex `EXPECTED_ENTRY = ({...})` out of the
// emitted output — the spelling the console payload used while it was a template literal — so after the payload moved
// into a module the read came back empty, the fixtures below were built with a blank digest, and python refused the
// file with "invalid syntax" instead of the gate saying what it could not read.
const { readFileSync } = require("fs");
const { pathToFileURL } = require("url");
(async () => {
  const { piecesOf } = await import(pathToFileURL(process.argv[1] + "/scripts/test/lib/emitted-pieces.mjs").href);
  const p = piecesOf(readFileSync(process.argv[2], "utf8"), "the emitted console script");
  const e = p.config && p.config.expectedEntry;
  if (!e || typeof e.bytes !== "number" || !e.sha) throw new Error("the pieces carry no expectedEntry digest");
  console.log(e.bytes + " " + e.sha);
})().catch((err) => { console.error(err.message); process.exit(1); });
' "$PWD" "$TMP/console-design-sweep.js")
[ -n "$BYTES" ] && [ -n "$SHA" ] && ok "the emitted console script names the entry it was built against ($BYTES / $SHA)" \
  || bad "could not read the baked entry digest from the emitted console script — the fixtures below would prove nothing"

write_con "$TMP/con-stale.json" "r['entryCheck'] = {'bytes': 1, 'sha': 'deadbeef0000', 'expected': {'bytes': $BYTES, 'sha': '$SHA'}, 'stale': True}"
[ "$(judge "$CON" "$TMP/con-stale.json")" = "1" ] && ok "console: the judge fails a stale delivered entry" || bad "console: a stale entry was NOT a finding"
write_con "$TMP/con-unreadable.json" "r['entryCheck'] = {'error': 'ENOENT', 'expected': {'bytes': $BYTES, 'sha': '$SHA'}, 'stale': True}"
[ "$(judge "$CON" "$TMP/con-unreadable.json")" = "1" ] && ok "console: an unreadable entry fails too, naming the expected digest" || bad "console: an unreadable entry was NOT a finding"

# A CURRENT ENTRY MUST SAY SO — provenance that only appears on failure cannot be checked.
write_con "$TMP/con-current.json" "r['entryCheck'] = {'bytes': $BYTES, 'sha': '$SHA', 'expected': {'bytes': $BYTES, 'sha': '$SHA'}, 'stale': False}"
if node "$CON" --judge "$TMP/con-current.json" > "$TMP/con-current.out" 2>&1; then
  ok "a current delivered entry still passes"
else
  bad "the judge failed a report whose entry matches the build"
fi
grep -q "delivered entry $BYTES bytes / sha $SHA" "$TMP/con-current.out" && ok "and the judge names the build it measured" || bad "the entry provenance note is missing"

# ── 4. the console's rules: target size and the theme it actually rendered ─────────────────────
write_con "$TMP/con-target.json" "r['targets'] = [{'page': 'overview', 'checked': 10, 'undersized': 1, 'distinct': [{'sel': 'button.x', 'text': 'x', 'w': 12, 'h': 12, 'nearest': 4.0, 'passesBySpacing': False}]}]"
[ "$(judge "$CON" "$TMP/con-target.json")" = "1" ] && ok "console: the judge fails an undersized target with no spacing" || bad "console: a planted 2.5.8 failure was NOT a finding"
write_con "$TMP/con-theme.json" "r['themeChecks'] = [{'page': 'overview-dark', 'intended': 'dark', 'stored': 'light', 'attr': '', 'bodyBackground': 'rgb(255, 255, 255)'}]"
[ "$(judge "$CON" "$TMP/con-theme.json")" = "1" ] && ok "console: the judge fails a report describing a page it did not render" || bad "console: the theme lie was NOT a finding"
# The spacing clause must still save a small target that has room — the half that keeps 2.5.8 from crying wolf.
write_con "$TMP/con-spacing.json" "r['targets'] = [{'page': 'overview', 'checked': 10, 'undersized': 1, 'distinct': [{'sel': 'input.cb', 'text': '', 'w': 13, 'h': 13, 'nearest': 93.8, 'passesBySpacing': True}]}]"
[ "$(judge "$CON" "$TMP/con-spacing.json")" = "0" ] && ok "console: an undersized target WITH spacing still passes" || bad "console: the 2.5.8 spacing clause was ignored"

echo
echo "sweep-judges: $PASS ok, $FAILED failed"
[ "$FAILED" = "0" ]
