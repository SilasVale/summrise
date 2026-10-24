# The per-user helper: the half of §4.1 that was not built

> **Status: design, 2026-10-24.** `2026-10-20-the-two-runtime-architecture-design.md` §4.1 names "one
> optional per-user helper, started at logon, for the things that must run in the interactive session:
> launching the user's Chrome for CDP, and showing the tray" — and then says "The helper is not built
> in this tree." The tray half is now `summrise-shell` (built, deployed on a canary, tray and window).
> This file is the contract for the other half.

## Why a helper at all

The app runs as SYSTEM in session 0. That is not an accident: it is the launcher's child, the launcher
is the SCM service, and the service is what installs, swaps and repairs the binary. The cost is
Windows' session isolation: a service has no desktop, so it cannot show a tray icon, cannot open a
folder dialog (measured in `2026-09-30-the-workspace-registry-design.md`), and cannot drive the
browser a person is looking at. The helper is the narrow process that lives in the user's session and
does only what session 0 cannot.

## What it may do, and what it may not

| may | may not |
|---|---|
| show a tray icon and the console window | run a command, or open a terminal |
| launch a browser **as the logged-in user** | read the device's data directory, token or keys |
| report that browser's CDP port | start, stop, update or repair the device |
| say whether the app is serving (the loopback health document) | act on a grant, or mint one |
| capture the user session's screen, when it is unlocked | capture a locked or disconnected session, or bypass the lock |

The helper holds **no device credential**. Everything an operator does to a device still goes through
the console, a grant, and the device's own authorization; the helper only decides *where* a browser
runs. That is the same boundary the tray already keeps, extended by one verb.

## The channel: loopback HTTP with a per-boot token (not a named pipe)

§4.1 sketched a named pipe. This design uses the loopback HTTP the device already serves, for two
reasons and one cost:

- **The device is `#![forbid(unsafe_code)]`.** A named pipe whose DACL admits a non-admin user needs
  `CreateNamedPipeW` with a security descriptor — unsafe by construction. The device's own MCP
  surface already solves "loopback plus a bearer token" in safe Rust; the helper reuses the pattern
  rather than opening a second unsafe seam.
- **The token is per boot and written by the device.** At startup the device mints 32 random bytes,
  writes `C:\ProgramData\Summrise\helper.token`, and keeps the value in memory. `ProgramData`'s
  inherited ACL gives users read (not write), so the logged-in user can hand the token back but cannot
  forge one.
- **The cost**: any local process can *reach* the endpoint; the token is what it must present. That is
  the same trade the loopback MCP already makes, and for the same reason: the reachability is the
  point, the token is the gate.

### The two calls

```
helper → device   POST http://127.0.0.1:18090/helper
                  Authorization: Bearer <per-boot token>
                  { "port": 49812, "pid": 2260 }
                  → 204, and the device now knows where the helper listens

device → helper   POST http://127.0.0.1:<port>/launch
                  Authorization: Bearer <per-boot token>
                  { "port": 51234 }            // the CDP port the device chose
                  → { "ok": true, "pid": 5120 } or { "ok": false, "error": "chrome not found" }
```

The device chooses the CDP port (it already does this for its own launch: bind `127.0.0.1:0`, read the
port, release it) and waits for `/json/version` exactly as it waits today. The helper's job is one
`CreateProcess` in the user's session and one answer.

## Which browser

The helper launches Chrome with **a profile of its own**:

```
chrome.exe --remote-debugging-port=<port> --user-data-dir=%LOCALAPPDATA%\Summrise\chrome
```

Not the user's default profile. Two reasons, in order:

1. **Chrome refuses it.** Recent Chrome releases reject `--remote-debugging-port` together with the
   default user-data directory — the "remote debugging is not allowed with the default user data
   directory" change — so "drive the profile the user is signed into" is not a thing the browser
   offers any more.
2. **A dedicated profile is the honest product story**: the agent gets a browser it owns, the user can
   open it on their desktop and sign into what the agent needs once, and the same signed-in state is
   there on the next device restart. It is inspectable, and it is not the user's private browsing.

## Degrading, never breaking

If the helper is not running, the device launches its own headless Chrome under the service, exactly
as it does today. `browser.*` answers either way; the difference is whether the browser is visible in
the user's session and whether it persists across a device restart. The helper is optional in the
architecture, and the code must treat "no helper" as normal rather than as an error.

## What would make this document wrong

- If Chrome ever lets CDP attach to the default profile again, the "sign in once" story gets stronger
  (the user's own logins), and the dedicated profile becomes a choice rather than a constraint.
- If a device is used by several people at once, "the user" is ambiguous and the helper needs one
  instance per session, with the device asking the right one.
- If the console learns to show a live view (screencast), the visible-browser half of the helper's
  value shrinks — the helper would still be the only way to run a browser with the user's GPU and
  audio, but "watch it from the console" would no longer require a local user.

## The implementation, in four commits

1. **Identity and registration** — device mints the token and serves `POST /helper`; the helper reads
   the token, starts its loopback server, and registers. No browser yet; the device ignores a
   registered helper.
2. **The launch verb** — helper `/launch` starts Chrome with the dedicated profile and answers; the
   helper's own tests cover the port argument and the refusal paths.
3. **The preference** — `browser.*` asks the helper first and falls back to the service launch, with
   the fallback proven by a test that runs with no helper registered.
4. **`screen.capture`** — the helper captures its session's desktop and answers with a PNG; the device
   exposes it as a tool, so an operator or an agent can verify a UI without a person sitting at the
   machine. A locked or disconnected session is a typed refusal, not a black image: Windows does not
   render a desktop nobody is looking at, and a tool that returned one would be lying.

All four are built (2026-10-24): `device/shell/src/helper.rs` (identity, launch, capture),
`device/device/src/helper.rs` (token, registration, capture client), the `screen` capability in the
registry, and the browser preference in `capabilities/browser.rs`. What is not yet done is the
deployment that puts the new device and helper binaries on the canary and the on-device verification
of the capture, which needs an unlocked session by definition.

The fourth exists because the first three cannot be verified from session 0. On 2026-10-24 the tray
could be launched into the user's session from a SYSTEM process (WTSQueryUserToken +
DuplicateTokenEx + CreateProcessAsUser, with the two privileges enabled and the device's job object
escaped), but its screen could not be read: the RDP session was not an active desktop, and
`CopyFromScreen` answered "invalid handle". That is the exact gap `screen.capture` closes, and the
exact limit it must report.
