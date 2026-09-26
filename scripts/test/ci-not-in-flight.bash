#!/usr/bin/env bash
# ci-not-in-flight — THE PUSH GUARD PROVEN IN BOTH DIRECTIONS, ON SAVED API RESPONSES.
#
# WHY FIXTURES AND NOT A STUB SERVER: this box is forbidden from listening on any port, and a saved response is a better
# fixture anyway — it is the exact bytes the API returned, so the check is tested against the real shape rather than one
# somebody typed from memory. `SUMMRISE_CI_RUNS_FILE` is the seam.
#
# THE FAIL-OPEN DIRECTION IS TESTED AS CAREFULLY AS THE REFUSAL. A guard that blocks a push because a MIRROR is flaky is a
# worse failure than the one it prevents, and this repository's git remote has been measured answering in 0s, 32s and >90s
# for the same request — so "the API did not answer" must exit 0, and that is asserted rather than assumed.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
CHECK="$PWD/scripts/hooks/ci-not-in-flight"
[ -f "$CHECK" ] || { echo "  FAIL scripts/hooks/ci-not-in-flight does not exist" >&2; exit 1; }

T=$(mktemp -d) || exit 2
trap 'rm -rf "$T"' EXIT
fail=0
run() { # name, expected-exit, fixture-file-or-EMPTY
  local name="$1" want="$2" file="$3" got
  if [ "$file" = "EMPTY" ]; then
    SUMMRISE_CI_RUNS_FILE="" bash "$CHECK" >/dev/null 2>&1
  else
    SUMMRISE_CI_RUNS_FILE="$file" bash "$CHECK" >/dev/null 2>&1
  fi
  got=$?
  if [ "$got" = "$want" ]; then echo "  ok   $name (exit $got)"
  else echo "  FAIL $name — expected $want, got $got"; fail=1; fi
}

cat > "$T/live.json" <<'JSON'
{"total_count":2,"workflow_runs":[
 {"head_sha":"728fe1e6aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"in_progress","name":"ci"},
 {"head_sha":"6af1dbe6aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","name":"ci"}]}
JSON
cat > "$T/quiet.json" <<'JSON'
{"total_count":2,"workflow_runs":[
 {"head_sha":"6af1dbe6aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","name":"ci"},
 {"head_sha":"f9d5c656aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","name":"ci"}]}
JSON
cat > "$T/queued.json" <<'JSON'
{"total_count":1,"workflow_runs":[{"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"queued","name":"ci"}]}
JSON
printf 'not json at all' > "$T/garbage.json"
printf '' > "$T/empty.json"

run "a run IN PROGRESS refuses the push" 1 "$T/live.json"
run "a run QUEUED refuses the push too — it has not started, and a push still supersedes it" 1 "$T/queued.json"
run "everything completed allows the push" 0 "$T/quiet.json"
run "an unparseable response fails OPEN — a guard that blocks on a bad answer is worse than the slip" 0 "$T/garbage.json"
run "an empty response fails OPEN" 0 "$T/empty.json"
run "an unreadable fixture fails OPEN" 0 "$T/does-not-exist.json"

# THE HOOK MUST ACTUALLY RUN, INVOKED THE WAY GIT INVOKES IT — and grepping for the call site is NOT that test.
# The first version of this file only grepped, and the hook it was checking died on a real push with
# "No such file or directory": it resolved `ci-not-in-flight` from `$0`, which is `.githooks/pre-push`, where the file is
# not. `scripts/test/hook-finds-its-repo.bash` exists because `pre-commit` had the identical defect for months.
D="$PWD/.githooks-probe"
rm -rf "$D"; mkdir -p "$D"
ln -sf ../scripts/hooks/pre-push "$D/pre-push"
probe_out=$(cd "$PWD" && SUMMRISE_CI_RUNS_FILE="$T/live.json" bash .githooks-probe/pre-push 2>&1); probe_rc=$?
rm -rf "$D"
if printf '%s' "$probe_out" | grep -q 'No such file or directory'; then
  echo "  FAIL the hook cannot find its own check when invoked as \`.githooks/pre-push\`:"; printf '%s\n' "$probe_out" | sed 's/^/         /'; fail=1
elif [ "$probe_rc" = "1" ] && printf '%s' "$probe_out" | grep -q 'REFUSED'; then
  echo "  ok   invoked the way git invokes it, the hook RUNS and refuses a live run (exit $probe_rc)"
else
  echo "  FAIL invoked as \`.githooks/pre-push\` the hook exited $probe_rc without refusing:"; printf '%s\n' "$probe_out" | sed 's/^/         /'; fail=1
fi

[ "$fail" = 0 ] && echo "ci-not-in-flight: 7 case(s), both directions, on saved API responses"
exit "$fail"
