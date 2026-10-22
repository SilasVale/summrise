# The two-runtime architecture — a clean-sheet design for Summrise

> **Status: the design, implemented through phase 4.** Phases 1–4 are built in `device/` (§13a):
> the supervisor, the app, the capability layer, the console and grants. The phase-5 cutover
> (`2026-10-21-phase-5-cutover-plan.md`) has **not** happened, and the two gaps called out below
> are real: the device has no mesh client, and the console has no device page. Sentences describe
> what is built unless they say "planned", "target" or "not built".
>
> This document answers the question the operator asked on 2026-10-20: *"if you ignored the existing
> code and treated it only as reference, how would you design it?"*
>
> The design was deliberately **smaller** than what existed then. "Perfect" is not a property a
> design can have; the property this one is optimised for is **the cost of the next change** — and
> the only honest way to buy that is to have fewer things, each with one owner and one way to prove
> it works.
>
> The migration mapping and phase plan are in §12–§13. The design is not allowed to pretend the fleet
> is empty: every phase must be shippable to a running device and reversible.

---

## 1. What the product is

> One command turns a Windows machine into an AI-operable workspace; one console lets its owner see and
> take back control from anywhere.

Four nouns carry the product: **device, session, grant, reachability**. Everything else (npm, Electron,
CDN workers, mail, model proxies) is either a delivery detail or a different product.

The operator is one person. The device is a real Windows box that may be on a bad network. The AI clients
are MCP clients on the device or anywhere else. The free path must stay free.

## 2. Principles

1. **Functional core, imperative shell.** Every decision is a pure Rust function; every OS effect is a
   thin, named call. The core runs on Linux and is tested there.
2. **One writer per fact.** A fact that two components can write is a fact that will disagree with
   itself.
3. **One path per concern.** One updater, one reachability path, one authorization function, one state
   schema.
4. **Prove it on the platform it runs on.** The device lifecycle runs in CI on Windows; text-pinned
   PowerShell is not a test.
5. **Delete before build.** Every addition must name what it replaces. The default answer to "and also"
   is no.

## 3. Deliverables

Two runtimes plus a control plane. A library is not a deliverable; it ships inside one.

| Deliverable | Form | Its own lifecycle |
|---|---|---|
| `summrise-device` | one app binary + one SCM supervisor (`summrise-launch`); the optional WebView2 host has not been built | Windows, offline, fleet |
| `summrise-console` | one Rust service (axum/Tokio): operator auth, registry, grant signing, the device channel, the `/mcp` proxy and the audit index; no Worker, no Durable Object, no SPA | deployed per commit (the operator's VPS or a loopback box) |
| `release` | the signed `release.json` document and the offline `summrise-release sign` tool; the `plan`/probe tooling described in §6 is **not built** | tags, channels, audit |

**Not deliverables, and not in this repository:** the operator's mail inbox and the model proxies.
They are products with different failure domains and different release cadences.

## 4. The device

### 4.1 Process model

- **One SCM service: `summrise-launch.exe`**. In the built tree the supervisor is the `launch`
  crate: the SCM, the job object, the swap transaction and the installer live behind
  `launch/src/win.rs` (~1 700 lines), and the crate depends on `ed25519-dalek`, `serde`,
  `serde_json`, `sha2`, `windows-service` and `windows-sys`. It is not a 200-line `std`-only shell;
  it verifies the app binary against the baseline, restores it when it is missing or corrupt, starts
  `summrise-device.exe` as its child, and owns the update swap. It is the one component that must
  never be replaced by an update — it is the root of the repair path.
- **The app**, `summrise-device.exe`, running as SYSTEM under the launcher, owning the HTTP/MCP
  server, the sessions, the state file and the update *staging*; the update **decision** is
  `summrise_release::select`, and the swap itself is the launcher's.
- **One optional per-user helper**, started at logon, for the things that must run in the interactive
  session: launching the user's Chrome for CDP, and showing the tray. It talks to the service over a
  named pipe with a per-boot token. If the helper is absent, everything except GUI browser control and
  the tray still works. The helper is **not built** in this tree.
- **No scheduled tasks, no PowerShell, no wrapper exe.** The SCM is the only supervisor, and the
  binary is its own updater (§4.4).

### 4.2 State

One file, one writer.

```jsonc
// <data>/state.json — written ONLY by the service, atomically (write .tmp, fsync, rename)
{
  "schema": 1,
  "install": { "dir": "D:\\Summrise", "version": "2.0.3", "channel": "stable" },
  "baseline": { "version": "2.0.3", "sha256": "…", "exe": "…\\summrise-device.exe" },
  "service": { "state": "running", "pid": 1234, "startedAt": "…" },
  "lastVerdict": { "ts": "…", "outcome": "ok|repaired|needs_attention", "detail": "…" },
  "sessions": [ { "id": "…", "kind": "pty", "state": "attached" } ],
  "update": { "phase": "idle|downloading|staged|applying", "to": null }
}
```

Readers (panel, console, CLI) call `GET /api/state`; nobody parses the file from the outside. There is
**no console device page and no console state route** in the built tree (I9), so `GET /api/state` is
the only rendering of this document; a console view is not built.

### 4.3 The control contract

The product is the permission to act. One function decides:

```rust
pub enum Decision { Allow, Ask { prompt: String }, Deny { reason: DenyReason } }

pub fn authorize(ctx: &Ctx, tool: &ToolCall) -> Decision;

pub struct Ctx<'a> {
    pub subject: Subject,          // Owner | Key { id, scopes } | Anonymous
    pub grant: Option<&'a Grant>,  // issued by the owner, may expire
    pub transport: Transport,      // Loopback | Relay | Console
    pub approval: ApprovalState,   // NotRequired | Pending | Granted(at)
    pub now_ms: i64,               // the decision's clock
}
```

Rules, in order, and they are the whole security model:

1. `Owner` → `Allow`, and `Owner` is reachable **only** with the install token (§9.1); a loopback
   socket is never identity, because on Windows every process of every user is "loopback".
2. `Relay`/`Console` → require a `Grant` that covers `tool` and has not expired; no grant → `Deny`.
3. A tool marked `destructive` → `Ask` unless the grant explicitly says `allow_destructive`.
4. Everything else → `Deny` by default.

`Grant`, `ApprovalState` and `DenyReason` are data; the panel, the console and MCP all render the same
decision. There is no second place that checks an origin, a scope or an approval.

Sessions are a state machine with four states and no others:

```
Creating -> Attached -> Detached -> Reaped
                \-> Closed <--------/
```

Every transition is one function in `core`; a session that is not in a legal state is a bug the state
file exposes, not something a surface has to infer.

### 4.4 Update and self-repair

- **One updater**, inside the binary: the hidden one-shot `summrise-device --check-update <url>
  --channel stable|canary` fetches the signed document, verifies it, stages the artifact and asks the
  supervisor for the swap. There is no HTTP update route. Channels: `stable`, `canary`.
- **One decision**: `summrise_release::select`. A **stable** device refuses a **canary** document; a
  **canary** device accepts both. A release older than the installed version is a downgrade (exit 4);
  a release **equal** to it is `up_to_date` (exit 0, no swap request, nothing changed), so re-running
  a check is a successful no-op. `core` exposes no second decision.
- Artifact: one signed binary. Signature (ed25519) and sha256 verified **before** it is staged.
- Swap: the updater writes `<install>/swap.request.json`; the running supervisor polls it, re-checks
  the staged pair with the **release** key, stops the app, renames the new file over the old, starts
  the app, probes `/api/health` twice, and on failure restores the previous binary and starts that.
  The previous binary is kept until the next successful boot. **Only the launcher writes the app
  binary** — one writer, one swap path. `summrise-launch --apply <staged> <sidecar>` remains an
  operator-only manual entry point; the product does not use it.
- **Two keys, two jobs.** The **launcher** key (`SUMMRISE_LAUNCHER_PUBKEY`) signs the seed sidecar
  and the first baseline generation; the **release** key (`SUMMRISE_RELEASE_PUBKEY`) signs every
  release document and the staged sidecar. The launcher embeds both, the device embeds the release
  key, and a seed signed with the release key (or a staged update signed with the launcher key) is
  refused. Both fail closed on the placeholder. These are compile-time seams; there is no runtime
  override.
- **Self-repair** is the launcher's start-up step, not the app's: verify the app binary against the
  baseline; if it is missing, zero-length or hashing differently, restore it from the baseline before
  starting. The baseline is written by the launcher only after a verified install, and it is the only
  other copy of the app binary on disk. This is why the design has a launcher at all: the process that
  would repair a missing binary cannot be that binary. A restore appends to `<install>/repair.log`,
  and the app folds it into `lastVerdict` and `<data>/events.jsonl`.
- The state file records every phase. The launcher polls the swap result and the app folds it into
  `state.json`; the console has no route that reads this document yet (§4.2).

### 4.5 Capabilities

One registry, one file per capability. The built registry declares **27 tools**:

```
files.read | files.write | files.mkdir | files.list
memory.append | memory.search
terminal.create | terminal.write | terminal.read | terminal.resize | terminal.close | terminal.list
browser.list | browser.open | browser.navigate | browser.eval | browser.screenshot | browser.close | browser.shutdown
monitor.add | monitor.remove | monitor.list | monitor.probe | monitor.status
runs.exec | runs.list | runs.get
```

`terminal.create` takes `kind: "pty" | "ssh" | "serial"`; there is no separate `terminal.pty` or
`terminal.serial` tool, and the browser tools are `browser.*`, not `browser.cdp`. Each capability
declares its tools, which of them are `destructive`, and its Windows effect module. The MCP tool list
and the console's grant validation are both generated from this registry.

## 5. The console

### 5.1 Shape

- One Rust service (`summrise-console`, axum/Tokio) serving:
  - `POST /auth` — exchange the operator token (a 64-hex file) for an in-memory 24-hour session.
  - `POST /enroll` — mint a one-time device registration code (operator session required).
  - `GET /devices`, `POST /devices` — the registry; a device registers with a key, a code and a
    signature over `register\n{id}\n{code}\n{ts_ms}`.
  - `GET /devices/:id/channel` — the device dials out and this WebSocket becomes its one live channel.
  - `POST /grants` — sign a grant for one device and a tool list.
  - `POST /mcp` — the console-specific `{device_id, grant, request}` envelope, not streamable MCP; the
    console verifies the grant, forwards the request over the device channel, and the device applies
    the same `authorize`.
  - `GET /audit` — the append-only operator-action index. `GET /health` is the one unauthenticated
    route.
- There is **no Worker, no Durable Object and no SPA**. State is `<data>/devices.json` and
  `<data>/codes.json` (written whole through a sibling temporary and a rename) plus two append-only
  JSONL indexes, `<data>/audit.jsonl` and `<data>/denials.jsonl`.
- There is **no device page and no `GET /devices/:id/state`**: the console is a proxy and an audit
  index, not a view of the device's state document.

### 5.2 Reachability: the target is mesh-first; today the console carries the payload

The target is a self-built Rust mesh. The design document that was supposed to hold the decision and
the measurement (`2026-10-20-the-mesh-transport-design.md`) is **not in this tree**, so neither can be
cited as evidence here. What is in the tree is `device/mesh`: the broker and relay binaries plus the
library, with no client wired into `summrise-device` (it does not depend on `summrise-mesh`). The
console WebSocket is therefore the **only** remote data path today, and it does carry the plaintext.

The target, unchanged:

- **Control prefers a direct peer path.** Devices would join the mesh; tool traffic would go
  device↔client directly whenever a path exists. Not built: no shipped binary speaks the mesh.
- **The console would be a broker, not a relay.** It would authenticate the operator, issue a signed
  grant, publish presence and help peers find each other. Today it also carries the payload, because
  there is no alternative path.
- **The floor is one self-hosted relay on the operator's VPS** (`scripts/DERP.md`), used only when no
  direct path can be established. That is the same relay the mesh design assumes — not a second relay.
- **Cross-machine files keep their own worker** (`relay/`, R2): a CHARTER red line and a different job
  from control. "One reachability path" is about control; a 100 MB transfer is not control.
- The device never accepts an inbound connection; it dials, always.

### 5.3 Model routing is not here

The `/v1` model router is a separate deployable with its own failure domain (upstream providers,
regional blocks, quotas). It must not share a Worker, a release or a CI job with device control.

## 6. The control plane

- `release.json` is the signed release document (`version`, `channel`, `sha256`, `artifactUrl`,
  `minVersion`, `issuedAtMs`), produced offline by `summrise-release sign`.
- **Built:** the signer. It verifies the key file's mode, the channel, the URL and the version before
  it signs; the device verifies the policy and artifact signatures and applies `select`.
- **Not built:** `release plan` and `release apply`. There is no deployer that orders the console, CDN
  and device channel, and no probe runner; deployment and verification are manual, recorded in the
  canary runbook.
- A canary channel is a **manifest value**, not a second deployment.
- The `topology.json` comparison described in the first draft is not built either.

## 7. Failure model

| Failure | Detected by | Repaired by | Evidence the operator can read |
|---|---|---|---|
| exe missing/corrupt | service start fails; baseline present | boot self-repair | `repair.log`, then `lastVerdict` "repaired from baseline" and the `repaired_from_baseline` event in `events.jsonl` |
| update downloaded bad | signature/sha check | discard, stay on old | `swap.result.json` / `lastVerdict` refusal |
| update applied, health fails | probe after start | previous binary restored | `swap.result.json` / `lastVerdict` "rolled back from X" |
| service hung | an in-process watchdog: its own health endpoint fails twice, the process exits | SCM failure action restarts it (3 attempts) | `/api/health` failure, then a new pid/version |
| console down | not needed for local control | local access and direct clients still work; **remote operator sessions stop**, because there is no mesh path yet | `/api/health` still answers; the console channel is down |
| operator offline | — | — | grants expire; nothing else changes |

## 8. Testing

| Layer | Where | What |
|---|---|---|
| `core` | Linux | unit + property tests for `authorize`, session transitions, grants, the state schema and the registry |
| `release` | Linux | the document's signatures and freshness, `select` (stable/canary, newer/equal/older, floors), the signer's output |
| device effects | Windows CI (`windows-latest`) | real binary in a temp dir: install → update → rollback → delete exe → self-repair → open a PTY → kill → reap |
| console | Linux | in-process axum tests with a fake device over the WebSocket channel |
| end-to-end | Windows CI + one canary | real device, `canary` channel, 24 h |

**Deleted by this design:** text-pinned PowerShell contract tests, the PowerShell brace gate, the
production-host allowlist ratchet (the hostnames move to one config), the 15-minute pre-push suite (CI
owns the full run), and the design-sweep emitters (moved out of this repository).

## 9. Security boundaries

1. **The install token is the root of trust**; loopback alone is never identity. Remote access is
   always a grant plus the console's auth.
2. The channel token never reaches the browser; the console stores only its hash. The device does
   **not** hold an identity key of its own yet: the registration key pair is generated by the
   operator, and the grant binds `device_id`, not a subject.
3. The mesh relay would carry opaque HTTP; it is not in the device's path yet. The console never
   terminates TLS to the device's internal services.
4. Update signatures are verified before any file is replaced; the signing key is offline.
5. The install token is minted by the launcher at install, stored in an ACL-protected file, and
   required on every local request; it is what makes `Subject::Owner` (and therefore rule 1)
   meaningful.
6. The audit log is append-only and exported daily; a denied call is an event like any other.

## 10. What this design refuses

- A second updater, a second relay, a second CDN arm, a second frontend.
- Electron: a WebView2 host (or the browser) is enough, and it removes node/npm from the device.
- npm as a distribution channel: one signed binary and a manifest.
- PowerShell as a logic language: it may remain only as the one-line install command.
- Multi-tenancy, paid tiers, a second platform.
- `mail/` and the model proxies in this repository.

## 11. What is lost

- Electron's uniform shell across macOS/Linux. The product is Windows-only; if that changes, WebView2
  has no equivalent and the shell returns as a separate deliverable.
- npm's free hosting and version resolution; a signed artifact needs a host and a manifest.
- cloudflared's zero-maintenance ingress; the relay is ours to operate, including bandwidth.
- The accumulated edge cases in the current comments are **not** carried by this document. They become
  the Windows CI cases and `field-notes.md`; anything not written there is lost on purpose, and that is
  the price of a clean design.

## 12. Migration map

| Today | Target | Fate |
|---|---|---|
| `agent/src/web`, `plugins/*`, `tools/*` | `summrise-device` http/capabilities | ported, decisions moved to `core` |
| `agent/src/mesh`, `tunnel.rs`, `relay.rs` | device relay client | replaced by §5.2 |
| `summrise-cli` (`psgen`, `swap`, `setup`) | device updater + install command | deleted except a thin `summrise` control CLI |
| `summrise-desktop-electron`, shell policy crates | WebView2 host + `core::admission` | deleted or reduced |
| `summrise-launch` (new) | SCM supervisor + the only writer of the app binary | replaces both swap scripts |
| `agent/deploy/*.ps1` | signed exe + one-line installer | deleted |
| `gateway/` (TS) device/auth half | `summrise-console` | ported |
| `gateway/` model routing | separate deployable | split out |
| `index/` | control plane + manifest | reduced |
| `mail/`, `proxies/` | separate repositories | moved |
| `agent/tests/design-sweep*`, `sweep-*`, emitters | separate dev repository | moved |
| `docs/design/0014` | `topology.json` | replaced by the manifest |

## 13. Phases

This is the plan, not a status report. What is actually built is §13a; it includes neither phase 2's
mesh client and console device page nor phase 3's WebView2 host.

| Phase | Deliverable | Shippable when | Rollback |
|---|---|---|---|
| 0 | Windows CI lane for the current binary (MSVC builds what ships) | green on `main` | none needed (no product change) |
| 1 | `core` + `state.json` + the one updater inside the current agent | canary device updates through it | old installer still present |
| 2 | mesh client + broker + relay floor + console device page | one canary device reached over the mesh for 24 h, and through the relay floor with the mesh disabled | cloudflared remains configured |
| 3 | WebView2 host + signed artifact distribution | a fresh install needs only the exe | npm channel still published |
| 4 | the capability/tool layer (the largest port; only after the shell is proven) | the MCP surface is served by the new device on a canary | the old agent still serves the fleet |
| 5 | delete the replaced paths; topology `plan` | zero references, proved | tags and installers retained |

Each phase ends with a deleted thing. A phase that only adds is not finished.

## 13a. Implementation status (2026-10-21)

The tree at `device/` implements phase 1, phase 3's signed distribution and phase 4's capability
layer, on branch `arch/two-runtime-phase-1`: `core` (state schema and the pure decisions), `release`
(the signed document and the **one** update decision, `select`), `launch` (the SCM supervisor and the
only writer of the app binary), `device` (atomic state, the health/state/authorize endpoints, the
stager, PTY/SSH/serial/browser/monitor/runs), and `console` (auth, registry, grants, the device
channel, the `/mcp` proxy and the audit index). A `windows-latest` job runs the real lifecycle — kill
the supervisor (the child dies with it), delete the exe (the baseline restores it), apply a validly
signed broken update (rollback is byte-identical), install/start/stop/delete the service.

**Phase 2 is partial, and the gap is load-bearing.** `device/mesh` builds the broker, the relay and
the library, but `summrise-device` does not depend on `summrise-mesh`: there is no mesh client, so
"one canary device reached over the mesh for 24 h" cannot be evidenced, and the console WebSocket is
the only remote data path. The console also has no device page or state route. The old `agent/` tree
remains authoritative for the fleet until phase 5 deletes what phase 4 replaced.

The phase-1 snapshot quoted 33 `core`, 38 `launch` and 57 `device` tests; those numbers were already
stale when phases 2–4 landed. Run `cargo test --workspace` for the current count rather than trusting
a number in a document.

## 14. Decisions taken after the builder review

| Question | Ruling | Why |
|---|---|---|
| Reachability: console hub or mesh | **target: mesh first, console brokers, one self-hosted relay floor** | The mesh is the operator's recorded decision and the measured Cloudflare path is hostile; the console should not be the data path. It **is** the data path today: no mesh client is wired |
| What starts/restarts the service | the launcher is the SCM service; the app is its child and exits when its own health check fails twice; the SCM failure action restarts it | the SCM has no liveness probe — pretending it does was a defect in the first draft |
| What makes a caller `Owner` | the install token, ACL-protected, on every request; loopback is never identity | windows "loopback" includes every user's process |
| Which phase ports the tool layer | phase 4, after the shell is proven; phase 5 deletes | the largest port must not be scheduled before the runtime that hosts it |
| Cross-machine files | the existing R2 file relay stays; the control path is separate | CHARTER red line; bytes are not control |
| Grants and device keys | console issues ed25519 grants over `{device_id, tools, expires_at_ms, allow_destructive}` for 60 s–7 d (no default), and the device verifies offline. There is **no subject in the signed message**, and the device holds no identity key: the registration key pair is operator-supplied | offline devices stay reachable; a bearer grant is enough for one operator, but the audit cannot name the operator session |
| Windows lane | MSVC builds and tests the artifact that ships; signing is a named manual step that fails CI when absent; reboot/24 h belong to the canary | the tested binary must be the shipped binary |
| Console facts | `devices.json` and `codes.json` written whole, `audit.jsonl`/`denials.jsonl` appended, sessions in memory; no D1 and no Durable Object | no database dependency; one operator, and the console is not an owner of device facts |

## 15. Open questions for the operator

1. Is a browser tab acceptable as the desktop surface, or is a WebView2 window required?
2. Which of `mail/` and the model proxies move to their own repositories now, and which wait?
