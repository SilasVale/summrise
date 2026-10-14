#!/usr/bin/env bash
# PROVISION THE MAIL BUCKET — one bucket and the one lifecycle rule that bounds it, idempotently.
#
# WHY A SCRIPT AND NOT A WRANGLER `r2_buckets` ENTRY: a bucket binding in `wrangler.jsonc` names a bucket that
# must ALREADY EXIST, and object lifecycles are BUCKET-LEVEL configuration rather than Worker configuration —
# neither is something `wrangler deploy` creates. So the two account-side facts this design depends on are made
# here, where they can be re-run, re-read and diffed.
#
# THE RETENTION NUMBER HAS EXACTLY ONE SOURCE: `RETENTION_DAYS` in `mail/wrangler.jsonc`. This script reads it
# and turns it into the rule; `mail/scripts/test/provision-stub.bash` fails if the number moves in one place and
# not the other, and fails if a second copy appears in the Rust.
#
# Usage:
#   bash mail/scripts/provision.sh --dry-run     # print the plan; touches nothing, needs no token
#   bash mail/scripts/provision.sh               # create the bucket if absent, then put + re-read the rule
#
# Env: CLOUDFLARE_API_TOKEN (else ~/.cloudflare-token), MAIL_CONFIG (else mail/wrangler.jsonc — the stub test
# points this at a copy), CLOUDFLARE_ACCOUNT_ID (only needed when the token sees more than one account).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG="${MAIL_CONFIG:-$ROOT/mail/wrangler.jsonc}"
BUCKET="summrise-mail"
DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

# The one number. `grep` rather than a JSON parser because the file is JSONC — it carries comments, and every
# JSON parser on this box would need the comments stripped first, which is a second thing to get wrong.
retention_days() {
  grep -o '"RETENTION_DAYS"[[:space:]]*:[[:space:]]*"[0-9]\+"' "$CONFIG" | grep -o '[0-9]\+' | head -1
}

DAYS="$(retention_days)"
if [[ -z "$DAYS" ]]; then
  echo "provision: no RETENTION_DAYS in $CONFIG — the rule's age has one source and it is missing" >&2
  exit 1
fi
MAX_AGE=$((DAYS * 86400))

# The rule body, byte for byte what the API takes (the shape `vale-temp-files` already carries: id, enabled,
# conditions.prefix, deleteObjectsTransition.condition).
LIFECYCLE="{\"rules\":[{\"id\":\"mail-${DAYS}d-expiry\",\"enabled\":true,\"conditions\":{\"prefix\":\"mail/\"},\"deleteObjectsTransition\":{\"condition\":{\"type\":\"Age\",\"maxAge\":${MAX_AGE}}}}]}"

if ((DRY_RUN)); then
  echo "provision (dry run): bucket=${BUCKET} retention=${DAYS}d maxAge=${MAX_AGE}s"
  echo "GET /accounts/<account>/r2/buckets/${BUCKET}"
  echo "POST /accounts/<account>/r2/buckets   # only when the GET answers 404"
  echo "PUT /accounts/<account>/r2/buckets/${BUCKET}/lifecycle"
  echo "${LIFECYCLE}"
  echo "GET /accounts/<account>/r2/buckets/${BUCKET}/lifecycle   # re-read: the output IS the proof"
  exit 0
fi

# shellcheck source=../../scripts/lib/release-lib.sh
source "$ROOT/scripts/lib/release-lib.sh"
CF_TOKEN="$(cf_token)"
if [[ -z "$CF_TOKEN" ]]; then
  echo "provision: no Cloudflare token — set CLOUDFLARE_API_TOKEN or write ~/.cloudflare-token" >&2
  exit 1
fi

api() { curl -s --max-time 30 -H "Authorization: Bearer ${CF_TOKEN}" "$@"; }

ACCOUNT="${CLOUDFLARE_ACCOUNT_ID:-}"
if [[ -z "$ACCOUNT" ]]; then
  ACCOUNT="$(api https://api.cloudflare.com/client/v4/accounts |
    python3 -c 'import json,sys; r=json.load(sys.stdin).get("result") or []; print(r[0]["id"] if len(r)==1 else "")')"
fi
if [[ -z "$ACCOUNT" ]]; then
  echo "provision: set CLOUDFLARE_ACCOUNT_ID — the token sees zero or several accounts" >&2
  exit 1
fi

echo "=== [mail] bucket ${BUCKET} (account ${ACCOUNT}) ==="
if api "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/r2/buckets/${BUCKET}" | grep -q '"success":true'; then
  echo "  bucket exists"
else
  echo "  creating the bucket"
  api -X POST "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/r2/buckets" \
    -H 'content-type: application/json' --data "{\"name\":\"${BUCKET}\"}" | head -c 300
  echo
fi

echo "  putting the lifecycle rule (${DAYS}d = ${MAX_AGE}s, prefix mail/)"
api -X PUT "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/r2/buckets/${BUCKET}/lifecycle" \
  -H 'content-type: application/json' --data "${LIFECYCLE}" >/dev/null

echo "  re-read, which is the only proof this script offers:"
api "https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/r2/buckets/${BUCKET}/lifecycle" |
  python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin).get("result"), indent=2))'
