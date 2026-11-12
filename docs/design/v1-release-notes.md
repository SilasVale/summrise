# Summrise v1 — release notes

Status: **v1 implemented on `hub/v1`, verified on the Linux build host; the Windows packaging rehearsal
is the one open item.** This page is the operator's summary. The evidence lives in
[`v1-verification.md`](v1-verification.md); the remaining procedure is
[`windows-handoff.md`](windows-handoff.md); the running record is
[`program-ledger.md`](program-ledger.md).

## What v1 is

One MCP endpoint per machine, a console that shows and reverses what ran, plugins when a capability is
missing, and a journal that keeps the record. The architecture — and why each boundary exists — is
[`architecture-of-record-2026-11-12.md`](architecture-of-record-2026-11-12.md).

It ships as four executables and three library crates:

| piece | what it is |
|---|---|
| `summrise-launch` | the Windows service supervisor; owns restarts, the baseline and the swap |
| `summrise-device` | the hub: MCP tools, the plugin host, the panel, the journal |
| `summrise-console` | the operator's surface: fleet, Activity, tokens, proxy, page |
| `summrise-shell` | the tray and the window; opens the console, or the device's `/panel` when it does not answer |
| `summrise-bus`, `summrise-plugin-host`, `summrise-console-page` | the journal, the supervised plugin host, the page role — linked in, not shipped separately |

## The operator journey

1. **Install.** On Windows, `SummriseSetup-<version>.exe` (silent: `/S /D=C:\Summrise`) registers
   `SummriseLaunch`; the installer workflow installs and uninstalls it on a clean runner on every
   build. Build it with `gh workflow run installer.yml -f version=… -f channel=… -f port=…`.
2. **Enroll.** The installer seeds the first generation; a device without a console can be pointed at
   one with `--console-url` and the one-time code (`console-bootstrap.json`), which it consumes and
   replaces with its own `console-token.json`.
3. **Connect.** In the console's Connect sheet, mint a token for the machine. It allows destructive
   tools by default — one click turns that off — and the sheet says so.
4. **See.** Activity lists every call the console proxied; the device's own journal keeps the rows a
   panel or a local client made, and its Device page carries the machine's session and plugin events.
   A console that reconnects after a long gap says how many rows it lost and rereads.
5. **Stop and trust.** Revoking a token, repeating a refused call, restarting a device or a plugin, and
   the audit rows that name the caller are the reverse gear. Plugins are trusted code (decided, §0.3)
   and are supervised for availability, not isolated.

## How it is verified

- **One command:** `scripts/check.sh` runs the CI device job's portable half — tests, clippy, fmt, the
  layer gate, the device smoke, the product smoke and the enrol smoke. `--fast` skips the slow parts.
- **The product loop:** `scripts/smoke-local.sh` (device alone: panel, MCP, audit, journal) and
  `scripts/smoke-enroll.sh` (real console + real device: enrol, online, token, call through the
  console, both audits).
- **The update path:** `scripts/rehearse-release.sh` signs a canary, serves it on loopback, and
  `verify-release.sh --device` proves the built device accepts the signed document — a tampered
  document fails with `the release signature does not verify`.
- **Numbers at the last sweep:** 1 487 Rust tests, 1 182 page tests, the gate at 14 packages / 22
  edges / 0 upward. See `v1-verification.md` for the command-by-command record.

## Known boundaries (deliberate, or still open)

- **Windows packaging is not yet exercised end to end.** The installer workflow does build, silently
  install, health-check and uninstall on a runner; the interactive `--check-window`, the visual panel
  fallback and Authenticode verification need a person at a machine (`windows-handoff.md`).
- **`GET /api/panel` is loopback-only, not panel-token-gated** (the mutating panel routes are). With one
  operator on their own machine that is the right trade; it is not a boundary that survives a second
  user, which the product does not claim.
- **The NSIS installer is not byte-reproducible** (timestamped), so verification checks the digest the
  document names, not a rebuilt installer.
- **The console's audit window is 8 MiB with one generation**, smaller than the device's 64 MiB by
  design; the page states the file's lifetime and the window's span rather than pretending the windows
  are equal.
- **`/api/panel` aside, the page holds no keys** — that is a crate boundary
  (`summrise-console-page` has no workspace dependency) plus a guard test.

## Where to start reading

`docs/PRODUCT.md` for what the product is; the architecture of record for why it is shaped this way;
`program-ledger.md` for what shipped when and with what evidence.
