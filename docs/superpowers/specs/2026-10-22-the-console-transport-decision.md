# The console's transport — TLS is the terminator's, not the console's

> **Status: decision, 2026-10-22.** Written after the canary proved that the console works and that
> the one thing standing between it and a real deployment is a certificate. It records why the
> console still serves plain HTTP, what an operator must run in front of it, and what would change
> the call.

## The facts the decision is made on

- The console binds **loopback by default**; `--bind 0.0.0.0` is an explicit act, and the design has
  always said the network deployment sits behind a TLS terminator.
- Devices reach the console **outbound**: the device dials `ws://<console>/devices/<id>/channel`.
  There is no inbound listener on the device, so the console is the only side that needs a public
  name or a private network.
- The canary today runs the console on a tailnet host over **plain HTTP inside the tunnel**, and the
  device's enrolment and update paths accept that only with `--allow-plaintext`, a flag whose whole
  meaning is "the transport underneath is already encrypted" (Tailscale/WireGuard/SSH forward).
- The device's HTTP client trusts the **platform's root store**. A publicly-trusted certificate
  therefore needs no device configuration; a self-signed one would need a root installed on every
  device, which is a worse problem than the one it solves.

## The options

| # | Shape | What the operator owns | What the console does | Verdict |
|---|---|---|---|---|
| A | Terminator in front (Caddy, nginx, Cloudflare) | a DNS name and a certificate | plain HTTP on `127.0.0.1:<port>` | **chosen for production** |
| B | The console terminates TLS itself (`rustls`, `--tls-cert/--tls-key`) | a certificate in the console's data directory | serves TLS | not now: a second TLS configuration to get right, and it does not remove the need for the certificate |
| C | Tailnet-only, plaintext | a tailnet | plain HTTP inside the tunnel | the canary's shape, and only that |

**A is chosen.** The console is a small Rust service whose job is auth, registry, grants and one
proxied frame; a TLS configuration is a different job with its own failure modes (renewal, SNI, chain
order), and every deployment of this project already has an HTTP server or a proxy in front of
something. Option B would make the console responsible for a certificate it cannot renew and would
still need the operator to obtain one; it removes a moving part only in the deployment where the
operator has no proxy, which is not the deployment being designed for.

## What the operator runs

A Caddy in front of a loopback-bound console is four lines and renews by itself:

```caddyfile
console.example.com {
    reverse_proxy 127.0.0.1:18082
}
```

The device then enrols with an HTTPS URL, and **no flag** is needed on either side:

```bash
summrise-device --enroll https://console.example.com <code> --data C:\ProgramData\Summrise
```

The WebSocket upgrade travels over the same `reverse_proxy` (Caddy passes it through), which is the
one thing a terminator must not break: the console's channel is a WebSocket on
`/devices/<id>/channel`.

## Consequences

- `--allow-plaintext` is a **canary/tunnel** flag and stays one. It must not appear in a production
  runbook; the section that shows it says so.
- The console keeps `--bind 127.0.0.1` as its default. A production deployment binds loopback and
  lets the terminator hold the port, so a compromised console has no public listener of its own.
- A certificate is an operator asset: DNS name, issuance, renewal. Nothing in this repository can
  test that, and nothing in it pretends to.
- The device side needs no change: `reqwest` already trusts the platform roots, so a public
  certificate is invisible to it, and the enrolment rule already requires `https` for a non-loopback
  host.

## What would change the decision

- A deployment with **no** HTTP proxy and no way to install one, where a self-signed certificate and
  a device-side root are acceptable: then option B (console-terminated TLS) plus a device trust
  anchor is the smaller total.
- A console that must also serve other services on the same host: the terminator is then mandatory
  anyway, and A is the only shape.
