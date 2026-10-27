# The aggregate MCP: one connection, every device

**Date:** 2026-10-27
**Status:** accepted for implementation

## Problem

The console's standard MCP endpoint is per device: `POST /mcp/<device-id>`, with a credential
scoped to that device. An AI client that should reach three machines therefore needs three MCP
servers, three tokens and three configuration entries, and the tools it sees are three separate
copies of the same tool names.

The legacy gate (`ai.saisi.online/mcp`) offered the other shape: one MCP server whose tools take a
`device` argument. Operators who used it want that ergonomics without giving up the console's
per-device grant verification, audit line and revocation. This spec adds that aggregate endpoint
to the console. It does not resurrect the gate: the console still signs an ordinary per-device
grant for every call, and the device still verifies it offline.

## Decision

Add **one** standard MCP endpoint, `POST /mcp/all`, authenticated by an **aggregate token**. An
aggregate token names a non-empty list of devices, a non-empty set of registry tools and the
operator's destructive consent. The endpoint is the standard MCP transport (JSON-RPC over HTTP,
`Accept: application/json`); it is not the operator-session proxy at `/mcp`.

The tool surface is the device's own tool surface with one added argument:

- every tool's `inputSchema` gains a **required** `device` string property;
- `device` is a device id, or a device name that is unique among the token's devices;
- the aggregate strips `device` before forwarding, so the device never sees it;
- the console mints a sixty-second grant for **the resolved device**, with the token's tools and
  destructive consent, exactly as `/mcp/<device-id>` does for a device token.

`tools/list` is answered from the first allowed device that currently has a channel. This keeps the
schemas device-generated (the console does not maintain a second copy) and works while any one of
the token's devices is online.

## Token model

`Token` (the `tokens.json` record) gains one optional field:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub device_ids: Option<Vec<String>>,
```

A **device token** has `device_ids: None` and a non-empty `device_id`; an **aggregate token** has
`device_id: ""` and `device_ids: Some(ids)`. The two shapes are mutually exclusive, validated at
creation, and the empty `device_id` cannot equal a registered device id (64 lowercase hex), so an
aggregate token is inert on `/mcp/<device-id>` even before the explicit refusal.

`TokenMeta` exposes `device_ids` so the operator's list shows the scope. `Draft` becomes a scope
enum so a caller cannot build the invalid "both" or "neither" shape:

```rust
pub enum TokenScope {
    Device(String),
    Devices(Vec<String>),
}

pub struct Draft {
    pub name: String,
    pub scope: TokenScope,
    pub tools: Vec<String>,
    pub allow_destructive: bool,
    pub expires_in_s: Option<u64>,
}
```

`Tokens::create` takes the known device set (not a single `bool`), so every id in an aggregate
scope is checked. It refuses an empty list, duplicates and unknown ids with distinct errors. The
record keeps the ids exactly as created; removing a device later does not rewrite a token (the
removed id is inert), which keeps revocation of a machine independent from token lifetime.

Rotation copies the whole scope. `last_used_ms` and the hash machinery are unchanged.

## Protocol

`POST /mcp/all` mirrors `POST /mcp/:id` for transport and protocol version, with these differences:

| method | answer |
|---|---|
| `initialize` | aggregate server info and instructions naming every allowed device id and name; no device is contacted |
| `ping` | `{}` |
| `notifications/*` | `202 Accepted`, no body |
| `tools/list` | the first allowed device with a channel; its tools filtered to the token's tools, each schema with the required `device` property |
| `tools/call` | `params.arguments.device` is required; resolve it, strip it, mint a grant for that device, forward |
| anything else | JSON-RPC `-32601` |

`device` is a free string, not an `enum`: a device name is mutable and a client that validates an
enum of ids could not use names. The console is the authority; the property's description lists
the current `id (name)` pairs. Resolution order inside the token's scope: an exact id wins; a
name that matches exactly one of the token's devices resolves to that id; a name matching more
than one is refused as ambiguous; anything else is a scope refusal.

A 60-second grant per call means the device's offline verification, tool enforcement and
destructive checks are unchanged. The token itself is never sent to the device.

## Refusals and auditing

| condition | wire answer | record |
|---|---|---|
| no bearer, unknown/expired/revoked token, device token on `/mcp/all`, grant on `/mcp/all` | `401` | denial log (`app.refuse`/`refuse_as`) |
| authenticated aggregate token, device outside its scope | `403` JSON-RPC `-32001 device_not_permitted` | operator audit, `denied`, with the actual requested device and token id |
| missing/non-string `device` | `422` or `400` JSON-RPC `-32602 invalid params` | denial or audit as the standard route does for a malformed tool call |
| tool not in the token's tools | `403` JSON-RPC `-32001 tool not permitted` | audit, token id |
| no allowed device has a channel (`tools/list`) | `503` with `retry_after` | the standard failure body |
| device offline/timeout for `tools/call` | the standard `503`/`502`/`504` | audit, token id, actual device |

Every forwarded line records the actual device id, never the aggregate path, and the token id.
A token cannot widen its own scope: the grant is signed from the record.

## Non-goals

- No multi-device **grant** and no change to the device protocol.
- No per-device tool sets inside one aggregate token; the tool set is one list applied to every
  named device. (A future spec could allow `{device_id, tools}` pairs; it is more UI and more
  scope for a leak, and nothing here needs it.)
- The `/mcp` operator-session proxy stays as it is; it is not a static AI-client credential.
- The legacy gate is not changed or removed by this feature.

## Compatibility and rollout

Existing device tokens deserialize with `device_ids: None` and behave exactly as before. A new
`tokens.json` read by an older console ignores the optional field; the record's empty `device_id`
keeps it inert. The console's version is bumped with the release; the Windows build is deployed
with the usual backup/rollback.

Once the endpoint is live, one DSH entry replaces the three per-device entries; the aggregate
token is created in the console, shown once, and pasted into that entry. The old gate can retire
after the aggregate entry has served real work.

## Acceptance

1. An aggregate token for `{A, B, C}` can `initialize`, `tools/list` and `tools/call` on `/mcp/all`
   with `device` naming any of the three by id or by unique name.
2. The device receives the call without `device` and an ordinary grant for itself; the audit line
   names the token id and that device.
3. A call naming D, a device outside the scope, is refused before a frame is sent; a name shared by
   two devices is ambiguous, not guessed.
4. Device tokens stay refused on `/mcp/all`; aggregate tokens stay refused on `/mcp/<id>`.
5. Removing a device leaves existing aggregate tokens intact and the removed id inert.
6. `cargo test -p summrise-console`, the web suite and the Windows console build are green.
