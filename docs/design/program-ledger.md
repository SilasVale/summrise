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

- 2026-11-13 — The Windows handoff is ready to execute: `docs/design/windows-handoff.md` (trigger, the
  five pieces of evidence, the interactive `--check-window` with the console down and up, signature
  and uninstall checks), and `installer.yml` now signs the four exes before `makensis` when a
  certificate is configured — it used to sign only the setup, so a signed install shipped four
  unsigned executables. The adapter is CI-only (13 steps, YAML valid).

- 2026-11-13 — `scripts/check.sh` is one command for the device job's portable half: it reads the
  single-line `run:` steps from `ci.yml` too, so the gate, the device smoke, the product smoke and the
  enrol smoke run with the cargo commands (and `--fast` skips tests and smokes). Running it end to end
  found `cargo fmt --check` red on the previous round's clippy fix — fixed; the gate now exits 0 with
  both smokes green.

- 2026-11-13 — `docs/design/v1-release-notes.md` is the operator's summary: what ships, the five-step
  journey, how it is verified, the sweep's numbers, and the known boundaries (Windows rehearsal,
  loopback `/api/panel`, installer reproducibility, the two audit windows).

- 2026-11-13 — v1 landed on `main`: a fast-forward from `hub/v1` (1328 commits, `main` and `hub/v1` now
  equal) after `scripts/check.sh --fast` passed on `main`. This is the local merge the operator asked
  for at the start of the program; nothing was pushed to `origin`, so the deploy act (push / PR) is
  theirs to make.

- 2026-11-13 — The release gate on `main` (`67e08cf`): full `scripts/check.sh` exit 0 with both smokes,
  1 182 page tests and `tsc`, the page-staleness gate, and the release rehearsal (five ok, installer
  built, 133 s). Recorded in `v1-verification.md`.

- 2026-11-13 — **The Windows runner answered the handoff's first half.** `installer.yml` dispatched on
  `hub/v1` at `e52fd6e` ([run 43344130298](https://github.com/SilasVale/summrise/actions/runs/43344130298))
  is green: the four exes build on `windows-latest`, the NSIS setup compiles, a silent install
  (`/S /D=C:\SummriseCI`) leaves `SummriseLaunch` Running and `/api/health` `ok`, a silent uninstall
  removes the service, and `SummriseSetup-0.3.993` (6,827,406 B) is attached to the run. What is left
  is the interactive half (`--check-window`, the visual read) and the signature check —
  no run metadata says whether a certificate was configured. Recorded in `v1-verification.md`.

- 2026-11-13 — **`origin/main` and this tree now share a history.** The remote main carried two commits
  from 2026-11-02 (PR #107: focus lands on `<main>` after a surface change; a `runs.get` in flight
  keeps the row focusable) that the v1 program's line never had — 1,240 commits meet 2. They could not
  merge as commits: their markup belongs to a console that predates Device tabs. Their behaviour is
  ported (`50ba9e2`), with the tests: `<main>` takes focus after a page or machine change; the record
  card is focusable and takes the focus when it arrives; the reading line is a `role="status"`; the
  row's control is `aria-disabled` with a guarded click, so the operator's focus is not dropped
  mid-read. `origin/main` is now an ancestor of `main` (`0 1242`), so a push of `main` is a
  fast-forward, not a rewrite.

- 2026-11-13 — **The phone's fleet card keeps its second line level (`e1b7ad5`).** The browser gate
  caught what the desk never showed: the `2 failed` chip is a 44 px thumb target, so on the card it
  grew the status cell to 104 px (79 without it) and pushed the version 12 px off the line the card's
  grammar states. Both cells are now top-aligned and the chip takes the card's next line at full width;
  the gate photographs the card it measures (`07c-fleet-390.png`). Gate PASS — 107 steps.

- 2026-11-13 — **Calls are journal events, joined across the two records on `trace.call`.** §4.2 said a
  call is exactly two events sharing one trace, and that a proxied call appears in both journals joined
  by it; neither half existed. Now: `summrise-bus` gains `publish_traced` (the envelope's `trace.call`,
  additive, absent on every previous publish); the device publishes `call.started` then
  `call.finished|denied` for every call from every door — console channel, mesh, panel, loopback — with
  `subject` naming the token or the door, and adopts the console's trace when its frame carries one
  (`RemoteFrame.trace`, also on the grant path). The five panel operations are journal calls too. The
  console mints `console-<8 hex>-<n>` per proxied exchange (random prefix, so a restart cannot reuse
  one), sends it on the frame, writes it on the audit line (`Entry.trace`, absent on operator actions),
  and **does not relay** the device's half of a call it proxied itself — the audit row is already the
  record — while every other `call.*` still reaches the page, which is the completeness gap closed.
  The page draws a call's outcome as one `mcp` row (`424` when refused, a sentence naming the tool and
  reason), drops the `started` half, and carries `trace` on both kinds of row so the join is in hand.
  Evidence: bus 13, device 649, console 174 + 254 + 15, page 1 194 and `tsc` clean,
  `build-web --check` green; `smoke-local` asserts the pair in the real journal and `smoke-enroll`
  asserts the join across the two real processes (`trace console-c794ca57dd780286-1: audit line joins
  journal seq 2 -> 3`).

- 2026-11-13 — **The join is drawn: Activity says which calls the machine can still prove.** The key
  existed on both records since the last item and nothing read it. Now the device offers `journal.get`
  (a new `journal` capability, one read-only tool, registered after `state`): for a bounded list of
  traces it scans the retained journal newest first (`Bus::scan_newest`) and answers which of them are
  still there, which are not, and its own `oldest_seq`/`last_seq`, with `bounded: true` when the scan
  stopped at its 10 000-row cap. The console serves `GET /audit/join?device=&traces=` (at most 128),
  which asks that tool down the device's own channel under the operator's scope, mints a trace so the
  console's relay drops the `call.*` pair the machine writes for the read, and **writes no audit line
  and no live event of its own** — a read that audited itself would grow the window it describes. An
  older machine's refusal or an unreadable answer is `unknown`, never `missing`. The page asks once per
  machine window (debounced, re-asked after 30 s because a journal rotates) and marks only the
  deviation: a row the journal no longer holds wears `only here` with the reason in its opened record,
  a row both records hold wears nothing and has the agreement stated there, and a fleet window (no one
  journal answers for it), an older machine, a failed read or a bounded scan is `unknown` and is never
  drawn as lost. Evidence below.

- 2026-11-13 — **The machine's record becomes readable: Activity draws the calls only the journal has.**
  The join could say whether the machine still held a call the console drew; nothing could ask the
  machine what it recorded. So a call that never reached the console — the panel's, a mesh peer's, a
  direct loopback client's — was drawn from the live relay and lost on reload, and a call the console's
  own window rotated away could be marked `only here` but not read back. Now the device offers
  `journal.read` in the existing `journal` capability: newest first, at most 200 rows, each a
  **bounded summary** built from fields the machine already has (`seq`, `ts`, `type`, `trace.call`, and
  on a call `tool`, `ok`, `reason`, the `door` it arrived by), plus `payload_bytes` and `more`; never a
  payload, because a read that relayed one would be an export wearing a read's name. The console serves
  `GET /journal/read?device=&before=&limit=`, asks that tool down the device's own channel under the
  operator's scope, mints a trace so its relay drops the read's `call.*` pair, and **writes no audit
  line and no live event of its own**. The page reads one window per machine (cached 30 s) and draws
  the calls that arrived by a door the console does not own — `panel`, `mesh`, `loopback` — so the
  console's own reads of the machine (`tools/list`, the join, this read) are never drawn as the
  operator's work, and a console row is that record's to keep; a machine row is dropped when the
  console already holds the same call, matched by `trace.call`. It shows the journal's own `door` where
  a console row shows `console`, and says in the footer that the journal was not read when the answer
  is `unknown` or the read failed. **`unknown` is never drawn as an empty journal**, and a machine with no journal
  answers `journal: false` with no rows — a fact, not an error. Evidence: bus 20, device 659, console
  174 + 279 + 15, page 1 290 and `tsc` clean; `smoke-enroll` reads the machine's own window through the
  console and finds the call's row with its `seq`, tool, outcome, door and trace.

- 2026-11-13 — **The audit line learns the machine's `seq`: a proxied call can be read back, not only
  proved present.** `journal.read` made the machine's record readable and the join proved a call was
  still there, but the console's line named no position in the machine's journal. Now the device puts
  the outcome half's `seq` on the channel frame (`journal_seq`, beside the JSON-RPC body, never inside
  it: an MCP client sees its result unchanged), only when this process keeps a journal and the
  exchange reached its outcome. The console carries it on the channel answer, writes it on the audit
  entry and the live event (`machine_seq`), and serves it with the rest of the line. The page reads
  that row back when a record is opened — `GET /journal/read?before=<seq + 1>&limit=1`, one small
  request per opened row, cached by `(device, seq)` — and draws the machine's own facts beside the
  console's; a rotated row is stated as gone, an `unknown` answer as not offered, a failed read as
  why, and a console row with no position keeps the console's facts alone. The device's refusals are
  also silent on values again (the round-9 review's finding): a bound breach names the bound, an
  unknown key the two accepted keys. Evidence: device 661, console 175 + 280 + 15, page 1 313 and
  `tsc` clean, `smoke-enroll` reads the audit line's `machine_seq` back as the journal's own outcome
  row across two processes.

- 2026-11-13 — **The machine's window pages: the page says how deep its record was read, and reads
  deeper.** `journal.read` has taken `before_seq` and answered `more` since round 8, but Activity used
  neither: on a busy machine the newest 200 rows are minutes, so a device-only call older than that
  page existed in the journal and nowhere the page could reach. The machine window is now a fetch
  session: the newest page when the scope opens, `before = oldest fetched` for each older page, and
  the footer offers "Read older rows from the machine" while the machine answers `more`. A 30 s
  refresh re-reads only the newest page and merges it in front of the pages already asked for by
  `seq`; pages are deduplicated by sequence, and a failed older page keeps the drawn rows and becomes
  a retry with the console's reason. The drawn list — not the console's 1 000-line cut — is the
  window paging deepens, because those older machine rows were already drawn as older days. Once the
  operator has paged, a last page's `more: false` is stated as "The machine's journal has no rows
  older than this." Evidence: page 1 335 and `tsc` clean; the App test pages from the footer and
  draws the older call (second read `before = 200, limit = 200`); no Rust change was needed.

- 2026-11-13 — **The counts name the calls the console's audit cannot see.** A client may call the
  machine's own endpoint (the journal's `loopback` door); the Timeline draws that call, but the
  Activity header's `Today` is `/audit/day`'s read of the console's audit — the same rule the hub
  serves — so the direct call was a client call counted nowhere, and the header and the rows could
  not be reconciled. The header now says it: "N calls drawn today came straight to this machine, not
  through this console (M failed)." The count is taken from the same contribution the timeline draws
  (`machineContribution`), within the day block the labels index — the console's `day_start_ms` plus
  24 hours, or the page's own midnights when the answer names none — so the two cannot disagree; `panel`,
  `mesh` and `console` rows are not client calls; a failed direct call is named, not folded into the
  day's failure count. The day sentence keeps its basis rather than adding a partial window's rows: a
  true machine day needs a time-bounded journal read. Evidence: page 1 345 and `tsc` clean; the
  machineRows tests pin the contribution and the door rule, and an App test draws a loopback call
  beside a proxied one and asserts the day number is unchanged while the new line names it.

## Next (in order)

1. Windows handoff, the interactive half — dispatch or run `summrise-shell.exe --check-window` in an
   interactive session (session 0 refuses it), console down and up, look at the WebView2 panel, and
   verify Authenticode when a certificate is configured. The runner half is green (run 43344130298).
2. Push `origin`: `main` now contains `origin/main` and `hub/v1` (`0 1242`), so `git push origin main`
   is a fast-forward. This is a publish, and it is the operator's call.
3. The machine's day can be counted: the header now names the direct client calls among the rows it
   drew, but a true day count needs a time-bounded read of the machine's journal (its cursor is
   `before_seq` today), so `journal.read` should learn a day window — or a `journal.day` summary with
   the same bounded-scan rules as `journal.read` and a stated `more`/rotation answer. Then the day
   sentence can include the machine's direct calls for the same local day, with its basis named in
   one spelling, and the failed count can carry them without mixing two windows. (A payload/export
   verb stays a separate stage: the journal records what happened, not the arguments or results.)
4. Keep improving from what the operator hits (one major item a round, green merges into `hub/v1`).

## Rules

- One stage per branch; merge green work into `hub/v1`; never wait for approval between batches.
- Targeted tests per change; full checks at stage boundaries.
- Sub-agents get self-contained prompts and disjoint files; the coordinator reviews, commits, merges.
- A task that returns red is fixed by the same sub-agent or by the coordinator if the fix is small.
