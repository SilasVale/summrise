#!/usr/bin/env bash
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

if [ "$FAILED" -gt 0 ]; then
  echo "script-syntax: $FAILED file(s) do not parse"
  exit 1
fi
echo "ok: script-syntax $PASS files parse"
