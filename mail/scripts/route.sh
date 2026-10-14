#!/usr/bin/env bash
# THE TWO ACCOUNT-SIDE FACTS THE READ PATH NEEDS — a proxied DNS record for the read host and ONE Email Routing
# rule — created idempotently, and then RE-READ so the run's output is the proof rather than a claim.
#
# WHAT THIS SCRIPT DOES NOT TOUCH, WHICH IS THE WHOLE DESIGN: the zone already carries ONE rule — a catch-all
# that sends every other address to the operator's 163 — and this script neither edits nor deletes it. What it
# adds is a MORE SPECIFIC rule: a literal matcher for one address at priority 0, which Cloudflare evaluates
# before the catch-all. The operator's standing rule for this work is "不该删的别删", and the re-read at the end
# is how that rule is checked instead of promised: the catch-all must still be there, byte for byte.
#
# Usage:
#   bash mail/scripts/route.sh --dry-run   # print the plan; touches nothing, needs no token
#   bash mail/scripts/route.sh             # create what is missing, then re-read and print both facts
#
# Env: CLOUDFLARE_API_TOKEN (else ~/.cloudflare-token), CLOUDFLARE_ZONE_ID (else looked up by name).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ADDRESS="dsh@saisi.online"
HOST="mail.saisi.online"
WORKER="summrise-mail"
ZONE_NAME="saisi.online"
# The Cloudflare-documented placeholder for "this name is proxied and served by a Worker": the address is never
# dialled, because the proxy answers and the route hands the request to the worker. A real host here would be a
# lie that outlives the deployment.
PLACEHOLDER="192.0.2.1"
DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

if ((DRY_RUN)); then
  echo "route (dry run): ${ADDRESS} -> worker ${WORKER}; ${HOST} -> ${PLACEHOLDER} (proxied)"
  echo "GET  /zones?name=${ZONE_NAME}"
  echo "GET  /zones/<zone>/dns_records?name=${HOST}          # POST an A record only when it answers empty"
  echo "GET  /zones/<zone>/email/routing/rules               # POST the rule only when no literal ${ADDRESS} rule exists"
  echo "PUT  /zones/<zone>/email/routing/rules               {\"enabled\":true,\"priority\":0,\"matchers\":[{\"type\":\"literal\",\"field\":\"to\",\"value\":\"${ADDRESS}\"}],\"actions\":[{\"type\":\"worker\",\"value\":[\"${WORKER}\"]}]}"
  echo "GET  /zones/<zone>/email/routing/rules               # re-read: the catch-all must be untouched"
  exit 0
fi

# shellcheck source=../scripts/lib/release-lib.sh
source "$ROOT/scripts/lib/release-lib.sh"
CF_TOKEN="$(cf_token)"
if [[ -z "$CF_TOKEN" ]]; then
  echo "route: no Cloudflare token — set CLOUDFLARE_API_TOKEN or write ~/.cloudflare-token" >&2
  exit 1
fi

api() { curl -s --max-time 30 -H "Authorization: Bearer ${CF_TOKEN}" "$@"; }

ZONE="${CLOUDFLARE_ZONE_ID:-}"
if [[ -z "$ZONE" ]]; then
  ZONE="$(api "https://api.cloudflare.com/client/v4/zones?name=${ZONE_NAME}" |
    python3 -c 'import json,sys; r=json.load(sys.stdin).get("result") or []; print(r[0]["id"] if r else "")')"
fi
if [[ -z "$ZONE" ]]; then
  echo "route: set CLOUDFLARE_ZONE_ID — no zone named ${ZONE_NAME} is visible to this token" >&2
  exit 1
fi

echo "=== [mail] routing (zone ${ZONE}) ==="

# ── the read host's DNS record ──────────────────────────────────────────────────────────────────────────
if api "https://api.cloudflare.com/client/v4/zones/${ZONE}/dns_records?type=A&name=${HOST}" | grep -q '"count":0'; then
  echo "  creating ${HOST} -> ${PLACEHOLDER} (proxied)"
  api -X POST "https://api.cloudflare.com/client/v4/zones/${ZONE}/dns_records" \
    -H 'content-type: application/json' \
    --data "{\"type\":\"A\",\"name\":\"${HOST}\",\"content\":\"${PLACEHOLDER}\",\"proxied\":true,\"ttl\":1,\"comment\":\"the mail worker's read host: proxied placeholder, the route hands it to the worker\"}" \
    >/dev/null
else
  echo "  ${HOST} already has an A record"
fi

# ── the one address's rule ──────────────────────────────────────────────────────────────────────────────
RULES="$(api "https://api.cloudflare.com/client/v4/zones/${ZONE}/email/routing/rules")"
if echo "$RULES" | grep -q "\"value\":\"${ADDRESS}\""; then
  echo "  the rule for ${ADDRESS} already exists"
else
  echo "  creating the rule ${ADDRESS} -> worker ${WORKER} (priority 0, ahead of the catch-all)"
  RULES="$(api -X POST "https://api.cloudflare.com/client/v4/zones/${ZONE}/email/routing/rules" \
    -H 'content-type: application/json' \
    --data "{\"enabled\":true,\"priority\":0,\"matchers\":[{\"type\":\"literal\",\"field\":\"to\",\"value\":\"${ADDRESS}\"}],\"actions\":[{\"type\":\"worker\",\"value\":[\"${WORKER}\"]}]}")"
  echo "$RULES" | grep -q '"success":true' || {
    echo "route: the rule was refused: $(echo "$RULES" | head -c 300)" >&2
    exit 1
  }
fi

# ── the re-read, which is what "别删不该删的" means mechanically ─────────────────────────────────────────
echo "  re-read — every rule in the zone, so the catch-all is visible where it stands:"
api "https://api.cloudflare.com/client/v4/zones/${ZONE}/email/routing/rules" |
  python3 -c '
import json, sys

def matcher(m):
    return str(m.get("field", "")) + "=" + str(m.get("value", "all"))

rules = json.load(sys.stdin).get("result") or []
for rule in rules:
    matchers = ",".join(matcher(m) for m in rule.get("matchers", []))
    actions = ",".join(a["type"] + ":" + str(a.get("value")) for a in rule.get("actions", []))
    print("    enabled=%s priority=%s %s -> %s" % (rule["enabled"], rule["priority"], matchers, actions))
'
