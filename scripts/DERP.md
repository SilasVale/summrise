# DERP.md — the self-hosted DERP relay, and why the tailnet has one

A runbook for the relay that sits on the same Oracle box as the tailnet's exit node. Everything below was
measured on 2026-10-12; the numbers are the baseline a future reader compares against, not decoration.

## WHY THIS EXISTS

The operator's Windows box (`desktop-cph4kb2`) reaches the internet **through the Tailscale exit node**
(`oracle-phx`, `100.79.21.110`) — that is a requirement, not a convenience. When the direct path holds, it is
fine: measured `tailscale ping oracle-phx` → `via 132.226.90.175:41641 in 197-198ms`, throughput 2.73 MB/s,
public IP `132.226.90.175`, Cloudflare sees `colo=LAX, loc=US`.

When the direct path **breaks** — domestic carriers throttle or kill long-lived UDP flows, so the hole-punch
dies — Tailscale falls back to a **DERP relay**, and the fallback was a *public* one. Measured during one such
episode:

```
pong from oracle-phx via DERP(sfo) in 201ms      <- the slow episode: every packet relayed via San Francisco
pong from oracle-phx via 132.226.90.175:41641    <- recovered: direct again
```

The latency is nearly the same (201 vs 199ms). What differs is that a relayed hop carries **the whole
internet** for an exit-node user, through a **shared public relay** designed for coordination, not egress.
That is the "suddenly slow" the operator reported, and this relay removes the public relay from the path:
both ends now home on a region that IS the exit node.

The rule that makes this worth doing: a node picks its **home DERP** by measured latency, and a region whose
latency cannot be measured is **never selected**. So a DERP that is unreachable for STUN might as well not
exist — which is why half of this runbook is about the STUN port.

## TOPOLOGY

```
desktop-cph4kb2 (China) ──direct UDP 41641──▶ oracle-phx (132.226.90.175) ──▶ internet
        │                                          ▲
        └──fallback (TCP 8443)──▶ derp.saisi.online ┘   region 900 "phx", on the exit node itself
```

## THE PIECES

| Piece | Where | What it does |
|---|---|---|
| `derper.service` | VPS, `/etc/systemd/system/` | the relay: TLS on `0.0.0.0:8443`, `-verify-clients` OFF (see traps) |
| `stun-responder.service` | VPS, `/etc/systemd/system/` | answers STUN on UDP 3478 — **derper's own STUN does not answer** (see traps) |
| `/usr/local/bin/stun-responder.py` | VPS | ~40 lines, RFC 5389 binding + XOR-MAPPED-ADDRESS; its docstring says why it exists |
| `/home/ubuntu/go/bin/derper` | VPS | built from source at the tailnet's own version |
| `/etc/derper-certs/derp.saisi.online.{crt,key}` | VPS | copies of the certbot files, `600 ubuntu`, because `-certmode manual` wants `<hostname>.crt/.key` |
| `/etc/letsencrypt/live/derp.saisi.online/` | VPS | certbot, **DNS-01** via `/etc/letsencrypt/cloudflare.ini` (`600 root`), auto-renewing |
| DNS `derp.saisi.online` → `132.226.90.175` | Cloudflare | **DNS-only** (grey cloud), TTL 60 |
| `derpMap` region `900/phx` | tailnet ACL policy | how clients learn the region exists |
| OCI ingress rule UDP 3478 | Oracle console | **the one step no command on the box can do** (see traps) |
| iptables 8443/tcp + 3478/udp | VPS, `/etc/iptables/rules.v4` | the instance's own firewall; persisted |

`nginx` is untouched: 443 belongs to `oracle.saisi.online` / `v.saisi.online`, and this relay took a port of
its own (8443) instead of fighting for it.

## REBUILD FROM SCRATCH

```bash
# 1. derper, at the SAME version the tailnet runs (mismatched versions are not a supported pairing)
sudo apt-get install -y golang-go
GOFLAGS=-mod=mod go install tailscale.com/cmd/derper@v1.102.4     # ~3 min on the free-tier ARM box
# NOTE: the official static tarball (pkgs.tailscale.com) ships tailscale + tailscaled ONLY — no derper.

# 2. the certificate, DNS-01 so nginx is never touched
sudo apt-get install -y python3-certbot-dns-cloudflare
printf 'dns_cloudflare_api_token = %s\n' "$(cat ~/.cloudflare-token)" \
  | sudo install -m 600 -o root -g root /dev/stdin /etc/letsencrypt/cloudflare.ini
sudo certbot certonly --dns-cloudflare --dns-cloudflare-credentials /etc/letsencrypt/cloudflare.ini \
  --dns-cloudflare-propagation-seconds 20 -d derp.saisi.online --non-interactive --agree-tos --keep-until-expiring

# 3. manual-mode cert layout
sudo mkdir -p /etc/derper-certs
sudo cp /etc/letsencrypt/live/derp.saisi.online/fullchain.pem /etc/derper-certs/derp.saisi.online.crt
sudo cp /etc/letsencrypt/live/derp.saisi.online/privkey.pem   /etc/derper-certs/derp.saisi.online.key
sudo chown -R ubuntu:ubuntu /etc/derper-certs && sudo chmod 600 /etc/derper-certs/*

# 4. the DNS record (Cloudflare API, DNS-only) — or the console
#    A derp.saisi.online -> 132.226.90.175, proxied=false

# 5. the two services. NOTE: `-c` is a FILE, not a directory: `-c /var/lib/derper` fails with
#    "read /var/lib/derper: is a directory".
sudo mkdir -p /var/lib/derper && sudo chown ubuntu:ubuntu /var/lib/derper
#    derper.service:
#      ExecStart=/home/ubuntu/go/bin/derper -c /var/lib/derper/derper.key -a 0.0.0.0:8443 \
#        -hostname derp.saisi.online -certmode manual -certdir /etc/derper-certs \
#        -http-port 8083 -stun=false
#    stun-responder.service:
#      ExecStart=/usr/bin/python3 /usr/local/bin/stun-responder.py 3478

# 6. the instance firewall, persisted (the nftables file is NOT the source of truth here:
#    `netfilter-persistent` + /etc/iptables/rules.v4 is, and nftables.service is disabled)
sudo iptables -I INPUT 1 -p tcp --dport 8443 -j ACCEPT
sudo iptables -I INPUT 1 -p udp --dport 3478 -j ACCEPT
sudo sh -c 'iptables-save > /etc/iptables/rules.v4'

# 7. the ONE console step: add an ingress rule to the VCN security list — UDP, source 0.0.0.0/0,
#    destination port 3478. Without it every remote node measures this region as blank and never
#    selects it. (TCP 8443 turned out to be already open; UDP is not.)

# 8. the dispatch, in the tailnet's ACL policy (admin console -> Access controls, or the API with
#    Content-Type: application/hujson and the policy itself as the body — NOT {"acl": ...}, which is
#    rejected with `json: unknown field "acl"`):
#      "derpMap": {
#        "OmitDefaultRegions": false,
#        "Regions": { "900": { "RegionID": 900, "RegionCode": "phx",
#          "RegionName": "Phoenix (self-hosted, saisi)",
#          "Nodes": [{ "Name": "900a", "RegionID": 900, "HostName": "derp.saisi.online",
#                      "DERPPort": 8443, "STUNPort": 3478 }] } }
#      }
```

## VERIFY

| Check | Command | Expected |
|---|---|---|
| relay protocol | `sudo tailscale debug derp phx` (on the VPS) | `Successfully established a DERP connection`, `returned IPv4 STUN response`, `Warnings: null` |
| reachable from outside | `curl -k https://derp.saisi.online:8443/derp/latency-check` | `200`; `/derp` → `426 Upgrade Required` |
| STUN reachable | send a STUN binding request to `132.226.90.175:3478` | a reply in ~200ms from outside, ~0.2ms from the VPS |
| latency measured | `tailscale netcheck` on a client | `phx: 203.2ms` on `desktop-cph4kb2` (fastest of 28 regions), `phx: 600µs` on the VPS |
| **home selected** | `tailscale status --json` → `.Self.Relay` | **`phx` on the VPS** — this is the whole point |
| the path itself | `tailscale ping oracle-phx` on the Windows box | `via 132.226.90.175:41641 in ~197-207ms` (direct) |
| cert | `openssl s_client -connect 132.226.90.175:8443 -servername derp.saisi.online` | `Verify return code: 0 (ok)` |

## TRAPS (each one cost time; do not re-learn them)

* **derper's own STUN does not answer on this box.** 1.102.4 logs `STUN server listening on [::]:3478` and
  binds the port, but neither a plain RFC 5389 probe nor Tailscale's own `debug derp` gets an IPv4 STUN
  response — locally, on 127.0.0.1, on any port. Hence `-stun=false` in the unit and the separate responder,
  which DOES answer and is what makes the region selectable.
* **Never put `derp.saisi.online` in the VPS's `/etc/hosts`.** It was tried, to make the box measure its own
  relay without a NAT hairpin. systemd-resolved serves `/etc/hosts` authoritatively, so the entry **leaked to
  the entire tailnet**: the Windows box resolved the host to `127.0.0.1`, measured a fake `600µs`, and would
  have dialled a dead address. With the OCI UDP rule in place the hairpin works on its own (measured `600µs`
  legitimately) and no override is needed.
* **`-verify-clients` is off.** With it on, the relay's own unauthenticated probes are refused. The trade is
  that anyone who learns the hostname can relay through it; the hostname is not published, and this was
  chosen deliberately over a region no client can select. Turn it back on only with a way to keep the latency
  measurable.
* **`-c` is a key FILE** (`/var/lib/derper/derper.key`); a directory fails with `is a directory`.
* **UDP 3478 needs the OCI console.** No command on the instance can open it — the VCN security list sits
  above the instance. UDP 41641 works because Tailscale's own rule is there; every other UDP port tested
  (443, 80, 53, 3478) was filtered from outside.
* `OmitDefaultRegions` is left **false** on purpose: the public regions stay as a last-resort fallback. With
  `true` and only `phx` declared, every relayed packet in the tailnet would be forced through Phoenix — a
  single point of failure, and a long way round for two China-side machines talking to each other.
* The IPv6 lookup for `derp.saisi.online` fails harmlessly on the VPS (`no such host`): there is no AAAA
  record, and `debug derp` still succeeds over IPv4.

* **NEVER point the tailnet's own DNS at a resolver that lives on the far side of the tunnel the clients
  need that DNS to build.** `nameservers: [100.79.21.110]` + `overrideLocalDNS: true` was tried, reasoning
  that the VPS already carries the traffic so resolving there would match the exit node and stop lookups
  leaking to a China-side ISP. It **stranded `desktop-cph4kb2` off the tailnet within minutes**: tailscaled
  resolves the control plane and the DERP hosts through the *system* resolver, that resolver was now the
  VPS, and the tunnel to the VPS could not be established without resolving them first. The tailnet settings
  were reverted at once (`nameservers: []`, `overrideLocalDNS: false`, `magicDNS: true`), but a client keeps
  its last netmap until it fetches a new one — so the stranded box needed a **local** action (`Restart-Service
  Tailscale` in an elevated PowerShell, or `tailscale set --accept-dns=false`) to break the deadlock. If
  US-side resolution is wanted, the resolver has to sit where the client can always reach it.
* **`POST /dns/preferences` takes the WHOLE object, not a patch.** Posting `{"overrideLocalDNS": true}`
  alone silently set `magicDNS: false` and turned MagicDNS off tailnet-wide (caught in the read-back, fixed
  with `{"magicDNS": true, "overrideLocalDNS": false}`). Send every field you care about, then read it back.

* **After any `tailscale up` (or a re-login) on a client, RE-ASSERT THE EXIT NODE.** `tailscale up` without
  the flags clears `--exit-node`, and the symptom is silent enough to be mistaken for this relay's fault: the
  machine stays on the tailnet, `tailscale ping` answers, the path is direct — and its EGRESS IP is suddenly
  the local one (`loc=CN` in a Cloudflare trace instead of `loc=US`), so every foreign site is slow. Measure
  the egress, not the tailnet state:

      curl -sS https://www.cloudflare.com/cdn-cgi/trace | findstr /I /C:"ip=" /C:"loc="
      tailscale set --exit-node=oracle-phx --exit-node-allow-lan-access=true

  Seen 2026-10-13 right after the DNS recovery above: the tailnet reported `active; direct`, while the trace
  read `ip=101.68.143.52 loc=CN`. Both checks matter and neither replaces the other.

## OPERATE / ROLLBACK

```bash
systemctl status derper stun-responder            # both should be `active`
sudo journalctl -u derper -n 20                   # "serving on 0.0.0.0:8443 with TLS"
sudo tailscale debug derp phx                     # the one-line health check
```

Rollback, cheapest first:

1. **the dispatch**: remove the `derpMap` block from the ACL policy → clients fall back to the public regions.
   The policy before this work is `~/tailscale-acl-backup-20261012-1615.hujson` on `zss`.
2. **the services**: `sudo systemctl disable --now derper stun-responder`.
3. **the DNS record**: delete `derp.saisi.online` in Cloudflare.
4. **the firewall**: `sudo iptables -D INPUT -p tcp --dport 8443 -j ACCEPT` and the UDP twin, then re-run
   `iptables-save`. The OCI ingress rule can stay or go; it is inert without a listener.

## CREDENTIALS AND OWNERSHIP

* `/etc/letsencrypt/cloudflare.ini` holds a Cloudflare API token (`600 root`) used only for renewals.
* The tailnet ACL was edited through the Tailscale API with a key that appeared in a chat transcript on
  2026-10-12 — **rotate it**. The key is cached at `~/.tailscale-api-token` (`600`) on `zss`.
* `~/.ssh/oracle-vps.key` on `zss` reaches the VPS as `ubuntu@100.79.21.110` (sudo is passwordless).
