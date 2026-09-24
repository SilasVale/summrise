#!/usr/bin/env bash
# Run every gate `ci.yml` invokes, locally, in one command.
#
# WHY THIS EXISTS, and it is a real cost rather than tidiness: AGENTS.md's table lists the
# PER-DIRECTORY test commands (`npm test`, `cargo test`, clippy, fmt) and NOT the forty-odd gate
# scripts the `pack-chain` job runs one step at a time. So "run the command the other end runs" — the
# rule that table exists to enforce — could not be satisfied by reading it, and round 41 paid:
# `contract-vocabulary-check` failed CI over a test fixture while every command in the table was green.
#
# THE LIST IS DERIVED, NOT RESTATED. It is read out of the workflow on every run, so a gate added to
# CI is covered here the moment it is added, and this file can never become a second list that drifts
# from the first.
#
# A gate that exits 2 is declaring "this host cannot run me" (the design sweep needs a browser and says
# so); that is reported as `n/a`, never as a pass and never as a failure.
#
# WHAT THIS DOES NOT COVER, learned the hard way on 2026-09-24: this is HALF of "what CI runs". It runs
# the GATE COMMANDS, not the per-directory SUITES in AGENTS.md's table — `gateway`'s
# typecheck/lint/format/test, `gateway/ui`'s build+test, `agent/resources/panel-react`'s, the three
# cargo configurations, and `cd agent/summrise-desktop-electron && npm test`. The stale Source Viewer
# mirror that this omission hid is checked by `gateway`'s suite (`code-viewer-mirror.test.mjs`), which
# is not among the commands below, so run BOTH halves before believing a tree is green:
#
#     bash scripts/test/all-gates.bash && (cd gateway && npm run typecheck && npm run lint \
#       && npm run format:check && npm test)
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

WORKFLOW=".github/workflows/ci.yml"
[ -f "$WORKFLOW" ] || { echo "  no $WORKFLOW — run from the repo" >&2; exit 1; }

mapfile -t gates < <(
  grep -ohE '(node|bash|python3) +scripts/test/[A-Za-z0-9._-]+' "$WORKFLOW" |
    sort -u |
    # ITSELF, EXCLUDED — load-bearing rather than tidy: this file is invoked from ci.yml (build-pins
    # enforces that EVERY scripts/test file is), so the extraction finds it, and running it would run
    # this script again, forever. It is the one command here that is a superset of the others rather
    # than a peer. Filtered out rather than blanked in the array, so the counts below stay exact.
    grep -v '^bash scripts/test/all-gates\.bash$'
)
if [ "${#gates[@]}" -lt 20 ]; then
  echo "  read only ${#gates[@]} gate command(s) from $WORKFLOW — the workflow moved, so this proves nothing" >&2
  exit 1
fi

fail=0
notrun=0
ok=0
for cmd in "${gates[@]}"; do
  out=$($cmd 2>&1)
  code=$?
  case $code in
    0) ok=$((ok + 1)); printf '  ok    %s\n' "$cmd" ;;
    2) notrun=$((notrun + 1)); printf '  n/a   %s (declared it cannot run here)\n' "$cmd" ;;
    *) fail=$((fail + 1)); printf '  FAIL  %s\n' "$cmd"; printf '%s\n' "$out" | tail -4 | sed 's/^/          /' ;;
  esac
done

printf '\n  %d ok, %d failed, %d not runnable here (of %d gate command(s) in %s)\n' \
  "$ok" "$fail" "$notrun" "${#gates[@]}" "$WORKFLOW"
[ "$fail" -eq 0 ]
