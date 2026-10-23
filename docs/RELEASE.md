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
    --mirror http://100.85.163.159:18100 \
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

## Proving it landed

```bash
scripts/verify-release.sh 0.3.21 \
    --mirror http://100.85.163.159:18100 \
    --console http://100.74.250.98:19000 \
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
