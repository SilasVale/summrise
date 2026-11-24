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

The console has four global destinations and a workspace for the selected machine:

| Destination | What it is |
|---|---|
| **Overview** | The fleet at a glance: machine status, today's activity and access to the selected machine. |
| **Machines** | The fleet list, machine selection and **Add a machine**. |
| **Activity** | The record: every call, its sentence, its device and its result; a row opens the evidence behind it. |
| **Access** | **Endpoint**, **Clients** and **Tokens** for connecting clients and managing their credentials. |
| **Device** | The selected machine's workspace: **Overview** · Terminal · Screen · Files · Runs · **Tools** · Memory · Machine. **Tools** is the whole catalogue — built-in and plugin, grouped by capability — with the plugin list as a section of the same page. |

**Connect a client** is also a quick sheet with **Endpoint**, **Clients** and **Tokens**. It opens
from Overview, Device and the account menu. The account menu holds the console version, the console
URL, the docs link, Tokens and sign out.

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
file, `summrise-console` embeds that file with `include_str!`, and `/` serves it. `GET /next` is a
303 shim that redeems a login code (`?code=…`, printed by `--login-link`) and hands the browser to the
React page; the maud pages behind it were deleted on 2026-11-04. There is no Node at runtime.

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
#home                      #machines                  #activity
#access                    #device/overview           #device/terminal
#device/screen             #device/files              #device/runs
#device/tools              #device/tools/<tool>       #device/memory
#device/machine            #run/<device-id>/<run-id>
```

`#home` remains the address for **Overview**. The previous shell's fragments keep working as aliases:
`#terminal`, `#screen`, `#files`, `#runs`, `#system` and `#audit` open the matching section or
destination; `#devices` opens Overview. `#capabilities`, `#capabilities/<tool>` and `#plugins` open
**Device → Tools** when a machine is selected, or Overview when none is selected. There is one
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
example `C:\SummrisePlugin\hello`), then install it from **Device → Tools → Plugins → Add plugin**.
For an MCP client, use the session and transaction lifecycle in
[`QUICKSTART.md`](docs/QUICKSTART.md#5-make-the-first-call). The installing token must name
`terminal.create`, `terminal.close` and `plugins.add`, with destructive consent for creating the
shell and installing the plugin. The `txn.begin`, `txn.commit` and `txn.abort` control tools are
admitted independently of the capability allowlist.

Send these requests in order. Use the `id` returned by `terminal.create` for `<session-id>` and the
`txn` returned by `txn.begin` for `<txn-id>`:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"terminal.create","arguments":{"kind":"pty"}}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call",
 "params":{"name":"txn.begin","arguments":{"session":"<session-id>","purpose":"Install the hello plugin."}}}
```

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call",
 "params":{"name":"plugins.add","arguments":{"path":"C:\\SummrisePlugin\\hello","txn":"<txn-id>"}}}
```

Close the session while the transaction is working, then declare the work done:

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call",
 "params":{"name":"terminal.close","arguments":{"id":"<session-id>"}}}
```

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call",
 "params":{"name":"txn.commit","arguments":{"txn":"<txn-id>"}}}
```

On failure, close the session while its transaction is working and use
`txn.abort` with `{"txn":"<txn-id>","reason":"Plugin installation did not complete."}` instead of
commit. Abort declares that the attempt did not achieve its purpose; it undoes nothing.

The device advertises `hello.greet` and `hello.echo` after installation. An existing token's tool set
does not expand when a plugin is installed: mint a new device token naming `hello.greet`, `terminal.create`
and `terminal.close`, with destructive consent for the shell bootstrap. Start a new session-bound
transaction as above, then call the plugin with the advertised `txn` argument:

```json
{"jsonrpc":"2.0","id":6,"method":"tools/call",
 "params":{"name":"hello.greet","arguments":{"name":"Ada","txn":"<txn-id>"}}}
```

Close that session before committing or aborting its transaction. The hub consumes `txn` before
forwarding the plugin's own arguments.

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

The console's **Machines → Add a machine** mints a one-time code for this machine (it works once,
for 15 minutes) and prints the command. Either path spends that code once and writes `console.json`
and `console-token.json` under the data directory. The running app builds its console link at boot.

- **From the panel (a person at the machine).** Open `http://127.0.0.1:<port>/` on the device — 18080
  for a stable install, 18090 for the canary builds published so far — and paste the console URL and
  the code under **Console link → Enrol this machine**, then press **Enroll**. The page is served on
  loopback only and carries a per-boot token the endpoint requires, so enrollment needs no command
  line and no login. Tick **Replace an existing enrollment** only to rotate this machine's token: the
  old token stops working. On a supervised install, the panel asks the launcher to restart the app
  and says so on the page.
- **From a command line:**

  ```bat
  summrise-device.exe --enroll <console-url> <code> --data C:\ProgramData\Summrise
  ```

  Add `--force` to replace an existing registration, and `--allow-plaintext` only for a console on a
  trusted plaintext tunnel. `--data` defaults to the running executable's parent plus `data`, which
  on an installed binary is **not** the service's store — so pass `--data C:\ProgramData\Summrise`,
  as above. After command-line enrollment, restart the `SummriseLaunch` service yourself.

The machine appears online in **Machines** when the channel is up. Choosing it opens **Device →
Overview**.

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
- `device/device/src/panel.rs` — the device's own panel, rendered in Rust from the same design tokens.

## License

See `LICENSE`.
