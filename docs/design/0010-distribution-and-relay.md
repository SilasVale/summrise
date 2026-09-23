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

**Checked against the platform's documentation the same day, and the decision survived — with one
correction to its stated reason.** The comment's "R2 has no compare-and-swap" is **half wrong**: R2
*does* have a compare-and-swap on **create** (conditional `put` via `onlyIf` / the `Headers` form;
on precondition failure `put()` returns `null` and nothing is stored; writes and deletes are
strongly consistent — [Workers API reference](https://developers.cloudflare.com/r2/api/workers/workers-api-reference/)).
What R2 lacks is exactly the operation a one-time **consume** needs: a **conditional delete**. The
binding's `delete` takes no options, and the S3 compatibility matrix lists conditional operations
for Get/Head/Put/Copy but **not** for `DeleteObject`/`DeleteObjects`
([S3 compatibility](https://developers.cloudflare.com/r2/api/s3/api/)). So "delete it only if the
ETag still matches" is impossible here, and the DO stays the correct primitive.

The alternatives were checked too, so the next reader does not re-open them blind:
**presigned URLs** are *"reused multiple times until it expires"*, bypass the Worker entirely (so
nothing learns the download happened) and cannot use custom domains
([presigned URLs](https://developers.cloudflare.com/r2/api/s3/presigned-urls/)); **KV** is
last-write-wins with up to 60 s propagation; **Queues** is at-least-once and **Workflows** retries
steps — neither is a claim primitive. If the DO is ever to go, the documented answer is **D1**:
single-threaded, one query at a time, implicit transaction, and `D1Result.meta.changes` gives
`DELETE … WHERE token = ?` + `changes === 1` as an atomic claim
([D1 limits](https://developers.cloudflare.com/d1/platform/limits/)) — with the caveat that *"each
individual D1 database is backed by a single Durable Object"*, i.e. that relocates the primitive
rather than removing it. **Non-Cloudflare note:** AWS S3 *does* support conditional deletes
(`If-Match`, 412 on mismatch), so an S3-based relay could be DO-free — D3 is what rules that out,
not the availability of the primitive.

**D5 — The CDN stays the install and update channel for now; the npm package name stays a
reservation.** Because (a) the version contract has two readers on the device side, (b) the CDN
is also the rollback (the previous release's tgz), and (c) nothing is broken about it. This is the
decision most likely to be revisited, and D7 is the prerequisite for revisiting it safely: once a
published package is *complete*, an npm-based install and an npm-based version check become a
small, testable change instead of a leap.

**D6 — The relay moves OUT of the CDN worker into its own worker.** The worker that serves the
release tarball should not also be the storage backend for the file relay: today "retire the CDN
worker" and "keep the relay" are the same question, and they should not be. The move keeps R2 and
the DO and keeps the endpoint **paths** (`/api/upload`, `/files/<token>`).

**Correction, same day, from the platform's own docs — the first version of this paragraph said
"every device-side tool is untouched, therefore no semantic risk", and that was wrong.** The relay
cannot simply take those paths on the CDN worker's hostname **for the upload leg**, because the
gateway reaches the upload endpoint with a **same-zone `fetch()`**, and Cloudflare is explicit that
*"Routes cannot be the target of a same-zone `fetch()` call"* while *"Custom Domains can be invoked
within the same zone via `fetch()`"*, and — the sentence that settles it — *"On the same zone, the
only way for a Worker to communicate with another Worker running on a route … is via **service
bindings**"* ([Routes and domains](https://developers.cloudflare.com/workers/configuration/routing/)).

**And a second correction, an hour later: no new hostname is needed either.** The Custom Domains
page documents the interaction with Routes, from the other side: *"A Worker running on a Custom
Domain is treated as an origin. Any Workers running on routes **before** your Custom Domain can …
call the Worker registered on your Custom Domain"* — with the worked example of a route on
`api.example.com/auth` triggering `auth-worker` while `api.example.com` itself belongs to a Custom
Domain ([Custom Domains → Interaction with Routes](https://developers.cloudflare.com/workers/configuration/routing/custom-domains/)).
So a route on the **existing** host wins for the paths it matches. The settled mechanics:

- **download** — a route `…/files/*` on the existing download host → `summrise-relay`. External
  clients fetch it (device, build host), where the same-zone limitation does not apply, and the URL
  the upload handler mints (`${url.origin}/files/${token}`) therefore keeps its host: **no doc, no
  tool and no device-side change**;
- **upload** — a **service binding** `gateway → summrise-relay`, so this leg is not public at all
  (a smaller surface than today, where `/api/upload` is reachable) and no custom domain is involved;
- **the secret** — the gateway injects `Bearer ${env.UPLOAD_KEY}` (`devices.ts:406`) and the worker
  compares it, and worker secrets are **write-only** (the same constraint BRAND.md records for the
  `vale-gate` worker name), so the shared value cannot be copied to the new worker. A fresh value is
  therefore minted and set on both — a coordinated deploy, and the one genuinely risky step;
- the old bucket drains on its 24 h TTL rather than being migrated.

**The mechanical split is already done and test-verified** (2026-09-23): `relay/` holds the upload
and claim halves plus their tests, and the two suites report **relay 49/49** and **index 90/90** —
the 16 failing cases in the first run were precisely the CDN's concerns (landing page, the
playwright/cloudflared/electron proxies, `/api/version`), which is how the boundary was confirmed
rather than assumed.

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

## The publishing model, read off what dsh actually publishes (measured 2026-09-23)

Rather than invent a registry flow, the design was taken from `@deepseek-ai/dsh` — a product with the
same shape (an npm-installed agent with a large dependency tree) and a public registry record:

| what the registry and the installed tree show | what it means here |
|---|---|
| **One scope, many small packages** — `@deepseek-ai/dsh-agent`, `dsh-agent-loop`, `dsh-api-gateway`, `dsh-anonymous-user-id`, … | D7's component packages are this pattern: `@summrise/cloudflared-win32-x64` and friends |
| **dist-tags as channels**: `latest: 0.1.5-rc.2`, `next: 0.1.5-rc.3`, **`alpha: 0.1.7-alpha.2`** — `latest` is *older* than `alpha` | a release goes to **`alpha`** first and is promoted to `latest` once a device has proved it. Better than the CDN's mutable `-latest.tgz` alias, which is trusted by construction |
| the `latest` tarball is **48,910 bytes / 10 files**, while the installed tree is 537 MB | the meta-package stays tiny; the fat parts are dependencies (D7) |
| **25 versions in ~6 weeks** (created 2026-08-10, modified 2026-09-22) | a fast alpha cadence is normal for this shape — their `1.2.N` cadence fits it |
| `attestations: false` | they do not use npm provenance. Summrise could: `--provenance` needs a public repo plus CI OIDC, and the repo is public |
| the landing page's quick start is **`npx @deepseek-ai/dsh web`** | ours becomes **`npx summrise-agent setup`** on the CDN landing page, once the registry carries a real version |

**Step 1 is implemented (2026-09-23).** `publish-release.sh` grew `--npm` /
`--npm-tag <alpha|next|latest>` (default `alpha`), a trimmed `~/.npm-token` or `NPM_TOKEN` credential
following the `cf_token` idiom — *trimmed*, because a token file ending in a newline authenticates as
nothing — a **fail-fast precondition placed with the other guards** (a missing token must not surface
after the CDN has already been deployed; a half-published release is the one state that script must
never leave behind), and a publish through a **temporary 600 userconfig** to `registry.npmjs.org`,
since this box's npm points at a read mirror. Without `--npm` the run prints a *named* `::warning::`
instead of drifting in silence. Its suite grew two cases that assert the refusal and the warning's
presence (10 checks, green).

**Step 1 is DONE and measured on the target OS (2026-09-23).** The audited pack
(`summrise-agent-1.2.452.tgz`, sha256 `4e48905a…`) was published to the registry under `alpha` and
then promoted to `latest` — which is the rule above applied literally: 1.2.452 is the version d1 has
been running, so it has been proved. On the device, from the registry and not the CDN:
| what was run on d1 | what it printed |
|---|---|
| `npx summrise-agent status` | `status: RUNNING / install dir: D:\Summrise / release: 1.2.452 / this CLI: 1.2.452` |
| `npx summrise-agent autostart` | `SummriseAgent: Running` |
| `npm i -g summrise-agent --prefix …` | shims `summrise`, `summrise.cmd`, `summrise.ps1` at the prefix ROOT, and the shim reports the same status |
| the tarball the registry serves | sha256 `4e48905a…` — **byte-identical to the audited artifact**, so the two channels cannot disagree |

**Three things this measured that the reasoning got wrong, recorded because each would otherwise be
written down as fact later:**

1. *"npx only runs a command named after the package, so a `summrise` bin makes `npx summrise-agent`
   useless."* **Wrong.** It ran. With a single `bin`, npx uses that one. (dsh works by the same rule,
   not by a special one.)
2. *"The global install created no shim."* **Wrong** — looked for it at `<prefix>/bin`, which is the
   POSIX convention. On Windows the shims land at the **prefix root**.
3. The failures on the build host were **`os`/`cpu` gating working as designed** (`"os": ["win32"]`,
   `"cpu": ["x64"]`): a Windows-only agent refusing to install on Linux is the feature, and npm's
   message (`notsup Actual cpu: x64`) says so. Testing a Windows package on Linux proved nothing, and
   the honest test had to move to the device.

**And one npm policy fact worth keeping:** a granular access token with *Bypass 2FA* may **publish**
but may **not** unpublish — `403 … may not perform this action`, npm's newer hardening. So the
`0.0.1` placeholder cannot be retired with the credential that created it; it needs either an
interactive 2FA session or the website, inside the 72-hour window. It is harmless meanwhile: both
dist-tags point at 1.2.452.

**One design error this release found by being RUN — the useful kind.** 1.2.453 was published to
`alpha` with `latest` promoted afterwards, and that deadlocked the device: the CDN's `-latest.tgz`
alias moves on *every* release, so `npm i -g summrise-agent` installed a CLI **older** than the
release, and the CLI's own refusal guard ("this CLI is 1.2.452 and the release channel has 1.2.453 —
install the new CLI first") then pointed at a command that could not deliver it. The fix is in the
script's DEFAULT: publish to `latest`, because for this product the CDN alias defines what latest
means; `alpha` stays available through `--npm-tag` for a deliberate prerelease channel. dsh can
afford alpha-first because its `latest` is a *release candidate*, not an alias a device already
follows.

**One leftover the same release surfaced.** d1's config still pointed `relay_url` at
`http://127.0.0.1:18990` — a LOCAL helper that lived in the old install and left with it — so the
new agent's relay client retried a dead port every 30 s. Emptying the key stops it
(`mcp/server.rs:256` spawns the client only for a non-empty URL). Mind the name collision: that
`relay` is the **outbound relay client** for remote MCP access (`agent/src/relay.rs`, the agent half
of `proxies/summrise-relay/`), and it is NOT the file relay this document is about.

**Sequencing decision (changed on purpose):** the landing page keeps advertising the CDN tarball
until **step 2** lands. The page is the promise and the package is the delivery, and advertising a
one-command install that yields a tunnel-less agent would be worse than a URL that does — both
channels ship the same incomplete pack today, so the text is not the bottleneck. Fix the delivery,
then make the promise.

**What step 2 turned out to be — re-scoped by reading the code, 2026-09-23.** The decision above said
"publish the components as our own pinned per-platform npm packages". **The release host already does
something better**, and the plan had not noticed:

| component | where a device gets it today | the anchor |
|---|---|---|
| playwright bundle | `/summrise-agent/summrise-playwright.zip`, streamed out of R2 | authored here |
| cloudflared (54 MB) | `/summrise-agent/cloudflared.exe` → a **PINNED, versioned** upstream GitHub asset, proxied | `agent/src/tunnel.rs`'s `CLOUDFLARED_SHA256`, checked on the path the agent uses |
| electron runtime (234 MB) | `/summrise-agent/electron-win32-x64.zip` → a **PINNED** upstream asset (`v33.4.11`), proxied | the pin in the route |

So a device behind the GFW needs no GitHub access, no `ELECTRON_MIRROR`, and there is no 54 MB package
of ours to keep current or to carry a redistribution duty for (cloudflared is Apache-2.0). **What was
actually missing was the FETCH**: `setup` looked only INSIDE the package, printed
`not in package (browser tools disabled)` for playwright, and for cloudflared said **nothing at all** —
and that silence is the 2026-09-23 migration measured in hours, because a device with no cloudflared
has no tunnel and a device with no tunnel is invisible to the console while `summrise status` calls it
healthy from inside. `resolveComponent()` now resolves each one — package copy first, otherwise the
host's route, `curl -fsSL` so an HTTP error is a FAILURE and not a 404 page written to disk — and the
failure branches WARN. This lands in **1.2.454**.

**`npx electron` was considered and rejected**, and the reasons are the ones that will come up again:
it is a postinstall download from GitHub releases (the host this network drops — the hand-install on
2026-09-23 needed `ELECTRON_MIRROR` for exactly that), and the launcher calls
`<shell>\node_modules\electron\dist\electron.exe` by path, so `npx` would mean rewriting the launcher
to buy a worse source.

**And the ORDER inside D6 is not the order the steps are listed in.** The relay is deployed and the
gateway is pointed at it FIRST; the file-relay routes come out of `summrise-dist` LAST. Trimming
`index/` before the relay answers would open a window with no file relay at all — and that relay is
the only sanctioned way to move bytes between these machines, so the window would be a self-inflicted
outage of the tool doing the work.

### D6 — EXECUTED 2026-09-23

Done in exactly that order, and each step verified before the next:

1. **Created** `summrise-relay-files` (its OWN bucket — sharing the CDN's would re-couple the two
   stores, and `summrise-temp-files` still feeds the playwright route on the other side), then put a
   fresh `UPLOAD_KEY` and its own `DO_AUTH` on the new worker.
2. **Deployed the relay with NO route**, so nothing could reach it: the state where a mistake costs
   nothing.
3. **Pointed the gateway at it over a SERVICE BINDING** (`RELAY` in `gateway/wrangler.jsonc`), with the
   existing URL path kept, unchanged, as the fallback — so the code is correct before AND after the
   relay exists. A same-zone `fetch()` would have been the documented failure case: the relay's
   download leg is a Route, and Routes cannot be the target of a same-zone fetch while Custom Domains
   can.
4. **Handed it the route** (`<download-host>/files/*`). A Route on the PATH takes precedence over
   `summrise-dist`'s dashboard-managed Custom Domain on the HOST — that is what makes a path-level
   handover possible without touching the hostname, the docs, the console or any device.
5. **Trimmed `summrise-dist`**: 173 lines of handlers, six test files and `claim.js` left (the index
   suite went 99 → 43; the relay's is 49), and `TempClaimDO` retired with a `deleted_classes`
   migration — the same step the gateway took for `PluginHubDO`, and safe because that DO held no
   storage. **`TEMP_FILES` stays**: this worker still streams the playwright bundle out of it.

**TWO MISTAKES, both caught by testing rather than reasoning, and both now impossible to repeat
silently.** A fresh `UPLOAD_KEY` written to the relay but NOT to the gateway answered 401 — "a fresh
shared secret is required" means both sides in one command. And the relay built its download URL from
`request.url.origin`, which through a binding is `https://summrise-relay.internal/…`, a host nothing
can resolve: `PUBLIC_BASE` is a var on the relay now, with the route's own origin as the fallback. A
unit test would not have caught the second one; a real transfer did.

**VERIFIED on the live host, twice** — once at cutover and again after the trim was deployed, so the
negative is covered too: with `summrise-dist`'s handlers gone, `/files/*` can only be the relay, and
it served the exact bytes, answered 404 to a second fetch (the one-time claim), and left every route
the CDN still owns at 200 (`/api/version`, the tgz, cloudflared, electron, playwright, the probe and
the landing).
