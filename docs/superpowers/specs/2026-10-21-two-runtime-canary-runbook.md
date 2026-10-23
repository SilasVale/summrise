# The two-runtime canary — the operator's runbook

> **What this proves.** The cutover plan (`2026-10-21-phase-5-cutover-plan.md`) says a deletion needs a
> canary week with named evidence. This is the sequence that produces it. Every step prints or writes
> something that can be pasted into the ledger; a step that "seemed fine" is not evidence.

## 0. What you need

- A Windows box you can afford to break, with an admin shell.
- The binaries built from `device/` for `x86_64-pc-windows-msvc`: `summrise-launch.exe`,
  `summrise-device.exe` and `summrise-release.exe` (the offline signer). The seed artifact is **the
  same** `summrise-device.exe`.
- **Three keys, generated once and kept offline.** They are not interchangeable, and each binary
  embeds the keys it checks at build time:

  | key | build variable / signer flag | signs |
  |---|---|---|
  | **launcher** | `SUMMRISE_LAUNCHER_PUBKEY` (launcher build); `summrise-release sign --key-file <launcher-key-file>` | the **seed sidecar** (`--seed-sidecar`), and therefore the first `baseline/current` generation. Never rotated by an update: changing it is a full reinstall. |
  | **release** | `SUMMRISE_RELEASE_PUBKEY` (device and launcher builds); `summrise-release sign --key-file <release-key-file>` / `summrise-release revoke --key-file <release-key-file>` | every `release.json` (the policy signature **and** `artifactSignature`), the staged `<install>\summrise-device.new.json` the app writes from it — the sidecar the launcher verifies before an update — and every `revocations.json` withdrawal list. |
  | **console grant** | `SUMMRISE_CONSOLE_GRANT_KEY` (device build) | operator grants, signed by the console; the device verifies them offline. This build variable is the **public** key's hex, not the seed. |

  Each `<…-key-file>` is a **file** holding the 32-byte ed25519 seed as **64 hex characters**, mode
  `0600` where Unix modes exist; the signer refuses a key file another user can read. The key never
  appears in the command line. The launcher embeds **both** the launcher and the release key: the
  first generation is signed by one, updates by the other. A release document signed with the release
  key cannot seed the baseline, and a sidecar signed with the launcher key cannot be applied as an
  update — that split is the point of the two keys. Both binaries fail closed on a placeholder key.
- The console running (`summrise-console --data <dir> --operator-token-file <file> --grant-key-file
  <file>`) on `127.0.0.1:18082` by default; `--bind` and `--port` move it. The operator-token file
  holds 64 hex characters; the grant-key file holds the 32-byte seed as hex, mode `0600`.

## 1. Sign what each key owns

**A release document (updates), signed with the release key.** The document carries two signatures:
`signature` over the policy (`version`, `channel`, `sha256`, `artifactUrl`, `minVersion`, `issuedAtMs`)
and `artifactSignature` over the artifact (`version`, `channel`, `sha256`). Do not hand-edit it; a
hand-edited document fails the policy signature, which is the point.

```bash
summrise-release sign \
  --key-file <release-key-file> --artifact summrise-device.exe \
  --version 0.1.0 --channel canary --url https://<host>/summrise-device.exe \
  --min-version 0.1.0 --out release.json
```

**The seed sidecar (the first generation), signed with the launcher key.** There is no separate
signer: `artifactSignature` is exactly the sidecar message `"{version}\n{channel}\n{sha256}"`, so the
release CLI produces the signature the launcher verifies when it is given the *launcher* key. Run it
a second time against the seed artifact, then write the sidecar beside the app:

```bash
summrise-release sign \
  --key-file <launcher-key-file> --artifact summrise-device.exe \
  --version 0.1.0 --channel canary --url https://<host>/summrise-device.exe \
  --min-version 0.1.0 --out seed.json
```

`summrise-device.sidecar.json` (in the install root):

```json
{
  "version": "0.1.0",
  "channel": "canary",
  "sha256": "<seed.json -> sha256>",
  "signature": "<seed.json -> artifactSignature>"
}
```

A sidecar whose `signature` came from the release-key invocation is refused by the launcher with a
message naming `SUMMRISE_LAUNCHER_PUBKEY`; if you see that, re-run the signing command with the
launcher key.

**A revocation list (withdrawing a release), signed with the release key.** A published document
stays valid for 180 days and must not be hand-edited, so withdrawing an artifact is a second signed
fact: `revocations.json`. Start a list from the document (or from an explicit version and digest),
then append to the same file. `--from` must name the existing list or the command refuses to
overwrite it; `--out` is always written through a temporary sibling and a rename.

```bash
# start a list from the signed document
summrise-release revoke \
  --key-file <release-key-file> --document release.json \
  --reason "boot loop on Windows 11" --out revocations.json

# append a second withdrawal to the same list
summrise-release revoke \
  --key-file <release-key-file> --from revocations.json \
  --version 0.1.1 --sha256 <64-hex digest> \
  --reason "installer fix needed" --out revocations.json
```

The list is `{"issuedAtMs":…,"revoked":[{"version":…,"sha256":…,"reason":…}],"signature":…}` and
the signature covers exactly `"revocations\n{issuedAtMs}\n"` followed by one
`"{version}:{sha256}:{reason-or-empty}\n"` line per entry, in order, with lowercase hex. **The
trailing newline is part of the signed bytes**, including for an empty list; an absent or empty
reason is the empty field, so the line ends `:{sha256}:\n`. The reason is signed whole, so a host
cannot rewrite the text the operator reads; a reason containing a line break is refused because it
would split the signed entry. The CLI refuses a duplicate `(version, sha256)` and a key file that is
not `0600`. **Re-sign the list before it is 365 days old** (`revoke` re-signs the whole list, so one
append refreshes it); once it is older, every update refuses until a fresh list is published.

**Where the list must live.** By default the device reads `revocations.json` in the directory of the
document's `artifactUrl` — the artifact's sibling, not the document's. For
`--url https://<host>/summrise-device.exe` that is `https://<host>/revocations.json`; publish the
signed file there, beside the artifact the document names. If the artifact lives in a versioned
directory, put the same signed list in that directory too, or pass `--revocations <url>` to
`--check-update` to name one shared list. A list on another host must be reachable over `https` (or
loopback `http`, the same rule as the document).

## 2. Enrol the device at the console

Do this **before** installing the service: `--install-service` leaves the console link alone and the
service starts the app immediately, and an app whose console link names a file that does not exist
yet is a startup error.

Put `summrise-device.exe` in the install root first (`D:\Summrise`; §3 installs the same file), then
mint a one-time code as the operator. The console listens on `127.0.0.1:18082` by default; exchange
the operator token for a session, then ask for a code (15 minutes, one use):

```bash
# operator: the token is the content of --operator-token-file, 64 hex characters
curl -sX POST http://127.0.0.1:18082/auth \
  -H 'Content-Type: application/json' -d '{"token":"<operator-token-hex>"}'
# -> {"token":"<operator-session>","expires_at_ms":…}   the session lasts 24 h, in memory only

curl -sX POST http://127.0.0.1:18082/enroll \
  -H "Authorization: Bearer <operator-session>" \
  -H 'Content-Type: application/json' -d '{}'
# -> {"code":"…","expires_at_ms":…}
```

Then run the device's one-shot enrolment mode:

```powershell
D:\Summrise\summrise-device.exe --enroll https://<console> <code>
```

`https` is the rule for a console that is not on this machine: the registration carries the one-time
code and the 200 carries the long-lived channel token. Plain `http` is accepted only for a loopback
console (`127.0.0.1`, `::1` or `localhost`); anything else is a usage error (exit 2). Redirects are
not followed, and a URL carrying `user:pass@` is refused.

The mode loads or mints `D:\Summrise\data\identity.key` (the same key the mesh names), signs
`register\n<id>\n<code>\n<ts_ms>` with it, and posts the registration to `<console>/devices`. On a
200 it writes the two files the service reads at start:

- `D:\Summrise\data\console-token.json` — the console's `{"id":"…","token":"…"}`, mode `0600` where
  Unix modes exist; on Windows the file and `<data>` get a protected DACL granting only `SYSTEM` and
  `BUILTIN\Administrators`, read back and refused if it is looser. `--install-service` protects
  `<data>` too, so a file created later inherits the same safe DACL;
- `D:\Summrise\data\console.json` — the console link `{"url":…,"token_file":…}`, merged into any
  mesh fields the file already carries.

It exits 0 on success, 2 for a usage error (a bad console URL or a missing argument), and 1 for a
refusal: a spent code, an absent or unreadable console, a signature the console refuses, or an
existing token file without `--force`. A second enrolment without `--force` is refused **before** the
console is contacted, because a registration rotates the token and spends the code.

> **The hand-signed path still works** on a machine that cannot run the binary yet (before the first
> install). Mint the code as above, then `POST /devices` with
> `{"id":"<hex>","public_key":"<hex>","code":"<code>","ts_ms":…,"signature":"…"}`, where `id` and
> `public_key` are the same 32-byte ed25519 public key in 64 lowercase hex characters and the
> signature is over `register\n<id>\n<code>\n<ts_ms>`. Write the answer to
> `D:\Summrise\data\console-token.json` as `{"id":"…","token":"…"}`, and write
> `{"url":"https://<console>","token_file":"D:\\Summrise\\data\\console-token.json"}` to
> `D:\Summrise\data\console.json` (create the data directory if this is a fresh tree). The channel
> authenticates with the returned token, so a hand registration may use any key; the mesh, however,
> names `identity.key`, so a hand-registered id is not a mesh peer id.
> **On Windows the app refuses a token file whose DACL allows anyone but `SYSTEM` and
> `BUILTIN\Administrators`**, so protect the hand-written tree before starting it:
> `icacls D:\Summrise\data /inheritance:r /grant:r "SYSTEM:(OI)(CI)(F)" "BUILTIN\Administrators:(OI)(CI)(F)"`,
> then the same for `console-token.json` and `identity.key` without `(OI)(CI)` (or run
> `--enroll`, which protects them itself).

## 3. Seed the first generation and install the service

The installer refuses to leave a device it cannot repair. §2 already put `summrise-device.exe` in
the install root and wrote the console link and token; put the **launcher-key-signed** sidecar beside
the exe from §1, then install **once**. The launcher leaves an existing console link alone, so the
service argv needs no console flags and **no second `summrise-device.exe` is started**.

```powershell
summrise-launch.exe --install-service --install D:\Summrise --data D:\Summrise\data `
  --seed-sidecar D:\Summrise\summrise-device.sidecar.json
```

Evidence: the command exits 0, `D:\Summrise\baseline\current\` holds a pair, `sc qfailure
SummriseLaunch` shows `3` restarts, `5000` ms, `3600` s, and `D:\Summrise\data\console.json` still
names the URL and token path §2 wrote. From here on, drive the device through the service and the
console; do not start a second app.

**The three release facts travel from the sidecar to the app.** The launcher starts the child with
`--release-version`, `--release-sha` **and `--release-channel`**, read from the booted
`baseline\current\baseline.json`. The channel is part of the signed message, so the app records the
real channel: after this install, `D:\Summrise\data\state.json` must say `install.channel =
"canary"` and `/api/health` must answer `"channel":"canary"`. A device whose sidecar has no channel
flag (a standalone development start) stays `stable`.

**One serving app per data directory.** The app takes an advisory lock on
`D:\Summrise\data\device.lock` before it reads or writes `state.json`, and records its pid in
`D:\Summrise\data\device.pid`. A second `summrise-device.exe run --install D:\Summrise --data
D:\Summrise\data` exits non-zero within a second and its message names the data directory and the pid
that holds the lock — that is the expected answer, not a bug; it prevents two processes from
overwriting each other's session rows and swap verdicts. The lock is released when the serving
process exits (including a crash), so a restart after a stop succeeds. The hidden `--check-update`
and `--stage` modes are short-lived helpers, not the serving app: they do **not** take the lock, and
§5 relies on running `--check-update` while the service serves.

## 4. The smoke list (each one is evidence)

| Step | Command | Evidence |
|---|---|---|
| alive | `curl http://127.0.0.1:18080/api/health` (twice) | each answer is `200 {"status":"ok","version":"0.1.0","channel":"canary","pid":…}`; a 503 with `"status":"degraded"` names the state error and is a failure |
| state | `curl -H "Authorization: Bearer <install-token>" http://127.0.0.1:18080/api/state` | `install.version`, `install.channel` = `canary`, `baseline`, `sessions`; the token is the content of `D:\Summrise\data\token` |
| MCP local | an MCP client at `http://127.0.0.1:18080/mcp` with the install token | `tools/list` = 28 tools; `state.get {}` returns the same state document plus `health` (`status`, `version`, `pid`, `poisoned`); `files.mkdir {path:"canary"}` + `files.write {path:"canary/mark.txt", content:"MARK"}` + `files.read {path:"canary/mark.txt"}` |
| PTY | `terminal.create {kind:"pty"}` → `id`; `terminal.write {id:"<id>", data:"echo MARK\n"}`; `terminal.read {id:"<id>"}` | `MARK` in the read output |
| browser | `browser.open {url:"data:text/html,<title>canary</title>"}` → `target`; `browser.eval {target:"<target>", expression:"document.title"}` | `"canary"` (a `data:` URL with no `<title>` has an empty title — do not use one) |
| ssh | `terminal.create {kind:"ssh", host:"<host>", port:22, user:"<user>", auth:{kind:"password", password:"<password>"}}` to a box you own | a prompt; the fingerprint stored in `known_hosts.json` |
| serial | `terminal.create {kind:"serial", port:"COM9"}` with a loopback plug | bytes in, bytes out |
| runs | `runs.exec {command:"cmd", args:["/c","echo hi"]}` | exit 0, `hi` |
| monitor | `monitor.add {kind:"tcp", host:"127.0.0.1", port:18080, interval_ms:15000}` → `id`; `monitor.probe {id:"<id>"}` | `ok:true` with a latency; `interval_ms` is required (minimum 5000) |
| remote MCP | operator mints a grant: `POST /grants` with `{"device_id":"<id>","tools":["files.read"],"expires_in_s":3600}`, then `POST /mcp` with `{"device_id":"<id>","grant":<the returned envelope>,"request":<the JSON-RPC call>}` and the operator session | the same tool answers; `<console-data>/audit.jsonl` has the call and the tool name |

### The console's device page is `state.get` over `/mcp`

The console has **no** `GET /api/devices/:id/state` and no SPA, and this runbook does not ask for
one. To see a device's state remotely, mint a grant naming `state.get` and call it through the same
console `/mcp` proxy as any other tool:

```json
POST /grants
{"device_id":"<id>","tools":["state.get"],"expires_in_s":3600}

POST /mcp
{"device_id":"<id>","grant":<the returned envelope>,
 "request":{"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"state.get","arguments":{}}}}
```

The result is one JSON document:

```json
{
  "state": { "schema": 1, "install": {"version": "0.1.0", "channel": "canary", …}, … },
  "health": {"status": "ok", "version": "0.1.0", "pid": 1234, "poisoned": false}
}
```

`state` is the document `/api/state` would answer. An unreadable (poisoned) `state.json` is a
**successful** call with `"state": null`, `"health": {"status": "degraded", "poisoned": true,
"reason": "state: …"}`: that is how “the device is up but its state is unusable”, a rollback, or the
wrong channel becomes visible to the console instead of looking like an offline device. A grant that
does not name `state.get` is refused before the tool runs, and the call is in
`<console-data>/audit.jsonl` like every other.

## 5. The update, and the rollback

`--check-update` (and `--stage`) never **write** `state.json`: they leave the staged pair and
`D:\Summrise\swap.request.json`, and the serving app records the stage. `--check-update` reads the
document (with `--data`) for the installed version and the rollback memory, and refuses (exit 2) when
there is none (a document it can open but not parse is exit 1); `--stage` reads it when `--data` is given, so a remembered digest is refused (exit 4) unless
`--force-reinstall <sha256>` names it. Run them while the service
serves — the app's one-second poll sees the request and sets `update.phase = "staged"` /
`update.to = "0.2.0"`; if the supervisor consumes the request before a poll, the document goes
straight to the outcome, which is still the same fact. A request for a version the document already
names is not recorded twice.

- **Canary update**: serve a signed `0.2.0` document and run
  `summrise-device --check-update http://<host>/release.json --channel canary --install D:\Summrise --data D:\Summrise\data`.
  Evidence: exit 0, `applied 0.2.0`, `/api/health` names `0.2.0` and `"channel":"canary"`,
  `state.json` says `install.version = "0.2.0"`, `install.channel = "canary"` and
  `update.outcome = "applied"` / `update.to = "0.2.0"` (with `update.phase = "staged"` visible while
  the swap is in flight), `baseline\current` names `0.2.0`, `baseline\previous` names `0.1.0`.
- **Re-run the same check after an applied update** (a retry, or the next morning): the document now
  names the installed version, so the mode prints `up_to_date 0.2.0`, exits **0**, and writes no swap
  request; the state document, the baseline and the running process are unchanged. That is a pass,
  not a refusal; a version *older* than the installed one is the exit-4 case.
- **Rollback**: serve a validly signed document whose artifact exits immediately (a copy of `cmd.exe`
  works) as `0.3.0`; the supervisor must answer `rolled_back`, the app hash must equal the `0.2.0`
  artifact byte-for-byte, and `/api/health` must still answer `0.2.0`. `swap.result.json` must carry
  the candidate's `sha256` (the launcher verified that artifact before the swap), and `state.json`
  must then say `lastVerdict.outcome = "needs_attention"` and `lastVerdict.detail = "rolled back from
  0.3.0"` — not "repaired from baseline" — and `rolledBack` must contain one entry naming the failed
  artifact's `sha256`, `version` and `atMs`. The digest comes from the result, so an operator's
  `summrise-launch --apply` rollback, or one whose staged pair was removed afterwards, is recorded
  the same way; only an older result without the field falls back to the staged sidecar.
- **Re-run after a rollback**: the app remembers the failed artifact by digest, so serving that exact
  document again is a policy refusal: exit **4**, and the message names the version and when it
  rolled back. The same version with a *different* digest is a rebuilt artifact and is allowed; the
  digest is the identity, not the version. An operator who has fixed the cause overrides exactly
  that digest:
  `summrise-device --check-update http://<host>/release.json --install D:\Summrise --data D:\Summrise\data --force-reinstall <sha256>`.
  The override is appended to `D:\Summrise\data\events.jsonl` as a `force_reinstall` line naming the
  version, digest and rollback time, and the update then follows the normal stage/swap path. If the
  audit line cannot be written, the mode refuses rather than overriding unrecorded.
- **Withdraw a release**: serve the signed `revocations.json` beside the artifact and run the same
  `--check-update` (optionally with `--revocations <url>`). A listed `(version, sha256)` is a policy
  refusal: exit **4**, the message names the version and the reason, and neither the artifact nor a
  swap request is written. A **404 or 403** for the list means nothing is published (an object store
  answers 403 when it will not GET an absent key); the mode logs one line and proceeds. A list that
  is present but malformed, badly signed, older than **365 days**, or
  issued more than ten minutes in the future is a **hard failure (exit 1)**, and nothing is staged;
  any other non-2xx from the list URL (5xx, 429) is that same hard failure and names the status.
  The same version with a different digest is a rebuilt artifact and is not refused. The check runs
  before the artifact download and before the version/channel decision, so re-checking a device that
  is already running a withdrawn release reports the revocation (exit 4) rather than `up_to_date`.
- **Repair**: delete `D:\Summrise\summrise-device.exe` and restart the service; within one launch the
  file is back and its sha256 equals the baseline's. The repair is a machine record, not a log line:
  `D:\Summrise\repair.log` gains one line
  `{"ts_ms":…,"kind":"repaired","from":"baseline","sha256":"<baseline sha>"}`, and after the app
  restarts `state.json` says `lastVerdict.outcome = "repaired"`, `lastVerdict.detail = "repaired from
  baseline"`, `repairOffset` is non-zero, and `D:\Summrise\data\events.jsonl` has a
  `repaired_from_baseline` event. A refused swap is **not** a repair and no longer claims to be one.

## 6. The week

Leave it running for seven days with the operator using the console as the daily path. Then collect:
`state.json` history, `swap.result.json`, `repair.log`, `<data>\events.jsonl`, the console's
`audit.jsonl`, and the list of every step above that needed a human.

## 7. Getting back

- **To the old runtime**: `summrise-launch --uninstall-service`, then install `1.2.513` from the
  archived npm tarball and its release assets. **Time this** — the cutover plan requires it to take
  under ten minutes, and the number goes in the ledger.
- **To the previous new build**: the same `--check-update` path with an older *signed* document is
  refused by design (no downgrade). The supported way back is the rollback path above (a build the
  device refuses to serve), not a version downgrade.
> On Windows the app refuses a token file whose DACL allows anyone but `SYSTEM` and
> `BUILTIN\Administrators`, so protect the hand-written tree before starting it:
> `icacls D:\Summrise\data /inheritance:r /grant:r "SYSTEM:(OI)(CI)(F)" "BUILTIN\Administrators:(OI)(CI)(F)"`,
> then the same for `console-token.json` and `identity.key` without `(OI)(CI)` (or run
> `--enroll`, which protects them itself).

## 9. The loop as it exists after the first canary week

The sections above are the manual sequence the first canary was built on. What is now the routine is
shorter, and each step has a command that produces evidence.

**Publishing is one command.** `scripts/publish-canary.bash` builds the runtime for
`x86_64-pc-windows-msvc`, signs the release document with the release key, signs the first-generation
sidecar with the launcher key, and builds the Windows installer — into one directory a mirror can
serve:

```bash
scripts/publish-canary.bash --version 0.2.7 --channel canary --port 18090 \
    --mirror http://<mirror-host>:18100 \
    --launcher-key <launcher.seed> --release-key <release.seed> --out /srv/canary
```

It emits `summrise-device-<version>.exe`, `release-<version>.json`, `SummriseSetup-<version>.exe`.
The installer and the document must name the **same** version: a 0.2.1 installer carrying a 0.1.9
sidecar installed "successfully" and kept reporting 0.1.9, which cost an afternoon to explain.

**The mirror is the release channel.** `raw.githubusercontent.com` is unreachable or slow from the
canary hosts often enough that a transfer in the middle of an install fails; a Tailscale host serving
the same directory answered every time. The document advertises the mirror's address (`--mirror`), so
the device downloads from it and not from the public host that published. `--allow-plaintext` is
needed on the signing and the update side when that address is plain http; the flag widens the
scheme and nothing else, and the tailnet is the transport that makes it honest.

**An update is one command, and it prints its verdict.**

```bash
summrise-device --check-update <mirror>/release-0.2.7.json --channel canary \
    --data C:\ProgramData\Summrise --install C:\SummriseInstall
```

`applied 0.2.7` means the supervisor swapped the baseline and the new app answered its health probe
twice; `/api/health` then reports that version. A 404 revocations list is read as "nothing revoked";
a stale or badly signed one is a hard refusal.

**The installer is built and tested by CI, not by a laptop.** `installer.yml` derives the compile-time
public keys from the seeds (`summrise-release pubkey`), builds the setup, then **installs it on the
runner**: the service must be running, `/api/health` must answer `status: ok`, and the uninstaller
must remove the service. A tag release carries the setup as a release asset.

**What the canary proved, in the order it happened.**

| evidence | where it comes from |
|---|---|
| an installer that upgrades and a fresh one that seeds | `%TEMP%\summrise-setup.log`: `health: ok on port 18090` |
| the update path, twice, on two machines | `applied 0.2.6`, then `/api/health` `{"version":"0.2.6"}` |
| the console's control channel across a tailnet | `POST /mcp` with a signed grant returned the other machine's `state` |
| liveness over time | the device gate's monitor on `/api/health`, `up_pct: 100` |

**The console binds loopback and is reached from the fleet, not the other way round.** A monitor that
probes a second machine's `/api/health` from a first machine reports DOWN, and that is not an outage:
the device listens on `127.0.0.1` and the console channel is outbound. Fleet liveness is the console's
`devices` count and its channels, not a reachable port on each host.

**A device looks for releases on a schedule it was given.** `<data>/update.json` names the mirror,
the channel and the interval (300 s to 7 days); the serving app reads it at start and checks on that
schedule with the same verifier the one-shot uses. There is no file on a device that was never told,
which is the pre-0.2.9 behaviour:

```bash
# give the device a schedule (an installer or an operator does this once)
summrise-device --set-update-source http://<mirror>/release-0.3.2.json \
    --channel canary --interval 21600 --allow-plaintext --data C:\ProgramData\Summrise
# stop checking on a schedule
summrise-device --update-source-clear --data C:\ProgramData\Summrise
```

The first check waits a per-device jitter (up to an hour) so a fleet rebooting together does not
ask the mirror for the same document in the same second. A refusal is one log line and the next
interval; the decision itself is never made twice by two code paths.

## 10. The console on a Windows host

The console is a control plane, so "it runs" is not the requirement: it must come back on its own.
On a Windows host it is a scheduled task, not a service, and the default task settings are hostile —
Windows stops a task after 72 hours, and restart-on-failure is off by default. Left at the defaults, the
console dies on the third day and stays dead.

The deployment (as run on `desktop-9vh92bj`):

```
C:\SummriseConsole\
  summrise-console.exe      the binary
  operator.token            the operator's token (one line, 0600-equivalent ACL)
  grant.seed                the ed25519 seed grants are signed with
  run-console.cmd           idempotent launcher: exit 0 when the process already runs
```

The task:

| Setting | Value | Why |
|---|---|---|
| actions | `run-console.cmd` | the script refuses to start a second instance |
| triggers | at startup, **and every 5 minutes** | a killed process is back inside one cycle without a service wrapper |
| execution time limit | none (`PT0S`) | the default 72-hour limit stops a long-running console |
| multiple instances | `IgnoreNew` | the tick cannot stack instances |
| restart on failure | 3 × 1 min | covers a crash the tick would otherwise wait for |
| user | `SYSTEM` | no interactive session to keep alive |

Verification, on the host:

```bat
curl http://127.0.0.1:19000/health        :: {"status":"ok","devices":N,...}
taskkill /F /IM summrise-console.exe      :: then wait one tick
curl http://127.0.0.1:19000/health        :: ok again, devices reconnected
```

Observed on 2026-10-23: killed the console (pid 26108), and five minutes later `/health` answered with
`uptime_s 309` and `devices: 2` — the tick restarted it and both devices re-dialled, with no operator
action. The first attempt at this deployment had the default 72-hour limit and restart disabled: the
process did **not** come back, which is why the task is documented here rather than assumed.
