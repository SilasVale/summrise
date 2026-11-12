# Summrise program ledger

Updated 2026-11-13. The coordinator reads this first; the architecture of record
(`docs/design/architecture-of-record-2026-11-12.md`) is the constraint until it is amended.

## Goal

Ship Summrise v1 and keep improving it: one operator installs the device, enrolls it into the
console, connects AI clients over MCP, and can see, stop and trust everything that runs. Every item
below ladders to that; one round completes one major item end to end.

## Shipped

- 2026-11-12 — architecture of record approved: trusted plugins (no signature/sandbox), shell is the
  only desktop surface, consent is the token's `allow_destructive`, operator session sees everything.
- 2026-11-13 — S1 merged to `hub/v1` (`df5608e`): `summrise-bus` (journal, `seq`, fsync before publish,
  replay + live, torn tail), `device.started` and `session.opened/closed` published, channel
  `events.from_seq` frames, console relays device events into the SSE with `id:`, Last-Event-ID replay
  (console lib 170, tokens 15, device console::tests 29 + remote::tests 12, bus 5).

- 2026-11-13 — S2 segments shipped on `feat/s2-journal` (`d41abf2`): 4 MiB segments, 64 MiB
  retention, cross-segment replay, torn tail only in the last segment (9 tests).
- 2026-11-13 — S1's visible outcome (`48dbae5`): the Activity page renders `device` events as session
  lines (21 targeted tests, 1 175 page tests, tsc clean).

- 2026-11-13 — S2 complete on `feat/s2-journal` (`d41abf2`, `ba4b734`): segmented journal, 4 MiB
  rotation, 64 MiB retention, cross-segment replay, torn tail only in the last segment, `oldest_seq` /
  `reset{snapshot_seq, lost_from, lost_to}`, and an atomic `snapshot-<seq>.json` every 1 000 events.
  12 bus tests.

- 2026-11-13 — S5 shipped (`3ff2311`): `scripts/check-architecture.mjs` reads `cargo metadata
  --no-deps`, rejects an upward edge by name, has an in-memory self-test and a `--metadata` fixture
  mode; CI runs it before the cache. 12 packages, 18 workspace edges, 0 upward. The supervisor may
  not import the device (the earlier allowance was unused and is gone).

## In flight (branch `feat/s2-journal`)

- S3 plugin lifecycle events on the bus (sub-agent D).

## Next (order may change as the work teaches us)

1. S2 — snapshot every 1 000 events / 5 minutes, `reset{snapshot_seq, lost}`, retention tests.
2. A test that `device.started` reaches the journal from the router (the one call without one).
3. S3 — extract `plugin-host` from `device/device/src/plugins`, emit the lifecycle events.
4. S4 — console/console-page split; page holds no keys.
5. S5 — `scripts/check-architecture.mjs` reading `cargo metadata` in CI.
6. S6 — desktop follow-through: Windows packaging and the shell's panel path.
7. UI work driven by what the operator actually hits (Activity, Device, Connect).

## Rules

- One stage per branch; merge green work into `hub/v1`; never wait for approval between batches.
- Targeted tests per change; full checks at stage boundaries.
- Sub-agents get self-contained prompts and disjoint files; the coordinator reviews, commits, merges.
- A task that returns red is fixed by the same sub-agent or by the coordinator if the fix is small.
