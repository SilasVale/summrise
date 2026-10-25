# Memory entries get ids, and a journal that can say "this one changed"

> **Status: design, 2026-10-25.** The plan's P2. It closes the two tools the surface audit could not
> decide (`memory.update`, `memory.delete`) — not by adding them, but by giving an entry an identity
> first. Without an id there is nothing for `update` to name and nothing for `delete` to forget.

## The problem, in one line

`memory.jsonl` is a file of facts in the order they were written, and the capability can only read
that order. An operator who wants to correct a typo, add a tag, or retire an obsolete note has no
tool: the only way to change what the device remembers is not to have written it.

Two things are missing, and they are the same thing:

1. **An entry has no name.** Append returns nothing, and nothing later can refer to what was written.
2. **The file is append-only by construction**, and correctness currently depends on that ("an
   existing byte is never rewritten"). Any update mechanism has to keep that promise or break the
   invariant every reader relies on.

## The decision: a journal, not a mutated file

The file stays append-only. What changes is what a line means: a line becomes one **operation** in a
journal, and the live entries are what you get by replaying it.

```jsonc
{"op":"append","id":"3f2a…","entry":{"text":"…","tags":["…"]}}
{"op":"update","id":"3f2a…","entry":{"text":"… corrected","tags":["…","…"]}}
{"op":"delete","id":"3f2a…"}
```

Three rules make replay unambiguous:

- **An append names its entry.** New lines carry an id, minted from the OS generator when the entry is
  written (8 random bytes, hex).
- **An update replaces the entry at its own position.** Reverse order is the reader's order, so the
  update line — which is later in the file — is the version a reader sees first. The append it
  supersedes is skipped by id.
- **A delete is a tombstone.** The line says the id is gone; nothing is rewritten. Deleting an id
  twice is `no_such_entry` the second time, because the second delete names nothing that is live.

### Legacy lines, and the ids they never had

A line with no `op` is an append — the file as it exists today. Its id is derived at read time from
the raw line: `sha256(line)` truncated to 16 hex characters, with a `-2`, `-3` suffix for identical
lines that appear more than once (two byte-identical entries are two entries, and they must be
distinguishable to be deletable separately).

The derivation is stable because it is over the line's own bytes, and it stops being a derivation at
the first compaction: a snapshot writes every live entry with its id, so older entries acquire real
ids on disk.

## Reading: a reverse scan with two sets

`search` and `list` currently walk the file backwards from the end, which is what makes them cheap and
what makes the result newest-first. That does not change. What changes is that the walk keeps two
small sets:

- `seen` — ids already emitted (or superseded by an update line).
- `deleted` — ids a tombstone names.

For each line, walking backwards:

| line | what the reader does |
|---|---|
| `append` (or a bare legacy line) with an id not in `seen`/`deleted` | emit it, add to `seen` |
| `update` with an id not in `seen`/`deleted` | emit the updated entry (it is newer than its append), add to `seen` |
| `delete` | add the id to `deleted` |
| anything already seen or deleted | skip |

An update therefore appears at the update's position in the newest-first order, not at the original
append's. That is the honest order: the correction is the newest fact about that entry.

## The caps

| cap | before | after |
|---|---|---|
| one entry's serialized size | 1 MiB | unchanged, checked on the operation line |
| tags | 32 × 128 bytes | unchanged |
| live entries | 10 000 (line count) | 10 000 **live** entries — a journal may hold more lines, and updates must not consume entry slots |
| search results | 100 | unchanged |
| export | 2 MiB of raw lines | 2 MiB of **live** entries, with ids |

The live-entry count is computed by replay at append time. At the ceiling (10 000 entries) that is a
10 000-line pass, which is the same order of work the search already does and is bounded by the caps
that exist.

## Compaction

The journal grows with every change, so at some point it is rewritten as a snapshot: every live entry
as one `append` line with its id, written in the **reverse of the current view** — oldest first — so
that replaying the snapshot reproduces the view exactly. "Original append order" would not: an entry
that was updated lives at its update's position in the view, and a snapshot in first-append order would
move it back and change what a reader sees. The rewrite goes to `memory.jsonl.new` and is
renamed over the old file — a crash leaves one file or the other, the shape the registry already uses.

The trigger is measured, not guessed: **2 × live entries + 64 lines**, evaluated after **every**
journal write — append, update and delete. An edit-only store would otherwise grow without limit (at
the entry ceiling appends are refused, so an append-only trigger can never fire again), and a
delete-heavy store would fall below the threshold and never be re-checked.

A snapshot that fails after the write has landed is logged and retried on the next write; it is not
reported to the caller, because the caller's operation did land durably and an error would invite a
retry that duplicates it.

## The two tools

| tool | arguments | answer |
|---|---|---|
| `memory.update` | `id`, `text`, `tags` | the updated entry (`id`, `text`, `tags`) |
| `memory.delete` | `id` | `{"deleted": true, "id": …}` |

Both are destructive (`a later read cannot undo them`), and both fail with a typed
`no_such_entry { id }` for an id that is not live. There is no timestamp on an entry, and none is added: the capability has no clock, `summrise-core`
is documented as having none, and a field that is computed for one answer and never persisted would
be a fact no later read could reproduce. The journal's line order **is** the time, and an update
moves an entry to its own position.

`memory.update` validates exactly as
`memory.append` does — the same caps, the same serialization check — because an update that could
store what an append refuses would be a way around the caps.

`memory.append` gains an id in its result. That is an additive change to a tool that returned
`{"appended": true}`: a client that ignores the field sees the same success.

`memory.list` and `memory.search` return entries with their ids (`{"id","text","tags"}`), which is what
makes a page able to name the entry it is about to edit.

`memory.export` returns the live entries as JSONL (each line `{"id","text","tags"}`), not the journal.
The journal is an implementation detail; the export is what the device remembers.

## What this does not do

- It does not add a delete-after-N-days policy, or any retention. That is a separate decision.
- It does not index. Search stays a reverse scan; the cap on entries is what makes it acceptable.
- It does not let an update change an id, or resurrect a deleted id: `append` mints a new one.

## Downgrade is out of scope, and the release note says so

A pre-change build parses `{"op":"append",…}` as a corrupt line, so rolling a device back after it has
written journal lines makes memory reads fail until it is upgraded again. The bytes are intact and the
reader is not. This is accepted: the release that ships the journal says it in its notes, and a
rollback after a journal write is a re-upgrade, not a downgrade.

## Acceptance

1. A store with two legacy lines, one of them duplicated: both searchable, both deletable by their
   derived ids, and the duplicate gets a distinct id.
2. `append` → `update` → `search`: the old text is not returned, the new text is, and the entry appears
   at the update's position.
3. `delete` removes the entry from `search`, `list` and `export`; a second `delete` is `no_such_entry`.
4. The live-entry cap counts edits as edits: 10 000 appends followed by one update succeed; the
   10 001st append is `full`.
5. Compaction: after enough changes to cross the threshold, the file is a snapshot whose replay equals
   the pre-compaction view, ids included, and a crash between write and rename leaves the old file.
6. `memory.update` with an oversized text is refused by the same cap as `append`.
7. Registry and wire lists carry 38 tools / 10 capabilities, and `memory.update`/`memory.delete` are
   marked destructive.

## What the console page will use

The Memory page reads with `memory.list` / `memory.search` (one grant), shows ids and tags, edits
through `memory.update` and retires through `memory.delete`. It does not read the file and does not
need a new console route: memory is device state, and the grant path already audits every call.
