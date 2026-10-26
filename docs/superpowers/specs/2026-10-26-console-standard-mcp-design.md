# The console speaks standard MCP: one endpoint an AI client can reach

> **Status: design, 2026-10-26.** This is the additive half of the operator-access question: it does
> not change the device channel, the grant format, the audit schema, or the custom `POST /mcp` the
> console page uses. The data-path change (operator traffic over the mesh) is a separate proposal,
> `2026-10-26-console-mesh-slice-2-plan.md`, and waits for the operator's go-ahead.

## The problem

The console can control a device today, but only through an endpoint shaped for its own page:

- `POST /mcp` takes `{device_id, grant, request}` — a console-specific envelope, not a standard MCP
  transport. It has no `initialize`, no `Mcp-Session-Id`, no SSE, and it requires an operator session
  in addition to the grant.
- A standard MCP client (DeepSeek Harness, Claude, any `StreamableHTTPClientTransport`) sends a bare
  JSON-RPC body, expects to negotiate with `initialize`, and authenticates with a header it was
  configured with. It cannot talk to `POST /mcp` without an adapter.
- The device's own `/mcp` *is* standard streamable HTTP (rmcp, stateful, SSE-capable), but it is
  loopback-only and behind the install token. It is not reachable by a remote client, and reaching it
  would bypass the console's audit.
- The legacy gateway offered a standard MCP endpoint; the rewrite did not carry it. The parity sweep
  records the gap: "the console has no way to hand an AI client an MCP endpoint plus a grant"
  (`2026-10-25-legacy-parity-sweep.md`, "AI-client onboarding").

So "the console controls the device" is true for the page and false for the class of clients MCP
exists for. This design closes that gap without making the console a second tool implementation.

## The shape

One new route per device, in its own namespace:

```
POST /mcp/<device-id>
Authorization: Bearer <lowercase hex of the GrantEnvelope JSON bytes>
Content-Type: application/json
Accept: application/json, text/event-stream

{"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}
```

The exact `POST /mcp` stays the page's custom envelope. The new route is the console's **standard MCP
transport**. The device remains the tool host; the console remains the broker. The path names the
device because the grant already names it.

### Why `/mcp/<id>` and not `/fleet/<id>/mcp`

The route has to be expressible as a Cloudflare Access **path**, because the AI client is not an
operator and must not receive an operator credential. `/fleet/<id>/mcp` is a mid-path wildcard, which
an Access path pattern may not express; `/mcp/*` is a prefix. So the standard route lives under
`/mcp/<id>`, beside the page's exact `/mcp`, and the deployment adds one Access application for
`console.example.com/mcp/*`:

- **bypass** — the signed grant is the credential; or
- **a service-token policy** — the client also presents `CF-Access-Client-Id` /
  `CF-Access-Client-Secret`.

The choice is the operator's, and the documentation names it as a dashboard step. What must **not**
happen is the AI client receiving the operator application's credential: `access.rs` turns any verified
Access JWT (including a service token) into a full operator, so such a client could mint any grant,
read the audit, and delete devices. That would make the grant a hint rather than a boundary. The
console's app-level authorization on this route is the grant, never the Access identity.

A loopback/tunnel deployment without Access needs no dashboard step.

### The credential is the grant, not a new key store

The bearer is the signed grant the console already mints at `POST /grants`:

```
{"grant":{"device_id":"<64 hex>","tools":["files.read", ...],
          "expires_at_ms":..., "allow_destructive":false},
 "signature":"<128 hex>"}
```

Lowercase hex is the encoding this service already uses for every secret and key on the wire
(`console/src/hex.rs`), so the grant joins that rule instead of introducing a second one. A 38-tool
grant is roughly 2.2 KB encoded, well under the header limits of the console, Cloudflare, and the
client; the endpoint refuses a header it cannot decode with `401` rather than guessing. The console
decodes it, verifies the signature with its own key, checks the path's device id, and checks expiry —
the same `grants::verify` the custom proxy uses. No new store, no second credential type, no minting
on this route.

Why this is enough:

- the grant is already a bearer capability by design (no subject, no grant id);
- its tool list is enforced twice — the console refuses a `tools/call` outside it before the frame
  leaves, and the device's `remote::admit` re-verifies the same bytes offline;
- `allow_destructive` is signed, so a grant without it still produces the device's `Ask` refusal for a
  destructive tool, and the refusal names the flag;
- the TTL is 60 s – 7 days, chosen at mint time.

What it does **not** add: revocation before expiry. That is the existing grant model, not a property
of this endpoint. A deployment that needs revocation needs a grant id and a revocation list (or a
device-scoped key store), which is a larger decision and is out of scope here.

## Authentication, ordering, and which log

The audit index has one invariant (`audit.rs`): it holds **authenticated** actions, and a request that
fails authentication never reaches it. The denial log is separate, rate-limited and capped, so an
unauthenticated flood cannot rotate the operator's history away. The new route has no operator
session, so the grant is the authentication, and the order follows from that:

1. **Media types** — a wrong `Content-Type` is `415`, an `Accept` without JSON is `406`. No credential
   is read and nothing is logged; these are malformed transport requests, not authentication attempts.
2. **The bearer** — absent or not decodable as hex JSON is `401`, recorded in **`denials.jsonl`**.
3. **The signature, the device binding and the expiry** — verified before the body is read. Each
   failure is `403` with the named code, recorded in **`denials.jsonl`**, because the caller has not
   proved a credential.
4. **The registry** — only now is the device looked up. An id that is not registered is `404`,
   recorded in **`audit.jsonl`** as `denied` / `unknown_device`: the caller held a valid grant, so the
   action is authenticated, and a caller without a grant never learns whether an id exists.
5. **The body** — read only after the grant verifies, at most 8 MiB.
6. **The tool call or transport outcome** — `forward` writes the `mcp` / `denied` row, exactly as the
   custom route does.

This differs from the custom route on purpose: there the operator session authenticates the caller
before the device check, so its unknown-device and bad-grant rows are authenticated actions and stay in
`audit.jsonl`. Its order and rows do not change.

Verifying the grant before reading the body matters for a public route: a body cap that is enforced
after buffering still lets an anonymous caller make the console hold 8 MiB per request. The handler
takes the request, checks the headers and the grant, and only then reads a bounded body.

## The protocol

The console answers the transport locally and forwards only what the device owns.

| Request | Handling |
|---|---|
| `initialize` | answered by the console: `protocolVersion` `2025-06-18`, `capabilities.tools.listChanged=false`, `serverInfo.name="summrise-console"`, `serverInfo.version` = the console's build version, `instructions` naming the one device. No `Mcp-Session-Id`; a client that sends one is ignored. |
| `notifications/initialized` | `202 Accepted`, empty body. No device frame. |
| `ping` | `{"jsonrpc":"2.0","id":...,"result":{}}`, answered locally. |
| `tools/list` | forwarded to the device over the existing channel (the device's registry is the one schema source); the returned `tools` array is **filtered to the grant's tool names** before it is returned. |
| `tools/call` | forwarded exactly as the custom proxy forwards it: tool scope checked, the same grant attached, the same channel, the same device-side verification and audit. |
| anything else | JSON-RPC `-32601` method not found. A batch array is `-32600` invalid request. A notification other than `initialized` is accepted with `202` and dropped. |

**Why `tools/list` is forwarded instead of built from `summrise-core`.** The registry names,
descriptions and destructive flags live in core, but the *input schemas* live in
`device/src/mcp.rs` and are the device's. Copying them into the console would be a second source that
drifts — the failure the one-registry rule exists to prevent. Forwarding costs one round trip and
makes an offline device unable to list its tools, which is honest: a client cannot call a device that
is not there.

**No SSE in v1.** Responses are `application/json`. The MCP streamable-HTTP specification allows a
server to answer either JSON or an SSE stream, and the official client used here
(`@modelcontextprotocol/client`, `StreamableHTTPClientTransport`) advertises both in its `Accept`
header. Nothing in this endpoint sends server-initiated messages, so an SSE stream would be a
long-lived connection with nothing to say. The client does open a `GET` SSE stream after
`initialize`; this route has no `GET` handler, so the answer is `405`, which the client tolerates
(measured with the official client against this endpoint — see the tests section). If a real client
turns out to require SSE, that is an additive change to this route, not a protocol redesign.

**No session id in v1.** The transport is stateless: every request carries the grant and a device path,
so there is nothing to keep between requests. If the verified client rejects an absent
`Mcp-Session-Id`, the fallback is an opaque in-memory id with a one-hour expiry and a bounded map;
that is deliberately not built until a client needs it.

**Accept and content type.** `Content-Type: application/json` is required (a missing or wrong type is
`415`). The `Accept` header must allow JSON; a client that sends only `text/event-stream` gets `406`
rather than a hang. An absent `Accept` is `*/*` by HTTP's rule and is allowed.

## The one authorization path

The new route must not become a second implementation of tool authorization. The work is a refactor of
the existing proxy into shared pieces, and both routes call them:

```
proxy_mcp (custom envelope)  ─┐
                              ├─> verify_presented + ensure_registered + forward
standard_mcp (JSON-RPC)      ─┘
```

- `verify_presented(app, device_id, grant) -> Result<(), GrantRefusal>` — signature, device binding,
  expiry. It does **not** log: the caller's route decides whether the refusal is a denial (no
  operator session) or an audit row (an operator session already proved a credential).
- `ensure_registered(app, device_id) -> Result<(), ApiError>` — the registry check.
- `forward(app, device_id, grant, audit_id, request, filter) -> Response` — the tool-scope check, the
  `runs.exec` wait extension and the general `mcp_timeout`, the channel request, the error mapping
  (`Offline` → 503 with `retry_after_ms`, `Busy` → 503 with a shorter hint, `Disconnected` → 502,
  `ReplyTimeout` → 504), and the console audit row (`mcp` on a device answer, `denied` with a detail on
  a refusal or transport failure; only the tool name, never its arguments). Everything `forward`
  records is an authenticated action, so it may own that row.

The custom route keeps its exact public shape and status codes: `ensure_registered`, then
`verify_presented`, then `forward`, with its refusals audited as they are today. The standard route
verifies first, records pre-authentication refusals in the denial log, then `ensure_registered` (an
audited `denied` when it fails), then `forward`. The `tools/list` filter is a parameter to `forward`
so the custom route keeps its unfiltered behaviour (the page does not use it).

## What does not change

- the grant format, its single-device binding, its expiry and its `allow_destructive` flag;
- the device channel envelope, the device's `remote::admit`, and the device's audit;
- the device's own `/mcp` and its install-token gate;
- the custom `POST /mcp`, its operator session, and the console page;
- the console's role as the only remote data path today (the mesh is the separate proposal);
- the audit schema (`kind` stays `enroll | register | grant | mcp | denied`);
- the audit/denial split and the denial log's rate limit.

## Bounds and errors

| Condition | Answer | Log |
|---|---|---|
| wrong `Content-Type` | `415 {"error":"unsupported_media_type"}` | none |
| `Accept` without JSON | `406 {"error":"not_acceptable"}` | none |
| no `Authorization`, or a bearer that is not hex JSON | `401 {"error":"unauthorized"}` | denials |
| bad signature | `403 {"error":"invalid_grant_signature"}` | denials |
| authentic grant, wrong device | `403 {"error":"grant_device_mismatch"}` | denials |
| authentic grant, expired | `403 {"error":"grant_expired"}` | denials |
| valid grant, unknown device | `404 {"error":"unknown_device"}` | audit `denied` |
| tool outside the grant | `403` JSON-RPC `-32001` `"tool not permitted by grant"` | audit `denied` |
| device offline | `503 {"error":"device reconnecting","retry_after_ms":1000}` | audit `denied` |
| channel busy / disconnected / timeout | `503` / `502` / `504`, as the custom route | audit `denied` |
| body over 8 MiB (after the grant verifies) | `413` | none |
| malformed JSON | `400` JSON-RPC `-32700` | none |
| batch array / missing `method` / missing `jsonrpc` | `400` JSON-RPC `-32600` | none |
| unknown method | `200` JSON-RPC `-32601` | none |
| `tools/call` with no `params.name` | `200` JSON-RPC `-32602` | audit `denied` (`invalid_tool_call`) |

A device `error` envelope is a JSON-RPC answer, not a transport failure: it is returned as HTTP 200
with the body unchanged, exactly as the custom proxy does. A tool-level refusal (`isError: true`) is
also HTTP 200.

## Tests

The console's integration suite already has the pieces: `Fixture`, a real `TestServer`, a fake device
that opens the WebSocket and answers frames, and `create_grant`. The standard route is tested through
the same seam:

1. **initialize** — 200, `protocolVersion` `2025-06-18`, `tools` capability, `serverInfo.name`, the
   device id in `instructions`, and no device frame.
2. **initialized notification** — 202, empty body, no device frame.
3. **tools/list is filtered** — the fake device answers three tools, the grant names one, the response
   contains one; the frame the device received is the bare `tools/list` request.
4. **tools/call round trip** — the same result the custom proxy returns, with the same frame shape on
   the device side.
5. **a tool outside the grant** — 403 JSON-RPC `-32001`, one audited `denied` row, and the device
   receives nothing.
6. **an unknown device with no grant** — 401, denied, and no device lookup (the enumeration oracle is
   closed).
7. **a valid grant for an unknown device** — 404, audited `denied`.
8. **a bad, mismatched or expired grant** — 403 with the named code in `denials.jsonl`; no device
   frame.
9. **an offline device** — 503 after the reconnect wait; the body names the condition; the row is in
   `audit.jsonl`.
10. **an unknown method** — 200 `-32601`; a batch array is HTTP 400 with `-32600`; a wrong content type
    is 415; an `Accept` without JSON is 406; a malformed body is 400 `-32700`.
11. **one audit row per device call** — `mcp`/`tools/call` carries the tool name; an out-of-scope call
    carries `denied` and the tool name; `initialize` adds no row.
12. **the custom `/mcp` still behaves** — the existing tests are the regression, unchanged.
13. **a real client** — the official `@modelcontextprotocol/client`
    `StreamableHTTPClientTransport` against a running console with a fake device, through the
    `#[ignore]`d `the_official_mcp_client_can_connect` test. Measured on 2026-10-26: `initialize`,
    `tools/list` (filtered to the grant) and `tools/call` all succeed with no `Mcp-Session-Id`; the
    post-initialize `GET` (SSE) is answered `405` and tolerated. This is not a CI test (the product
    tree has no Node test lane); the test prints the endpoint and a short-lived grant for the client.

The grant header helper is part of the test fixture: one function that serializes a `GrantEnvelope`
and hex-encodes it, used by every test above.

## Rollout

1. Land the refactor and the new route with its tests; the custom route's tests stay green.
2. Add one Access application for `console.example.com/mcp/*` — **bypass** or a service-token policy
   — and never add the AI client to the operator application. Document the choice in
   `docs/CONNECT-MCP.md`.
3. Document the client configuration: mint a grant with `POST /grants`, hex-encode it, point the
   client at `https://<console>/mcp/<device>`, and add the `CF-Access-*` headers only when the
   service-token policy is chosen.
4. Canary: the official client lists and calls a tool through the console; the console's audit shows
   the call and the device's audit shows the same call.
5. A follow-up page card ("Connect an AI client") can mint the grant and show the endpoint and the
   header once. It is deliberately not in this change: the API is the product surface, and the card is
   a convenience whose copy must be right before it ships.

## My calls (the builder should not re-litigate these)

- `/mcp/<id>`, not `/fleet/<id>/mcp`: the Access path must be a prefix, and the AI client must not be
  an operator.
- The grant is the bearer; no new key store in this change.
- `tools/list` is forwarded, not rebuilt; the device's schemas are the only schemas.
- JSON responses only; no SSE, no session id until a verified client needs one.
- Refusals before a verified grant go to `denials.jsonl`; a verified grant's outcomes go to
  `audit.jsonl`. The grant is verified before the device lookup and before the body is read.
- Audit `tools/list` and `tools/call`; do not audit `initialize`/`ping`.
- 8 MiB body cap, matching the device's `/mcp`; the custom route keeps axum's 2 MiB.
- Hex, not base64: the console's one wire encoding, no new dependency.

## What would change the decision

- A client that requires SSE or a session id: add the shape the client requires behind the same route
  and the same grant check.
- A requirement to revoke a grant before its expiry: add a grant id and a revocation list (console) or
  a device-scoped key store; both are larger than this change and both are product decisions.
- An Access deployment where a bypassed route is unacceptable and a service-token policy is
  preferred: the same route works; only the dashboard step and the client headers change.
- Operator traffic moving to the mesh: the endpoint stays, and `forward` gains a path parameter; the
  grant and the audit stay the same.
