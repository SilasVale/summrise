# The console's public name: `console.saisi.online`

> **Status: deployed, 2026-10-23.** The new console had no public entry point: it listened on
> `desktop-9vh92bj:19000` over the tailnet, so an operator away from the tailnet could not reach it and
> `ai.saisi.online` kept showing the old stack. This records how it was exposed, what was reused, and
> what was measured — including the part that is not good yet.

## What was reused, not built

The host already runs `cloudflared` (pm2 id 0) with tunnel `<tunnel-id>`, serving `zss-dsh.saisi.online`
→ `http://127.0.0.1:18081`. The console did **not** need its own tunnel:

```
ingress:
  - hostname: zss-dsh.saisi.online      (unchanged)
  - hostname: console.saisi.online
    service: http://<canary>:19000   (the console, over the tailnet)
  - service: http_status:404
```

plus a proxied CNAME `console` → `<tunnel-id>.cfargotunnel.com` in the
`saisi.online` zone. TLS terminates at Cloudflare — the shape
`2026-10-22-the-console-transport-decision.md` chose, with no certificate to install and no port open.

The device has **no Cloudflare interface**, deliberately: it dials out to the console over one
WebSocket and the console proxies operator requests down it. A per-device tunnel and a per-device
public hostname (`*.agent.saisi.online`) is what this replaces.

## What was measured

`https://console.saisi.online/health` answers `{"status":"ok","devices":2}`, so the page, the API and the
two enrolled devices are reachable from the internet.

The whole flow was then driven **through the public name** from the console's own machine, so the
operator token never left it:

```
auth:  1689 ms
grant: 2450 ms
mcp:   3543 ms   → {"health":{"pid":22364,"poisoned":false,"status":"ok","version":"0.3.10"}, …}
```

Login, grant, and a device call answering with that device's state — Cloudflare edge, tunnel, console,
device channel, and back. Under a second per hop, which is the number that matters; the 10-second tails
measured from a different host on the same uplink did not appear on this path.

Authentication was checked from outside as well: `POST /grants` without an operator token answers
`401 {"error":"unauthorized"}`.

The path is not fast or steady yet: repeated requests measured **1.6 s to 10.5 s**, with occasional
15-second timeouts, while the tailnet hop itself is **1.5 ms**. The tunnel shares this host's uplink and
the same latency shows on `zss-dsh.saisi.online` (a Cloudflare Access 302 that took 3.8 s and sometimes
timed out), so the tunnel is not the suspect — the uplink is. Switching the tunnel to `protocol: http2`
made it worse (3 of 5 requests timed out) and was reverted.

## What this means, and what would fix it

The console is reachable and usable, and its latency currently tracks the host's uplink rather than
anything in the console. For an operator-facing control plane that is acceptable as a pilot and not
acceptable as a product:

- **A second replica on a stable host** (the console is stateless apart from its data directory; the
  devices dial one URL, so the replica needs that URL as a DNS alias with the data directory on
  persistent storage), or
- **cloudflared on the console's own machine**, which removes this host's uplink from the path but
  inherits whatever that machine's uplink is.

Neither was done here: the first is a hosting decision, the second moves a resident service onto the
operator's machine and was left for their word.

## What is still open on this name

- **Cloudflare Access** protects `zss-dsh.saisi.online` and `ai.saisi.online`; `console.saisi.online` is
  **not** behind an Access application. The console authenticates with its operator token, so the page
  is not open, but adding an Access policy for the name is a two-minute change in the dashboard and is
  recommended before the name is shared.
- **The mirror's URL and the console URL are both public now**; neither carries a secret, and both are
  plain HTTPS with a certificate the operator does not manage.

## 2026-10-23 — Cloudflare Access in front of the page, and why the device channel needs no bypass

The console had only its operator token. Access was added the same way the operator's own harness is
protected (the `allow-me` policy that already guards `zss-dsh.saisi.online`):

```
app     Summrise Console            console.saisi.online       session 24h
policy  allow-me                    decision allow             email 13122231591@163.com
```

`GET https://console.saisi.online/` now answers `302` to `vale-saisi.cloudflareaccess.com`; the page is
reachable only after the operator's email is verified, and the console's token remains the second
factor.

**The device paths had to be reachable, and the first attempt at that was wrong.** A bypass app for
`console.saisi.online/devices` was created, then deleted after a test that only killed a device and
watched it reconnect: the **channel** upgrade is not challenged by Access, so the bypass looked inert.

It was not inert — it was **mislabelled**. A Cloudflare Access application with no policy does not
bypass; it treats the request as unauthenticated. The deleted app had no policy at all, so it never
bypassed anything and the test proved only that the channel survives without it. Device **registration**
(`POST /devices`) was 302'd by Access, which means a new machine could not enrol through the public
name at all.

The app was re-created with a real policy (`decision: bypass`, everyone) and verified from outside:

```
POST /devices            → 422  (the console's own validation error: the path is reached)
GET  /                   → 302  (the operator page is still behind Access)
```

So the split is deliberate: the two device-facing paths are reachable, everything an operator reads is
not. The registration still costs a one-time code, and the channel still costs the token the device
registered with — the bypass removes a network gate, not an authorization one.

## 2026-10-23 — Access becomes the operator identity, and what that cost

With Access protecting the hostname, the console still asked for a 64-hex token kept in a file on the
host. The operator asked the right question — *why type a secret when Access already knows who I am?* —
and the answer was that the console verified nothing but its own token.

It now verifies Access's answer. Three flags turn it on, all or none:

```
--access-jwks   <file>   the team's certificates, pinned in a file
--access-aud    <tag>    this application's audience
--access-issuer <url>    https://vale-saisi.cloudflareaccess.com
```

A verified `Cf-Access-Jwt-Assertion` is an operator session; `GET /session` tells the page, which then
skips the login box. The token and `--login-link` stay as break-glass paths, so a broken Access
configuration cannot lock the operator out of the console — only out of the pretty way in.

**Why not a password store of our own.** The operator invoked Tailscale's login as the model. Tailscale
does not store passwords either: it delegates to Google/GitHub/Microsoft. A password store means
hashing, reset, lockout, rate limiting, MFA and session revocation — several thousand lines of
security-critical code serving one person, with every line ours to get wrong. Access is the same
delegation with almost none of the code.

**What was tested, and what was not.** Against a throwaway RSA key: a correct token accepted; another
audience, another issuer, an expired token, an unknown `kid`, an unusable JWKS — all refused. On the
deployed console a forged header answered `{"authenticated":false}` and `401` on a data route, which is
the property that matters: the header is what an attacker controls.

The positive path needed a real Access session, which only the operator's browser can produce. **The
operator opened `https://console.saisi.online/` on 2026-10-23 and the page came up signed in** — it
shows `Signed in as 13122231591@163.com through Cloudflare Access`, the device table with both canaries
(`online`, `v0.3.13`, last seen), and the Add/Rename/Remove/Grant sections with no token box anywhere.
That is the positive evidence this section was waiting for, and it came from a browser rather than from
a test.

Two consequences worth knowing: `Sign out` clears the tab's session but cannot end the **Access**
session (only Cloudflare can, at `…/cdn-cgi/access/logout`), and a browser whose Access session expired
falls back to the token box rather than to an error.
