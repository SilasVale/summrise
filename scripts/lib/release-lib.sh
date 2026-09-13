#!/usr/bin/env bash
# release-lib.sh — testable stages of publish-release.sh, extracted verbatim
# (structure refactor: the last-5-per-minor prune and the version.json writer
# are pure file operations; scripts/test/release-lib.bash pins them so the
# round-309 keep-policy can never silently regress).

# Write the CDN manifest the agent_update tool consumes (round-119: sha256
# REQUIRED). $1 = version, $2 = packed tgz path, $3 = output dir,
# $4 = optional staged installer exe path (ValeAgent-Setup-<ver>.exe).
# Echoes the sha256 so the caller keeps it for the reconcile stages.
# The installer fields are ADDITIVE (old readers ignore them): when $4 is
# given and exists, the manifest also carries installer (flat basename) +
# installer_sha256 so fresh installs are verifiable the same way tgz
# updates are. When absent (older flow / installer not rebuilt yet), the
# manifest keeps the tgz-only shape and every old consumer keeps working.
write_version_json() {
  local ver="$1" tgz="$2" out="$3" installer_exe="${4:-}"
  local sha
  sha=$(sha256sum "$tgz" | cut -d' ' -f1)
  if [[ -n "$installer_exe" && -f "$installer_exe" ]]; then
    local ish
    ish=$(sha256sum "$installer_exe" | cut -d' ' -f1)
    printf '{"version":"%s","tarball":"vale-agent-latest.tgz","updated":"%s","sha256":"%s","installer":"%s","installer_sha256":"%s"}\n' \
      "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$sha" "$(basename "$installer_exe")" "$ish" > "$out/version.json"
  else
    printf '{"version":"%s","tarball":"vale-agent-latest.tgz","updated":"%s","sha256":"%s"}\n' \
      "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$sha" > "$out/version.json"
  fi
  echo "$sha"
}

# Last-5-per-minor prune (round-309): keep the newest 5 of EACH major.minor
# line + the latest alias; delete every other vale-agent-1.*.*.tgz. A flat
# last-5 across all 1.x would evict the previous minor line the moment the
# new line ships 5 releases and break pinned installs. $1 = asset dir.
prune_last5_per_minor() {
  local dir="$1"
  shopt -s nullglob
  mapfile -t KEEP < <(ls "$dir"/vale-agent-1.*.*.tgz 2>/dev/null | grep -v latest | sort -V | awk '
    { ver = $0; sub(/.*vale-agent-/, "", ver); sub(/\.tgz$/, "", ver); n = split(ver, a, "."); key = a[1] "." a[2]; c[key]++; line[key, c[key]] = $0 }
    END { for (k in c) { from = (c[k] > 5 ? c[k] - 4 : 1); for (i = from; i <= c[k]; i++) print line[k, i] } }
  ')
  local f k keep=0
  for f in "$dir"/vale-agent-1.*.*.tgz; do
    keep=0
    for k in "${KEEP[@]}"; do [ "$k" = "$f" ] && keep=1 && break; done
    if [ "$keep" -eq 0 ]; then rm -f "$f"; echo "pruned $(basename "$f")"; fi
  done
}

# Installer prune: keep the newest $2 (default 5) versioned
# ValeAgent-Setup-1.*.*.exe PER MAJOR.MINOR, plus the versionless
# ValeAgent-Setup.exe alias (never pruned — the landing page links it when the
# manifest advertises one).
#
# PER MINOR, because this comment used to CALL ITSELF the companion to the tgz
# last-5-per-minor policy above while doing something else: a flat last-5 across
# all 1.x. Round 127 measured what that costs — with five 1.2.x installers and a
# 1.3 line at the cap, the flat policy deleted EVERY 1.2.x installer. A versioned
# installer pins the tgz it was built against, so the previous line's rollback
# path went with them — the exact loss the tgz policy exists to prevent, and its
# own comment says so 20 lines above. The one-minor test fixture could not tell
# the two policies apart; there is a two-minor one now.
prune_installers() {
  local dir="$1" keep_n="${2:-5}"
  shopt -s nullglob
  mapfile -t KEEP_EXE < <(ls "$dir"/ValeAgent-Setup-1.*.*.exe 2>/dev/null | sort -V | awk -v keep="$keep_n" '
    { ver = $0; sub(/.*ValeAgent-Setup-/, "", ver); sub(/\.exe$/, "", ver); n = split(ver, a, "."); key = a[1] "." a[2]; c[key]++; line[key, c[key]] = $0 }
    END { for (k in c) { from = (c[k] > keep ? c[k] - keep + 1 : 1); for (i = from; i <= c[k]; i++) print line[k, i] } }
  ')
  local f k keep=0
  for f in "$dir"/ValeAgent-Setup-1.*.*.exe; do
    keep=0
    for k in "${KEEP_EXE[@]}"; do [ "$k" = "$f" ] && keep=1 && break; done
    if [ "$keep" -eq 0 ]; then rm -f "$f"; echo "pruned $(basename "$f")"; fi
  done
}

# ── the reconcile ledger (round 123) ─────────────────────────────────────────
# WHAT THIS IS FOR. `--skip-reconcile` publishes to the CDN with no GitHub
# release to audit against, and until round 123 that fact lived ONLY in the
# operator's scrollback: no file recorded it, so nothing could refuse the next
# one. Measured 2026-09-14: the newest GitHub release was v1.2.361 while the CDN
# had shipped 1.2.363 and 1.2.364 — the debt was three versions old, invisible,
# and `scripts/lib/release-audit.sh` had never once run against a real release.
#
# One line per version, `#` comments allowed, TRACKED in git so the debt
# survives this machine and shows up in review. The alternative — keeping it on
# the CDN next to version.json — was rejected: a state file that only exists
# where the publish puts it cannot fail a publish that has not happened yet.
RECONCILE_LEDGER="${RECONCILE_LEDGER:-docs/agents/release-reconcile.txt}"

# Echo the versions that owe a reconcile, one per line, in file order.
#
# ONE awk, NOT `grep -v | awk`. The pipeline version returned grep's status —
# 1 when the ledger held only comments — and under `set -e -o pipefail` that
# killed publish-release.sh SILENTLY the first time the ledger existed with every
# debt settled. Its own test caught it (the suite died with no output, which is
# what a `set -e` death inside a command substitution looks like). awk exits 0 on
# empty input, so the question "what is owed?" always has an answer.
reconcile_pending() {
  [ -f "$RECONCILE_LEDGER" ] || return 0
  awk '!/^[[:space:]]*#/ && NF {print $1}' "$RECONCILE_LEDGER"
}

# Record <ver> as published-but-unreconciled. Idempotent: a re-run must not grow
# the ledger, or "how many versions owe a reconcile" becomes unanswerable.
reconcile_record() {
  local ver="$1" note="${2:-published with no GitHub release to audit against}"
  # here-string, not `producer | grep -q`: under pipefail an early-exiting grep
  # SIGPIPEs the producer and the pipeline reports failure even on a match
  # (round-288, learned the same way in publish-release.sh).
  if grep -qx "$ver" <<<"$(reconcile_pending)"; then return 0; fi
  mkdir -p "$(dirname "$RECONCILE_LEDGER")"
  if [ ! -f "$RECONCILE_LEDGER" ]; then
    printf '# Versions on the CDN with no GitHub release to audit against.\n' > "$RECONCILE_LEDGER"
    printf '# Written by scripts/publish-release.sh --skip-reconcile; cleared by --audit-only <ver>.\n' >> "$RECONCILE_LEDGER"
  fi
  printf '%s %s %s\n' "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$note" >> "$RECONCILE_LEDGER"
}

# Clear <ver> — its audit finally ran and passed. Comments are preserved.
reconcile_clear() {
  local ver="$1" tmp
  [ -f "$RECONCILE_LEDGER" ] || return 0
  tmp="$(mktemp)"
  awk -v v="$ver" '/^[[:space:]]*#/ {print; next} $1 != v {print}' "$RECONCILE_LEDGER" > "$tmp"
  mv "$tmp" "$RECONCILE_LEDGER"
}

# ── the installer manifest verdict (round 130) ───────────────────────────────
# A DEPLOY THAT LEAVES THE MANIFEST BEHIND IS NOT A SUCCESS.
# `build-installer.sh`'s default path staged the exe, deployed the worker and
# printed "== done ==" with the new URLs — while `/api/version` still described
# the PREVIOUS installer (or none), because nothing rewrote version.json. The
# landing page asks the manifest before offering the button (round 125), so that
# build was invisible at best and advertised a stale digest at worst, and the run
# said it succeeded. `publish-release.sh:309` tells operators to run this script
# standalone, so the path is not hypothetical.
#
# Echoes one verdict for <manifest-json> <sha256-of-the-exe-just-deployed>:
#   ok               the manifest advertises exactly this build
#   absent           no installer is advertised at all — this door will not link it
#   mismatch:<sha>   the manifest advertises a DIFFERENT installer
installer_manifest_verdict() {
  local json="$1" want="$2" got
  got="$(printf '%s' "$json" | grep -oP '"installer_sha256":"\K[^"]+' | head -1)" || got=""
  if [ -z "$got" ]; then echo "absent"; return 0; fi
  if [ "$got" = "$want" ]; then echo "ok"; return 0; fi
  echo "mismatch:$got"
}

# pack_input_mode_verdict <repo_root> <npm_dir>
# Prints one line per pack input whose WORKTREE mode differs from the mode git
# records, and returns 0 when none do. Extracted from publish-release.sh (round
# 154) so the gate has BEHAVIOURAL tests: `npm pack` preserves worktree modes, so
# a 0600 file packs a tarball that differs from a fresh checkout's by its tar
# HEADER alone — the measured cause of twenty consecutive "packaging metadata"
# WARNs and of the 3-byte drift on the 1.2.348 pair (README.md's mode field plus
# the header checksum, with every file's sha256 matching, the exe included).
# The gate sat mid-chain in the orchestrator, behind the reconcile gate, the
# version check and the exe check, so no test could reach it; this is that logic,
# reachable.
pack_input_mode_verdict() {
  local root="${1:?repo root}" npm_dir="${2:?npm dir}"
  local bad="" f idx want have
  while IFS= read -r f; do
    [ -f "$root/$f" ] || continue
    idx=$(git -C "$root" ls-files -s -- "$f" | awk '{print $1}')
    [ -n "$idx" ] || continue
    want=644; [ "$idx" = "100755" ] && want=755
    have=$(stat -c '%a' "$root/$f" 2>/dev/null || echo '?')
    [ "$have" = "$want" ] || bad="${bad}$f (worktree $have, this checkout should be $want)"$'\n'
  done < <(git -C "$root" ls-files -- "$npm_dir/bin" "$npm_dir/src" "$npm_dir/test" \
             "$npm_dir/README.md" "$npm_dir/vale-desktop-electron" "agent/vale-desktop-electron")
  if [ -n "$bad" ]; then printf '%s' "$bad"; return 1; fi
  return 0
}
