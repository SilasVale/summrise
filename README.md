# Summrise

The Summrise device runtime: a Windows agent that supervises itself, updates from signed releases,
and offers its capabilities to a local operator, a mesh peer, or a console session.

This repository is the clean start for the runtime that replaces the pre-1.3 `agent/`. The old tree
lives in `summrise-legacy`; nothing here depends on it.

## The shape

```
device/
  core/      pure decisions: the state document, grants, sessions, update selection, the registry
  launch/    the supervisor and the only writer of the app binary (SCM service, install, swap, repair)
  device/    the app: capabilities, local HTTP/MCP, the console channel, the mesh service
  mesh/      pinned-QUIC sessions, the broker and relay, STUN candidates
  release/   signed release documents, channels, revocation lists, the signer
  console/   the control plane: operator sessions, enrollment codes, grants, the device channel, audit
  winsec/    the Windows security helpers (protected DACLs), shared by launch and device
  winsys/    the Windows calls safe crates cannot make: process listing and termination
```

Two runtimes, one control plane:

- **`summrise-launch`** starts, restarts, updates and repairs the app. It is the only process that
  writes `<install>/summrise-device.exe`, so an update cannot race the process it replaces.
- **`summrise-device`** is the product. It owns the state document, executes tools, and serves MCP
  over loopback, the console channel, and the mesh with one authorization check.
- **`summrise-console`** brokers: it enrolls devices, signs grants offline-verifiable by the device,
  and carries operator traffic without being able to forge a device's answer.

## Build and test

Rust 1.98.1 (`rust-toolchain.toml`).

```bash
scripts/check.sh              # the commands CI's `device` job runs, read out of ci.yml
scripts/check.sh --fast       # format and lints only
scripts/check.sh --windows    # also cross-compile the Windows target (needs cargo-xwin)
```

Windows is the target: the lifecycle suite only runs there (`ci.yml`'s `device-windows` job), and a
Linux `cargo test` proves the portable half. `--windows` compiles the Windows-only paths from Linux;
it proves they build, not that they pass.

The console page is Rust. `/` redirects to `/next`, which `summrise-console` renders from `maud`
templates; there is no npm, no committed bundle and no second build. A page change is gated by the
same `cargo test -p summrise-console` the rest of the crate uses.

To look at every page at once — light and dark, at any width — the console renders itself into a
directory and a browser photographs it:

```bash
scripts/console-screenshots.bash --out /tmp/console-shots          # 26 pages, light + dark
scripts/console-screenshots.bash --out /tmp/console-shots --width 900 --light-only
open /tmp/console-shots/index.html
```

A second run can say what moved: `--diff <previous run>` compares every screenshot, lists the pages
whose pixels changed (with the share), writes a highlighted diff image per changed page under
`<out>/diff/`, and `--fail-on-change` makes it an exit code for a gate. With no argument, `--diff`
compares against the baseline (`--baseline`, default `$XDG_CACHE_HOME/summrise-console-shots`), which
`--update-baseline` replaces with the run just made — so a styling change is two commands on a fresh
checkout: `--update-baseline` once, `--diff` after every change.

The only JavaScript left is vendored xterm and the two terminal pages' glue
(`device/console/assets/`, `device/device/src/panel.html`). `scripts/console-smoke.mjs` drives the
console in a browser — the fleet table, every device link, and a real shell typed into — and needs
a machine where a browser can start (a signed-in desktop session, not the device service):

```bash
CONSOLE_URL=http://127.0.0.1:19000 CONSOLE_TOKEN_FILE=/path/operator.token \
CONSOLE_BROWSER='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe' \
node scripts/console-smoke.mjs            # add --terminal for the shell leg
```

## The documents

- `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` — the design.
- `docs/superpowers/specs/2026-10-21-remote-access-two-client-classes-design.md` — who may reach a device.
- `docs/superpowers/specs/2026-10-21-two-runtime-canary-runbook.md` — how to install and verify it.
- `docs/superpowers/specs/2026-10-21-phase-5-cutover-plan.md` — when the old tree goes away.
- `docs/CONNECT-MCP.md` — how an AI client reaches one device through the console.

## License

See `LICENSE`.

## Install (from a release)

A release carries the installer, the app binary and the signed release document:

| Asset | For |
|---|---|
| `SummriseSetup-<version>.exe` | installing on a Windows machine |
| `summrise-device-<version>.exe` | the updater's artifact (a device fetches it, not a human) |
| `release-<version>.json` | the signed document the updater verifies |
| `SHA256SUMS` | checking what you downloaded |

```bat
:: interactive, or silent into a chosen directory (/D= must be last and unquoted)
SummriseSetup-0.3.7.exe /S /D=C:\SummriseInstall

:: verify
curl http://127.0.0.1:18090/api/health
summrise-launch.exe --self-check C:\SummriseInstall --data C:\ProgramData\Summrise
```

Defaults: install `%ProgramFiles%\Summrise`, data `%ProgramData%\Summrise`, service
`SummriseLaunch`, port 18080 (canary builds published so far use 18090 so they can coexist with the
pre-1.3 agent). Uninstall from Add/Remove Programs or `uninstall.exe /S`; it removes the service and
asks about the data directory only when interactive.

A device updates itself once it is given a source, and never without a verified document:

```bat
summrise-device.exe --set-update-source <document-url> --channel canary --interval 21600 ^
    --data C:\ProgramData\Summrise
```

`--allow-plaintext` is needed only for a plain-http mirror on a trusted tunnel; a device applies a
release only after checking the signature, the artifact's SHA-256, the channel and the revocation
list, and the supervisor rolls back to the previous generation if the new one fails its health probe.

## Onboarding a machine

The console's **Add a device** mints a one-time code for this machine. Either path spends that code
once and writes `console.json` and `console-token.json` under the data directory; the running app
builds its console link at boot, so restart the `SummriseLaunch` service afterwards.

- **From the panel (a person at the machine).** Open `http://127.0.0.1:<port>/` on the device — 18080
  for a stable install, 18090 for the canary builds published so far — and paste the console URL and
  the code under **Console link → Enrol this machine**, then press **Enroll**. The page is served on
  loopback only and carries a per-boot token the endpoint requires, so enrollment needs no command
  line and no login. Tick **Replace an existing enrollment** only to rotate this machine's token: the
  old token stops working.
- **From a command line:**

  ```bat
  summrise-device.exe --enroll <console-url> <code> --data C:\ProgramData\Summrise
  ```

  Add `--force` to replace an existing registration, and `--allow-plaintext` only for a console on a
  trusted plaintext tunnel. `--data` defaults to `<install>\data`, so an installed binary needs only
  the URL and the code.

## Cutting a release

1. **Build and sign**: `scripts/publish-canary.bash --version <v> --channel canary --mirror <url>
   --launcher-key <seed> --release-key <seed> --out <dir>` builds the launcher and the app, signs both
   sidecars and the release document, and builds the installer. It fails closed when a public key is
   missing — an installer built without its keys cannot verify its own seed.
2. **Prove the installer, not just the build**: run it on a canary machine (`/S /D=<dir>`), then
   `summrise-launch --self-check <dir> --data <data>` and `curl http://127.0.0.1:<port>/api/health`.
   Both must pass before the version is called released.
3. **Tag and publish**: `git tag -a v<v> -m "..." && git push origin v<v>`. `installer.yml` builds the
   assets, installs its own installer on the runner, and attaches them to the release when Actions can
   start a job; when it cannot, assemble the release from the local build and say so in the notes.
4. **Publish the checksums** and point the mirror's README at the new version.

## Where to read next

The design is a record, not a wiki page: each document states a decision and what would overturn it.

| Question | Document |
|---|---|
| how to go from an installed device to an AI client | [`docs/QUICKSTART.md`](docs/QUICKSTART.md) |
| why two runtimes, and what each owns | `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` |
| where the console's TLS lives | `2026-10-22-the-console-transport-decision.md` |
| why there is no desktop shell | `2026-10-22-the-desktop-shell-decision.md` |
| how a canary is run, step by step | `2026-10-21-two-runtime-canary-runbook.md` |
| what the canary actually proved | `2026-10-22-the-canary-observation-report.md` |
| can the old runtime be retired | `2026-10-22-the-cutover-verdict.md` (verdict: not yet, and what would make it yes) |

## Where things are

* `docs/RELEASE.md` — how a release is published, deployed and **verified against the world**.
* `docs/superpowers/specs/` — the designs and decisions, including the ones that were reversed.
* `device/console/web` — the console page (React, built into one file the binary embeds).
* `device/device/src/panel.html` — the device's own panel (hand-written, same design tokens).
