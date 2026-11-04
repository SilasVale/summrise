# Deploying the console

> **Status: verified, 2026-11-02 — `0.3.251` deployed to `DESKTOP-9VH92BJ` with this runbook.** The
> canary device was upgraded to the same build through a signed release document. `/health` named it,
> the served page was the committed `dist/index.html` byte for byte, and the real terminal, plugin and
> live-catalogue checks passed. `0.3.246` is kept below as the previous baseline.

## The interim path (measured 2026-11-04)

**The procedure below cannot run on this box as written.** Measured today: `cargo-xwin` is not
installed, nothing serves `100.85.163.159:18100` (`/tmp/canary-mirror` still holds installer images),
and the console builds shipped from `0.3.271-ux17` through `0.3.271-ux31` were made for
`x86_64-pc-windows-gnu`, not `x86_64-pc-windows-msvc`. Restore the tool and the mirror and the runbook
applies again unchanged.

Until then, a deploy is three steps on this side and one on the host:

```bash
cd device
SUMMRISE_BUILD_VERSION=<version> cargo build --release -p summrise-console --target x86_64-pc-windows-gnu
sha256sum target/x86_64-pc-windows-gnu/release/summrise-console.exe
mkdir -p /tmp/console-deploy
cp target/x86_64-pc-windows-gnu/release/summrise-console.exe /tmp/console-deploy/summrise-console-<version>.exe
(cd /tmp/console-deploy && nohup python3 -m http.server 8099 --bind 100.85.163.159 &)
```

On the host, fetch the binary through the device's `runs.exec` and check the digest:

```powershell
$dst='C:\SummriseConsole\summrise-console.exe.new'; if (Test-Path $dst) { Remove-Item $dst -Force }
curl.exe -fsS --retry 2 -o $dst http://100.85.163.159:8099/summrise-console-<version>.exe
' sha=' + (Get-FileHash $dst -Algorithm SHA256).Hash.ToLower()
```

Then swap it from a **detached** process, because the swap stops the task that serves the shell it was
started from: `schtasks /end /tn SummriseConsole`, copy the live binary to
`summrise-console.exe.bak-<previous>`, `Move-Item` the `.new` over it, `schtasks /run`, sleep eight
seconds, and append `curl /health` to `C:\SummriseConsole\swap-<version>.log`. Poll `/health` from
Linux until it names the new version.

**What this path does not do:** it skips `scripts/build-web.sh --check`, the dirty-tree refusal and the
SHA-256 check the `.cmd` performs before touching the live binary. From the repository root, run
`scripts/build-web.sh --check` by hand before the build, and compare the digest the host reports with
the one `sha256sum` printed; nothing here verifies that `HEAD` contains `origin/main`.

## Why the console deploys differently from a device

Three version streams ship from this repository and none of them is deployed by the same act:

| stream | how it reaches a machine |
|---|---|
| device runtime | the installer, or the device's own update check against the mirror |
| **console** | **built here, fetched by the host over the tailnet, swapped by a `.cmd`** |
| console page | React 19, built into one HTML file and embedded in the console binary — no separate deploy |

The console host has **no Rust toolchain**: `cargo`, `rustc` and `git` are absent (measured
2026-10-28). So the binary is built on the Linux box for `x86_64-pc-windows-msvc`, staged on the
canary mirror the operator already serves, and fetched by the host itself. No inbound port is opened
on the host; no bytes pass through an AI context.

## What is where

| thing | value |
|---|---|
| host task | `SummriseConsole` (SYSTEM), action `C:\SummriseConsole\run-console.cmd` |
| live binary | `C:\SummriseConsole\summrise-console.exe` |
| log | `C:\SummriseConsole\console.log`, previous run in `console.log.1` (the wrapper rotates) |
| operator token | `C:\SummriseConsole\operator.token` — read by the verification script, never printed |
| mirror directory | `/tmp/canary-mirror` on the Linux box |
| mirror address the host uses | `http://100.85.163.159:18100` (tailnet) |
| version source | `SUMMRISE_BUILD_VERSION` at build time (`device/console/build.rs`); without it the binary reports `CARGO_PKG_VERSION` |
| operator page | `/` — the React page; `/next` is the legacy page and only a `code` query goes there |

## Before you deploy

- **There is no approval queue to check.** A deploy restarts the console, so a call that is in flight
  fails once and the operator retries it; nothing is waiting on a person.
- Device channels reconnect on their own; no device needs to be touched.
- The **aggregate MCP goes through the console**, so restarting it drops the AI's own MCP connection
  for a few seconds. A deploy may surface one `fetch failed`; the deploy script does not depend on
  MCP, so it completes regardless.
- **Stage checks provenance first.** It refuses a dirty worktree, a `HEAD` that does not contain the
  fetched `origin/main`, and a page that does not match its source (`scripts/build-web.sh --check`).
  Build from a clean tree that contains current `main`.
- **The console may ship before the device half.** The page degrades on an older device: no
  `plugins.*` in the live catalogue means Plugins states that the machine needs a newer Summrise and
  the Add button is not shown. The endpoint and built-ins are unchanged. Acceptance 12 in
  `docs/design/capability-hub-2026-11-02.md` is the check.
- **The page is new.** The five destinations, Home, Capabilities, Plugins, Connect and the Device tabs
  are a page change, so the deploy is the first place the whole shell is read against a real device.
  Do not deploy a device feature the device build does not have.

## The procedure

Replace `<version>` with the release being deployed in every command below. The staging script
accepts digits and dots only.

1. **Stage** (Linux, once per version, from a clean tree):

   ```bash
   SUMMRISE_BUILD_VERSION=<version> \
     scripts/stage-console-deploy.bash <version> --mirror-dir /tmp/canary-mirror
   ```

   Before the Rust build it runs `scripts/build-web.sh --check`; a dirty or stale page aborts here
   instead of shipping. It then builds with `cargo xwin`, refuses a binary whose bytes do not contain
   the version string, copies it and `deploy-console-<version>.cmd` to the mirror directory, publishes
   `real-device-check.ps1` beside them, and prints the binary's size and sha256. The `.cmd` bakes in
   that digest.

2. **Run it on the host**:

   ```bat
   curl.exe -f -sS -o C:\Windows\Temp\deploy-<version>.cmd http://100.85.163.159:18100/deploy-console-<version>.cmd
   cmd /c C:\Windows\Temp\deploy-<version>.cmd
   ```

   The script downloads, verifies SHA-256 against the staged digest, reads the live version from
   `/health`, **stops the task**, backs up to `summrise-console.exe.bak-<previous>`, replaces, starts
   the task, waits, and prints `/health`. It aborts on a hash mismatch before touching the live
   binary.

3. **Verify**:

   ```bash
   curl -sS http://100.74.250.98:19000/health
   # {"status":"ok","devices":<n>,...,"version":"<version>"}

   curl -sS http://100.74.250.98:19000/ -o /tmp/live.html
   wc -c < /tmp/live.html
   stat -c %s device/console/web/dist/index.html   # must be the same number

   curl -s -D - -o /dev/null 'http://100.74.250.98:19000/?v=<version digits>' | head -1
   # HTTP/1.1 200 OK — only a `code` query goes to /next
   ```

   `console.log` has the `listening on http://0.0.0.0:19000` line. The page is the committed
   `dist/index.html`, not a stale embedded copy: a mismatch means the binary and the built page came
   from different trees. Record the version, the served page's byte count and the binary's sha256 in
   the Measured table; do not copy the previous version's numbers.

## Real-device checks after a deploy

The browser gate proves the page's shape against an echo; it runs in CI (`npm run gate:browser`).
The real device needs the other half.

**Terminal, through the console's own API.** The stage step publishes `real-device-check.ps1` next
to the binary. On the console host:

```bat
curl.exe -f -sS -o C:\Windows\Temp\real-device-check.ps1 http://100.85.163.159:18100/real-device-check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Windows\Temp\real-device-check.ps1
```

`scripts/real-device-check.ps1` reads the operator token file, mints a session through `/auth`,
picks the first online device from `/fleet`, opens a pty through `/mcp`, writes a unique
`echo <marker>`, reads it back, closes the pty, and prints:

```
device=<name> id8=<8 hex> online=True
terminal_created=session-<n>
marker_seen=True bytes=<n> cursor=<n>
REAL_TERMINAL_OK
```

It never prints the operator token, the session token or the command output.

**Plugins, when the device carries the plugin host.** Stage the example on the device host at
`C:\SummrisePlugin\hello` (copy `examples/plugins/hello` there). The staging step publishes the
terminal check only, so copy the plugin check to the mirror first:

```bash
cp scripts/real-plugin-check.ps1 /tmp/canary-mirror/
```

Then, on the console host:

```bat
curl.exe -f -sS -o C:\Windows\Temp\real-plugin-check.ps1 http://100.85.163.159:18100/real-plugin-check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Windows\Temp\real-plugin-check.ps1
```

It adds the plugin through the deployed console,
lists it, calls `hello.greet`, reads its log, removes it, proves the tool disappeared, and prints
`REAL_PLUGIN_OK`. It reads the operator token from its file and never prints it. If the device build
does not carry the plugin host yet, record the deferral and verify the old-build state on Plugins
instead: the page says the machine needs a newer Summrise and shows no Add button. Do not report a
plugin success the device cannot produce.

**The page, by hand.** Open `/` in a browser on the host and sign in with the operator token. The
React page keeps its bearer **in memory**, not in a cookie or storage; a reload asks for the token
again. Do not quote the token into a shell, an MCP call or this document.

Read these against a real machine, in the order a first visit takes them:

- **Home** with no machine chosen: the fleet, one row per machine, **Add a machine** as the only
  accent. With a machine chosen: the status line, the endpoint with Copy, the counts line, the last
  three calls, and the two actions **Connect a client** / **Add a capability**.
- **Capabilities**: the device's live tools grouped by capability, built-ins first, plugin tools with
  their source badge; a tool sheet shows the schema, annotations and **Try it**.
- **Plugins**: the empty state, or the running `hello` row with its tool count; **Add plugin** opens
  the sheet, the trust sentence is above the button, and a bad path is refused before anything runs.
- **Activity**: the record, one row per call, the device's name rather than its id, and the token
  that made the call.
- **Device**: Terminal · Screen · Files · Runs · Machine in one workspace; the header never loses the
  machine, and Terminal is the default tab.
- **Connect**: Endpoint / Clients / Tokens in one sheet. The token is shown once, and closing the
  sheet drops it.
- **The five destinations are the only rail items.** There is no status strip and no second machine
  picker inside Device.

## Rollback

The previous binary is beside the live one. Rollback is the same three steps with the backup:

```bat
schtasks /end /tn SummriseConsole
taskkill /F /IM summrise-console.exe
copy /Y C:\SummriseConsole\summrise-console.exe.bak-<previous> C:\SummriseConsole\summrise-console.exe
schtasks /run /tn SummriseConsole
```

Then the same `/health` check; the version line must name the backup's version. Rollback is a console
rollback only: a device already running the new plugin host keeps it, and the console's page must
still speak to it. If the page and the device disagree, roll the console back first, then decide the
device half separately (it has its own backup and update document).

## What the script deliberately does not do

- It does not leave the task stopped: a stopped task is a dead console.
- It does not deploy a binary whose version string is not in the bytes — a build that ignored
  `SUMMRISE_BUILD_VERSION` would report yesterday's release.
- It does not copy the binary through an MCP message.
- It does not deploy the device runtime. The plugin host is a device build; the console half degrades
  when the device does not have it.

## Measured

**Last verified: `0.3.246`, 2026-11-02.** Kept as the baseline for comparison; these are not this
build's numbers.

| step | 0.3.246 |
|---|---|
| binary | 6,976,000 B, sha256 `fd83ead5…ffa27d` |
| page | 786,744 B |
| stop → `/health` | ~20 s (the task's own restart) |
| after | version `0.3.246`, 3 devices, 3 online; `REAL_TERMINAL_OK` |

**Last verified: `0.3.251`, 2026-11-02.**

| step | 0.3.251 |
|---|---|
| binary | 7,130,112 B, sha256 `ffb92bb5…f24122` |
| page (`dist/index.html`) | 851,346 B |
| stop → `/health` | ~20 s (the task's own restart; uptime was 17 s at the first answer) |
| `/health` after restart | `{"status":"ok","devices":3,"online":2,"version":"0.3.251"}` |
| real terminal check | `REAL_TERMINAL_OK` (DESKTOP-9VH92BJ, pty marker) |
| real plugin check | `REAL_PLUGIN_OK` (add → 48 tools, `hello.greet` answered, 11 log lines, removed, no `plugin.ps1` child; after `host.restart` the registered plugin came back `running` with 2 tools and exactly one child, and removing it left zero) |
| real console check | `REAL_CONSOLE_OK` (live `/fleet/:id/tools` = 46 tools incl. `plugins.list`; a `hello.greet` token minted and revoked) |
| page by hand | not opened on the host; the served page is the committed `dist/index.html` byte for byte (851,346 B), the browser gate passed against that bundle, and the gallery was read at 360/390/800/1280 in both themes |
| rollback binary kept | `summrise-console.exe.bak-0.3.250` and `.bak-0.3.251`; the directory was trimmed from 93 backups to these two |

**Device half, same session.** `summrise-device` 0.3.218 → **0.3.251** through the signed
`release-0.3.251.json` (artifact sha256 `919aa0f8…8a0f`; the launcher kept 0.3.218 as `previous`).
The example plugin was added, called, logged and removed through the console; the registry ended
empty and the plugin process was gone.

The directory accumulates `summrise-console.exe.bak-0.3.x` files; `docs/RELEASE.md` asks for two.
Keep the last two and delete the rest as part of a deploy.
