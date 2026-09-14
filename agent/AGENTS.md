# Vale Agent Build Guide

> Mirrors agent/CLAUDE.md (build/verify/architecture semantics must stay
> identical). Post-2026-08-28 additions: registry-
> first path resolution (src/paths.rs), npm-only install channel, Gateway
> Settings card + POST /api/gateway/connect.

## Cross-compilation to Windows (MSVC)

Panel-first: the raw `cargo xwin build` commands below do NOT rebuild the panel SPA — after touching `resources/panel-react/`, run `npm run build` there first (or use `scripts/build.sh agent`, which does both), since panel.js is embedded at compile time via include_str!.

Requires `cargo-xwin` for cross-compiling from Linux:

```bash
# Install
cargo install cargo-xwin

# Windows check (fast, run after touching Cargo.toml or feature-gated code)
cargo xwin check -p vale-agent --target x86_64-pc-windows-msvc --features terminal,keyring

# Debug build
cargo clean && cargo xwin build -p vale-agent --target x86_64-pc-windows-msvc --features terminal,keyring

# Release build
cargo clean && cargo xwin build -p vale-agent --target x86_64-pc-windows-msvc --features terminal,keyring --release
```

Output binaries:
- `target/x86_64-pc-windows-msvc/debug/vale-agent.exe` (debug)
- `target/x86_64-pc-windows-msvc/release/vale-agent.exe` (release)

`scripts/build.sh agent` cross-compiles vale-agent (the retired tray/Tauri desktop builds were removed round-330).

## Install / update — npm is THE single channel

- `vale setup` = PURE LOCAL install (no key/tunnel/cloud). `--reg-key <key>`
  and `--tunnel <host>` are OPTIONAL extras; the Settings page Gateway card
  (`POST /api/gateway/connect`) is the GUI way to configure them.
- Install layout is registry-first: `HKLM\SOFTWARE\Vale\Agent\{InstallDir,DataDir}`
  — all path resolution goes through `src/paths.rs` (`install_dir()`/`data_dir()`
  + layout-v2 subdir helpers per `docs/adr/0008-install-layout-v2.md`: `etc\`,
  `components\`, `scripts\` under InstallDir, logs + `pwout\` under DataDir);
  zero `current_exe()` guesses outside it, zero legacy-directory probing
  (one versioned v2 migration exception).
- Boxed components: `vale-playwright.zip` → `InstallDir\components\playwright\`,
  `cloudflared.exe` → `InstallDir\components\` (agent-supervised, no Windows service).
- The OLD NSIS installer / setup.ps1 / run-setup.bat are RETIRED
  (`deploy/retired/`). Sharing front-end: the NEW online installer
  (`deploy/vale-setup.nsi` + `vale-online-setup.ps1`, NSIS 3.12, built by
  `scripts/build-installer.sh <ver>`) wraps the same npm channel (bootstraps
  Node, installs the pinned tgz, runs `vale setup`); test checklist in
  `deploy/README-installer.md`.

## Device update — npm one-click update (THE ONLY sanctioned rollout path)

**Always ship device updates through the npm flow. Never hand-roll
kill/copy/restart scripts over a terminal PTY** — the PTY is hosted by the
agent itself, so an inline `Stop-Process` kills your own shell before the
restart command runs and leaves the device dark (happened twice on d1).

**And never START a second `vale-agent.exe` from an agent-hosted terminal.**
The same hosting cuts the other way, and this was learned the hard way:

- The agent puts itself in a **kill-on-close Job Object**, and *every child it
  spawns inherits membership* — that is the documented design (see
  `setup_child_reaper_job` in `src/winmain.rs`), and it is why an update can
  never leave orphaned PTY shells behind.
- A shell running inside that agent is therefore IN that job. Anything launched
  from it inherits the same membership, so a second agent started that way ends
  up nested inside the first one's job.
- Observed on d1: launching a test build this way **killed the running agent**
  (which the 60 s watchdog then restarted, so the device came back on its own).
  It cost a device restart and an hour of diagnosis.

**There is also no way to isolate a second instance.** `paths.rs` resolves
`data_dir()` registry-first (`HKLM\SOFTWARE\Vale\Agent\DataDir`, else
`install_dir()`) and there is **no environment override** — so a second agent
shares the live one's session directory and would run `recover_interrupted`
over its audit files. A `VALE_DATA_DIR=` env var does nothing.

If a second instance is genuinely needed, launch it DETACHED — WMI
`Win32_Process.Create` (what `vale update` already uses, precisely so the swap
script survives the agent dying) or a one-shot scheduled task — and accept that
the data dir is still shared. For verifying a new Windows build, prefer the
static checks (PE validity + `strings` for the new code paths) over running it
beside the live agent.

Release + rollout:

```bash
# 1. Build the exe (panel changes must be built BEFORE this — panel.js is
#    embedded at compile time via include_str!):
cd resources/panel-react && npm run build && npm test && cd ../..
cargo xwin build --target x86_64-pc-windows-msvc --release --features terminal,keyring --bin vale-agent

# 2. Stage artifacts into the npm package and bump its version:
cp target/x86_64-pc-windows-msvc/release/vale-agent.exe vale-agent-npm/vale-agent.exe
# then bump "version" in vale-agent-npm/package.json (1.2.x)
# (bridge.js was removed in round-263 — the npm package ships no bridge)
cd vale-agent-npm && npm pack          # → vale-agent-1.2.N.tgz

# 3. Publish: stage the tgz into the dist worker assets (ALSO the
#    versionless latest alias + the version.json discovery manifest)
#    and deploy them (or run scripts/publish-release.sh <ver>, which wraps
#    pack + stage + alias + manifest + last-5 prune + commit + deploy):
cp vale-agent-1.2.N.tgz ../../index/public/vale-agent/
cp vale-agent-1.2.N.tgz ../../index/public/vale-agent/vale-agent-latest.tgz
# version.json MUST carry the tgz sha256 — /api/version requires ver && sha
# (else it answers 503 and agent_update refuses the install, round-119):
SHA=$(sha256sum vale-agent-1.2.N.tgz | cut -d' ' -f1)
printf '{"version":"1.2.N","tarball":"vale-agent-latest.tgz","updated":"%s","sha256":"%s"}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$SHA" > ../../index/public/vale-agent/version.json
cd ../../index && CLOUDFLARE_API_TOKEN=$(cat ~/.cloudflare-token) npx wrangler deploy

# 4. On the device (PowerShell), exactly two commands — WITH the --prefix (see the
#    Install/update block above: without it the install lands elsewhere, reports
#    success, and `vale update` silently ships the OLD release):
npm i -g --prefix (Split-Path (Get-Command vale).Source) https://agent.saisi.online/vale-agent/vale-agent-latest.tgz   (or pin the version)
vale update
```

# 5. GitHub Release = CI (release.yml rewrite): after the version-bump
# commit is pushed, create the tag ON GITHUB VIA THE API — direct git
# push of tags intermittently times out on this network; the API is
# reliable, and the tag-push event triggers the workflow (panel build →
# xwin exe → npm pack → gh release create --verify-tag). package.json
# version MUST equal the tag; mismatch fails fast. keep-latest: delete
# the previous release AND older tag refs manually (API, /git/refs/tags/
# <tag> — the URL needs the full refs path, not just the name).

# 6. Collapse the two builders (recommended whenever byte-identity matters):
./scripts/publish-cdn-from-ci.sh <ver>
#    Stages the artifact CI just built onto the CDN — installer rebuilt from
#    THAT tgz, manifest rewritten, redeployed, smoked — so "CDN == GitHub
#    release" holds BY CONSTRUCTION and scripts/lib/release-audit.sh reports
#    byte-for-byte equality instead of listing an exe difference.
#
#    Why it exists: the two builders produce the same artifact, and this makes
#    that structural instead of audited.
#
#    THE "EXES ARE NOT BYTE-IDENTICAL" STORY WAS FALSE, AND I MEASURED IT. On the
#    live release pair the two tarballs differ by exactly 3 bytes out of
#    17,774,080, and the 17.5 MB vale-agent.exe is BYTE-IDENTICAL between the two
#    builders — as it has been for at least twenty consecutive releases, since the
#    audit's WARN arm is reachable ONLY when the exes match. The 3 bytes were a
#    FILE MODE: `package/README.md` packed -rw------- here and -rw-r--r-- in CI,
#    because npm pack preserves the worktree's permissions while git tracks only
#    the executable bit. The compile environment was never the problem; a
#    worktree whose permissions differed from a fresh checkout was.
#    publish-release.sh now REFUSES to pack on that difference and
#    release-audit.sh compares modes and fails on any drift.
#
#    Opt-in on purpose: making the DEFAULT publish wait on CI would put the
#    delivery channel behind a pipeline that still fails on environment
#    issues. Fail-closed: it runs the audit first and refuses to touch the CDN
#    unless the CI artifact packages the same source.

Toolchain (reproducible builds — do not undo this):
- `rust-toolchain.toml` pins rustc 1.98.1 for every rust command in this repo;
  release.yml/ci.yml pass the same version explicitly (`dtolnay/rust-toolchain@master`
  + `toolchain: 1.98.1`). NEVER reintroduce `@stable` — a floating channel is
  what made the two builders drift in the first place.
- CI installs LLVM 18.1.8 from the OFFICIAL release tarball (cached; 1 GB) and
  symlinks cargo-xwin's tool cache at it, mirroring the release box's ~/llvm18.
  It also unpacks focal's libtinfo5 beside it — the 18.04 tarball's clang needs
  .so.5 and the 24.04 runner only has .so.6.
- `cargo install cargo-xwin` must stay `--version 0.23.0`.
- `agent/build.rs` passes `/Brepro` + `/DEBUG:NONE` for the MSVC target only.
  Without them lld stamps a freshly randomised PDB GUID plus a build-time PE
  timestamp into every link, so even two LOCAL builds differed (by 20 bytes);
  with them the exe is a pure function of its inputs.
- Doubting the runner's toolchain? Dispatch the `toolchain-fingerprint` job
  (`gh workflow run ci.yml` / the API) and compare its hashes with the release
  box's `~/llvm18` and rustup toolchain.

What `vale update` does (bin/vale.js): stages the exe (and desktop shell
sources) next to the install dir, hands a PS swap script to WMI
Win32_Process.Create (parented by
WmiPrvSE so it survives the CLI AND the agent dying; plain `-NoProfile -File`
only — `-ExecutionPolicy Bypass` / `-EncodedCommand` die silently on d1),
then: stop ValeAgent task → kill agent tree → copy with retry →
restart task. The terminal connection DROPS for ~10 s mid-update; reconnect
and verify via `/api/status` → `version`.

**A DROPPED CONNECTION IS NOT PROOF THE UPDATE STARTED.** It is the documented
signature of a successful swap, and that is exactly the trap: a transport
failure that never delivered the command looks identical from the caller's side.
Observed on d1 (round 17): `vale update` returned a connection error, was read
as "the swap is running", and had in fact never reached the device — no
`update-busy` marker, no staged `vale-agent.new.exe`, no `scripts\vale-update.ps1`,
no `update start` line. The device was still on the old version.

Verify by EFFECT, with two commands that answer it without guessing:

```powershell
vale status                  # release running, this CLI's version, and the update state
Get-Content "$env:ProgramData\Vale\logs\vale-update.log" -Tail 20
```

`vale status` reads the update-busy marker and reports one of three things, which
mean different things and only one of them is an error: **none in flight** (no
marker — a finished swap clears it), **IN FLIGHT** (fresh marker, a swap is
running now), or **STARTED AND DID NOT FINISH** (marker past the 10-minute
freshness window — the update died before its cleanup, and re-running is safe).
It also prints the drift between the running release and this CLI, which is the
plainest answer to "did the update take?".

`vale update` now appends an `update requested X -> Y` receipt to
`vale-update.log` BEFORE the handoff. So one file separates the cases the
connection drop conflates. Read it as a THREE-way distinction, because the
Rust `agent_update` path (the console/auto channel) writes `update start` to
this same log too, but has no receipt — it has its own tool-result channel:

| `update requested` | `update start` | what it means |
|---|---|---|
| present | absent | the CLI reached the device, the swap never launched — re-run |
| present | present | the CLI's swap launched; check `copy ok=` / `task restarted` below it |
| absent | present | the swap was launched by `agent_update` (Rust), not by the CLI |
| absent | absent | the command never reached the device at all |

The busy marker is cleared by a completed swap, so a marker still present past
the 10-minute window means the swap died before its cleanup. Re-running is
always safe — the operation is idempotent.

**WHAT THE ROUND-17 EVIDENCE ACTUALLY PROVES** (worked out afterwards; the log
originally said only "cause not established"). The marker is the FIRST statement
of `update()`, and every branch of that block either creates it or exits 1 — so a
run that reached `update()` ALWAYS leaves a marker. Observed on d1: no marker, no
staged exe, no swap script, no log line. A marker would also have made the SECOND
attempt refuse (fresh <10 min), and it did not. So the CLI-side explanations are
EXCLUDED: **the CLI never executed on that device.** Why is still unknown, but the
investigation belongs at the MCP tool-call transport, not in the script — and that
is a real narrowing, because three plausible explanations were eliminated rather
than guessed away.

**A FAILED SWAP CANNOT BE DETECTED FROM THE CLI'S EXIT CODE.** `vale update`
returns 0 the moment the WMI handoff is ACCEPTED — `ReturnValue=0` means a process
was created, and the script has not yet written its first line. Every decision
that matters (the fail-closed migration gate, the 12× copy retry, the `$ok`-gated
marker write, the task restart) happens after, in a WmiPrvSE-parented process
whose exit code nobody reads. So: **no path returns non-zero for a failed swap.**
`vale rollback` used to depend on that exit code and wrote its version marker
unconditionally as a result — claiming a version the device was not running, which
makes every UI lie AND makes `agent_update` answer `up_to_date` forever. It now
reads the marker BACK and requires it to show the staged version before pinning.
`vale update` itself still returns on handoff, deliberately: it can block without
risk only where the caller already blocks (`rollback` does).

`vale rollback <x.y.z>` (bin/vale.js): HEAD-checks the pinned tgz on the CDN
(last-5-per-minor keeps the recent line), `npm install -g --prefix
<components\npm-global> <tgz>`, then runs the TARGET build's own `vale update`
so the staged exe IS the rollback build. It then **reads `etc\.vale-release`
back and requires it to show the target version** (bounded 90 s) before writing
`etc\.rollback-pin` and deleting a pre-v2 root-level marker. An unproven swap
writes NO pin, NO marker, and exits non-zero naming the version the device is
actually on — see "A FAILED SWAP CANNOT BE DETECTED FROM THE CLI'S EXIT CODE"
above for why the old unconditional write was a lie that could strand a device.
`agent_update`
(Rust) returns `{"status":"pinned"}` for any remote version other than the pin
while the pin exists; `force:true` on agent_update or `vale rollback --clear`
removes it. `vale update` does NOT clear the pin (it swaps what npm-global
holds = the pinned build). `vale autostart <on|off|status>` flips the ENABLED
flag on both boot tasks — `vale stop` is one-shot (the 5-min watchdog revives
it), so autostart is the only real "don't start at boot" control.

Gateway (`gateway/`) deploys separately: `cd gateway && wrangler deploy`.

**THE GATEWAY'S MODEL CATALOGUE IS HARDCODED AND NOTHING WATCHES IT.** It comes
from `gateway/src/channels.ts`'s `MODEL_REGISTRY`, where one record carries SIX
facets (advertised id, upstream wire slug, US-egress policy, web-search
capability, health card, vision) and `model-registry.test.mjs` keeps them
bidirectional. Adding or retiring a model means editing that source and
redeploying — and no workflow deploys this worker (CI runs `wrangler deploy
--dry-run` for the proxies only). So an upstream adding a model, or silently
RETIRING one, tells nobody.

`node scripts/model-drift.mjs [--json] [--strict] [--gateway <url>]` reports what
each channel advertises against what its upstream offers. Four upstreams answer an
unauthenticated `/models` (or 445, nv 82, cm 69, og 37); gmi/qw/amd/ds answer 401
and are reported as NOT CHECKED rather than as empty. `advertisedNotOffered` is
printed as CHECK — never as a verdict — because the router normalises further
(`[1m]` markers, `og/` wire remaps) and raw name diffing reports false drift; see
the first live run, which flagged one genuine absence among several aliases that are
fine. THAT ONE WAS ACTED ON (round 57): `nv/minimaxai/minimax-m3` was advertised and
NVIDIA offers no MiniMax at all — not prefixed, not bare — so the entry was retired
and the live `/v1/models` went 22 -> 21. The check held up under the obvious
objection (that the wire name might differ from the advertised one) precisely
because the BARE name is absent too; `nv/moonshotai/kimi-k3` and
`nv/nvidia/nemotron-3-ultra-550b-a55b` resolve exactly, so the list is current.
`og/minimax-m3` is a different channel and is untouched. It is an OPS TOOL, deliberately NOT a CI gate: it needs four live
third-party endpoints.

## Architecture

vale-agent is a pure service — MCP server + terminal backends + SSE endpoints
+ the Electron desktop shell (embedded real browser on CDP 9333). The Tauri
desktop (`vale-desktop/`), the standalone `vale-tray/`, and the NSIS-era
installers are RETIRED; the Electron shell (`vale-desktop-electron/`) and the
gateway device app replaced them. The
web panel (`/panel`, Apple-style terminal) is served by `src/web/` — the
browser either carries the proxy-secret marker, runs on loopback, or presents
a one-time `?grant=` the agent redeems at the gateway (round of the panel-
grant fix: the permanent device token never rides in a URL); the token is
injected server-side into the panel HTML.

- **MCP** (rmcp): served at `/mcp` ON THE MAIN AGENT PORT (default 18080,
  same HTTP surface) — token-gated via `TokenGate` in `src/web/mod.rs` (rmcp has
  no server-side auth hook). There is no separate port 3000 any more.

### Module map

```
src/
  main.rs          server binary (config path as argv[1]); Windows service
                   mode via windows-service when launched by the SCM
  lib.rs           crate root; DEFAULT_CONFIG_YAML embedded (include_str!)
  paths.rs         REGISTRY-FIRST path resolution (the single source of truth:
                   `install_dir()`/`data_dir()` + the layout-v2 subdir helpers).
                   FOUNDATION module — zero `current_exe()` guesses and zero
                   legacy-directory probing outside it.
  bootstrap.rs     vale_command::bootstrap::load_or_create(path, fallback) —
                   create-if-missing, load, ensure_token. Single bootstrap site.
  register.rs      pure self-register PLANNING seam (`self_register_plan`):
                   decides whether/where to self-register, so the network call
                   in main.rs's loop stays untested-thin (boundary review
                   2026-09-06).
  metrics.rs       device vitals for /api/status (CPU delta + memory, kernel32)
                   AND the sampler that owns their clock: one reading every 30 s
                   into a bounded ring, served to /api/vitals/history and drawn as
                   the panel's Device health charts.
  monitor.rs       REACHABILITY monitoring: watched host:port targets (persisted,
                   bounded list), a TCP-connect probe every 15 s into a bounded
                   per-target series, and the summaries /api/monitors serves. The
                   DEVICE keeps the watch so it outlives the session, the terminal
                   and the panel — see the module header for why it does not ping.
  tunnel.rs        cloudflared tunnel PROVISIONING for the Gateway card
                   (rewrites tunnel.yml + signals restart via crate::tunnel_ctl).
                   The RUNNING child is owned by main.rs's supervisor, not here.
  winmain.rs       `#![cfg(windows)]` process plumbing: boot self-heal,
                   kill-on-close child-reaper job, bounded helper runner, the
                   SCM service entry, the supervised cloudflared owner. Moved
                   verbatim from main.rs (structure refactor A7); non-Windows
                   builds compile none of it.
  filelog.rs       size-rotating tracing writer -> DataDir\logs\agent.log (layout v2)
  session_log.rs   per-session JSONL audit log (trim-on-close + 30 d retention)
  evidence.rs      the pwout AI-evidence feed (crate-private, SOLID R98):
                   actions.jsonl append/newest-first read, shot listing,
                   basename guard, `browser-actions-changed` push, and the
                   feed's AGE-BOUNDED RETENTION (`prune`: screenshots +
                   pwai_*.js + old action lines; a 1-day floor makes an
                   in-flight action's artifacts undeletable). ONE owner for
                   both producers (playwright browser_run_script + mcp-client
                   tools) and the /api/browser/* readers.
  text.rs          byte-budget text clipping (crate-private, SOLID R105):
                   `boundary_at_or_below` / `clip` — the "cut to <= N bytes on
                   a char boundary" rule that was hand-written at 8 sites and
                   panicked the session drainer three times.
  jsonl.rs         append-only JSONL crash safety (crate-private, SOLID R111):
                   `prepare_append` (version header on a fresh file, terminate
                   a torn final line) + `has_torn_tail` + `rewrite_atomically`
                   (temp file, fsync, rename — what the two retention prunes
                   age records out with, and why their writers hold a lock).
                   Shared by the audit trail and the memory store.
  operation.rs     the device's MERGED operation timeline (crate-private):
                   terminal audit + browser actions on ONE ordered axis,
                   served by GET /api/operation. Orders on `ts_ms` only — the
                   two feeds stamp `ts` in different units, so a record
                   lacking the explicit millisecond stamp is DROPPED rather
                   than placed by guess. Device-level, not session-level: the
                   embedded browser has no session ownership.
  runs.rs          RUN identity, one AI execution's mint/end log
                   (crate-private): `begin`/`end`/`recent`/`trim` over an
                   append-only runs.jsonl (age-bounded by `trim`, same 1-day
                   floor as the evidence feed). The id is minted DEVICE-side
                   and is a LABEL, NEVER A CREDENTIAL — nothing here returns
                   an authorization decision, and
                   `run_id_is_never_a_credential` pins that.
  runstate.rs      the agent PROCESS's run journal — NOT `runs.rs`'s AI run
                   identity, and the pair is easy to confuse: this one is
                   started / last heartbeat / exited-cleanly for the process
                   itself, written at boot and beaten on a timer, so a run that
                   died without a word is distinguishable from one that exited
                   on purpose. `classify` (pure, tested) owns the verdict —
                   first-run / clean-exit / replaced / machine-restart /
                   crashed, with the host's boot time telling a reboot from a
                   crash — and `describe_previous` phrases that same answer for
                   `startup.log` while `begin` persists `kind=<x>` above it for
                   `/api/status` (`last_boot` + `last_boot_kind`). One rule, a
                   sentence and a datum, and an agreement test binding them.
  state.rs         AppState { serial_pool, terminal_mgr, event_bus,
                   plugin_registry, config } — managers are Arc<Manager>,
                   managers own their locks internally (inside AppState only
                   config_path carries a small std Mutex and config a std
                   RwLock — write-through via update_config, read via
                   config_snapshot)
  mcp/server.rs    DeviceServer (rmcp ServerHandler), bind() -> (addr, handle)
                   (port 0 = ephemeral, used by tests), serve_with_token
  web/             HTTP surface — hand-rolled Tower service (NOT axum route
                   handlers: they break Windows cross-compilation). TokenGate<S>
                   wraps the /mcp route with the bearer check. mod.rs owns auth +
                   dispatch + the api_* handlers (routes: GET / (minimal status
                   page), /api/status, /api/spec, /api/events (SSE),
                   /api/events/poll, /api/events/term (SSE), GET/PUT
                   /api/settings (buffer_mb + console_url),
                   POST /api/gateway/connect (Settings-page Gateway card:
                   persist console_url, reg-key → CF token exchange, optional
                   free tunnel via provision_tunnel),
                   POST /api/tools/{name}, GET /api/plugins/status,
                   GET /api/browser/{pwshots,pwshot,actions} (AI evidence —
                   the pwout screenshots/action feed), GET /api/sessions
                   (audit list)); panel.rs serves the embedded /panel (static
                   whitelist + token injection + one-time ?grant= redemption
                   at the gateway) as the WebPanel fallback service; sse.rs
                   holds the SSE streams (bounded conns, heartbeat, epoch).
  plugins/         PluginRegistry (tools cached once at register); monitor/ (the
                   reachability watch surface — monitor_list/add/remove/probe over
                   crate::monitor); terminal/
                   mod.rs (plugin struct + shared helpers) + tools/ (ctx.rs =
                   ToolCtx, the shared runtime state builders take, plus the
                   jobs map; per-domain builders exec/sessions/files/
                   output/secrets/connections; mod.rs owns registry assembly
                   + the exact tool order); memory/ (store.rs = the JSONL
                   knowledge store, whose append hygiene comes from
                   crate::jsonl); runs/ (the RUN IDENTITY tool surface —
                   run_begin/run_end, a thin shell over crate::runs: begin
                   MINTS the id device-side, end ACCEPTS one and reports
                   `known`. MCP tools rather than a route because the caller
                   IS the AI and MCP is its only channel here)
  tools/           terminal/ (TerminalManager + TermBackend trait; pty.rs,
                   ssh.rs, serial.rs, secrets.rs, stub.rs), serial.rs, ssh.rs
vale-command-core/      Plugin/ToolDef/ToolHandler/NavItem, Config (+ensure_token via
                   getrandom), DeviceError (typed variants), EventBus/AppEventBus.
                   CANONICAL import path for core types: `vale_agent_core::…`
                   (lib.rs's `vale_agent::` re-exports are a compat shim for
                   external/embedding consumers — internal code never adds
                   consumers to them; unified 2026-09-05)
(vale-tray/ and vale-desktop/ Tauri source deleted round-330 — both
 retired; the npm CLI + Electron shell replaced them. Git history has
 the old crates.)
```

## Conventions

- **Commit style**: conventional commits with stage tags (`fix(stage-g)`,
  `refactor(stage-i)`, `perf(stage-h)`, `feat(stage-k)` …). Each commit must
  leave the workspace green.
- **Verification per change**: `cargo test` → `cargo clippy --all-targets
  -- -D warnings` (round-301: CI promotes EVERY warning — mirror CI exactly;
  grep ^error misses warnings CI fails on) → `cargo xwin check -p vale-agent
  --target x86_64-pc-windows-msvc --features terminal,keyring`. After touching
  feature-gated code, also run `cargo test --features terminal,keyring` and
  `cargo clippy --features terminal,keyring --all-targets -- -D warnings`. Smoke:
  `cargo run --bin vale-agent --features terminal,keyring -- /tmp/ct.yaml`
  then curl `/api/status` and `/api/tools/terminal_list` with the Bearer token
  from `/tmp/ct.yaml`.
- **Feature-gating rule**: real terminal code is gated behind the `terminal`
  feature (PTY/SSH/serial); secrets behind `keyring`. The boundary lives only
  in the `#[cfg]` mod declarations and re-export lines (`pub use desktop_impl::X` /
  `pub use stub_impl::X`). Public paths must stay identical across configs so
  headless tests exercise the full dispatch path against the stubs.
- **Locks**: managers own their locks internally (tokio Mutex on Inner).
  Callers hold `Arc<Manager>` and never `.lock()`. Poison recovery:
  `unwrap_or_else(|p| p.into_inner())` — never silently drop data.
- **Channels**: output bounded with backpressure (blocking_send in reader
  threads); keystrokes try_send drop-on-full.
- **MCP tool additions**: define the tool in
  `src/plugins/<plugin>/tools.rs` (terminal: `tools/<domain>.rs` — pick the
  domain module whose concern it shares; mod.rs owns registration order);
  the registry caches it at register time — no other registration site.
  Update the tool-count test in plugins/terminal/mod.rs (26 tools:
  22 terminal_* incl. env/jobs/saved/connect/forget + secret_* legacy aliases)
  if adding/removing terminal tools; the plugin tests in
  plugins/{memory,system,mcp_client}/mod.rs cover their own counts.
- **Console MCP visibility is a SEPARATE decision**: the gateway's `/mcp`
  registry (`gateway/src/mcp-tools.ts`) is a hand-maintained SUBSET of the
  device's, and `tools/call` looks a name up there BEFORE routing — an
  unmirrored device tool is not merely unlisted, it is uncalled (21 of 49
  tools sat invisible with every gate green). After adding or removing a
  tool, regenerate the inventory the gateway contract reads:
  `VALE_REFRESH_SPEC=1 cargo test --features terminal,keyring spec_snapshot`
  (rewrites `agent/spec-tools.json`), then either register the name in
  `mcp-tools.ts` (and satisfy `isDeviceDirectTool()`'s routing) or add it to
  that test's `NOT_EXPOSED` map WITH A REASON. Doing neither fails the
  gateway suite.

## Device memory + desktop shell

- **memory plugin** (`src/plugins/memory/`): device-local knowledge base shared
  across AI clients — 6 MCP tools (`memory_save/search/list/update/delete/
  export`). JSONL + in-memory index at `<install>/memory/memory.jsonl`, soft
  delete, capacity capped OLDEST-WRITTEN-FIRST (not LRU — reads never move
  `updated_at`) from config `memory: { max_entries, max_bytes,
  retention_days }`, credential sanitizer (`sanitize.rs`). Lives at
  `data_dir()/memory` (registry-first `DataDir`), NOT under InstallDir.
- **stdio transport (no port)**: `mcp_client_connect` defaults to
  `transport=stdio` — the bundled playwright-mcp is spawned over stdin/stdout
  (newline-JSON frames, rmcp `TokioChildProcess`), NO listening port.
  `transport=http` (9229) remains for external servers. Test override:
  `VALE_TEST_STDIO_NODE` / `VALE_TEST_STDIO_ENTRY` (see
  `tests/mcp_stdio_integration.rs`).
- **saisi decouple**: `config.yaml platform.console_url/download_url` are
  OPTIONAL — unset means a purely local install; `agent_update` and
  `page_view` remote pages error explicitly, device self-register skips.
- **desktop shell**: `vale-desktop-electron/` (Electron) loads
  `http://127.0.0.1:<port>/desktop/` (`<port>` = config.yaml server.port, default 18080) — the same SPA in desktop mode (terminal/
  browser/memory/plugins/settings rail). Owns CDP 9333 for AI driving, a tray
  with health + vitals, a 60 s AGENT WATCHDOG (`schtasks /run ValeAgent`), and
  a wait page that reappears when the agent dies mid-session. The
  `vale-desktop/` Tauri shell is retired. `/desktop/` reuses `/panel/` assets +
  loopback token injection (web/mod.rs). round-274: main.ts sets
  backgroundThrottling:false + the --disable-renderer-backgrounding /
  --disable-backgrounding-occluded-windows switches — a hidden window
  (hide-to-tray / background session) otherwise flips the SPA to
  visibilityState=hidden, Chromium stops requestAnimationFrame, and xterm's
  rAF-driven DOM renderer silently stops painting (blank terminals while the
  AI keeps operating).

## vale-tray / vale-desktop (Tauri) — DELETED (round-330)

Both crates were retired long ago (npm CLI replaced the tray; the
Electron shell replaced the Tauri desktop) but their source + build
steps lingered. Round-330 removed the source trees and their builds
from build.sh (git history retains them). The npm CLI
(`vale` from `vale-agent-npm/bin/vale.js`) is the management surface.

## Windows smoke checklist (manual)

Terminal: open pty (PowerShell), type + resize, ssh + serial sessions, saved
connections + keychain password. MCP: `claude` direct device MCP
(`https://dN.../mcp`) and `/api/tools/terminal_list` with the Bearer token.
Events: `/api/events` SSE + `/api/events/term` stream. Electron shell:
tray shows health + vitals, 60 s watchdog recovers a dead agent, wait page
reappears when the agent dies; desktop SPA mirrors the panel (CDP :9333
drives the same view). Gateway card: `POST /api/gateway/connect` registers
console URL + key from the Settings page. `/api/status` reports the npm
release (not the Cargo version).

## History

The per-round iteration log that used to live here (147 entries, ~14k lines) was **deleted on the
operator's instruction** — it was ceremony: the same story is in the commit messages, attached to
the tree it describes, and `git log` is where it is read. `docs/agents/iteration-coverage.md`,
`docs/agents/iteration-loop.md`, `scripts/surface-coverage.mjs` and the four tests that enforced
that bookkeeping (`ledger_head`, `coverage_numbers`, `review_cadence`, `ideas_inbox`) went with it.

What remains true: the operator's own inbox is `docs/agents/ideas.md`, and their rules are
`docs/CHARTER.md`.
