#!/usr/bin/env bash
# HOW FULL IS THE MAIL BUCKET, AND THE OTHER THREE — the same query every number in the design came from.
#
# The 10 GB free tier is ACCOUNT-wide, and the mail bucket shares it with the installer's component cache and
# the release artifacts. So "are we about to be billed" is a question about four buckets, not one, and it should
# be one command rather than a dashboard visit.
#
# Usage: bash mail/scripts/usage.sh
# Env:   CLOUDFLARE_API_TOKEN (else ~/.cloudflare-token), CLOUDFLARE_ACCOUNT_ID (when the token sees several)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../../scripts/lib/release-lib.sh
source "$ROOT/scripts/lib/release-lib.sh"
CF_TOKEN="$(cf_token)"
if [[ -z "$CF_TOKEN" ]]; then
  echo "usage: no Cloudflare token — set CLOUDFLARE_API_TOKEN or write ~/.cloudflare-token" >&2
  exit 1
fi

TODAY="$(date -u +%Y-%m-%d)"
ACCOUNT="${CLOUDFLARE_ACCOUNT_ID:-}"
if [[ -z "$ACCOUNT" ]]; then
  ACCOUNT="$(curl -s --max-time 30 -H "Authorization: Bearer ${CF_TOKEN}" \
    https://api.cloudflare.com/client/v4/accounts |
    python3 -c 'import json,sys; r=json.load(sys.stdin).get("result") or []; print(r[0]["id"] if len(r)==1 else "")')"
fi
[[ -n "$ACCOUNT" ]] || { echo "usage: set CLOUDFLARE_ACCOUNT_ID" >&2; exit 1; }

# `r2StorageAdaptiveGroups` reports the day's PEAK per bucket, which is the quantity R2 bills (GB-month, the
# average of daily peaks) — a sum of per-object sizes would be a different number and a wrong one.
#
# THE PROGRAM IS A `-c` STRING AND NOT A HEREDOC, WHICH IS A MEASURED CORRECTION: `python3 - <<'PY'` inside a
# pipeline takes its SCRIPT from the same stdin the JSON was meant to arrive on, so the first version read the
# query's answer as Python source and died with `JSONDecodeError: Expecting value: line 1 column 1 (char 0)`.
READER=$(
  cat <<'PY'
import json, sys
today = sys.argv[1]
groups = json.load(sys.stdin)["data"]["viewer"]["accounts"][0]["r2StorageAdaptiveGroups"]
seen, total, objects = {}, 0, 0
for group in groups:
    name = group["dimensions"]["bucketName"]
    if group["dimensions"]["date"] != today or name in seen:
        continue
    row = group["max"]
    size = row["payloadSize"] + row["metadataSize"]
    seen[name] = (row["objectCount"], size)
    total += size
    objects += row["objectCount"]
for name in sorted(seen):
    count, size = seen[name]
    print(f"{name:24s} {count:5d} object(s) {size/1e6:10.1f} MB")
print(f"{'TOTAL':24s} {objects:5d} object(s) {total/1e6:10.1f} MB   ({total/1e9:.3f} GB of the 10 GB free tier)")
PY
)

curl -s --max-time 60 -X POST -H "Authorization: Bearer ${CF_TOKEN}" -H 'content-type: application/json' \
  https://api.cloudflare.com/client/v4/graphql \
  --data "{\"query\":\"query{viewer{accounts(filter:{accountTag:\\\"${ACCOUNT}\\\"}){r2StorageAdaptiveGroups(limit:100,filter:{date_geq:\\\"${TODAY}\\\"},orderBy:[date_DESC]){max{payloadSize metadataSize objectCount} dimensions{bucketName date}}}}}\"}" |
  python3 -c "$READER" "$TODAY"
