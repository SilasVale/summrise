# Memory Entry Ids and the Two Missing Tools — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a memory entry an identity, so `memory.update` and `memory.delete` can exist, and build the console page that uses them.

**Architecture:** `memory.jsonl` becomes a journal of operations (`append`/`update`/`delete`) replayed in reverse for newest-first reads; the file stays append-only, and a snapshot rewrites it when it grows past `2 × live + 64` lines. The device gains two destructive tools; the console gains a Memory page that calls them through a grant.

**Tech Stack:** Rust (device crate, `serde_json`, `sha2`, `rand_core`), React + TypeScript (console page), the existing `callTool` grant path.

**Spec:** `docs/superpowers/specs/2026-10-25-memory-entries-get-ids-design.md`

## Global Constraints

- The file is append-only: an existing byte is never rewritten except by the snapshot's atomic rename (a `fsutil::create_new_sibling` temporary → `memory.jsonl`).
- Caps unchanged: 1 MiB serialized entry, 32 tags × 128 bytes, 100 search results, 10 000 **live** entries.
- `memory.update` validates exactly as `memory.append` (same caps, same serialization check).
- Both new tools are `destructive: true` in the registry; unknown id is a typed `no_such_entry { id }`.
- Repository language is English: code, comments, commits.
- Every task's verification runs the real suites: `cargo test -p summrise-device`, `cargo test -p summrise-core`, `cargo test -p summrise-console`, `./scripts/build-web.sh`, `npx tsc --noEmit`.

## Review Focus

1. **Two byte-identical legacy entries** — they must get distinct ids, and deleting one must not delete the other; the reader must not collapse them.
2. **An update followed by a search** — the pre-update text must never be returned, and the entry must not appear twice.
3. **A delete of an id that was never live** — typed `no_such_entry`, not a silent success.
4. **The journal at the entry ceiling** — an update must not consume an entry slot; the 10 001st live append is `full`.
5. **A crash between snapshot write and rename** — the old journal must still be the one read.

---

### Task 1: The journal record, ids, and a replayed read

**Files:**
- Modify: `device/device/src/capabilities/memory.rs` (record type, `append`, `search`, `list`, `export`, `count_entries`)
- Test: same file, `mod tests`

**Interfaces:**
- Produces: `struct Record { op: Op, id: Option<String>, entry: Option<Entry> }` with `#[serde(tag = "op", rename_all = "lowercase")]`; `fn mint_id() -> String` (8 random bytes hex); `fn legacy_id(line: &str, occurrence: usize) -> String` (`sha256(line)` first 16 hex, `-{occurrence}` when `occurrence > 1`); `Entry { id: String, text: String, tags: Vec<String> }`.
- Produces: `Memory::append(&self, text, tags) -> Result<Entry, MemoryError>` (was `Result<(), _>`).

- [ ] **Step 1: Write the failing test** — a store with two legacy lines, one duplicated, searches as three entries with distinct ids; an append returns its id; the file's first line still parses as the old bare entry.
- [ ] **Step 2: Run it** — `cargo test -p summrise-device memory` → FAIL (`Entry` has no `id`).
- [ ] **Step 3: Implement** the record type, `mint_id`, `legacy_id`, replay in `search`/`list` with `seen`/`deleted` sets, `append` writing `{"op":"append","id":…,"entry":{…}}` and returning it; a bare line reads as an append.
- [ ] **Step 4: Run it** → PASS.
- [ ] **Step 5: Commit** — `memory: an entry has a name`.

### Task 2: update, delete, the live cap, and the snapshot

**Files:**
- Modify: `device/device/src/capabilities/memory.rs`
- Test: same file

**Interfaces:**
- Produces: `Memory::update(&self, id: &str, text: &str, tags: &[String]) -> Result<Entry, MemoryError>`; `Memory::delete(&self, id: &str) -> Result<(), MemoryError>`; `MemoryError::NoSuchEntry { id: String }`.
- **Carried from Task 1's review (Important):** `replay()` takes the read lock, so it cannot be called from `append`/`update`/`delete`/`compact` while they hold the write lock (std's `RwLock` is not reentrant). Split it: a lock-free `replay_locked(&self) -> Result<Vec<Entry>, MemoryError>` core that does the file work, and a thin `replay()` wrapper that takes the read lock; the write-side callers call the core under the write lock.
- **Carried from Task 1's review (minor, same file):** one whitespace predicate for blank-line detection and `count_entries` (they currently disagree on non-ASCII whitespace); `TooLarge`/`Corrupt` Display strings that describe what is actually measured/raised; the module doc's "one pass" claim (reads now do an offset pass plus a reverse replay — say so).
- Produces: `fn compact(&self, live: &[Entry]) -> io::Result<()>` writing a `fsutil::create_new_sibling` temporary in reverse view order, then renaming; threshold `2 * live + 64` lines, evaluated after every journal write; a failed snapshot is logged and retried on the next write.
- Consumes: Task 1's `Record`, `legacy_id`, replay.

- [ ] **Step 1: Write the failing tests** — update replaces text and appears at the update's position; delete removes from search/list/export; second delete is `NoSuchEntry`; an oversized update is `TooLarge`; 80 edits on a 1-entry store cross the threshold and leave a snapshot whose replay equals the pre-compaction view (the snapshot is written in reverse view order, so replay reproduces it exactly).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** update/delete/cap/compaction as the spec describes.
- [ ] **Step 4: Run** → PASS; then `cargo test -p summrise-device` (whole suite) and `cargo clippy -p summrise-device --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** — `memory: a correction is a new line, and the file compacts itself`.

### Task 3: The two tools on the wire

**Files:**
- Modify: `device/core/src/registry.rs` (two `ToolDef`s, destructive; `memory.update`, `memory.delete`; the exact-list test)
- Modify: `device/device/src/mcp.rs` (Handler variants, `HANDLERS`, dispatch, schemas, the wire-list test)
- Test: both files' `mod tests`

**Interfaces:**
- Consumes: Task 2's `Memory::update` / `Memory::delete` and `MemoryError::NoSuchEntry`.
- **Carried from Task 2:** remove the transitional `#[cfg_attr(not(test), allow(dead_code))]` on
  `update`, `delete`, `liveness_locked` and `NoSuchEntry` — the tools are what make them reachable.
- Produces: tools `memory.update {id, text, tags}` → the updated entry (`id`, `text`, `tags`); `memory.delete {id}` → `{"deleted": true, "id": …}`; `memory.append` result gains `id`; `memory.list`/`memory.search` entries gain `id`.

- [ ] **Step 1: Write the failing tests** — registry list has 38 tools / 10 capabilities with the two marked destructive; wire list matches; a call to `memory.update` with an unknown id is a tool error whose text names the id.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** registry and mcp wiring.
- [ ] **Step 4: Run** → PASS; `cargo test -p summrise-device` and `cargo test -p summrise-core`.
- [ ] **Step 5: Commit** — `memory.update and memory.delete: the last two undecided tools`.

### Task 4: The console Memory page

**Files:**
- Create: `device/console/web/src/MemoryPage.tsx`
- Modify: `device/console/web/src/App.tsx` (a `memory` page in the rail), `Rail.tsx` (icon + label), `app.css` (page styles if needed)
- Modify: `device/console/web/src/GrantForm.tsx` — its fallback tool list predates `memory.update`,
  `memory.delete`, `host.facts`, `screen.capture`, `terminal.screen`, `memory.list`, `memory.export`
  and the `system.*` tools; the list is only the fallback for a failed `/tools` request, but a stale one
  offers a surface that does not exist.

**Interfaces:**
- Consumes: `callTool(grant, "memory.list" | "memory.search" | "memory.update" | "memory.delete" | …)`; `Grant` from `./terminal`.
- Produces: a page that lists entries with id/tags, filters by tag, searches, edits and deletes.

- [ ] **Step 1: Write the failing check** — `npx tsc --noEmit` passes today; the new page must keep it at zero. (The page has no unit-test harness in this repo; its gate is the typecheck, the build, and a canary call, as with the other panes.)
- [ ] **Step 2: Implement** the page, the rail entry and the styles.
- [ ] **Step 3: Verify** — `./scripts/build-web.sh` and `npx tsc --noEmit`.
- [ ] **Step 4: Commit** — `console: the memory browser`.

### Task 5: Verification, and the plan's own record

**Files:**
- Modify: `docs/superpowers/specs/2026-10-25-memory-entries-get-ids-design.md` (a "verified" section with the canary evidence)
- Modify: `docs/superpowers/specs/2026-10-25-the-iteration-plan.md` (mark P2 done, re-point P3)

- [ ] **Step 1:** Run every gate: `cargo test -p summrise-device`, `-p summrise-core`, `-p summrise-console`, `cargo clippy` on both, `cargo xwin check` on both, `./scripts/build-web.sh`, `npx tsc --noEmit`.
- [ ] **Step 2:** Release the device (`scripts/release-canary.sh 0.3.30`) and the console (`SUMMRISE_BUILD_VERSION=0.3.35`), deploy both, and on the canary: append, search, update, delete, export, through the device's MCP with the install token.
- [ ] **Step 3:** Record the evidence in the design doc and update the plan's phase table; commit.
