# Quick start: an installed device to an AI client

Five minutes from an installed device to an AI client holding a permanent token. The long forms are
[`CONNECT-MCP.md`](CONNECT-MCP.md) (the endpoint, grants and Access) and
[`RELEASE.md`](RELEASE.md) (how a console is deployed and what each public path answers).

## 1. Install

Install from the release; the assets, the silent flags and the uninstall are in
[`README.md`](../README.md#install-from-a-release):

```bat
SummriseSetup-<version>.exe /S /D=C:\SummriseInstall
```

`http://127.0.0.1:18080/` serves the device's own panel; canary builds published so far use **18090**.
It is loopback only and has no login — if you can read it, you are on the box. The service is up when
`curl http://127.0.0.1:18080/api/health` answers.

## 2. Enroll the machine

In the console: **Devices → Add a device → New enrollment code**. The code is single-use and expires
in minutes. Spend it either way:

- **At the machine:** in the panel, open **Console link → Enrol this machine**, paste the console URL
  and the code, and press **Enroll**. Tick **Replace an existing enrollment** only to rotate this
  machine's token.
- **From an elevated command line:**

  ```bat
  summrise-device.exe --enroll <console-url> <code> --data C:\ProgramData\Summrise
  ```

  Add `--force` to replace an existing registration, and `--allow-plaintext` only for a console on a
  trusted plaintext tunnel.

Either path spends the code once and writes the console link and channel token under the data
directory. The app builds its console link at boot. A **panel** enrollment on a supervised install
asks the launcher to restart the app automatically (the page says so); after a **command-line**
enrollment, restart the service yourself, and the card turns **online**:

```bat
sc stop SummriseLaunch
sc start SummriseLaunch
```

## 3. Create a permanent token

In the console, select the device and press **Connect an AI client** (the device card's **AI client**
button opens the same dialog), then choose **Permanent token**. Name the token, select the tools it may
call (at least one), decide whether destructive tools are pre-approved, and press **Create token**.

The value is `srm_` plus 64 lowercase hex characters, shown once and stored only as a hash: neither the
list nor the audit can produce it again, so keep it in a secret, not a repository or a screenshot. It
is permanent by default; an optional expiry (1 hour–365 days) can be set at creation, and revocation
is the only withdrawal for a permanent token ([the scope rules](CONNECT-MCP.md#scope-and-revocation)).

## 4. Point the client at the endpoint

The endpoint is the console's MCP route for that device; the device id is on its card (64 lowercase
hex characters):

```http
POST https://console.saisi.online/mcp/<device-id>
Authorization: Bearer srm_<64 lowercase hex characters>
```

Any streamable-HTTP MCP client takes those two facts; the transport sends `Content-Type: application/json`
and `Accept: application/json, text/event-stream` ([the wire form](CONNECT-MCP.md#the-endpoint)).

For DeepSeek Harness, a fixed configuration with the token written literally — keep the file out of the
repository, or have the deployment inject the value from a secret:

```yaml
- id: mcp-summrise-device
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: summrise-device
    transport: streamable-http
    url: https://console.saisi.online/mcp/<device-id>
    headers:
      Authorization: Bearer srm_<64 lowercase hex characters>
```

The same endpoint accepts a grant for a session instead of a token: the card's **Session grant** tab
mints one (an hour by default), and its lowercase hex goes in the same header
([details](CONNECT-MCP.md#mint-a-grant)). A grant expires; a permanent token does not (one created
with an expiry does). Behind Cloudflare
Access with a service-token policy for `/mcp/*`, add the two `CF-Access-*` headers too
([details](CONNECT-MCP.md#cloudflare-access)).

## 5. Check it

With the token and the device id in the shell:

```bash
TOKEN='srm_<64 lowercase hex characters>'
curl -sS -X POST "https://console.saisi.online/mcp/<device-id>" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

`result.tools` is exactly the token's tool set: a tool you did not select is absent, not refused. An
AI client that lists the tools (under `mcp__summrise-device__<tool>` in DeepSeek Harness) made the same
call; [`CONNECT-MCP.md#a-quick-check`](CONNECT-MCP.md#a-quick-check) has `initialize` and `tools/call`.

## 6. Revoke

In **Connect an AI client → Permanent token → Existing tokens**, press **Revoke** and confirm. The next
request with that token is `401`, and the value is never shown again. Create a new token and update the
client's configuration if the client should keep working.

## 7. Local only: no console

A device is usable by the machine it runs on with no console at all. The install token at `<data>/token`
— `C:\ProgramData\Summrise\token` on a default Windows install — is the local owner credential: reading
that file (`Get-Content C:\ProgramData\Summrise\token`, as an administrator) is what proves you own the
box, and a call bearing it is allowed outright, including destructive tools, with no grant and no
approval prompt. The file is protected to the service account and administrators; treat it as this
machine's password.

The panel's **Use this machine** section shows the loopback endpoint (`http://127.0.0.1:18080/mcp`), the
token file's path and read command, and a client configuration with the value left out — the page is
served to anyone on loopback, so it does not print the secret. Send the 64 characters as the bearer,
without the file's trailing newline:

```http
Authorization: Bearer <contents of C:\ProgramData\Summrise\token>
```

## 8. When it does not work

| What you see | What it means |
|---|---|
| `302` to `…cloudflareaccess.com` | The request hit a Cloudflare Access application before the endpoint. `/mcp/*` must be its own application — a bypass, or a service token whose two `CF-Access-*` headers the client sends — never the operator application. |
| `401` | The bearer was not accepted: the header is missing or is not a bearer, the token is unknown, revoked, expired, or was created for another device, or the grant expired. Check the id in the path, and create a new token if this one was revoked or expired. |
| `403`, JSON-RPC `-32001`, "tool not permitted by grant" | The tool is outside the token's (or grant's) set. A token's tools and destructive consent are fixed at creation: create a token that includes the tool. |
| `404`, `no_such_device` | The id in the path is not a registered device (for a valid grant: the device was removed). Copy the id from the card again. |
| `503` with `retry_after_ms` | The device has no open channel or its queue is full — offline, between reconnects, or busy. Retry after that many milliseconds; the card's **Channel** row says whether it is back. |
| The card says **offline** right after enrolment | The console link is built at boot: restart `SummriseLaunch` (the commands in step 2). |
| The card stays **offline** after the restart | The device cannot reach the console over outbound HTTPS, or enrolment did not complete. **Last seen** is the last contact, and the retry sentence is one attempt behind, because a device cannot report the connection that failed. |
