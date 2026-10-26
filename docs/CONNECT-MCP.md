# Connecting an AI client to one device

> In a hurry? [`QUICKSTART.md`](QUICKSTART.md) is the five-minute path from an installed device to a working client; this document is the long form.

The console carries a standard streamable-HTTP MCP endpoint for each device. It is **not** the page's
`POST /mcp` envelope: that route belongs to the console's own page and requires an operator session.
This endpoint is for an MCP client (DeepSeek Harness, any `StreamableHTTPClientTransport`), and its
credential is a bearer the console accepts: a console-signed grant for a session, or a permanent token
for a configuration that must not change.

## The endpoint

```
POST https://<console>/mcp/<device-id>
Authorization: Bearer <lowercase hex of the GrantEnvelope JSON, or srm_… for a token>
Content-Type: application/json
Accept: application/json, text/event-stream
```

The path is the device id: 64 lowercase hex characters, the registered public key. The endpoint speaks
MCP `2025-06-18`; it answers JSON (no SSE), and every request carries a grant, or a token the console
turns into one — there is no session.

The console page has the same flow without the curl: each device card's **AI client** button opens the
**Connect an AI client** card, which either mints a session grant or creates a permanent token through
the page's own operator session, and shows the endpoint, the header and a DeepSeek Harness
configuration. The curl below is for a script or an operator on a machine without the page.

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

## Permanent tokens

A grant expires, so a configuration built around one has to be rebuilt. A **permanent token** is the
console's answer for a client whose configuration must stay fixed: the console issues it once, the
client sends it on every request, and the console turns it into a short grant each time.

Create it in the console page: device card → **AI client** (**Connect an AI client**) → **Permanent
token**. Name it, choose the tools it may call, and set the destructive consent (the same
`allow_destructive` a grant carries). The token is `srm_` followed by 64 lowercase hex characters (68
in all), returned once at creation:

```
srm_<64 lowercase hex characters>
```

The console stores only the token's SHA-256 hash, so the page cannot show it again — not in the list,
not in a later session, not in the audit. It is a **bearer**: treat it like a password. Do not commit
it, and keep it out of a URL, a shell history and a screenshot.

The endpoint is the same; only the bearer changes:

```
POST https://<console>/mcp/<device-id>
Authorization: Bearer srm_<64 lowercase hex characters>
```

The console hashes the token, finds the record and checks that the record's device is the path's
device. It then mints a **60-second grant** naming exactly the token's tools and carrying the token's
destructive consent, and runs the existing grant path. The device receives an ordinary signed grant
and verifies it offline; a token never reaches it. An unknown or revoked token, or one created for
another device, is a `401`, and no frame is forwarded.

### A fixed DeepSeek Harness configuration

```yaml
- id: mcp-summrise-device
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: summrise-device
    transport: streamable-http
    url: https://console.example.com/mcp/<device-id>
    headers:
      Authorization: Bearer srm_<64 lowercase hex characters>
```

Replace the placeholder with the token the console showed you. The value is fixed: it does not change
when a grant expires, because this configuration holds no grant — the console mints one per request,
so there is nothing here to renew. The only edit it ever needs is a new value after you revoke a token
and create another. Keep the file that holds it out of the repository (or have the deployment inject
the literal from a secret); the header is the password. Behind Cloudflare Access, add the same
`CF-Access-*` headers as the grant configuration below — the token is the endpoint's credential, not
the edge's.

### Scope and revocation

A token is bound to **one device**, a **non-empty tool set**, and the destructive consent it was
created with. The path must match: a token for device A sent to `/mcp/<device B>` is refused with
`401`, and a tool outside its set never reaches the device, because the minted grant does not name it.
A token is not an operator session: it cannot mint grants, list devices, read the audit, or create
another token. The console accepts at most 64 tokens.

A token is **permanent by default**. Creation may give it an **expiry** between one hour and 365
days; the card's form offers Never, 30, 90 and 365 days, and the list says `expires <date>` or `never
expires`. An expired token is refused with `401` (the denial log keeps `expired_api_token`) before it
can mint a grant; expiry is checked before the device match. **Rotate** keeps the lifetime policy —
the new value gets the same duration counted from the rotation, not the old absolute date.

Revocation is the only withdrawal for a permanent token. The device card's token list shows each
token's name, creation date, tool set, expiry and **last use** (`last used <date>`, or `never used`) —
never the token — and **Revoke** asks for confirmation. After it, the next request with that token is
`401` and the token is gone from the list: neither the page nor the audit can show it again.

The audit names the token's **record id** (`token_id`, not the secret) on every MCP line that token
authenticated, so a call can be attributed to the credential that made it; the grant-authenticated
path and the page's operator route carry no `token_id`. The audit table resolves that id to the
token's name when the token list has it, shows the short id otherwise, and its **Token** filter
narrows the window to one credential. The `srm_…` value never enters the audit.

**Rotate** on the same row replaces a token in one action: the new value keeps the name, device,
tools and destructive consent, the old value stops working the moment the replacement succeeds, and
the new value is shown once. The audit records the change as a `token` creation and a `token_revoke`
with detail `rotated`, both naming the device. Rotation writes the new record and drops the old in a
single atomic file replacement.

### Which one to use

- **Grant** — an interactive session you start and end. It expires on its own, so there is no
  revocation step, and a short lifetime limits a leak. Mint a fresh one when it expires; the config
  changes, and that is the cost.
- **Permanent token** — a client whose configuration cannot be rewritten: an unattended agent, a CI
  job, a workstation assistant. Configure once, then manage access by creating and revoking tokens. It
  is a long-lived password, so scope it narrowly and revoke it when the client is retired.

If the client can be handed a fresh grant each session, that is the safer default. A token is for the
configurations that cannot.

## DeepSeek Harness

Below is the grant path, for a session. For the same client with a fixed credential, see
[A fixed DeepSeek Harness configuration](#a-fixed-deepseek-harness-configuration) above.

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
`mcp__summrise-device__<tool>`. A new grant means a new `SUMMRISE_GRANT`; the token form above has no
grant to renew.

Both forms need the two edge headers if the console is behind Cloudflare Access with a
**service-token** policy for the endpoint:

```yaml
      CF-Access-Client-Id: <the service-token id>
      CF-Access-Client-Secret: <the service-token secret>
```

## Cloudflare Access

The endpoint must be its own Access application for `console.example.com/mcp/*` — the wildcard path,
**not** the exact `/mcp`, which is the console page's own envelope and must keep its operator-session
gate:

- **bypass** — the bearer (a signed grant or an `srm_` token) is the credential; the client sends no
  Access headers; or
- **service token** — the client sends the two `CF-Access-*` headers, and the bearer is still required.

Do **not** add the AI client to the operator application. Any verified Access JWT, including a
service token, is a full operator in this console: it can mint any grant, read the audit and delete
devices. The bearer would then be a hint rather than a boundary. The endpoint's app-level authorization
is the bearer — grant or token — never the Access identity.

A loopback or tunnel deployment without Access needs no dashboard step.

## Security

- **One grant or token, one device.** A grant names the device id and the path must match it; a token
  is bound to one device at creation, and the console refuses a path that names another.
- **Tool scope is enforced twice:** the console refuses a `tools/call` outside the grant before the
  frame leaves (for a token, outside the token's tool set), and the device re-verifies the same signed
  bytes offline.
- **Grants cannot be revoked before expiry.** Rotate by minting a new grant and letting the old one
  expire. A token is permanent unless created with an expiry; **Revoke** in the device card withdraws
  it, and its next request is
  `401`.
- **HTTPS only.** The bearer is a secret in a header; never put it in a URL, a shell history, a
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
and instructions naming the one device. Replace the method with `tools/list` to see the grant's tools
(for a token, the token's tools), then `tools/call` with
`{"params":{"name":"files.read","arguments":{"path":"notes.txt"}}}`.

The credential above is a grant. Send an `srm_…` token instead to exercise the permanent-token path:
the console mints the 60-second grant behind the request, and the response is the same.

The transport shape was verified with the official DeepSeek Harness MCP client
(`@modelcontextprotocol/client`, `StreamableHTTPClientTransport`) against this endpoint itself, with a
fake device behind it: `initialize`, `tools/list` (filtered to the grant) and `tools/call` all
succeeded, with no `Mcp-Session-Id` and with the client's post-initialize `GET` (SSE) answered `405`
and tolerated. The canary is the `#[ignore]`d `the_official_mcp_client_can_connect` test in the
console suite; it prints the endpoint and a short-lived grant for the client.

The deployed endpoint was verified with the same client against a real device on 2026-10-26:
`initialize`, `tools/list` (filtered to the grant) and `tools/call host.facts` answered through the
console's public Access edge, with the operator's service token for the test and the grant still the
endpoint's authorization. The first public call failed at the edge and the same grant answered on
retry; see `RELEASE.md` for the tunnel caveat. The AI client's own gate is the separate `/mcp/*`
application described above.
