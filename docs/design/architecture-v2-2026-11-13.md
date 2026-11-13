# Summrise v2 — the architecture

> **Status: proposal on `next`, 2026-11-13.** This is the spec the slices implement; the feature gate is
> `v1-parity.md`. It restates the earlier drafts by interface: every promise lives on the interface that
> owns it, with its guarantee and its test. There is no separate chapter of rules.

![One record per role; its change feed is the bus](assets/architecture-target-proposal.png)

## 0. What "best" means

1. **The record does not lose facts.** Not by rotation, crash, upgrade, or reset.
2. **Adding a thing costs one place.**
3. **The loop is short.**
4. **The operational surface is small.**
5. **A stranger can find things.**

## 1. The model

Two roles. The **device** is one process per machine: capabilities, a record, a bus. The **console** is
one process for the operator: devices, a channel, a record, a page. Each owns exactly one SQLite
record; the record's **change feed** is the bus. Three carriers: MCP (AI ↔ device/console), the channel
(console ↔ device, one WebSocket the device dials), SSE (console → page). The only unrecorded stream is
terminal output.

Dependencies point at the interfaces below, never at an implementation: `Dispatch` depends on
`Capability`, `Capability` receives `CallCx`, the page depends on `Record`'s read operations, the
console depends on the channel's frames.

## 2. Interfaces

### 2.1 `Capability` — a unit of work

```rust
pub trait Capability: Send + Sync {
    fn spec(&self) -> &'static CapabilitySpec;
    async fn call(&self, tool: &str, args: &Args, cx: &CallCx<'_>) -> ToolOutcome;
    async fn dispose(&self, cx: &CallCx<'_>) {}
}

pub struct CapabilitySpec {
    pub name: &'static str,
    pub tools: &'static [ToolSpec],   // name, JSON schema, destructive label, guide
    pub guide: &'static str,
    pub panel: &'static [PanelEntry],
}
```

**Guarantees.**
- `call` is invoked only for a tool in `spec`, after the dispatcher has recorded the start row and never
  inside a transaction.
- `CallCx` exposes identity, trace, deadline, cancellation and read-only record access — **no record
  writer, no transaction handle**. A capability that tries to write the call row does not compile.
- A refusal is a sentence (`ToolOutcome::Refused`), not a protocol error; a failure the caller can act
  on is a `ToolOutcome::Failed` with a next action.
- `dispose` runs once, on unload or shutdown, after which no new `call` is admitted; sessions, monitors
  and children it created are gone when it returns.
- Tool names are namespaced by `spec.name`; the registry is the union of the specs, built once.

**Tests.** Conformance suite per capability: every declared tool executes or refuses with a sentence;
an undeclared tool never reaches `call`; `dispose` leaves nothing behind; the guide names exactly its
tools; the panel entries resolve.

### 2.2 `Record` — the durable facts

```rust
pub trait Record: Send + Sync {
    fn record(&self, fact: Fact) -> Result<FeedId>;
    fn calls(&self, query: CallsQuery) -> Result<Page<CallRow>>;
    fn day(&self, range: DayRange) -> Result<DayCounts>;
    fn find_trace(&self, trace: &Trace) -> Result<Vec<CallRow>>;
    fn feed(&self, after: Option<FeedId>, limit: usize) -> Result<FeedPage>;
    fn prune(&self) -> Result<Pruned>;
    fn backup(&self, to: &Path) -> Result<()>;
    fn check(&self) -> Result<Health>;
}
```

**Guarantees.**
- `record` writes the fact **and its feed row in one transaction**, and returns the feed id. It is the
  only write path; there is no public transaction handle, so nothing can write one without the other.
- One writer per database (`BEGIN IMMEDIATE`), many read-only connections; WAL, `synchronous=FULL`,
  short transactions; no capability call runs inside one.
- `feed(after)` returns committed rows in id order. Live push is an optimization of it; a reconnect is
  the same call. A cursor older than the oldest retained id is answered with that id (`pruned`), never
  with silence.
- `calls`, `day` and `find_trace` are indexed queries over the fact tables, not scans of the feed.
- `prune` deletes by policy in bounded batches, fact and feed rows together; `backup` is consistent
  (`VACUUM INTO`); `check` fails closed — a damaged record is opened read-only and says so.
- Schema is forward-only: a migration runs in one transaction after a backup, and an older build
  refuses a newer schema instead of guessing.

**Tests.** Kill between the two commits (a started row with no end is a fact, and boot marks it
interrupted); a duplicate trace records once; feed replay from a cursor and from a pruned cursor; a
reader during a write sees no half row; a migration interrupted mid-way; a restore from backup; a
newer schema refused.

### 2.3 `Bus` — the record's change feed, pushed

```rust
pub trait Bus {
    fn subscribe(&self, after: Option<FeedId>) -> Subscription;   // replay, then live
    fn publish(&self, row: FeedRow);                              // called by Record after commit
}
```

**Guarantees.**
- Total order per record: `feed.id` is monotonic in commit order.
- At-least-once: a push may repeat; a reader dedupes by `feed.id`.
- One bounded queue per subscriber; a slow subscriber is told how far behind it is and catches up from
  `Record::feed`, not from a replay buffer.
- Subscribers are independent cursors; one slow reader cannot block another, and a subscriber that
  disconnects loses nothing — its cursor is in its own record.
- The feed row is self-contained (kind, ids, one-line summary): a subscriber renders without a second
  query.

**Tests.** Two subscribers with one held back; a disconnect and a reconnect from the cursor; a pruned
cursor; a publish that repeats (dedupe); a slow subscriber that never blocks the writer.

### 2.4 `Dispatch` — the one path a request travels

```rust
pub trait Dispatch {
    async fn call(&self, request: CallRequest, caller: Caller) -> CallOutcome;
}
```

**Guarantees.**
- Every door (MCP, channel, panel, CLI) produces a `CallRequest` and calls this one method; no door
  invokes a capability.
- The chain is ordered: identity → policy → record(start) → capability → record(end) → answer. Policy
  is a middleware, open by default (token = identity); its refusal path is tested.
- The call row is written by the chain, never by the capability; `record(start)` and `record(end)` are
  separate transactions, so a long call never holds the write lock.
- The answer carries `{ok, result|error, feed_id}`; a refusal names the next action.

**Tests.** The same conformance suite runs against every door; a refusal is a sentence with a next
action; a policy middleware can be switched on and off; the call row exists exactly once per call.

### 2.5 `Channel` — the console ↔ device carrier

```rust
pub enum Frame {
    Call { id, mcp, token, trace },
    Subscribe { id, feed_from },
    Follow { id, terminal_read, cursor },
    Unsubscribe { id, sub },
}
```

**Guarantees.**
- `id` correlates a request with its answer; `trace` joins the two records.
- Send timeout 5 s (a wedged socket is closed, not accumulated); reply timeout 30 s (a 504 says the
  device may still be working); at most N in flight per device.
- A feed subscription is `{feed_from}`; pushes are `{id, feed:[…]}`; a reconnect re-subscribes from the
  console's stored cursor. The console writes a device fact once, deduped by `(device, feed id)`.
- The terminal follow carries exactly one audited `terminal.read`; its chunks carry byte offsets and are
  never journaled.

**Tests.** A wedged socket; a reply after the timeout; a reconnect mid-stream; a duplicate feed row; a
terminal gap.

## 3. The flows

- **Call:** door → `Dispatch::call` → `Record::record(start)` → `Capability::call` → `Record::record(end)`
  → answer with `feed_id`; `Bus::publish` after each commit.
- **Live:** the console subscribes to the device's feed; the page subscribes to the console's feed over
  SSE (`id:` = feed id, `Last-Event-ID` = cursor). Both recover by `Record::feed(after)`.
- **Terminal:** a channel follow → chunks with offsets → the page's SSE. The one stream that is not a
  record row.

## 4. Failure modes

| Failure | What happens | Guarantee |
|---|---|---|
| device dies | launcher restarts; console resumes from its cursor | committed rows survive; the feed continues where the record is |
| power loss | WAL replays | a fact and its feed row both survive, or neither |
| record corrupt | `check` fails: read-only, says why | never write over a damaged record; restore from backup |
| console down | device serves; its panel reads its own record | no fact depends on the console |
| device unreachable | console shows the last cursor and "offline" | no fabricated rows |
| cursor pruned | `feed` names the oldest retained id | the UI says rows were pruned; no silent gap |
| crash between start and end | boot marks the call interrupted | the in-flight fact is recorded, not lost |
| duplicate line | readers dedupe by `feed.id` / trace | one row per fact |
| upgrade | migrate in a transaction after backup | older build refuses newer schema; rollback restores |
| plugin crash loop | supervisor records it and unregisters its tools | one row, one feed entry |

## 5. Why this is the best for this product

| criterion | v1 | this design |
|---|---|---|
| record | JSONL audit capped at 64 MiB, silently rotated | SQLite, policy retention, backup, integrity |
| live updates | a separate bus with `seq`, `epoch`, `lagged`, `reset` | the record's feed; one cursor; the same guarantees |
| adding a tool | 4 places + docs | one `Capability` implementation |
| adding a wire field | ~20 sites | one `protocol` type + generated TS |
| resume | replay the log or reset the subscriber | `Record::feed(after)` |
| crates | 13 | 10 |

Rejected: **polling only** — the live view and the terminal make push worth keeping; **journal as the
only record** — capped and rebuildable; **a bus beside the record** — two orders, two retentions, an
epoch to reconcile them; **everything-is-a-plugin** — provenance and packaging cost; **microservices** —
a hop in the hot path.

## 6. Slices

| Slice | Lands | Acceptance |
|---|---|---|
| S0 | this spec + `v1-parity.md`; v1 through the release pipeline | reviewed; v1 installer installed |
| S1 | `store` + console `Record` and `Bus`; import `events.jsonl`; Activity reads the record and follows its feed | same rows; import idempotent; live view works |
| S2 | device record + feed; `journal.*` answered from the record; the old bus log and epoch deleted | answers unchanged; rows survive retention |
| S3 | `Capability` contract + generated registry + guide | one-file capability |
| S4 | `Dispatch` + four doors | conformance suite per door |
| S5 | `protocol` + generated TS + fixtures; wasm deleted | one type per wire change |
| S6 | crate merge, dead code, v2.0 | released, deployed, rollback drilled |

## 7. Review focus

- Each interface's guarantees are tested by the named test, not by prose.
- The record's failure path before its happy path.
- The page reads the record rather than re-deriving it.
- A capability is one file, demonstrated.
- No slice makes `hub/v1` unshippable.
