# Quick start: an installed device to a first call

The short path: install, enrol, open the console, connect a client, make a call, add a plugin. The
long forms are [`CONNECT-MCP.md`](CONNECT-MCP.md) (the endpoint, tokens and grants) and
[`PLUGINS.md`](PLUGINS.md) (the plugin contract). The product statement is
[`PRODUCT.md`](PRODUCT.md).

## 1. Install

Install from the release; the assets, the silent flags and the uninstall are in
[`README.md`](../README.md#install-from-a-release):

```bat
SummriseSetup-<version>.exe /S /D=C:\SummriseInstall
```

The device's own panel is loopback only and has no login — if you can read it, you are on the box.
The compiled default port is **7737**, but the installer passes the port the publish baked in (the
canary builds so far use **18090**), so `http://127.0.0.1:7737` is expected to refuse on those
installs. The service is up when `curl http://127.0.0.1:18090/api/health` answers; from another
machine neither port is reachable, and the console is the remote surface.

## 2. Enrol the machine

In the console, **Home → Add a machine**. It mints a one-time code (15 minutes) and prints the
command that spends it. Either path spends the code once:

- **At the machine:** in the panel, open **Console link → Enrol this machine**, paste the console URL
  and the code, and press **Enroll**. Tick **Replace an existing enrollment** only to rotate this
  machine's token. A panel enrollment on a supervised install asks the launcher to restart the app;
  the page says so.
- **From an elevated command line:**

  ```bat
  summrise-device.exe --enroll <console-url> <code> --data C:\ProgramData\Summrise
  ```

  Add `--force` to replace an existing registration, and `--allow-plaintext` only for a console on a
  trusted plaintext tunnel. After a command-line enrollment, restart the service yourself:

  ```bat
  sc stop SummriseLaunch
  sc start SummriseLaunch
  ```

The app builds its console link at boot. The machine appears in Home's fleet, online, when the
channel is up.

## 3. Open the console

Sign in with the operator token (the page keeps its bearer in memory; a reload asks for it again).
Home is the fleet until a machine is chosen: one row per machine, its build and when it was last
seen. Choose the machine; Home becomes the hub.

The hub screen states the facts in order: **Running** (or **Offline**) with the build, today's
timeline for this machine, and the tool counts. The one action is **Connect a client** (the endpoint
lives in that sheet, nowhere else). **All activity** is the full record.

## 4. Connect a client

**Connect a client** opens the sheet with three sections: **Endpoint**, **Clients**, **Tokens**.

1. **Endpoint** gives this machine's MCP URL, with Copy:

   ```http
   POST https://console.saisi.online/mcp/<device-id>
   Authorization: Bearer srm_<64 lowercase hex characters>
   ```

   The path is the device id (64 lowercase hex characters); the console knows it, and the endpoint
   shown in the sheet is the one to copy.

2. **Tokens → Create token.** Name it, choose the tools it names, and decide whether destructive
   tools are allowed (unticked means a destructive call is refused immediately, and the refusal names
   the consent it needs). The value is `srm_` plus 64 lowercase hex characters, shown once and stored
   only as a hash: neither the list nor the audit can produce it again. It is permanent by default;
   the **Expires** select gives it 30, 90 or 365 days instead.

3. **Clients** offers DSH / Claude / Codex / Cursor configurations with `<token>` as the placeholder.
   Copy the endpoint and the token into the client you use. A fixed DeepSeek Harness block is in
   [`CONNECT-MCP.md#a-fixed-deepseek-harness-configuration`](CONNECT-MCP.md#a-fixed-deepseek-harness-configuration).

A grant is the short-lived alternative to a token: one device, named tools, an expiry. The sheet does
not mint grants; [`CONNECT-MCP.md#mint-a-grant`](CONNECT-MCP.md#mint-a-grant) does.

## 5. Make the first call

Any streamable-HTTP MCP client takes the endpoint and the bearer. With the token and the endpoint in
the shell:

```bash
TOKEN='srm_<64 lowercase hex characters>'
ENDPOINT='https://console.saisi.online/mcp/<device-id>'

curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

`result.tools` is the token's tool set, built-ins and plugin tools together: a tool you did not
select is absent, not refused. Then drive a real shell:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"terminal.create","arguments":{"kind":"pty"}}}'
```

The answer carries the session id. Write to it, then read the output:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"terminal.write","arguments":{"id":"<session-id>","data":"echo hello\r"}}}'
```

`terminal.read` with `{"id":"<session-id>","cursor":0}` returns the output. The call is in
**Activity** with its tool, token, device and result. Open **Device → Terminal** to see the same
session and take the keyboard back.

You can also skip the client for a first look: **Device → Tools** lists every tool the machine
advertises with its schema. To call one, mint a token for it in **Connect → Tokens** and use a client;
the console does not run tools from the catalogue.

## 6. Add a plugin

A plugin adds a capability the built-ins do not have. It is an MCP server installed from a directory
on the device; its tools appear behind the same endpoint, under the same token, in the same record.

1. From the repository root, copy the reference to the Windows machine, for example:

   ```powershell
   New-Item -ItemType Directory -Force C:\SummrisePlugin | Out-Null
   Copy-Item -Recurse examples\plugins\hello C:\SummrisePlugin\hello
   ```

2. In the console, **Device → Tools → Plugins → Add plugin**, paste `C:\SummrisePlugin\hello`. The path field calls
   `plugins.inspect`, which reads the manifest and starts nothing; **Add and run** stays disabled
   until the path is valid. The trust sentence is above the button:

   > This runs the plugin's code with this machine's privileges. Install only code you trust.

3. The row shows **Running**, its namespace and its tool count. The same page's catalogue lists
   `hello.greet` with its schema; a client with a token that carries it calls it, and Activity records
   the call.

**Remove** forgets the plugin and stops its process; it never deletes `C:\SummrisePlugin\hello`. The
full contract is [`PLUGINS.md`](PLUGINS.md). On the console host,
`scripts/real-plugin-check.ps1` runs the same path end to end and prints `REAL_PLUGIN_OK`.

## 7. Revoke the token

**Connect a client → Tokens**, then **Revoke** on the row. The next request with that token is `401`,
and the value is never shown again. Create a new token and update the client's configuration if it
should keep working. This is the brake: the token names the tools and the destructive consent, and
revocation is immediate for new calls.

## 8. Local only: no console

A device is usable by the machine it runs on with no console at all. The install token at `<data>/token`
— `C:\ProgramData\Summrise\token` on a default Windows install — is the local owner credential:
reading that file (`Get-Content C:\ProgramData\Summrise\token`, as an administrator) is what proves
you own the box, and a call bearing it is allowed outright, including destructive tools, with no grant
and no approval prompt. The file is protected to the service account and administrators; treat it as
this machine's password.

The panel's **Use this machine** section shows the loopback endpoint
(`http://127.0.0.1:18080/mcp`), the token file's path and read command, and a client configuration with
the value left out — the page is served to anyone on loopback, so it does not print the secret. Send
the 64 characters as the bearer, without the file's trailing newline:

```http
Authorization: Bearer <contents of C:\ProgramData\Summrise\token>
```

## 9. When it does not work

| What you see | What it means |
|---|---|
| `302` to `…cloudflareaccess.com` | The request hit a Cloudflare Access application before the endpoint. `/mcp/*` must be its own application — a bypass, or a service token whose two `CF-Access-*` headers the client sends — never the operator application. |
| `401` | The bearer was not accepted: the header is missing or is not a bearer, the token is unknown, revoked, expired, or was created for another device, or the grant expired. Check the endpoint's device id, and create a new token if this one was revoked or expired. |
| `403`, JSON-RPC `-32001`, "tool not permitted by grant" | The tool is outside the token's (or grant's) set. A token's tools and destructive consent are fixed at creation: create a token that includes the tool. |
| `404`, `no_such_device` | The id in the endpoint is not a registered device (for a valid grant: the device was removed). Copy the endpoint from Connect again. |
| `503` with `retry_after_ms` | The device has no open channel or its queue is full — offline, between reconnects, or busy. Retry after that many milliseconds; the machine's row in Home says whether it is back. |
| The machine says **offline** right after enrolment | The console link is built at boot: restart `SummriseLaunch` (the commands in step 2). |
| The machine stays **offline** after the restart | The device cannot reach the console over outbound HTTPS, or enrolment did not complete. **Last seen** is the last contact, and the retry sentence is one attempt behind, because a device cannot report the connection that failed. |
| `http://<machine>:7737` does not open | The device binds `127.0.0.1` only, and the installer passes the publish's port (`18090` for these builds), not the compiled default 7737. Open the console; the panel is only for someone on the box. |
| No tray icon after an upgrade | The installer kills the old `summrise-shell.exe` and starts the new one in every **active** session. A locked or disconnected session (or a failed start) leaves no icon until the next sign-in, which the Run entry covers, or until `summrise-launch.exe --start-shell --install <install>` runs in an interactive session. |
| `PLUGIN_TIMEOUT` from a plugin tool | The plugin did not answer inside 120 s. The call may still be running; read the plugin's log in **Tools → Plugins → Logs**, then retry. |
| Plugins says the machine needs a newer Summrise | The device build has no plugin host. The endpoint and the built-ins still work; update the device to add plugins. |
