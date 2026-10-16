#!/usr/bin/env bash
# local-gates — THE PRE-PUSH GATE'S MAPPING, PROVEN ON FIXTURES IN BOTH DIRECTIONS.
#
# The hook's VALUE is not that it runs commands (a reader can see that); it is that it picks the RIGHT ones for what
# changed — **and a mapping is exactly the kind of thing that reads correctly and routes wrongly.** So the check is
# run in dry-run mode (`SUMMRISE_LOCAL_GATES_DRY=1`) against a scratch repository whose HEAD~1..HEAD diff is planted
# with one directory's change at a time, and each case asserts WHAT the hook would run.
#
# THE NEGATIVE DIRECTION IS TESTED AS CAREFULLY AS THE POSITIVE: a docs-only push must run NOTHING, because a hook
# that runs the whole list for a README is a hook people learn to bypass with --no-verify.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
HOOK="$PWD/scripts/hooks/local-gates"
[ -f "$HOOK" ] || { echo "  FAIL scripts/hooks/local-gates does not exist" >&2; exit 1; }

T=$(mktemp -d) || exit 2
trap 'rm -rf "$T"' EXIT
fail=0

# A scratch repository: one commit of nothing, then a planted change.
git init -q "$T/repo" || exit 2
cd "$T/repo" || exit 2
git config user.email t@t; git config user.name t
mkdir -p agent/src agent/resources/panel-react gateway/src docs proxies/mail
echo x > placeholder
git add -A && git commit -qm base
git branch -f main HEAD

plant() { # path -> commit it, so HEAD~1..HEAD carries exactly that path
  mkdir -p "$(dirname "$1")"
  echo "change" >> "$1"
  git add -A && git commit -qm "plant $1"
}

check() { # name, expect-substring-or-EMPTY
  local name="$1" want="$2" out
  out=$(SUMMRISE_LOCAL_GATES_ROOT="$T/repo" SUMMRISE_LOCAL_GATES_BASE=HEAD~1 SUMMRISE_LOCAL_GATES_DRY=1 \
        bash "$HOOK" 2>&1)
  if [ "$want" = "EMPTY" ]; then
    if printf '%s' "$out" | grep -q "would run"; then
      echo "  FAIL $name — nothing should run, got:"; printf '%s\n' "$out" | sed 's/^/         /'; fail=1
    else
      echo "  ok   $name (nothing runs)"
    fi
    return
  fi
  if printf '%s' "$out" | grep -qF "$want"; then
    echo "  ok   $name"
  else
    echo "  FAIL $name — expected to see '$want', got:"; printf '%s\n' "$out" | sed 's/^/         /'; fail=1
  fi
}

plant docs/README.md
check "a docs-only change runs NOTHING" EMPTY

plant gateway/src/index.ts
check "a gateway source change runs the gateway suite" "gateway: tests"

plant agent/src/vocabulary.rs
check "an agent source change runs rustfmt" "agent: rustfmt"
check "an agent source change runs clippy on the agent crate" "clippy -p summrise-agent"
check "an agent source change runs the agent crate's tests" "tests -p summrise-agent"

plant agent/resources/panel-react/src/lib/api.ts
check "a panel source change runs the panel's typecheck" "panel: typecheck"
check "a panel source change runs vitest" "panel: vitest"

plant gateway/wasm/src/mcp_tools.rs
check "a wasm crate change runs the crate's tests" "wasm: tests"
check "a wasm crate change runs the wasm32 clippy pass" "clippy (wasm32)"

plant gateway/wasm/verify.mjs
check "a harness change runs the byte comparisons" "byte comparisons (the harness)"

plant gateway/wasm/fixtures/models-corpus.json
check "a fixture change runs the byte comparisons too" "byte comparisons (the harness)"

plant core/summrise-core/src/lib.rs
check "a core change runs the shared core's gates" "core: tests"

plant agent/summrise-agent-core/src/lib.rs
check "a core crate change runs THAT crate's tests" "tests -p summrise-agent-core"

[ "$fail" = 0 ] && echo "local-gates: 13 case(s), routing proven in both directions"
exit "$fail"
