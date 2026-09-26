#!/usr/bin/env bash
# ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
# Read this when you change this file: the mutation is how you find out whether the gate can still
# fail at all. A gate that cannot be broken is worse than no gate.
#
# MUTATION: disable the stale-exe refusal
# RESULT:   exit 1 — **after round 67 ADDED the case that does it**

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
stray=$(find index/public/summrise-agent -name 'summrise-agent-*.tgz' -newermt '-2 minutes' 2>/dev/null | head -3)
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
#    (`.gitignore: agent/summrise-agent-npm/*.exe`), so the pack-chain job — "npm artifact gates, NO exe",
#    by its own name — has no such file, and the first version of this case failed there with "the
#    staged exe is missing". A missing fixture it needs is this case's business to supply, not a
#    reason to skip: only the MTIME matters to the check under test.
#    BOTH exes, because the script's refusals are ordered and BOTH paths are git-ignored build output
#    that a fresh checkout lacks — the pack-chain job has neither. The first draft supplied only the
#    staged one and CI refused on the cross-compile output before ever reaching the check under test;
#    the second would have skipped, which is how a case stops running without anyone noticing.
EXE="agent/summrise-agent-npm/summrise-agent.exe"
BUILD_EXE="agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe"
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
WANT_VERSION=$(python3 -c "import json;print(json.load(open('agent/summrise-agent-npm/package.json'))['version'])")
# --acknowledge-unreconciled: the RECONCILE gate is not what this case asserts, and
# it fires first whenever the CDN carries a version whose GitHub asset is missing or
# packages a different tree (1.2.453 does, on purpose and on the record). Without the
# flag the case reads that refusal and reports ITSELF as failed -- which is how this
# suite sat at 8/10 for two checks that belonged to a state it does not own. The flag
# is the documented escape ("this run ADDS to the ledger"), so the case reaches the
# exe check it is actually about.
out=$(bash scripts/publish-release.sh "$WANT_VERSION" --acknowledge-unreconciled 2>&1); rc=$?
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

if pack_input_mode_verdict "$PWD" "agent/summrise-agent-npm" >/tmp/mode.out 2>&1; then
  ok "this checkout's pack inputs match a fresh checkout"
else
  bad "this checkout's pack inputs DO NOT match: $(cat /tmp/mode.out)"
fi

# A fixture whose recorded mode is 0644 but whose worktree mode is 0600 must be
# REFUSED, and the refusal must NAME the file — a verdict without the filename
# tells the operator nothing.
FIX_TMP=$(mktemp -d)
git -C "$FIX_TMP" init -q
mkdir -p "$FIX_TMP/agent/summrise-agent-npm/bin"
printf 'x\n' > "$FIX_TMP/agent/summrise-agent-npm/README.md"
git -C "$FIX_TMP" add -A
git -C "$FIX_TMP" -c user.email=t@t -c user.name=t commit -qm fixture
chmod 600 "$FIX_TMP/agent/summrise-agent-npm/README.md"
FIX_OUT=$(pack_input_mode_verdict "$FIX_TMP" "agent/summrise-agent-npm" 2>&1); FIX_RC=$?
if [ "$FIX_RC" -ne 0 ] && grep -q 'README.md' <<<"$FIX_OUT"; then
  ok "a 0600 pack input is refused BY NAME"
else
  bad "mode drift not refused: rc=$FIX_RC out=$(head -c 200 <<<"$FIX_OUT")"
fi
chmod 644 "$FIX_TMP/agent/summrise-agent-npm/README.md"
if pack_input_mode_verdict "$FIX_TMP" "agent/summrise-agent-npm" >/dev/null 2>&1; then
  ok "the same fixture passes once its mode is corrected"
else
  bad "corrected mode still refused"
fi
rm -rf "$FIX_TMP"

# D7: --npm without a credential must refuse BEFORE anything is packed, deployed
# or pruned, and the message must name where the token goes. Asserting only
# "non-zero" would pass if the script died later for an unrelated reason, so the
# MESSAGE is the evidence the guard ran (round 124's lesson: a check a removal
# can leave green proves nothing).
VER_NOW=$(node -p "require('./agent/summrise-agent-npm/package.json').version")
out=$(env -u NPM_TOKEN HOME=/nonexistent bash scripts/publish-release.sh "$VER_NOW" --npm --acknowledge-unreconciled 2>&1); rc=$?
if [ "$rc" -ne 0 ] && grep -q 'no token' <<<"$out" && grep -q 'NPM_TOKEN' <<<"$out"; then
  ok "--npm with no token refuses early, and names NPM_TOKEN / ~/.npm-token"
else
  bad "--npm with no token: rc=$rc out=$(head -c 200 <<<"$out")"
fi

# And WITHOUT --npm the run must say out loud that the registry was not updated.
# Silence is the drift the flag exists to prevent: the CDN would move, the
# registry would keep serving the placeholder, and 'npx summrise-agent' would
# keep installing nothing. Cannot be exercised end-to-end without a release, so
# the branch's presence is the seam — a deletion fails this.
if grep -q 'the npm registry was NOT updated' scripts/publish-release.sh; then
  ok "without --npm the script carries the named warning (no silent drift)"
else
  bad "the not-published branch lost its warning"
fi

# ── C2, THE ARCHITECTURE ROUND: the order is data, and --dry-run is the seam ────────────────
# A release's order used to live in four places — the script's layout, its header comment,
# AGENTS.md, and cases HERE that asserted it by line number. The header had already drifted from
# the code once (it listed the audit before npm, which is the order a FIRST publish never
# survives). `SEQUENCE=` declares it now; this walks the EFFECT banners in the source and fails
# when they run in a different order.
#
# EXACT banner names, with an alias table for the decorated ones. The first version matched
# SUBSTRINGS and called a CORRECT script non-monotonic: "deploy" also appears in the installer's
# banner ("installer (self-contained, staged, no deploy yet)"). A check that lies about a good
# artifact is worse than no check — so the alias table is explicit, and the case below proves the
# check fails on a mutated copy.
seq_check() {
  python3 - "$1" <<'PY'
import re, sys
src = open(sys.argv[1]).read()
m = re.search(r'^SEQUENCE="([^"]+)"', src, re.M)
if not m:
    print("no SEQUENCE declaration"); sys.exit(1)
seq = m.group(1).split()
ALIAS = {"prune": "last-5-per-minor prune", "smoke": "post-publish smoke",
         "npm": "npm registry", "audit": "release audit"}
banners = []
for i, l in enumerate(src.split("\n")):
    b = re.match(r'echo "== (.+?) =="\s*$', l.strip())
    if b:
        banners.append((i, b.group(1)))
pos = []
for tok in seq:
    want = ALIAS.get(tok, tok)
    hit = [i for i, n in banners if n == want or n.startswith(want + " ") or n.startswith(want + ":")]
    if not hit:
        print("no banner for step %r" % tok); sys.exit(1)
    pos.append((tok, hit[0]))
if [p for _, p in pos] != sorted(p for _, p in pos):
    print("the source runs these in a DIFFERENT order than SEQUENCE declares: %s" % pos); sys.exit(1)
print("sequence ok: %s" % " ".join(t for t, _ in pos))
PY
}

if out=$(seq_check scripts/publish-release.sh); then
  ok "the declared SEQUENCE matches the order the source actually runs ($out)"
else
  bad "the sequence check failed on the real script: $out"
fi

# AND IT MUST BE ABLE TO FAIL, or it is decoration. MOVE a banner line, so both banners still
# exist and ONLY the order changes — and require the refusal to NAME THE ORDER.
# THE FIRST VERSION OF THIS CASE RENAMED one banner instead of moving it: the check then failed
# with "no banner for step 'deploy'", which is the PRESENCE rule, not the ordering rule, and the
# case passed while proving something else. A mutation aimed at the wrong property is evidence
# about the mutation first.
MUT=$(mktemp)
python3 - scripts/publish-release.sh "$MUT" <<'PY'
import sys
src, dst = sys.argv[1], sys.argv[2]
lines = open(src).read().split("\n")
d = next(i for i, l in enumerate(lines) if l.strip() == 'echo "== deploy =="')
n = next(i for i, l in enumerate(lines) if l.strip() == 'echo "== npm registry =="')
lines[d], lines[n] = lines[n], lines[d]          # both survive; the order does not
open(dst, "w").write("\n".join(lines))
PY
if out=$(seq_check "$MUT" 2>&1); then
  bad "the sequence check PASSED a script with deploy and npm swapped — it proves nothing"
elif grep -q 'DIFFERENT order' <<<"$out"; then
  ok "the sequence check bites on ORDER (not presence) when two effect banners trade places"
else
  bad "the mutation was caught for the wrong reason: $(head -c 90 <<<"$out")"
fi
rm -f "$MUT"

# --dry-run: EVERY GATE, NOTHING CHANGED. This is the seam the round added — the 30 refusals are
# this module's real interface, the whole gate block needs no credential, and it used to sit
# inline above the first effect, where no test could reach it.
#
# THE ASSERTION IS THE INVARIANT, NOT THE VERDICT: on CI there is no staged exe, so a dry run
# there MUST refuse. What holds everywhere is that it either passed or refused BY A GATE, and
# that it left the worktree exactly as it found it.
TREE_B4=$(git status --porcelain --untracked-files=no | sort)
out=$(timeout 300 bash scripts/publish-release.sh 1.2.999 --dry-run --acknowledge-unreconciled 2>&1); rc=$?
if [ "$rc" -ne 0 ] && grep -q 'package.json version is' <<<"$out"; then
  ok "--dry-run runs the version gate and refuses a version package.json does not carry"
else
  bad "--dry-run with a wrong version: rc=$rc out=$(head -c 200 <<<"$out")"
fi

REAL_VER=$(python3 -c "import json;print(json.load(open('agent/summrise-agent-npm/package.json'))['version'])")
out=$(timeout 600 bash scripts/publish-release.sh "$REAL_VER" --dry-run --acknowledge-unreconciled 2>&1); rc=$?
TREE_AF=$(git status --porcelain --untracked-files=no | sort)
if [ "$TREE_B4" = "$TREE_AF" ]; then
  ok "a dry run changes NOTHING (rc=$rc)"
else
  bad "the dry run changed the worktree: $(diff <(printf '%s\n' "$TREE_B4") <(printf '%s\n' "$TREE_AF") | head -3 | tr '\n' ' ')"
fi
if [ "$rc" -eq 0 ]; then
  if grep -q '== dry run ==' <<<"$out" && grep -q 'sequence: pack stage prune deploy smoke npm audit' <<<"$out"; then
    ok "and when every gate passes it exits 0 with the sequence printed (no credentials used)"
  else
    bad "dry run exited 0 without its verdict block"
  fi
fi

# ── C3, THE ARCHITECTURE ROUND: the reconcile ledger is an INPUT, and its contract is tested
# through its interface. `RECONCILE_LEDGER` has been overridable in release-lib.sh since the gate
# was written, and NOTHING EVER OVERRODE IT — the seam existed and stayed hypothetical, so the one
# piece of state that can refuse a publish was only ever observed against the real, gitignored,
# machine-local file. A reviewer reading the code concluded the gate was dead code; it refused a
# publish on 2026-09-23. The cases below drive the REAL script with a FIXTURE ledger.
LED=$(mktemp)
printf '# fixture\n9.9.99 2026-01-01T00:00:00Z published with no GitHub release\n' > "$LED"
out=$(RECONCILE_LEDGER="$LED" timeout 300 bash scripts/publish-release.sh 9.9.99 2>&1); rc=$?
if [ "$rc" -ne 0 ] && grep -q 'refusing to publish' <<<"$out" && grep -q '9\.9\.99' <<<"$out"; then
  ok "a pending version REFUSES the publish, and the refusal names it (fixture ledger)"
else
  bad "the reconcile gate did not fire for a fixture ledger: rc=$rc $(head -c 200 <<<"$out")"
fi
# THE OTHER SIDE, which is what makes the case above evidence rather than decoration: an EMPTY
# ledger must let the run continue to the NEXT gate. Without it, "the gate fired" could be any
# refusal at all.
EMPTY=$(mktemp)
printf '# empty fixture\n' > "$EMPTY"
out=$(RECONCILE_LEDGER="$EMPTY" timeout 600 bash scripts/publish-release.sh 1.2.999 --dry-run 2>&1)
if ! grep -q 'refusing to publish' <<<"$out" && grep -q 'package.json version is' <<<"$out"; then
  ok "an empty ledger does not refuse — the run reaches the NEXT gate (a control, not decoration)"
else
  bad "an empty ledger changed the outcome: $(head -c 200 <<<"$out")"
fi
# AND THE ACKNOWLEDGEMENT, the documented escape, carries it past.
out=$(RECONCILE_LEDGER="$LED" timeout 600 bash scripts/publish-release.sh 1.2.999 --acknowledge-unreconciled --dry-run 2>&1)
if ! grep -q 'refusing to publish' <<<"$out"; then
  ok "--acknowledge-unreconciled carries the run past a pending ledger entry"
else
  bad "the acknowledgement did not clear the gate"
fi
rm -f "$LED" "$EMPTY"

# ── C4, THE ARCHITECTURE ROUND: the packed-tgz content list has ONE owner, and it cannot be
# emptied in silence. It used to be written out verbatim in scripts/publish-release.sh AND
# .github/workflows/release.yml — two owners of one fact as long as nobody edits it, and two
# DIFFERENT release gates the moment somebody does. Both read this file now, which is exactly why
# DELETING or TRUNCATING it would disable the gate in both builders at once.
REQ="agent/summrise-agent-npm/required-in-tgz.txt"
req_n=$(grep -vcE '^[[:space:]]*(#|$)' "$REQ" 2>/dev/null || echo 0)
if [ -f "$REQ" ] && [ "$req_n" -ge 5 ] && grep -qx 'summrise-agent\.exe' "$REQ" && grep -qx 'bin/summrise\.js' "$REQ"; then
  ok "the packed-tgz content list has $req_n entries, with the exe and the CLI among them"
else
  bad "the packed-tgz content list is missing, empty, or lacks the exe/CLI: $REQ"
fi
# AND NEITHER BUILDER RESTATES IT. This is a source assertion, and it is the honest kind: the
# property IS about the source (a second copy is the defect), and a behavioural test cannot see a
# copy that happens to agree today.
if grep -q 'required-in-tgz.txt' scripts/publish-release.sh \
   && grep -q 'required-in-tgz.txt' .github/workflows/release.yml \
   && ! grep -q '"summrise-agent.exe" \\' scripts/publish-release.sh \
   && ! grep -q '"summrise-agent.exe" \\' .github/workflows/release.yml; then
  ok "both builders derive the list from the one owner (no restated copy in either)"
else
  bad "the packed-tgz list is restated in a builder again"
fi

printf '\npublish-release: %d checks passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
