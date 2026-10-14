#!/usr/bin/env bash
# THE RETENTION NUMBER HAS ONE SOURCE, AND THIS IS THE THING THAT SAYS SO.
#
# WHAT IT GUARDS: `mail/scripts/provision.sh` turns `RETENTION_DAYS` in `mail/wrangler.jsonc` into the bucket's
# lifecycle age. If the two ever disagree — because one moved and the other did not, or because someone added a
# second copy of the number in Rust — the bucket's only bound on storage stops matching the design, and nothing
# else in the repository would notice.
#
# MUTATION (both halves were run): set `"RETENTION_DAYS": "3"` in a copy of the config and leave the script's
# arithmetic alone → the maxAge case fails naming 259200; add a `const RETENTION_DAYS` to
# `mail/worker/src/lib.rs` → the second-home case fails.
#
# No token, no network: the dry run prints the plan before any credential is read, which is what makes this
# testable in CI.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
fail() {
  echo "provision-stub: FAIL: $*" >&2
  exit 1
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ── case 1: the shipped config ──────────────────────────────────────────────────────────────────────────
out="$(MAIL_CONFIG="$ROOT/mail/wrangler.jsonc" bash "$ROOT/mail/scripts/provision.sh" --dry-run)" ||
  fail "the dry run exited non-zero: $out"
echo "$out" | grep -q '"type":"Age","maxAge":604800' ||
  fail "expected maxAge 604800 for the shipped 7 days, got: $out"
echo "$out" | grep -q '"prefix":"mail/"' ||
  fail "the rule must be scoped to prefix mail/ — a bucket-wide rule would delete anything else in the bucket, got: $out"
echo "$out" | grep -q 'summrise-mail' || fail "the plan does not name the bucket: $out"

# ── case 2: the number moves in the config and the rule follows it ──────────────────────────────────────
sed 's/"RETENTION_DAYS": "7"/"RETENTION_DAYS": "3"/' "$ROOT/mail/wrangler.jsonc" >"$TMP/wrangler.jsonc"
grep -q '"RETENTION_DAYS": "3"' "$TMP/wrangler.jsonc" ||
  fail "the copy still says 7 — this case would prove nothing, so it fails instead"
out="$(MAIL_CONFIG="$TMP/wrangler.jsonc" bash "$ROOT/mail/scripts/provision.sh" --dry-run)" ||
  fail "the dry run (3 days) exited non-zero: $out"
echo "$out" | grep -q '"maxAge":259200' ||
  fail "expected maxAge 259200 for a 3-day window, got: $out"

# ── case 3: the number must not have a second home in the crate ─────────────────────────────────────────
# The worker does not expire anything itself (R2 does), so a Rust constant would be a sentence nothing reads.
if grep -q 'RETENTION_DAYS' "$ROOT/mail/worker/src/lib.rs"; then
  fail "mail/worker/src/lib.rs names RETENTION_DAYS — the retention number has a second home"
fi

echo "provision-stub: ok"
