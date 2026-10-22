# Remote access — two classes of client

> **Status: decision note, amending the two-runtime design's §5.2.** Written 2026-10-21, during phase 4,
> when the capability layer made the question unavoidable: who is allowed to reach a device, and what
> does the console see?

## The ways to reach a device

| Class | Who | Path | The console sees |
|---|---|---|---|
| **Local** | a process on the device | loopback + install token | nothing |
| **Mesh peer** | a client that holds an identity and speaks frames | **not wired**: `summrise-device` does not depend on `summrise-mesh`, so there is no mesh client in the shipped binary | nothing (nothing to see yet) |
| **Operator session** | a browser, a script, an LLM client that speaks HTTPS/MCP | the console proxies the request over a device-dialled channel | the request and the response |

The local class exists. The mesh class is the design's target, not a current path: the tree has the
mesh broker/relay library and binaries, but no device client. The operator session is therefore the
**only remote class**, and the console carries its payload. The §5.2 sentence "the console is a broker,
not a data path" describes where this is going; it does not describe the built system.

## The operator path, precisely

- **The device dials out** to the console (WSS) and keeps one channel. It authenticates with the token
  the console returned at registration; the console stores only that token's hash. No inbound port.
- **The console authenticates the operator** (the operator token exchanged at `POST /auth` for an
  in-memory session), then issues a **grant**: an ed25519 signature over
  `{device_id, tools, expires_at_ms, allow_destructive}`, signed by the console's grant key. The
  lifetime is chosen per grant and must be 60 s – 7 days; there is no default. The 24 hours is the
  **operator session**, a different thing.
- The grant binds **no subject and no grant id**. It is a bearer capability: anyone holding the signed
  bytes can present them for that device until they expire, and the device's audit records the tool
  and the decision, not which operator session used the grant.
- **The device verifies the grant offline** against a pinned console key; `authorize` treats a valid
  grant as `Subject::Key { id: "console", scopes: tools }`, so the four rules are unchanged and the
  console cannot widen them.
- **The console proxies the MCP exchange** over the device channel. It sees the plaintext, because the
  operator's TLS terminates at the console. That is the price of the browser class, and it is why the
  console is small, single-operator, and audited. While the mesh is unwired it is also the price of
  **every** remote call, not only the browser's.

## Consequences, stated rather than implied

1. **A compromised console can mint grants.** It holds the grant key; the device pins that key, so
   rotation is a console/device configuration act, not a message. The mitigation is that the console is
   the operator's own, and the audit index lives there.
2. **Only local access survives a console outage.** The console *is* the data path today, so an
   operator session stops when it stops. The design's "control does not need the console" becomes true
   when a mesh client is wired; it is not true now.
3. **Two signatures, two jobs — one of them missing.** The mesh was to pin a device identity (who the
   machine is); the console signs a grant (what a person may do). The device holds no identity key
   today, so only the grant half exists; when the mesh is wired, neither signature will substitute for
   the other.
4. **Files are different.** Large transfers keep the dedicated R2 relay (CHARTER); the operator channel
   carries control, not megabytes.

## What this built

The `console` crate (axum, Rust, deployed on the operator's VPS or on a loopback box) now has: operator
auth, a device registry, grant signing, the device channel and the audit index. Its tests run
in-process with a fake device, which is why it was buildable before the canary. It is not a Worker and
it has no device page.
