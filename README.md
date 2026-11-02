# Summrise

**Summrise is the capability hub for AI on a real machine: one MCP endpoint with hands that work on
install, plugins when one is missing, and a console that shows every call and lets its owner take the
keyboard back.**

That sentence, the model, the five surfaces and the eleven design rules are
[`docs/PRODUCT.md`](docs/PRODUCT.md). The plan of record is
[`docs/design/capability-hub-2026-11-02.md`](docs/design/capability-hub-2026-11-02.md). This README is
the build/run map; it does not restate the product.

## The hub

The hub is the device runtime:

- **`summrise-device`** advertises the built-in capabilities (terminal, files, browser, runs, screen,
  host, and the rest in `device/core/src/registry.rs`), executes their tools, writes the audit row and
  serves MCP over loopback and the console channel with one authorization check. It also hosts
  plugins: each installed plugin is an MCP server child process, and its tools are advertised behind
  the same endpoint as `<namespace>.<tool>`, under the same token and in the same record.
- **`summrise-launch`** starts, restarts, updates and repairs the app. It is the only process that
  writes `<install>/summrise-device.exe`, so an update cannot race the process it replaces.
- **`summrise-console`** is the window and the door: it enrolls devices, serves the operator page,
  proxies MCP to each device, mints tokens, and reads each device's live tool list for the page. It
  does not execute tools and does not own plugin state.
- **`summrise-shell`** is the tray process in the user's session: the icon, the console window, and
  the helper the device calls to launch Chrome or capture that session's screen.

The console is one hub per machine, with five destinations:

| Destination | What it is |
|---|---|
| **Home** | The hub at a glance: running, the endpoint, the tool counts, the next action. Before a machine is chosen it is the fleet. |
| **Capabilities** | Every tool the device advertises — built-in and plugin — grouped by capability, with its schema and **Try it**. |
| **Plugins** | Install, watch, restart and remove the capabilities that are not built in. |
| **Activity** | The record: every call, its token, its device and its result. |
| **Device** | The operator's hands: Terminal · Screen · Files · Runs · Machine, as one workspace. |

**Connect is a sheet, not a destination**: Endpoint, Clients, Tokens. It opens from Home, Device and
the account menu. The account menu holds the console version, the console URL, the docs link, the
fleet tokens and sign out. The fleet is Home's zero state.

```
device/
  core/      pure decisions: the state document, grants, sessions, the registry, the plugin manifest
  launch/    the supervisor and the only writer of the app binary
  device/    the app: capabilities, local HTTP/MCP, the console channel, the plugin host
  console/   the window: operator sessions, enrollment, tokens, the device channel, audit, React page
  mesh/      pinned-QUIC sessions, the broker and relay
  release/   signed release documents, channels, revocation lists, the signer
  shell/     the tray process in the user's session
  ui/        the design tokens and shared page shell
  winsec/    the Windows security helpers
  winsys/    the Windows calls safe crates cannot make
examples/
  plugins/hello/   the reference plugin: an MCP server in one PowerShell file
landing/           the public page, English and Chinese, checked by landing/check.mjs
```

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

The console page is **React 19** (`device/console/web`). Vite builds it into one self-contained HTML
file, `summrise-console` embeds that file with `include_str!`, and `/` serves it. `/next` is the Rust
fallback that redeems a login code; it is not the operator's normal path. There is no Node at runtime.

```bash
cd device/console/web
npm ci && npm run typecheck && npm test && npm run build
cd ../../..
scripts/build-web.sh --check     # fail when the committed dist is stale
```

CI's `console-web` job runs the typecheck, the freshness check and the browser gate. The jsdom suite
is a local tool, not a required gate; the Rust suite covers logic and the token contract.

The public page is plain static assets, checked without dependencies:

```bash
node landing/check.mjs
```

It refuses a translation that exists on only one side, a broken anchor, a missing asset, a contrast
pair under WCAG AA, and a social card that is not 1200×630. CI's `landing` job runs the same check
and `scripts/layout-gate.mjs` on the page.

## The console's URLs

The destination is the fragment. A reload keeps it, and the selected machine is stored separately:

```
#home                      #capabilities              #capabilities/<tool>
#plugins                   #activity                  #device/terminal
#device/screen             #device/files              #device/runs
#device/machine            #run/<device-id>/<run-id>
```

The previous shell's fragments keep working as aliases: `#terminal`, `#screen`, `#files`, `#runs`,
`#system` and `#audit` open the matching tab or destination; `#devices` opens Home. There is one
navigation; the old names are bookmarks, not a second rail.

To look at the React page without a live console, the screenshot harness serves the built file from
loopback, answers the JSON routes with fixtures, and captures the destinations and the Device
workspace in light and dark:

```bash
cd device/console/web
node scripts/screenshot.mjs r13
```

The gallery is `/tmp/console-react-shot/r13/index.html`.

## Plugins

A plugin is an MCP server installed from a directory on the device. The hub starts it as a child,
namespaces its tools, and merges them into `tools/list`; the full contract — manifest, protocol,
lifecycle, errors — is [`docs/PLUGINS.md`](docs/PLUGINS.md).

**Installing a plugin runs its code with this machine's privileges. There is no sandbox, signature or
registry in v1; install only code you trust.**

The reference is [`examples/plugins/hello/`](examples/plugins/hello/). Copy it to the device (for
example `C:\SummrisePlugin\hello`), install it from the console's **Plugins → Add plugin**, or call
the same tool through any MCP client:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"plugins.add","arguments":{"path":"C:\\SummrisePlugin\\hello"}}}
```

`tools/list` then advertises `hello.greet` and `hello.echo`. A call returns the plugin's answer:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call",
 "params":{"name":"hello.greet","arguments":{"name":"Ada"}}}
```

After the console is deployed, `scripts/real-plugin-check.ps1` proves the path end to end — add, list,
call `hello.greet`, read the log, remove, and prove the tool disappeared — and prints
`REAL_PLUGIN_OK`. It reads the operator token from its file and never prints it.

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
SummriseSetup-<version>.exe /S /D=C:\SummriseInstall

:: verify
curl http://127.0.0.1:18080/api/health
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

The console's **Home → Add a machine** mints a one-time code for this machine (it works once, for 15
minutes) and prints the command. Either path spends that code once and writes `console.json` and
`console-token.json` under the data directory; the running app builds its console link at boot, so
restart the `SummriseLaunch` service afterwards.

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
  trusted plaintext tunnel. `--data` defaults to the running executable's parent plus `data`, which
  on an installed binary is **not** the service's store — so pass `--data C:\ProgramData\Summrise`,
  as above.

The machine appears in Home's fleet, online, when the channel is up. Choosing it opens the hub.

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
| what the product is, in one sentence and five surfaces | [`docs/PRODUCT.md`](docs/PRODUCT.md) |
| how to go from an installed device to a first call | [`docs/QUICKSTART.md`](docs/QUICKSTART.md) |
| how an AI client reaches a device through the console | [`docs/CONNECT-MCP.md`](docs/CONNECT-MCP.md) |
| how to write, install and debug a plugin | [`docs/PLUGINS.md`](docs/PLUGINS.md) |
| how the console is deployed and rolled back | [`docs/console-deploy.md`](docs/console-deploy.md) |
| why two runtimes, and what each owns | `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` |
| how a canary is run, step by step | `docs/superpowers/specs/2026-10-21-two-runtime-canary-runbook.md` |

## Where things are

- `docs/RELEASE.md` — how a release is published, deployed and verified against the world.
- `docs/superpowers/specs/` — the designs and decisions, including the ones that were reversed.
- `device/console/web/` — the React page and its screenshot harness; the built `dist/index.html` is
  committed and embedded in the console binary.
- `device/console/src/http/page.rs` — the maud fallback at `/next`, kept for login-code redemption.
- `device/device/src/panel.rs` — the device's own panel, rendered in Rust from the same design tokens.

## License

See `LICENSE`.
