# The console's public name: `console.saisi.online`

> **Status: deployed, 2026-10-23.** The new console had no public entry point: it listened on
> `desktop-9vh92bj:19000` over the tailnet, so an operator away from the tailnet could not reach it and
> `ai.saisi.online` kept showing the old stack. This records how it was exposed, what was reused, and
> what was measured — including the part that is not good yet.

## What was reused, not built

The host already runs `cloudflared` (pm2 id 0) with tunnel `f27ca571-…`, serving `zss-dsh.saisi.online`
→ `http://127.0.0.1:18081`. The console did **not** need its own tunnel:

```
ingress:
  - hostname: zss-dsh.saisi.online      (unchanged)
  - hostname: console.saisi.online
    service: http://100.74.250.98:19000   (the console, over the tailnet)
  - service: http_status:404
```

plus a proxied CNAME `console` → `f27ca571-78b9-43e8-8957-9b7ccde11f8d.cfargotunnel.com` in the
`saisi.online` zone. TLS terminates at Cloudflare — the shape
`2026-10-22-the-console-transport-decision.md` chose, with no certificate to install and no port open.

The device has **no Cloudflare interface**, deliberately: it dials out to the console over one
WebSocket and the console proxies operator requests down it. A per-device tunnel and a per-device
public hostname (`*.agent.saisi.online`) is what this replaces.

## What was measured

`https://console.saisi.online/health` answers `{"status":"ok","devices":2}`, so the page, the API and the
two enrolled devices are reachable from the internet.

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
