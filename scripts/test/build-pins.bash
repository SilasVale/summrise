#!/usr/bin/env bash
# ── THE MUTATION THAT MUST FAIL THIS GATE (moved here from the ledger table, landing 4b) ──
# Read this when you change this file: the mutation is how you find out whether the gate can still
# fail at all. A gate that cannot be broken is worse than no gate.
#
# MUTATION: bump rust-toolchain's channel alone
# RESULT:   exit 1, names the workflow literal

# build-pins.bash — the four build inputs that are HAND-COPIED, each with a
# single source of truth and NOTHING comparing them (round 128).
#
#   1. rust-toolchain.toml's `channel` vs every `toolchain:` literal in the two
#      workflows. Five copies, maintained by hand. The drift it would cause is
#      UNDETECTABLE by the release audit: that audit tolerates a differing
#      summrise-agent.exe ("differs by TOOLCHAIN (expected)"), so bumping the .toml
#      alone builds a different exe on this box and CI, both compile, and the
#      audit still says OK.
#   2. release.yml's cargo-xwin pin vs ci.yml's installs. CI was installing
#      whatever is newest that day while the release pinned 0.23.0 — the pin's
#      own comment names that exact drift.
#   3. agent/summrise-agent-npm/package.json's `files[]` (the real source of truth
#      for what the tgz carries) vs the three hand-maintained content gates.
#      One omission is DOCUMENTED IN THE WORKFLOW ITSELF: the xwin-less CI job
#      cannot contain summrise-agent.exe, and says so in its step name.
  #   4. the TYPESCRIPT version, three hand-copied literals and TWO versions (round 106).
  #      The freshness gate compares a tsc's OUTPUT against a committed file byte for byte, and the
  #      binary it uses is installed by the electron step above it. That install said `typescript@5`
  #      — a MOVING major — while the package's build script said the same, and the committed bin came
  #      from 5.9.3. Two tsc versions emit different JavaScript from identical TypeScript, so the gate
  #      failed a commit that was CORRECT, in BOTH workflows. A byte-for-byte comparison needs ONE
  #      compiler, and this is what holds the three places that name it together.
#
# Run: bash scripts/test/build-pins.bash
set -euo pipefail
cd "$(dirname "$0")/../.."

PASS=0
check() { # check <desc> <actual> <expected>
  if [ "$2" = "$3" ]; then PASS=$((PASS+1)); else
    echo "FAIL: $1"; echo "  actual:   $2"; echo "  expected: $3"; exit 1
  fi
}
has() { # has <desc> <haystack> <needle>
  case "$2" in *"$3"*) PASS=$((PASS+1));; *) echo "FAIL: $1"; echo "  looked for: $3"; echo "  in: $2"; exit 1;; esac
}

# ── 1. the toolchain, five hand-copied literals ──────────────────────────────
CHANNEL="$(grep -oP '^channel\s*=\s*"\K[^"]+' rust-toolchain.toml)"
[ -n "$CHANNEL" ] || { echo "FAIL: rust-toolchain.toml carries no channel pin"; exit 1; }
while IFS= read -r line; do
  wf="${line%%:*}"
  lit="$(sed -E 's/.*toolchain:[[:space:]]*//' <<<"${line#*:}")"
  check "toolchain literal in $wf matches rust-toolchain.toml ($CHANNEL)" "$lit" "$CHANNEL"
done < <(grep -h "toolchain: " .github/workflows/ci.yml .github/workflows/release.yml | sed 's/^/x:/' >/dev/null; grep -n "toolchain: " .github/workflows/ci.yml .github/workflows/release.yml)

# ── 2. cargo-xwin: the release pins it, so CI must install the same ──────────
XWIN="$(grep -oP 'cargo install cargo-xwin.*--version \K[0-9.]+' .github/workflows/release.yml | head -1)"
[ -n "$XWIN" ] || { echo "FAIL: release.yml no longer pins cargo-xwin at all"; exit 1; }
while IFS= read -r l; do
  has "every ci.yml cargo-xwin install carries the release's pin (--version $XWIN)" "$l" "--version $XWIN"
done < <(grep -h "cargo install cargo-xwin" .github/workflows/ci.yml)

# ── 3. the shipped-file list vs the ONE owner, and the builders that read it ─
FILES="$(node -p "require('./agent/summrise-agent-npm/package.json').files.join('\n')")"
[ -n "$FILES" ] || { echo "FAIL: agent/summrise-agent-npm/package.json declares no files[]"; exit 1; }
# THE MANY OWNERS BECAME ONE (architecture round C4). The packed-tgz content list stood verbatim in
# three builders and this gate compared their TEXT to package.json's files[]; both builders now read
# agent/summrise-agent-npm/required-in-tgz.txt, so the question that still has teeth is the one
# below: does the ONE owner name every file the package declares? It also catches the drift the old
# shape could not — a file ADDED to files[] and never added to the required list.
OWNER="agent/summrise-agent-npm/required-in-tgz.txt"
OWNER_TEXT="$(cat "$OWNER" 2>/dev/null)"
[ -n "$OWNER_TEXT" ] || { echo "FAIL: the packed-tgz content list is missing or empty: $OWNER"; exit 1; }
while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  case "$entry" in */) continue ;; esac   # a directory in files[] is covered by the files under it
  has "$OWNER names $entry (package.json declares it)" "$OWNER_TEXT" "$entry"
done <<<"$FILES"
# AND EVERY BUILDER MUST READ IT rather than restate it: a restated copy is exactly the defect C4
# removed, and it is what let the local gate and the CI gate drift apart.
for path in scripts/publish-release.sh .github/workflows/release.yml .github/workflows/ci.yml; do
  has "$path reads the one owner" "$(cat "$path")" "required-in-tgz.txt"
done
check "the documented exception is still documented" \
  "$(grep -c 'no exe in this job' .github/workflows/ci.yml)" "1"

# EVERY test file must be WIRED (round 146). This check exists because round 145
# wrote scripts/test/publish-release.bash and never added it to ci.yml, so the new
# test would have run for exactly one person, once. The directory listing and the
# CI step list are two things that must agree with NOTHING comparing them — the
# same shape this loop has closed in the audit, the smoke and the packaging pins.
# A test no workflow invokes is a check whose failure cannot fail the run.
unwired=""
for f in scripts/test/*; do
  base=$(basename "$f")
  case "$base" in
    *.bash|*.mjs|*.py) ;;
    *) continue ;;
  esac
  grep -q "scripts/test/$base" .github/workflows/ci.yml || unwired="$unwired $base"
done
check "every scripts/test file is invoked from ci.yml" "${unwired:-none}" "none"

# ── 4. and a step that CAN silently test nothing is the same defect one step in ──
# Round 146's rule (above) covers a test no workflow invokes. This is its sibling: a step the workflow DOES invoke that
# can decide at run time to test nothing and still exit 0 — a green that means nothing, which is the class of defect
# `panel-audit-skip-check.mjs` exists for on the panel side. The installer's PowerShell logic cannot run on the Linux
# box, so its step is the one place this could happen; round 40 of the standing goal turned its missing-pwsh branch from
# a ::warning:: into a failure, and this keeps it that way.
PWSH_BLOCK="$(awk '/installer integrity tests \(pwsh\)/{p=1} p{print} p && /SummriseIntegrity.tests.ps1$/{exit}' .github/workflows/ci.yml)"
[ -n "$PWSH_BLOCK" ] || { echo "FAIL: no installer-integrity step found in ci.yml"; exit 1; }
has "the pwsh step still runs the integrity tests" "$PWSH_BLOCK" "SummriseIntegrity.tests.ps1"
has "a missing pwsh FAILS the step" "$PWSH_BLOCK" "exit 1"
if grep -q '::warning::pwsh is not installed' .github/workflows/ci.yml; then
  echo "FAIL: the pwsh step warns and continues again — that green means the installer's logic was never tested"
  exit 1
fi

# ── 5. the TypeScript pin, three hand-copied literals ─────────────────────────
TS_PIN="$(node -p 'require("./agent/summrise-agent-npm/package.json").scripts.build' | grep -oP 'typescript@\K[0-9][0-9.]*' | head -1)"
[ -n "$TS_PIN" ] || { echo "FAIL: the package's build script names no typescript version"; exit 1; }
case "$TS_PIN" in
  *.*.*) PASS=$((PASS + 1)) ;;
  *) echo "FAIL: the build script pins '\''$TS_PIN'\'' — a moving major cannot hold a byte-for-byte comparison"; exit 1 ;;
esac
for wf in .github/workflows/ci.yml .github/workflows/release.yml; do
  # Every literal that compiles this package's TypeScript must name that same version. A bare major
  # (`typescript@5`) is what round 106 removed, and it is what this catches if it comes back.
  while IFS= read -r lit; do
    [ -n "$lit" ] || continue
    check "typescript literal in $wf matches the build script ($TS_PIN)" "$lit" "$TS_PIN"
  done < <(grep -oP 'typescript@\K[0-9][0-9.]*' "$wf" | sort -u)
done

# ── 5. the LLVM the builders compile with: one tarball, and nothing from apt ──
# THE SAME SPECIES AS #2, AND IT HAD ALREADY BITTEN (round 193). ci.yml s agent-windows-target ran
# `apt-get install -y llvm` while release.yml refuses that exact package: "those are per-distro builds,
# and the resulting lld laid .data out differently (measured on 1.2.317 — a 32-byte shift plus a 0x100
# difference in where rust_panic landed)". The job pinned cargo-xwin against build drift in the step
# below and took the compiler from the distribution. Nothing compared them; this does.
LLVM_KEY="$(grep -oP "key: \Kllvm-[0-9.]+-official[^ ]*" .github/workflows/release.yml | head -1)"
[ -n "$LLVM_KEY" ] || { echo "FAIL: release.yml no longer names an official LLVM tarball"; exit 1; }
LLVM_TAR="$(grep -oP "llvmorg-\K[0-9.]+" .github/workflows/release.yml | head -1)"
[ -n "$LLVM_TAR" ] || { echo "FAIL: release.yml no longer pins an LLVM release"; exit 1; }
for wf in .github/workflows/ci.yml .github/workflows/release.yml; do
  has "$wf takes its LLVM from the official tarball ($LLVM_KEY)" "$(cat "$wf")" "$LLVM_KEY"
  has "$wf pins the same LLVM release (llvmorg-$LLVM_TAR)" "$(cat "$wf")" "llvmorg-$LLVM_TAR"
  # A DISTRIBUTION LLVM IS THE DRIFT ITSELF, and a version check cannot see it: the distro package
  # carries whatever version that image ships. The ban is the assertion.
  if grep -qE "apt-get install[^\n]*[[:space:]]llvm([[:space:]]|$)" "$wf"; then
    echo "FAIL: $wf installs llvm from the distribution — release.yml refuses that package by name as a"
    echo "  measured source of build drift (a 32-byte .data shift on 1.2.317). Use the official tarball."
    exit 1
  fi
  PASS=$((PASS+1))
done

# ── 6. wrangler: the tool that PUBLISHES the workers, on a moving major until now ──
# Same species as #2 and #5 (round 195). CI installed `wrangler@4` and dry-ran every proxy with it while
# the deploy box ran 4.127.0 — ten minor versions apart when this was written. The pin lives in ci.yml
# and build.sh names the same version in its install message; this holds the two together and refuses a
# bare major, because `@4` is precisely the drift.
WRANGLER="$(grep -oP "wrangler@\K[0-9]+\.[0-9]+\.[0-9]+" .github/workflows/ci.yml | head -1)"
[ -n "$WRANGLER" ] || { echo "FAIL: ci.yml no longer pins wrangler to an exact version"; exit 1; }
has "build.sh names the same wrangler ($WRANGLER) in its install message" "$(cat scripts/build.sh)" "wrangler@$WRANGLER"
# COMMENTS ARE STRIPPED FIRST, and this gate learned it by firing on its own explanation (round 195): the
# step s comment QUOTES the removed `wrangler@4` to say why it is gone, and the raw scan read that as the
# drift. The repo has the rule already — retired-colours-check and css-vars-check both strip comments, and
# the ledger keeps the reason: a gate that deletes its reasons is worse than no gate.
for f in .github/workflows/ci.yml .github/workflows/release.yml scripts/build.sh; do
  if sed "s/#.*$//" "$f" | grep -qE "wrangler@[0-9]+([^.]|$)"; then
    echo "FAIL: $f names a BARE wrangler major — that resolves to whatever is newest that day, which is"
    echo "  the drift this pin exists to stop. Pin the exact version ($WRANGLER today)."
    exit 1
  fi
  PASS=$((PASS+1))
done

echo "build-pins: $PASS checks passed"
