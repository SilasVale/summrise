#!/usr/bin/env bash
# The FIRST executable coverage of scripts/publish-release.sh (ledger D13: three
# files mentioned it, all as SOURCE PINS — nothing ever RAN it). These cases drive
# the REAL script and assert the refusals it owns, so they need no build, no
# network and no deploy. Run: bash scripts/test/publish-release.bash
set -uo pipefail
cd "$(dirname "$0")/../.."
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

# 1. --audit-only with no version must REFUSE, and the refusal must name the usage.
#    Asserting only "non-zero" would not do: without the guard the script would
#    stumble into the audit with an empty version and still exit non-zero, so the
#    MESSAGE is the thing that proves the guard ran. (Round 124's lesson: a check a
#    removal can leave green proves nothing.)
out=$(bash scripts/publish-release.sh --audit-only 2>&1); rc=$?
if [ "$rc" -ne 0 ] && grep -q 'usage: ./scripts/publish-release.sh --audit-only <1.2.N>' <<<"$out"; then
  ok "--audit-only with no version refuses and names the usage"
else
  bad "--audit-only with no version: rc=$rc out=$(head -c 200 <<<"$out")"
fi

# 2. It must refuse BEFORE touching the tree. A guard that fires after the first
#    side effect is not a guard: assert the run left the worktree as it found it.
# --untracked-files=no: an untracked file could be the TEST ITSELF (it was, the
# first time this ran), and "a new file exists" is not a side effect of the script.
dirty=$(git status --porcelain --untracked-files=no | head -5)
[ -z "$dirty" ] && ok "the refusal left the worktree clean" \
  || bad "the refusal modified the tree: $dirty"

# 3. The script must not reach wrangler/npm on a refusal — the whole point of a
#    fail-closed entry is that no build starts. Detected by effect: a refusal that
#    had packed anything would have created a tgz under index/public.
stray=$(find index/public/vale-agent -name 'vale-agent-*.tgz' -newermt '-2 minutes' 2>/dev/null | head -3)
[ -z "$stray" ] && ok "no tgz was packed by the refusals" || bad "packed during a refusal: $stray"

printf '\npublish-release: %d checks passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
