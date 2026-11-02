# Hello

A reference plugin for the Summrise capability hub: a small MCP server in Windows
PowerShell 5.1, with no dependencies and no external commands. The hub starts it as
a child process and speaks newline-delimited JSON-RPC 2.0 over stdio.

## Run it

From this directory, pipe the four-line smoke test into the server:

```powershell
$ask = @'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"greet","arguments":{"name":"Ada"}}}
'@
$ask | powershell -NoProfile -ExecutionPolicy Bypass -File plugin.ps1
```

Three JSON lines come back (initialize, tools/list, tools/call); the last one's
content says `hi, Ada!`. Diagnostics go to stderr, and EOF on stdin ends the process.

## Tools

| Tool | Arguments | Returns |
| --- | --- | --- |
| `hello.greet` | optional string `name` | `hi, name!`; `name` defaults to world |
| `hello.echo` | required string `text` | `text`, unchanged |

Inside the plugin the tools are `greet` and `echo`; the hub adds the `hello`
namespace, so a device advertises `hello.greet` and `hello.echo`. Both are read-only.

`HELLO_GREETING` is the manifest's env example; unset, the greeting is `hi`.

## Install

Point `plugins.add` at this directory (an absolute path). The manifest is validated
before anything spawns. **A plugin runs with this machine's permissions; install
only code you trust.**
