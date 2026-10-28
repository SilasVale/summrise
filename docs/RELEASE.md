# Releasing, and proving it landed

Three version streams ship from this repository and none of them is deployed by the same act:

| stream | what it is | how it reaches a machine |
|---|---|---|
| **device runtime** | `summrise-launch` + `summrise-device`, the Windows service | the installer, or the device's own update check against the mirror |
| **console** | `summrise-console`, the operator's surface | copied to the console host and restarted |
| **console page** | Rust handlers in `console/src/http/page.rs`, rendered per request | inside the console binary — no separate deploy, no npm |

They share a version number because they are released together, but a *device* release does not redeploy a
*console*, and a console deploy does not touch a device. `verify-release.sh` is the tool that keeps those
facts straight (see "Proving it landed").

## The browser is headless unless someone asks to watch

`browser.*` used to launch Chrome in the operator's session first, headless only as a fallback: the visible
browser has their profile and GPU, and the person at the machine can take it over. For automation that default is
backwards — windows open on their desktop, an unlocked session is required, and any page the tool visits can read
their signed-in cookies.

The order is now the other way round. `SUMMRISE_BROWSER_VISIBLE=1` (any value except `0`) asks for the visible
browser and falls back to headless if the helper cannot start it; without the variable every call is served by the
service's own headless Chrome and nobody's screen changes.

## Catching a window that flashes on an operator's desktop

"The screen blinks" has three plausible authors and only one of them is the product: a console child of a GUI
process (see `CREATE_NO_WINDOW`), a deployment script (a download holds a window open for minutes), and any
other automation on the machine. A sampler answers it in three minutes — a SYSTEM task running `wmic process` in
a loop, one snapshot every ~400 ms, then `findstr` for the suspects:

```bat
for /L %%i in (1,1,120) do (
  wmic process where "name='cmd.exe' or name='schtasks.exe' or name='powershell.exe'" get Name,ProcessId,ParentProcessId,CommandLine /format:csv
  ping -n 1 -w 400 127.0.0.1 >nul
)
```

Measured on DESKTOP-14RJCR8 over three minutes (2026-10-28, 19:00): **no** `schtasks` and **no** `powershell`
process appeared, which is what retired the "the tray registers its reconnect task repeatedly" hypothesis after
the `CREATE_NO_WINDOW` fix. The sampler's own `cmd.exe` dominates the `cmd.exe` rows — filter by command line,
not by name.

## The console's stderr is a file now

The console task used to run the binary with nothing attached, so its own diagnostics went nowhere: the two
lines that explain a missing answer — `device answered unknown request id N` and `device sent a malformed
frame` — were written to a handle nobody could read. `C:\SummriseConsole\run-console.cmd` now appends the
console's stdout and stderr to `C:\SummriseConsole\console.log`, rotating the previous run to `console.log.1`
first. On the machine that hosts the console, that is the first file to open when a call is reported as
"the device did not answer" (before it, only the device's `events.jsonl` could be read).

The same directory had accumulated **seventy** `summrise-console.exe.bak-0.3.x` files — one per deploy, about
half a gigabyte. Two are kept now (the two newest), and a deploy that keeps making them should delete the
rest as part of the deploy.

## Where to look when the console says the device did not answer

Two logs, and the order matters.

- **The device's `events.jsonl`** carries `begin` and `end` rows per call with `ts_ms` either side, the tool
  name, and `ok`/`error_kind`. A call that looks slow in the console is usually milliseconds here
  (measured: `terminal.screen` `begin` 1793180839310, `end` 1793180839312, `ok:false`, `error_kind:refused` —
  2 ms). If the device answered, the console's thirty-second wait failed for a console-side reason: a channel
  that was replaced mid-call, or a reply that never reached the waiter.
- **The console's audit** (`/audit`, and the dashboard's Recent activity) has one row per proxied call with
  `status` and `ts_ms`. A `denied 504` there is the console's own bound expiring — it is not Cloudflare's
  504, which arrives as an HTML error page with a ray id.

Filter by time before drawing a conclusion: a `504` in a window that starts before a deploy may be older
than the fix you are testing. After 0.3.148 reached both machines, 22 audited calls in the first minutes had
zero 504s, against four-in-six before it.

## Why a proxied call is cut at ninety seconds

The operator's console is reached through Cloudflare Access, and Cloudflare gives the origin about a
hundred seconds before it renders its own 504 page — with a ray id, not the console's message. On
2026-10-28 two long calls came back that way (a `browser.eval` and a `runs.exec`), and the audit showed the
console had never answered: the run proxy was allowed to wait the run's own ten minutes.

`run_wait` now caps the wait at `RUNS_MAX_PROXY_WAIT` (90 s), which sits under the proxy's ceiling and over
any tool that answers in milliseconds. A run that needs longer keeps running on the device; the console
answers with its own 504 ("the device accepted the request and did not answer inside the console's bound;
it may still be working on it") and the run is collected through `runs.list` / the Runs page.

## Another automation on the operator's machine edits `C:\Windows\Temp`

DESKTOP-14RJCR8 carries tasks this project did not create — `SummriseAgent`, `SummriseDSH`,
`SummriseHop7738/7739/7801`, `SummriseTbCap`, `SummriseZssTunnel`, `SummriseSetup048…053` — and something
there deletes `C:\Windows\Temp` scripts and `Summrise*` scheduled tasks. On 2026-10-28 an install script
verified present (`dir` showed `1,134 i146c.cmd`) had vanished before its task ran, twice: the task reported
`成功` and then the file it named was gone.

What to do about it: name deployment tasks something other than `Summrise*` (a `Z` prefix survived), keep
the script out of `C:\Windows\Temp` if the machine is that busy, and check the file still exists at the
moment the task runs rather than at the moment it was written.

## A second instance is a request, not a mistake (2026-10-28)

The tray is per-session, and Windows re-creates the notification area when a disconnected session comes
back; an icon added before that is simply not there afterwards. The session-reconnect task starts this
binary again for exactly that reason — but the running instance holds the `Local\SummriseShell` mutex, so
the new one used to exit at the check, before its first log line, and the icon stayed missing.

The mutex is now a hand-off. The running instance creates `Local\SummriseShell.Wake` and waits on it; a
second instance opens it, sets it, and exits. The waiter posts a `Repaint` event and the loop rebuilds the
tray (menu, icon, tooltip, in that order — a tray without its menu is a tray whose items do nothing).

Measured on DESKTOP-14RJCR8, running a second copy in the same session:

```
tray built: pid=20168
another instance owns the tray; asked it to re-add the icon
tray re-added after a wake signal
```

## The seed path, exercised (2026-10-28)

Every install until now replaced an existing `baseline/current`; the path a machine with NO generations
takes — the one an uninstall followed by an install, or a genuinely new machine, goes through — had never
run. It does now, on DESKTOP-14RJCR8:

```
uninstall exit 0
install exit 0
STATE : 4  RUNNING
{"status":"ok","version":"0.3.145"}
baseline\current: baseline.json summrise-device.exe
baseline\         current            <- no `previous`, which is what a seed means
```

Two facts worth keeping:

- **the uninstaller does not kill the running shell**; only the installer's `.onInit` does. A shell started
  from an image that the uninstall has since deleted keeps running and keeps the single-instance mutex, so
  the install's "start the shell" step exits quietly ("another instance owns the tray; exiting") and the
  operator is left with the old icon. Kill `summrise-shell.exe` after an uninstall/install cycle before
  starting the new one.
- **`baseline/previous` absent is the signature of a seed.** A replace always rotates the old generation
  into `previous`; a first generation has nothing to rotate.

## Two traps in driving a silent install from a batch file

Both cost a round on 2026-10-28, and both are cmd's, not the installer's.

- **`start /wait setup.exe /S /D=C:\path` does not pass `/D` to the installer.** `start` has its own
  `/D <dir>` (the startup directory), so it eats the switch and the installer runs with `/S` alone — a
  silent install into the default directory, or a dialog with nobody to click it. Call the installer
  directly; cmd waits for it anyway.
- **`curl` to this build box is not fast from every device.** The same 6 MB installer took 8 s to
  DESKTOP-9VH92BJ and 5 min 35 s to DESKTOP-14RJCR8 (~18 KB/s, a relayed tailnet path). A script that
  downloads with no progress line looks hung; write a line before and after the download, and do not
  diagnose a slow link as a stuck installer.

## When an install leaves a machine with no runtime

An install that fails after the old service was removed leaves a machine that cannot be reached at all
— no device channel, no MCP, nothing to ask. This happened on DESKTOP-CPH4KB2 on 2026-10-28, with
installer 0.3.130: the service was removed in `.onInit`, then `--install-service --replace-baseline`
failed while rotating `baseline/current`, and the device went silent. The installer builds since 0.3.131
keep the registration and kill the app before rotating, so the failure mode is closed; the recovery
below is what works on a machine that is already in it.

Two steps, in this order. The first one needs nothing but the launcher that is already installed:

```bat
:: 1. RECOVER: register and start the generation that is already there, without replacing it.
::    ensure_baseline keeps `current` (it is consistent), so this is safe to run on a live install too.
C:\<install>\summrise-launch.exe --install-service --install C:\<install> --data <data> ^
    --seed-sidecar C:\<install>\summrise-device.sidecar.json --port 18090
curl http://127.0.0.1:18090/api/health          :: the device answers again, at the old version
```

```powershell
# 2. UPGRADE (only after step 1 answers): the installer replaces the files and the launcher.
powershell -NoProfile -Command "iwr -UseBasicParsing http://<mirror>/SummriseSetup-<version>.exe -OutFile C:\Windows\Temp\s.exe; Start-Process C:\Windows\Temp\s.exe -ArgumentList '/S','/D=C:\<install>' -Wait; 'exit=' + $LASTEXITCODE"
```

Installers from 0.3.132 on also leave a boot-time repair task, `SummriseRepair`, which re-registers the
service when it is missing — the one failure a machine cannot leave from inside, because the process
that would rebuild the service IS the service. Its script is `repair.cmd` and its log is
`repair-task.log` in the install directory (NOT `repair.log`: the launcher writes that one as a JSON
journal of the generations it restored, and the 2026-10-28 rehearsal proved the two collide), and the
uninstaller deletes both along with the task.

```powershell
```

If step 2 fails, `%TEMP%\summrise-setup.log` names the stage it reached — the file is appended by both
`.onInit` and the install section. Step 1 is verified on a live machine, twice: run against DESKTOP-9VH92BJ it answered `exit=0` and the
health endpoint kept serving the same version throughout, and on 2026-10-28 the service was removed
outright (`--uninstall-service`) and the same command brought the device back — `/api/health` answered
`0.3.139` and the fleet page showed it online again. That is the command the `SummriseRepair` boot task
runs; the task itself was then exercised end to end on DESKTOP-9VH92BJ on 2026-10-28 — the service was
removed by a scheduled task (not by the device's shell, which dies with it), `SummriseRepair` was run, and
its log reads `… the SummriseLaunch service is missing; re-registering` / `service SummriseLaunch is
installed` / `repair exit 0`, with `/api/health` answering again 1.2 s later. The boot TRIGGER is the one
part still unexercised, and it is a Windows guarantee rather than this project's code.

A rehearsal that REMOVES the service cannot be driven from the device's own shell: the shell dies with
the service (its process tree goes too), so the step that would run the repair never executes. Run it from
a scheduled task with a repeating dead-man switch — `schtasks /sc minute /mo 5` running the step-1
command — and delete that task once the device answers.

## The notification icon: what an app can ask for, and what it cannot

The shell draws a tray icon and the operator has asked, repeatedly, where it should sit. On
2026-10-28 the answer was settled by the operator, not by the code: **the icon belongs in the overflow
(the `^` flyout), like the other applications' icons, and not in the visible taskbar area.**

- **The shell no longer asks for a place.** It used to write `IsPromoted = 1` into its own
  `HKCU\Control Panel\NotifyIconSettings` entry, which moves the icon into the visible area — measured
  on DESKTOP-14RJCR8, confirmed by the operator, and rejected: the visible area is where the system
  icons live. That write is gone, and with it the `winreg` dependency; the shell now touches no
  notification-icon setting at all.
- **An application cannot choose its position inside the flyout.** The order lives in
  `HKCU\Control Panel\NotifyIconSettings\UIOrderList`, a binary list Explorer owns; a new icon is
  appended, so it takes the last slot in the grid (five per row) no matter what the app does. Pinning
  or reordering is the operator's one-off action, and Windows remembers it.
- **`IsPromoted` is read when the icon is ADDED.** An icon added before the value changed keeps the
  old placement for the life of that Explorer session; the next add (logon, reconnect, or an Explorer
  restart) reads the new value. This is why the placement seemed to fight back.
- **A force-killed shell leaves a ghost icon.** `taskkill /F` gives the process no chance to remove its
  icon, and Explorer keeps drawing it until it is restarted. Restarting Explorer (`taskkill /f /im
  explorer.exe` from a task in the user's own session, then `start explorer.exe`) removed the duplicate
  and brought the desktop back in about three seconds; doing it from a SYSTEM context would kill the
  shell and start the replacement in session 0, which is why the restart is scheduled as the user.
- **The tray is per-session, and a disconnected session has none.** Windows refuses to start an
  interactive process in a disconnected session at all: `schtasks /run` for a `/IT` task answered
  `0x80070005` on DESKTOP-9VH92BJ. The shell registers `SummriseShellOnReconnect`, triggered by the
  TerminalServices session-reconnection event (25), as the user it runs as, so the icon returns when the
  operator does.

What follows for support: the icon lives in the `^` flyout by design. If there are two of them, one is a
ghost — restart Explorer. If there is none, check the session is connected before blaming the shell.

## Retiring the legacy runtime on a machine

An older laptop can carry both runtimes at once: `summrise-device` (the new one, as a service) and the
legacy `SummriseDesktop` / `SummriseAgent` pair, which live as **scheduled tasks**, not services. Two
tray icons then sit in the notification area, and the old one sorts first because it was registered
first — the new icon cannot take its place (Windows owns that order), so the way to have one Summrise
icon is to stop the old ones.

What the legacy pair is, observed on all three canaries (2026-10-28):

| task | runs | what it is |
|---|---|---|
| `SummriseDesktop` | `D:\Summrise\scripts\summrise-launch.exe powershell ensure-desktop.ps1`, as the logged-in user | the Electron desktop app (`electron.exe`, 4–5 processes per session) — this is the old tray icon |
| `SummriseAgent` | `summrise-agent.exe`, one in the user session (the tray) and one in session 0 | the legacy agent |

```bat
:: as SYSTEM / an elevated prompt; the tasks are per-machine, the processes per-session
schtasks /end /tn SummriseDesktop
schtasks /change /tn SummriseDesktop /disable
schtasks /end /tn SummriseAgent
schtasks /change /tn SummriseAgent /disable
taskkill /F /IM electron.exe
```

The new runtime is untouched by this (its service is `SummriseLaunch`, which runs `summrise-device` as
its child), and the new tray starts at logon from `HKLM\...\Run\SummriseShell`. Reversing it is one
command per task: `schtasks /change /tn SummriseDesktop /enable` (and the same for `SummriseAgent`).

If the new tray is not running in a session, start it there — a process started from a SYSTEM shell
puts no icon on anyone's taskbar:

```bat
schtasks /create /tn SummriseShell /tr C:\<install>\summrise-shell.exe /sc once /st 00:00 /ru <user> /it /f
schtasks /run /tn SummriseShell
```

On 2026-10-28 all three canaries ended with only the new runtime serving, the new tray running in the
logged-in session, and every device answering `0.3.129` on the console's fleet page.

## The tray is installer-owned

`summrise-shell.exe` — the tray and the window — is the one piece the canary update does **not** carry.
The update path stages and swaps `summrise-device.exe`; the shell is installed by `SummriseSetup`, started
at logon from `HKLM\...\Run\SummriseShell`, and replaced only by running a newer installer. A shell change
therefore reaches a machine through the installer, never through a canary document; `verify-release.sh`
reports whether the publish even has one, because a publish without NSIS (`makensis`) can update the app
and silently leave every tray behind.

Repairing a tray by hand, when an install cannot be run right now (the exact procedure this was learned
with, on DESKTOP-14RJCR8 in October 2026):

```bat
:: from an elevated prompt, with the console's operator token and a mirror that serves the shell
copy /Y C:\<install>\summrise-shell.exe C:\<install>\summrise-shell.exe.bak-old
curl -f -o C:\<install>\summrise-shell.exe.new http://<mirror>/summrise-shell-<version>.exe
certutil -hashfile C:\<install>\summrise-shell.exe.new SHA256   :: compare with the build's digest
move /Y C:\<install>\summrise-shell.exe.new C:\<install>\summrise-shell.exe
```

A shell started by a service or a scheduled task with `/IT` runs in the user's session, which is where the
tray belongs; starting it from a SYSTEM shell does not put an icon on anyone's taskbar. After a start, read
`%LOCALAPPDATA%\Summrise\shell.log`: it records `starting`, `event loop built`, `menu built`, `tray built`,
and a 15 s watchdog turns "never built" into a logged non-zero exit rather than a ghost process.

## Before anything

* The three public keys are compile-time: `SUMMRISE_LAUNCHER_PUBKEY`, `SUMMRISE_RELEASE_PUBKEY`,
  `SUMMRISE_CONSOLE_GRANT_KEY`. A build without them produces a binary that trusts nothing, so a publish
  exports them rather than relying on a shell profile.
* The signing keys live outside the repository: `--launcher-key`, `--release-key` (seed files).
* The mirror directory is the one the public URL serves. Pass it as `--mirror-dir`; the publish copies
  there itself, because a copy left to a human is a copy that eventually is not made.

## Publishing a device release

```bash
export SUMMRISE_LAUNCHER_PUBKEY=… SUMMRISE_RELEASE_PUBKEY=… SUMMRISE_CONSOLE_GRANT_KEY=…

scripts/publish-canary.bash \
    --version 0.3.21 --channel canary --port 18090 \
    --mirror http://<dev-box>:18100 \
    --mirror-dir /path/that/the/mirror/serves \
    --launcher-key …/launcher.seed --release-key …/release.seed \
    --out /tmp/canary-mirror \
    --console http://127.0.0.1:19000        # optional: verify the world at the end
```

The page is Rust rendered per request, compiled into the console binary, so there is no separate
artifact that can be older than its sources — a publish's binary IS the page. What the publish still
checks is the version it signs and the mirror it names.

What it produces:

* `summrise-device-<version>.exe` and `release-<version>.json` — the signed document naming the artifact
  URL and its sha256;
* `SummriseSetup-<version>.exe` — the installer, which carries the Rust tray (`summrise-shell.exe`) and
  registers it under `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` so each interactive login
  shows the device's status dot. The service is headless; the tray is the visible half;
* `release-<channel>.json` — the **channel pointer**, the file a device's `update.json` names, always the
  newest document for that channel;

## Deploying the console

```powershell
# on the console host
copy console.exe  C:\SummriseConsole\summrise-console.exe
taskkill /F /IM summrise-console.exe
C:\SummriseConsole\run-console.cmd     # or: schtasks /run /tn SummriseConsole
curl http://127.0.0.1:19000/health
```

The version comes from the build (`build.rs` reads `SUMMRISE_BUILD_VERSION`), not from a flag: a label
typed into a launcher script drifted once and reported a superseded release for a day. `/health` is the
only version that matters.

### The console's run script, and the rollback

`run-console.cmd` is the service's whole definition, and the flags it passes are the console's data
identity: a redeploy replaces the **binary**, never the script.

```bat
@echo off
"C:\SummriseConsole\summrise-console.exe" ^
  --operator-token-file C:\SummriseConsole\operator.token ^
  --grant-key-file      C:\SummriseConsole\grant.key ^
  --data                C:\SummriseConsole\data ^
  --bind                0.0.0.0 ^
  --port                19000
:: optional, when the fleet has a mesh broker:
::  --mesh-broker http://<broker>:<port>
```

- `--data` holds `devices.json`, `codes.json`, `audit.jsonl` and `denials.jsonl`: the registry and the audit
  are that directory, so a redeploy that changes it is a different console, not an upgrade.
- `--operator-token-file` (64 hex characters) and `--grant-key-file` (a 32-byte seed as hex) are runtime
  secrets, mode 0600, not build inputs.
- `--bind 0.0.0.0` is what the tunnel needs to reach port 19000; the Windows firewall and Cloudflare Access
  are what keep that bind from being public. Bind the tailnet interface instead if the host has one.
- `--access-jwks/--access-aud/--access-issuer` only applies to the Access deployment; there the JWT is the
  operator identity and the token box is skipped.
- `--mesh-broker` is optional: without it `/mesh` reports `configured: false` and the device card says so.

**The rollback is the previous binary, not a code revert:**

```powershell
taskkill /F /IM summrise-console.exe
copy /Y C:\SummriseConsole\summrise-console.exe.bak-<old> C:\SummriseConsole\summrise-console.exe
C:\SummriseConsole\run-console.cmd
curl http://127.0.0.1:19000/health   # the old version answers
```

### Proving the standard MCP endpoint after a deploy

The console binary carries the page and, for every registered device, the standard MCP endpoint
`/mcp/<device-id>`. After a redeploy, prove all three without an AI client:

1. `curl http://127.0.0.1:19000/health` — the version must be the one just built, and `devices` the
   registry size.
2. The page at `http://127.0.0.1:19000/`: sign in, open a device's **Connect an AI client** page (rail:
   **Tokens**) and **Mint a token**; it shows the endpoint and the header once. A session grant is the
   same call through `POST /grants`, not a page.
3. The endpoint: use the card's header (or a grant minted through the loopback `/grants`) and send
   `initialize`, `tools/list` and one `tools/call`. The data directory's `audit.jsonl` records
   `{"kind":"mcp","tool":"<tool>","status":200}` for the call; `initialize`/`tools/list` are not a tool
   call and carry no tool name.

An AI client on another machine also has to cross the console's edge. The endpoint needs its **own
Cloudflare Access application at `/mcp/*`** — a bypass, or a service token scoped to that application —
and never the operator application: a verified Access JWT there (a service token included) is a full
operator in this console. Access decides who may reach the path; the signed grant remains the
endpoint's authorization. `CONNECT-MCP.md` has the grant, the header and the Access details.

A public `fetch failed` while the loopback call succeeds is the edge/tunnel leg, not the endpoint:
retry once and then look at the console host's tunnel. The first public canary on 2026-10-26 failed one
`tools/call` that way and the same grant answered on the retry; the official DeepSeek Harness client
then completed `initialize`, `tools/list` (filtered to the grant) and `tools/call` against the real
device. The public `/health` is a `302` to Access, not the console's health JSON; the loopback
`/health` is the version that matters.

## Updating a device

A device with `update.json` pointing at the channel pointer updates itself on its interval (15 minutes in
the canary). To make it check now:

```powershell
C:\SummriseInstall\summrise-device.exe --check-update http://…/release-canary.json \
    --channel canary --allow-plaintext --data C:\ProgramData\Summrise --install C:\SummriseInstall
```

`--allow-plaintext` is required for an `http://` mirror that is not loopback. The device then verifies the
document's signature, fetches the artifact, checks the digest, stages it, and asks the supervisor to swap;
the supervisor swaps, health-checks, and rolls back if the new build does not answer.

## The browser pane needs a browser on the device

`browser.*` drives a real Chrome/Chromium **on the device**, not in the console. Without one, every call
answers `no Chrome or Chromium was found; set SUMMRISE_CHROME to its executable` — which is a deployment
requirement, not a bug, and the pane shows that sentence verbatim.

```
# on the device, as an administrator
[Environment]::SetEnvironmentVariable('SUMMRISE_CHROME', 'C:\Chromium\chrome-win\chrome.exe', 'Machine')
sc stop SummriseLaunch
sc start SummriseLaunch
```

Two traps, both measured:

* **The service is `SummriseLaunch`**, not `SummriseDevice`. Stopping the wrong name is a silent no-op and
  looks like "the variable did not take".
* **The variable is read when the process starts.** The launcher must be restarted, not just the app: it
  is the launcher that spawns the app, and a child inherits the launcher's environment.
* Google's `ChromeStandaloneSetup64.exe` link returns **404** now. The Chromium snapshot
  (`https://storage.googleapis.com/chromium-browser-snapshots/Win_x64/LAST_CHANGE` then
  `.../<rev>/chrome-win.zip`, ~369 MB) needs no installer: unzip it anywhere and point `SUMMRISE_CHROME`
  at `chrome-win\chrome.exe`.

## Onboarding a machine

The path was walked end to end against the live console, from a throwaway data directory so the installed
device was untouched:

```
console: POST /auth   → a session
console: POST /enroll → a code, 900 s
device:  summrise-device.exe --enroll http://<console> <code> --allow-plaintext \
             --data <scratch>/data --install <scratch>/install
console: GET /devices → the new id is listed
```

That is the whole onboarding story for a machine the operator can reach: mint a code, run one command.
The installer's own `/CONSOLE=` and `/CODE=` flags are **not** part of it — `GetOptions` truncates
`/CONSOLE=http://…` at the `//`, so those flags have never worked and are parked. A deployment that cannot
run a command line leaves `<data>/console-bootstrap.json` instead; the app reads it once at first start and
deletes it as the receipt.

## What the public URL exposes

| path | through the tunnel | why |
|---|---|---|
| `/` | 302 to Cloudflare Access | the console is the operator's, not the internet's |
| `/next` | 302 to Cloudflare Access | the Rust sign-in and fleet page; the same operator policy as `/` |
| `/health` | 302 to Cloudflare Access | same: it names the build and the device count |
| `/devices` | reaches the console, 401 without a session | the device channel uses this path and bypasses Access |
| `/mcp/<device-id>` | its own Access application (`/mcp/*`, not the exact `/mcp`): bypass, or a service token; the console still requires the grant | the standard MCP endpoint an AI client uses; see `CONNECT-MCP.md` |

A monitor that needs an unauthenticated health check has to be added to the bypass application — that is a
deliberate act, not an accident of the configuration.

## Proving it landed

```bash
scripts/verify-release.sh 0.3.21 \
    --mirror http://<dev-box>:18100 \
    --console http://<canary>:19000 \
    --console-version 0.3.21
```

It checks three things the repository cannot see: the channel pointer names this version, the artifact
the signed document names hashes to the digest it declares, and the console answering `/health` is running
the build the operator deployed. `--console` without `--console-version` checks liveness and the device
count only — a device release does not redeploy a console, and claiming it does would make a correct
publish fail.

The mutation proof: asking for `0.3.17` while `0.3.18` is live reports two failures and exits 1. That was
a real state for a day.

## The page itself

Nothing is built beside the binary: the page is `maud` templates in `console/src/http/page.rs` and
the binary renders it. To look at every page without a browser workflow:

```bash
scripts/console-screenshots.bash --out /tmp/console-shots   # 28 pages, light + dark
```

The two scripts the page serves (`assets/approvals.js`, `assets/browser.js`) are `include_str!`'d,
and the terminal's xterm stack is vendored the same way.

## Rolling back

* **One device, a bad release**: the supervisor keeps `baseline/previous`; a build that fails its health
  check is rolled back automatically and `state.json` records `rolledBack`.
* **A release that must not spread**: publish a revocations list and point devices at it
  (`--revocations` on the update check). A revoked version is refused before its artifact is fetched.
* **A console that misbehaves**: keep the previous binary; the page is inside it, so restoring the binary
  restores the page.

## Traps this document exists for

* **NSIS `/D=` must be last.** Before it, the rest of the command line becomes the install path — measured:
  a directory literally named `C:\SummriseCanary CONSOLE=http… CODE=…` and a service pointing at it.
* **PowerShell writes a BOM** with `Set-Content -Encoding utf8`, and JSON forbids one. The device tolerates
  a leading BOM now and quarantines a source it cannot read instead of refusing to start.
* **A device reconnects on its own backoff** (1 s → 60 s). A check that runs right after a console restart
  must wait for the channels; the smoke script does.
* **A grant without its signature is not a grant.** Send the whole envelope; the device checks it.
