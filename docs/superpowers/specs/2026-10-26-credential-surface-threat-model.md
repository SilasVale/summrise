# The credential surface: a threat model

> **Status:** 2026-10-26. Scope: the console and the device runtime (`device/`), not the legacy
> `summrise-legacy` tree or the operator's own machines. This is a design record, not a promise: it
> names the assets, the boundaries, the controls that exist in the tree, and the risks that remain.

## Assets

| Asset | Where it lives | What compromise buys |
|---|---|---|
| Console grant-signing key | console data dir (`grant.key`), never leaves it | Mint any grant for any registered device; the console is the authority |
| Operator token | operator's file; console holds none (hashes/compares) | Full operator: enroll, grants, tokens, audit |
| Operator session | browser cookie; console `sessions.json` holds the hash; 30 d | Same as the operator token until expiry |
| API token (`srm_…`) | shown once; console stores SHA-256 | A scoped, revocable device credential |
| Device identity key | device `<data>/identity.key` | Impersonate the device to the console/mesh |
| Device channel token | device `<data>/console-token.json`; console stores its hash | Reconnect as the device, consume its frames |
| Install token | device `<data>/token` (ACL-protected) | Full local owner: every tool, destructive included |
| Panel token | device memory, served only in the loopback HTML | The panel's enroll POST for that boot |
| Enrollment code | console hashes only; single use; 900 s | Register one new device |
| Audit / denial logs | console data dir | Operator's history; no payload bodies are written |

## Boundaries

1. **Operator browser ↔ edge ↔ console.** TLS and Cloudflare Access terminate at the edge; the console
   verifies the Access JWT when configured and otherwise checks the operator session. The page's session
   is an `HttpOnly`, `SameSite=Lax` cookie scoped to `/next` (`Secure` over TLS); no API mutation
   accepts the cookie, and every `/next` mutation carries the session's CSRF value, which is not in it.
2. **Console ↔ device.** The device dials **outbound** `wss://console/devices/<id>/channel`; it never
   listens for the network. The console forwards MCP frames over that channel; the device re-verifies
   the grant offline against its pinned console key.
3. **Device loopback.** `127.0.0.1:18080` only: the panel (no login; loopback is the boundary), `/mcp`
   and the JSON APIs (install token), the helper registration (per-boot token), and the panel's
   enroll POST (per-boot panel token + loopback peer).
4. **Install vs data.** The launcher is the only writer of the install; the data dir holds identity,
   tokens and state, with Windows ACLs (`protect_dir`) or 0600 files on Unix.

## Controls in the tree

- **Offline grants.** The device verifies the console's Ed25519 signature and never needs the console
  online; the private key never leaves the console. Grant lifetime is 60 s–7 d; the device tolerates a
  bounded clock skew.
- **Permanent tokens.** `srm_` + 32 random bytes, shown once, SHA-256 at rest, device/tool scoped,
  optional 1 h–365 d expiry, revocable, rotated in one atomic write. The console exchanges a token
  for a 60-second grant; the device never sees the token.
- **Single-use enrollment.** 900 s, hashed at rest, spent by a signature over the device key, code and
  timestamp; the device id is the public key.
- **Revocation durability.** `tokens.json` has no backup. `devices.json` and `codes.json` write the
  **current** list to both the file and its `.bak`, so a lost primary cannot resurrect a removed device
  or a spent code (the pre-fix behavior restored the previous list).
- **Loopback-only local surface.** The server binds `127.0.0.1`; the panel and the enroll handler also
  check the peer address; the panel's credential is a per-boot 32-byte token in a meta tag, required
  in a header no cross-site page can set without a CORS preflight the device never grants.
- **CSRF / sessions.** No API mutation accepts the page cookie; the page's session is an `HttpOnly`,
  `SameSite=Lax` cookie scoped to `/next` (`Secure` over TLS), and every `/next` mutation carries the
  session's CSRF value, kept in the session rather than the cookie. Sessions last 30 d, are stored as a
  hash in `sessions.json`, and `POST /logout` removes the presented one. Access identity is verified per
  request, never stored.
- **Secrets in logs.** Audit/denial lines carry ids, codes and statuses, never payload bodies or
  plaintext credentials; tests grep for `srm_` and the token value.
- **Comparison discipline.** Token hashes, install tokens, panel tokens and sessions compare with
  `constant_time_eq` over bytes.
- **No payload bodies.** The audit cannot hold an MCP body by type; the denial log is rate-limited and
  separate so unauthenticated traffic cannot rotate the operator's history. The operator can read the
  denial log at the console's **Security** page (`GET /denials`, operator-only, newest first); the page
  states the 100/minute cap and the `suppressed N` summary.

## Residual risks (accepted or open)

1. **A compromised console can mint grants.** The design makes the console the authority; the only
   bound is that the operator's own service stays the operator's. A stolen console data dir is a full
   compromise of the fleet's authority.
2. **No online revocation for a grant.** A copied grant is valid until its expiry; token revocation is
   the console's record (the device trusts a token's minted grant for 60 s). Expiry is the bound.
3. **Bearer theft.** The operator token, a session, an API token and the install token are all bearer
   credentials; a copy is usable until expiry/revocation. Treat them like passwords; the docs say so.
4. **Clock skew.** The device accepts a bounded skew for grants; a device with a badly wrong clock can
   refuse valid grants or accept a slightly expired one within the bound.
5. **Local trust.** The panel has no login: any local user/process on the machine can read it (it never
   prints the install token) and can use the per-boot panel token to enroll. The install token is the
   owner credential; its file ACL is the boundary.
6. **Volume/DoS.** The console caps MCP bodies and queues; the denial log is rate-limited. A
   authenticated operator can still saturate a device's session or queue within those limits.
   Probing is observable on the Security page but not alerted in real time: a suppressed run shows
   as one `suppressed N` line on the next write, and nothing pushes a notification.
7. **Edge configuration.** Access bypasses (`/devices`, the channel) and `/mcp`'s own application are
   deployment decisions; a wrong bypass exposes the registration/channel paths to the internet (they
   still require a valid code/signature, but the surface is larger).
8. **Supply chain.** Dependencies are pinned in the lockfiles; no audit gate runs in CI (the mirror
   used here has no npm advisory endpoint and `cargo audit` is not installed in the workspace).

## Review triggers

Re-read this when: a new credential/entry point lands, the grant or token format changes, the console
deployment (edge/Access) changes, a revocation/backup path is touched, or a new device-local endpoint
is added. The tests that pin the claims live beside each module (`tokens.rs`, `codes.rs`, `devices.rs`,
`http.rs`, `access.rs`, `capabilities/terminal.rs`, the panel token tests).
