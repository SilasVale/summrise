#!/usr/bin/env bash
# build-pins.bash — the three build inputs that are HAND-COPIED, each with a
# single source of truth and NOTHING comparing them (round 128).
#
#   1. rust-toolchain.toml's `channel` vs every `toolchain:` literal in the two
#      workflows. Five copies, maintained by hand. The drift it would cause is
#      UNDETECTABLE by the release audit: that audit tolerates a differing
#      vale-agent.exe ("differs by TOOLCHAIN (expected)"), so bumping the .toml
#      alone builds a different exe on this box and CI, both compile, and the
#      audit still says OK.
#   2. release.yml's cargo-xwin pin vs ci.yml's installs. CI was installing
#      whatever is newest that day while the release pinned 0.23.0 — the pin's
#      own comment names that exact drift.
#   3. agent/vale-agent-npm/package.json's `files[]` (the real source of truth
#      for what the tgz carries) vs the three hand-maintained content gates.
#      One omission is DOCUMENTED IN THE WORKFLOW ITSELF: the xwin-less CI job
#      cannot contain vale-agent.exe, and says so in its step name.
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

# ── 3. the shipped-file list vs the three content gates ─────────────────────
FILES="$(node -p "require('./agent/vale-agent-npm/package.json').files.join('\n')")"
[ -n "$FILES" ] || { echo "FAIL: agent/vale-agent-npm/package.json declares no files[]"; exit 1; }
# gate path : entry this job legitimately cannot contain (with its reason)
GATES=(
  "scripts/publish-release.sh:"
  ".github/workflows/release.yml:"
  # "no exe in this job" — the job name says so; it has no cargo-xwin.
  ".github/workflows/ci.yml:vale-agent.exe"
)
for g in "${GATES[@]}"; do
  path="${g%%:*}"
  excluded="${g#*:}"
  # The gate block: from its `tar tzf …` listing to the closing `; do`.
  block="$(awk '/tar tzf .*tgz-list/{p=1} p{print} p && /; do$/{exit}' "$path")"
  [ -n "$block" ] || { echo "FAIL: no tgz content gate found in $path"; exit 1; }
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    [ "$entry" = "$excluded" ] && continue
    case "$entry" in
      */) # a directory in files[] is covered by the files under it
        has "$path's gate covers $entry" "$block" "$entry" ;;
      *)
        has "$path's gate ships $entry" "$block" "\"$entry\"" ;;
    esac
  done <<<"$FILES"
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
PWSH_BLOCK="$(awk '/installer integrity tests \(pwsh\)/{p=1} p{print} p && /ValeIntegrity.tests.ps1$/{exit}' .github/workflows/ci.yml)"
[ -n "$PWSH_BLOCK" ] || { echo "FAIL: no installer-integrity step found in ci.yml"; exit 1; }
has "the pwsh step still runs the integrity tests" "$PWSH_BLOCK" "ValeIntegrity.tests.ps1"
has "a missing pwsh FAILS the step" "$PWSH_BLOCK" "exit 1"
if grep -q '::warning::pwsh is not installed' .github/workflows/ci.yml; then
  echo "FAIL: the pwsh step warns and continues again — that green means the installer's logic was never tested"
  exit 1
fi

echo "build-pins: $PASS checks passed"

