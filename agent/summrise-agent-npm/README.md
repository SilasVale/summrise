# summrise-agent

Windows device MCP server — terminal (PTY/SSH/serial), memory, system tools
and a web panel, with an Electron desktop shell. Installable as a global npm
package; the `summrise` CLI manages setup, updates and the tunnel.

## Install (fresh device)

```powershell
npm i -g https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise setup --reg-key <key-from-console>   # --reg-key is optional (local mode works without it)
```

`setup` installs to the registry-configured directory
(`HKLM\SOFTWARE\Summrise\Agent\InstallDir`), registers the boot-start
`SummriseAgent` scheduled task as SYSTEM (no execution-time limit,
restart-on-failure ×8, 5-min repetition watchdog) and starts it. With a
registration key the device registers itself with a Summrise Gate console.

## Update (one click)

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

## CLI commands

```
summrise <setup|status|start|stop|restart|update|uninstall|run|tunnel>
```

## Features

- **OSC 633 shell integration** (VS Code approach): PowerShell prompts and
  command boundaries arrive as invisible sequences — clean display, exit codes.
- **56 MCP tools**: terminal (27: PTY/SSH/serial, history with exit codes, SFTP,
  saved connections, secrets, background jobs), system (9), memory (6),
  mcp-client (4), monitor (4), playwright (2), runs (2), update (agent_update),
  design (page_view). Measured from `agent/spec-tools.json`, which
  `cargo test spec_snapshot` regenerates from the live registry.
- **Electron desktop shell**: tray with live agent status, native menu,
  CDP :9333 for AI-driven UI.
- **Memory plugin**: device-local knowledge base with multi-word search and
  compaction.

## License

MIT — see the repository LICENSE.
