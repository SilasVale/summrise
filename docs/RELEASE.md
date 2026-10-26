# Releasing, and proving it landed

Three version streams ship from this repository and none of them is deployed by the same act:

| stream | what it is | how it reaches a machine |
|---|---|---|
| **device runtime** | `summrise-launch` + `summrise-device`, the Windows service | the installer, or the device's own update check against the mirror |
| **console** | `summrise-console`, the operator's surface | copied to the console host and restarted |
| **console page** | `console/web`, built into one HTML file | inside the console binary — no separate deploy |

They share a version number because they are released together, but a *device* release does not redeploy a
*console*, and a console deploy does not touch a device. `verify-release.sh` is the tool that keeps those
facts straight (see "Proving it landed").

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

It refuses to publish when the console page is older than its sources (`build-web.sh --check`): a
TypeScript edit that never reached the bundle is the one failure a publish cannot see on its own.

What it produces:

* `summrise-device-<version>.exe` and `release-<version>.json` — the signed document naming the artifact
  URL and its sha256;
* `release-<channel>.json` — the **channel pointer**, the file a device's `update.json` names, always the
  newest document for that channel;
* `SummriseSetup-<version>.exe` — the installer, carrying the first generation's sidecar.

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
| `/health` | 302 to Cloudflare Access | same: it names the build and the device count |
| `/devices` | reaches the console, 401 without a session | the device channel uses this path and bypasses Access |
| `/mcp/<device-id>` | its own Access application (`/mcp/*`, not the exact `/mcp`): bypass, or a service token; the console still requires the grant | the standard MCP endpoint an AI client uses; see `docs/CONNECT-MCP.md` |

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

```bash
scripts/build-web.sh          # npm ci (once) + vite build → console/web/dist/index.html
scripts/build-web.sh --check  # fails when the artifact is older than the sources
```

`dist/index.html` is embedded with `include_str!`. Nothing from `node_modules` ships.

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
