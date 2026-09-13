#!/usr/bin/env bash
# smoke-index.bash — the installer-ALIAS arm of smoke_index_release, driven with
# `curl` stubbed as a shell function (the same instrument release-audit.bash uses:
# functions beat PATH, so nothing about the smoke is re-implemented here).
#
# WHY THIS FILE EXISTS (round 126). The whole installer block in the smoke sits
# behind `if [ -n "$inst_url" ] || [ -n "$inst_want" ]` — i.e. it only runs when
# the manifest ADVERTISES an installer. A tgz-only publish advertises none, so
# the smoke checked nothing about the alias and still printed
# "ok: /api/version smoke passed". Measured live on 2026-09-14: the CDN's
# `ValeAgent-Setup.exe` and `ValeAgent-Setup-1.2.361.exe` shared an etag while the
# release was 1.2.364 — a three-release-old artifact that no check ever named.
#
# Run: bash scripts/test/smoke-index.bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source "scripts/smoke-index.sh"

PASS=0
check() { # check <desc> <actual> <expected>
  if [ "$2" = "$3" ]; then PASS=$((PASS+1)); else
    echo "FAIL: $1"; echo "  actual:   $2"; echo "  expected: $3"; exit 1
  fi
}
has() { # has <desc> <haystack> <needle>
  case "$2" in *"$3"*) PASS=$((PASS+1));; *) echo "FAIL: $1"; echo "  looked for: $3"; echo "  in: $2"; exit 1;; esac
}
hasnt() { # hasnt <desc> <haystack> <needle>
  case "$2" in *"$3"*) echo "FAIL: $1"; echo "  must NOT contain: $3"; echo "  in: $2"; exit 1;; *) PASS=$((PASS+1));; esac
}

VER=9.9.9
FIX_TGZ='vale-agent-tarball-bytes'
FIX_TGZ_SHA="$(printf '%s' "$FIX_TGZ" | sha256sum | cut -d' ' -f1)"
FIX_ALIAS='stale-setup-exe-bytes'
FIX_ALIAS_SHA="$(printf '%s' "$FIX_ALIAS" | sha256sum | cut -d' ' -f1)"
FIX_INST='bundled-setup-exe-bytes'
FIX_INST_SHA="$(printf '%s' "$FIX_INST" | sha256sum | cut -d' ' -f1)"

# run_smoke <raw-version.json> <live-/api/version> <alias-body|"">
#   TWO SHAPES, because the deployment has two: `/vale-agent/version.json` is the
#   file publish-release.sh writes (a flat `tarball` basename, an `installer`
#   basename), while `/api/version` is the WORKER's answer, which rewrites both
#   into absolute URLs (index/src/index.js:445). A harness that served one shape
#   for both would test a deployment that does not exist.
run_smoke() {
  # THE FIXTURE NAMES ARE PREFIXED ON PURPOSE. Bash locals are DYNAMICALLY scoped,
  # so a fixture called `live` is shadowed by `smoke_index_release`'s own
  # `local live` and the stub reads an empty string — which is how this harness
  # first failed, with "live version mismatch: want 9.9.9, got:". release-audit.bash
  # records the same lesson.
  local raw="$1" live_json="$2" alias_body="$3"
  curl() {
    local url=""
    while [ $# -gt 0 ]; do
      case "$1" in
        -o) shift 2;;
        -m|--retry|--retry-delay) shift 2;;
        -*) shift;;
        *) url="$1"; shift;;
      esac
    done
    case "$url" in
      */api/version) printf '%s' "$live_json";;
      */vale-agent/version.json) printf '%s' "$raw";;
      */vale-agent/vale-agent-latest.tgz) printf '%s' "$FIX_TGZ";;
      */vale-agent/vale-agent-$VER.tgz) printf '%s' "$FIX_TGZ";;
      */vale-agent/ValeAgent-Setup-$VER.exe) printf '%s' "$FIX_INST";;
      */vale-agent/ValeAgent-Setup.exe)
        [ -n "$alias_body" ] && printf '%s' "$alias_body" || return 22;;
      *) return 22;;
    esac
  }
  SMOKE_RETRY_SLEEP=0 smoke_index_release "$VER" "$FIX_TGZ_SHA"
}

# ── 1. the manifest advertises an installer (the arm that already worked) ────
FIX_RAW_WITH="{\"version\":\"$VER\",\"tarball\":\"vale-agent-latest.tgz\",\"sha256\":\"$FIX_TGZ_SHA\",\"installer\":\"ValeAgent-Setup-$VER.exe\",\"installer_sha256\":\"$FIX_INST_SHA\"}"
FIX_LIVE_WITH="{\"version\":\"$VER\",\"download\":\"https://cdn.example/vale-agent/vale-agent-latest.tgz\",\"sha256\":\"$FIX_TGZ_SHA\",\"installer\":\"https://cdn.example/vale-agent/ValeAgent-Setup-$VER.exe\",\"installer_sha256\":\"$FIX_INST_SHA\"}"
if OUT="$(run_smoke "$FIX_RAW_WITH" "$FIX_LIVE_WITH" "$FIX_INST" 2>&1)"; then rc=0; else rc=$?; fi
check "an advertised installer passes" "$rc" "0"
has "and says so" "$OUT" "installer smoke passed"
hasnt "with no stale-alias warning" "$OUT" "STALE INSTALLER ALIAS"

# ── 2. tgz-only manifest + NO alias: consistent, and said out loud ──────────
FIX_RAW_BARE="{\"version\":\"$VER\",\"tarball\":\"vale-agent-latest.tgz\",\"sha256\":\"$FIX_TGZ_SHA\"}"
FIX_LIVE_BARE="{\"version\":\"$VER\",\"download\":\"https://cdn.example/vale-agent/vale-agent-latest.tgz\",\"sha256\":\"$FIX_TGZ_SHA\"}"
if OUT="$(run_smoke "$FIX_RAW_BARE" "$FIX_LIVE_BARE" "" 2>&1)"; then rc=0; else rc=$?; fi
check "a tgz-only release with no alias still passes" "$rc" "0"
has "and states the alias is absent" "$OUT" "alias is absent (consistent)"
hasnt "with no stale-alias warning" "$OUT" "STALE INSTALLER ALIAS"

# ── 3. THE MEASURED STATE: tgz-only manifest + a stale alias present ────────
# This is 2026-09-14 on the live CDN, reproduced. It must NOT fail (a tgz-only
# publish is the documented emergency path) and it must NOT pass silently.
if OUT="$(run_smoke "$FIX_RAW_BARE" "$FIX_LIVE_BARE" "$FIX_ALIAS" 2>&1)"; then rc=0; else rc=$?; fi
check "a stale alias does not fail the release" "$rc" "0"
has "but it is NAMED" "$OUT" "advertises NO installer"
has "with the digest it is serving" "$OUT" "${FIX_ALIAS_SHA:0:24}"
has "and it is called what it is" "$OUT" "STALE ARTIFACT"
has "and the way out is printed" "$OUT" "build-installer.sh $VER"
has "and the summary line does not claim everything is consistent" "$OUT" "WITH A STALE INSTALLER ALIAS"

echo "smoke-index: $PASS checks passed"
