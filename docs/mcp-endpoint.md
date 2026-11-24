# `mcp.saisi.online`: a public MCP door, and nothing else

> **Status: live, 2026-11-04.** One hostname for MCP clients, reachable from anywhere, while the
> operator page stays on the tailnet at `console.saisi.online`.

A client configures it exactly as the console's Connect sheet prints:

```yaml
- id: mcp-summrise
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: summrise
    transport: streamable-http
    url: https://mcp.saisi.online/mcp/<device_id>   # or /mcp/all for the fleet
    headers:
      Authorization: Bearer <secret>
```

The token is a console-issued MCP token; moving the endpoint does not change it.

## What carries it

| piece | value |
|---|---|
| DNS | `CNAME mcp.saisi.online → f27ca571-….cfargotunnel.com`, **proxied** (Cloudflare terminates TLS) |
| tunnel ingress | `mcp.saisi.online → https://100.85.163.159` with `noTLSVerify: true` (self-signed origin) |
| origin | nginx `mcp-saisi.conf` on 100.85.163.159:443 → `http://100.74.250.98:19000` |
| exposed paths | `^~ /mcp` only; everything else is `404` |

A proxied A record to the tailnet address is refused by Cloudflare (`9003: Target 100.85.163.159 is
not allowed for a proxied record`), which is also what a proxied record to any private IP produces at
the edge — the 1002 the operator saw. The CNAME-to-tunnel form is the supported one.

## Why the origin is nginx and not the console

The console resolves an operator *before* any session:

```rust
// device/console/src/http.rs
fn session_operator_from(&self, headers, peer) {
    if let Some(operator) = self.proxy_operator(headers, peer) { return Some(operator); }  // first
    …
}
fn proxy_operator(&self, headers, peer) {
    if peer != Some(trusted) { return None; }               // --trusted-proxy 100.85.163.159
    let login = headers.get("x-forwarded-user")?;            // non-empty is enough
    …
}
```

The tunnel runs on 100.85.163.159, so a tunnel that pointed straight at the console would arrive from
the trusted address with the client's own headers intact: **any public request could name itself**.
Measured: a direct `curl -H 'X-Forwarded-User: attacker@example.com' http://100.74.250.98:19000/fleet`
returns `200` with the fleet. Through nginx the same header returns `401` because the block sets
`proxy_set_header X-Forwarded-User "";`, which makes `proxy_operator` return `None` and leaves the
console with token checks.

## The checks that define "working"

```
GET  https://mcp.saisi.online/               -> 404   (no operator UI on this name)
GET  https://mcp.saisi.online/fleet          -> 404
POST https://mcp.saisi.online/mcp/<id>       -> 401   {"error":"unauthorized"}
POST https://mcp.saisi.online/mcp/all        -> 401
GET  https://console.saisi.online/           -> 200   (unchanged)
```

`401` is the console's own JSON: it proves the path reaches the console and that a credential is
required.

## Renewal and residual risk

- The origin certificate is **self-signed** (`~/.config/dsh-tls-mcp`, expires 2029-02-06) and is only
  ever seen by cloudflared, which ignores it (`noTLSVerify`). The browser sees Cloudflare's.
- The console still trusts `X-Forwarded-User` from `100.85.163.159` **on any port**, so any process on
  that host can impersonate an operator against `http://100.74.250.98:19000` directly. The public door
  does not change that; closing it needs a shared-secret header or binding the console to the proxy.
