# Summrise v2 — one record per role, one bus over its change feed

> **Status: proposal on `next`, 2026-11-13.** The record is SQLite; the bus is its change-feed
> table, written in the same transaction. This replaces both the "journal as the only record" draft
> and the polling sketch: the guarantees of a bus are kept, the second store is not. The feature gate
> is `v1-parity.md`.

![One record per role; its feed carries the live updates](assets/architecture-target-proposal.png)

## 0. What "best" means

1. **The record does not lose facts.** Not by rotation, crash, upgrade, or reset.
2. **Adding a thing costs one place.**
3. **The loop is short.**
4. **The operational surface is small.**
5. **A stranger can find things.**

The previous drafts failed on criterion 1 by making a capped log the record, or failed on criterion 4
by keeping a second event subsystem beside the record. This design keeps one durable record per role
and gives it a **change feed** — a table, not a second system.

## 1. The model

Every process owns exactly one record. The record has ordinary tables (calls, sessions, plugins,
tokens, denials, deploys) and one special table, `feed`: one row per change, with a monotonic `id`.
**Writing a fact and appending its feed row happen in one transaction.** That is the whole event
mechanism.

```
write:  capability ──▶ record tables ─┐
                                      ├─ same transaction
                        feed row ─────┘
push:   feed row ──▶ subscriber (WS / SSE)
resume: subscriber reconnects with its cursor ──▶ SELECT * FROM feed WHERE id > cursor
```

**This is the bus, and it keeps every guarantee a bus exists for** — it is simply not a second store:

| guarantee | how |
|---|---|
| total order per record | one writer; `feed.id` is monotonic in transaction order |
| durability | the feed row commits with the fact, or neither does |
| at-least-once | a push may repeat; a reader dedupes by `feed.id` |
| resume from any retained point | `read_feed(after: id)` is the same call live and after a reconnect |
| bounded memory | one bounded queue per subscriber; a slow reader is told how far behind it is |
| fan-out | subscribers are independent cursors; one slow reader cannot block another |
| retention | record and feed are pruned by one policy; the oldest retained id is the floor |
| no dual write | there is no second log that can diverge from the record |

So there is no separate journal, no epoch, no `lagged`/`reset` frame, and no second retention policy.
A cursor older than the retained oldest is answered with the oldest available id, and the subscriber
says "rows before this were pruned" — one fact, one sentence.

| concept | count | what it is |
|---|---|---|
| **record** | 1 per process | SQLite: the facts, their history, and their feed |
| **feed** | 1 per record | the change table a subscriber follows by id |
| **call** | 1 | MCP at the edge; one dispatch; one envelope |
| **terminal stream** | 1 | the only unrecorded data: bytes with offsets, for an open terminal |
| **guide** | 1 per device | generated from the capability specs |

Two roles: the **device** (one process per machine) and the **console** (one process for the operator).
Three carriers: MCP (AI), the channel (console ↔ device, one WS the device dials), SSE (console → page).
They are carriers of the call envelope, not separate mechanisms.

## 2. Invariants

1. **One writer, one home per fact.** The device's record holds machine facts; the console's record
   holds operator facts (and what it learned from devices, with the device's trace).
2. **A fact and its feed row commit together.** There is no window where a row exists without its
   notification, or a notification without its row.
3. **The feed is a hint; the record is the truth.** A subscriber that misses a push catches up by
   querying the record. Losing a push is allowed; losing a record row is a defect.
4. **One cursor: the feed id.** The page's SSE `id:`, the console's per-device cursor, and every
   resume are the same number in their own record.
5. **A capability is declared once.** Tools, schemas, guide, panel and conformance tests derive from
   `CapabilitySpec`.
6. **No door calls a capability directly.** Only the dispatch chain invokes a capability and writes the
   call row.
7. **Policy is a middleware**, open by default (token = identity), with its refusal path tested.
8. **Data-plane bytes are never recorded.** Terminal output is pushed with offsets; a lost chunk is a
   `stream_gap`, not a shorter record.
9. **A plugin is a child process**; its tools enter the same registry and dispatch; its lifecycle is
   recorded and feeds the same feed.
10. **One version, one installer, forward-only schema.** Upgrade backs the record up before migrating;
    a rollback restores that backup.

## 3. The flows

### 3.1 A call

```
door ─▶ CallRequest ─▶ identity ─▶ policy ─▶ record(start) ─▶ capability ─▶ record(end) ─▶ answer
```

Each `record(start)`/`record(end)` is a row plus a feed row, one transaction. The answer's
`{feed_id}` is what the console writes on its own row. A door never sees a capability; a capability
never writes the call row.

### 3.2 Live updates

The console subscribes to a device's feed by sending `{"feed":{"from": N}}` on the channel; the device
pushes `{"feeds":[{"id":N+1,"kind":"call.finished",…}]}`. On reconnect, the console asks from its stored
cursor and the device answers from the record. The console writes its own rows (operator view) and its
own feed; the page follows the console's feed over SSE (`id:` = feed id, `Last-Event-ID` = cursor).

No gap frames: the only loss fact is "the oldest retained id is X", answered by the record.

### 3.3 The terminal

A channel `subscribe` carries one `terminal.read` — the one audited call — then pushes chunks with byte
offsets on the session's notifier. The page reads `/terminal/:device/:session/stream` (SSE). This is the
one push that is not a record row, because a PTY flood must not become the record.

## 4. Modules

```
crates/
  protocol/   envelope, channel frame, feed row, journal answers, guide; fixtures; generated TS
  core/       registry, policy decision, sessions — pure, no I/O
  store/      SQLite: open, migrate, record operations, feed operations, retention, backup, integrity
  device/     bin summrise-device: capabilities/ entries/ dispatch/ record/ supervisor/ update/
  console/    bin summrise-console: devices.rs channel.rs record/ http/ page/
  ui/         design tokens
  platform/   the Windows boundary
  mesh/       NAT traversal
  shell/      bin summrise-shell
  launch/     bin summrise-launch
vendor/portable-pty
```

Ten crates, four binaries. The page role is a module with no key accessor (the boundary is a type, not
a crate); the release document is a protocol type; the wasm decision layer is deleted in favour of
generated types.

### 4.1 The capability contract

```rust
pub trait Capability: Send + Sync {
    fn spec(&self) -> &'static CapabilitySpec;
    fn call(&self, tool: &str, args: &Args, cx: &CallCx<'_>) -> ToolOutcome;
    fn dispose(&self, cx: &CallCx<'_>) {}
}

pub struct CapabilitySpec {
    pub name: &'static str,
    pub tools: &'static [ToolSpec],   // name, schema, destructive label, guide
    pub guide: &'static str,
    pub panel: &'static [PanelEntry],
}
```

The registry is the union of the specs. `tools/list`, the guide, the tool catalogue, the panel and the
docs are renderings of the same data. Adding a tool is one module and its test.

### 4.2 The record contract

Append a fact and its feed row in one transaction; list and paginate by time, tool, token, device,
status; aggregate a day; find by trace; read the feed after a cursor; prune by policy (record and feed
together); back up and restore; check integrity at boot; refuse a second writer. SQLite is the default
engine; SQL lives only in `store`, and callers use record operations.

### 4.3 The bus contract

`publish(fact) -> feed.id` is part of the same transaction as the fact. `read_feed(after: id, limit)
-> rows` is the only read a subscriber needs; live push is an optimization of it. A subscriber's state
is one `id`; a reconnect replays from it; a pruned cursor answers the oldest retained id. There is no
subscription registry in the store, no epoch, and no replay buffer beyond the retained rows: the
record is the buffer.

### 4.4 The transaction contract

The rule "a fact and its feed row commit together" is only real if the transaction mechanism is pinned.
This is the pin:

- **Engine settings.** `journal_mode=WAL`, `synchronous=FULL` (the record's promise outranks write
  speed; writes are human-scale), `foreign_keys=ON`, `busy_timeout=5000`.
- **One writer, many readers.** A single write connection issues `BEGIN IMMEDIATE` (no lock upgrade
  surprises); read-only connections with `query_only=ON` serve pages and tools. SQLite's single-writer
  rule is per database, and each process owns one database, so there is no cross-process contention.
- **Short transactions.** One unit of work is one write transaction. **No capability call runs inside a
  transaction.** `record(start)` and `record(end)` are separate transactions: a crash between them
  leaves a *started* row, which is a fact — the call was in flight — not a lost one. Boot marks
  interrupted calls as such, with a row and a feed entry.
- **The feed row is self-contained.** It carries the kind, the ids and the one-line summary a
  subscriber draws, so a subscriber never needs a second query to render an event. The full fact lives
  in its table.
- **No distributed transaction.** The device records; the console records its own perspective after the
  answer. If the console dies before writing, the device's row and its trace still answer the question.
  If the device dies, the console's row says the call was attempted — the trace joins them when the
  device returns.
- **Pruning and migration are their own transactions.** Rows are pruned in bounded batches (1 000) with
  the feed; a migration runs in one transaction after `VACUUM INTO` backup and a `user_version` check;
  a build refuses a newer schema rather than guessing.
- **Tests.** Kill at each commit boundary; a writer held for the duration of a slow call; a reader
  against a write in flight; a busy retry; a migration interrupted mid-way; a restore from backup; an
  interrupted call detected at boot.

## 5. Failure modes

| Failure | What happens | Guarantee |
|---|---|---|
| device dies | launcher restarts; console resumes from its cursor | committed rows survive; the feed resumes where the record is |
| power loss | SQLite WAL replays | a committed fact and its feed row both survive, or neither does |
| record corrupt | boot `quick_check` fails: read-only and says so | never write over a damaged record; restore from backup |
| console down | device serves; its panel reads its own record | no fact depends on the console |
| device unreachable | console shows the last cursor and "offline" | no fabricated rows |
| cursor pruned | answer names the oldest retained id | the UI says "older rows were pruned"; no silent gap |
| upgrade | migrate in a transaction after `VACUUM INTO` | older build refuses newer schema; rollback restores backup |
| duplicate delivery | the same fact is written once by its writer | the console dedupes a device row by `(device, feed id)` |
| plugin crash loop | supervisor marks it failed and records it | one row, one feed entry |
| crash between `start` and `end` | boot marks the call interrupted (row + feed) | the in-flight fact is recorded, not lost |
| writer busy / lock | bounded retry with backoff, then the call fails loudly | no silent partial write |
| read during a write | WAL snapshot; readers never block | a page never sees a half-written call |

## 6. Why this is the best for this product

| criterion | v1 | this design |
|---|---|---|
| record | JSONL audit capped at 64 MiB, silently rotated | SQLite, policy retention, backup, integrity |
| live updates | a separate bus with `seq`, `epoch`, `lagged`, `reset` | the record's change feed, with one cursor; every bus guarantee named and tested |
| adding a tool | 4 places + docs | 1 module |
| adding a wire field | ~20 sites | 1 type + generated TS |
| resume | replay the log, or reset the subscriber | query the record after the cursor |
| crates | 13 | 10 |

Rejected: **polling-only** — the live view and the terminal make push worth keeping; **journal as the
only record** — capped and rebuildable; **a separate bus log beside the record** — two orders, two
retentions, an epoch to reconcile them: that is the v1-shaped mess this design removes while keeping
every delivery guarantee; **everything-is-a-plugin** — provenance and packaging cost; **microservices**
— a hop in the hot path.

## 7. Slices

| Slice | Lands | Acceptance |
|---|---|---|
| S0 | this spec + `v1-parity.md`; v1 through the release pipeline | reviewed; v1 installer installed |
| S1 | `store`; console record + feed; import `events.jsonl`; Activity reads the record and follows its feed | same rows; import idempotent; live view works |
| S2 | device record + feed; `journal.*` answered from the record; the old bus log and the epoch deleted; the feed becomes the bus | answers unchanged; rows survive retention |
| S3 | capability contract + generated registry + guide | one-file capability |
| S4 | entries + dispatch chain; four doors | conformance suite per door |
| S5 | `protocol` + generated TS + fixtures; wasm deleted | one type per wire change |
| S6 | crate merge, dead code, v2.0 | released, deployed, rollback drilled |

## 8. Review focus

- The feed and the fact commit together — crash between them is impossible.
- The resume path: cursor, pruning, and the "older rows were pruned" fact.
- The page reads the record, not a re-derivation.
- One capability is one file, demonstrated.
- No slice makes `hub/v1` unshippable.
