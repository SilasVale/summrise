#!/bin/bash
# Live-vs-repo parity probe (round-541): the round-537 stale deploy proved
# test-only rounds never trigger deploys — 19 src commits sat undeployed
# while every local gate stayed green. Run this after ANY gateway/src
# change (or on a schedule) to catch drift between the LIVE worker's
# /code/ viewer and the repo mirror (public/code/files/summrise-gate/).
#
#
# MUTATION: append a line to ONE mirrored file in a local copy of the mirror served on a loopback port
#           (`echo "// mutated" >> <served>/code/files/summrise-gate/src/mcp.ts`).
# RESULT:   exit 1 — "DRIFT src/mcp.ts", "checked 41 files, 1 drifted — PERSISTED across 1 attempts".
# MUTATION: point the probe at a DEAD PORT (`http://127.0.0.1:9`), so every request fails to connect.
# RESULT:   exit 2 — "checked 41 files, 41 could not be FETCHED and 0 drifted — the probe could not look",
#           plus the sentence that keeps the exit honest: "nothing here says the worker matches".
# AND THE MUTATION THAT MUST NOT BITE: the same mirror unmodified on the same loopback port -> exit 0,
#           "checked 41 files, 0 drifted". All three measured 2026-10-09 with PARITY_ATTEMPTS=1.
#   base-url defaults to https://api.saisi.online
#
# PROPAGATION RETRY (round-546): a deploy's new assets are not visible at every
# POP the instant `wrangler deploy` returns. The probe used to make ONE pass,
# which build.sh fed 8 s after the deploy — so on 2026-09-13 it compared against
# the PREVIOUS assets, printed 9 "DRIFT" lines, and reported a deploy that had
# SUCCEEDED as a failure. "Drift" is only a verdict once it PERSISTS, so the
# pass is retried (the shape smoke-index.sh has had since round-59) and the
# per-file list is printed only after the LAST attempt. A genuinely stale worker
# still fails — just ~2 min later, not 8 s later.
#   PARITY_ATTEMPTS     default 8   (set 1 for a single fast pass)
#   PARITY_RETRY_SLEEP  default 20  (seconds between passes)
#
# **A FETCH FAILURE IS NOT A DRIFT — MEASURED 2026-10-09, ON A BOX WHOSE NETWORK COULD NOT REACH THE CDN.**
# `curl -sL` without `-f` SUCCEEDS on a 404, so an HTTP error shows up as DRIFT (a comparable body was
# read and differs). A `FETCH-FAIL` therefore means curl EXITED NON-ZERO — a connection or timeout failure
# — and this probe used to count that as drift too. Measured on the deploy box: 20 of 41 files
# FETCH-FAIL'd with `curl` exit **28** (`--max-time 20` fired; one file got a 200 and then stalled, another
# never answered, and the host root timed out), the probe printed "live worker differs from repo", and
# `build.sh gateway` reported a deploy as FAILED — **while the upload had succeeded**
# (`Uploaded summrise-gate (6.54 sec)`, `Current Version ID: 3b11c8ab`). That is the one verdict this
# file's own comments say a deploy gate must not get wrong, arriving from the other side: the round-546 fix
# covered a *propagation* lag, not an *unreachable* host.
#
# Exit 0 = every mirrored file byte-identical.
# Exit 1 = a CONTENT drift persisted across every attempt, with the per-file list below.
# Exit 2 = the probe COULD NOT LOOK (every failure was a fetch failure) — the `n/a` convention
#          `all-gates.bash` implements and `rust-byte-checks.bash` established. A deploy is what FIXES a
#          stale worker, so being unable to read the live one must not block the thing that would correct
#          it; `build.sh` warns and proceeds on 2, and still fails on 1.
set -e
cd "$(dirname "$0")/.."
BASE="${1:-https://api.saisi.online}"
MANIFEST=public/code/manifest.json
MIRROR=public/code/files/summrise-gate
ATTEMPTS="${PARITY_ATTEMPTS:-8}"
RETRY_SLEEP="${PARITY_RETRY_SLEEP:-20}"

paths=$(python3 -c "
import json
d = json.load(open('$MANIFEST'))
for f in d['files']:
    if f.get('group') == 'summrise-gate':
        print(f['path'].replace('files/summrise-gate/', ''))
")
count=$(printf '%s\n' $paths | wc -l)

# ONE pass over the manifest. Fills PASS_DRIFT (count) and PASS_DETAIL (the
# per-file lines) rather than printing them, so a retry does not spam the list.
check_pass() {
  local drift=0 fetchfail=0 p
  PASS_DETAIL=""
  PASS_DRIFT=0
  PASS_FETCHFAIL=0
  for p in $paths; do
    # -L: the viewer 307-redirects bare files to their directory form
    # (public/index.html → public/); the target body is the comparable one.
    if ! curl -sL --max-time 20 "$BASE/code/files/summrise-gate/$p" -o /tmp/parity-live.ts; then
      PASS_DETAIL+="FETCH-FAIL $p"$'\n'
      fetchfail=$((fetchfail + 1))
      continue
    fi
    if ! diff -q /tmp/parity-live.ts "$MIRROR/$p" >/dev/null 2>&1; then
      PASS_DETAIL+="DRIFT $p"$'\n'
      drift=$((drift + 1))
    fi
  done
  PASS_DRIFT="$drift"
  PASS_FETCHFAIL="$fetchfail"
}

PASS_DRIFT=0
PASS_FETCHFAIL=0
PASS_DETAIL=""
for attempt in $(seq 1 "$ATTEMPTS"); do
  check_pass
  if [ "$PASS_DRIFT" = 0 ] && [ "$PASS_FETCHFAIL" = 0 ]; then
    echo "checked $count files, 0 drifted"
    exit 0
  fi
  if [ "$attempt" -lt "$ATTEMPTS" ]; then
    echo "  .. attempt $attempt/$ATTEMPTS: $PASS_DRIFT drifted, $PASS_FETCHFAIL unreachable of $count — retrying in ${RETRY_SLEEP}s" >&2
    sleep "$RETRY_SLEEP"
  fi
done
printf '%s' "$PASS_DETAIL"
# A CONTENT DRIFT OUTRANKS AN UNREACHABLE FILE: if even one file was read and differs, that is a verdict
# about the worker, and it is the failure this probe exists to report.
if [ "$PASS_DRIFT" != 0 ]; then
  echo "checked $count files, $PASS_DRIFT drifted — PERSISTED across $ATTEMPTS attempts"
  exit 1
fi
echo "checked $count files, $PASS_FETCHFAIL could not be FETCHED and 0 drifted — the probe could not look"
echo "  (every failure was a connection or timeout failure, not a difference; nothing here says the worker matches)"
exit 2
