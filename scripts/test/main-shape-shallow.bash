#!/usr/bin/env bash
# main-shape-shallow — THE GATE MUST WORK IN THE CLONE CI ACTUALLY GIVES, WHICH IS SHALLOW.
#
# MEASURED 2026-09-26, and it cost a red CI job that NO LOCAL RUN COULD REPRODUCE. `actions/checkout@v4` defaults to
# `fetch-depth: 1`, so the runner holds a SHALLOW clone. Git grafts the shallow boundary commit, so
# `git rev-list --parents -n 1 HEAD` answers with the SHA ALONE — ZERO PARENTS — for a commit that has two. The gate's first
# version used that, so it FAILED CI ON A `main` THAT WAS A MERGE while passing on every developer's full clone:
#
#     git rev-list --parents -n 1 HEAD    ->  '3a54cfa35e97c483fb868ed6c7ef7747790639e7'   (no parents at all)
#     git cat-file -p HEAD | grep ^parent ->  2 parent line(s)                            (the object always has them)
#
# The gate now reads the COMMIT OBJECT, and this file is what stops it drifting back.
#
# THE FIXTURE IS A REAL SHALLOW CLONE OF A REAL REPOSITORY WHOSE HEAD IS A REAL MERGE, because nothing else reproduces the
# condition: `git clone --depth 1` is what creates the graft, and no amount of setting variables does. The first line of
# output states what rev-list saw, so a clone that is NOT grafting is visible rather than silently weakening the test.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
GATE="$PWD/scripts/test/main-shape-check.mjs"
[ -f "$GATE" ] || { echo "  no scripts/test/main-shape-check.mjs — run from the repo" >&2; exit 2; }

T=$(mktemp -d) || exit 2
trap 'rm -rf "$T"' EXIT
cd "$T" || exit 2

# 1. a source repository whose HEAD is a --no-ff merge
git init -q src
cd src || exit 2
git symbolic-ref HEAD refs/heads/main
git config user.email t@t.invalid && git config user.name t
echo one > f && git add f && git commit -qm root
git checkout -qb change/x && echo two > f && git add f && git commit -qm work
git checkout -q main && git merge --no-ff -q -m "merge change/x" change/x
fixture_parents=$(git cat-file -p HEAD | grep -c '^parent ')
[ "$fixture_parents" = "2" ] || { echo "  FAIL the fixture's HEAD has $fixture_parents parent(s), not 2 — it is not a merge" >&2; exit 2; }
cd "$T" || exit 2

# 2. a SHALLOW clone of it — this is the whole point, and the shallow file is asserted rather than assumed
git clone -q --depth 1 "file://$T/src" shallow || { echo "  FAIL could not clone shallowly" >&2; exit 2; }
cd shallow || exit 2
[ -f .git/shallow ] || { echo "  FAIL the clone is not shallow, so this fixture would test nothing" >&2; exit 2; }

# 3. the two counting methods side by side, so a failure names WHICH one regressed
words=$(git rev-list --parents -n 1 HEAD | wc -w)
rev_parents=$((words - 1))
obj_parents=$(git cat-file -p HEAD | grep -c '^parent ')
echo "  note  shallow clone: rev-list reports $rev_parents parent(s), the commit object has $obj_parents"
[ "$rev_parents" = "0" ] || echo "  note  rev-list saw $rev_parents — this clone is NOT grafting, so the fixture is weaker than it looks"

# 4. the gate, on main, at a merge, in a shallow clone: it must PASS
out=$(GITHUB_REF_NAME=main node "$GATE" 2>&1)
rc=$?
if [ "$rc" = 0 ]; then
  echo "  ok   the gate passes in a shallow clone at a merge — $(printf '%s' "$out" | head -c 88)"
else
  echo "  FAIL the gate refused a real merge in a shallow clone (exit $rc):" >&2
  printf '%s\n' "$out" | sed 's/^/         /' >&2
  echo "  This is the CI failure of 2026-09-26: \`git rev-list --parents\` cannot see parents across a shallow graft." >&2
  exit 1
fi

echo "main-shape-shallow: 1 case — the clone CI actually gives, which no full clone can reproduce"
exit 0
