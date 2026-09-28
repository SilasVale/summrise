#!/usr/bin/env bash
# teardown.sh — delete every resource this measurement created, then RE-QUERY.
# A teardown that is not re-queried is a claim (P3.0's own rule).
set -uo pipefail
export CLOUDFLARE_API_TOKEN="$(cat "$HOME/.cloudflare-token")"
ACCOUNT=8e9ea6cb01f2336a2f00039a51f96c6d
ZONE=79860b60a8681147d0ce38294d404ead
API=https://api.cloudflare.com/client/v4

echo "=== DELETE workers ==="
for w in p30-coldfrac-probe p30-coldfrac-tail900 p30-coldfrac-tail1800 p30-coldfrac-tail3600 p30-coldfrac-probe-wasm; do
  code=$(curl -s -o /tmp/p30frac/del.json -w '%{http_code}' -X DELETE -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/accounts/$ACCOUNT/workers/scripts/$w")
  echo "  DELETE $w -> HTTP $code $(python3 -c "import json;d=json.load(open('/tmp/p30frac/del.json'));print('success=',d.get('success'))" 2>/dev/null)"
done

echo "=== DELETE routes matching p30 ==="
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/zones/$ZONE/workers/routes" \
 | python3 -c "
import json,sys,subprocess,os
d=json.load(sys.stdin)
tok=os.environ['CLOUDFLARE_API_TOKEN']; zone='$ZONE'
for r in d.get('result',[]):
    if 'p30' in r['pattern']:
        c=subprocess.run(['curl','-s','-o','/dev/null','-w','%{http_code}','-X','DELETE','-H',f'Authorization: Bearer {tok}',f'https://api.cloudflare.com/client/v4/zones/{zone}/workers/routes/'+r['id']],capture_output=True,text=True)
        print('  DELETE route',r['pattern'],'-> HTTP',c.stdout)
"

echo "=== DELETE DNS records matching p30 ==="
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/zones/$ZONE/dns_records?per_page=200" \
 | python3 -c "
import json,sys,subprocess,os
d=json.load(sys.stdin)
tok=os.environ['CLOUDFLARE_API_TOKEN']; zone='$ZONE'
for r in d.get('result',[]):
    if r['name'].startswith('p30'):
        c=subprocess.run(['curl','-s','-o','/dev/null','-w','%{http_code}','-X','DELETE','-H',f'Authorization: Bearer {tok}',f'https://api.cloudflare.com/client/v4/zones/{zone}/dns_records/'+r['id']],capture_output=True,text=True)
        print('  DELETE dns',r['type'],r['name'],'-> HTTP',c.stdout)
"

echo
echo "=== RE-QUERY (the part that makes the teardown a fact) ==="
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/accounts/$ACCOUNT/workers/scripts" \
 | python3 -c "
import json,sys; d=json.load(sys.stdin)
hit=[s['id'] for s in d.get('result',[]) if 'p30' in s['id']]
print('VERIFY workers matching p30 :', hit if hit else 'NONE')
print('VERIFY all workers on account:', ', '.join(s['id'] for s in d.get('result',[])))
"
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/zones/$ZONE/workers/routes" \
 | python3 -c "
import json,sys; d=json.load(sys.stdin)
hit=[r['pattern'] for r in d.get('result',[]) if 'p30' in r['pattern']]
print('VERIFY routes matching p30  :', hit if hit else 'NONE')
print('VERIFY all routes on zone   :', ', '.join(r['pattern'] for r in d.get('result',[])))
"
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$API/zones/$ZONE/dns_records?per_page=200" \
 | python3 -c "
import json,sys; d=json.load(sys.stdin)
hit=[r['name'] for r in d.get('result',[]) if r['name'].startswith('p30')]
print('VERIFY dns matching p30     :', hit if hit else 'NONE')
"
echo "VERIFY probe host answers   :"
curl -s -o /dev/null -w '  https://p30frac.saisi.online/probe -> HTTP %{http_code}\n' --max-time 20 https://p30frac.saisi.online/probe || echo "  (connection failed — no worker serves it)"
