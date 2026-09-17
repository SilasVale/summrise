#!/usr/bin/env bash
# sweep-judges.bash — the CONSOLE and EXTENSION judges must fail what they exist to catch.
#
# WHY THIS EXISTS (round 179). `panel-design-sweep.bash` has gated the panel's judge since round 50, with a
# planted defect per axis. The other two adapters had NO gate at all: their adapter-specific rules — the
# extension's message tones, the console's target-size findings and theme check — were mutation-proven by
# hand when written and by nothing since. The shared judge they call is covered through the panel's gate,
# which is exactly why the gap was easy to miss: most of the machinery IS tested, and the parts that are not
# are the parts each adapter wrote for itself.
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

# A clean report for each adapter: the minimum the judge needs to reach the rule under test. Anything absent
# is absent on purpose — the judge treats missing sections as nothing to say, which is the behaviour a fixture
# for ONE rule wants.
write_ext() { # write_ext <path> <python-mutation>
  python3 - "$1" <<PY
import json, sys
r = {
  "rows": [{"sel": "body", "text": "hello", "size": 14, "weight": "400", "need": 4.5, "cr": 15.0, "density": "extension", "page": "options", "width": 900, "theme": "light"}],
  "surfaces": [], "names": [], "focus": [], "unstyled": [], "targets": [],
  "messages": [
    {"label": "refusal", "state": "error", "text": "not a url", "color": "rgb(179, 38, 30)", "size": 12, "surface": "rgba(0, 0, 0, 0)", "bodyBg": "rgb(245, 245, 247)"},
    {"label": "saved", "state": "ok", "text": "saved", "color": "rgb(11, 122, 110)", "size": 12, "surface": "rgba(0, 0, 0, 0)", "bodyBg": "rgb(245, 245, 247)"},
  ],
}
$2
json.dump(r, open(sys.argv[1], "w"))
PY
}

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

EXT=agent/scripts/extension-design-sweep.mjs
CON=agent/scripts/console-design-sweep.mjs

# ── 1. both tools still emit a script that parses ──────────────────────────────────────────────
for tool in "$CON" "$EXT"; do
  name="$(basename "$tool" .mjs)"
  if node "$tool" --emit > "$TMP/$name.js" 2>"$TMP/$name.err"; then
    ok "$name --emit exits 0"
  else
    bad "$name --emit exited non-zero: $(head -3 "$TMP/$name.err")"
  fi
  if node --check "$TMP/$name.js" 2>/dev/null; then
    ok "$name --emit script parses ($(wc -c < "$TMP/$name.js") bytes)"
  else
    bad "$name --emit script does not parse"
  fi
done

# ── 2. clean reports pass ──────────────────────────────────────────────────────────────────────
write_ext "$TMP/ext-clean.json" "pass"
[ "$(judge "$EXT" "$TMP/ext-clean.json")" = "0" ] && ok "extension: a clean report passes" || bad "extension: a clean report was rejected"
write_con "$TMP/con-clean.json" "pass"
[ "$(judge "$CON" "$TMP/con-clean.json")" = "0" ] && ok "console: a clean report passes" || bad "console: a clean report was rejected"

# ── 3. the extension's message rules (round 149's defect, guarded since 158) ────────────────────
tests=(
  "identical|r['messages'][1]['color'] = r['messages'][0]['color']|the refusal and the confirmation painted the same colour"
  "underAA|r['messages'][0]['color'] = 'rgb(200, 160, 155)'|a message under AA"
  "nostate|r['messages'][0]['state'] = ''|a message with no data-state"
)
for t in "${tests[@]}"; do
  IFS='|' read -r name mutation label <<< "$t"
  write_ext "$TMP/ext-$name.json" "$mutation"
  rc="$(judge "$EXT" "$TMP/ext-$name.json")"
  if [ "$rc" = "1" ]; then ok "extension: the judge fails $label"; else bad "extension: $label was NOT a finding (rc=$rc)"; fi
done

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
