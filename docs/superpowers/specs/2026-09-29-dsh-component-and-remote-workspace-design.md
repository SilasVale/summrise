# DSH as a managed component of the agent, and the remote workspace it opens

**Date:** 2026-09-29 · **Status:** design, awaiting implementation · **Path:** architectural (brainstorming skill)

> **THIS FILE IS A DESIGN ARTIFACT, NOT AN APPARATUS.** It exists to be implemented, and it is DELETED when the
> implementation lands — its durable content moves to `CONTEXT.md` and to the commit that implements it. Nothing
> appends to it.

---

## 1 · Why

Two problems, discovered together, and the second is the reason the first matters.

**The Linux development box lost its international egress.** Measured 2026-09-29 from `10.10.61.83`: TLS to
**every** host in this deployment's own zone, to `cloudflare.com` and to `github.com` dies *inside the handshake* — the
transcript is `TLSv1.3 (OUT), TLS handshake, Client hello (1)` followed immediately by
`OpenSSL SSL_connect: SSL_ERROR_SYSCALL`, with no ServerHello — while TCP connects fine (`connect=0.0037s` to the API
host on 443) and `api.deepseek.com` answers normally. The same Cloudflare edge IP with a different SNI
(`example.com`) fails identically, so the interference is **not** SNI-specific; it is the Cloudflare edge IPs on 443.
Consequences measured the same day: `git push`, `npm`, `wrangler` and `web_fetch` are all dead from that box, and the
**summrise gateway MCP was down** (`terminal_open` answered `fetch failed`) — the operator's own DSH had lost its
terminal/browser/device surface.

**The operator's Windows machine has clean egress and is already a summrise device.** Measured from that device on the
same day: the API host **200**, the gateway MCP **302**, `github.com` **200**, `cloudflare.com` **301**,
`api.deepseek.com` **401**. It runs the summrise agent and the desktop shell, and — measured by `ipconfig` — is
`172.16.0.177` on the office LAN plus `192.168.1.100` on the device network.

**The operator's requirement, in their words:** *"windows 安装 summrise agent 就可以使用 dsh remote 功能"* — install
the agent on Windows and the DSH remote-workspace capability is simply there, with the **Linux box's working
environment** as the workspace. Not a second product to install, not a second set of credentials.

**What this spec is not.** It is not "add remote SSH" — that capability exists and works today (§8.3). It is the two
things that do **not** exist: DSH delivered and supervised *by the agent*, and a workspace backend that reaches
machines a plain SSH client cannot.

## 2 · Intent and success criteria

| | |
|---|---|
| **Intent** | `summrise setup` on a Windows machine yields a working DSH whose workspace can be another machine's working environment, with no second install, no second credential store, and no hand-edited profile. |
| **Success 1** | One install. After `SummriseAgent-Setup-<ver>.exe` plus `summrise.cmd setup`, DSH is present, pinned, running and reachable at `http://127.0.0.1:18080/dsh/`. No `npm i -g` by the operator. |
| **Success 2** | The DSH version is **chosen by summrise**, not discovered. `index/components.json` carries its pin, and `setup` refuses a mismatch the way it already does for cloudflared and electron. |
| **Success 3** | The workspace is the **remote** machine: the built-in `read`/`edit`/`grep`/`glob` tools and the file tree operate on the remote filesystem, and `bash` executes **on the remote host** with the remote toolchain. |
| **Success 4** | DSH's own crash cannot take the device's hands with it. The agent keeps serving `/panel/`, `/api/*`, terminal, browser and monitor while DSH is down or restarting. |
| **Success 5** | All implementation logic is **Rust**. The only TypeScript is a generated delegation shim inside DSH's process, and regenerating it against a new DSH release is one command (§6.8). |
| **Success 6** | A DSH upgrade does not require editing JavaScript by hand — it requires re-running the generator and disposing of what it reports. |

## 3 · What already exists — measured, not assumed

This spec is cheap because six of the mechanisms it needs are already in the tree. Each row names the artifact.

| mechanism | where | evidence |
|---|---|---|
| A self-contained Windows installer | `scripts/build-installer.sh`, `agent/deploy/summrise-setup.nsi` | NSIS; the pinned `summrise-agent-<ver>.tgz` is `File`-ed **into** the installer so it installs with no network; signed (sign + verify); uninstall removes the `SummriseAgent`/`SummriseDesktop` tasks and both exes |
| **A portable Node runtime, already shipped** | `agent/deploy/summrise-online-setup.ps1:144` — *"Layout v2: portable node lives at `components\node`"*; `agent/src/paths.rs:333` lists `"node"` and `"npm-global"` | the install carries Node LTS win-x64 **and** an npm global prefix. DSH needs no new runtime |
| Component fetch + verify + stage | `resolveComponent()`, component names in `agent/src/paths.rs` (`cloudflared.exe`, `playwright`, `node`, `npm-global`) | `curl -fsSL` (an HTTP error is a failure, not a 404 page on disk), sha256 pinned in `index/components.json` |
| Supervised long-lived process | the `SummriseDesktop` scheduled task + `ensure-desktop.ps1` / `desktop-pulse.vbs` | the desktop shell is already a supervised second process |
| **A hard rule about how to spawn it** | `agent/summrise-desktop-electron/src/main.js` | *"No `spawn()` of `summrise-agent.exe` from JS — that was the d1 Chrome-OOM root cause"*; *"`schtasks /run` — the ONLY sanctioned spawn path"* |
| Embedding a foreign UI | the desktop shell's `WebContentsView` + `src/url-policy.js` | used today for browser sessions, with a loopback-origin veto (control API on 9444) and a foreign-origin veto on **reads** too |
| The `ctx.fs` seam | `@deepseek-ai/dsh-fs` 0.2.0-rc.1 | *"Mounting any backend populates `ctx.fs`; swapping backends changes nothing for the policy plugin, the tools, or the tool schemas."* |
| **Discovery tools that bypass `ctx.fs`** | `@deepseek-ai/dsh-tool-fs-search` 0.2.0-rc.1 | `glob`/`grep` *"execute as ordinary foreground spawns through `ctx.subprocess`"*, against a packaged ripgrep — so the workspace's correctness depends on the **subprocess** seam as much as on `ctx.fs` (§6.6) |

**And one thing does not exist — though the manifest says otherwise.** The agent's HTTP layer is `axum 0.8` +
`tower 0.5` + `hyper 1` (`features = ["http1", "server"]`), and its browser streaming is **SSE only**
(`web/sse.rs`). There is **no WebSocket anywhere in the crate**: no `Sec-WebSocket-*` handling, no `/ws` route, and
`hyper` and `sha1` are declared as **direct dependencies with no reference in any `.rs` file of the crate**.

**The trap is the comment sitting above them**, which reads: *"round-137 Plan C: the hand-rolled WebSocket upgrade
for `/api/browser/ws` needs to compute `Sec-WebSocket-Accept` (SHA-1); hyper is explicitly introduced to use
`upgrade::on` and get the raw IO after the upgrade"*. Measured 2026-09-29: `/api/browser/ws` is **not** in the route
table (the browser routes are — sixteen of them), `browser/ws` never appears in this repository's git history, and
neither `hyper` nor `sha1` is referenced by any Rust source. **An implementer who reads that comment will believe the
mechanism already exists.** It does not.

DSH's web client talks to its server over **WebSocket** (the operator's own profile carries
`websocketHeartbeatIntervalMs` for `typert-gateway`, and `dsh-client-connection` is that link), so serving DSH's UI
from the agent's address means building it. That is §5.4, and it is the single largest new piece in deliverable A.

## 4 · Decomposition

Two independently landable deliverables. They do not depend on each other: B works against a hand-installed DSH, and
A is useful with no workspace at all.

| | deliverable | what it buys | new work |
|---|---|---|---|
| **A** | **DSH as a staged component** — install, pin, supervise, serve | one install; a version summrise chooses; a UI inside summrise | a component entry, a scheduled task, and the **WebSocket passthrough** |
| **B** | **The remote workspace** — a Rust fs/subprocess backend in the agent, plus a generated TS shim in DSH | the workspace *is* the remote machine | an exec-and-capture primitive over SSH (none exists), a staged argv-transparent helper on the target (§6.6), the `ctx.fs` semantics, the generator |

Each gets its own plan. They may land in either order. **B's first step is the subprocess seam, not `ctx.fs`** — §7
measured why.

## 5 · A · DSH as a staged component

### 5.1 The component

DSH joins `node` / `npm-global` / `cloudflared.exe` / `playwright` as a component under `<install>\components\`:
fetched by `resolveComponent()`, verified against a sha256 pinned in `index/components.json`, staged, and reported
honestly when a release carries no pin. The portable Node already in `components\node` is its runtime — **no new
runtime, and no second Node on the machine**.

### 5.2 The version is summrise's decision

This is the point of the deliverable, not a side effect. Taking over `ctx.fs` (§6) binds a backend to a DSH release
— measured on the community plugin that does the same thing, whose package carries
`"compatibility":{"dshReleases":{"0.1.7-rc.1":"compatible"}}` and whose README says *"Other releases are untested."*
If summrise stages DSH, the binding stops being an external constraint and becomes a version we pin and test.

### 5.3 Supervision

DSH runs as **its own process**, started and watched through the sanctioned path — a scheduled task, exactly as the
desktop shell is. Not embedded in the agent, and not spawned from JS.

**Why not embedded — measured, and it is a design position, not a preference.** The agent is a 17.8 MB Rust binary
with **no JS runtime** (no v8/deno/quickjs/boa/napi/neon in `agent/Cargo.toml`) and **no LLM code at all** (no
anthropic/openai/chat-completions, no API key, no model config). Its own plugin documentation states what it is:
`runs` — *"RUN identity for the AI's work on this device … the caller DECLARES it"*; `memory` — *"device-local
knowledge base shared across AI clients. AI clients (Claude Code / DSH / the desktop shell) …"*. The agent is the
**hands**, deliberately client-agnostic. Embedding one AI client into it would trade that position for a release
coupling with a 0.2.0-rc.1.

### 5.4 The `/dsh/` reverse proxy, and the WebSocket it must carry

The agent already serves `/panel/` and `/desktop/`. A new `/dsh/` prefix proxies to the local DSH port, so the UI is
reachable at the agent's own address — locally and, through the existing tunnel, remotely.

**The new capability is WebSocket passthrough.** The agent's HTTP stack is http1-only with no `ws` feature, its
streaming is SSE, and — per §3 — the crate contains no WebSocket code at all, despite a manifest comment claiming
otherwise. Proxying DSH requires bridging `Upgrade: websocket` in both directions, plus three smaller obligations:
DSH's SPA must be served under a base path, SSE must not be buffered, and **DSH's own browser auth must be satisfied
by the proxy** — this deployment disables it because Cloudflare Access stands in front
(`enableBrowserAuth: false`), and a Windows machine has no Cloudflare Access, so the agent holds the token.

Two ways to build it, and the choice belongs to the plan, not here: enable axum's `ws` feature (a new dependency
edge on a hand-rolled dispatcher), or take the path the stale comment intended and drive `hyper`'s upgrade directly.
The second reuses two dependencies already declared and currently referenced by nothing, which is worth weighing
against the first's convenience.

**Why not an iframe pointing at `127.0.0.1:<port>`:** inside a page viewed **remotely**, `127.0.0.1` is the
*viewer's* loopback, not the device's. It fails silently and in the wrong direction. Same-origin `/dsh/` is the only
form that holds in both cases. The agent sets no CSP and no `X-Frame-Options` (measured), so nothing on our side
blocks framing.

### 5.5 The cheap half of the UI, which can land first

The desktop shell opens DSH's own address in a `WebContentsView` — a separate web contents, not a frame, so it needs
**no WebSocket passthrough at all**. The mechanism exists and is already used for browser sessions. If seeing DSH
inside summrise matters sooner than the panel does, this is the piece to land first.

## 6 · B · The remote workspace

### 6.1 The boundary, stated once

**Rust owns every operation. TypeScript owns one delegation layer, and that layer is generated.**

`ctx.fs` is a TypeScript service class inside DSH's Node process. To populate `ctx.fs`, something in that process
must construct a TS object — this is DSH's process model, not a language preference, and it is the only place a
non-Rust file is unavoidable. Everything that does work lives in the agent.

```
DSH (Windows)
  └─ generated TS shim (~13 delegations)      ← the only non-Rust file
       └─ loopback HTTP
            └─ summrise agent (Rust)          ← all logic
                 ├─ fs: resolve/stat/lstat/read*/listDir/writeText/editText/watch
                 ├─ version guards (sha256) + atomic writes (temp + rename)
                 └─ shell: remote execution over the existing terminal machinery
                      └─ SSH → the workspace host
```

### 6.2 Why the backend is the agent, not the shim

A shim that speaks SSH itself (which is what the existing community plugins do) cannot reach a host the DSH machine
cannot route to. Putting the backend in the agent means the same code path serves a directly-reachable host and one
behind the tunnel — one implementation, not two. It also means **credentials live in one store** (the agent's secret
store, which the terminal plugin already uses) rather than being duplicated into a plugin config.

### 6.3 The `ctx.fs` contract, method by method

The seam is 13 abstract methods. Mapped to what the agent already has:

**Two rules apply to every row, and they are not optional — §7 measured 32 ms per round trip.** First, **fold**: one
RPC serves a whole tool call (resolve + stat + read together), rather than one RPC per seam method. Second, **cache**:
resolved targets are memoized by path, so a repeat operation on the same file skips resolution entirely. The seam
grants both explicitly — *"folding or caching resolution is left to such a backend."* A backend that maps one RPC to
each method below is correct and unusable.

| method | how |
|---|---|
| `resolve(path, opts)` | normalize + `realpath`. **Target identity is the resolved absolute path**, which is what makes aliases agree on one `targetKey` |
| `stat` / `lstat` / `listDir` | SFTP directly; `lstat` must not follow the final symlink |
| `readText` / `streamText` | SFTP ranged reads + our own cross-chunk UTF-8 decoding and binary rejection |
| `readBytes` / `readByteRange` | SFTP ranged reads. `maxBytes` is enforced from `stat` **before** reading — never by buffering the whole file and truncating |
| `writeText` / `editText` | write temp + `rename` (atomic publication), guarded by a sha256 version |
| `processPath` | the remote absolute path — consistent with §6.6, where commands also run remotely |
| `fileUrl` / `contains` | `file:///…`; prefix comparison over normalized paths |
| `watch` | **degraded: polling** (§6.7) |

**Error codes are exact and non-negotiable.** The seam requires callers to branch on the code and never on message
text: `FS_NOT_FOUND`, `FS_STALE_VERSION`, `FS_AMBIGUOUS_EDIT`, `FS_NOT_TEXT`, `FS_TOO_LARGE`. The Rust side maps its
own errors onto these and invents none.

**Two seam constraints worth stating before someone rediscovers them:** the contract is **text-only for mutations**
(binary writes are out of contract), and there is **no delete, rename or copy** — `listDir` is one level, and
recursion, globbing and search are out of scope.

### 6.4 The version guard has a race, and the design must close it

`editText` reads, hashes, matches and writes. Between the hash and the write another writer can land. The guard is
only real if the agent **serializes mutations per target path** (or takes a lock) — otherwise `FS_STALE_VERSION` is
decoration. This is a design requirement, not an optimization.

### 6.5 What "true workspace" is bought with

The stock providers must be disabled — `fs-sandbox`, `bash-sandbox`, `pwsh-sandbox`, `subprocess` — because the host
plane holds one implementation per service. This is the inherent cost of taking over `ctx.fs`, and the community
plugin that routes these seams does exactly the same thing. It is recorded here so it is not discovered as a
surprise.

### 6.6 The subprocess seam is the load-bearing one — and it carries more than `bash`

A workspace where `read` reaches the remote host but `bash` runs locally is **not** a working environment: the code
would be the remote machine's and the compiler would be the local one. `cargo build`, `npm test` and `git push` must
execute on the remote host.

**And `bash` is not the only consumer of that seam.** Measured 2026-09-29 in `@deepseek-ai/dsh-tool-fs-search`
0.2.0-rc.1 — the package behind `glob` and `grep`:

> *"Both tools execute as ordinary foreground spawns through **`ctx.subprocess`** — never `ctx.shell`, never
> `ctx.shell.start()`, never a model-visible background task."* — and its `inject` list contains `"subprocess"`.

So the model's discovery tools **do not go through `ctx.fs` at all**: they spawn a **packaged ripgrep** binary
(`@vscode/ripgrep`, or a `-rg.exe` sidecar beside the executable). This is why a filesystem-only remote backend
cannot work — and, per §7, it is also what makes remote search *cheap*: ripgrep on the target host walks a local
filesystem, and not one directory listing crosses the network.

**The requirement this creates — which no amount of care with `ctx.fs` would have surfaced.** The spawn passes an
**absolute path resolved on the DSH host** (`resolveRgPath()`: `process.execPath`'s `-rg.exe` sidecar, or
`@vscode/ripgrep`'s `rgPath`, e.g. a Windows `…\rg.exe`). A remote subprocess backend that forwards that argv
verbatim asks a Linux host to execute a Windows path. The backend must therefore **resolve the ripgrep binary on the
target side** — a staged, sha256-verified `rg` (the component mechanism of §5.1 already does fetch + verify) or the
host's own — and substitute it while leaving every other argv element untouched. The tool's own hardening
(`--no-config` is prepended precisely because the spawn is unconfined) must survive the substitution.

**Where the substitution happens is not where it looks like it should.** The seam has a `resolveExecutable(command)`
method, and the obvious design is to implement it on the target. Measured 2026-09-29: **`dsh-tool-fs-search` never
calls it** (`grep -c resolveExecutable` → 0). It calls the package's own `resolveRgPath()` and puts the result
straight into `argv[0]`. Its callers are `dsh-api-terminal-controller`, `dsh-host-open-in-app`,
`dsh-ptc-runtime-node` and `dsh-workspace-changes` — not the search tools. So the backend must do the substitution
**inside `spawn`, on `argv[0]`, when it is a host-absolute path**, and cannot delegate it to `resolveExecutable`.

**And `spawn` cannot be forwarded over SSH as an argv vector — SSH's `exec` request takes a string.** The seam is
explicit that no shell layer exists (*"every model value an unquoted argv element; no shell layer exists"*), so
quoting an argv into a command line would reintroduce exactly the hazard the seam was built to remove, on values the
model chose. The correct shape is an **argv-transparent transport**: the agent stages a small helper on the target
(length-prefixed fields on stdin, `execve` on the far side) and the SSH `exec` string carries only that helper's path.
This is a **new artifact on the target**, staged and sha256-verified by the same component mechanism as §5.1 — not a
re-use of existing machinery.

**The working directory is one of those fields, and it is easy to miss.** `spawn` carries a `cwd`, and it is as
caller-supplied as the argv: a workspace path may hold a space, a quote or a `$(…)`. There is nowhere else to put it —
the exec string is handed to the login shell, so a `cd` there is the same hazard wearing a different hat — which is why
the helper `chdir`s and exits `126` when it cannot, distinctly from the target's own status. **"The command never ran"
and "the command ran and failed" are different answers**, and a caller that cannot tell them apart reports the first as
the second. Implemented and verified in `9d7e4655`.

**This also corrects an earlier assumption in this spec.** The agent's `SshSession` exposes `connect`,
`sftp_session` and `open_shell` — **there is no exec-and-capture primitive**, because the agent's SSH exists for
interactive terminals, not for one-shot commands with a collected exit status. So the execution half is genuinely new
work: a session channel, separated stdout/stderr, an exit status, cancellation, and the helper above. It remains the
**first** thing B needs — it just is not wiring.

### 6.7 `watch`, honestly degraded

The seam's `watch(target, changed, signal)` reports invalidations. There is no ready-made remote equivalent. The
first implementation **polls** and says so — the interface is identical, so a later `inotify`-over-SSH
implementation is a swap, not a rewrite. A polling backend that is documented as polling is honest; one presented as
event-driven is not.

### 6.8 The shim is generated, and that is the version strategy

The shim is generated from `dsh-fs`'s type definitions. A DSH upgrade therefore produces a compile error at a known
place rather than a silent behavioural drift, and the maintenance procedure is *re-run the generator, dispose of what
it reports* — which is Success 6. Hand-writing this file would convert the version binding from a build step into a
permanent chore.

## 7 · The measurement that decides B's shape — TAKEN 2026-09-29

**Measured: RTT `desktop-14rjcr8` → `10.10.61.83` is 32 ms.** `ping -n 10`: 10/10 received, min 32 / max 35 /
average **32 ms**, 0 % loss. It is a real routed path and not an overlay artifact — `tracert -d -h 4` puts the first
hop (`172.16.0.1`) at `<1 ms`, two non-responding routers, and the target at hop 4 with the same 32 ms. **That is an
order of magnitude above what a LAN-local assumption predicts, and it is the floor cost of every remote operation.**

Priced against the work this repository actually contains — **299 directories and 1,412 files** outside
`node_modules`/`target`/`.git`, and **7,361 directories** with them:

| operation | round trips (naive) | cost at 32 ms |
|---|---|---|
| `listDir` — resolve + list | 2 | 64 ms |
| `read` — resolve + stat + read | 3 | 96 ms |
| `edit` — resolve + read + write + rename | 4–5 | 128–160 ms |
| **a glob/grep tree walk through `ctx.fs`** | 1 per directory | **299 × 32 ms ≈ 9.6 s**, and **≈ 4 min** with `node_modules`/`target` in scope |

**Verdict: B survives — but not as a naive backend, and the reason it survives is not `ctx.fs`.**

1. **`ctx.fs` must fold and cache.** The seam says so itself: *"Resolve-then-operate costs a remote backend two
   round-trips per tool call — folding or caching resolution is left to such a backend."* One RPC per **tool call**
   (resolve + stat + read, server-side) and resolved targets memoized by path. That is what turns 96 ms into 32 ms.
2. **Search must never traverse the tree over the network — and it does not have to.** §6.6 measures that
   `glob`/`grep` are **spawn-backed through `ctx.subprocess`**, so making that seam remote runs ripgrep *on the target
   host* over a local filesystem. **Remote search is fast for free — because of the subprocess seam, not `ctx.fs`.**

**The consequence is a re-weighting, not a rescope: `ctx.subprocess` is the load-bearing seam, not `ctx.fs`.** It
carries `bash`, and it carries `glob`/`grep`. §6.6 is rewritten accordingly, and it is now the first thing B needs.

**Still to measure for A:** whether DSH's SPA tolerates a `/dsh/` base path, and whether its client link is
WebSocket-only or has a fallback. Both are measured by proxying a running DSH, not by reading its code.

## 8 · Alternatives considered, and why not

**8.1 Embed DSH inside the agent process.** Rejected: it requires embedding an entire Node runtime (DSH is 82
dependencies plus Node's stdlib), it couples the agent's release cycle to a 0.2.0-rc.1, and it puts the device's
hands inside the same failure domain as the AI client. §5.3.

**8.2 A `WinFsp` filesystem in Rust, mounted as a drive letter.** Attractive because it is 100 % Rust and needs
**zero DSH code** — DSH would see an ordinary local path and every built-in tool would work. Rejected on two
measured grounds: the shell seam would still be local, so `cargo build` would use the **Windows** toolchain against
Linux sources — the exact failure §6.6 exists to prevent; and writing a correct user-mode filesystem (caching,
locking, error mapping) is harder than the generated shim it would avoid.

**8.3 Ship `dsh-remote` and stop.** This is the honest baseline and it should be *used* regardless: `flymysql/dsh-remote`
is at 0.8.23 with 102 stars, 60 published versions, pure-JS dependencies (`ssh2`, `iconv-lite`) and an install path
that already documents the Windows case. It is **not** the product, for two reasons. Its own FAQ states the
architectural limit: *"`@` lists remote files but the built-in read tool cannot open them — the harness's own file
tools see the session's **local mirror** (`$DSH_HOME/remote-workspaces/…`)"* — so it is remote *tools* plus a mirror,
not a remote *workspace*. And its transport is plain SSH, so it cannot reach a host the DSH machine cannot route to.
It also declares `^0.1.x` peers against a `0.2.0-rc.1` harness.

**8.4 The other community plugin, `lengmoXXL/dsh-remote-workspace`.** It is the right *architecture* — it routes
`ctx.fs`, `ctx.subprocess`, `ctx.shell` and `ctx.tty`, and the model sees ordinary local paths — and it is worth
reading as a reference. It is not a candidate: 0 stars, version 0.1.17, a `node-pty` native dependency plus two
binaries downloaded from GitHub Releases, four stock services to disable, and a package-level
`"compatibility":{"dshReleases":{"0.1.7-rc.1":"compatible"}}` against our 0.2.0-rc.1.

**8.5 Integrate DSH into the desktop shell's code.** Rejected: the shell is the UI layer, and the workspace path
does not pass through a window. It remains the cheapest *view* of DSH (§5.5).

**8.6 Serve DSH's UI from an iframe at the loopback port.** Rejected in §5.4 — it resolves to the wrong machine
whenever the panel is viewed remotely.

## 9 · Risks

| risk | posture |
|---|---|
| **The version binding is real.** Taking over `ctx.fs` ties B to a DSH release; the seam moved between 0.1.x and 0.2.0-rc.1, and the community plugin's pin is evidence of the cost. | A pins the version; §6.8 makes the shim generated. Neither removes the binding — they make it a build step instead of an incident. |
| **`watch` is the weakest method.** Polling is a real degradation, not a placeholder. | Declared as polling in the code and in the docs; the interface does not change when it is upgraded. |
| **The WebSocket passthrough is new surface in the agent's HTTP layer**, which is hand-rolled and http1-only. | It is the largest new piece of A; it is scoped as its own step and gated by the measurement in §7. |
| **A remote workspace is slower than a local one, always.** Measured: **32 ms per round trip**, which is 3× a naive `read` and 5× a naive `edit`. | §7 prices it and §6.3 makes folding and caching mandatory rather than optional. The number is survivable *only* because search is spawn-backed and runs remotely. |
| **The ripgrep spawn carries a host-resolved absolute path.** A remote subprocess seam that forwards it verbatim asks Linux to execute a Windows binary. | §6.6: the backend resolves `rg` on the target side and substitutes that one argv element, preserving the tool's `--no-config` hardening. Found by measurement, not by reading the seam. |
| **Credentials reach a new consumer.** The agent's secret store would now serve a workspace, not only a terminal. | Reuses the existing store and its access path; no new credential format, no second copy. |

## 10 · Testing

- **B, Rust side.** `ctx.fs` semantics are testable without DSH: version-guard rejection (`FS_STALE_VERSION`), an
  ambiguous edit (`FS_AMBIGUOUS_EDIT`), a non-UTF-8 file (`FS_NOT_TEXT`), an over-cap read (`FS_TOO_LARGE`), atomic
  publication under a concurrent writer, and the per-path serialization of §6.4. These run against a local SFTP
  target in CI and against the real Linux host on demand.
- **B, the round-trip budget, asserted rather than hoped for.** A test counts the RPCs a `read`, an `edit` and a
  `listDir` actually issue, and fails when one exceeds the folded budget of §6.3 — because at 32 ms per round trip a
  regression here is a 3× slowdown that no functional test would notice.
- **B, the ripgrep substitution.** A spawn whose argv[0] is a host-resolved `rg` path is asserted to reach the
  target as the target's `rg`, with every other argv element byte-identical — including the `--no-config` the tool
  prepends for its own safety.
- **B, shim.** The generator's output is compared against `dsh-fs`'s type definitions, so a seam change fails the
  build rather than the runtime.
- **A.** The component pin is verified the way the existing component pins are; supervision is tested by killing the
  DSH process and observing the restart **and the log line that records it** — the desktop shell's own incident
  showed that a restart nothing records is a restart nobody can diagnose.
- **A, proxy.** `/dsh/` is exercised locally *and* through the tunnel, with a WebSocket assertion (the client link
  must connect), not merely an HTTP 200 on the SPA shell.
- **The device is the acceptance surface.** `desktop-14rjcr8` is where both deliverables are finally observed.

## 11 · Out of scope

- Changing what the public deployment serves, or migrating it. This spec makes the capability available on a Windows
  machine; where the operator points their public hostname afterwards is a separate decision.
- Any change to the agent's client-agnostic position: DSH stays one client among several (§5.3).
- Binary-safe mutations, delete/rename/copy, and recursive listing — out of contract in `ctx.fs` itself (§6.3).
- Replacing `dsh-remote` for the operator's immediate use. It should be installed first, in parallel, to unblock the
  work and to produce the §7 measurement.
