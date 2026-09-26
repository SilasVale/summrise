#!/usr/bin/env bash
# main-only-by-merge — THE RULE PROVEN ON A REAL REPOSITORY, IN BOTH DIRECTIONS, PLUS ITS WIRING.
#
# WHY A THROWAWAY REPOSITORY: the subject is a SEQUENCE of git operations, and no file edit can represent it. A rule about
# git history cannot be proven by mutating a source file and re-running a checker.
#
# THE RULE IS INSTALLED AS THIS REPO'S REAL pre-commit HOOK (`core.hooksPath`), so every case below goes through the path a
# developer's commit actually takes — git invokes the hook, the hook decides. Nothing here calls the rule by hand, because a
# rule that passes when invoked directly and is never wired up is a rule that does not exist.
#
# WHY THE RULE IS ITS OWN SCRIPT RATHER THAN A BLOCK INSIDE `scripts/hooks/pre-commit` — A MEASUREMENT, NOT A PREFERENCE.
# `pre-commit` exits 0 on its fifth line unless `agent/scripts/panel-design-sweep.mjs` exists, because it is designed to be
# symlinked into a GLOBAL `core.hooksPath` and must be inert in every other repository the operator owns. A throwaway
# repository therefore NEVER REACHES anything below that guard: the first version of this proof copied that hook in and would
# have reported five green cases having run NOTHING. As its own script the rule is testable here, and case 6 asserts that the
# real hook invokes it.
#
# AND THE FIXTURE'S BRANCH NAME IS ASSERTED, because this script's first run got it wrong: `git init` takes its default
# branch from `init.defaultBranch`, which is `master` on this box, so the rule — which matches `main` exactly, and
# correctly — was INERT and two cases reported exit 0 that should have been refused. A proof whose fixture is not the shape
# it claims to test reports green having tested nothing.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
RULE="$PWD/scripts/hooks/main-only-by-merge"
HOOK="$PWD/scripts/hooks/pre-commit"
[ -f "$RULE" ] || { echo "  FAIL scripts/hooks/main-only-by-merge does not exist — there is no rule to prove" >&2; exit 1; }
[ -f "$HOOK" ] || { echo "  no scripts/hooks/pre-commit — run from the repo" >&2; exit 2; }

T=$(mktemp -d) || exit 2
trap 'rm -rf "$T"' EXIT
cd "$T" || exit 2
git init -q . && git symbolic-ref HEAD refs/heads/main
git config user.email t@t.invalid && git config user.name t
[ "$(git branch --show-current)" = "main" ] || { echo "  FAIL the fixture repo is not on main — nothing below would test the rule" >&2; exit 2; }

# HISTORY FIRST, THEN THE RULE — which is how a real repository gets it, and it is also the only order that works: the rule
# refuses a ROOT commit on main (there is no merge to make, and main-shape-check's fixture table asserts the same verdict, so
# the two halves agree). Installing the hook before the root commit made this fixture unable to create its own history — the
# rule doing its job, reported as a failure of the proof.
echo one > f && git add f && git commit -qm "root"
mkdir -p scripts/hooks && cp "$RULE" scripts/hooks/pre-commit && chmod +x scripts/hooks/pre-commit
git config core.hooksPath scripts/hooks
echo "  ok   the fixture is on main with history, and the rule is installed as its pre-commit hook"

fail=0
run() { # name, expected-exit, command...
  local name="$1" want="$2"; shift 2
  "$@" >/dev/null 2>&1
  local got=$?
  if [ "$got" = "$want" ]; then
    echo "  ok   $name (exit $got)"
  else
    echo "  FAIL $name — expected exit $want, got $got"
    fail=1
  fi
}

# 1. a direct commit on main is REFUSED — through the installed hook, which is the path a real commit takes
echo two > f && git add f
run "a direct commit on main is refused" 1 git commit -qm "direct"
git reset -q --hard HEAD

# 2. --amend on main is REFUSED — rewriting main is the same shape and must not slip through a different door
echo three > f && git add f
run "--amend on main is refused" 1 git commit -q --amend -m "rewrite"
git reset -q --hard HEAD

# 3. a commit on a BRANCH is ALLOWED — the direction that keeps this rule from being reverted within a week
git checkout -qb change/x
echo four > f && git add f
run "a commit on a branch is allowed" 0 git commit -qm "on branch"

# 4. a --no-ff merge into main is ALLOWED. THIS IS THE CASE THAT NEEDS THE HOOK TO RUN *DURING* THE MERGE, with MERGE_HEAD
#    present — an earlier version invoked the rule by hand before merging, when no merge was in progress, and got a correct
#    refusal reported as a failure.
git checkout -q main
run "a --no-ff merge into main is allowed" 0 git merge --no-ff -q -m "merge change/x" change/x

# 5. and the merge really produced a merge commit — the CI gate checks the parent count, so this is its half
parents=$(git rev-list --parents -n 1 HEAD | awk '{print NF-1}')
if [ "$parents" = "2" ]; then
  echo "  ok   the merge produced a 2-parent commit (what main-shape-check requires)"
else
  echo "  FAIL the merge produced $parents parent(s), so main-shape-check would refuse it"
  fail=1
fi

# 6. the WIRING: a rule nothing calls does not exist
if grep -q 'main-only-by-merge' "$HOOK"; then
  echo "  ok   scripts/hooks/pre-commit invokes the rule"
else
  echo "  FAIL scripts/hooks/pre-commit does NOT invoke scripts/hooks/main-only-by-merge"
  fail=1
fi

[ "$fail" = 0 ] && echo "main-only-by-merge: 6 case(s), both directions, through the installed hook on a real repository"
exit "$fail"
