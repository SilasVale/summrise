#!/usr/bin/env bash
# release-audit.bash — regression tests for the P0 dual-builder verdict.
#
# WHY THIS FILE EXISTS. `release-audit.sh` decides whether the CDN and the GitHub
# release are the same artifact. It was the only lib in this toolbox with NO TEST,
# and two defects lived in it through TWENTY consecutive releases:
#   * it compared file CONTENT only, so a one-file MODE difference was reported
#     as "packaging metadata" and PASSED (WARN, exit 0);
#   * its two `cd`s were unchecked command substitutions, so a tarball whose top
#     directory is not `package` made BOTH listings empty, compared nothing, and
#     returned 0 — an audit that passes while comparing ZERO files.
#
# These tests drive the REAL `audit_release_asset` end to end: `curl` is stubbed
# as a shell function (functions beat PATH), so the function's own download and
# API steps run against fixtures. Nothing about the comparison is re-implemented
# here, which is the only way a test of it can be trusted.
set -euo pipefail
cd "$(dirname "$0")/../.."
source "scripts/lib/release-audit.sh"

PASS=0
check() { # check <desc> <actual> <expected>
  if [ "$2" = "$3" ]; then PASS=$((PASS+1)); else
    echo "FAIL: $1"; echo "  actual:   $2"; echo "  expected: $3"; exit 1
  fi
}
has() { # has <desc> <haystack> <needle>
  case "$2" in *"$3"*) PASS=$((PASS+1));; *) echo "FAIL: $1"; echo "  looked for: $3"; echo "  in: $2"; exit 1;; esac
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export GITHUB_TOKEN=test-token-not-used   # the token CHECK runs; the calls are stubbed

# --- fixtures ---------------------------------------------------------------
# Build a tgz whose single file carries a chosen MODE, paired against a 644 twin.
mk_tgz() { # mk_tgz <out.tgz> <top> <mode> [content]
  local out="$1" top="$2" mode="$3" content="${4:-hello}"
  local d; d="$(mktemp -d)"
  mkdir -p "$d/$top"
  # THE DIRECTORY MODE IS PART OF WHAT THIS TEST CONTROLS, and it was the one thing it left to the
  # environment. The audit compares every entry's mode, directories included, so a umask of 0022 here
  # (0755) and 0002 on CI (0775) made case 2 — "identical trees must PASS" — fail there and pass here,
  # with nothing in the diff to look at. The file modes below were always explicit; now the directory
  # is too.
  chmod 755 "$d/$top"
  printf '%s\n' "$content" > "$d/$top/a.txt"
  chmod "$mode" "$d/$top/a.txt"
  printf 'exe\n' > "$d/$top/vale-agent.exe"; chmod 644 "$d/$top/vale-agent.exe"
  tar czf "$out" -C "$d" "$top"
  rm -rf "$d"
}

# --- the harness: drive the REAL function with `curl` stubbed ----------------
# `curl` is a shell FUNCTION here, and functions beat PATH. So the function's own
# API check, its two downloads, its extraction and its comparison all run — only
# the network is replaced. Nothing about the comparison is re-implemented.
run_audit() { # run_audit <gh.tgz> <cdn.tgz> -> rc, prints the audit's output
  # NAMES MUST NOT COLLIDE WITH THE FUNCTION UNDER TEST. Bash locals are
  # DYNAMICALLY scoped, so a local named `cdn` here is shadowed by
  # `audit_release_asset`'s own `local cdn="$2"` — and my first version of this
  # harness copied the literal URL "https://cdn.example" as its fixture.
  local fix_gh="$1" fix_cdn="$2"
  curl() {
    local url="" out=""
    while [ $# -gt 0 ]; do
      case "$1" in
        -o) out="$2"; shift 2;;
        -m|--retry|--retry-delay) shift 2;;
        -*) shift;;
        *) url="$1"; shift;;
      esac
    done
    case "$url" in
      *api.github.com*) printf '{"assets":[{"name":"vale-agent-9.9.9.tgz"}]}\n'; return 0;;
      *releases/download*) cp "$fix_gh" "$out"; return 0;;
      *vale-agent/vale-agent-*) cp "$fix_cdn" "$out"; return 0;;
      *) return 22;;
    esac
  }
  audit_release_asset 9.9.9 "https://cdn.example"
}

# --- 1. a FILE MODE difference must FAIL, not warn --------------------------
# The live defect: identical bytes, one file 600 vs 644 — called "packaging
# metadata" and returned 0 for twenty consecutive releases.
mk_tgz "$WORK/gh.tgz"   package 644
mk_tgz "$WORK/cdn.tgz"  package 600
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
if [ "$rc" != 1 ]; then echo "--- audit output ---"; echo "$out"; fi
check "a FILE MODE difference must FAIL" "$rc" "1"
has "the message must name modes" "$out" "FILE MODES"
has "and name the file" "$out" "./a.txt"

# --- 2. matching modes and content pass ------------------------------------
mk_tgz "$WORK/gh.tgz"   package 644
mk_tgz "$WORK/cdn.tgz"  package 644
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
# PRINT THE AUDIT'S OWN OUTPUT WHEN THIS ONE FAILS, exactly as case 1 does. This check failed once on CI
# (2026-09-17) and passed on an identical re-run with NO LOCAL REPRODUCTION in between — and because only
# the failure branch above echoed anything, the log said "must PASS" and nothing else. The fixture's modes
# are all explicit (the directory since the umask fix above), so there is no known environment dependency
# left in it; if this fires again, the audit's own words are the first thing anyone will need.
if [ "$rc" != 0 ]; then echo "--- audit output ---"; echo "$out"; fi
check "identical trees (modes included) must PASS" "$rc" "0"

# --- 3. a listing that cannot be taken must NOT compare equal ---------------
# Both `cd`s failed, both listings were empty, they compared equal, ZERO files
# were compared, and the function returned 0.
# The two tarballs must DIFFER, or the function's own sha shortcut returns 0
# before extracting anything — my first version of this test used identical
# fixtures and "proved" the guard was missing when it had simply never been
# reached. (A test whose premise skips the code under test.)
mk_tgz "$WORK/gh.tgz"   notpackaged 644 "one"
mk_tgz "$WORK/cdn.tgz"  notpackaged 644 "two"
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
check "a tarball without a top-level package/ must FAIL" "$rc" "1"
has "and say nothing could be compared" "$out" "nothing could be compared"

# The SECOND guard, which that fixture does not reach: a `package/` that exists
# and holds NO REGULAR FILES. Both listings are still empty, so without the guard
# they compare equal and zero files are compared — the same silent pass, one
# directory deeper. (I found this by mutating the guard away and watching this
# file stay green: a test that passes for a reason unrelated to its claim.)
mk_empty_tgz() { # mk_empty_tgz <out.tgz> <top> [content]
  local out="$1" top="$2" content="${3:-}"
  local d; d="$(mktemp -d)"
  mkdir -p "$d/$top"
  # A directory entry, never a file; the content argument only varies the bytes
  # so the two tarballs' shas differ and the comparison is actually reached.
  mkdir -p "$d/$top/sub-$content"
  tar czf "$out" -C "$d" "$top"
  rm -rf "$d"
}
mk_empty_tgz "$WORK/gh.tgz"  package one
mk_empty_tgz "$WORK/cdn.tgz" package two
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
check "a package/ with no regular files must FAIL" "$rc" "1"
has "and say the audit compared nothing" "$out" "compared nothing"

# --- 4. content drift still fails ------------------------------------------
mk_tgz "$WORK/gh.tgz"   package 644 "different"
mk_tgz "$WORK/cdn.tgz"  package 644 "hello"
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
check "content drift must FAIL" "$rc" "1"

# --- 5. the bytes-differ-and-I-cannot-say-why arm must FAIL ----------------
# This arm returned 0 ("WARN … unexplained packaging metadata") and the caller
# then printed "audit OK: … its source-derived files match the GitHub asset" —
# a verdict the function had explicitly failed to reach, on the one artifact
# that IS the release. Identical content AND modes, different mtimes: exactly
# the case that reaches it.
mk_tgz_ts() { # mk_tgz_ts <out.tgz> <touch-when>
  local out="$1" when="$2" d; d="$(mktemp -d)"
  mkdir -p "$d/package"
  printf 'hello\n' > "$d/package/a.txt"; chmod 644 "$d/package/a.txt"
  printf 'exe\n'   > "$d/package/vale-agent.exe"; chmod 644 "$d/package/vale-agent.exe"
  find "$d" -exec touch -d "$when" {} +
  tar czf "$out" -C "$d" package
  rm -rf "$d"
}
mk_tgz_ts "$WORK/gh.tgz"  "2020-01-01T00:00:00Z"
mk_tgz_ts "$WORK/cdn.tgz" "2021-06-15T12:00:00Z"
out="$(run_audit "$WORK/gh.tgz" "$WORK/cdn.tgz" 2>&1)" && rc=0 || rc=$?
check "identical content+mode but different tarball BYTES must FAIL" "$rc" "1"
has "and say the difference could not be named" "$out" "cannot name the difference"

# --- 6. the three verdicts of audit_asset_names ----------------------------
# rc 1 ("could not ask") and rc 3 ("does not exist") are NOT the same thing:
# `--skip-reconcile`'s guard reads 3 as first-publish and must refuse on 1.
# Both were 1 before round 122, so an expired token skipped a P0 audit and the
# run reported success.
names_code() { # names_code <http-code|network> -> rc; names printed on 0
  local want="$1"
  curl() {
    local url=""
    while [ $# -gt 0 ]; do case "$1" in -w) shift 2;; -*) shift;; *) url="$1"; shift;; esac; done
    [ "$want" = "network" ] && return 7
    printf '{"assets":[{"name":"vale-agent-9.9.9.tgz"}]}\n%s\n' "$want"
    return 0
  }
  audit_asset_names 9.9.9
}
out="$(names_code 200 2>&1)" && rc=0 || rc=$?
check "HTTP 200 = the release exists" "$rc" "0"
has "and its asset names come back" "$out" "vale-agent-9.9.9.tgz"
out="$(names_code 404 2>&1)" && rc=0 || rc=$?
check "HTTP 404 = the release does NOT exist (rc 3)" "$rc" "3"
out="$(names_code 500 2>&1)" && rc=0 || rc=$?
check "HTTP 500 = could not ask (rc 1)" "$rc" "1"
has "and names the status" "$out" "HTTP 500"
out="$(names_code 403 2>&1)" && rc=0 || rc=$?
check "HTTP 403 = could not ask (rc 1)" "$rc" "1"
out="$(names_code network 2>&1)" && rc=0 || rc=$?
check "a network failure = could not ask (rc 1)" "$rc" "1"
has "and says the call failed" "$out" "failed (network)"
# No token at all: same verdict, and it must not be mistaken for 404 either.
mkdir -p "$WORK/nohome"
out="$(GITHUB_TOKEN= GH_TOKEN= HOME="$WORK/nohome" audit_asset_names 9.9.9 2>&1)" && rc=0 || rc=$?
check "no token = could not ask (rc 1)" "$rc" "1"
has "and says so" "$out" "no GitHub token"

# --- 7. the caller must not flatten those verdicts again -------------------
# publish-release.sh has no test harness of its own (the orchestrator never
# did), so this is a SOURCE pin — the same instrument the repo uses for the
# boot-task contract and the pre-v2 path rule. It fails if someone puts the
# `|| true` back or drops the rc branch.
GUARD="$(sed -n '/^if \[ "\$SKIP_RECONCILE" -eq 1 \]/,/^else$/p' scripts/publish-release.sh)"
has "the guard branch exists" "$GUARD" "audit_asset_names"
case "$GUARD" in *"audit_asset_names \"\$VER\" >\"\$SKIP_LIST\" 2>/dev/null || true"*)
  echo "FAIL: the --skip-reconcile guard swallows the verdict again (|| true)"; exit 1;;
  *) PASS=$((PASS+1));;
esac
has "and distinguishes 'does not exist' from 'could not ask'" "$GUARD" 'SKIP_RC" -ne 3'
has "and refuses rather than skipping when it could not ask" "$GUARD" "refusing to skip the audit"

echo "release-audit: all $PASS checks passed"
