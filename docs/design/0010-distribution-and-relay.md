# 0010 — Distribution and the relay: the decisions

> Opened 2026-09-23, because the operator asked whether "these problems can be solved at all"
> and then delegated the calls (*「你能自己决定吗」*). This file is those calls, the two facts
> that decided them, and what each one costs. ADR 0008 (layout v2) and ADR 0009 (self-contained
> pinned tgz) are referenced from the code but their records are not in this repository, so this
> file takes the next number by inference — correct it if the sequence lives elsewhere.

## The facts that decided it (measured 2026-09-23, not inferred)

| fact | how it was measured |
|---|---|
| **The npm registry is as fast as the CDN from the device** | `registry.npmjs.org` tarball: 4.17 MB in 1.80 s = **2.31 MB/s**. The CDN tgz the installer uses today: 6.69 MB in 1.95 s = **3.44 MB/s**. Same order. *An earlier claim in this session — "the device's npm points at a mirror, so a registry install is worse here" — was an inference from config, and this measurement retracts it.* |
| **Big binaries do travel as ordinary npm packages** | `@deepseek-ai/dsh` publishes **92 KB of its own JS** with 80 dependencies, **zero install scripts**, and a **537 MB installed tree that is all `node_modules/`**. Platform binaries arrive as *files inside packages* — `node-pty/third_party/conpty/…/win10-x64/{conpty.dll,OpenConsole.exe}`, `@deepseek-ai/node-addon-system-linux-x64/bin/{glibc,musl}/system.node`. Nothing is downloaded in a postinstall. |
| **Per-platform cloudflared packages already exist** | `agentroam-cloudflared-win32-x64`, `agent-remoteops-cloudflared-linux-{x64,arm64}` — the pattern is established in the ecosystem, so it needs no invention. |
| **The Cloudflare token on the build host reaches the Workers API** | `GET /accounts/<id>/workers/scripts` → **200**, listing `summrise-dist`, `vale-gate`, `vale-dist`, `opencode-go-proxy`, `zen-us-proxy`. |
| **The CDN worker is not only a download host** | `index/wrangler.jsonc` binds `TEMP_FILES` (R2) and `TEMP_CLAIM` (Durable Object) — the file relay's storage and its one-time claim. It also serves the landing page and `/api/version`. |
| **The version contract has two readers** | `index/src/index.js:396` serves it; `agent/summrise-agent-npm/src/summrise.ts:743` (CLI) and `agent/src/plugins/update/tools.rs:12` (Rust agent) read it. |

## The decisions

**D1 — The goal is a COMPLETE INSTALL, not portability.** The defect that actually hurt this
week is that a fresh install cannot bring up a tunnel, browser tools or a desktop window,
because the package carries no boxed components. That is fixable with npm's own mechanism. The
portability goal ("stop depending on the CDN host / Cloudflare") buys nothing today, costs a
server to operate, and does not follow from fixing the packaging — so it is deferred, not
rejected. *(Reversible: nothing below forecloses it.)*

**D2 — The audience is the operator and their own machines.** Not public users, not today.
Therefore discoverability and a public install experience are worth ~nothing, and the npm names
stay a **reservation** rather than a channel. If that ever changes, D7 is what makes the switch
cheap.

**D3 — No new server to operate.** No VPS, no self-hosted MinIO, no database. Everything stays on
the infrastructure that already exists. The cost of "fewer dependencies" would otherwise be paid
in TLS certificates, backups, credentials and availability — all of it the operator's time.

**D4 — The relay KEEPS its single-use Durable Object.** `claim.js:1-17` records the race it
closes: without serialization two concurrent GETs can both stream a one-time file. The DO is
short-lived per claim (milliseconds), holds no WebSocket, installs no alarm and writes no storage.
Removing it would save almost nothing measurable and reintroduce a defect that has already been
fixed once. What it *does* need is a page of documentation, not a rewrite.

**D5 — The CDN stays the install and update channel for now; the npm package name stays a
reservation.** Because (a) the version contract has two readers on the device side, (b) the CDN
is also the rollback (the previous release's tgz), and (c) nothing is broken about it. This is the
decision most likely to be revisited, and D7 is the prerequisite for revisiting it safely: once a
published package is *complete*, an npm-based install and an npm-based version check become a
small, testable change instead of a leap.

**D6 — The relay moves OUT of the CDN worker into its own worker.** The worker that serves the
release tarball should not also be the storage backend for the file relay: today "retire the CDN
worker" and "keep the relay" are the same question, and they should not be. The move keeps R2 and
the DO, keeps the endpoint shapes (`/api/upload`, `/files/<token>`) so the gateway's proxy and
every device-side tool are untouched, and is therefore a change with no semantic risk.

**D7 — Components ship as OUR OWN pinned per-platform npm packages**, declared as
`optionalDependencies` with `os`/`cpu` gating — not as a postinstall download and not as "copy
them from the previous install". The distinction that matters, learned the hard way: **binaries
carried as files inside a published package** (`node-pty`'s ConPTY) survive a locked-down
network; **binaries fetched in a postinstall** (`electron`, `playwright`) do not — that is why the
desktop shell needed `ELECTRON_MIRROR` on this network. Per-platform cloudflared packages are
published on the registry (`cloudflared` is Apache-2.0, so redistribution with its licence and
attribution is permitted; the binary is pinned and sha256-verified in the release pipeline, never
fetched on a device).

## Deferred, with the reason — so they are not rediscovered as new ideas

| deferred | why | what would revive it |
|---|---|---|
| npm-only install **and** update | Two readers of `/api/version`, and the CDN is the rollback. Nothing is broken. | D7 landing, then a small change |
| Leaving Cloudflare (VPS / S3 / B2) | D3. It also does **not** remove the need for an intermediary. | A concrete pain with Cloudflare, priced |
| Dropping the relay's Durable Object | D4 — it would reintroduce a fixed race | Proof that single-use is not wanted |
| Peer-to-peer transfer | The device is behind NAT, and this dev box is under a no-listening-ports policy: a ZeroTier/Tailscale interface is exactly the forbidden socket shape. | The policy changing |

## What each decision costs, and who pays

| decision | cost |
|---|---|
| D1/D7 | Publishing + maintaining three component packages; a licence/attribution obligation for the vendored binaries; ~90 MB of uploads per refresh |
| D5 | Keeping a bespoke update path alive a while longer |
| D6 | One more worker to see in the dashboard |

## Execution order

1. **D6 first** — it needs no new credential, and it removes the coupling that makes every other
   question feel entangled.
2. **D7 second** — needs exactly one npm credential (publish rights) at the moment of the first
   publish; the packages themselves can be built and tested before that.
3. D5/D1 are then re-evaluated on evidence, not on argument.
