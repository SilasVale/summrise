# Writing a plugin

> **Installing a plugin runs its code with this machine's privileges. There is no sandbox, no
> signature and no registry in v1. Install only code you trust.**

A plugin is an **MCP server**: a program the hub starts as a child process and speaks MCP to over
stdio. Its tools are advertised behind the device's one endpoint as `<namespace>.<tool>`, under the
same token and in the same Activity record as the built-ins. A plugin is for the capability the
built-ins do not have; it is not a second product, a second authorization model or a hosted service.

This guide is for the author who has the hub installed and a directory to put the plugin in. The
reference implementation is [`examples/plugins/hello/`](../examples/plugins/hello/); it has two tools,
no dependencies, and fits in one PowerShell file.

## The directory

A plugin is one directory containing at least:

```
hello/
  plugin.json    the manifest: identity, argv, working directory, environment
  plugin.ps1     the MCP server (any executable or script the manifest names)
```

The directory stays where you keep it. Installing **registers its path**; the hub does not copy the
files, and removing the plugin never deletes them.

## `plugin.json`

UTF-8 JSON, no comments. Unknown keys are refused — a typo is an error, not a silent default. The
manifest is validated before anything is spawned.

| Field | Rule |
|---|---|
| `id` | Required. `[a-z0-9][a-z0-9._-]{2,63}`, 3–64 bytes, unique on the device. |
| `namespace` | Required. `[a-z][a-z0-9-]{1,31}`, 2–32 bytes, unique on the device, and not a built-in capability name. Every tool is advertised under it. |
| `name` | Required. 1–64 characters, for a human. |
| `version` | Required. Exactly `x.y.z`, each component digits. |
| `description` | Optional. One sentence on what the plugin adds. |
| `command` | Required, non-empty array of strings. `command[0]` is the executable; the rest are arguments. |
| `cwd` | Optional. Relative to the plugin directory, no `..`, no absolute or drive-rooted path. Default: the plugin directory. |
| `env` | Optional object of strings. Keys `[A-Za-z_][A-Za-z0-9_]*`; added to the inherited environment. |
| `transport` | Optional. Must be `"stdio"`; absent means `stdio`. This is the only transport in v1. |

`command[0]` must resolve when the plugin is added: an absolute path, a path containing `/` or `\`
(relative to the plugin directory), or a name found on `PATH`. `~` is not expanded.

The reference manifest, exactly as shipped:

```json
{
  "id": "com.example.hello",
  "namespace": "hello",
  "name": "Hello",
  "version": "0.1.0",
  "description": "Two friendly tools, and the reference for writing a plugin.",
  "command": ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "plugin.ps1"],
  "cwd": ".",
  "env": { "HELLO_GREETING": "hi" }
}
```

## The protocol

The hub speaks **MCP `2025-06-18` over stdio**. One JSON-RPC 2.0 message per line, in both
directions. **stdout carries protocol JSON only**; write diagnostics to stderr. A malformed stdout
line is logged and skipped; it does not stop the other tools.

The conversation the hub drives:

| Step | Direction | Meaning |
|---|---|---|
| `initialize` | hub → plugin | Request with `protocolVersion: "2025-06-18"`, `capabilities: {}`, `clientInfo`. Answer with your `protocolVersion`, `capabilities` and `serverInfo`. |
| `notifications/initialized` | hub → plugin | Notification, no answer. |
| `tools/list` | hub → plugin | Return `{ "tools": [...] }`. |
| `tools/call` | hub → plugin | `{ "name": "<tool without the namespace>", "arguments": {...} }`. |
| `ping` | hub → plugin | Health check; answer with an empty result. |

An older protocol version is accepted if the plugin answers `initialize`; the negotiated version is
recorded in the log. A plugin that never answers `initialize` fails its start (10 s) and is not
recorded.

Each tool in `tools/list` is an MCP tool object:

```json
{
  "name": "greet",
  "title": "Greet",
  "description": "Greet someone by name.",
  "inputSchema": {
    "type": "object",
    "properties": { "name": { "type": "string" } },
    "additionalProperties": false
  },
  "annotations": { "readOnlyHint": true }
}
```

`title`, `description`, `inputSchema`, `readOnlyHint`, `destructiveHint`, `idempotentHint` and
`openWorldHint` are passed through to the catalogue. A missing `description` falls back to the tool
name.

### Name mapping

The plugin advertises `greet`; the device advertises `hello.greet` because the manifest's namespace is
`hello`. The `tools/call` the hub sends names `greet` — the plugin never sees the namespace. The audit
and Activity name `hello.greet`, so the call is attributable.

A tool name is 1–64 bytes from `[A-Za-z0-9_.-]`. A name outside that set is refused when the plugin
starts, with the offending name in the error; the post-namespace name must also not collide with a
built-in tool or another running plugin's tool.

### Annotations and destructive defaults

The destructive flag is MCP's default, computed once:

```
!readOnlyHint && destructiveHint.unwrap_or(true)
```

Consequences for an author:

- **Omit both hints and the tool is destructive.** A token without destructive consent cannot call it.
- Set `readOnlyHint: true` for a tool that only observes; that is what `hello.greet` and `hello.echo`
  do.
- Set `destructiveHint: false` with intent; it is a promise about the tool, and the hub still treats a
  missing `readOnlyHint` as change-capable unless `destructiveHint` says otherwise.

The destructive flag decides the token consent, not a plugin-specific permission. There is no second
policy object.

## Lifecycle

The six management tools are ordinary built-in MCP tools, so they appear in `tools/list`, in the
token scope and in Activity with no special case.

| Tool | Input | Output | Destructive |
|---|---|---|---|
| `plugins.list` | `{}` | `[{id, namespace, name, version, description, state, tools, path, last_error}]` | no |
| `plugins.inspect` | `{path}` | `{id, namespace, name, version, description}` — read-only; starts nothing, writes no record | no |
| `plugins.add` | `{path}` | `{plugin, tools}` | yes |
| `plugins.remove` | `{namespace}` | `{plugin}` (state `stopped`) | yes |
| `plugins.restart` | `{namespace}` | `{plugin}` | yes |
| `plugins.logs` | `{namespace, lines?}` | `{namespace, lines: [string]}` | no |

`path` is an absolute path to the plugin directory on the device. `state` is one of `running`,
`starting`, `stopped`, `failed`; `tools` is a number; `last_error` is a string or `null`.

**Add.** The hub canonicalises the path, requires a directory, reads and validates `plugin.json`,
refuses a namespace or id that is taken, refuses a namespace that is a built-in capability, checks
that `command[0]` resolves, then spawns and handshakes. The record is written to
`<data>/plugins.json` only after the handshake and the tool-collision check succeed. A plugin whose
process starts but whose handshake fails comes back `failed` with `last_error`, 0 tools, and no
record; the next boot does not retry it.

**Start and health.**

```
add → validate → spawn → initialize → tools/list → running
                              │ failure                │ ping × 3 failed / crash
                              ▼                        ▼
                           failed ← backoff restart (1s, 2s, 4s, 8s, 16s, 30s cap)
                              │
                              └── manual restart ──────┘
```

- Startup budget: 10 s for spawn + `initialize` + `tools/list`.
- `ping` every 30 s, 5 s timeout. Three consecutive misses restart the plugin.
- After five starts in five minutes the plugin stays `failed` until `plugins.restart`.
- Its tools leave `tools/list` the moment it is not `running`; a call to one is refused rather than
  answered from a stale catalogue.
- One `tools/call` per plugin at a time. A call times out after 120 s with `PLUGIN_TIMEOUT`; the
  plugin is not killed for a slow call unless it stops answering pings.

**Remove.** Forgets the record, stops the process (close stdin, 2 s grace, kill) and never touches
the source directory. Calls to its tools then fail with `PLUGIN_NOT_FOUND`. On device shutdown every
plugin is stopped; no child survives the hub.

**Logs.** The child's stderr and the hub's protocol notes append to
`<data>/logs/plugin-<namespace>.log`; the file rotates at 4 MiB to `.1`. `plugins.logs` returns the
last 200 lines by default (at most 2,000). A malformed stdout line is logged and skipped; a valid
stdout frame is not logged.

## Errors

Every refusal is the product's shape, as the tool's text:

```json
{"code": "PLUGIN_NOT_FOUND", "message": "no plugin named hello", "action": "plugins.list"}
```

| Code | Where | What it means for the author |
|---|---|---|
| `NOT_FOUND` | `plugins.inspect` | The path does not resolve to a directory. Check the absolute path on the device. |
| `NO_MANIFEST` | `plugins.inspect` | The directory has no `plugin.json`. |
| `INVALID_MANIFEST` | `plugins.inspect` | `plugin.json` is not valid JSON or fails a field rule; the message names the field. |
| `PLUGIN_PATH_INVALID` | `plugins.add` | The path is missing, unreadable or not a directory. |
| `PLUGIN_MANIFEST_INVALID` | `plugins.add` | The manifest is missing, unreadable or fails a field rule. |
| `PLUGIN_ALREADY_INSTALLED` | `plugins.add` | The `id` or `namespace` is already registered. Remove the old one, or change the manifest. |
| `PLUGIN_NAMESPACE_RESERVED` | `plugins.add` | The namespace is a built-in capability name. Choose another namespace. |
| `PLUGIN_COMMAND_MISSING` | `plugins.add` | `command[0]` does not resolve from the plugin directory or `PATH`. |
| `PLUGIN_TOOL_NAME_INVALID` | `plugins.add` | A tool name is outside `[A-Za-z0-9_.-]` or longer than 64 bytes; the message names it. |
| `PLUGIN_TOOL_COLLISION` | `plugins.add` | The namespaced tool would shadow a built-in or another running plugin's tool. Nothing is persisted. |
| `PLUGIN_SPAWN_FAILED` | `plugins.add` | The operating system could not start the process. Check the argv and the executable's architecture. |
| `PLUGIN_NOT_FOUND` | `plugins.remove`, `plugins.restart`, `plugins.logs`, calls | No installed plugin has that namespace. `plugins.list` names the ones that do. |
| `PLUGIN_NOT_RUNNING` | `tools/call` | The plugin is `starting`, `stopped` or `failed`. `plugins.restart` or read the log. |
| `PLUGIN_TOOL_NOT_FOUND` | `tools/call` | The plugin does not advertise that tool. Compare with `plugins.list`/`tools/list`. |
| `PLUGIN_TIMEOUT` | `tools/call` | No answer inside 120 s. The call may still be running in the plugin; read `plugins.logs`. |
| `PLUGIN_EXITED` | `tools/call` | The child died during the call. The supervisor restarts it. |
| `PLUGIN_ERROR` | `tools/call` | The plugin answered a JSON-RPC error, or `isError: true`. The message is the plugin's sentence. |
| `PLUGIN_PROTOCOL` / `PLUGIN_IO` | `tools/call` | The answer was not MCP, or the pipe failed. Read `plugins.logs`. |

Collisions are refused **before** the record is written, and a tool collision discovered after the
handshake is rolled back: a refused plugin is not left half-installed. A duplicate `id` or namespace
and a built-in namespace are checked before spawning, so a good plugin is not stopped by a bad
neighbour.

## A five-minute test

Prerequisites: a Windows machine running the hub (the device carries the plugin host), the console
open, and the example copied to a directory on the device, for example
`C:\SummrisePlugin\hello`. The plugin path is a path **on the device**, not on the console host.

1. **Install it.** In the console: **Plugins → Add plugin**, paste the absolute path. The path field
   calls `plugins.inspect` (read-only), so a typo is caught before anything runs: it shows
   `hello · v0.1.0` or the refusal and its action. Press **Add and run**. The trust sentence is above
   the button: *This runs the plugin's code with this machine's privileges. Install only code you
   trust.*

   Or call the same tool through any MCP client:

   ```json
   {"jsonrpc":"2.0","id":1,"method":"tools/call",
    "params":{"name":"plugins.add","arguments":{"path":"C:\\SummrisePlugin\\hello"}}}
   ```

   The answer is `{"plugin": {...state: "running"...}, "tools": 2}`.

2. **List it.** `plugins.list` returns the record, and `tools/list` now contains `hello.greet` and
   `hello.echo` after the built-ins. In **Capabilities**, the plugin's tools carry the `plugin:hello`
   source badge; **Try it** calls them without a client.
3. **Call it.** `hello.greet` with `{"name":"Ada"}` returns `hi, Ada!`. With no argument it returns
   `hi, world!`; `HELLO_GREETING` from the manifest changes the greeting. Activity shows the call
   under `hello.greet` and the token that made it.
4. **Remove it.** Plugins → Remove. The process stops, the tools leave `tools/list`, and
   `C:\SummrisePlugin\hello` is still there.

The same path end to end is `scripts/real-plugin-check.ps1`: it adds the example through the deployed
console, lists the plugin, calls `hello.greet`, reads the log, removes the plugin, proves the tool
disappeared, and prints `REAL_PLUGIN_OK`. It reads the operator token from its file and never prints
it.

The protocol alone can be checked without the hub, by piping four lines into the server:

```powershell
$ask = @'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"greet","arguments":{"name":"Ada"}}}
'@
$ask | powershell -NoProfile -ExecutionPolicy Bypass -File plugin.ps1
```

See [`examples/plugins/hello/README.md`](../examples/plugins/hello/README.md) for the reference's
tools and the same smoke test.

## What v1 is not

- No marketplace, registry or signature; a directory path is the whole contract.
- No git or npm source. Copy or clone the code yourself, then register the directory.
- No sandbox. A plugin runs as the device's user — SYSTEM when the service runs as SYSTEM — with that
  user's filesystem and network.
- No plugin-specific permission UI. The token names tools and destructive consent; that is the runtime
  brake, and every plugin call is in the same record.
- No approval queue. Installing is the trust decision; the token is what limits a call.

These are v1 boundaries, not missing screens. The plan names them in
[`docs/design/capability-hub-2026-11-02.md`](design/capability-hub-2026-11-02.md) §10.
