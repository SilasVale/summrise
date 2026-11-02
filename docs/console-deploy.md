# Deploying the console

> **Status: verified, 2026-11-02 — `0.3.242` deployed to `DESKTOP-9VH92BJ` with this runbook.**
> The digest matched, the script backed up `0.3.241`, `/health` reported `0.3.242` after the restart,
> the served page was the committed `dist/index.html` byte for byte (784,276 B), `GET /?v=242`
> stayed on the React page (200, no `/next` redirect), and the real-device terminal check answered
> `REAL_TERMINAL_OK`. The console is the operator's control plane; a deploy restarts it.

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

- **There is no approval queue to check.** A deploy restarts the console, so a call that is in
  flight fails once and the operator retries it; nothing is waiting on a person.
- Device channels reconnect on their own; no device needs to be touched.
- The **aggregate MCP goes through the console**, so restarting it drops the AI's own MCP connection
  for a few seconds. Observed on the first run: one `fetch failed` mid-deploy. The deploy script does
  not depend on MCP, so it completes regardless.

## The procedure

1. **Stage** (Linux, once per version):

   ```bash
   SUMMRISE_BUILD_VERSION=0.3.242 \
     scripts/stage-console-deploy.bash 0.3.242 --mirror-dir /tmp/canary-mirror
   ```

   It builds with `cargo xwin`, refuses a binary whose bytes do not contain the version string,
   copies it and `deploy-console-<version>.cmd` to the mirror directory, publishes
   `real-device-check.ps1` beside them, and prints the binary's size and sha256. The `.cmd` bakes in
   that digest.

2. **Run it on the host**:

   ```bat
   curl.exe -f -sS -o C:\Windows\Temp\deploy-242.cmd http://100.85.163.159:18100/deploy-console-0.3.242.cmd
   cmd /c C:\Windows\Temp\deploy-242.cmd
   ```

   The script downloads, verifies SHA-256 against the staged digest, reads the live version from
   `/health`, **stops the task**, backs up to `summrise-console.exe.bak-<previous>`, replaces, starts
   the task, waits, and prints `/health`. It aborts on a hash mismatch before touching the live
   binary.

3. **Verify**:

   ```bash
   curl -sS http://100.74.250.98:19000/health
   # {"status":"ok","devices":3,...,"version":"0.3.242"}

   curl -sS http://100.74.250.98:19000/ -o /tmp/live.html
   wc -c < /tmp/live.html
   stat -c %s device/console/web/dist/index.html   # must be the same number

   curl -s -D - -o /dev/null 'http://100.74.250.98:19000/?v=242' | head -1
   # HTTP/1.1 200 OK — only a `code` query goes to /next
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
terminal_created=session-20
marker_seen=True bytes=542 cursor=542
REAL_TERMINAL_OK
```

It never prints the operator token, the session token or the command output.

**The page, by hand.** Open `/` in a browser on the host and sign in with the operator token. The
React page keeps its bearer **in memory**, not in a cookie or storage; a reload asks for the token
again. Do not quote the token into a shell, an MCP call or this document.

Measured on `0.3.242`:

- **Files** listed the real root (48 entries), opened `console-transfer.txt`, then created
  `summrise-round-check-25`, uploaded a 28 B file, read it, edited it to 27 B, saved it, and the test
  folder was removed from the host.
- **Runs** executed `cmd /c echo ui-run-33` through the form: exit 0, 40 ms, 11 B stdout
  (`ui-run-33`), and the same command appeared in the history. This is the `runs.exec` path the
  Activity and deep-link checks open afterwards.
- **Screen → Desktop** refused with `screen.capture: the per-user helper has not registered this
  boot`; the page says which session the helper needs. That host's desktop session is disconnected,
  so the refusal is correct.
- **Screen → Browser** opened `example.com`, captured 27,119 bytes, filtered the target list from six
  Chrome internals to the two real pages, showed the selected page's URL, and click/type reached the
  page (`document.activeElement` became the input; its value became `hello via ui`).
- **Activity → Runs** and `#run/<device>/<run>` opened the same real run record. `#files`/`#terminal`
  survive a reload and sign-in as surface fragments.
- A hidden browser target pauses live view by design; the frame counter does not advance while
  `document.hidden` is true.

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

| step | 0.3.242 |
|---|---|
| binary | 6,971,904 B, sha256 `354fb34d…68f1c29a` |
| page | 784,276 B |
| stop → `/health` | ~20 s (the task's own restart) |
| after | version `0.3.242`, 3 devices, 3 online; `REAL_TERMINAL_OK` |

The directory accumulates `summrise-console.exe.bak-0.3.x` files; `docs/RELEASE.md` asks for two.
Keep the last two and delete the rest as part of a deploy.
