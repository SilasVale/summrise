# Connecting an AI client to one device

The console carries a standard streamable-HTTP MCP endpoint for each device. It is **not** the page's
`POST /mcp` envelope: that route belongs to the console's own page and requires an operator session.
This endpoint is for an MCP client (DeepSeek Harness, any `StreamableHTTPClientTransport`), and its
credential is a console-signed grant.

## The endpoint

```
POST https://<console>/mcp/<device-id>
Authorization: Bearer <lowercase hex of the GrantEnvelope JSON>
Content-Type: application/json
Accept: application/json, text/event-stream
```

The path is the device id: 64 lowercase hex characters, the registered public key. The endpoint speaks
MCP `2025-06-18`; it answers JSON (no SSE), and every request carries the grant — there is no session.

The console page has the same flow without the curl: each device card has an **AI client** button,
which mints a grant through the page's own session and shows the endpoint, the header and a DeepSeek
Harness configuration. The steps below are for a script or an operator on a machine without the page.

## Mint a grant

A grant is minted by the console and signed with its grant key. It names one device, the tools the
client may call, an expiry and whether destructive tools are pre-approved. Use the operator token:

```bash
CONSOLE=https://console.example.com
SESSION=$(curl -sS -X POST "$CONSOLE/auth" \
  -H 'content-type: application/json' \
  -d '{"token":"<the 64-hex operator token>"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')

GRANT_JSON=$(curl -sS -X POST "$CONSOLE/grants" \
  -H "authorization: Bearer $SESSION" -H 'content-type: application/json' \
  -d '{"device_id":"<device-id>","tools":["files.read","files.list"],
       "expires_in_s":3600,"allow_destructive":false}')

GRANT_HEX=$(printf '%s' "$GRANT_JSON" | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin),separators=(",",":")).encode().hex())')
```

The hex is the envelope's JSON bytes. Re-serializing the JSON is safe: the signature is over the
grant's canonical message, not over this byte encoding. The grant is a **bearer**: whoever holds the
hex until `expires_at_ms` can call the tools it names on that one device. A one-hour grant is a
reasonable interactive session; a seven-day grant is not.

`allow_destructive` is the operator's signed consent. Leave it `false` and a destructive tool still
reaches the device, which answers with its `Ask` refusal naming the flag; set it `true` only for the
window in which you want a destructive call to run without a per-call prompt.

## DeepSeek Harness

```yaml
- id: mcp-summrise-device
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: summrise-device
    transport: streamable-http
    url: https://console.example.com/mcp/<device-id>
    headers:
      Authorization: !!js '`Bearer ${process.env.SUMMRISE_GRANT}`'
```

Put the hex in `SUMMRISE_GRANT` for that process; do not commit it. The tools appear under
`mcp__summrise-device__<tool>`.

If the console is behind Cloudflare Access with a **service-token** policy for the endpoint, add the
two headers the edge expects:

```yaml
      CF-Access-Client-Id: <the service-token id>
      CF-Access-Client-Secret: <the service-token secret>
```

## Cloudflare Access

The endpoint must be its own Access application for `console.example.com/mcp/*` — the wildcard path,
**not** the exact `/mcp`, which is the console page's own envelope and must keep its operator-session
gate:

- **bypass** — the signed grant is the credential; the client sends no Access headers; or
- **service token** — the client sends the two `CF-Access-*` headers, and the grant is still required.

Do **not** add the AI client to the operator application. Any verified Access JWT, including a
service token, is a full operator in this console: it can mint any grant, read the audit and delete
devices. The grant would then be a hint rather than a boundary. The endpoint's app-level authorization
is the grant, never the Access identity.

A loopback or tunnel deployment without Access needs no dashboard step.

## Security

- **One grant, one device.** The grant names the device id; the path must match it.
- **Tool scope is enforced twice:** the console refuses a `tools/call` outside the grant before the
  frame leaves, and the device re-verifies the same signed bytes offline.
- **No revocation before expiry.** Rotate by minting a new grant and letting the old one expire.
- **HTTPS only.** The grant is a bearer secret in a header; never put it in a URL, a shell history, a
  repository or a screenshot.

## Limits

- One device per endpoint; one endpoint per device.
- No SSE: a `GET` on the path is `405`, and the official client tolerates that.
- `tools/list` is answered by the device, so an offline device cannot list its tools.
- The body cap is 8 MiB (the device's own `/mcp` cap); files are base64 and capped at 1 MiB.
- `runs.exec` may block for its own timeout, up to ten minutes.

## A quick check

```bash
curl -sS -X POST "https://console.example.com/mcp/<device-id>" \
  -H "authorization: Bearer $GRANT_HEX" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'
```

The result carries `"protocolVersion":"2025-06-18"`, `"serverInfo":{"name":"summrise-console",...}`,
and instructions naming the one device. Replace the method with `tools/list` to see the grant's tools,
then `tools/call` with `{"params":{"name":"files.read","arguments":{"path":"notes.txt"}}}`.

The transport shape was verified with the official DeepSeek Harness MCP client
(`@modelcontextprotocol/client`, `StreamableHTTPClientTransport`) against this endpoint itself, with a
fake device behind it: `initialize`, `tools/list` (filtered to the grant) and `tools/call` all
succeeded, with no `Mcp-Session-Id` and with the client's post-initialize `GET` (SSE) answered `405`
and tolerated. The canary is the `#[ignore]`d `the_official_mcp_client_can_connect` test in the
console suite; it prints the endpoint and a short-lived grant for the client. The deployed endpoint's
canary is the same client against a real device.
