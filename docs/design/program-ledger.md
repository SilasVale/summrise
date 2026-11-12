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

- 2026-11-13 — S3b shipped and merged (`22f2ad8`): `summrise-plugin-host` owns the host, the session reader and
  the logs, depending only on core and bus; the device re-exports it as `plugins`. A `test-support`
  feature carries the scripted launcher for device tests. 32 plugin-host tests, 9 device plugin tests,
  gate 14 packages / 22 edges / 0 upward.

- 2026-11-13 — The console's operator surface shipped and merged: a token minted from the console
  defaults to `allow_destructive: true` with a one-click off and an on-state note; the Device page
  shows the chosen machine's journal events (session/plugin lifecycle, newest first, 20 cap). The
  embedded page was rebuilt (`8a9f2fe`) so the binary ships the new UI, and `build-web.sh --check`
  is green. 1 182 page tests, layout contract included.
- 2026-11-13 — CI now tests and lints `summrise-plugin-host` and `summrise-console-page` (`9104933`).

- 2026-11-13 — The local smoke shipped and merged (`a988d37`): `scripts/smoke-local.sh` starts the real
  device and console on free ports with temp data and asserts 22 things across the product loop —
  the install token format, `/api/health`, `/panel` and its meta token, `/api/panel`, an MCP
  `initialize`/`tools/list`/`tools/call` (`host.facts`) over the streamable-http flow, the audit
  lines for that call, `device.started` in `journal/000001.seg`, and the console's `/health` and
  embedded page. It always builds incrementally first, so it tests HEAD rather than a stale binary.
  Finding: `/api/panel` is loopback-only, not panel-token-gated (the mutating panel routes are); with
  one operator on their own machine that is the right trade, but it is a contract worth knowing.

- 2026-11-13 — Docs refreshed (`a6a3824`): the architecture's §6 table now names the bus, the two
  extracted crates, the journal's segments/reset, the subscription and the shell fallback; the status
  callout and §7 say what shipped as slices and point at this ledger; I7 records the page's crate
  boundary and I8 the gate's numbers. RELEASE.md names the three library crates and the four exes.

- 2026-11-13 — The release rehearsal shipped and merged (`aa33706`): `scripts/rehearse-release.sh`
  derives the three public keys from the archive seeds, publishes a canary on loopback (four exes via
  `cargo xwin`, a signed document, the NSIS installer), serves it and runs `verify-release.sh`; the
  positive run is 25 s and the corrupt-artifact copy fails on the digest. Findings worth keeping:
  `verify-release.sh` checks consistency, not signatures (a rewritten document passes; the sidecar is
  not checked at all), the signed `artifactUrl` is absolute so the mirror host must be signed, and the
  NSIS installer is not byte-reproducible. The device's own verifier is `summrise-device --check-update`,
  which `publish-canary.bash` runs at the end of a real publish.

- 2026-11-13 — `verify-release.sh` now proves what the device accepts (`b9b4bbb`): `--device`,
  `--device-data` and `--device-install` run the built device's `--check-update`, so the signed document
  must pass the same signature, revocation, policy and artifact-signature checks the machine runs; a
  sidecar the mirror serves is checked against the signed pointer. The rehearsal builds a Linux verifier
  from the derived keys, boots it once for its state (`--release-version/--release-channel`) and passes
  it through. Positive: five ok lines including `the device accepts the signed document (version
  0.3.995)` in 26 s. Negative: edit the document's version and the device check fails with `the release
  signature does not verify`, while the mirror checks stay ok.

- 2026-11-13 — v1 verification sweep shipped (`docs/design/v1-verification.md`): at `e61ff51`, 1 487
  tests across every shipped crate, clippy and fmt clean, the gate at 14 packages / 22 edges / 0
  upward, `build-web --check` green, 1 182 page tests and `tsc` clean, the local smoke 22 PASS, and the
  release rehearsal five ok including device acceptance. The sweep caught one real clippy failure
  (`map_or` in `audit.rs`) and one load-only page-suite flake. What remains is the Windows handoff and
  nothing else on the v1 checklist.

- 2026-11-13 — The product smoke is a CI step: the device job runs `scripts/smoke-local.sh` after the
  device smoke, so the panel, the MCP flow, the audit and the journal are proven on every push, not
  only by hand.

- 2026-11-13 — The operator loop is a smoke and a CI step: `scripts/smoke-enroll.sh` starts a real
  console (`--setup`, `/auth`, `POST /enroll`), a real device consumes the one-time code from
  `console-bootstrap.json` and goes online in `/fleet`, the console mints an `srm_` token, an MCP
  `initialize`/`tools/list`/`tools/call` goes through `POST /mcp/<device>` and returns `host.facts`,
  and both the console audit and the device journal carry the call; neither file holds the secret.
  1.6 s warm; the device job runs it after the product smoke.

## Next (in order)

1. Windows handoff — build the installer on a Windows runner, silent install/uninstall, run
   `summrise-shell.exe --check-window` with the console down, sign the four exes. This is the only v1
   item this Linux machine cannot verify.
2. Keep improving from what the operator hits (one major item a round, green merges into `hub/v1`).

## Rules

- One stage per branch; merge green work into `hub/v1`; never wait for approval between batches.
- Targeted tests per change; full checks at stage boundaries.
- Sub-agents get self-contained prompts and disjoint files; the coordinator reviews, commits, merges.
- A task that returns red is fixed by the same sub-agent or by the coordinator if the fix is small.
