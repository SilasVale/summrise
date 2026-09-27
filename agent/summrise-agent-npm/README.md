# summrise-agent

Windows device MCP server — terminal (PTY/SSH/serial), memory, system tools
and a web panel, with an Electron desktop shell. Installable as a global npm
package; the `summrise` CLI manages setup, updates and the tunnel.

## Install (fresh device)

**Two ways, and the first needs neither Node nor npm.**

### One file (a clean Windows machine)

Download and run **[SummriseAgent-Setup.exe](https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe)**.
It is self-contained — the release's own package is embedded, so it needs no network for the agent itself — and it
elevates itself, writes the registry keys, registers both scheduled tasks and starts the agent. If the machine has no
Node >= 18 it fetches a portable one; if it has one, it reuses it and leaves it alone.

### Through npm

```powershell
npm i -g https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise setup --reg-key <key-from-console>   # --reg-key is optional (local mode works without it)
```

**Run `setup` from an elevated PowerShell.** The install directory is under `Program Files` and the registry key is
`HKLM`, so a normal shell fails with `EPERM: operation not permitted, mkdir 'C:\Program Files\Summrise\scripts'` —
a stack trace where the useful sentence is "run it as administrator".

**In PowerShell, `summrise` resolves to npm's `.ps1` shim, which the default execution policy blocks**
(*"无法加载文件 …summrise.ps1，因为在此系统上禁止运行脚本"*). **Use `summrise.cmd <command>`**, which bypasses the
policy and changes nothing on the machine — or `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, once.

The device's name is written to `etc\summrise-agent.hostname` and defaults to **this machine's `%COMPUTERNAME%`**
(lower-cased, under `.agent.saisi.online`). `setup --hostname <name>` overrides it; the name is what the console lists
the device as and what its tunnel is called.

`setup` installs to the registry-configured directory
(`HKLM\SOFTWARE\Summrise\Agent\InstallDir`), registers the boot-start
`SummriseAgent` scheduled task as SYSTEM (no execution-time limit,
restart-on-failure ×8, 5-min repetition watchdog) and starts it. With a
registration key the device registers itself with a Summrise Gate console.

## Update (two steps, in this order)

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
```

**The `--prefix` is not optional on a device that is already set up.** Plain
`npm i -g <url>` installs into npm's DEFAULT global prefix, which is not where
`summrise` lives when the agent runs as SYSTEM. On a real device the two prefixes
disagreed (`summrise` resolved to `D:\Summrise\components\npm-global\summrise.ps1` while
`npm prefix -g` was `C:\WINDOWS\system32\config\systemprofile\AppData\Roaming\npm`):
npm printed success, `summrise update` then ran the OLD CLI from the other prefix and
staged the OLD exe, and the device quietly stayed on its previous release with no
error anywhere. `Split-Path (Get-Command summrise).Source` asks the machine where
`summrise` actually is, so the install lands where the running CLI will find it.

That applies to UPDATE only. A fresh install has no `summrise` to ask, and the
command above this section is correct as written.

Verify by EFFECT, not by exit code: after the update, `/api/status` must report
the new release AND `etc\.summrise-release` must equal it. The exe's mtime is what
caught the silent case last time.

`update` stages the new exe, then swaps it via a WMI-launched
script (survives the CLI and the agent dying): stop task → kill agent
tree → copy with retry → restart task. The terminal connection
drops ~10 s; reconnect afterwards. Even a failed copy restarts the task —
the device is never left dark.

## Gateway and tunnel

A device is useful locally with no cloud at all. Pointing it at a Summrise Gate console adds remote access, and the
**Gateway** card in the panel is where that happens: the console URL, an optional registration key, and **Save &
connect**.

The tunnel is provisioned **through the Cloudflare API**, which matters because the alternative does not work on a
service: `cloudflared tunnel login` is INTERACTIVE — a browser, a human and a zone — so a device running as SYSTEM can
never produce the `cert.pem` that `cloudflared tunnel create` and `cloudflared tunnel route dns` require. Every one of
those steps now goes through the API instead, including finding a tunnel that already exists (the API answers
`1013 You already have a tunnel with this name`, which is success, not failure).

If a connect attempt fails, the panel's message carries the API's own reply, and the agent's log at
`<DataDir>\logs\agent.log` carries more — `<DataDir>` is `HKLM\SOFTWARE\Summrise\Agent\DataDir`.

## CLI commands

```
summrise <setup|status|start|stop|restart|update|uninstall|run|tunnel>
```

## Features

- **OSC 633 shell integration** (VS Code approach): PowerShell prompts and
  command boundaries arrive as invisible sequences — clean display, exit codes.
- **MCP tools** for the terminal (PTY/SSH/serial, history with exit codes, SFTP,
  saved connections, secrets, background jobs), the system, memory, the browser,
  monitors, runs and updates. **Count them rather than trusting this file:**

  ```bash
  grep -oE '"name": *"[a-z_]+"' agent/spec-tools.json | sed 's/.*"\(.*\)"/\1/' | sort -u | wc -l
  ```

  (It answers **57** today. This section used to state a number and a per-plugin
  breakdown, and both had gone stale — a hand-written count is a claim nothing
  re-checks, and the command above is one line.) `agent/spec-tools.json` is
  regenerated from the live registry by `cargo test spec_snapshot`.
- **Electron desktop shell**: tray with live agent status, native menu,
  CDP :9333 for AI-driven UI.
- **Memory plugin**: device-local knowledge base with multi-word search and
  compaction.

## License

MIT — see the repository LICENSE.
