# Remote MCP access to a loopback-only agent

Findings from primary sources (specs, official docs, vendor source repositories). Every claim below
cites the document that owns it. Written 2026-09-22 against MCP protocol revision **2026-07-28**, the
revision current at that date ([spec index](https://modelcontextprotocol.io/specification)).

The case: a Windows agent serves `127.0.0.1:18080` — a panel at `/panel/` and a Streamable HTTP MCP
endpoint at `/mcp` behind a bearer token — and the machine is usually behind NAT. The owner wants a
WAN-side machine to drive it over MCP, preferably without Cloudflare and without installing a VPN
client on every client machine.

Loopback-only is not an accident of Vale's design; it is what MCP asks for. The Streamable HTTP
binding says servers **MUST** validate the `Origin` header, **SHOULD** bind only to localhost
(127.0.0.1) rather than 0.0.0.0 when running locally, and **SHOULD** implement proper authentication
for all connections, because otherwise attackers can use DNS rebinding to reach local MCP servers
from remote websites ([Streamable HTTP § Security & Endpoint](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)).

---

## Summary — the mainstream options, ranked by how much is left to the operator

1. **Cloudflare Tunnel (named/remotely-managed) + Access** — `cloudflared` makes outbound-only
   connections to Cloudflare's edge, so nothing listens publicly on the LAN
   ([Tunnel overview](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/)),
   and Access puts an identity-aware policy in front of the app with deny-by-default
   ([Access self-hosted app](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/)).
   Cloudflare even documents fronting *MCP* specifically, including OAuth for non-browser clients and
   service tokens for machine-to-machine ([MCP server portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)).
   Least operator work; a vendor is in the path (excluded by the brief).
2. **Tailscale** — a peer-to-peer WireGuard mesh across NAT with no port forwarding
   ([What is a tailnet](https://tailscale.com/kb/1151/what-is-a-tailnet)); `tailscale serve` publishes
   a local service to the tailnet only, `tailscale funnel` publishes it to the public internet for
   people who do not use Tailscale at all, on ports 443/8443/10000
   ([Funnel](https://tailscale.com/kb/1223/funnel), [Serve](https://tailscale.com/kb/1242/tailscale-serve)).
   Tailnet = every WAN machine installs a client; Funnel = no client install, but public.
3. **Managed agent-dials-out products** — VS Code Remote Tunnels
   ([docs](https://code.visualstudio.com/docs/remote/tunnels)), Portainer Edge Agent
   ([docs](https://docs.portainer.io/advanced/edge-agent)), Home Assistant Cloud
   ([Snitun protocol](https://github.com/NabuCasa/snitun)), GitLab Runner
   ([security docs](https://docs.gitlab.com/runner/security/)) and GitHub Actions self-hosted runners
   ([reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)) all invert
   the direction: the far side dials **out** and the server never dials in. This is the industrial
   norm for NAT'd estates — but each of these products owns both ends.
4. **A relay you host yourself** — `frp` (`frpc` on the NAT side dials a public `frps`;
   [README](https://github.com/fatedier/frp), [token/OIDC auth](https://gofrp.org/en/docs/features/common/authentication/))
   or an OpenSSH remote forward (`ssh -R`; [ssh(1)](https://man.openbsd.org/ssh.1)) on a small VPS.
   No vendor, no client software on the WAN side beyond an SSH/MCP client, but the operator now runs
   and patches the relay.
5. **A public HTTPS reverse proxy in front of the service** — Caddy
   ([automatic HTTPS](https://caddyserver.com/docs/automatic-https)), nginx
   ([TLS config](https://nginx.org/en/docs/http/configuring_https_servers.html)) or Traefik
   ([ACME resolver](https://doc.traefik.io/traefik/reference/install-configuration/tls/certificate-resolvers/acme/))
   with automatic certificates. This is the dominant answer to "give me HTTPS + auth in front of a
   loopback service" — but it presupposes a public name and reachable 80/443, which a NAT'd box does
   not have by itself.
6. **Quick tunnels for evaluation only** — Cloudflare quick tunnels are "intended for testing and
   development only" and hand out a random public subdomain
   ([Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/));
   localtunnel says it is for "easy testing and sharing" ([localtunnel](https://github.com/localtunnel/localtunnel)).
   Fastest to try, not a production posture.
7. **What the sources jointly support**: put a relay in the middle that the agent dials out to, let the
   relay terminate TLS and authenticate the *human/client*, and do not expose the bearer token as the
   only gate. MCP's own authorization chapter makes the MCP server an OAuth 2.1 **resource server**
   ([Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)), and
   RFC 6750 requires TLS whenever a bearer token is sent
   ([RFC 6750 §5.3](https://www.rfc-editor.org/rfc/rfc6750.html#section-5.3)).

---

## 1. MCP itself

### 1.1 Transports

The protocol defines two standard transports and permits custom ones
([Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)):

- **stdio** — newline-delimited JSON-RPC over the standard streams of a client-launched subprocess;
  the client launches the server as a subprocess, the server must not write non-MCP data to stdout
  ([stdio binding](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)).
  Local by construction.
- **Streamable HTTP** — "each message is an HTTP POST to a single MCP endpoint; replies arrive as a
  JSON object or a request-scoped SSE stream"
  ([Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)). The
  server **MUST** provide one endpoint path that supports POST, e.g. `https://example.com/mcp`; clients
  must send `Accept: application/json, text/event-stream`; the server answers with either a JSON object
  or an SSE stream scoped to that request
  ([Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)).

**What happened to HTTP+SSE.** The 2024-11-05 revision defined a two-endpoint HTTP+SSE transport
([2024-11-05 Transports § HTTP with SSE](https://modelcontextprotocol.io/specification/2024-11-05/basic/transports#http-with-sse)).
The 2025-03-26 revision replaced it with Streamable HTTP, and the 2025-06-18 text states plainly that
Streamable HTTP "replaces the HTTP+SSE transport from protocol version 2024-11-05"
([2025-06-18 Transports](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)).
In 2026-07-28 HTTP+SSE is formally reclassified as Deprecated under the feature-lifecycle policy, with
removal scheduled three months after SEP-2596 reaches Final
([Deprecated features](https://modelcontextprotocol.io/specification/2026-07-28/deprecated),
[Key changes § Deprecated](https://modelcontextprotocol.io/specification/2026-07-28/changelog)).

The 2026-07-28 revision also **removed protocol-level sessions and the `Mcp-Session-Id` header**,
removed the HTTP GET endpoint (long-lived notifications now arrive on a `subscriptions/listen`
response stream), removed SSE resumability/`Last-Event-ID` redelivery, and made the protocol stateless
— every request carries its protocol version and client capabilities in `_meta`, and `initialize` is
gone ([Key changes](https://modelcontextprotocol.io/specification/2026-07-28/changelog)). Practical
consequence for any deployment: "sticky session" assumptions at a load balancer or tunnel edge no
longer have a protocol-level session to stick to, and a broken response stream loses the in-flight
request, which the client must re-issue ([Key changes](https://modelcontextprotocol.io/specification/2026-07-28/changelog)).

### 1.2 What the spec says about remote servers

The architecture chapter is explicit that a server "can be local processes or remote services", while
the host enforces security boundaries and owns the client instances
([Architecture](https://modelcontextprotocol.io/specification/2026-07-28/architecture)). The
remote-server story is therefore about the **transport URL** and **authorization**, not about any
particular network topology: the spec never mentions NAT traversal, relays, tunnels or reverse proxies.
The official client-side guide frames a remote server simply as "hosted on the internet rather than
your local machine", reachable "from any MCP client with an internet connection"
([Connect to remote MCP servers](https://modelcontextprotocol.io/docs/develop/connect-remote-servers)).

### 1.3 The official authorization story

A protected MCP server **acts as an OAuth 2.1 resource server**, the MCP client acts as an OAuth 2.1
client, and the authorization server is a separate role that may be hosted with the resource server or
elsewhere; the spec's normative base is the OAuth 2.1 draft, `draft-ietf-oauth-v2-1-13`
([Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)).
The requirements that matter for a remote deployment:

- Authorization servers **MUST** implement OAuth 2.1 with appropriate security measures
  ([Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)).
- MCP servers **MUST** implement OAuth 2.0 Protected Resource Metadata, RFC 9728, and clients
  **MUST** use it to discover the authorization server; authorization servers must expose either
  RFC 8414 metadata or OpenID Connect Discovery, and clients must support both
  ([Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization),
  [Authorization Server Discovery](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery)).
- Clients **MUST** implement Resource Indicators (RFC 8707) and include `resource` in both the
  authorization and token requests, using the MCP server's canonical URI
  ([Authorization § Resource Parameter](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)).
- Servers **MUST** validate access tokens per OAuth 2.1 §5.2 and **MUST** verify the token was issued
  for them as the intended audience; invalid or expired tokens get HTTP 401. Servers "**MUST** only
  accept tokens that are valid for use with their own resources" and "**MUST NOT** accept or transit
  any other tokens"
  ([Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization),
  [Authorization Security Considerations](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations)).
- Communication security: all authorization-server endpoints **MUST** be HTTPS, and redirect URIs
  **MUST** be either `localhost` or HTTPS; clients **MUST** implement PKCE with `S256`
  ([Authorization Security Considerations](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations)).
- Client registration is shifting: Dynamic Client Registration (RFC 7591) is now **deprecated** in
  favour of Client ID Metadata Documents
  ([Key changes § Deprecated](https://modelcontextprotocol.io/specification/2026-07-28/changelog)).

### 1.4 Security requirements that bear directly on exposure

From the spec's Security Best Practices chapter
([Security Best Practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices)):

- **Token passthrough** is forbidden: accepting a token that was not issued for the server bypasses
  its own controls (rate limiting, request validation) and breaks auditability.
- **SSRF** guidance tells MCP clients to reject `http://` URLs except loopback and to block private
  and reserved IP ranges in production, aligned with OAuth 2.1 §1.5 and RFC 9728 §7.7 — i.e. the
  ecosystem's default is *not* to let clients reach arbitrary private addresses.
- **State handle hijacking** replaces the old session-hijacking section now that 2026-07-28 removed
  protocol sessions; state must be bound to the authenticated principal rather than to a guessable
  handle.
- For servers deliberately run **locally**, the spec's own mitigation list is: use `stdio` to limit
  access to just the MCP client; or, if using an HTTP transport, restrict access, "such as require an
  authorization token, use unix domain sockets or other IPC mechanisms with restricted access"
  ([Local MCP Server Compromise](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices)).

---

## 2. How mainstream MCP clients connect remotely today

| Client | Local server | Remote server | Auth model for remote |
|---|---|---|---|
| Claude Code | `claude mcp add --transport stdio …` ([docs](https://docs.claude.com/en/docs/claude-code/mcp)) | `claude mcp add --transport http <name> <url>`; HTTP is "the recommended option for connecting to remote MCP servers" ([docs](https://docs.claude.com/en/docs/claude-code/mcp)) | OAuth (PRM at `/.well-known/oauth-protected-resource`, falling back to RFC 8414), CIMD or DCR, pre-registered client credentials, fixed callback port; or a static header, e.g. `--header "Authorization: Bearer your-token"` ([docs](https://docs.claude.com/en/docs/claude-code/mcp)) |
| Claude Desktop / claude.ai / Cowork / mobile | stdio config in `claude_desktop_config.json` ([Connect to local MCP servers](https://modelcontextprotocol.io/docs/develop/connect-local-servers)) | Custom connector = remote MCP **server URL** entered in Claude settings ([docs](https://support.claude.com/en/articles/11175166-getting-started-with-custom-connectors-using-remote-mcp), [MCP guide](https://modelcontextprotocol.io/docs/develop/connect-remote-servers)) | Optional OAuth Client ID/Secret in "Advanced settings"; otherwise the server's own flow ([docs](https://support.claude.com/en/articles/11175166-getting-started-with-custom-connectors-using-remote-mcp)) |
| Cursor | `command`/`args` in `mcp.json` ([docs](https://cursor.com/docs/context/mcp)) | `"url": "https://…/mcp"` with optional `headers`; remote HTTP/SSE servers are "approved by URL entry pattern" by admins ([docs](https://cursor.com/docs/context/mcp)) | `"headers": { "Authorization": "Bearer ${env:MY_SERVICE_TOKEN}" }`, or an `auth` object with static OAuth client credentials and scopes ([docs](https://cursor.com/docs/context/mcp)) |
| VS Code | `"command": "npx"` entries in `mcp.json` ([docs](https://code.visualstudio.com/docs/copilot/chat/mcp-servers)) | `"type": "http", "url": "https://api.githubcopilot.com/mcp"` in the same file ([docs](https://code.visualstudio.com/docs/copilot/chat/mcp-servers)) | Server-side; VS Code separately requires an explicit trust confirmation before a server is started, resettable with `MCP: Reset Trust` ([docs](https://code.visualstudio.com/docs/copilot/chat/mcp-servers)) |
| OpenAI (Responses API / Agents SDK) | `MCPServerStreamableHttp` against `http://localhost:8000/mcp` with an `Authorization` header ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/)) | **Hosted MCP** — "Let OpenAI's Responses API call a publicly reachable MCP server on the model's behalf"; the round trip happens inside OpenAI's infrastructure ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/)) | Hosted tools take `server_url` plus an `authorization` value or `connector_id`; the SDK's own security note says to keep access tokens in authorization fields or headers rather than URLs and to require approval for sensitive operations ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/)) |

Two facts in this table decide the Vale case:

1. **A client that runs on the operator's own machine can reach loopback.** Claude Code, Cursor and
   VS Code accept an arbitrary URL and can therefore point at `http://127.0.0.1:18080/mcp` on a
   machine that is itself running the MCP client — no exposure at all.
2. **A cloud-side connector cannot.** Claude's custom connectors "connect to your remote MCP server
   from Anthropic's cloud infrastructure, rather than from your local device", across every Claude
   client ([Claude connectors](https://support.claude.com/en/articles/11175166-getting-started-with-custom-connectors-using-remote-mcp)),
   and OpenAI's hosted MCP requires a **publicly reachable** server
   ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/)). For those clients a
   loopback surface is unreachable no matter what, and a bearer token in a header is the *client's*
   configuration, not a substitute for reachability.

Also note the ecosystem has begun to treat "local MCP over HTTP" as a distinct risk: MCP's guidance
for servers that intend to run locally is stdio or a restricted HTTP transport with a token or
IPC ([Security Best Practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices)).

---

## 3. Exposing a NAT'd service — the mainstream patterns

### 3.1 Outbound-agent relays / reverse tunnels

#### Cloudflare Tunnel

*What it is.* A daemon (`cloudflared`) in your infrastructure creates outbound-only connections to
Cloudflare's global network; no traffic is sent to an external IP and the origin needs no publicly
routable address, which also means attacks that bypass Cloudflare cannot reach the origin
([Tunnel overview](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/)).

*Mechanism.* Origin dials out; Cloudflare's edge dials nothing into the LAN. Requests arrive at the
edge and are proxied down the tunnel.

*Quick vs named.* Quick tunnels (TryCloudflare) launch a process that generates a **random**
`trycloudflare.com` subdomain and proxy it to localhost, and are "intended for testing and development
only"; production use should create a remotely-managed tunnel
([Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/)).
Named/remotely-managed tunnels are created from the Zero Trust dashboard and run by `cloudflared`
([Create a tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/)).

*Auth model.* Two layers. The tunnel itself is authenticated by the connector's credentials; access to
the published application is governed by Cloudflare Access, whose applications are **deny by default**
— a user must match an Allow policy, with identity providers selected per application
([Publish a self-hosted application](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/)).
Access policies have actions Allow / Block / Bypass / **Service Auth**, and combine Include / Exclude /
Require rules ([Access policies](https://developers.cloudflare.com/cloudflare-one/access-controls/policies/)).
Service tokens authenticate machine-to-machine with `CF-Access-Client-Id` /
`CF-Access-Client-Secret`, and the secret is displayed exactly once
([Service tokens](https://developers.cloudflare.com/cloudflare-one/access-controls/service-credentials/service-tokens/)).
Cloudflare also documents an **MCP-specific** path: an MCP server portal that fronts one or many MCP
servers on a single `/mcp` endpoint, accepts stateless MCP `2026-07-28` (and earlier Streamable HTTP)
clients, returns `401` plus `WWW-Authenticate` OAuth discovery metadata to non-browser clients, and
supports service-token access ([MCP server portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)).

*Operational cost.* An account, a connector binary/service on the Windows box, a DNS zone for named
tunnels, and an identity provider for Access. This is the least operator work of any option here, and
the one the brief excludes.

#### ngrok

*What it is.* An agent that opens endpoints on the public internet from behind NAT/firewall
([ngrok agent](https://ngrok.com/docs/agent/)).

*Mechanism.* On start, the agent and the Agent SDKs "establish long-lived TLS connections to the ngrok
service through which they create new endpoints and receive connections from ngrok's cloud service
that are intended for your upstream"; all connections to ngrok servers are made on **port 443**
([ngrok agent](https://ngrok.com/docs/agent/)). Outbound only.

*Auth model.* The agent authenticates with an **authtoken** from the dashboard
(`ngrok config add-authtoken <TOKEN>`), and ngrok recommends a separate authtoken per agent so a
compromised token is isolated and ACLs can be applied per agent; the full token is shown only once
([ngrok agent](https://ngrok.com/docs/agent/)). Visitor access can be gated independently with the
OAuth Traffic Policy action, which "restricts access to your endpoints to only authorized users by
enforcing OAuth through an identity provider of your choice", configured with a provider, client
ID/secret, scopes and session duration
([OAuth action](https://ngrok.com/docs/traffic-policy/actions/oauth/)).

*Operational cost.* Minimal — one binary plus a token. A third party terminates the TLS session in
front of your service unless you layer your own auth.

#### frp

*What it is.* "frp is a fast reverse proxy that allows you to expose a local server located behind a
NAT or firewall to the Internet", supporting TCP/UDP plus HTTP/HTTPS with domain-based routing
([README](https://github.com/fatedier/frp)).

*Mechanism.* `frps` runs on a host with a public IP and a `bindPort` (default example 7000); `frpc`
runs on the LAN host, sets `serverAddr` to the public server, and dials it
([README](https://github.com/fatedier/frp)). The server never dials into the LAN.

*Auth model.* `frpc` and `frps` share a **token** by default (`auth.token`, or `tokenSource` from a
file or an external command — the latter requires `--allow-unsafe=TokenSourceExec`), or they can use
**OIDC** provider authentication
([Authentication](https://gofrp.org/en/docs/features/common/authentication/)). Transport between them
can be TLS-encrypted with optional unidirectional or mutual certificate verification; setting
`transport.tls.force = true` on `frps` makes it accept only TLS clients and is a prerequisite for
server-side verification of `frpc`
([Custom TLS Protocol Encryption](https://gofrp.org/en/docs/features/common/network/network-tls/)).
Encryption and STCP protect content but "cannot determine whether the other party's identity is
legitimate, posing a risk of man-in-the-middle attacks" without TLS verification
([same page](https://gofrp.org/en/docs/features/common/network/network-tls/)). For a service that must
not be public at all, the `stcp` (secret tcp) proxy type requires visitors to run their own `frpc`
with the same secret, so the service is reachable only by authorized visitors
([STCP example](https://gofrp.org/en/docs/examples/stcp/), [README feature list](https://github.com/fatedier/frp)).

*Operational cost.* You own a public host, the server config, patching, and TLS material. No third
party in the data path.

#### localtunnel

*What it is.* "localtunnel exposes your localhost to the world for easy testing and sharing! No need to
mess with DNS or deploy just to have others test out your changes"
([README](https://github.com/localtunnel/localtunnel)).

*Mechanism.* The client connects to a tunnel server and is told which URL to use; the URL stays alive
for the session and the client reconnects if the local server restarts; `--subdomain` requests a named
subdomain and the default is random characters
([README](https://github.com/localtunnel/localtunnel)). Self-hosting the server requires DNS for
`domain.tld` and `*.domain.tld` and the ability to accept incoming TCP connections on a non-root port;
the server "will listen on any OS-assigned TCP port for client connections"
([server README](https://github.com/localtunnel/server)).

*Auth model.* Neither the client README nor the server README documents an authentication mechanism
for the tunnel or for visitors; the public default is the `localtunnel.me` service
([client](https://github.com/localtunnel/localtunnel), [server](https://github.com/localtunnel/server)).
The server README warns against running without SSL ("If you do not want ssl support for your own
tunnel (not recommended)") ([server README](https://github.com/localtunnel/server)). Treat this as a
development tool: no auth story, no SLA, an explicitly random public URL.

### 3.2 Mesh VPN

#### Tailscale

*What it is.* "Tailscale creates a peer-to-peer mesh network (known as a tailnet)", enabling encrypted
point-to-point connections using the WireGuard protocol so that "only devices on your private network
can communicate with each other"; connections work "across firewalls and Network Address Translation
(NAT) without requiring port forwarding or complex firewall rules"
([What is a tailnet](https://tailscale.com/kb/1151/what-is-a-tailnet)).

*Mechanism.* Every participating device runs the Tailscale client and is authenticated into the
tailnet. There is no relay dial-in to the NAT'd host; the mesh resolves paths itself (with relay
fallback), and Tailscale notes traffic can flow directly between devices
([What is a tailnet](https://tailscale.com/kb/1151/what-is-a-tailnet)).

*Two publication modes:*
- `tailscale serve` — "lets you share a local service securely within your Tailscale network
  (tailnet)"; it can also be exposed publicly with `tailscale funnel`, "open to the entire internet"
  ([tailscale serve](https://tailscale.com/kb/1242/tailscale-serve)).
- `tailscale funnel` — routes traffic "from the broader internet to a local service running on a device
  in your Tailscale network … for anyone to access—even if they don't use Tailscale"; it is **beta**,
  requires HTTPS certificates enabled for the tailnet plus a `funnel node` attribute in the tailnet
  policy file, and can only listen on ports **443, 8443 and 10000**
  ([Funnel](https://tailscale.com/kb/1223/funnel)). Public DNS resolves to a Funnel relay server, not
  the device's IP, and the relay does not decrypt the traffic
  ([Funnel](https://tailscale.com/kb/1223/funnel)).

*Auth model.* For a tailnet, access control is the tailnet policy file: ACLs and grants follow
deny-by-default and are directional; **but** "in the absence of an `acls` section in the tailnet
policy file, Tailscale applies the default allow all policy", and a new tailnet's default policy
enables communication between all its devices
([ACLs](https://tailscale.com/kb/1018/acls)). Funnel is public — the app must bring its own
authentication.

*Operational cost.* Zero for the NAT'd box beyond installing the client and logging in; one client
install per WAN machine for tailnet access, none for Funnel. The trade is identity living with
Tailscale.

#### WireGuard

*What it is.* A general-purpose VPN that "securely encapsulates IP packets over UDP"; an interface is
configured with a private key and peers' public keys, and "all issues of key distribution and pushed
configurations are out of scope of WireGuard" ([WireGuard](https://www.wireguard.com/)).

*Mechanism.* Each peer is associated with a remote endpoint (IP:port); when the interface sends a
packet it encrypts it for the peer's public key and sends it to "that peer's most recent Internet
endpoint", and inbound packets update the remembered endpoint — this is the built-in roaming behaviour
([WireGuard](https://www.wireguard.com/)). WireGuard itself provides no discovery, relay or NAT
traversal service: reaching a peer behind NAT requires some peer that is reachable, or a separate
coordination layer (which is exactly what Tailscale/ZeroTier add on top — Tailscale describes itself
as "enhanced" WireGuard with NAT-transparent connectivity
([What is a tailnet](https://tailscale.com/kb/1151/what-is-a-tailnet))).

*Auth model.* Public-key pairs per peer, mapped to allowed tunnel IPs via the cryptokey routing table
([WireGuard](https://www.wireguard.com/)). Network-level: once on the interface, a peer can reach
whatever the allowed-IPs say.

*Operational cost.* Low software cost, real configuration cost: key exchange, endpoint management, and
a relay/rendezvous host if both ends are behind NAT.

#### ZeroTier

*What it is.* A virtual networking platform: you create a network, join devices to it, and then
authorize each device on the network before it can communicate
([Getting started](https://docs.zerotier.com/start/)).

*Mechanism.* Devices join a network by ID and must be authorized in ZeroTier Central; after that
"two authorized nodes … can talk over ZeroTier"
([Getting started](https://docs.zerotier.com/start/)).

*Auth model / security claims.* "All network traffic uses state-of-the-art asymmetric encryption with
private keys that never leave the device", and "ZeroTier routes traffic directly between peers, so our
infrastructure cannot observe or modify packets on user networks"
([ZeroTier security](https://docs.zerotier.com/security/)). Membership authorization is the gate
([Getting started](https://docs.zerotier.com/start/)).

*Operational cost.* A client on every device, plus Central membership management — like Tailscale,
this fails the "no VPN client on every machine" constraint.

### 3.3 SSH tunnels

*What it is.* Port forwarding built into OpenSSH. `-L [bind_address:]port:host:hostport` forwards a
local listening port to a host/port on the remote side; `-R [bind_address:]port:host:hostport`
allocates a listening socket on the **remote** host and forwards connections back to the local side
([ssh(1)](https://man.openbsd.org/ssh.1)).

*Mechanism for a NAT'd server.* The NAT'd host runs `ssh -R` **outbound** to a machine that has a
public address and an SSH server; that public machine now has a listening socket which tunnels back
over the encrypted channel to the LAN service ([ssh(1)](https://man.openbsd.org/ssh.1)). The server
never dials in; the WAN client connects to the public host's forwarded port.

*Auth model.* Whatever `sshd` requires (keys/passwords/2FA) for the tunnel itself, plus whatever auth
the forwarded service has for its users. Note the default is conservative: "By default, TCP listening
sockets on the server will be bound to the loopback interface only", and a non-loopback bind address
only succeeds if the server has `GatewayPorts` enabled — whose default is `no`
([ssh(1)](https://man.openbsd.org/ssh.1), [sshd_config(5)](https://man.openbsd.org/sshd_config)).
So a bare `ssh -R 8080:localhost:18080 public-host` publishes the port **only on the public host's
loopback**, which is what you want if a reverse proxy on that host fronts it; publishing it on all
interfaces requires `GatewayPorts clientspecified|yes` on the server
([sshd_config(5)](https://man.openbsd.org/sshd_config)).

*Operational cost.* A reachable SSH host (VPS or office server), key management, and a supervised
long-lived SSH connection (`autossh`-style respawn is common but is not part of OpenSSH). Very little
new code.

### 3.4 Self-hosted "agent dials out to a server" products

These are the closest architectural analogues to Vale: a managed control plane, and an agent that
establishes the connection outward. In every one of them, **the server never dials in**.

#### Portainer Edge Agent

- *Who dials whom.* Edge agents **poll** the Portainer instance "every 5 seconds by default"; they
  check in with their Edge UUID and join token, and if Portainer needs them it replies "Yes, I do need
  you. Please connect using these tunnel credentials." The agent then opens a **TLS tunnel on port
  8000** to the Portainer instance ([The Portainer Edge Agent](https://docs.portainer.io/advanced/edge-agent)).
  Portainer's design note is explicit that this inversion exists so that "only the environments need
  to access Portainer. There is now no need to expose the Portainer agents to the Internet"
  ([same](https://docs.portainer.io/advanced/edge-agent)).
- *Does the server ever dial in?* No — it answers the poll; the agent opens the tunnel.
- *Auth.* A per-environment random Edge ID (UUID) and a join token (`EDGE_KEY`); if UUID/join token do
  not match, the connection is rejected; tunnel credentials are encrypted with the Edge UUID and
  intended as one-time-use ([same](https://docs.portainer.io/advanced/edge-agent)).
- *Cost note from the vendor:* 15,000 environments polling at 5 s produced ~7 Mbps and needed 4 CPUs
  for encryption/tunnel load ([same](https://docs.portainer.io/advanced/edge-agent)).

#### GitLab Runner

- *Who dials whom.* The runner registers with a GitLab instance and picks up jobs from it; "all runners
  will need outbound network connectivity to GitLab.com or your GitLab instance"
  ([Runner security](https://docs.gitlab.com/runner/security/)). The server does not dial into the
  runner.
- *Auth.* Registration links the runner using a **runner authentication token** (prefix `glrt-`),
  stored in `config.toml` ([Registering runners](https://docs.gitlab.com/runner/register/)).
- *Security model.* GitLab's own docs are blunt that the runner "is designed to run user-controlled
  scripts" and recommend network segmentation, blocking SSH from the internet, restricting runner-to-
  runner traffic, filtering cloud metadata endpoints, and cleaning the build directory
  ([Runner security](https://docs.gitlab.com/runner/security/)). This is the cautionary analogue: an
  outbound agent that executes commands is still a remote-execution surface, and the vendor treats
  network isolation, not authentication alone, as the control.

#### VS Code Remote Tunnels (Microsoft dev tunnels)

- *Who dials whom.* The `code tunnel` CLI runs **on the remote machine**, starts VS Code Server there
  and creates the tunnel to it; the client opens a `https://vscode.dev/tunnel/...` URL or connects with
  the Remote - Tunnels extension ([Remote Tunnels](https://code.visualstudio.com/docs/remote/tunnels)).
  Microsoft's service documentation states the rule directly: "Tunneling requires outbound connections
  to be made to the service hosted in Azure. **No inbound connections are required to use the
  service**" ([Dev tunnels security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)).
- *Does the server ever dial in?* No.
- *Auth.* By default, hosting and connecting both require authentication with the same Microsoft,
  Microsoft Entra ID, or GitHub account that created the tunnel; tunnels and ports are private to the
  creator unless an `--allow-anonymous` access control entry is added, and access can be extended to
  an Entra tenant or GitHub organizations; scoped access tokens (`devtunnel token`) exist for limited
  delegated access ([Dev tunnels security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security),
  [Remote Tunnels](https://code.visualstudio.com/docs/remote/tunnels)). Transport: HSTS with a one-year
  max-age, minimum TLS 1.2 (1.3 preferred), TLS terminated at service ingress with Microsoft CA
  certificates ([Dev tunnels security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)).

#### Home Assistant Cloud / Nabu Casa

- *Who dials whom.* The Home Assistant instance dials out to Nabu Casa's infrastructure and holds a
  multiplexed connection; external users reach the instance through that. The protocol library's
  connection flow shows: the client (the instance) authenticates to a session master, receives a
  **Fernet token** containing an expiry, hostname and AES key, connects to the SniTun server, answers
  a challenge-response to prove key possession, "enters multiplexer mode", and only then does an
  external device connection to the SniTun server get forwarded over the multiplexer to a local
  connection on the instance ([SniTun README](https://github.com/NabuCasa/snitun)). The SniTun server
  rejects the token once its expiry passes ([same](https://github.com/NabuCasa/snitun)).
- *Does the server ever dial in?* No inbound connection to the home network is required; the external
  device connects to the relay.
- *Auth.* Nabu Casa account + the instance's token minted by the session master
  ([SniTun README](https://github.com/NabuCasa/snitun)); the marketing/security claim is end-to-end
  encryption between the user's device and the instance with Nabu Casa unable to view the data
  ([Nabu Casa](https://www.nabucasa.com/)). Home Assistant documents Cloud as the optional
  subscription that "adds secure remote access to your Home Assistant from anywhere"
  ([Home Assistant Cloud](https://www.home-assistant.io/cloud/)).

#### GitHub Actions self-hosted runners

- *Who dials whom.* The runner is a system you deploy that executes jobs from GitHub
  ([Self-hosted runners](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners));
  the machine "must be able to make outbound HTTPS connections over port 443" to communicate with
  GitHub Actions — the requirements are stated as outbound only
  ([Self-hosted runners reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)).
- *Does the server ever dial in?* No.
- *Auth.* Runner registration/authentication credentials scoped to a repository, organization or
  enterprise ([Self-hosted runners](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners)).
- *Risk model.* GitHub warns that self-hosted runners have no ephemeral clean-VM guarantee, "can be
  persistently compromised by untrusted code in a workflow", and "should almost never be used for
  public repositories … because any user can open pull requests against the repository and compromise
  the environment" ([Security hardening for GitHub Actions](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions#hardening-for-self-hosted-runners)).

### 3.5 The trade, in one table

| Pattern | Who dials whom | Nothing listens publicly | Auth in the box | Client software on the WAN machine |
|---|---|---|---|---|
| Cloudflare Tunnel + Access | agent → Cloudflare; client → Cloudflare | yes ([overview](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/)) | Access identity policy, deny by default ([docs](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/)) | an MCP client only |
| ngrok + OAuth action | agent → ngrok:443 ([agent](https://ngrok.com/docs/agent/)) | yes | ngrok authtoken + OAuth IdP ([OAuth action](https://ngrok.com/docs/traffic-policy/actions/oauth/)) | an MCP client only |
| frp (self-hosted) | `frpc` → `frps` ([README](https://github.com/fatedier/frp)) | yes, on the LAN | shared token or OIDC, optional mutual TLS ([auth](https://gofrp.org/en/docs/features/common/authentication/), [TLS](https://gofrp.org/en/docs/features/common/network/network-tls/)) | an MCP client only |
| localtunnel | client → tunnel server ([README](https://github.com/localtunnel/localtunnel)) | yes, on the LAN | undocumented | an MCP client only |
| Tailscale tailnet | mesh; agent → peers ([tailnet](https://tailscale.com/kb/1151/what-is-a-tailnet)) | yes | tailnet policy file, deny-by-default only if ACLs are written ([ACLs](https://tailscale.com/kb/1018/acls)) | **Tailscale client** |
| Tailscale Funnel | agent → relay; public → relay ([Funnel](https://tailscale.com/kb/1223/funnel)) | yes | app's own; Funnel is public | an MCP client only |
| SSH `-R` | agent → public sshd ([ssh(1)](https://man.openbsd.org/ssh.1)) | yes, unless `GatewayPorts` ([sshd_config](https://man.openbsd.org/sshd_config)) | sshd auth + service auth | an MCP client (or a proxy) |
| Portainer Edge | agent polls + opens tunnel ([docs](https://docs.portainer.io/advanced/edge-agent)) | yes | Edge UUID + join token | product client |
| VS Code tunnels | CLI on the box → Azure ([security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)) | yes | Microsoft/Entra/GitHub account, private by default | VS Code / browser |
| HA Cloud | instance → Nabu Casa multiplexer ([SniTun](https://github.com/NabuCasa/snitun)) | yes | account + expiring Fernet token | browser/app |
| GitHub runners | runner → GitHub:443 ([reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)) | yes | runner credentials | GitHub |

---

## 4. HTTPS + auth in front of a loopback service

### 4.1 Reverse proxies with automatic certificates

This is the dominant answer, and the three mainstream proxies document it as a first-class flow.

- **Caddy** "provisions TLS certificates for all your sites and keeps them renewed. It also redirects
  HTTP to HTTPS for you": public DNS names get certificates from a public ACME CA such as Let's
  Encrypt or ZeroSSL, while IP addresses and internal/local hostnames get automatically trusted
  self-signed certificates; HTTP (80) is redirected to HTTPS (443) automatically
  ([Automatic HTTPS](https://caddyserver.com/docs/automatic-https)). The minimal reverse-proxy form is
  the `reverse_proxy` directive, and Caddy's quick start shows `reverse_proxy :9000` with the hostname
  on the first line deciding whether HTTPS is real or self-signed
  ([Reverse proxy quick-start](https://caddyserver.com/docs/quick-starts/reverse-proxy)).
- **nginx** is configured with `listen 443 ssl;` plus `ssl_certificate`/`ssl_certificate_key`
  ([Configuring HTTPS servers](https://nginx.org/en/docs/http/configuring_https_servers.html)), and
  `proxy_pass http://localhost:8000;` is the documented example of forwarding to a loopback upstream
  ([ngx_http_proxy_module](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass)).
  Certificates are not automatic: Certbot is the standard client, with ACME-based issuance and
  automated renewals configured for nginx and Apache
  ([Certbot docs](https://eff-certbot.readthedocs.io/en/stable/using.html)).
- **Traefik** has an ACME certificate resolver with automatic renewal and the usual challenge types
  ([ACME](https://doc.traefik.io/traefik/reference/install-configuration/tls/certificate-resolvers/acme/)),
  with TLS options configured per router ([TLS overview](https://doc.traefik.io/traefik/reference/routing-configuration/http/tls/overview/)).

The important precondition, common to all three: automatic public certificates need a public DNS name
and the CA's reachability of ports 80/443. Caddy states that these are "common requirements for any
basic production website … set your DNS records properly before running Caddy so it can provision
certificates", and warns that clients not trusting Caddy's root CA will show security errors for
self-signed sites ([Automatic HTTPS](https://caddyserver.com/docs/automatic-https)). A NAT'd host with
no port forwarding therefore cannot use automatic certificates on its own — which is precisely why the
tunnel/relay patterns in §3 exist, and why the sensible composition is *tunnel/relay on the outside,
reverse proxy + auth on the inside*.

### 4.2 What the mainstream guidance says about a bearer token as the only gate

- RFC 6750, which MCP's authorization section builds on, says clients "**MUST** always use TLS (https)
  or equivalent transport security when making requests with bearer tokens", because otherwise the
  token is exposed; clients must also validate certificate chains, and tokens must not be stored in
  cookies ([RFC 6750 §5.3](https://www.rfc-editor.org/rfc/rfc6750.html#section-5.3)).
- MCP's own security chapter forbids MCP servers from accepting or transiting tokens issued for
  anything else, and requires audience validation — a bare long-lived static token presented to the
  MCP server has no audience binding and no issuer to validate
  ([Security Best Practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices),
  [Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)).
- Where the ecosystem's own managed products front MCP servers, they put an identity layer in front
  rather than relying on the server's token: Cloudflare's MCP portal authenticates the user through
  Access via an identity provider, or accepts an Access service token for machine-to-machine, and
  returns OAuth discovery metadata to non-browser clients
  ([MCP server portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)).
  ngrok's equivalent is the OAuth traffic-policy action
  ([OAuth action](https://ngrok.com/docs/traffic-policy/actions/oauth/)).
- Reading these together: a bearer token in front of a loopback service is acceptable **only** over
  TLS, **only** as one layer, and ideally as a token a client can rotate — nothing in the primary
  sources endorses "public URL + static bearer token" as a complete posture for a remote-execution
  surface.

---

## 5. What the sources say about the risks of exposing remote execution

### 5.1 Rate limiting and lockout

- OWASP's API Security Top 10 lists **API2:2023 Broken Authentication**, whose weakness list includes
  permitting credential stuffing and permitting brute force "without presenting captcha/account
  lockout mechanism"; its prevention guidance is to "implement anti-brute force mechanisms to mitigate
  credential stuffing, dictionary attacks, and brute force attacks on your authentication endpoints"
  and to make that mechanism "stricter than the regular rate limiting mechanisms", plus account
  lockout/CAPTCHA for attacks against specific users; it also calls for checking "weak or predictable
  tokens used to enforce authentication"
  ([OWASP API2:2023](https://owasp.org/API-Security/editions/2023/en/0xa2-broken-authentication/)).
- The OWASP Authentication Cheat Sheet's controls are **Login Throttling** (a maximum number of
  attempts) and **Account Lockout** ([Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)),
  and it notes MFA is "by far the best defense against the majority of password-related attacks".
- Nothing in the MCP specification sets a rate limit or lockout requirement; its transport security
  section asks only for Origin validation, loopback binding when local, and "proper authentication"
  ([Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)).
  Rate limiting is therefore an operator/proxy-layer duty in MCP's own reading.
- The tunnel products do not supply it either: Access policies are allow/deny authorization decisions
  ([Access policies](https://developers.cloudflare.com/cloudflare-one/access-controls/policies/)), and
  the ngrok OAuth action gates visitors by identity
  ([OAuth action](https://ngrok.com/docs/traffic-policy/actions/oauth/)); neither page documents
  per-token attempt throttling.

### 5.2 Token handling

- Bearer tokens are bearer instruments: "client implementations **MUST** ensure that bearer tokens are
  not leaked to unintended parties, as they will be able to use them to gain access to protected
  resources" ([RFC 6750 §5.3](https://www.rfc-editor.org/rfc/rfc6750.html#section-5.3)).
- MCP forbids token passthrough and requires audience-bound tokens
  ([Security Best Practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices)).
- MCP clients already do their part: Claude Code ignores an entry with a `url` but no `type`, warns
  about hidden leading/trailing whitespace in `headers.Authorization` without echoing the value, and
  never includes the expanded server URL (which can carry secrets) in failure detail
  ([Claude Code MCP](https://docs.claude.com/en/docs/claude-code/mcp)). OpenAI's SDK security note says
  to keep access tokens "in authorization fields or headers rather than URLs"
  ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/)).
- Concrete operational consequence from Cloudflare's service tokens: the client secret is displayed
  exactly once and, if lost, must be regenerated
  ([Service tokens](https://developers.cloudflare.com/cloudflare-one/access-controls/service-credentials/service-tokens/)).
  The same "shown once" property is documented for ngrok authtokens
  ([ngrok agent](https://ngrok.com/docs/agent/)).

### 5.3 TLS

- OWASP: "Web applications must default to TLS 1.3 and may support TLS 1.2 for compatibility. TLS 1.0
  and TLS 1.1 are formally deprecated by RFC 8996 … and must be disabled"; SSLv2/v3 must always be
  disabled, and AEAD cipher suites are preferred
  ([Transport Layer Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Transport_Layer_Security_Cheat_Sheet.html)).
- Microsoft's dev tunnels service is a useful reference implementation of the floor: HSTS with a
  one-year max-age, minimum TLS 1.2 with TLS 1.3 preferred, TLS terminated at ingress
  ([Dev tunnels security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)).
- MCP requires HTTPS for authorization-server endpoints and for redirect URIs other than localhost,
  with PKCE `S256` ([Authorization Security Considerations](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations)),
  and RFC 6750 requires TLS for any bearer-token request
  ([RFC 6750](https://www.rfc-editor.org/rfc/rfc6750.html#section-5.3)).

---

## 6. WHAT THIS MEANS FOR VALE

Ranked from least to most product work. "Product work" = changes to this repo shipped to operators;
everything else is operator configuration on hardware they already have.

**Option A — Operator-run tunnel, no product change (operational only).**
The operator runs `cloudflared` (named tunnel + Access) or `ngrok` (agent + OAuth action) or `frpc`
against their own VPS, targeting `127.0.0.1:18080`. Nothing in the agent changes; the loopback posture
and the existing bearer token stay as they are. The sources support every piece: outbound-only
connectors ([Cloudflare](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/),
[ngrok](https://ngrok.com/docs/agent/), [frp](https://github.com/fatedier/frp)) and an identity layer
in front ([Access](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/),
[ngrok OAuth](https://ngrok.com/docs/traffic-policy/actions/oauth/)). Cost: one binary and one account
per deployment; Cloudflare is excluded by the brief, which leaves ngrok or self-hosted frp.
*Product work: none.*

**Option B — Vale Gate as the relay the agent dials out to (the Portainer/VS Code/Nabu Casa shape).**
The agent keeps an outbound connection to the operator's own gateway and the gateway terminates TLS and
authenticates the calling client; `/mcp` is proxied over that outbound connection. This is the
architecture every comparable product already uses: Portainer's agents poll out and open the tunnel, so
"there is now no need to expose the Portainer agents to the Internet"
([Portainer](https://docs.portainer.io/advanced/edge-agent)); Microsoft's dev tunnels require "outbound
connections … No inbound connections are required"
([dev tunnels](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)); Home Assistant
Cloud's instance dials the multiplexer and external users arrive through it
([SniTun](https://github.com/NabuCasa/snitun)); VS Code's CLI creates the tunnel from the machine itself
([Remote Tunnels](https://code.visualstudio.com/docs/remote/tunnels)). It directly satisfies "no
Cloudflare" and "no VPN client on the client machine" — the WAN machine only needs an MCP client.
*Product work: high* (a persistent outbound channel, gateway-side proxying, auth on the gateway, and
reconnect/liveness semantics). The vendor cost evidence is worth heeding: Portainer measured ~7 Mbps
and 4 CPUs for 15,000 environments polling every 5 s
([Portainer](https://docs.portainer.io/advanced/edge-agent)).

**Option C — Tailscale on the agent host; WAN side uses the tailnet or Funnel.**
No product work. `tailscale serve` keeps the panel/MCP tailnet-only; `tailscale funnel` publishes it
publicly without any client install on the WAN machine, but Funnel is beta, limited to ports 443/8443/10000,
and requires HTTPS certificates plus a `funnel node` attribute
([Funnel](https://tailscale.com/kb/1223/funnel), [Serve](https://tailscale.com/kb/1242/tailscale-serve)).
The tailnet route needs the client installed on the WAN machine, which the brief rules out; the Funnel
route does not, at the price of a public URL with only the bearer token in front of it. Note the ACL
footgun: without an `acls` section the tailnet policy is **allow all** by default
([ACLs](https://tailscale.com/kb/1018/acls)).
*Product work: none.*

**Option D — Self-hosted frp or `ssh -R` on a VPS (no vendor, no new product surface).**
`frpc` dials the VPS and publishes the MCP endpoint; `frps` is protected by a shared token or OIDC and
optionally mutual TLS, or `stcp` requires visitors to run their own `frpc` with the secret so the
service is not public at all ([frp auth](https://gofrp.org/en/docs/features/common/authentication/),
[frp TLS](https://gofrp.org/en/docs/features/common/network/network-tls/),
[stcp](https://gofrp.org/en/docs/examples/stcp/)). The SSH variant is `ssh -R` from the Windows box to
the VPS, keeping the forwarded port on the VPS loopback (the `GatewayPorts` default) and fronting it
there with a reverse proxy ([ssh(1)](https://man.openbsd.org/ssh.1),
[sshd_config(5)](https://man.openbsd.org/sshd_config)). Cost: the operator owns a VPS, its patching,
and TLS material.
*Product work: none.*

**Option E — Move the agent off loopback behind a public reverse proxy.**
Only viable with a public host or port-forward. Caddy/nginx/Traefik + automatic certificates is the
documented way to put HTTPS in front ([Caddy](https://caddyserver.com/docs/automatic-https),
[nginx](https://nginx.org/en/docs/http/configuring_https_servers.html),
[Traefik](https://doc.traefik.io/traefik/reference/install-configuration/tls/certificate-resolvers/acme/)),
but it contradicts the MCP transport guidance to bind loopback when local
([Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http))
and it makes the panel public too unless the proxy is path-scoped. Highest risk, and the only option
that makes the *panel* internet-facing.
*Product work: none in the agent, but the operator takes on TLS, patching, and exposure.*

**Option F — Harden `/mcp` itself regardless of which transport wins (orthogonal, and the sources are unambiguous).**
Whatever fronts the agent, the server side should look like a modern MCP resource server rather than a
static-token endpoint: publish RFC 9728 protected-resource metadata, validate token audience, never
accept or transit foreign tokens, and require HTTPS
([MCP Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization),
[Security Best Practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices)).
This is what makes Option B's relay able to hand the gateway a verifiable identity instead of a shared
secret, and it is what Cloudflare's own MCP portal assumes when it answers non-browser clients with
`401` + `WWW-Authenticate` ([MCP server portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)).
*Product work: medium* (metadata endpoint, token validation, scope handling), and it is prerequisite
work for any deployment where a third party — an MCP client vendor's cloud, a reverse proxy, an
identity provider — must decide whether to trust the call.

**Which the sources most support for this exact case.** Eliminating Cloudflare and per-client VPN
installs leaves two shapes: (i) a relay the agent dials out to, with TLS and identity at the relay —
the architecture of every comparable product named in §3.4; or (ii) a self-hosted frp/`ssh -R` VPS with
the reverse proxy on the VPS. For Vale specifically, Option B is the same shape as the products the
sources describe and is the only one that also gives the operator a place to enforce OWASP-style
lockout and throttling ([OWASP API2:2023](https://owasp.org/API-Security/editions/2023/en/0xa2-broken-authentication/))
and to keep the agent's loopback binding intact. Option D is the fastest credible path today with zero
product change, and Option F is the part of B that cannot be deferred.

---

## UNSETTLED

Things the primary sources do **not** decide. Each is stated as an open question, not a finding.

1. **No spec-level answer for NAT.** MCP defines stdio, Streamable HTTP and custom transports, and says
   a server may be a remote service, but says nothing about relays, tunnels, reverse proxies or NAT
   traversal ([Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports),
   [Architecture](https://modelcontextprotocol.io/specification/2026-07-28/architecture)). Which
   exposure pattern is "MCP-compliant" is simply not a question the spec answers.
2. **"Proper authentication" is not defined normatively.** The Streamable HTTP binding says servers
   **SHOULD** implement proper authentication and **MUST** validate `Origin`
   ([Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)),
   and the authorization chapter's OAuth 2.1 machinery applies when the server is *protected* — the
   spec never says a static bearer token is insufficient, nor that a public URL is impermissible.
3. **No rate-limiting or lockout requirement anywhere in MCP.** The only concrete attempt-throttling
   guidance found is OWASP's ([Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html),
   [API2:2023](https://owasp.org/API-Security/editions/2023/en/0xa2-broken-authentication/)); no MCP
   or tunnel-vendor page read here specifies a numeric budget for an MCP endpoint.
4. **Proxy behaviour for MCP's transport headers is unspecified.** 2026-07-28 requires `Mcp-Method` and
   `Mcp-Name` headers on POSTs and supports custom headers via `x-mcp-header`
   ([Key changes](https://modelcontextprotocol.io/specification/2026-07-28/changelog)), and says the
   body remains the source of truth with bindings defining mismatch handling
   ([Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)). Whether
   any given tunnel/reverse proxy preserves or rejects those headers, and how SSE responses survive
   buffering intermediaries, is documented by neither the spec nor the proxies' pages read here.
5. **Tunnel products' production/abuse policies.** Cloudflare says quick tunnels are for testing only
   ([Quick Tunnels](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/)),
   but no page read states quota, burst, or acceptable-use limits for a *named* tunnel carrying MCP;
   ngrok's pages read here do not state which plans include the OAuth action; localtunnel documents no
   auth or durability guarantees at all ([client](https://github.com/localtunnel/localtunnel),
   [server](https://github.com/localtunnel/server)).
6. **Tailscale Funnel's operational limits beyond ports.** Beta status, the 443/8443/10000 port
   restriction and the certificate/attribute prerequisites are documented
   ([Funnel](https://tailscale.com/kb/1223/funnel)); rate limits, per-request logging, and any abuse
   policy for a public Funnel endpoint are not.
7. **WireGuard alone has no NAT story, by design.** The project documents peer endpoints and roaming
   but leaves key distribution, discovery and configuration "to other layers"
   ([WireGuard](https://www.wireguard.com/)); the docs do not say what to run when both ends are behind
   NAT, and Tailscale's/ZeroTier's coordination is a separate product claim
   ([Tailscale](https://tailscale.com/kb/1151/what-is-a-tailnet), [ZeroTier](https://docs.zerotier.com/security/)).
8. **ZeroTier's "no inbound ports" property is asserted only indirectly.** Its pages read here cover
   encryption, direct peer routing and membership authorization
   ([security](https://docs.zerotier.com/security/), [getting started](https://docs.zerotier.com/start/)),
   but none states the firewall/inbound-port requirement as plainly as Microsoft's dev-tunnels page does
   ([dev tunnels](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)).
9. **GitHub runner docs state the outbound requirement but never say "no inbound".** The reference says
   the host must make outbound HTTPS on 443
   ([reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)); the absence
   of an inbound requirement is inference, not a quoted rule.
10. **OpenAI's own MCP guide pages could not be read from this network.** `platform.openai.com` and
    `developers.openai.com` returned HTTP 403 (Cloudflare) to every fetch attempted here, so the
    hosted-MCP and connector claims above rest on OpenAI's official Agents SDK documentation repository
    ([Agents SDK MCP](https://openai.github.io/openai-agents-python/mcp/), which links to
    [the hosted MCP security guide](https://developers.openai.com/api/docs/guides/tools-connectors-mcp#risks-and-safety)).
    The exact wording of that guide — approval requirements, allowed-tools semantics, connector scopes —
    is unverified in this document.
11. **Whether any mainstream MCP client will accept a self-signed certificate.** Claude Code, Cursor and
    VS Code accept arbitrary URLs, but no page read here says whether they trust a private CA; Caddy's
    page warns that clients not trusting its root CA "will show security errors"
    ([Caddy](https://caddyserver.com/docs/automatic-https)), which suggests the answer matters and is
    client-specific.
12. **No conformance or interop statement for "MCP through a tunnel".** Every tunnel vendor's page read
    here is transport-agnostic HTTP; only Cloudflare documents MCP-specific behaviour (protocol
    selection, tool namespacing, `401` + `WWW-Authenticate` for non-browser clients)
    ([MCP server portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)).
    Whether frp, ngrok, Tailscale Funnel or an `ssh -R` forward preserve stateless MCP 2026-07-28
    semantics end-to-end is untested by any source found.
