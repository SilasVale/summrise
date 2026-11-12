# Architecture of Record: the two-plane kernel

> **Status: architecture of record, 2026-11-12. Approved by the operator: plugins are trusted (no
> signature or sandbox); the desktop surface is the shell, not Electron. Nothing here is built yet.**
> This is the architecture `summrise-product` implements from here on. It is argued from
> `docs/PRODUCT.md` (what the product is) and `docs/design/capability-hub-2026-11-02.md` (the plan of
> record); it replaces neither. The first slice that proves it is §8.

Perfect is not a property a design can have before it runs. What this document claims is narrower and
checkable: **every failure mode we know has a named boundary, a stated guarantee, and a test.**

![The two-plane kernel](assets/architecture-v2.png)

## 0. Six decisions, everything else is a consequence

1. **Two planes, one envelope.** Calls, grants, sessions and plugin lifecycle are the *control plane*:
   totally ordered per device, durable, replayable. Terminal bytes, file chunks and screen frames are
   the *data plane*: per-stream offsets, coalesced, lossy, never journaled. One envelope, two delivery
   classes — so "one bus" stops being a lie the first time a PTY floods it.
2. **The journal is bounded.** It is the device's event log: sealed segments, a periodic snapshot, and
   a retention cap. "Append-only" does not mean "grows forever" on a machine whose constraint is disk.
3. **Plugins are trusted code, supervised.** Installing a plugin is trusting it: it runs on the
   operator's own machine, as the operator. There is no signature, no sandbox and no capability prompt
   in v1. The host still supervises the process — start, restart with backoff, mark it failed after a
   crash loop, kill the tree on stop — because a crash loop is an availability problem, and the install
   screen says once, plainly, that the code runs with the operator's privileges.
4. **The console splits into control and page.** Control holds the keys, enrollment, the device
   channel and the audit store. Page serves the static page, SSE and read-only JSON and holds no keys.
   A page restart cannot mint a token; a console restart cannot corrupt a call record. The split is a
   code boundary first: one binary may host both roles, and the page still never holds a key.
5. **The operator sees everything; a client token sees its tools.** There is one person and one
   operator session, and the event stream is not filtered for them. A token minted for an AI client
   carries tool scopes and does not subscribe, so the model is one rule instead of a topic-scope
   language.
6. **Dependencies are enforced, not drawn.** A CI gate reads `cargo metadata` and fails on an upward
   edge. A poster that says "only downward" and a tree that does it are different things.

## 1. Scope and non-goals

One product, one repo, one runtime per machine, one operator, a handful of machines. Windows is the
target; Linux runs the device in CI and on servers.

The legacy repository (`/home/zss/summrise`, the Agent line) is a separate product. This document does
not merge the two histories; a module moves from it only with a product decision, and the one candidate
(the Electron client) is decided in §9. "One repo" is the target for this product, not a statement that
the other one is gone.

Non-goals for v1: multi-tenant SaaS; device-to-device federation; event sourcing as the system of
record (§9); a second UI.

**Priority order: convenience, then operability, then security.** One person uses this on their own
machines. A gate that costs a click is a bug unless it prevents a failure that person cannot see; a
boundary that stops only a hypothetical attacker is not worth a prompt. Security is accepted where it
is free at the point of use — the record, the stop button, the disk bound — and revisited the day the
product is shared or exposed. An earlier draft of this document proposed a signed plugin manifest and an
OS sandbox; that section is withdrawn (§9).

## 2. Invariants

| # | Invariant | Enforced by |
|---|---|---|
| I1 | One writer for the device journal, a monotonic `seq` per device | `bus` owns the journal; `launch` keeps its own supervisor log, which the bus ingests at boot (S1) |
| I2 | One envelope, two delivery classes | `plane` is a field on the envelope; the writer rejects a class it cannot deliver |
| I3 | The kernel has no I/O | dependency gate; `core` names no runtime crate and imports no `std::fs` |
| I4 | The journal is bounded; state is derived and rebuildable | retention job; `state.json` can be rebuilt from journal + snapshot on recovery |
| I5 | The operator sees everything; a client token sees its tools | one operator session; tokens are tool scopes, not topic scopes |
| I6 | A plugin is trusted; its process is supervised | restart with backoff, mark failed, kill on close — availability, not isolation |
| I7 | The page holds no keys | module boundary + CI check |
| I8 | Dependencies point one way | target: `scripts/check-architecture.mjs` in CI (S5); nothing enforces it today |
| I9 | Failure domains restart independently | process domains: device, launch, console, plugin children; the page may be a module and restarts with the console |
| I10 | One UI, one auth path | desktop is a surface of the console, never a second device auth |
| I11 | A destructive call runs only for the owner on loopback, or under a token that carries destructive consent; otherwise it is refused with a reason | `allow_destructive` on the token; a refusal row naming the fix |

## 3. Code shape

```
device/
  core/           kernel: registry, policy, sessions, contracts — no I/O
  bus/            NEW  journal, dispatch, subscriptions, replay — the only writer
  device/         app: adapters (terminal, files, browser, screen, runs, host), channel, MCP
  plugin-host/    NEW  supervision, MCP children (extracted from device/src/plugins)
  console/        control: enroll, tokens, device channel, audit store, subscription hub
  console-page/   NEW  static page, SSE, read-only JSON — no keys
  console/web/    React page
  shell/          tray + WebView2 (Windows)
  launch/         supervisor, updater, the only writer of the app binary
  mesh/           QUIC, rendezvous, relay
  release/        signed release documents, channels, revocation
  ui/             design tokens, shared page shell
  winsec/ winsys/ the OS boundary
```

The device's own panel is not a crate: `device/device/src/panel.rs` renders it in Rust and the device
serves it at `/panel` — the surface that works when the console does not.

The dependency rule: `core ← bus ← {device, plugin-host, console, console-page} ← surfaces`.
`core` imports no crate above it; `bus` imports only `core`; no surface imports another surface;
`device` and `console` import `bus` and `core`, never each other.

## 4. Contracts

### 4.1 Envelope

```json
{"v":1,"plane":"control","seq":123,"ts":"2026-11-12T14:00:00Z","type":"plugin.crashed",
 "subject":{"device":"9VH","plugin":"hello"},
 "trace":{"call":"01J..."},
 "payload":{"reason":"exit 1","restarts":4}}
```

`plane` is `control` or `data`; a data-plane envelope carries `stream` and `offset` where a control one
carries `seq`. `seq` is the control plane's order; `ts` is informational and never orders anything. `v` is the major
schema; within a major changes are additive only, and the bus refuses an unknown major rather than
guessing. `type` is the topic. v1 topics: `call.started|finished|denied`, `session.opened|closed`,
`plugin.installed|starting|ready|stopped|failed|tools_changed`,
`device.started|joined|left|updated`, `grant.minted|revoked`, `stream.gap`, `update.staged|failed`.
Plugin topics: `plugin.installed|starting|ready|stopped|failed|tools_changed`.

### 4.2 Control plane

Totally ordered per device; durable before it is published; at-least-once; replayable from any
`seq` still inside retention. A call is exactly two events (`started`, `finished|denied`) sharing one
`trace.call`, so a reader can always pair them.

**Two journals, two scopes.** The device journal records what ran on the machine. The console audit
records what the operator did: token mints, subscriptions and proxied calls. A proxied call appears in
both, joined by `trace.call`; neither log is rebuilt from the other, and a reader that needs the pair
joins them. The two windows differ — 64 MiB on the device, 8 MiB with two generations on the console
(`device/console/src/audit.rs:54`) — so Activity must say which rows are still replayable and which are
archived. Today the page reads only the console's audit: a call made from the device's own panel, from
the mesh or over loopback lives in the device journal and reaches Activity only when S1's subscription
lands. That is the current completeness gap, not a design choice.

### 4.3 Data plane

A stream has an id (`terminal:<device>/<session>`, `file:<id>`, `screen:<id>`) and a byte offset.
Frames coalesce for 16 ms; a slow consumer drops oldest and receives `{"stream_gap":{"from":n,"to":m}}`.
Data-plane frames are never journaled and never counted by the audit; the terminal's end is a control
event, its bytes are not.

### 4.4 Journal

`journal/000001.seg` seals at 4 MiB; `journal/snapshot-<seq>.json` is written every 1 000 events or
5 minutes; retention is 64 MiB or 7 days, whichever comes first (`--journal-max-bytes`,
`--journal-retain`). The cap is not new: today's `events.jsonl` is already capped at 64 MiB and rotated to
`events.1.jsonl`, one generation kept, older lines discarded by design (`device/device/src/mcp.rs:89`).
The target keeps 64 MiB as the whole journal — today's 64 MiB is per file, one rotation — and adds
segments plus the time bound.

Writes batch for 16 ms: assign `seq` → append → fsync → publish. A crash may lose the un-fsynced batch,
which was never published; on boot the next `seq` is the journal's last durable one plus one, so a
published `seq` never repeats. Framing is length-prefixed with a CRC; a torn tail is truncated to the
last valid frame at boot. The journal carries an `epoch` (16 random bytes written when it is created): a
new journal — a rollback, a data-directory reset — starts a new epoch, and every cursor is `(epoch, seq)`.

`replay(from)`: inside the window, replay from `from`; past it, `reset{snapshot_seq, lost:{from,to}}`.
The snapshot is always at or before the deletion boundary, so the lost range is named rather than
silently skipped. A journal append that fails refuses the mutating call (`begin`); a failed `end` is
reported as a lost row and the call still returns — the existing audit policy (`mcp.rs:3196-3199`).

### 4.5 Subscription

![The subscription lifecycle](assets/architecture-v2-subscription.png)

```
{"id":7,"sub":{"from_seq":41}}
{"id":7,"ack":{"from_seq":41}}
{"sub":7,"evt":{"seq":48,...}}
{"id":9,"unsubscribe":{"sub":7}}
{"sub":7,"lagged":{"dropped":12,"from_seq":60}}
{"sub":7,"reset":{"snapshot_seq":1000,"lost":{"from":41,"to":999}}}
{"sub":7,"revoked":{"reason":"session revoked"}}
```

One channel per device; N subscribers per channel; each subscriber has a bounded queue (1 024
control events). A subscriber whose oldest queued event is more than 256 behind the head is lagging: it
is told `lagged{dropped, from_seq}`, then either resets from the journal or is disconnected — never
silently skipped. Only an operator session subscribes; a client token calls tools and cannot. A
subscription is an audit row, and a revoked session closes it with a sentence.

The page's SSE carries `id: <epoch>:<seq>` on every event. The console numbers its own rows into the
same sequence (a console-local counter plus its boot epoch), so one `Last-Event-ID` resumes both
sources; a console restart bumps the epoch and the page resets to a snapshot.

### 4.6 Plugin

```json
{"id":"com.example.hello","namespace":"hello","name":"Hello","version":"1.0.0",
 "description":"…","command":["hello.ps1"],"cwd":".","env":{},"transport":"stdio"}
```

The manifest is the one the code reads today (`device/core/src/plugins.rs:37-51`): `id`, `namespace`,
`name`, `version`, `description`, `command`, `cwd`, `env`, `transport`, with `deny_unknown_fields`. The
target adds nothing to it. Installing registers the directory and copies nothing
(`device/device/src/plugins/host.rs:4-6`); the tools arrive from `tools/list` and are advertised as
`<namespace>.<tool>`.

**Trust is stated, not enforced.** Installing runs the code with the operator's privileges;
`PRODUCT.md` already says so and the install screen says it once. There is no signature, no copy to
sign and no sandbox. The one boundary that stays is the process itself, because it keeps a crash from
taking the device down.

**Supervision is about availability.** `installed → starting → ready → stopped → failed`: a child that
exits is restarted with 1/2/4/8/16/30 s backoff; five starts in 300 s mark it `failed` and its tools are
unregistered; the whole child tree is killed when the device stops. Re-enabling is explicit. A memory cap
is optional; fs and network are the operator's own. Every transition is a control event
(`plugin.starting`, `plugin.ready`, `plugin.stopped`, `plugin.failed`, `plugin.tools_changed`), so the
record shows what happened.

### 4.7 Tokens and consent

A token carries `tools:<glob>`, `device:<id>` and `allow_destructive` — what an AI client may call, not
which events it may see. There are no topic scopes: the operator session sees every event, and a client
token does not subscribe at all.

The token is the consent, as `PRODUCT.md:47-49` says. `allow_destructive` is a field on the token
(`device/console/src/tokens.rs:107-108`), and the console mints the grant's `auto_for_destructive` from
it. A destructive call under a grant without it goes to `Ask` (`device/core/src/auth.rs:221-276`) and is
refused with `approval_required` (`device/device/src/mcp.rs:702-720`) — the sentence already says to mint
a grant with `allow_destructive=true`. The owner on loopback is allowed outright (auth rule 1). The
refusal is a row.

**Convenience first: a token minted from the console defaults to `allow_destructive: true`.** The
console shows it as a toggle, already on; turning it off is one click. The operator's console session is
not a token — it is the owner, and rule 1 allows it outright. A client token defaults to off and
can be narrowed to a tool list. There is no pending-approval state in v1: the panel's per-command gate is
the legacy product's mechanism and stays there, because carrying it here would add a queue this product
does not have and would contradict `PRODUCT.md:48-49`. If it is ever wanted, it is a product change that
starts in `PRODUCT.md`.

## 5. Failure modes

| Failure | What happens | Guarantee |
|---|---|---|
| device dies | `launch` restarts it; the channel reconnects | a published event is durable; the ≤16 ms un-fsynced batch was never published; `seq` never repeats |
| console dies | devices keep serving; the page is down; the shell opens the panel | on restart the page resumes from its `Last-Event-ID` |
| journal full | seal, delete oldest sealed segment | older subscribers get `reset{snapshot_seq, lost}`; the snapshot is at or before the boundary |
| plugin crash loop | backoff 1/2/4/8/16/30 s; five starts in 300 s mark it `failed` | tools unregistered; `plugin.failed` is one event |
| slow subscriber | control: `lagged` → reset/disconnect; data: drop + `stream.gap` | no silent loss |
| destructive call, no consent | refused with `approval_required`; the row names the token to mint | never silently run |
| schema skew | publisher or subscriber on an unknown major is refused | no best-effort decoding |
| clock skew | `seq` orders; `ts` is display only | ordering never depends on time |
| journal append fails | a failed `begin` refuses the call; a failed `end` is a lost row and the call returns | a completed side effect is never retried by the client |
| torn tail | truncate to the last valid frame at boot | no half event is replayed |
| fleet cursor | the cursor is `(epoch, seq)`; a new journal starts a new epoch | two devices — and two journals — never share a cursor |
| rollback | an older build refuses a newer journal major and starts a new journal with a new epoch | no silent decode after a downgrade |

## 6. Current state against the target

| Target | Today |
|---|---|
| `core` | `device/core` — registry, grants, sessions, state; already the pure half |
| `bus` | does not exist. `device/console/src/audit.rs` has the only fan-out (a broadcast); the device has no event stream |
| `plugin-host` | `device/device/src/plugins` — a module inside the app, not a supervised subsystem |
| `console` + `console-page` | one binary doing enroll, tokens, channel, proxy and page |
| journal | `events.jsonl`: capped at 64 MiB, rotated to `events.1.jsonl` (one generation, older lines discarded — `mcp.rs:89`); each line carries a per-process `id` that resets to 1, so it is not a cursor; `summrise-launch` also appends (`launch/src/lib.rs:1956`) — two writers today |
| subscription | terminal only (`device/device/src/console.rs:919` → `device/console/src/http.rs:2412`); the page's SSE has a snapshot and `lagged` but no `id:` (`http.rs:5906`); the device audit carries a per-process `id`, the console audit carries none |
| Activity | reads only the console audit; a call made from the device's panel, the mesh or loopback lives in the device journal and reaches the page only when S1 subscribes |
| desktop | `device/shell`: tray + WebView2; the Electron client lives in the legacy repo |
| dependency gate | does not exist; S5 adds it |

## 7. Migration

Each stage ships alone; none requires the next.

- **S1 — L1, the control-plane slice.** `bus` crate (envelope, journal, hub); `session.opened|closed` and
  `device.started` published; channel `sub/evt/unsub` with `from_seq`, an operator-scoped subscription and
  the `subscribe` audit row; `launch`'s event leaves `events.jsonl` for `supervisor.jsonl`, which the bus
  ingests at boot; both journals gain `trace.call`; console audit rows gain a monotonic id and every SSE
  event carries `id:`; console subscribes and merges into the existing SSE; the page resumes with
  `Last-Event-ID`. S4 adds the console-page split; subscriptions stay operator-only.
- **S2 — the bounded journal.** Segments, snapshot, retention; adds `reset{snapshot_seq, lost}` to the
  subscription protocol and the retention job.
- **S3 — plugin host.** Extract `plugin-host`, keep the manifest as-is, supervise the process
  (backoff, mark failed, kill the tree), emit the lifecycle; the plugin UI becomes a projection instead
  of a poll.
- **S4 — console split.** `console-page` as a crate with no keys; the operator-only subscription rule
  and its audit row.
- **S5 — data plane and the gate.** `stream_gap`, coalescing, and `check-architecture.mjs` in CI.
- **S6 — desktop.** The shell is the product's desktop surface. The Electron client stays in the legacy
  line: `PRODUCT.md` says "not an Electron/npm runtime", so adopting it would be a product change, not a
  migration step.

## 8. L1: the first slice

Files: `device/bus/{Cargo.toml,src/lib.rs}`; a `bus` field on the device app; `sub/evt/unsub` frames in
`device/device/src/console.rs` and `device/console/src/channels.rs`; `id:` on the console's SSE
(`device/console/src/http.rs`); `Last-Event-ID` on the page's `EventSource`.

Acceptance — four tests and one manual run:

1. `seq` is monotonic across a restart (journal replay assigns the next, not a reset).
2. `replay(from=41)` returns 41..n once, in order, and continues live with no duplicate.
3. A subscriber that misses events is told `lagged` and recovers from the journal or is disconnected.
4. The page's `EventSource` reconnects with `Last-Event-ID` and receives exactly the missed events —
   device events and console rows share one id sequence.
5. Manual: open a terminal in the console, see `session.opened` arrive without a refresh; drop the
   network for ten seconds, restore it, and see the missed event once.

## 9. Deferred and open

- **L2 (event sourcing as the system of record).** The journal could become the only truth and
  `state.json` only a cache. Deferred: it rewrites state ownership, needs schema migration and
  compaction, and buys time-travel the product does not yet need.
- **Rich desktop client.** Out of scope: `PRODUCT.md` says the product is not an Electron/npm runtime.
  A rich client would be a product change, and if it ever happens it is a console client, never a second
  device authorization.
- **Retention numbers** (64 MiB / 7 days / 1 000 events) are defaults to measure, not constants to
  defend.
- **Sandbox and signing — withdrawn.** Proposed earlier in this document, then withdrawn on the
  operator's rule: convenience first, one person, their own machines. Revisit if a plugin ever comes
  from an untrusted source, or if the product is shared or exposed.

## 10. Review focus

The five things most likely to bite, and where each is pinned:

1. A PTY or file transfer flooding the control plane — pinned by the data-plane boundary (§4.3) and
   the `stream_gap` test (S5).
2. A subscriber that is slow rather than dead — pinned by `lagged`/`reset` (S1/S2).
3. A plugin that crashes during `tools/list` — pinned by supervision: restart, then failed, tools
   unregistered (S3).
4. A page that reconnects during a journal reset — pinned by `reset{snapshot_seq, lost}` (S2).
5. A dependency edge added "just this once" — pinned by `check-architecture.mjs` (S5).

## 11. The operator's journey

The architecture above is the machine's side. This section is the person's side: six moments, and what
the design must guarantee at each. If a contract in §4 cannot serve one of these, the contract is wrong.

1. **Install and enroll — five minutes, once per machine.** The installer puts the launcher, the device
   and the tray on the machine; the tray is the only thing the person sees until enrollment. Home gains
   a row: name, state, version, last seen.
   *Guarantee:* enrollment is a control event; `device.joined` is audited and the fleet is a projection over `device.joined|left`, so
   a console restart does not lose the machine.
2. **Connect a client — two minutes, once per client.** Connect is a sheet: endpoint, client config,
   tokens. The person copies one config, mints one token, pastes it into the AI client.
   *Guarantee:* a token is scoped and audited; the first call appears in Activity within a second, which
   is how the person knows the loop closed.
3. **The agent works — every day.** Activity is the record: each call is a row, and a row opens its
   evidence — arguments, result, grant, duration. A destructive call runs because the token minted from the
   console carries destructive consent, on by default; a client token without it is refused with a sentence that
   says which token to mint.
   *Guarantee:* `started` and `finished|denied` are one pair joined by `trace.call` (S1 adds it to both
   journals); the row is the same whether the console or the device wrote it, and a refusal is a row,
   not a silent drop.

4. **Take the keyboard back.** From a call or Device, Terminal and Screen open on the machine; the
   person types into the session the agent is using.
   *Guarantee:* session bytes are the data plane; `session.opened|closed` are control events; the person
   can always tell whether the session is live.
5. **Move between machines.** One machine control above every page; Home is the fleet when nothing is
   chosen.
   *Guarantee:* one journal per device; no event crosses a device boundary; a machine that is away says
   since when.
6. **Recover.** The console is down, the device is offline, the journal rolled, a plugin crashed. Each is
   a state with words and one next action.
   *Guarantee:* the shell opens the device's own panel when the console is unreachable; the page resumes
   from its last `id`; a journal `reset` is a visible row, not a silent gap; a failed plugin names
   why and what re-enables it.

**What this adds to the contracts.**

- **One Activity over two journals.** The person sees one record; the architecture has two logs (§4.2).
  The page joins them on `trace.call`, which S1 adds to both, and marks a row that exists in only one.
- **Gaps are visible.** A data-plane drop is an inline marker on the session surface and a rate-limited
  `stream.gap` control event; a journal reset and a refused call are rows. Nothing is a hole.
- **The operator is not a token.** An operator session sees every event on its machine; a tool token sees
  its scope. Authorization filters tokens, not the person.
- **The console split is a code boundary.** The person sees one product and one next action; the split
  is about which code may hold a key, not about two things to operate.
- **Retention has words.** Live → replayable → archived; "the record" never silently becomes shorter.

**The first five minutes must answer five questions:** Where am I? What can this machine do? How does my
AI connect? Did it work? What do I do when it doesn't?

## 12. Decision record

Approved by the operator on 2026-11-12. Items 3 and 5 were confirmed explicitly; the rest stand as
written. Nothing here is built yet.

- [x] **Two planes.** Control events are ordered and journaled; data-plane bytes are per-stream,
      coalesced and never journaled.
- [x] **Bounded journal.** Segments, a snapshot every 1 000 events or 5 minutes, a bounded retention
      (64 MiB / 7 days today, measured), and `reset{snapshot_seq, lost}` when a subscriber falls off the
      window.
- [x] **Plugins.** *(confirmed 2026-11-12)* Installing is trusting: no signature, no sandbox, no capability prompt. The host
      restarts a child with backoff, marks a crash loop failed, kills the tree on stop, and the install
      screen says the code runs with the operator's privileges.
- [x] **Console split.** Control holds the keys and the channel; page serves static + SSE + read-only
      JSON and holds none.
- [x] **Desktop.** *(confirmed 2026-11-12)* The shell is the only desktop surface; the Electron client stays out of this
      product. Adding it would amend `PRODUCT.md`.
- [x] **The gate.** `scripts/check-architecture.mjs` reads `cargo metadata` in CI and fails an upward
      edge; `core` may not name a runtime crate.
- [x] **The journey.** One Activity over two journals joined on `trace.call`, gaps visible (inline and
      `stream.gap`), retention named, the operator unfiltered by tool scopes, and a destructive call
      refused only when the token says so.
