# Iteration coverage ledger

What has been looked at, by which round, and what is still open. Read the
newest entries of the round log in `agent/AGENTS.md` alongside this; keep the
table small (a row is a summary, not a report).

State: `unseen` · `partial` · `seen`. Round numbers refer to the round log.

Seeded 2026-09-14 at round 110.

## Current state

- Round log head: **round 121** (the extension's link allowlist: it now refuses credential homes,
  resolves relative mentions against the real workspace, and stops turning prose into links);
- Round 120 (the DISCOVERY round: four read-only scouts on surfaces no round
  had audited — extension, the agent's `design` plugin, proxies/vrelay, the release machinery —
  ~45 findings, two HIGHs fixed here, one of them LIVE-CONFIRMED and deployed); the console/gateway fixes of rounds 116-119 are all DEPLOYED
  (`vale-gate` version `3187a717-f35d-48e3-a0d3-16eb9624a0b4`, console bundle `index-CR3KX755.js`).
- **THE STANDING LIST IS CLEAR OF OFFLINE WORK**, so the loop moves to the DISCOVERY track the
  protocol describes: the surfaces below still marked `unseen` (the extension, the agent's `design`
  plugin, brand) and `partial` (proxies, the deploy PowerShell, CI) are the fuel now. The three
  items that remain open all wait on someone outside the loop: the user (CHARTER-1), a maintenance
  window (the dead-agent revival), the next boot (the restart mystery).
- **A "STILL OPEN" LINE IS A CLAIM, NOT A FACT — two rounds running now.** Round 115 found memory
  F5 already closed since round 110; round 116 read round 105's three leftovers and found two of
  them touched by four later commits (`99b42928`, `5030ab60`, `9249e790`, `1d8b8468`) that were
  never written into this journal at all (rounds 106–109 have no entry). Verify before inheriting.
- **HEAD WAS RED AND NOBODY KNEW**: `tests/module_map.rs` failed against BOTH guides because
  round 110 added `src/runstate.rs` without adding it to the module map, and round 111 shipped
  on top of that. Found by round 112's independent gate run, fixed there. The lesson is cheap to
  state and expensive to forget: `cargo test` also runs `tests/`, and one of those tests READS THE
  GUIDES — so adding a module is a map change, not only a code change.
- **d1 IS BACK AND CURRENT (1.2.364)** — `vale status`: RUNNING, `this device is current`, no
  update in flight; install `D:\Vale`, data `C:\ProgramData\Vale`. The multi-day dark period is
  over. Round 114 spent the live session verifying what rounds 110/111 could only pin: the boot
  task's revival contract (trigger/settings read from the live task, and the protective half
  proven BY EFFECT — a manual start while healthy is refused and the process count stays 1), the
  tunnel supervisor's spawn line, and the run journal. All read-only; see the round-114 log.
- **The restart-every-1-2-hours mystery is still OPEN, with the instrument now armed**: the run
  journal exists and is heartbeating, but its first boot line reads "no previous run on record"
  because the journal only exists since 1.2.363 — so the NEXT boot is the one that answers it.
  Do not read that line as "the previous run was clean".
- **NOT tested, deliberately**: reviving a DEAD agent (the boot task's other half). Killing the
  agent takes cloudflared with it (kill-on-close job), darkening the only device for up to five
  minutes, with a human recovery if the revival fails. Proposed as a maintenance window, not
  taken.

## Surfaces

| Surface | State | Rounds | Open |
|---|---|---|---|
| gateway `device-fetch` + `plugins/devices` | seen | 89, 90, 95 | F5 host allowlist is a family match — hardening only |
| gateway `store/` (KV cache, grants) | seen | 94 | grant single-use needs the device-side backstop (shipped) |
| gateway `auth` / `session` / `access` | seen | 61, 89 | — |
| gateway `plugins/admin` + `store/providers` (custom providers) | seen | 116, 117 | F6/F7/F8 fixed in 117; F9 (file layer marked per prefix only) open |
| gateway `plugins/models.ts` + `models-probe` + `model-route` (the catalogue) | seen | 105, 116 | F6 above; a provider write propagates in ≤60 s but `models:*` takes up to 24 h (`store/cache.ts`), so a built-in facet edit can read back stale |
| gateway `plugins/translate` (+vision) | seen | SOLID R117–R121 | — |
| gateway `mcp.ts` / `mcp-tools.ts` / `mcp-browser.ts` | seen | 78, 89 | — |
| gateway `reliability` / `upstream` / `channels` / `body-scan` / `http` | partial | 64, 89 | — |
| gateway console SPA (`gateway/ui`) | seen | 61, 91–93, 117–119 | two behavioural harnesses now exist (`smoke:models`, `smoke:overview` — jsdom over the BUILT bundle); other pages still have none |
| index worker | seen | 99, 115 | — (F2/F3 closed and DEPLOYED round 115) |
| proxies (zen-go / zen-us / vrelay) | seen | 64, 120 | P1 (HIGH) protocol-relative path sent the caller's GitHub token to any host — FIXED + DEPLOYED + verified live in round 120; P2 **CLOSED in 120** (verified again in 129: `upstreamUrl`'s origin assert is in all three handlers and the live probes `/api/github/web//example.com/` and `/api/gform//example.com/` both answer 400 — the ledger line calling it open was stale); P3 **CLOSED in 129 as a CONTRACT fix, with its impact explicitly NOT reproduced**: all five handlers now use the documented headers-only budget (`clearTimeout` once headers arrive) and a test proves a body outliving the budget still arrives — but reverting to `AbortSignal.timeout` does NOT truncate the body on Node 24/undici, so "bodies were being cut at 30 s" is not established; what was real is that three handlers did not implement the budget their own README documents while two siblings did; P4 **CLOSED in 131 for both CF workers** (`redactSecrets` ported from the gateway and applied to the 4xx client text + the 5xx log lines in zen-go and zen-us, with leak tests and a short-secret guard test) — NOT DEPLOYED (a worker deploy is a release action); the vrelay `zen.js`/`proxy.js` paths are BYOK (a caller seeing their own key echoed is not a third-party disclosure), so P4's credential scope is closed; P5b **CLOSED in 133** (github.ts and gform.ts now genericize 5xx too — the contract is uniform across all five relay handlers; no credentials ride those two, so it was a consistency fix); P5 **CLOSED in 132** (zen.js and proxy.js now return generic client text on 5xx — the upstream body no longer streams to the caller — and the 4xx pass-through stays, deliberately: that text is what a caller needs, and these two forward the CALLER's own key, so an echo returns a secret to its owner, unlike the workers fixed in 131); P6 a stale routing comment at the decision site; P7 the disproven zen-us US pin still asserted in its own files; P8 `count_tokens` ignores system+tools; P9 **PARTLY CLOSED in 136**: the PASS-THROUGH now uses the upstream's own `content-type` (with the SSE default kept for an upstream that declares nothing), pinned by two tests; **P9b stays open** — a `stream:false` answer takes a CONVERSION branch that still forces SSE, measured by a `todo` test in `zen-us-proxy/test/behavior.test.mjs` (a todo that starts passing is the signal it closed); P10 **CLOSED in 135**: the comment now states BOTH halves of the contract (an ABSENT `target` is og, a documented default; an UNLISTED one is 400 so the caller's key cannot ride an unvetted upstream), and the default is pinned by a test that compares an absent target against an explicit `?target=og` rather than hardcoding a host; **CI does not run these 54 tests** — only `node --check` |
| extension (Vale Code Links) | seen | 120, 121 | **X2/X5/X7 CLOSED in 121** (no link outside the code-server root, no dot-directory but `.github`, the workspace base, a leading boundary + a known-extension rule for relative mentions), and X4 half-closed (an extensionless FILE still resolves to itself — recorded as a known limit, not guessed at with a name list); X1 (HIGH) rewrites React-owned text nodes → streaming freezes; X2 no folder allowlist: any chat text mints a one-click link into `.ssh`/`.dsh` inside the authenticated IDE; X3 the processed-stamp kills later content; X4 the dot heuristic is wrong both ways; X5 the "configurable base" is hardcoded and the test pins the WRONG path; X6 options shows an origin that is not in effect; X7 prose becomes links (`read/write`); X8 the security model named in comments is wrong (`--auth none`) |
| agent `plugins/terminal` (26 tools) | partial | 88, SOLID | — |
| agent `plugins/memory` | seen | 99, 110 | F5 was closed in round 110 in code (the unread `load_failed` flag; `true` after a failed append) with the test forced through a REAL failure seam — the LIVE half stays test-level by choice, since proving it on the device means corrupting the store |
| agent `plugins/runs` + `runs.rs` | seen | 110 | — |
| agent `plugins/update` | seen | 553–555, SOLID | layout-v2 migration + rollback still need a device run |
| agent `plugins/system` / `mcp_client` / `playwright` | partial | 89, SOLID R98 | — |
| agent `plugins/design` | unseen | — | — |
| agent `web/` (auth, panel grant, SSE, api) | seen | 88, 92 | grant single-use / audit record |
| agent `winmain.rs` boot path + `tunnel.rs` + `runstate.rs` | partial | 110, 112, 113 | the 5-minute repetition trigger and the supervisor fix are pins only — RUNTIME needs a device; tunnel F1/F2 are closed (113), leaving the runtime half |
| agent `paths.rs` + layout v2 | seen | 555 | — |
| agent CLI (`vale-agent-npm/bin/vale.js`) | seen | 78 | — |
| agent packaging + Electron shell (`vale-desktop-electron`) | partial | 555 | — |
| agent `deploy/` PowerShell (setup/update/installer/tasks) | partial | 555, 110 | — |
| agent panel SPA (`resources/panel-react`) | seen | 90–93 | — |
| ops `scripts/` (publish / installer / audit / cdn-from-ci) | seen | 102–110, 120 | D1 (HIGH) `--skip-reconcile`'s guard is neutralized by `\|\| true`: a missing token = "no release exists" → the P0 audit is skipped and the run reports success; D2 (HIGH) an unexplained tarball difference is a PASS in `release-audit.sh`; D3 (HIGH) the tag/release/audit stage is printed text, not code — **half-closed in 123**: the debt is now a tracked file the next publish refuses to ignore (ADR 0009), MEASURED as three versions (1.2.362-364, CDN 200 / GitHub release 404) — the manual tag step itself remains, and creating those tags is a published-release action awaiting CHARTER-1; D4 **CLOSED (the door half) in 125**: MEASURED live — the CDN's `ValeAgent-Setup.exe` was byte-identical (same etag) to `ValeAgent-Setup-1.2.361.exe` while the release was 1.2.364 and the manifest advertised no installer, and the landing page linked it unconditionally; the page now offers an installer only when the manifest carries BOTH `installer` and `installer_sha256`, deployed and verified live. **D4b CLOSED in 126**: `smoke_index_release` now NAMES the alias when the manifest advertises no installer — absent is reported as consistent, present is reported with its digest, the words "stale artifact", the way out, and a summary line that no longer reads as if everything matched (`scripts/test/smoke-index.bash`, 12 checks over the real body with `curl` stubbed, four mutations caught). The ARTIFACT itself is still the 1.2.361 installer — that is a release action (build-installer for a current version), not a code change; D5 **CLOSED in 128** (`scripts/test/build-pins.bash` compares all five `toolchain:` literals against `rust-toolchain.toml`, and mutation M2 — bumping the .toml alone — turns it red); D6 **CLOSED in 128** (README.md — the file whose MODE caused the twenty-release WARN — was in `files[]` and in NONE of the three tgz content gates; all three carry it now, the check keeps them in step, and the ONE documented exception (the xwin-less CI job has no exe) is itself asserted to stay documented); D7 **CLOSED in 128** (ci.yml's install is pinned to 0.23.0 and the check requires every `cargo install cargo-xwin` to carry release.yml's pin — it was RED on the unpinned line before the fix); D8 three release paths call bare `npx wrangler deploy` (unpinned); D9 **CLOSED in 127** (an undateable exe input now REFUSES — the scout's `git log` claim re-verified here: rc 0 with empty output — pinned by two source checks); D10 **CLOSED in 124** (the installer's CDN fallback now verifies the tgz against `/api/version` before `npm install`, logic tested on CI's pwsh runners, wiring pinned in `agent/tests/installer_integrity.rs`, live digests verified identical); D11 **CLOSED in 127** (the flat-5 deleted EVERY installer of the previous minor line — measured red-first with a two-minor fixture; now per-minor like its sibling, 3 checks + 3 mutations); D12 **CLOSED in 130** (the standalone deploy path now verifies that `/api/version` advertises the installer it just published and FAILS otherwise, naming the way out; the comparison lives in the CI-driven lib and the wiring is pinned before the success line — the deploy path itself was NOT executed, since that publishes); D13 the orchestrators have no executable coverage at all ; **D1/D2 CLOSED in 122** (three-valued verdicts + the unexplained-bytes arm now fails) ; **MEASURED 122: the reconcile leg has not run since v1.2.361** — tags `v1.2.363`/`v1.2.364` and their GitHub releases are 404 while the CDN ships those versions to devices, so `--skip-reconcile` is not an edge case, it is the only path any recent release took, and the audit has never run on one |
| ops CI (`ci.yml`, `release.yml`) | partial | 102–110 | — |
| docs (README / ARCHITECTURE / ADRs / this ledger) | partial | — | — |
| field device d1 | seen | 114 | current on 1.2.364; verified live (boot-task contract, tunnel supervisor, run journal). NOT tested: reviving a dead agent — proposed maintenance window. |

## Open items (each needs an owner round)

**Closed in round 112: the orphan `agent/src/tunnel.rs` change.** It had already been committed
as `86d84e5b` (round 111, paused, with no gate numbers recorded). Round 112 verified it: both
correct mutations of the seam fail its test — a best-effort write inside `install_tunnel_config`,
and a restart requested BEFORE the write. Mutating the CALLER's `if let Err` away stays green —
that gap was finding tunnel F2, closed in round 113 below.

**Closed in round 113: tunnel F1 + F2.** `update_remote_config` now RETURNS its verdict
(`RemoteConfig::Updated` / `Failed(reason)`) instead of returning `()` through eight bare
`return`s, and the operator's line is decided by a pure `tunnel_outcome(local, remote, hostname)`.
Four mutations are caught: the remote arm reporting plain success, the local arm reporting
success, the PARTIAL string dropping its cause, and — round 112's blind spot — the CALL SITE
inventing the verdict, which is pinned by a source check because that caller does network I/O and
no behavioural test can reach it. A remote config that did not land now reads
`PARTIAL (host): … NOT updated (<cause>) — a remote configuration OVERRIDES the local file, so
verify this tunnel before trusting it`.

**Closed in round 114: d1 recovery.** The device is up, current on 1.2.364, and round 114 used it
to verify the round-110/111 device fixes at runtime (boot-task trigger + IgnoreNew read from the
live task and the protective half proven by effect; the tunnel supervisor's spawn line; the run
journal heartbeating).

**Closed in round 115: index F2 + F3, deployed and verified live.** `vale-playwright.zip` is an
executed artifact at a MUTABLE key, so it now carries R2's content digest as a quoted ETag plus
`public, no-cache` and the object's authoritative size: a revalidating device gets a 304 (0 bytes),
a replaced bundle changes the digest, and a stale archive can no longer be served for a day. The
three binary proxies now answer every failure — including a REJECTED fetch, which used to become
the platform's 500 HTML — through one `proxyFailure` helper carrying the JSON envelope this file
already defined for uploads, with `cache-control: no-store`. Live: 200 + etag `"5fd7cf29…"` +
`content-length 31374231` + `public, no-cache`; `If-None-Match` → 304/0 bytes; a non-matching
validator → the full body; tgz, `version.json` and the cloudflared proxy unchanged.

**Closed in round 120 (the discovery round): the `design` plugin's HTTP layer.** A read-only audit
found that `page_view` returned a non-2xx as the page (a 530 error page read as "the design of the
download site"), that its token redactor silently REWROTE panel.js (613,677 bytes returned for a
613,667-byte file) while missing the token in other shapes, that its description advertised a
"remote host" the gate refuses, that it defaulted to a hardcoded 18080 (reading a stranger's
service on a custom-port install), that it read the whole body before the 64KB clip, and that it
followed redirects past its own loopback gate. All fixed, with three new pins and one VACUOUS
assertion replaced (`is_char_boundary(len)` is true for every &str).

**Closed in round 120: the vrelay host escape (HIGH, live-confirmed).** `GET
https://v.saisi.online/api/git//example.com/` returned Example Domain's HTML with the caller's
GitHub `authorization` header attached — a protocol-relative path REPLACES the origin. Two layers
now (shape + resolved-origin), three copies pinned byte-identical, deployed, and verified live:
all three escape forms 400, the legit route still 200, the 401 gate intact.

**Closed in round 119: the last item of round 105's list — the first-run hint.** The Overview now
tells a fresh operator what to do, and stays quiet when the deployment already has credentials of a
kind the key matrix cannot see (a custom provider's own key). Round 105's leftovers are now ALL
closed: b2 + F6/F7/F8 in 117, F9 in 118, F10 here.

**Closed in round 118: the file layer's per-ID marking.** A model the CONFIG FILE declares kept a
live `Edit facets` and a live `Delete/Disable`: both answered 200 and the next deploy silently
reverted them. The panel already carried the sentence for exactly this — the per-ID facts simply
never arrived. Now they do.

**Closed in round 117: four per-model controls now match the store that owns the model.**
A provider's model has a facet editor (a re-post with one entry rebuilt); the DEFAULT row's edit
button is gone (every save was a 400); "Adopt" carries untouched entries verbatim instead of
erasing their `reasoningEffort`; and Delete no longer renders where it can only 404. Proven by
`gateway/ui/models-render-smoke.mjs` — jsdom, the built bundle, a real click, and the captured
request — with six mutations caught.

**Closed in round 116: the provider record could not be updated without its key.** `POST
/api/admin/providers` demanded exactly one of `apiKey`/`apiKeyEnv` on EVERY post while the admin
view returns only a mask — so the console's documented edit path ("re-post the prefix", in
`adminAddProvider`'s own comment) was a 400 for every panel-created provider, taking "add a model",
"Adopt" and any future per-model edit with it. An omitted key is now carried forward from the
stored record; a create still demands one and a supplied key still replaces it. Deployed, and the
deployed mirror is byte-identical to the tested source.

1. **CHARTER-1 — a conflict inside our own constitution (proposal, waits for the user).** The
   CHARTER's blast-radius row puts "published releases" in the propose column, while the goal
   objective says release cadence is the loop's and reserves sign-off for full rollout and
   deprecation windows. Read strictly, the CHARTER forbids what the objective grants. Proposed
   replacement for that cell: **"a release that replaces the current version for everyone, and
   closing a deprecation window"** — publishing under the cadence stays autonomous, replacing
   everyone's version does not. Until the user answers, no release ships (the CHARTER is stricter,
   and stricter wins).
2. **Reviving a DEAD agent is untested** (the boot task's other half). Killing the agent takes
   cloudflared with it, so the only device goes dark for up to five minutes and needs a human if
   the revival fails. Proposed: a maintenance window — `Stop-Process vale-agent`, then confirm the
   task revives it within 5 minutes and the tunnel returns.
3. **Agent restarted every 1–2 h before round 110** (cause unknown) — the run journal answers
   *how* a run ended at the next boot; round 120 added the *who/what* half by enabling
   `Microsoft-Windows-TaskScheduler/Operational` on d1 (`wevtutil sl … /e:true`, verified
   `enabled: true`; it was off, so no history survived). Measured at 2026-09-14 01:37:02:
   `started=1789317807 last=1789320989 exited=0`, process StartTime 00:43:27 → **53m35s of
   uninterrupted uptime**, which is INSIDE the old 1–2 h window, so it proves nothing yet. Read
   the task-scheduler log at the next restart (or after ~2 h of uptime to see whether the old
   pattern stopped with 1.2.364).
4. **The panel's provider-model surface** — round 105's leftovers, verified in round 116, and the
   controls fixed in round 117. **Closed:** "provider-model effort is not in the add row" (it was
   there); **b2** the provider-model facet editor (re-posts the provider record with one entry
   rebuilt); **F6** the DEFAULT row's dead "Edit facets" button (every save was a 400); **F7**
   "Adopt" erasing declared `reasoningEffort`; **F8** the Delete button that 404'd on a provider's
   model. Evidence: `npm run smoke:models` (14 checks, 6 mutations caught).
   - **F9 CLOSED (round 118): the file layer is now marked per ID too.** `GET /api/admin/models`
     returns `fileModels` / `fileOverrides` from the same pure helper the prefix list uses, so a
     file-declared model offers no edit, no delete, and shows the config-file tag. Proven by 20
     harness checks (with a CONTROL case) and four mutations.
   - **F10 CLOSED (round 119): the Overview carries a "Start here" card** — add a channel key
     (→ /keys) and register a device (→ /devices) — each line rendered only while its own
     precondition holds, and the key line additionally requires that no CUSTOM PROVIDER holds a
     ready key, because 0/8 keys is not "no credentials". Proven by `smoke:overview` (three scenes,
     8 checks) and four mutations.
5. **Panel F5** (host allowlist family match) — recorded as hardening only.
6. **ADR 0007 step 3** — `RELAY_ADMIN_CUTOVER` flag exists, default off; flipping it is a deprecation-window decision (propose).
7. **CDN ⇄ GitHub release reconcile** — member-wise comparison + exe provenance proposal awaits sign-off.

## Finding ID registry

`gateway F2` redirects followed without re-validation · `gateway F3` `deviceFetch`
skips the suffix allowlist at dial time (fixed round 95) · `panel F1` loopback
branch trusts a client Host header · `panel F2` `?grant=` not single-use over
eventual consistency, no device-side audit · `panel F3` grant route shape check
looser than the gateway's · `panel F5` host allowlist family match · `panel F10` no first-run hint (fixed round 119) · `panel F9` the file layer was marked per prefix only (fixed
round 118) · `panel F6/F7/F8` a 400-ing edit button on the DEFAULT row,
"Adopt" erasing effort, a 404-ing delete (fixed round 117) · `memory F5`
a failed append reported as a saved record, an unreadable store read as EMPTY
(fixed round 110) · `index F2/F3` no validator on the executed bundle, failures
outside the file's protocol (fixed and deployed round 115) · `tunnel F1`
`update_remote_config` failed silently · `tunnel F2` the FAILED branch was unpinned
(both fixed round 113).

## ADR index

Adopted: 0001 plugin core · 0003 repo topology · 0004 panel grants · 0005
write-through config · 0006 retire studio · 0007 scoped relay token (step 3
pending) · 0008 install layout v2 · 0009 self-contained installer · 0010 two
products, one repo. Proposals: control-path, game-design, interactive-browser.
0002 is intentionally unused.
