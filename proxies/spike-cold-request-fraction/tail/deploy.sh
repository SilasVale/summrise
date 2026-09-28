#!/usr/bin/env bash
# THROWAWAY tail probes: one script per gap, because ONE script has ONE isolate
# pool — three gaps cannot be tested from one worker without the tests refreshing
# each other's isolate. Deleted after the measurement.
set -euo pipefail
export CLOUDFLARE_API_TOKEN="$(cat "$HOME/.cloudflare-token")"
ZONE=79860b60a8681147d0ce38294d404ead
for g in 900 1800 3600; do
  host="p30tail${g}.saisi.online"
  curl -s -X POST -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" -H 'Content-Type: application/json' \
    "https://api.cloudflare.com/client/v4/zones/$ZONE/dns_records" \
    -d "{\"type\":\"AAAA\",\"name\":\"$host\",\"content\":\"100::\",\"proxied\":true,\"ttl\":1,\"comment\":\"P3.0 cold-fraction tail probe — THROWAWAY\"}" \
    | python3 -c "import json,sys; d=json.load(sys.stdin); print('dns $host success=',d.get('success'), json.dumps(d.get('errors'))[:200] if not d.get('success') else '')"
  mkdir -p "/tmp/p30frac/tail-$g"
  cp tail/index.js "/tmp/p30frac/tail-$g/index.js"
  cat > "/tmp/p30frac/tail-$g/wrangler.jsonc" <<JSON
{
  "name": "p30-coldfrac-tail$g",
  "main": "index.js",
  "compatibility_date": "2025-09-01",
  "routes": [{ "pattern": "$host/*", "zone_id": "$ZONE" }]
}
JSON
  ( cd "/tmp/p30frac/tail-$g" && wrangler deploy 2>&1 | tail -3 )
done
