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

- 2026-11-13 — **The machine's day can be counted: `journal.day`, and a day sentence that includes
  it.** Round 11 could name the direct calls among the rows a page drew, but a page is not a day.
  `journal.day` now answers a day as one aggregate: two instants the caller owns, the five doors'
  call and failure counts for the outcome rows inside the range, the retained bounds, and `more`
  when the scan was cut at 20 000 rows. The door is on a call's started half; an outcome whose half
  is just below the range is resolved by a bounded lookback (2 000 rows), and an outcome whose half
  is not found counts as `unknown`, never as a client's. The console relays it at
  `GET /journal/day?device=&from=&to=` (no audit line, no live event) and the page asks for the block
  its labels index (the console's cut plus 24 hours, or its own midnights). The day sentence now adds
  the machine's `loopback` calls and failures to its client number and names the share — "2 straight
  to this machine (1 failed)" — with "at least" when the machine's scan was cut; a failed or
  `unknown` day read keeps the console's number and says the direct calls were not counted. The
  hub's fleet number is unchanged: a fleet asks no machine's journal. Evidence: bus 24, core 104,
  device 665, console 175 + 283 + 15, page 1 347 and `tsc` clean; `smoke-enroll` unchanged (no new
  console route smoke yet — the route's tests answer a fake device field by field).

- 2026-11-13 — **The fleet's record is asked for, bounded: "All machines" draws and counts the
  machines' own journals.** A fleet window spans machines, so it drew only the console's rows and
  counted only the console's audit. Activity now fans out to the online machines the fleet names — at
  most 8, in the fleet's order, one newest `journal.read` page and one `journal.day` of the block the
  header counts each, cached 30 s, only while Activity shows the fleet window (Home and the Device
  picker ask nothing) — merges their `panel`/`mesh`/`loopback` calls into the console's rows (the
  console's row wins a call by `trace.call`; the untraced fallback names the machine, and the row key
  is the machine plus the journal's own position, because two machines — or two calls on one — can
  write the same instant, tool and outcome), and adds their `loopback` counts to the day sentence as
  "N straight to the machines". A refresh keeps the page and day it had (a failed refresh does not
  turn an answered machine into an unread one), a still-running read makes the number "at least", and
  what was not read is named, never zeroed: the footer lists the machines whose journal was not read
  and why and the machines the cap left out, and the header's note names the machines whose **day** is
  missing — a machine whose rows were read but whose day failed is only in the note, and vice versa.
  A journal-less machine is complete, not missing, from either answer. Four stated limits: 16 device
  calls per 30 s at the cap while Activity is open (~3 MB a day per asked machine in its journal,
  console-door rows the page never draws), a fleet day that is a floor above 8 machines, a window the
  page left forgotten after ten cache windows, and join marks that stay a machine window's fact.
  **"Clients only" now counts a `loopback` door as a client call** (a machine row's only proof;
  `panel`/`mesh` are not the AI's clients), page-only: the shared audit table has no door. Evidence:
  page 1 396 and `tsc` clean, `build-web.sh --check` green, the browser gate PASS (107 steps — its
  stub now answers both journal routes, because the fan-out asks them for every machine it draws),
  `scripts/check.sh` every command passed; App tests fan out to two machines, count both days, name
  the one that failed, and keep two machines' identical rows as two rows; a review found the refresh
  erase and the note/footer split, both fixed with tests before merge.

## The v2 program (2026-11-13)

The operator asked for the best maintainable architecture, not the current one's incremental polish.
`next` carries the proposal (`architecture-v2-2026-11-13.md`) and its gate (`v1-parity.md`). The storage
model changes on review: **the journal is the live stream, not the record** — it is capped at 64 MiB / 7
days, so a record that can be rotated away is not a record. The record is SQLite per process: WAL,
migrations, indexes, policy retention, backups, integrity check. One writer and one home per fact; the
console's database holds operator facts, the device's holds machine facts, and a proxied call appears in
both as two perspectives joined by its trace. The first draft's "one journal, one record" sentence is
withdrawn here because the operator was right to reject it.

The program runs as slices on `next`, each one round, each with tests and a parity row; `hub/v1` stays
shippable. Order (2026-11-13, after the operator said the record is not urgent): the **capability seam
first** — one module per capability, with its spec and guide, because it does not touch storage and it
is what makes "add a tool = one file" true. The SQLite record (S1) follows the seam; the release
milestone (S0's second half) waits until the operator names a machine for it.

**Round 15 — the capability seam, proved on `files`.** The declaration types are in
(`ToolDef::schema`, `Capability::guide`) and so is the generated guide (`core::registry::guide()`).
`files` is the first capability whose tools, tool schemas and manual entry live in one module
(`core/src/capabilities/files.rs`), whose execution lives in one module
(`device/src/capabilities/files.rs`), and which the dispatcher routes **before** the legacy table.
The four files tools left `HANDLERS`; the registry-alignment test now counts `HANDLERS` plus migrated
capability tools, and new tests fail if a migrated tool keeps a legacy schema or dispatch row, if the
guide names a tool twice or one the capability does not own, or if the advertised schema differs from
the declaration. Evidence: 669 device tests (the full bin suite), `clippy` and `fmt` clean,
`check-architecture` 14 packages / 22 edges / 0 upward. The other twelve capabilities migrate one per
round; the legacy `input_schema` match and `HANDLERS` shrink as they do.

**Round 16 — `memory` follows the same shape.** Six tools moved to
`core/src/capabilities/memory.rs`; its two caps (100 results, default 10) are constants of the
declaration now, and the device module imports them rather than keeping a second copy. The router
dispatches two capabilities through one `Ctx` (files roots + the memory store), which is the narrow
context the contract asks for. The seam tests no longer name `files`: they iterate
`core::capabilities::DECLARED`, so every new capability is held to the same four rules — schema
declared beside the tool, legacy dispatch row gone, one guide heading, router covers the declaration.
Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 packages / 22 edges / 0
upward. Next: `terminal` (seven tools, the largest), then the rest.

**Round 17 — `terminal`, the largest capability, moves.** Seven tools (create, write, read, resize,
close, list, screen) declared in `core/src/capabilities/terminal.rs`; its caps (1000 rows/cols, 1000
screen lines, 60 default lines, 24×80 default size, 115200 8N1) are the declaration's constants and
the implementation re-exports them, so the schema, the guide and the validation cannot disagree. The
device implementation is async (create/write/read/resize/close await) and takes the `PtyRegistry`
through `Ctx`, which now carries files roots, memory and terminal — exactly what a capability reads,
nothing else. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0.
Next: `browser` (four tools, lazy registry), then `runs`, `system`, `forward`, `monitor`, `host`,
`journal`, `plugins`, `guide` — one per round until `HANDLERS` and `input_schema` are only the
fallback for what has not moved.

**Round 18 — `browser` moves.** Eight tools (list, open, navigate, eval, screenshot, cast, close,
shutdown) declared in `core/src/capabilities/browser.rs`; the async implementation calls the lazy
`BrowserRegistry` through `Ctx`, so the browser starts only when a browser tool is called. The guide
now has four capabilities (files, memory, terminal, browser) and 25 tool headings, all generated.
Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0. Next: `monitor`
(five tools), then `runs` (three), `system` (three), `forward` (three), `host` (two), `journal`
(three), `plugins` (five) and `guide`.

**Round 19 — `monitor` moves.** Five tools (add, remove, list, probe, status) declared in
`core/src/capabilities/monitor.rs`; the 5 s minimum interval is the declaration's constant, re-exported
by the implementation so schema and validation agree. `Ctx` carries the `Arc<MonitorRegistry>` (`add`
installs per-target timers, so it needs the shared handle). Guide: five capabilities, 30 tool headings.
Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0. Next: `runs`
(three), then `system`, `forward`, `host`, `journal`, `plugins`, `guide`.

**Round 20 — `runs` moves.** Three tools (exec, list, get) declared in `core/src/capabilities/runs.rs`;
the timeout and list caps (60 s / 10 min, 20 / 100) are the declaration's constants, re-exported by
the implementation. `runs.exec` keeps its blocking-pool discipline and its "a non-zero exit is a
tool-level failure" rule inside the capability module, which also took the failure sentence. Guide:
six capabilities, 33 tool headings. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean,
gate 14 / 22 / 0. Next: `system` (three), then `forward`, `host`, `journal`, `plugins`, `guide`.

**Round 21 — `system` moves.** Three tools (process_list, process_kill, net_test) declared in
`core/src/capabilities/system.rs`; the network-test caps (60 s ceiling, 5 s default) are the
declaration's constants and the implementation re-exports them. `net_test` keeps its blocking-pool
discipline inside the module: the resolver parks there, not on a reactor worker. Guide: seven
capabilities, 36 tool headings. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate
14 / 22 / 0. Next: `forward` (three), then `host`, `journal`, `plugins`, `guide`.

**Round 22 — `forward` moves.** Three tools (add, remove, list) declared in
`core/src/capabilities/forward.rs`; the rule view (`forward_row`) moved with them and is shared with
the test that asserts its keys, so the answer and the assertion still share one source. The `Ctx`
carries the `Arc<ForwardRegistry>` (a rule installs its own listener). Guide: eight capabilities, 39
tool headings. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0.
Next: `host` (two), then `journal`, `plugins`, `guide`.

**Round 23 — `host` moves.** Two tools (facts, restart) declared in `core/src/capabilities/host.rs`.
The facts still come from `crate::host::facts`, but the tool surface and the restart rule live in
`capabilities/host.rs`: the `Ctx` now carries the data directory, the launcher flag and the restart
hook, and the old `restart_app` method is gone (a second copy of the rule). Guide: nine capabilities,
41 tool headings. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0.
Next: `journal` (three), then `plugins` (five), `guide`.

**Round 24 — `journal` moves.** Three tools (read, get, day) declared in
`core/src/capabilities/journal.rs`; the row/document shapes, the day counting, the trace scan and the
five caps (10 000 scan, 200 read, 20 000 day, 26 h span, 512 traces) moved into
`capabilities/journal.rs`, and the `Ctx` now carries `Option<&Bus>` — a device with no journal still
answers `journal: false` rather than failing. The declaration keeps the registry's wire order
(get, read, day). Guide: ten capabilities, 44 tool headings. Evidence: 669 device tests, 104 core
tests, `clippy`/`fmt` clean, gate 14 / 22 / 0. Next: `plugins` (five), then `guide`; after that the
legacy `HANDLERS` and the schema match are only the fallback for `plugins`/`state`/`screen`.

**Round 25 — `plugins` moves.** Six tools (list, inspect, add, remove, restart, logs) declared in
`core/src/capabilities/plugins.rs`; the implementation delegates to the plugin host's own
`dispatch` and maps its two error kinds, so the host crate keeps its protocol and the surface keeps
one error vocabulary. The `Ctx` carries the `Arc<PluginHost>`. Guide: eleven capabilities, 50 tool
headings. Evidence: 669 device tests, 104 core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0.
Next: `screen` and `state` (one tool each) finish the seam; after that `HANDLERS` and the schema
match can be deleted rather than kept as a fallback.

**Round 26 — the seam is complete.** `screen` and `state` moved (one tool each; the helper refusal and
the state document/health shapes keep their sentences), and the legacy surface is **deleted**: no
`Handler` enum, no `HANDLERS` table, no `handler()` lookup, no schema `match`, no `dispatch_supports`.
`dispatch` now routes to a capability module or to a plugin, and nothing else. `input_schema` is a
one-line resolver that reads the declaration (a missing schema is a bug, not an empty fallback), and
the registry-alignment test compares the registry with the modules directly. Thirteen capabilities,
52 tools, one declaration module and one implementation module each. Evidence: 669 device tests, 104
core tests, `clippy`/`fmt` clean, gate 14 / 22 / 0. Next: the protocol types (S5) — the envelope,
frames and feed rows as `protocol` + generated TS — then the record.

**Round 27 — the wire is one crate.** `protocol` (`summrise-protocol`) now owns `Frame`, `Answer`,
`Body`, `Subscribe`, `EventsSubscribe`, `Unsubscribe` and `TokenScope` (the grant envelope is
re-exported from core), with constructors for the four frame shapes and golden fixtures under
`protocol/tests/fixtures/`. The device parses `Frame` and builds `Answer` (its private copies are
gone); the console builds the call/follow/events/unsubscribe frames with the same type, parses
`Answer`, and its `TokenScope` is the protocol's. The gate knows the crate: 15 packages, 25 edges,
0 upward. Evidence: 669 device tests, 5 protocol tests, 175 console lib tests, 283 console integration
tests, `clippy`/`fmt` clean. Next: generated TypeScript from these types for the page (the last half
of S5), then the record (SQLite).

**Round 28 — the wire exports TypeScript.** `ts-rs` derives `TS` on every protocol type (optional
members become `?:`, absent stays absent; `u64` ids are `number`; the grant body is `unknown` until
core exports its own shape). The bindings are generated into `console/web/src/protocol/`, and
`console/web/src/protocol.ts` re-exports them — the page's type surface now comes from
`protocol/src/lib.rs`. `scripts/check-protocol-ts.mjs` regenerates (and `--check` fails on a stale
committed binding); the page's `tsc --noEmit` is clean. Evidence: 7 protocol tests, 12 protocol
tests total with the golden set, gate 15 / 25 / 0. Next: the record (SQLite) — `record.db` with the
feed table as the bus, migration from the segmented journal, retention and backup — starting with
the store crate and its crash/replay tests.

**Round 29 — the record is SQLite, and the bus is its feed.** `summrise-bus` no longer writes
segments: `record.db` (WAL, `synchronous=FULL`) holds `feed(seq, ts_ms, json)`; a publish is one
transaction that inserts the envelope, prunes under the byte budget, and only then fans out to
subscribers. Reads are queries: `scan_newest`, `read_window` (one row past the page for `more`),
`read_between` (by the row's own moment), `oldest_seq`/`last_seq`, and `reset` from the oldest
retained row. `backup_to` is `VACUUM INTO`; `check` is `PRAGMA integrity_check`; `publish_traced_at`
exists so an import can keep a row's original moment. The segment/snapshot tests are replaced by 15
record tests (reopen, replay-then-live, retention, reset, windows, ranges, backup, check). Evidence:
15 bus tests, 669 device tests, 175 console lib tests, 283 console integration tests, gate 15 / 25 /
0. Gap named for next round: an existing `journal/` with `.seg` files is not imported yet — the
device starts an empty record beside it. Next: that import (once, marked in `meta`), then the
console's operator record.

**Round 30 — the old journal is imported once.** `Bus::open` reads `*.seg` beside `record.db` when
the record is empty, keeps each row's `seq` and moment, and writes `meta.journal_import` in the same
transaction — a crash mid-import retries, a second open does nothing. A record that already has rows
wins: the segments stay on disk untouched and the marker says `skipped:record-not-empty`. A torn
last line is skipped, not fatal. Tests: import keeps sequence and moment, a second open does not
duplicate, a torn segment imports its whole lines, an existing record is not merged, and the
`read_between` range still finds the imported moment. Evidence: 19 bus tests, 669 device tests,
gate 15 / 25 / 0. Next: the console's operator record — `events.jsonl` (audit + activity) into its
own `record.db` with the same transaction/retention rules, then the page reads the record.

**Round 31 — the record is its own crate.** `summrise-record` owns the interface every process will
use: `Record::open(dir, max_bytes)`, `append(ts_ms, json) -> seq`, `insert(row)` (import, keeps the
sequence), `newest` / `window(before, limit)` / `between(from_ms, to_ms, limit)` returning
`Row { seq, ts_ms, json }`, `read_from`, `meta` / `set_meta`, `backup_to`, `check`. The bus no
longer holds SQL: it serializes an envelope, takes its sequence under one publish lock, inserts the
row, then fans out — `summrise-bus` is now a wrapper over `summrise-record` (and keeps the legacy
import, which is a bus concern). Eight record tests cover sequence/reopen, windows, moments,
retention, idempotent insert, meta, backup and integrity. Evidence: 8 record tests, 19 bus tests, 669
device tests, clippy/fmt clean, gate 16 packages / 26 edges / 0 upward. Next: the console's operator
record — open its own `record.db` through `summrise-record`, import `events.jsonl` once, and let
`/audit` reads and the page use the record.

**Round 32 — the device page names the record.** `state.get` now carries `record`: the file path,
its size, `schema_version`, and the sequence bounds (`last_seq` / `oldest_seq`), `null` on a process
with no record. `Record` gained `open_file` and `span` — the two things a console-side store will
need. The console cut-over to `summrise-record` was built and **backed out this round**: the
console's `/audit/source` answer and a dozen integration tests name `audit.jsonl`, its one rotated
generation and `rotations: 1`. Moving to a record changes that answer (`audit.db`, no generation),
so it is a page-contract change, not a storage swap — it needs the page's `AuditSource` type and
the tests updated in the same slice. Nothing half-migrated shipped. Evidence: 670 device tests,
clippy/fmt clean, gate 16 / 26 / 0. Next: the console record with that contract change in one
slice — import `events.jsonl`/`denials.jsonl` once, `source` reports `audit.db` with `rotations: 0`
and no `rotated`, retention replaces rotation, and the page's type and tests move with it.

**Round 33 — the console's operator record is its own database.** `JsonLines` is now two
`summrise-record` files (`audit.db`, `denials.db`): one row per line, `ts_ms` as the moment, and
retention that prunes the oldest rows instead of rotating a file. The old `audit.jsonl` and its
`.1` generation are imported once on first open (marker in `meta`), keeping each line's moment; a
record with rows is authoritative and the files stay on disk. The `/audit/source` answer moved with
the store — `file` is `audit.db`, `rotations` is 0, `rotated` is omitted — and the page's `AuditSource`
doc, a dozen integration tests and the cap test moved with it. Two record fixes came out of making
it real: the sequence is read inside the insert transaction (a second connection cannot mint a used
sequence) and retention counters live in `meta`, so a publish is one insert + counter updates, not a
full-table scan (the 50,001-row range test seeds in one `append_many` transaction instead of
50,001 fsyncs). Evidence: 176 console lib tests, 283 console integration tests, 8 record tests, 670
device tests, 1,396 page tests, page `tsc` clean, gate 16 packages / 27 edges / 0 upward. Next: the
entry/dispatch interface (S4) — one `Dispatch` seam the MCP door, the console channel and the panel
share — then the release milestone, which needs the operator to name the machine it deploys to.

**Round 34 — the release rehearsal is green on v2, and the record can be verified.** Two changes and
one proof. (1) The device's record now lives at `<data>/record.db` — one process, one record, at the
data root rather than a `journal/` subdirectory; the legacy importer still reads `<data>/journal/`,
so an upgraded machine keeps its rows. (2) `summrise-device --check-record <data>` verifies a running
build's record: `PRAGMA integrity_check`, the file path and size, schema version, sequence bounds and
the moments of the oldest and newest rows, printed as one JSON line; a missing or corrupt record is
exit 2, never an empty success. (3) `scripts/rehearse-release.sh --version 2.0.0` passed end to end
(29 s, cached builds): the signer derived the three public keys, published a signed document, the
canary pointer named 2.0.0, the artifact matched its signed digest, the sidecar agreed, NSIS built
`SummriseSetup-2.0.0.exe`, and the real device binary accepted the signed document. Then the
release-built device booted on a temp data directory, wrote `record.db`, and `--check-record` answered
`ok: true`, `schema_version: 1`, one row, path and size present. Evidence: 671 device tests, 19 bus
tests, clippy/fmt clean, gate 16 / 27 / 0. Remaining half of the milestone: deploy that published
build to the machine the operator names, and prove rollback there — both need the name.

**Round 35 — v2.0.0 is running on the machine the operator named, and rollback is proven there.**
Operator named DESKTOP-9VH92BJ (the console host). The 2.0.0 installer was rebuilt with the host's
port (18090) so the update cannot move the service off its port, staged on the canary mirror
(`SummriseSetup-2.0.0.exe`, sha256 `b8137828…70b2`, not made the channel pointer) and installed
silently; the supervised setup relayed through Task Scheduler as designed (exit 3), and
`/api/health` answered `2.0.0` (pid 27404). Verification on the machine: `summrise-device
--check-record C:\ProgramData\Summrise` answered `ok: true`, `schema_version: 1`,
`path: C:\ProgramData\Summrise\record.db`; the device's own `state.get` names the record and
reports 49 rows. **Rollback:** reinstalling `SummriseSetup-0.3.292.exe` put the service back on
0.3.292 (health confirmed), then 2.0.0 was reinstalled and the record continued (rows 1→49 across the
cycle). The **console** stream was deployed too: `summrise-console-2.0.0.exe` (sha256 `e9225e8b…04e6`)
swapped by the staged `deploy-console-2.0.0.cmd`, `/health` answered `2.0.0` with page 200; the
previous binary was restored (`0.3.274-ux255`, health confirmed) and 2.0.0 redeployed, so the
console's rollback is proven by the same backup/restore path. What is **not** done: 2.0.0 is not yet
the fleet's default release (the canary pointer still names 0.3.292) — that is a channel decision
for the operator, not a deploy. Next: v1 parity checks and the channel decision; then the pointer.

**Round 36 — v2.0.0 is the canary default, and v1 is green at parity.** The publish that was only a
rehearsal last round is now live: `publish-canary.bash --version 2.0.0 --channel canary --port 18090
--mirror http://100.85.163.159:18100` signed and wrote the document, artifact, sidecar and installer
into the canary mirror, and the pointer `release-canary.json` now names 2.0.0.
`verify-release.sh 2.0.0 --mirror … --console http://100.74.250.98:19000 --console-version 2.0.0`
answered ok: pointer 2.0.0, artifact digest matches, sidecar agrees, installer served (sha256
`17a82ae7…eff9`, the host-port build), console runs 2.0.0 with 4 devices registered. On the named
machine the device and console are both 2.0.0 (previous round's install and console swap), and its
record is `C:\ProgramData\Summrise\record.db` schema 1. **v1 parity:** a clean worktree of
`hub/v1` (`cd8dfed7 Merge the fleet's record`) ran `cargo test --manifest-path device/Cargo.toml
--workspace` to exit 0 across 45 test binaries — v1 remains releasable and is untouched by the v2
line. Rollback stays proven on the named machine for both streams (device 0.3.292 ↔ 2.0.0; console
0.3.274-ux255 ↔ 2.0.0). The fleet picks the pointer up on its own schedule: DESKTOP-CPH4KB2 is on
0.3.292 with a recent check and will update; DESKTOP-84T3LIN (0.3.291) reports no update check yet;
DESKTOP-14RJCR8 has been offline for nine days. Nothing further is required of this line: the
default release is v2.0.0, parity is green, rollback is proven.

**Round 37 — stable is 2.0.0, DESKTOP-84T3LIN is updated, and the sidecar is channel-qualified.**
The stable channel now publishes 2.0.0 too: `publish-canary.bash --channel stable --port 18090`
wrote `release-stable.json` and the mirror serves it; `verify-release.sh 2.0.0 --channel stable`
is ok, and the canary pointer still names 2.0.0. Publishing both channels from one version exposed a
real bug: the sidecar at `summrise-device-$version.sidecar.json` names one channel, so the second
publish made the first channel's verification fail (`sidecar disagrees: channel is 'stable'`). The
publish now writes `summrise-device-$version-$channel.sidecar.json` and `verify-release.sh` reads
that channel-qualified file (falling back to the generic name); both channels verified ok after the
re-publish. DESKTOP-84T3LIN (0.3.291, no update check) was updated by hand from the published
installer (sha `17a82ae7…eff9`): health is 2.0.0, `record.db` at `C:\ProgramData\Summrise` schema 1,
and its baseline sha `d9d0ea9d…` matches the published device artifact. DESKTOP-14RJCR8 stays
offline (nine days); its registration can be deleted, which revokes its tokens and refuses its next
connect until re-enrolment — an operator decision, not done here. `next` cannot be pushed: GitHub is
unreachable from this host (curl and `git ls-remote` time out, no proxy configured), so the push and
the `main` fast-forward remain blocked on network access, not on the tree.

**Round 38 — the push landed, `main` is fast-forwarded, and the reachable fleet is on 2.0.0.**
GitHub became reachable again (curl 200, TCP 443 open, no proxy needed). `next` was pushed
(`origin/next` = `62ec7837`), `origin/main` was fast-forwarded from `abf796e1` to `62ec7837` (the
tip was an ancestor: 0 behind, 1302 ahead), and the local `main` now tracks it. Both channels verify:
`verify-release.sh 2.0.0 --channel canary` and `--channel stable` are ok, each reading its
channel-qualified sidecar. DESKTOP-84T3LIN's record is verified by the deployed binary itself:
`--check-record C:\ProgramData\Summrise` answered `ok: true`, schema 1, 4 rows. DESKTOP-14RJCR8
remains offline (nine days) — a concrete blocker, not retried further; its removal is an operator
decision. The rollback installer `SummriseSetup-0.3.292.exe` stays on the mirror. Full workspace
tests ran green (50 test binaries, exit 0) and the gate says 16 packages / 27 edges / 0 upward.
One deployed machine needed a correction: DESKTOP-CPH4KB2 had auto-updated to a 2.0.0 binary whose
actual bytes were `3a80a057…`, not the published `d9d0ea9d…`, and it answered `unknown argument
'--check-record'` — the launcher had recorded the document's digest while the file on disk was a
different build. Reinstalling the published installer (sha `8fc8a802…`) put `d9d0ea9d…` in
`baseline/current`, and `--check-record C:\ProgramData\Summrise` then answered `ok: true`,
schema 1, 43 rows. All three reachable machines now run the published device binary and verify their
record with the deployed binary itself.

**Round 39 — final sweep.** `origin/next` and `origin/main` are both `21a4f5c3`; the tree is clean.
`verify-release.sh 2.0.0` is ok on canary and stable (each reads its channel-qualified sidecar; the
installer served is `8fc8a802…`). All three reachable machines answer `/api/health` with 2.0.0 and
run baseline sha `d9d0ea9d…`: DESKTOP-9VH92BJ (record 55 rows), DESKTOP-84T3LIN (6), DESKTOP-CPH4KB2
(45, after the reinstall corrected the stale binary); each record is schema 1 at
`C:\ProgramData\Summrise\record.db` and verified with `--check-record`. `SummriseSetup-0.3.292.exe`
stays on the mirror and rollback remains proven on DESKTOP-9VH92BJ for device and console.
DESKTOP-14RJCR8 is still not connected (9d 1h) — the one concrete blocker; it is conditional in
the objective and its deletion or reconnection is the operator's call. Gate: 16 packages / 27 edges
/ 0 upward; workspace tests were green in round 38. Nothing in the rollout remains unowned.

**Round 40 — 14R removed, and the baseline tells the truth about its bytes.** DESKTOP-14RJCR8's
registration was deleted through the console (`DELETE /fleet/<id>`; id `8780d0ee…`), which revoked its
tokens and refused its next connect; the console now lists 3 devices, all online. The CPH4KB2 anomaly
was identified: when its `baseline/current/summrise-device.exe` was hashed it was **0.3.277**
(`3a80a057…`, the mirror's old artifact), while the recorded sidecar said 2.0.0
(`d9d0ea9d…`) — a mixed pair; the running app was 2.0.0, which is why health looked right and only
`--check-record` exposed it. Reinstalling the published installer rewrote the pair. Hardening landed:
`state.get` now hashes the baseline binary itself (chunked SHA-256) and answers
`baseline: {path, recorded, file, matches}`, so a sidecar that disagrees with the bytes can no longer
hide behind its own metadata (`baseline_facts` unit test flips the file and sees `matches: false`).
Device suite 672 green, clippy/fmt clean, gate 16 / 27 / 0. Note: the fleet MCP token used by this
host expired during the round (console says mint a new `/mcp/all` token), so further remote
inspection needs a fresh token. Next: the entry/dispatch seam (S4), then the fleet join.

**Round 41 — installers name their channel.** `SummriseSetup-<version>.exe` was one file per version,
so publishing canary and stable of 2.0.0 meant the second publish overwrote the first channel's
installer. The NSIS `OutFile`, `publish-canary.bash` (sign + copy + mirror), `verify-release.sh`
(channel-qualified first, unqualified as the older-release fallback) and `rehearse-release.sh` now
use `SummriseSetup-<version>-<channel>.exe`; the console's `/installer` parser accepts both spellings
and still reports the version in the middle (unit test covers `-canary`/`-stable`). Both channels
were re-published: canary `f8a67840…` and stable `6bca2a9f…` are served under their own names, and
`verify-release.sh 2.0.0` is ok on both (3 devices registered). Console lib tests 176 and integration
283 green; docs/RELEASE.md has the naming rule. Next: the entry/dispatch seam (S4).

**Round 42 — one dispatch seam for every door (S4).** `mcp.rs` now names the call the doors were
already sharing: `Call { tool, arguments, caller }` and `Answer { outcome, journal_seq }`, with
`DeviceServer::dispatch(Call) -> Answer` as the single seam. `handle_acknowledged` — the path the MCP
door, the console channel and the mesh all reach — builds a `Call` and calls it; policy, the journal
pair and capability/plugin routing stay on that one path, so a new door cannot skip authorization by
parsing differently. The old internal capability table dispatcher was renamed `invoke` so the name
means the seam. New test `the_dispatch_seam_is_one_path_for_every_door` calls `dispatch` directly:
`state.get` answers with a journal row, and an unknown tool is refused (still journaled) before any
capability runs. Device suite 673 green, clippy/fmt clean, gate 16 / 27 / 0. Next: the fleet join.

**Round 43 — the fleet window joins every machine it draws.** `useJoinMarks` no longer refuses the
fleet's window: when `deviceId` is null it splits the rows by the `device_id` each carries and asks
`/audit/join` (the device's `journal.get`) once per machine for that machine's own traces, keeping the
per-machine cache, debounce and 30 s age. A row without a device id stays `unknown`, one offline
machine records `unknown` for its own traces and does not silence the others, and the aggregate
counts are the window's own. Tests: the hook's fleet case (two machines, two calls, both `one`), an
offline machine beside a healthy one, and the App test that used to assert "no machine is asked"
now asserts the machine that wrote the row is asked and agreement draws no mark. Page suite 1,397
green, typecheck clean. Next: a launcher-level regression for the mixed baseline pair (the CPH4KB2
shape: 2.0.0 sidecar beside a 0.3.277 binary) so the pair is refused rather than booted.

**Round 44 — the join effect's dependency is a dependency, not a comment.** The page carries no
eslint at all (no dependency, no config, no script), so the `// eslint-disable-next-line
react-hooks/exhaustive-deps` lines across `useJoinMarks`, `useFleetRecord`, `useMachineRows` and
others suppress a rule that never ran — comments pretending to be enforcement. The one in the
rewritten join hook is gone: `groupsForWindow` is memoised and the effect depends on `[groups,
answers]`, so the constraint is expressed by the code rather than disabled. (The rest of the inert
disables remain; a real checker or their removal is a separate decision.) Page suite 1,397 green,
typecheck clean, gate 16 / 27 / 0. Push is the one open item: GitHub's TLS endpoint keeps dropping
(`GnuTLS recv error -110`), so `origin/next` is still `dec53ebb` while the local branch is
`87b6bca1`; three commits are waiting for the network.


## Next (in order)

1. Windows handoff, the interactive half — dispatch or run `summrise-shell.exe --check-window` in an
   interactive session (session 0 refuses it), console down and up, look at the WebView2 panel, and
   verify Authenticode when a certificate is configured. The runner half is green (run 43344130298).
   The MCP-reached Windows devices can supply the interactive session, but installing a build on the
   operator's machines is their call, not the program's.
2. Push `origin`: `main` now contains `origin/main` and `hub/v1` (`0 1242`), so `git push origin main`
   is a fast-forward. This is a publish, and it is the operator's call.
3. ~~**The journal's epoch.**~~ **Withdrawn (2026-11-13):** the v2 record's change feed gives a
   cursor that never restarts — the feed id — so the epoch has nothing to reconcile. The parked
   `feat/journal-epoch` branch is abandoned; do not rebuild it on v1. The page's single-row read-back
   and its `trace.call` check stay as they are until the feed lands.
4. The fleet's join: with the fan-out in place, a fleet window can ask each drawn machine's
   `journal.get` for the traces that machine's rows carry (same cap, same 30 s), so `only here` stops
   being a machine-window-only fact and a fleet row can say which machine still holds it.
5. Keep improving from what the operator hits (one major item a round, green merges into `hub/v1`).

## Rules

- One stage per branch; merge green work into `hub/v1`; never wait for approval between batches.
- Targeted tests per change; full checks at stage boundaries.
- Sub-agents get self-contained prompts and disjoint files; the coordinator reviews, commits, merges.
- A task that returns red is fixed by the same sub-agent or by the coordinator if the fix is small.

**Round 45 — the push has a relay, and the script proves it.** GitHub stayed unreachable from the
build host (`GnuTLS recv error -110`, `ls-remote` timeouts), but the tailnet peer `oracle-phx`
(100.79.21.110) answers on GitHub (HTTP 200). SSH access was found under the Oracle Cloud key
(`~/.ssh/imported/oci-ssh-key-2026-09-08.key`, user `ubuntu`); a `ssh -D` SOCKS tunnel let
`git push` land: `origin/next` and `origin/main` are now `a4c0485f` (the five commits that had been
waiting: dispatch seam, fleet join, the join-deps fix, and two ledger commits). `scripts/push-via-relay.sh`
packages it: it opens the tunnel, waits for GitHub through it, pushes `next` and `next:main`, and
prints the remote refs; the second run answered "Everything up-to-date" and the expected refs. The
procedure is documented in `docs/RELEASE.md`. Next: DESKTOP-CPH4KB2's tray icon and the 7737 port.

**Round 46 — the dead port and the missing icon are named.** The two symptoms have one shape: the
device's own HTTP is loopback only and its port is the one the installer baked (`18090` on these
machines) while the compiled default is `7737`, so `http://<machine>:7737` is expected to refuse and
no remote host can reach either port. `docs/QUICKSTART.md` now says that beside the panel command and
adds rows for the refused `7737` and for a tray that does not come back after an upgrade (the
installer kills the old shell and starts the new one only in **active** sessions; a locked or
disconnected session waits for the next sign-in or an interactive `summrise-launch --start-shell`).
Verifying CPH4KB2's tray is blocked on remote access: the fleet token this host used for `/mcp/all`
has expired, and re-minting one needs the console's operator token (or a login link) on
DESKTOP-9VH92BJ; the host has no SSH server reachable with the keys here. The relay work was pushed
before this round (`origin/next` = `origin/main` = `7e749d26`).

**Round 47 — the tray restart now covers disconnected sessions.** The reason CPH4KB2 can lose its
icon after an upgrade is visible in the launcher: `--start-shell` enumerated sessions and skipped
everything that was not `WTSActive`, so an RDP session whose user was still signed in but whose
client was gone got no shell, and the installer's one-shot start found nobody to start it for. The
predicate is now a tested part of the pure half (`session_can_start_shell`: `WTSActive`,
`WTSConnected`, `WTSDisconnected`) and `win.rs` uses it, so the installer's start-shell task covers a
disconnected user as well; a shell started there is waiting when the user reconnects. Launch tests
157 + 9 green, clippy/fmt clean. This fix reaches a machine with the next build (2.0.1 or later);
the current 2.0.0 install still needs the documented manual start (`summrise-launch --start-shell`
in an interactive session) or a sign-in, and verifying it on CPH4KB2 remains blocked on fleet access
(expired `/mcp/all` token; no SSH to the Windows hosts).

**Round 48 — 7737 was the symptom: the shell could not read its port.** With a fresh fleet token
(minted through the console's trusted-proxy door, `x-forwarded-user` from 100.85.163.159) `host.facts`
and `runs.exec` reached CPH4KB2. `summrise-shell.exe` was running and its log said `tray built`, but
every start also logged `the device did not accept the helper registration … os error 10061`, and
`helper.registered` stayed false. The cause: the device writes `<data>/device.port` with the data
directory's ACL (SYSTEM and Administrators only), so the user's shell cannot read it and falls back
to the compiled default **7737**, where nothing listens — the same reason `http://<machine>:7737`
"does not open". The tray was built; its health check, helper registration and panel all dialled a
dead port. Immediate fix on the three machines: `icacls …\device.port /grant Users:(R)`; after
restarting the shell on CPH4KB2 the helper registered (`pid 27492, session 1`) and `screen.capture`
started working — the screenshot shows the desktop in a full-screen game, which is also why the tray
is not visible at that moment. Durable fix: the device also publishes its port to
`%ProgramData%\SummriseHelper\device.port` (user-readable, beside the helper token) and the shell
reads that copy when the data file is unreadable; a shell test covers the file read. Device 673 +
shell 25 green, clippy/fmt clean.

**Round 49 — the relay's first handshake is slow, not dead.** After the port fix, the relay script's
readiness check kept timing out at 10 s while the same tunnel answered GitHub in 19.6 s on the first
TLS handshake; the VPS itself reached GitHub in 0.55 s. The check now allows 30 s per attempt, and
the last two commits landed through it: `origin/main` and `origin/next` are **`fca95cfe`**. The
operator-facing facts for CPH4KB2: `summrise-shell.exe` is running, its log says `tray built`, the
helper is registered (`pid 27492`, session 1), and `screen.capture` works; the screenshot taken at
the time is a full-screen game, which is why the tray was not visible in it. The port ACL fix is
applied on all three machines; the device-side durable fix (publish the port beside the helper
token) ships with the next build.

**Round 50 — the legacy Electron shell is in the product.** The operator prefers the legacy
Electron local app over the Rust/WebView2 `summrise-shell`, so `agent/summrise-desktop-electron` is
vendored as `device/desktop-electron`: its `src/` (main/preload, the two wasm policy pairs, embedded
slots, browser sessions), its `test/` suite (76 tests) and the icons, with the test fixture at
`device/tests/fixtures/embedded-bridge.json`. Two adaptations make it belong to this product: a
`src/device-port.js` module resolves the device's real port from
`%ProgramData%\SummriseHelper\device.port` then `<data>\device.port` (with the legacy env/config
fallback after that), and its liveness probe asks the device's `/api/health`; the window loads
`/panel` instead of the legacy agent's `/desktop/`. `npm test` is 80 green (76 vendored + 4 port
tests), `tsc --noEmit` clean. The **legacy panel UI is not wired yet**: `panel-react` calls a large
legacy `/api/*` surface the device does not serve, so the window still shows the device's own
`/panel`; mapping or shimming that surface is the next slice. `npm install` is still fetching the
Electron binary on this host, so a Windows package has not been produced in this round.

**Round 51 — the Electron shell runs on CPH4KB2.** Installed from `dist/win-unpacked` (electron-builder
`--win dir`, unsigned, icon regenerated at 256×256 from the 128 px source) into
`C:\Windows\Temp\SummriseDesktop` and started in the user session: the process stays up, its control
server answers `/api/shell/icon-status` with a resolved tray icon (`icon.ico`, 256×256, non-empty), and
the window is visible showing the device's `/panel` page with the legacy File/Edit/View/Session menu.
`/api/health` is what the shell probes and `device.port` is where it gets 18090, so the legacy 18080
path is no longer used. The tray icon is created but Windows 11 still keeps it in the notification-area
overflow: its `NotifyIconSettings` entry for `SummriseDesktop.exe` is `IsPromoted=1` (set while
explorer was running), and an explorer restart re-added the icon without promoting it — the old
`summrise-shell.exe` entry that is visible was promoted through the Settings UI, not by this edit.
The legacy panel itself is still not shown; the window loads the device page, which is the remaining
work (build `panel-react`, add the adapter between its `/api/*` calls and the device, and decide the
tray-promotion path that actually works on Windows 11).

**Round 52 — the "legacy panel" is not in the 0.3.277 binary.** The 0.3.277 agent still exists under
`C:\SummriseInstall\versions\0.3.277` and starts with `--install <dir> --data <dir> --port 18080`
(it wants a token file; protected routes 401 until one exists). It serves **the same new-style page**
at `/` ("Summrise — this machine", inline CSS and `/panel/xterm.css`); `/desktop/`, `/api/status` and
`/api/sessions` are 404. The legacy `panel-react` source does build (`npm run build` → 677 kB
`panel.js`), but that output is the `panel/` bundle the old agent served at `/`, not the React app at
`/desktop/`, and its `/api/*` backend is not in this workspace: `panel-react` calls sessions, settings,
workspaces, tools, boots, monitors, plugins and an SSE events stream, none of which the 2.0 device
serves. The Electron shell is therefore still pointed at the device's `/panel`; the legacy UI needs its
backend (or a compatibility layer) before it can be shown. The spare legacy agent started for this
test was stopped; the 2.0 device on 18090 was left alone.
