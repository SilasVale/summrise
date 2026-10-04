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
#
# **AND THE GATE IT DRIVES IS THE RUST ONE NOW** (2026-10-03). `scripts/test/main-shape-check.mjs` came across to
# `agent/tests/main_shape.rs`, and this fixture was the reason the port needed one thing the JavaScript did not:
# **`cargo test` runs the test binary with its working directory set to the PACKAGE ROOT**, so a fixture that
# `cd`s into a shallow clone and runs the gate would silently measure the REAL repository — the one thing this
# file exists to prevent. The gate therefore takes the repository as `SUMMRISE_MAIN_SHAPE_REPO`, defaulting to
# the CWD, and step 4 below is what proves it reads the clone.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
ROOT="$PWD"
MANIFEST="$ROOT/agent/Cargo.toml"
[ -f "$MANIFEST" ] || { echo "  no agent/Cargo.toml — run from the repo" >&2; exit 2; }
command -v cargo >/dev/null 2>&1 || { echo "  no cargo on PATH — this fixture drives the Rust gate" >&2; exit 2; }

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

# 4. the gate, on main, at a merge, in a shallow clone: it must PASS.
#    `SUMMRISE_MAIN_SHAPE_REPO` IS THE WHOLE POINT OF THIS STEP: without it the gate would read the repository
#    cargo was invoked from, pass for the wrong reason, and this fixture would prove nothing.
out=$(GITHUB_REF_NAME=main SUMMRISE_MAIN_SHAPE_REPO="$T/shallow" \
      cargo test --manifest-path "$MANIFEST" -p summrise-agent --test main_shape -- --nocapture 2>&1)
rc=$?
if [ "$rc" = 0 ]; then
  echo "  ok   the gate passes in a shallow clone at a merge — $(printf '%s' "$out" | grep -m1 'main-shape:' | head -c 88)"
else
  echo "  FAIL the gate refused a real merge in a shallow clone (exit $rc):" >&2
  printf '%s\n' "$out" | tail -20 | sed 's/^/         /' >&2
  echo "  This is the CI failure of 2026-09-26: \`git rev-list --parents\` cannot see parents across a shallow graft." >&2
  exit 1
fi

# 4b. AND IT MUST REFUSE THE OTHER SHAPE IN THE SAME CLONE, or step 4 could be passing for the wrong reason.
#     A gate that says yes to everything passes step 4 too; this is the "must not bite" pair the Rust gate's
#     fixture table carries, re-run against the REAL clone rather than a table.
git -C "$T/shallow" checkout -q -b direct 2>/dev/null || true
if [ "$(git -C "$T/shallow" rev-parse --abbrev-ref HEAD)" = "direct" ]; then
  git -C "$T/shallow" commit -q --allow-empty -m "a direct commit on a branch"
  out2=$(GITHUB_REF_NAME=direct SUMMRISE_MAIN_SHAPE_REPO="$T/shallow" \
         cargo test --manifest-path "$MANIFEST" -p summrise-agent --test main_shape -- --nocapture 2>&1)
  if [ $? = 0 ]; then
    echo "  ok   a branch at one parent is accepted in the same clone (the must-not-bite case)"
  else
    echo "  FAIL a plain branch was refused in the shallow clone" >&2
    printf '%s\n' "$out2" | tail -12 | sed 's/^/         /' >&2
    exit 1
  fi
fi

echo "main-shape-shallow: 1 case — the clone CI actually gives, which no full clone can reproduce"
exit 0
