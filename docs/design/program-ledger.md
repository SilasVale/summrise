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

- 2026-11-13 — S3 shipped (`a59ec79`): plugin.starting/.ready/.stopped/.failed/.tools_changed on the
  journal; the host mirrors the bus into the supervisor's Weak<Inner>; 37 plugin tests. The boot-order
  gap was closed in `94e1d9d` (the journal opens in main before start_all).
- 2026-11-13 — `feat/s2-journal` merged into `hub/v1` (fast-forward): S2 + S3 + S5 + the Activity
  device events. Boundary checks: bus 12, console lib 170.

- 2026-11-13 — S4 first slice shipped and merged (`hub/v1`): `page.rs` owns `/`, `/app`, the icon and
  `/events`; `Page` holds only the page, the icon, `Audit`, `Running` and the clock, enforced by a
  source-scanning guard test; the `/events` operator gate stays in control; the device-event replay
  ring moved to `Audit`. 171 console tests. `/health` and the read-only JSON routes stay in control.

- 2026-11-13 — S6 shipped and merged (`hub/v1`): the shell resolves the console from
  `SUMMRISE_CONSOLE_URL` → `<data>/console.json` → build default, probes it for 900 ms and opens
  `http://127.0.0.1:<port>/panel` when it does not answer; the installer builds `summrise-shell` and
  checks all four exes; the CI clippy lists name bus and ui. Windows: `cargo xwin check` (msvc) and
  `--target x86_64-pc-windows-gnu` pass; plain msvc `cargo check` needs MSVC's `lib.exe`. Shell 24
  tests. What still needs Windows: link/NSIS/signing and an interactive `--check-window`.

- 2026-11-13 — S2 close-out shipped and merged: the device sends `reset{snapshot_seq, lost}` before
  the retained rows when a cursor falls off; the console subscribes from its last seen `seq + 1`
  (live-only on a first connect) and relays the reset as an SSE `reset` event; the page adds the lost
  range to its existing gap count and rereads. Commits `30baf09` (page) and `bd72800` (Rust).
  Tests: console 173, device console 30, page 24.

- 2026-11-13 — S4b shipped and merged (`f1d5366`): `summrise-console-page` owns `PageSource`, `Page`, the
  router and the assets; it has no workspace dependency, so it cannot name a key. The console
  implements `PageSource` over `Audit`/`Running`; the gate reports 13 packages, 19 edges, 0 upward.
  Also fixed the integration harness that the S1 subscription frame had broken (the fake device now
  skips `events`/`unsubscribe` frames); the full console integration suite passes (254).

## In flight (branch `feat/s3b-plugin-crate`)

- S3b: extract `summrise-plugin-host` as a crate (core + bus only), so the supervised plugin host is a
  subsystem rather than a module inside the device.

## Next (in order)
2. S4b — extract `console-page` as a crate so "the page holds no keys" is a compile-time boundary.
3. S3b — extract `plugin-host` as a crate; the lifecycle events are already on the bus.
4. UI — the Device page's live tail (sessions, plugins) from the journal.
5. Windows runner — build the installer, run the shell's `--check-window`, sign.

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
