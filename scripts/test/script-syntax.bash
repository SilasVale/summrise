#!/usr/bin/env bash
# ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
# Read this when you change this file: the mutation is how you find out whether the gate can still
# fail at all. A gate that cannot be broken is worse than no gate.
#
# MUTATION: append an orphan `fi` to a shell script
# RESULT:   exit 1, with file and line

# script-syntax.bash — every shell script in the repo must PARSE.
#
# WHY THIS EXISTS (round 28). A one-line edit to scripts/publish-release.sh left an orphan
# `fi`. Nothing noticed: CI's gates run the *test* suites, not the release script, so two
# commits shipped with the release path broken and the breakage surfaced only when a publish
# was attempted — the most important script in the repo, silently unrunnable. `bash -n` costs
# milliseconds and answers exactly that question.
set -euo pipefail
cd "$(dirname "$0")/../.."

PASS=0
FAILED=0
# Every tracked shell script (git decides what is part of the repo, not a glob that may miss
# a directory or pick up a vendored copy).
while IFS= read -r f; do
  case "$f" in
    *.bash|*.sh) ;;
    *) continue;;
  esac
  [ -f "$f" ] || continue
  if bash -n "$f" 2>/tmp/script-syntax.err; then
    PASS=$((PASS+1))
  else
    echo "FAIL: $f does not parse:"
    sed 's/^/  /' /tmp/script-syntax.err
    FAILED=$((FAILED+1))
  fi
done < <(git ls-files '*.sh' '*.bash')
rm -f /tmp/script-syntax.err

# A SCAN THAT READ NOTHING IS NOT A CLEAN SCAN (round 170). `git ls-files` returns an EMPTY list when git
# refuses the tree — a dubious-ownership / safe.directory refusal in a container does exactly that — and
# the loop above then runs ZERO times, FAILED stays 0, and this printed `ok: script-syntax 0 files parse`
# with EXIT 0. `set -euo pipefail` cannot catch it: the failure happens inside a process substitution,
# which the shell does not check. MEASURED by the fifteenth exploration on a copy with .git removed.
# 27 scripts are tracked today; 20 leaves room for a deliberate removal and none for a collapse.
FLOOR=20
if [ "$PASS" -lt "$FLOOR" ]; then
  echo "script-syntax: read only $PASS file(s), expected at least $FLOOR — the scan is reading the wrong thing"
  exit 1
fi

if [ "$FAILED" -gt 0 ]; then
  echo "script-syntax: $FAILED file(s) do not parse"
  exit 1
fi
echo "ok: script-syntax $PASS files parse"
