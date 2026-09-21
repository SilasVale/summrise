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

TREE_BEFORE=$(git status --porcelain --untracked-files=no | sort)

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
#    side effect is not a guard: assert the run left the worktree as it FOUND it.
#    THE FIRST VERSION REQUIRED A CLEAN TREE and blamed the refusal for any dirt — it failed on a checkout carrying an
#    unrelated operator edit while naming the wrong cause, and could not reproduce in CI because a CI checkout is clean.
TREE_AFTER=$(git status --porcelain --untracked-files=no | sort)
if [ "$TREE_BEFORE" = "$TREE_AFTER" ]; then
  ok "the refusal left the worktree as it found it"
else
  bad "the refusal changed the worktree: $(diff <(printf '%s\n' "$TREE_BEFORE") <(printf '%s\n' "$TREE_AFTER") | head -5 | tr '\n' ' ')"
fi

# 3. The script must not reach wrangler/npm on a refusal — the whole point of a
#    fail-closed entry is that no build starts. Detected by effect: a refusal that
#    had packed anything would have created a tgz under index/public.
stray=$(find index/public/vale-agent -name 'vale-agent-*.tgz' -newermt '-2 minutes' 2>/dev/null | head -3)
[ -z "$stray" ] && ok "no tgz was packed by the refusals" || bad "packed during a refusal: $stray"

# 4. THE STALE-EXE REFUSAL (round 67). Found by MUTATION, not by reading: with the refusal's
#    `[ "$EXE_TS" -lt "$SRC_TS" ]` turned into `if false`, this whole file still exited 0. The check
#    that stopped three publishes in a single session had no coverage here at all — it was proven in
#    production and nowhere else, which is exactly the kind of guard a refactor can delete quietly.
#
#    The setup moves the STAGED exe's mtime into the past rather than committing anything: the gate
#    asserts a clean worktree a few lines above, so a mutation (or a fixture) that dirties the tree
#    is rejected before it can prove anything.
#    The setup CREATES the exe when a checkout has none: it is git-ignored build output
#    (`.gitignore: agent/vale-agent-npm/*.exe`), so the pack-chain job — "npm artifact gates, NO exe",
#    by its own name — has no such file, and the first version of this case failed there with "the
#    staged exe is missing". A missing fixture it needs is this case's business to supply, not a
#    reason to skip: only the MTIME matters to the check under test.
#    BOTH exes, because the script's refusals are ordered and BOTH paths are git-ignored build output
#    that a fresh checkout lacks — the pack-chain job has neither. The first draft supplied only the
#    staged one and CI refused on the cross-compile output before ever reaching the check under test;
#    the second would have skipped, which is how a case stops running without anyone noticing.
EXE="agent/vale-agent-npm/vale-agent.exe"
BUILD_EXE="agent/target/x86_64-pc-windows-msvc/release/vale-agent.exe"
CREATED_STAGED=0
CREATED_BUILD=0
if [ ! -f "$EXE" ]; then : > "$EXE"; CREATED_STAGED=1; fi
if [ ! -f "$BUILD_EXE" ]; then mkdir -p "$(dirname "$BUILD_EXE")"; : > "$BUILD_EXE"; CREATED_BUILD=1; fi
SAVED_MTIME=$(stat -c %Y "$EXE")
touch -d '2020-01-01' "$EXE"
# The cross-compile output too: it is an INPUT to the check under test, and a 2020 mtime on one side
# is what the refusal compares.
SAVED_BUILD_MTIME=$(stat -c %Y "$BUILD_EXE")
touch -d '2020-01-01' "$BUILD_EXE"
# The REAL version: with a bogus one the script refuses on the version gate first and this case
# would prove nothing about the exe check (measured — that is exactly what the first draft did).
WANT_VERSION=$(python3 -c "import json;print(json.load(open('agent/vale-agent-npm/package.json'))['version'])")
out=$(bash scripts/publish-release.sh "$WANT_VERSION" 2>&1); rc=$?
touch -d "@$SAVED_BUILD_MTIME" "$BUILD_EXE"
[ "$CREATED_BUILD" = "1" ] && rm -f "$BUILD_EXE"
if [ "$CREATED_STAGED" = "1" ]; then rm -f "$EXE"; else touch -d "@$SAVED_MTIME" "$EXE"; fi
if [ "$rc" -ne 0 ] && grep -q 'predates the newest exe-input commit' <<<"$out"; then
  ok "a stale staged exe is refused, and the message names the reason"
else
  bad "stale exe: rc=$rc out=$(head -c 200 <<<"$out")"
fi

printf '\npublish-release: %d checks passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]

# ---------------------------------------------------------------------------
# THE PACK-INPUT MODE GATE (round 154). It lived mid-chain in the orchestrator,
# behind the reconcile gate, the version check and the exe check, so no test
# could reach it — which is why a gate whose absence caused twenty consecutive
# "packaging metadata" WARNs had none. Extracted into release-lib.sh, it has
# behavioural cases now.
# The gate is ALSO reachable from the script's own entry point now (round 155):
# `--check-modes-only` runs it and exits, so the assertion below can drive the
# REAL script rather than the sourced function. Both are checked: the function for
# behaviour, the entry point for reachability — a gate no entry point can run is
# the state this round exists to leave.
if out=$(bash scripts/publish-release.sh --check-modes-only 2>&1); then
  grep -q "pack input modes match a fresh checkout OK" <<<"$out" \
    && ok "--check-modes-only reaches the gate and reports OK" \
    || bad "entry point ran but did not report: $(head -c 120 <<<"$out")"
else
  bad "--check-modes-only refused on this tree: $(head -c 200 <<<"$out")"
fi

source scripts/lib/release-lib.sh

if pack_input_mode_verdict "$PWD" "agent/vale-agent-npm" >/tmp/mode.out 2>&1; then
  ok "this checkout's pack inputs match a fresh checkout"
else
  bad "this checkout's pack inputs DO NOT match: $(cat /tmp/mode.out)"
fi

# A fixture whose recorded mode is 0644 but whose worktree mode is 0600 must be
# REFUSED, and the refusal must NAME the file — a verdict without the filename
# tells the operator nothing.
FIX_TMP=$(mktemp -d)
git -C "$FIX_TMP" init -q
mkdir -p "$FIX_TMP/agent/vale-agent-npm/bin"
printf 'x\n' > "$FIX_TMP/agent/vale-agent-npm/README.md"
git -C "$FIX_TMP" add -A
git -C "$FIX_TMP" -c user.email=t@t -c user.name=t commit -qm fixture
chmod 600 "$FIX_TMP/agent/vale-agent-npm/README.md"
FIX_OUT=$(pack_input_mode_verdict "$FIX_TMP" "agent/vale-agent-npm" 2>&1); FIX_RC=$?
if [ "$FIX_RC" -ne 0 ] && grep -q 'README.md' <<<"$FIX_OUT"; then
  ok "a 0600 pack input is refused BY NAME"
else
  bad "mode drift not refused: rc=$FIX_RC out=$(head -c 200 <<<"$FIX_OUT")"
fi
chmod 644 "$FIX_TMP/agent/vale-agent-npm/README.md"
if pack_input_mode_verdict "$FIX_TMP" "agent/vale-agent-npm" >/dev/null 2>&1; then
  ok "the same fixture passes once its mode is corrected"
else
  bad "corrected mode still refused"
fi
rm -rf "$FIX_TMP"

printf '\npublish-release: %d checks passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
