# Deploying the console

> **Status: verified, 2026-11-02 — `0.3.232` deployed to `DESKTOP-9VH92BJ` with this runbook.**
> The digest matched, the script backed up `0.3.231`, `/health` reported `0.3.232` after the restart,
> the served page was the committed `dist/index.html` byte for byte (782,331 B), and the real-device
> terminal check answered `REAL_TERMINAL_OK`. The console is the operator's control plane; a deploy
> restarts it.

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
| operator page | `/` — the React page; `/next` is the legacy page and is not the operator surface |

## Before you deploy

- **There is no approval queue to check.** A deploy restarts the console, so a call that is in
  flight fails once and the operator retries it; nothing is waiting on a person.
- Device channels reconnect on their own; no device needs to be touched.
- The **aggregate MCP goes through the console**, so restarting it drops the AI's own MCP connection
  for a few seconds. Observed on the first run: one `fetch failed` mid-deploy. The deploy script does
  not depend on MCP, so it completes regardless.

## The procedure

1. **Stage** (Linux, once per version):

   ```bash
   SUMMRISE_BUILD_VERSION=0.3.232 \
     scripts/stage-console-deploy.bash 0.3.232 --mirror-dir /tmp/canary-mirror
   ```

   It builds with `cargo xwin`, refuses a binary whose bytes do not contain the version string,
   copies it and `deploy-console-<version>.cmd` to the mirror directory, and prints the binary's
   size and sha256. The `.cmd` bakes in that digest.

2. **Run it on the host**:

   ```bat
   curl.exe -f -sS -o C:\Windows\Temp\deploy.cmd http://100.85.163.159:18100/deploy-console-0.3.232.cmd
   cmd /c C:\Windows\Temp\deploy.cmd
   ```

   The script downloads, verifies SHA-256 against the staged digest, reads the live version from
   `/health`, **stops the task**, backs up to `summrise-console.exe.bak-<previous>`, replaces, starts
   the task, waits, and prints `/health`. It aborts on a hash mismatch before touching the live
   binary.

3. **Verify**:

   ```bash
   curl -sS http://100.74.250.98:19000/health
   # {"status":"ok","devices":3,...,"version":"0.3.232"}

   curl -sS http://100.74.250.98:19000/ -o /tmp/live.html
   wc -c < /tmp/live.html
   stat -c %s device/console/web/dist/index.html   # must be the same number
   ```

   `console.log` has the `listening on http://0.0.0.0:19000` line. The page is the committed
   `dist/index.html`, not a stale embedded copy: a mismatch means the binary and the built page came
   from different trees.

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
device=DESKTOP-9VH92BJ id8=4c718d6e online=True
terminal_created=session-9
marker_seen=True bytes=542 cursor=542
REAL_TERMINAL_OK
```

It never prints the operator token, the session token or the command output.

**The page, by hand.** Open `/` in a browser on the host and sign in with the operator token. The
React page keeps its bearer **in memory**, not in a cookie or storage; a reload asks for the token
again. Do not quote the token into a shell, an MCP call or this document.

Measured on `0.3.232`:

- Files listed the real root (48 entries) and opened `console-transfer.txt` as
  `transferred through the console`.
- Screen refused with `screen.capture: the per-user helper has not registered this boot` and said
  which session the helper needs; that host's desktop session is disconnected, so the refusal is
  correct.
- Activity → Open in Runs and `#run/<device>/<run>` both opened the same real run record.

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

## Measured

| step | 0.3.232 |
|---|---|
| binary | 6,971,392 B, sha256 `491d7f63…32153d8e` |
| page | 782,331 B |
| stop → `/health` | ~20 s (the task's own restart) |
| after | version `0.3.232`, 3 devices, 3 online; `REAL_TERMINAL_OK` |

The directory accumulates `summrise-console.exe.bak-0.3.x` files; `docs/RELEASE.md` asks for two.
Keep the last two and delete the rest as part of a deploy.
