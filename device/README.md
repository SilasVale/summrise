# The device runtime

The two-runtime architecture
(`docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md`) is implemented here
through phase 4: the supervisor, the app, the capability layer, the console and the update path. The
existing `agent/` tree is untouched and still serves the fleet until the phase-5 cutover deletes it.

Six workspace members, six owners:

| crate | what it owns | what it may write |
|---|---|---|
| `core` | every decision that is a value (state schema, `authorize`, session transitions, grants, the registry) | nothing — no I/O |
| `release` | the signed release document and its channels; **the one update decision** (`select`); the offline `summrise-release sign` tool | nothing at runtime |
| `launch` | the app binary and its baseline; the SCM service; the swap; the repair log | `<install>/summrise-device.exe`, `<install>/baseline/{current,previous,pending}/*`, `<install>/swap.result.json`, `<install>/repair.log`, `<data>/token`, `<data>/console.json` (console **and** mesh facts), `<data>/launcher.pid`, `<data>/app.pid` |
| `device` | the running app: sessions, HTTP/MCP, console and mesh channels, update staging | `<data>/state.json`, `<data>/device.lock`, `<data>/device.pid`, `<data>/identity.key`, `<data>/console-token.json`, `<data>/console.json` (the console half; mesh fields are preserved), `<install>/summrise-device.new.exe`, `<install>/summrise-device.new.json`, `<install>/swap.request.json`, `<data>/events.jsonl`, `<data>/memory.jsonl`, `<data>/runs.jsonl`, `<data>/known_hosts.json`, `<data>/monitors.json`, `<data>/workspace/` |
| `console` | the operator's control plane: the embedded operator page, auth, device registry, grant signing, the device channel, the `/mcp` proxy and `/mcp/all` aggregate, the audit index | `<data>/devices.json`, `<data>/codes.json`, `<data>/audit.jsonl`, `<data>/denials.jsonl` |
| `mesh` | the broker/relay library and its two binaries | nothing under the device's install or data directory |

**The two one-writer rules are the design.** Only the serving `device` app writes the state document
(the stager writes files, never the document); only `launch` writes the app binary. If a change needs
a second writer, the design is wrong, not the rule.

The capability registry declares **28 tools** across `files`, `memory`, `terminal` (PTY, SSH and
serial behind `terminal.create`), `browser`, `monitor`, `runs` and `state`.

The baseline is three generation directories, never a flat pair:

```text
<install>\baseline\current\   summrise-device.exe + baseline.json   the pair that boots
<install>\baseline\previous\  summrise-device.exe + baseline.json   the pair a failed update returns to
<install>\baseline\pending\   summrise-device.exe + baseline.json   staged, not yet proven by health
```

## Run it

```bash
cd device
cargo test --workspace          # the Linux suite; the Windows lifecycle test is a no-op here
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

cargo xwin check --target x86_64-pc-windows-msvc --workspace --all-targets
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
```

The Windows lane (`device (the Windows lifecycle)` in `ci.yml`) is the acceptance test: it builds with
**two distinct fixture keys**, runs `summrise-launch run`, kills the supervisor (the child must die
with it), deletes the app binary (the launcher restores it from the baseline), stages a validly signed
broken update (the swap rolls back byte-identically), and installs/starts/stops/deletes the service.
Every test that needs the real SCM service, or a different build (the unkeyed rebuild), is `#[ignore]`d
and run by exact name from its own CI step: the `sc.exe` service mode, the product installer's
baseline seed, the launcher-key-signed seed with a **release-key** refusal (the two-key seam), the
token mint plus its `sc qfailure` restart-policy assertion, and the placeholder-key refusal. A skipped
step therefore fails the job rather than passing it.

## Install, migrate and supervise

`summrise-launch --install-service` gives an install its first bootable generation **before** it
registers anything:

1. `baseline\current\` is already a complete pair (sidecar parses, binary hashes to it) — leave it
   alone;
2. the pre-generation flat pair `baseline\summrise-device.exe` + `baseline\summrise-device.json`
   exists — copy both into `current\`, renaming the sidecar to `baseline.json`, and verify the sha;
3. otherwise seed `current\` from the installer's `<install>\summrise-device.exe` plus the sidecar
   passed with `--seed-sidecar <path>`, verifying the signature with the embedded **launcher** key
   and the binary's sha256.

If none of the three is possible the installer exits non-zero and registers nothing. An install that
started a service into `NeedsHuman` would be an install that reported success and served nothing.

There is one supervisor per install. `run` and the service both take a `Global\` named mutex keyed by
a digest of the install path; a second supervisor exits non-zero and names the install instead of
racing the first one through the swap transaction. The mutex name lives in the global namespace, so
the console supervisor needs the privilege the service already has (Administrator or SYSTEM).

There is also one **serving app** per data directory. Before its first read or write of
`state.json`, `summrise-device` takes an advisory lock on `<data>/device.lock` (`flock` on Unix,
`LockFileEx` on Windows, through `std::fs::File::try_lock`) and records its pid in `<data>/device.pid`
(the pid record is diagnostic, never the lock: a Windows lock is mandatory, so the refused process
could not read the locked file's own bytes). A second `run` on the same `<data>` exits non-zero and
names the data directory and the holder's pid instead of racing the first process's read-modify-write
of the document. The kernel releases the lock when the holder exits, including on a crash, so a stale
pid cannot keep the next start out. The hidden `--stage` and `--check-update` modes are short-lived
helpers, not the serving app, so they may run beside the service — the canary update path depends on
that.

The launcher starts the child with `--install`, `--data`, `--port`, `--token-file` and the three
release facts from the booted baseline sidecar: `--release-version`, `--release-sha` and
`--release-channel`. The app records those facts in `state.json` and `/api/health` instead of
re-deriving a version (or a constant channel) from its own compile time.

## Enrolment (`--enroll`)

A device registers itself with the console before the supervised install:

```bash
summrise-device --enroll <console-url> <code> [--data <data>] [--install <install>] [--force]
```

`<code>` is the one-time code the console mints (`POST /enroll` as the operator; 15 minutes, one
use). The mode loads or mints `<data>/identity.key` — the same key the mesh names — signs
`register\n{id}\n{code}\n{ts_ms}`, and posts the body to `<console-url>/devices`. On a 200 it writes
two files:

- `<data>/console-token.json` — the console's `{"id":"…","token":"…"}`, owner-only (0600 on Unix;
  on Windows a protected DACL for `SYSTEM` and `BUILTIN\Administrators` only, read back and refused
  if it is looser). The mode protects `<data>` before it writes either credential, so files created
  later inherit the same DACL; `--install-service` hardens the directory as well, for installs that
  ran the installer first;
- `<data>/console.json` — merges `url` and `token_file` into the existing link, keeping any
  `mesh_broker` / `mesh_relay` fields (and every other field) already there.

Both are written atomically through a random temporary sibling and a rename. `<data>` defaults to
`<install>/data`, with `<install>` from `--install`, `SUMMRISE_INSTALL`, or the directory the binary
lives in, so an installed `summrise-device --enroll <url> <code>` needs no path flags.

The console URL is `https` unless the host is loopback (`127.0.0.1`, `::1` or `localhost`): the
registration carries the one-time code and the answer carries the long-lived channel token, so plain
`http` to a remote console is refused as a usage error. A userinfo component is refused too — the base
is persisted in `console.json` and printed. Redirects are not followed at all, so a console that
answers one is reported as a status, never forwarded the code.

Exit codes are the mode's contract: **0** enrolled, **2** usage (a missing argument, a non-https
non-loopback URL, or a URL with userinfo/query/fragment), **1** every refusal — a spent code, an
absent or unreadable console, a signature the console refuses, or an existing token file without
`--force`. The token-file
check runs **before** the request: a registration rotates the token and spends the code, so a second
enrolment without `--force` is refused while both are still untouched.

## The remote link (`console.json`)

A supervised app cannot be given its remote flags on its command line: the SCM starts a fixed argv.
The launcher therefore persists them as `<data>/console.json` (mode 0600 on Unix, written
atomically), and `--enroll` writes the same console half before an install by merging into the file
rather than replacing it. The file keeps its old name and carries both features:

```json
{
  "url": "https://console.example.test",
  "token_file": "/var/lib/summrise/console-token.json",
  "mesh_broker": "http://127.0.0.1:8080",
  "mesh_relay": "127.0.0.1:9000",
  "mesh_stun": "203.0.113.10:3478"
}
```

Every field is optional, and an absent one is **omitted**, never written as `null`: a console-only
install writes `url` and `token_file`, a mesh-only install writes `mesh_broker` and `mesh_relay`, and
an install with neither writes no file at all. The console pair is all-or-nothing, and a relay or a
STUN reflector without a broker is refused: the relay is the floor a rendezvous leads to, and a
reflection is an address the rendezvous publishes. The launcher checks the broker is an
`http://host:port` origin and the relay and STUN reflector are `host:port` addresses before writing
them, so a supervised install does not register a service that can only fail at startup. It passes
none of those facts to the child. `--enroll` preserves `mesh_stun` when it merges the console half,
and a launcher rewrite that names the mesh but not a reflector carries the installed one forward, so
a broker/relay update cannot silently disable the direct path.

Resolution is per feature, and the order is **flag → environment → file**:

| feature | flag | environment |
|---|---|---|
| console | `--console-url` + `--console-token-file` | `SUMMRISE_CONSOLE_URL` + `SUMMRISE_CONSOLE_TOKEN_FILE` |
| mesh | `--mesh-broker` + `--mesh-relay` (optional `--mesh-stun`) | `SUMMRISE_MESH_BROKER` + `SUMMRISE_MESH_RELAY` (optional `SUMMRISE_MESH_STUN`) |

`--mesh-stun <addr>` makes the join ask a STUN reflector on the same UDP socket QUIC will own, before
the endpoint is built: a NAT mapping belongs to that socket's 5-tuple, so a reflection asked on a
separate socket would name a port QUIC is not listening on. An unspecified bind (`0.0.0.0` or
`[::]`) is never a candidate: with `--mesh-stun` the reflected address is the published candidate;
if the reflector is silent, the join falls back to the local address the kernel would use to reach
it, with the QUIC socket's own port. A reflector that is silent, dead or malformed is one log line
— it costs the direct path, not the session. Naming a reflector also moves the mesh socket off
loopback (it binds `0.0.0.0:0`), because a loopback-bound socket's reflection is `127.0.0.1` and no
other machine can use it; without `--mesh-stun` the socket stays on `127.0.0.1:0` and a peer
elsewhere uses the relay floor.

The app reads the file at start **only** for a feature whose own flag and environment are both
absent. Naming one feature does not name the other: a console flag beside a mesh-only file arms both,
from their respective sources, and a mesh flag never grafts the file's relay or reflector onto the
broker the operator named. When the file is absent, both remote paths stay off and nothing changes.
An unreadable link, one that is group/world-readable (it names a credential path), one that names
half a console pair or a relay/reflector with no broker, and one that names no fact at all are each a
startup error naming the file — not a device that quietly serves without a console or without a
mesh. Re-running `--install-service` without the flags leaves an existing link in place; removing
the file is how the operator disarms it.

## Health is readiness, not liveness

`GET /api/health` re-reads `<data>/state.json` on every probe:

- readable document (an absent one is a fresh document, not a failure) → `200
  {"status":"ok","version":"<release>","channel":"<stable|canary>","pid":<pid>}`;
- unreadable document (torn, or a schema this build cannot parse) → `503
  {"status":"degraded","reason":"state: <error>"}`.

The app's own watchdog and the launcher's two swap probes require a 200 whose body says
`"status":"ok"`; anything else is a failed probe, and two consecutive failures make the app exit so
the supervisor restarts it. A device that cannot read its own state document is therefore not healthy,
and it recovers without a restart the moment the document is replaced.

## The operator page (`GET /`)

The console serves one page at `/`: `console/src/page.html`, embedded with `include_str!` so the
binary carries its own UI and there is no build step. Its CSS and JavaScript are inline and it uses
no framework.

The page asks for the operator token, exchanges it at `POST /auth`, and keeps the session token in a
JavaScript variable. Nothing is written to browser storage; a comment in the page says the token
stays in memory, and the integration test asserts the page contains no token and no storage call.

The page's **AI client** card creates one-device tokens. A token can also name several devices
(`POST /tokens` with `device_ids`), and `POST /mcp/all` is the one standard MCP endpoint those
tokens use: every tool takes a required `device` argument and every call still becomes an ordinary
per-device sixty-second grant. The Security page's **Fleet tokens** section creates and revokes
these tokens; the per-device card is unchanged.

It lists `GET /devices` with `last_seen_ms` rendered as an age, builds a grant form from a hardcoded
list of the registry's 28 tool names (`POST /grants`), calls `state.get` through `POST /mcp` with
that grant and pretty-prints the answer, and shows the newest 20 lines of `GET /audit?limit=20`. The
page's tool list is compared with `summrise_core::registry` by a test, and every path in its route
table is probed against the router, so a renamed tool or a missing route fails CI rather than a
browser.

## Reading a device's state

The console has no state route of its own, by design: the operator page at `/` reads a device by
calling `/mcp` with a grant naming `state.get`. That tool (capability `state`, non-destructive)
returns the same document `/api/state` would answer plus `health` — `status` (`ok` or `degraded`),
the release `version`, the serving `pid`, and `poisoned`, which is `true` when `state.json` cannot be
read. A poisoned document is a successful call with `state: null`, `health.status: "degraded"`,
`health.poisoned: true` and a `reason` naming the parse error, so the console can show the failure
instead of rendering the device as offline. The call goes through the console's grant check and
lands in its audit index like every other tool.

## Updates

The stager writes files; the app writes the document; the launcher swaps. `--stage` and
`--check-update` verify the release, write the canonical staged pair and `<install>/swap.request.json`
— and **never write `state.json`**, so they can run while the service serves (and even against a
read-only document). `--check-update` **reads** it for the installed version and the rollback memory;
it refuses (exit 2) when there is no state document to read rather than treating the memory as empty
(a document that exists but cannot be parsed is a failed attempt, exit 1). `--stage` reads it only when `--data` is given: a digest in `rolled_back` is refused (exit 4)
unless `--force-reinstall <sha256>` names exactly that digest, and the override is appended to
`<data>/events.jsonl` before staging. Without `--data`, `--stage` cannot see the memory and stages —
the guard is opt-in by naming the state document, and the mode's help says so. The serving app reads the request at start and every second
(`SWAP_POLL_INTERVAL`; the supervisor consumes the request on its own two-second poll) and records
`update.phase = "staged"` / `update.to`. A request naming a version the document already records is
not recorded again, so a lingering request cannot flip a finished update back to `staged`.

The same poll folds `<install>/swap.result.json` into `update.outcome` / `update.lastResultAtMs` and
`lastVerdict` (`updated to X`, `rolled back from X`, `repaired from baseline`, or `refused: …`).
`lastResultAtMs` is what makes that idempotent across restarts; a genuine repair is also in
`<install>/repair.log`, and its event is `repaired_from_baseline` in `<data>/events.jsonl`.

Every start also rewrites the release facts from the booted sidecar: `install.version`,
`baseline.{version,sha256}` and `install.channel`. A device updated to a canary build therefore
records `install.channel = "canary"` (and `/api/health` says `"channel":"canary"`), and a restart
under a stable sidecar records `stable` again — the channel is a fact, not a constant.

`summrise-release::select` is the **only** update decision:

- a **stable** device refuses a **canary** document;
- a **canary** device accepts both channels;
- a version **older** than the installed one is a downgrade (exit 4);
- a version **equal** to the installed one is `UpToDate`: `--check-update` writes no swap request,
  changes nothing, prints `up_to_date <version>` and exits **0**, so re-running a check is a safe,
  successful no-op.

`summrise-device --check-update` exits 0 for `applied` or `up_to_date`, 3 for `rolled_back`, 4 for a
refused document, a remembered digest, or a refused swap, 1 for a failed attempt, and 2 for a usage
error (including a state document it cannot read). A rolled-back artifact is refused by digest, not by
version: the same version rebuilt with a different digest is allowed. The operator-only
`summrise-launch --apply` is the last resort: a digest this device rolled back is named in a warning
and audited as `operator_apply_rolled_back` in `<data>/events.jsonl`, and the transaction continues.

## Signing keys are compile-time

Three public keys are embedded; each fails closed on the all-zero placeholder, and none has a runtime
override.

| build variable | embedded in | verifies |
|---|---|---|
| `SUMMRISE_LAUNCHER_PUBKEY` | `summrise-launch` | the seed sidecar and the baseline generation — the launcher is never replaced by an update, so changing this key is a full-reinstall boundary |
| `SUMMRISE_RELEASE_PUBKEY` | `summrise-device` **and** `summrise-launch` | every `release.json` (policy and artifact signature) and the staged `<install>\summrise-device.new.json` the launcher checks before a swap |
| `SUMMRISE_CONSOLE_GRANT_KEY` | `summrise-device` | operator grants signed by the console (the **public** key is compiled in; the seed stays with the console's `--grant-key-file`) |

A release document signed with the release key cannot seed the baseline, and a sidecar signed with
the launcher key cannot be applied as an update. That split is the point of the first two keys.

## Not here yet

- **The mesh has no console surface.** A device joins the mesh when `--mesh-broker` is configured,
  serves the same MCP frame the console sends, and writes `<data>/identity.key` as its peer id. The
  console does not yet show mesh peers or dial one on the operator's behalf, so remote operator
  traffic still goes through the console WebSocket.
- **A reflection is a hypothesis, not a guaranteed path.** `--mesh-stun` publishes the address the
  reflector saw on the socket QUIC owns; whether a direct path opens still depends on both peers'
  NATs (a per-destination NAT only forwards an inbound datagram from a source it has a mapping for),
  and the relay floor remains the fallback. The STUN exchange is one request with a two-second
  deadline at join time, never a per-connection cost.
- The console has no state route: `/`, `/health`, `/auth`, `/enroll`, `/devices`,
  `/devices/:id/channel`, `/grants`, `/mcp` and `/audit` are its whole surface. The page at `/`
  reads a device through the `state.get` tool over `/mcp` (see “Reading a device's state”); adding a
  console state route would be a second data path and a second thing to authorize.
- The console does not generate the device's identity: `--enroll` uses `<data>/identity.key`, which
  is also the device's mesh peer id. A hand registration can use any key (the channel authenticates
  with the returned token), but only `identity.key` is the mesh name.
- `agent/` is untouched.
