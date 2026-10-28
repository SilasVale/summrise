# Deploying the console

> **Status: verified, 2026-10-28 — `0.3.153` deployed to `DESKTOP-9VH92BJ` with this runbook.**
> The console is the operator's control plane; a deploy restarts it. Everything below is measured on
> that host, not assumed.

## Why the console deploys differently from a device

Three version streams ship from this repository and none of them is deployed by the same act:

| stream | how it reaches a machine |
|---|---|
| device runtime | the installer, or the device's own update check against the mirror |
| **console** | **built here, fetched by the host over the tailnet, swapped by a `.cmd`** |
| console page | inside the console binary (`maud`, server-rendered) — no separate deploy |

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
| mirror directory | `/tmp/canary-mirror` on the Linux box |
| mirror address the host uses | `http://100.85.163.159:18100` (tailnet) |
| version source | `SUMMRISE_BUILD_VERSION` at build time (`device/console/build.rs`); without it the binary reports `CARGO_PKG_VERSION` |

## Before you deploy

- **A pending approval does not survive.** Held destructive calls live in memory; the restart loses
  one that a person has not answered yet. Check the fleet page's *Waiting for your approval* card
  first, or deploy when nobody is driving.
- Device channels reconnect on their own; no device needs to be touched.
- The **aggregate MCP goes through the console**, so restarting it drops the AI's own MCP connection
  for a few seconds. Observed on the first run: one `fetch failed` mid-deploy. The deploy script does
  not depend on MCP, so it completes regardless.

## The procedure — two commands

1. **Stage** (Linux, once per version):

   ```bash
   scripts/stage-console-deploy.bash 0.3.153
   ```

   It builds with `cargo xwin`, refuses a binary whose bytes do not contain the version string,
   copies it to the mirror, and writes `deploy-console-<version>.cmd` with the digest baked in. It
   prints the next command.

2. **Run it on the host** (the command the script prints):

   ```powershell
   powershell -NoProfile -Command "Invoke-WebRequest -UseBasicParsing http://100.85.163.159:18100/deploy-console-0.3.153.cmd -OutFile C:\Windows\Temp\deploy.cmd; cmd /c C:\Windows\Temp\deploy.cmd"
   ```

   The script: downloads, verifies SHA-256 against the staged digest, reads the live version from
   `/health`, **stops the task**, backs up to `summrise-console.exe.bak-<previous>`, replaces, starts
   the task, waits 12 s and prints `/health`. It aborts on a hash mismatch before touching the live
   binary.

3. **Verify** (from the device or here):

   ```bash
   curl -sS http://100.74.250.98:19000/health
   # {"status":"ok","devices":3,...,"version":"0.3.153"}
   ```

   The page at `/next` answers 200, `console.log` has the `listening on http://0.0.0.0:19000` line,
   and the fleet page's markup carries the current UI (`rail_items=1`, `more_pages=True` were the
   markers for the 0.3.153 change).

## Rollback

The previous binary is beside the live one. Rollback is the same three steps with the backup:

```bat
schtasks /end /tn SummriseConsole
taskkill /F /IM summrise-console.exe
copy /Y C:\SummriseConsole\summrise-console.exe.bak-<previous> C:\SummriseConsole\summrise-console.exe
schtasks /run /tn SummriseConsole
```

Then the same `/health` check; the version line must name the backup's version.

## What the script deliberately does not do

- It does not leave the task stopped: a stopped task is a dead console.
- It does not deploy a binary whose version string is not in the bytes — a build that ignored
  `SUMMRISE_BUILD_VERSION` would report yesterday's release.
- It does not copy the binary through an MCP message.

## The first run's numbers

| step | reading |
|---|---|
| build | 19.37 s incremental (release, `x86_64-pc-windows-msvc`) |
| binary | 5 955 584 B, sha256 `63872992…513c80a0` |
| swap | ~15 s from stop to `/health` |
| after | version `0.3.153`, uptime reset to 33 s, 3 devices, `/next` 200 |
| backup | `summrise-console.exe.bak-0.3.145` |

The directory had also accumulated old backups; `docs/RELEASE.md` asks for two. Keep the last two
and delete the rest as part of a deploy.
