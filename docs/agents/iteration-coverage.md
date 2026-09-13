# Iteration coverage ledger

What has been looked at, by which round, and what is still open. Read the
newest entries of the round log in `agent/AGENTS.md` alongside this; keep the
table small (a row is a summary, not a report).

State: `unseen` · `partial` · `seen`. Round numbers refer to the round log.

Seeded 2026-09-14 at round 110.

## Current state

- Round log head: **round 116**; HEAD `9777a1df` + this round's gateway fix (DEPLOYED,
  `vale-gate` version `2484d970-af69-4bd2-add8-b729fae54393`).
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
| gateway `plugins/admin` + `store/providers` (custom providers) | seen | 116 | F6 a dead "Edit facets" button on the DEFAULT row; F7 "Adopt" erases declared effort; F8 Delete 404s on provider models; F9 the file layer is marked per prefix only; the provider-model editor itself |
| gateway `plugins/models.ts` + `models-probe` + `model-route` (the catalogue) | seen | 105, 116 | F6 above; a provider write propagates in ≤60 s but `models:*` takes up to 24 h (`store/cache.ts`), so a built-in facet edit can read back stale |
| gateway `plugins/translate` (+vision) | seen | SOLID R117–R121 | — |
| gateway `mcp.ts` / `mcp-tools.ts` / `mcp-browser.ts` | seen | 78, 89 | — |
| gateway `reliability` / `upstream` / `channels` / `body-scan` / `http` | partial | 64, 89 | — |
| gateway console SPA (`gateway/ui`) | seen | 61, 91–93 | — |
| index worker | seen | 99, 115 | — (F2/F3 closed and DEPLOYED round 115) |
| proxies (zen-go / zen-us / vrelay) | partial | 64 | — |
| extension (Vale Code Links) | unseen | — | — |
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
| ops `scripts/` (publish / installer / audit / cdn-from-ci) | partial | 102–110 | CDN-vs-GitHub reconcile proposal awaits sign-off; `npm publish` awaits `npm login` |
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
3. **Agent restarted every 1–2 h before round 110** (cause unknown) — the run journal is armed and
   answers it at the NEXT boot; its first line, "no previous run on record", is a gap in the
   instrument, not a clean bill of health.
4. **The panel's provider-model surface — round 105's leftovers, VERIFIED in round 116** (two
   read-only scouts; the claims had been carried across four commits that touched that very page).
   - **CLOSED: "provider-model effort is not in the add row"** — it lives in the row's Advanced
     block (`Models.tsx:884-933`) and the server has always accepted it.
   - **OPEN (b2): a model inside a CUSTOM provider has NO editor.** The intent is written at
     `Models.tsx:687-693` and the gate `(!pv || custom.includes(id))` makes it unreachable. The
     server half is now in place (round 116), so the fix is a sibling of `addModelToProvider` that
     replaces ONE entry in the re-posted record, plus widening that gate.
   - **OPEN (F6): the DEFAULT row's models render an "Edit facets" button whose every save is a
     400** — `unknown channel prefix deepseek/`, because `parseModelSpec` knows only the registry
     prefixes. A control that cannot work is worse than no control.
   - **OPEN (F7): "Adopt" silently erases declared `reasoningEffort`** — `adoptModel` re-posts the
     existing models without the field, and the server replaces the array wholesale.
   - **OPEN (F8): the Delete/Disable button renders on provider-model rows and 404s**
     (`No custom model my/llama-3`).
   - **OPEN (F9): the file layer is marked per PREFIX only**, so a file-declared `models:` /
     `overrides:` entry stays editable in the panel and the next deploy silently reverts it — the
     panel already receives `file.{providers,models,overrides}` and reads only `.text`.
   - **OPEN (F10): no onboarding / needs-setup hint anywhere** — Overview has no zero-key branch
     and i18n has no such key.
5. **Panel F5** (host allowlist family match) — recorded as hardening only.
6. **ADR 0007 step 3** — `RELAY_ADMIN_CUTOVER` flag exists, default off; flipping it is a deprecation-window decision (propose).
7. **CDN ⇄ GitHub release reconcile** — member-wise comparison + exe provenance proposal awaits sign-off.

## Finding ID registry

`gateway F2` redirects followed without re-validation · `gateway F3` `deviceFetch`
skips the suffix allowlist at dial time (fixed round 95) · `panel F1` loopback
branch trusts a client Host header · `panel F2` `?grant=` not single-use over
eventual consistency, no device-side audit · `panel F3` grant route shape check
looser than the gateway's · `panel F5` host allowlist family match · `memory F5`
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
