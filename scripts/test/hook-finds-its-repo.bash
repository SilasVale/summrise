#!/usr/bin/env bash
# hook-finds-its-repo — THE pre-commit HOOK MUST RESOLVE ITS OWN REPOSITORY, NOT THE PATH IT WAS INVOKED BY.
#
# MEASURED 2026-09-26, and this is the largest silent failure found in this repository. `core.hooksPath` here is `.githooks`,
# so GIT INVOKES THE HOOK AS `.githooks/pre-commit` and `$0` is that path — not the symlink's target. The hook's first act
# was:
#
#     cd "$(dirname "$0")/../.." 2>/dev/null || cd "$(git rev-parse --show-toplevel)"
#
# From `.githooks/pre-commit` that is **/home/zhengsaisi — ONE LEVEL ABOVE THE REPOSITORY**. The `cd` SUCCEEDED at the wrong
# place, so the `||` fallback never fired, the guard found no `agent/scripts/panel-design-sweep.mjs`, and the hook exited 0 on
# its fifth line. **NOTHING IT CHECKS HAD RUN** — not the five emitters, not the archive-only rule — for every commit since
# the hook was installed.
#
# AND THE PROOF RECORDED FOR IT COULD NOT HAVE CAUGHT IT: AGENTS.md says it was "proven by making an empty commit and watching
# it run". A commit that SUCCEEDS looks exactly like a hook that ran and passed. **The only proof of a check is watching it
# REFUSE something**, which is why this file asserts the opposite direction: with `$0` set the way git sets it, the hook must
# NOT take its inert path.
#
# `SUMMRISE_HOOK_DEBUG=1` is what makes that exit observable — it is silent in normal use, because it fires in every OTHER
# repository on this machine and a line of noise per commit there would be its own defect.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
REPO="$PWD"
HOOK="$REPO/scripts/hooks/pre-commit"
[ -f "$HOOK" ] || { echo "  no scripts/hooks/pre-commit — run from the repo" >&2; exit 2; }

D="$REPO/.githooks-probe"
rm -rf "$D"
mkdir -p "$D"
ln -sf ../scripts/hooks/pre-commit "$D/pre-commit"
trap 'rm -rf "$D"' EXIT

# GIT RUNS HOOKS WITH cwd AT THE WORK-TREE ROOT AND `$0` AS THE hooksPath-RELATIVE PATH. Reproduce exactly that — a test
# that invokes the hook by its real path cannot see this defect at all, which is why the earlier proof did not.
out=$(cd "$REPO" && SUMMRISE_HOOK_DEBUG=1 bash .githooks-probe/pre-commit 2>&1)
rc=$?

if printf '%s' "$out" | grep -q 'not the Summrise repo'; then
  echo "  FAIL the hook took its INERT path when invoked as \`.githooks-probe/pre-commit\`:" >&2
  printf '%s\n' "$out" | sed 's/^/         /' >&2
  echo "  It resolved its repository from \$0 instead of asking git, so NOTHING it checks runs on a real commit." >&2
  exit 1
fi

echo "  ok   invoked as \`.githooks-probe/pre-commit\` the hook resolved its own repository and reached its checks (exit $rc)"
echo "hook-finds-its-repo: 1 case — the invocation path git actually uses, which the old proof never reproduced"
exit 0
