# Console mesh, slice 2: operator traffic as a second audited path

**Status: PROPOSAL — a data-path change; it needs the operator's go-ahead before code.**
Date: 2026-10-26.

## What is already shipped (slice 1)

The console can *see* the mesh, and nothing more:

- the mesh crate looks a peer up without an identity (`lookup_candidates`) and validates a broker URL
  (`validate_broker_url`);
- the console takes `--mesh-broker`, reports `GET /mesh`, and asks `GET /fleet/:id/mesh` whether a device
  is registered right now (3 s bound, typed 408/502/404 answers); the device card shows the result and
  triggers the check on demand;
- the mesh wire is version 3: the signed hello covers a per-connection TLS exporter binding, so a hello
  recorded on one connection cannot be replayed on another.

No operator frame crosses the mesh. The console WebSocket is still the only remote data path.

## The question slice 2 must answer

May an operator's MCP call take the mesh instead of the console's WebSocket channel — **per request**, with
the same grant, the same device-side verification, and an audit that says which path served it?

## Design (recommended)

1. **The console gets its own mesh identity.** `--mesh-identity <file>` holds a 32-byte Ed25519 seed as hex,
   mode 0600, minted on first use like the grant key. Its peer id is the public key hex. The identity is a
   *client* identity: the console joins no service, serves nothing, and uses it only to dial a device. The
   file is one more root credential and is documented beside `token`; it can be rotated by replacing it
   (the broker learns the new id, the old registration lapses).
2. **Routing is explicit, never ambient.** `POST /mcp` accepts an optional `"path": "mesh"`; the default is
   `"channel"`, exactly today's behaviour. A per-device preference can come later; the first slice must be
   testable one request at a time.
3. **The request is the same envelope.** With `"path": "mesh"` the console joins the mesh lazily and opens a
   session to the device's peer id — the device id **is** the peer id — and sends
   `{"id","mcp":{...},"grant":{...}}`, the envelope the device's mesh channel already parses. The device's
   `remote::admit` verifies the grant offline, exactly as it does for the console channel; the grant stays
   the only authorization. No new trust is introduced by the mesh identity: the hello binding authenticates
   the transport, and the grant authorizes the call.
4. **Fallback is explicit and only for idempotent calls.** If the mesh dial fails (device absent, timeout,
   wire mismatch), the console falls back to the channel **only when the request is marked idempotent**
   (the caller says so, or the method is a read). A non-idempotent call returns the mesh error instead: a
   silent retry of a write is how an operator gets two side effects and one audit line. On fallback the
   audit records the reason.
5. **The audit names the path.** Every `/mcp` line gains `"path": "channel" | "mesh"` and, after a fallback,
   `"fallback_reason"`. The device's own audit already records the call; the console's line is the one the
   operator reads when asking "where did this go?".
6. **Readiness gates the route.** If `/mesh` says unconfigured, or `/fleet/:id/mesh` says the broker does
   not know the device, `"path": "mesh"` answers 409 with the readiness answer before any device contact.

## What must not change

- the grant format, its single-device binding and its expiry (`grants::verify`, `remote::admit`);
- the channel protocol and the device's `Caller::Granted` path;
- the device's audit for a mesh request (it is the same envelope);
- no console mesh *server*: a device does not dial the console in this slice.

## Staged path (each with its tests)

1. **Identity and custody.** `--mesh-identity`, mint on first use, 0600, `/mesh` reports the peer id; tests
   for the mode, the id and a restart that reuses the file.
2. **One request over the mesh, in process.** A device serving a mesh channel with a signed grant answers a
   read; the console returns the reply and the audit line says `path: mesh`.
3. **Fallback and refusal.** A dead mesh plus an idempotent read falls back with `fallback_reason`; a
   non-idempotent call returns the mesh error and the channel sees nothing.
4. **The operator view.** The workbench shows the path per call (the readiness chip already exists).

## Risks and questions for the operator

- **Availability.** The console dials out, so the console may sit behind NAT; the device must be reachable
  (punch or relay). The mesh's relay fallback is the same floor the device uses today.
- **One identity for every device.** The grant scopes each call to one device, so one console identity is
  enough; rotation is operational, not per device. Is that acceptable?
- **Enabled by default?** Recommended: no. `path: "mesh"` stays opt-in per request until a canary shows the
  mesh is at least as reliable as the channel.
- **Wire v3.** The binding merged; a v2 peer cannot talk to a v3 one. The first release that carries this
  main must upgrade the device and the console together (they ship from one tree).

## What this refuses

- ambient routing (the path is always explicit and audited);
- a silent fallback for writes;
- any change to the grant;
- a console that devices can dial.
