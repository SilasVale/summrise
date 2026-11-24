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

The tray finds the port without a constant: the device writes `<data>\device.port` (SYSTEM-only)
**and** `%ProgramData%\SummriseHelper\device.port`, which inherits ProgramData's user-read ACL; the
shell reads the data file, then the user-readable copy, then the default. A shell that can read
neither dials 7737 — the symptom that looks like "the tray is dead and 7737 does not open".

## 2. Enrol the machine

In the console, **Machines → Add a machine**. It mints a one-time code (15 minutes) and prints the
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

The app builds its console link at boot. The machine appears online in **Machines** when the channel
is up.

## 3. Open the console

Sign in with the operator token. The browser keeps an **HttpOnly session cookie**; a reload preserves
the session while it is valid.

The global destinations are **Overview**, **Machines**, **Activity** and **Access**. Overview shows
the fleet at a glance; Machines lists each machine, its build and when it was last seen. Choose a
machine to open **Device → Overview**. Its workspace has **Overview**, **Terminal**, **Screen**,
**Files**, **Runs**, **Tools**, **Memory** and **Machine** sections.

Device Overview shows the machine's state, build, activity and tool counts. **Connect a client**
opens the quick sheet from Overview, Device or the account menu; **All activity** opens the full
record.

## 4. Connect a client

**Access** and the **Connect a client** quick sheet both have three sections: **Endpoint**, **Clients**,
**Tokens**.

1. **Endpoint** gives this machine's MCP URL, with Copy:

   ```http
   POST https://console.saisi.online/mcp/<device-id>
   Authorization: Bearer srm_<64 lowercase hex characters>
   ```

   The path is the device id (64 lowercase hex characters); the console knows it, and the endpoint
   shown in the sheet is the one to copy.

2. **Tokens → Create token.** Choose **this machine** for the endpoint above. Name it, choose the
   tools it names, and decide whether destructive
   tools are allowed (unticked means a destructive call is refused immediately, and the refusal names
   the consent it needs). The value is `srm_` plus 64 lowercase hex characters, shown once and stored
   only as a hash: neither the list nor the audit can produce it again. It is permanent by default;
   the **Expires** select gives it 30, 90 or 365 days instead. A non-destructive preset is not a strict
   read-only guarantee; inspect tools whose destructive metadata is unknown before granting them.

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

`result.tools` is the token's capability tool set, built-ins and plugin tools together: a tool
you did not select is absent. The protocol control tools `txn.begin`, `txn.commit` and `txn.abort`
are admitted independently of that allowlist. For an inspection-only first call, use `host.facts`
with `arguments: {}` and a token naming just that tool; no shell or transaction is needed. The full
request is in [`CONNECT-MCP.md#a-quick-check`](CONNECT-MCP.md#a-quick-check).

To drive a real shell, the token must explicitly name `terminal.create`, `terminal.write`,
`terminal.read` and `terminal.close`, with destructive consent for **create** and **write**. Close is
not marked destructive, but it still needs tool scope and a working transaction. An inspection-only
token should not gain a shell just to complete this example.

First create the terminal session:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"terminal.create","arguments":{"kind":"pty"}}}'
```

The tool's answer carries the session `id`. Use it as `session` when beginning the unit of work:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"txn.begin","arguments":{"session":"<session-id>","purpose":"Run the first shell command."}}}'
```

Keep the returned `txn` id. Calls naming this terminal session inherit its working transaction:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"terminal.write","arguments":{"id":"<session-id>","data":"echo hello\r"}}}'

curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"terminal.read","arguments":{"id":"<session-id>","cursor":0,"wait_ms":1000}}}'
```

Close the terminal while its transaction is working, then commit the transaction:

```bash
curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"terminal.close","arguments":{"id":"<session-id>"}}}'

curl -sS -X POST "$ENDPOINT" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"txn.commit","arguments":{"txn":"<txn-id>"}}}'
```

If the attempt fails after begin, close the terminal while the transaction is working and send
`txn.abort` instead of commit, using the same endpoint and headers:

```json
{"jsonrpc":"2.0","id":8,"method":"tools/call",
 "params":{"name":"txn.abort","arguments":{"txn":"<txn-id>","reason":"The first shell command did not complete."}}}
```

Commit declares the work done; abort declares that the attempt did not achieve its purpose. Neither
undoes side effects. These terminal sessions and work transactions are separate from MCP's HTTP
transport, which has no transport session.

A client token or grant must begin inside a live terminal session: `txn.begin` without `session` is
refused with `session_required`. A Required tool that names no terminal session, such as `files.read`
or `plugins.add`, must carry the returned id explicitly in its advertised `txn` argument. For example,
while the transaction is working, a token that also names `files.read` can send
`{"path":"notes.txt","txn":"<txn-id>"}` as that tool's arguments. Exempt reads such as `host.facts`,
`runs.list` and `runs.get` need no transaction and do not accept an extra `txn` argument.

The calls appear in **Activity** with their tool, token, device and result. **Device → Terminal**
shows a live session and lets you take the keyboard back.

You can also skip the client for a first look: **Device → Tools** lists every tool the machine
advertises with its schema. To call one, mint a token for it in **Access → Tokens** or **Connect →
Tokens** and use a client; the console does not run tools from the catalogue.

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
   `hello.greet` with its schema. Installing a plugin does not expand an existing token's tool set:
   mint a new device token naming `hello.greet` and the needed `terminal.create` / `terminal.close`
   bootstrap tools, with destructive consent for creating the shell. Use step 5's create → begin
   flow, pass `{"name":"Ada","txn":"<txn-id>"}` to `hello.greet` while the transaction is working, then
   close the session before committing or aborting. Activity records the call. The MCP installation
   example in [`README.md`](../README.md#plugins) also passes `txn` to `plugins.add`.

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
you own the box, and a call bearing it is authorized, including destructive tools, with no grant
and no approval prompt. Authorization does not waive transaction requirements: Required tools still
need a working transaction. The install owner may begin a device-scope transaction with
`txn.begin` and `{"purpose":"Inspect a local file."}`, then run a Required machine call with that
transaction. The file is protected to the service account and administrators; treat it as this
machine's password.

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
| `session_required` | A client token or grant tried to begin without a live terminal session, or called a Required tool without its session-bound transaction. Create an authorized session, begin with `session` and `purpose`, then inherit it on terminal calls or pass its `txn` id to a Required tool that names no session. |
| `txn_ended` | The transaction was already committed or aborted. Begin a new transaction for new work; close the terminal while its transaction is working, before ending it. |
| `404`, `no_such_device` | The id in the endpoint is not a registered device (for a valid grant: the device was removed). Copy the endpoint from Connect again. |
| `503` with `retry_after_ms` | The device has no open channel or its queue is full — offline, between reconnects, or busy. Retry after that many milliseconds; the machine's row in Machines says whether it is back. |
| The machine says **offline** right after command-line enrolment | The console link is built at boot: restart `SummriseLaunch` (the commands in step 2). |
| The machine stays **offline** after the restart | The device cannot reach the console over outbound HTTPS, or enrolment did not complete. **Last seen** is the last contact, and the retry sentence is one attempt behind, because a device cannot report the connection that failed. |
| `http://<machine>:7737` does not open | The device binds `127.0.0.1` only, and the installer passes the publish's port (`18090` for these builds), not the compiled default 7737. Open the console; the panel is only for someone on the box. If the **tray** also points at 7737, the shell could not read its port file: check that `%ProgramData%\SummriseHelper\device.port` exists and is readable by the user. |
| No tray icon after an upgrade | The installer kills the old `summrise-shell.exe` and starts the new one in every **signed-in** session (console, connected RDP, and disconnected RDP whose user is still signed in). If nobody is signed in, the login Run entry starts it at the next sign-in; in a session where it is still missing, run `summrise-launch.exe --start-shell --install <install>`. |
| `PLUGIN_TIMEOUT` from a plugin tool | The plugin did not answer inside 120 s. The call may still be running; read the plugin's log in **Tools → Plugins → Logs**, then retry. |
| Plugins says the machine needs a newer Summrise | The device build has no plugin host. The endpoint and the built-ins still work; update the device to add plugins. |
