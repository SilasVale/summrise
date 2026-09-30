# The workspace registry: one place that knows which host a path lives on

**Date:** 2026-09-30 · **Status:** design, awaiting implementation · **Path:** architectural (brainstorming skill)

> **THIS FILE IS A DESIGN ARTIFACT, NOT AN APPARATUS.** It exists to be implemented, and it is DELETED when the
> implementation lands — its durable content moves to `CONTEXT.md` and to the commit that implements it. Nothing
> appends to it.

---

## 1 · Why

The operator asked, in their words: *"我要的是一个 remote ssh 可以添加到工作区的功能"* — a remote SSH workspace
they can **add from the UI**.

**The capability exists. The way to add one does not.** Measured on `desktop-14rjcr8`, 2026-09-30:

| what | state |
|---|---|
| the remote-workspace seam | **works end to end**: a model turn asked to run `uname -a` in a DSH session answered with the Linux kernel string, `toolMs 2253`, through `bash-sandbox → bash-local → ctx.subprocess → /api/workspace/exec → SSH → the staged helper` |
| the shim's connection | **pinned to ONE host** in the profile's config (`agent/summrise-workspace-dsh`). Every workspace added lands on that host, whatever its path says |
| DSH's own "add workspace" | a **local directory picker** (`dsh-host-directory-picker-auto` → `-native` on win32), and on this device it cannot work at all: DSH runs as a SYSTEM service in session 0, so the native folder dialog has no desktop to appear on. Measured: `directoryPicker/pick` **never answers** (no response in 15s) |
| adding a workspace today | hand-editing `dsh-home/storages/workspace.json` — which is what the Linux workspace in this deployment was, and it is not a feature |

So the gap is not the transport. It is **"which host does this path live on, and where do its credentials come
from"** — a question nothing in the system currently answers.

## 2 · Intent and success criteria

| | |
|---|---|
| **Intent** | From DSH's own UI: pick one of the agent's saved connections, type a remote path, and that path becomes a workspace whose files and commands are on **that** host. No credential is typed into DSH, and no config file is edited by hand. |
| **Success 1** | The form lists the agent's saved connections and takes a path. Nothing else. |
| **Success 2** | Two workspaces on two different hosts coexist: a call against path A reaches host A, and one against path B reaches host B. |
| **Success 3** | Credentials never leave the agent. The DSH side and the shim hold a **connection id**; the password or key stays in the agent's keychain. |
| **Success 4** | Nothing that works today breaks: a request that matches no mapping still works with the connection parameters it carries inline. |
| **Success 5** | A path with no mapping, or a mapping whose connection has been forgotten, **fails by name** — not by a 30-second timeout. |

## 3 · What already exists — measured, not assumed

| mechanism | where | evidence |
|---|---|---|
| The doors | `POST /api/workspace/exec`, `/api/workspace/fs` | `agent/src/web/mod.rs`; both verified against a real sshd from the device |
| The staged argv helper | `<install>\components\summrise-exec-argv-linux-x86_64`, staged onto the target by SFTP | the `126`-on-bad-cwd and exit-3 measurements in the merge commit `9017a865` |
| **The agent's saved connections** | `agent/src/tools/terminal/connections.rs` — `remember` / `list` / `forget`, stored at `<DataDir>\summrise-connections.json` | id is `kind:target`; the file **does not hold passwords** (its own comment records why: they belong in the keychain, which `agent/src/tools/terminal/secrets.rs` owns) |
| DSH's workspace API | `@deepseek-ai/dsh-api-workspace-controller` registers `create`, `rename`, `delete`, `createDirectory`, `insertBefore`, … | so a workspace record can be created through DSH's own surface, not by writing `workspace.json` |
| **A third-party plugin can ship UI** | `@deepseek-ai/dsh-client-modules` resolves `exports["./client"]` for **any** package (`clientExportOf(pkgName, exportsField)`) | so the form can live in DSH's UI without forking DSH |
| The attach validation reaches the seam | `bf2438af` — the workspace registry patch (`agent/summrise-workspace-dsh/patches/dsh-workspace-registry.mjs`) | a POSIX workspace attaches on a Windows DSH only because of it |

## 4 · The boundary

**The agent is the only authority for two things that were previously nowhere: which connection serves a path, and
the credentials for it.** That is the whole design.

```
DSH (device)                       agent (device)                       workspace host (Linux)
  client half ── the form ──┐
  host half ──── loopback ──┴─→  /api/workspace/register|connections
                                  the mapping store  (path → connection id)
                                       │
  ctx.fs / ctx.subprocess ─ shim ──────┤  resolves: longest prefix → connection → keychain
  (paths only)                         └────────── SSH ─────────────────→ the host
```

Three consequences, each deliberate:

- **The shim stops pinning a host.** Its `host`/`user`/`port`/`keyPath` config become *optional* — the fallback for
  a request that carries them (Success 4), not the rule.
- **The plugin is two halves.** The host half runs inside DSH's Node process on the device and calls the agent over
  **loopback** (no CORS, no Cloudflare, no Access); the client half is the form and talks to its own host half
  through DSH's RPC. This is the same split the shim already uses for `ctx.fs`/`ctx.subprocess`.
- **The DSH side never learns an address or a credential.** It learns a connection **id**, and the id is only
  meaningful to the agent.

## 5 · The mapping

**The key is a path, and the match is the longest prefix over normalized absolute paths.**

| rule | why |
|---|---|
| Normalization is the shim's `normalizePath` rules — POSIX, `.` dropped, `..` popped, repeated slashes collapsed, and a **Windows-spelled absolute path** (`D:\home\…`) folded to `/home/…` | the same path arrives spelled two ways on this deployment (measured: `b3eda13d`), and a mapping store that normalized differently from the shim would answer "unknown path" for a path the shim had just resolved |
| Longest prefix wins | `/home/zhengsaisi` and `/home/zhengsaisi/summrise` may be two workspaces on two hosts; the more specific one must win |
| The path comes from the request: `path` for `/api/workspace/fs`, `cwd` for `/api/workspace/exec` | those are the only fields that name a place |
| **An `exec` with no `cwd` cannot be matched** | there is no other field that names the workspace, and guessing one would be worse than saying so: it falls back to the request's inline connection (Success 4) |
| Registration is idempotent; unregister is by path | a form that is submitted twice must not create two mappings |
| A mapping names a connection **id**, never credentials | Success 3 |

## 6 · The doors

New endpoints, all on the agent's existing authenticated HTTP surface:

| endpoint | what it does |
|---|---|
| `GET /api/workspace/connections` | the agent's saved connections (`connections::list()`), so the form can offer them |
| `GET /api/workspace/mappings` | the current path → connection table |
| `POST /api/workspace/register` | `{path, connection_id}` → stored (idempotent) |
| `POST /api/workspace/unregister` | `{path}` → removed; `false` when nothing was there, which is a fact and not an error |

The two existing doors keep their shape and gain a resolution step **before** they connect. **Their request fields
do not change meaning; `host` and `user` stop being required** — they become the inline fallback, and a request
that carries them behaves exactly as it does today:

1. match the request's path against the mapping table (longest prefix);
2. if it matches, resolve the connection by id — and if the id is gone, refuse **by name**;
3. if it does not match, use the inline parameters exactly as today;
4. if neither is present, refuse **by name** (today's "no host" refusal, unchanged).

**Refusals are named, and they are new codes rather than prose:** `workspace/unknown-path` (no mapping matched and
no inline connection was given) and `workspace/unknown-connection` (a mapping matched, its connection is gone).
Both are facts the operator can act on; a timeout is neither.

## 7 · The plugin

`agent/summrise-workspace-dsh` grows the two halves it does not have (today it is `lib/index.js` + `lib/fs.js` +
their transports — a *provider*, with no user surface at all):

| half | what it does |
|---|---|
| host (`lib/workspace-registry.js`) | calls the agent on `127.0.0.1:18080` (the endpoint it already knows, from its own config) and exposes three operations to the client half over DSH's RPC: `listConnections()`, `register(path, connectionId)`, `unregister(path)` |
| client (`client.js`, exported as `exports["./client"]`) | the form: a `<select>` of the agent's connections, a path input, Add/Remove, and the outcome said plainly (registered / refused by name / the agent is down) |

**Creating the DSH workspace record** is the third call, and it belongs to the client half: it is DSH's own
`workspace/create`, and the plugin is already inside DSH.

**When the agent is down**, the form says so — it does not render an empty list as "no connections exist", which
would be a status nobody checked (the rule this repository already carries).

## 8 · Risks

| risk | posture |
|---|---|
| The mapping store is a **new place a path is interpreted** | it normalizes with the shim's rules, and a unit test asserts the two agree on the spellings this deployment actually sees (POSIX and Windows-spelled) |
| A mapping outlives its connection (forgotten, or the device re-provisioned) | `workspace/unknown-connection`, by name, with the path in the message |
| The plugin binds to DSH's plugin API | the same binding the shim already carries, and the same answer: the pin is a build step, and the shim's transports stay importable without DSH so their logic is testable |
| A path prefix that is a lie (`/home/z` mapping while the caller means `/home/zeta`) | segment-wise prefix matching, exactly as the shim's `pathContains` already does — not `startsWith` |

## 9 · Testing

- **Agent, the store:** register/unregister idempotence, longest-prefix, segment-wise containment (`/home/z` must
  not claim `/home/zeta`), normalization agreeing with the shim's spellings, unknown-connection refusal.
- **Agent, the doors:** a request whose path matches is routed to the mapped connection **without** inline
  parameters; one that matches nothing still works inline; both refusals answer by name.
- **Shim:** the transport forwards the path and no longer requires a host; the inline fallback still works.
- **Plugin:** the form's pure logic (validation, the outcome strings) unit-tested without DSH; the client half's
  members pinned against the host half the way `embedded-bridge.json` pins the shell's bridges.
- **The device is the acceptance surface:** register a **second** path (and, if a second host is available, a second
  host), then prove from a DSH session that a command in workspace A ran on host A and one in workspace B on host B —
  the same evidence style as the `uname -a` turn, which is the only kind this deployment has ever accepted.

## 10 · Out of scope

- Entering credentials in DSH (the operator chose the agent's store: *"复用 agent 已保存的终端连接"*).
- Jump hosts, port forwarding, or anything the agent's SSH client does not already do.
- Replacing DSH's local directory picker. It stays what it is; this design adds a second way to add a workspace,
  and the picker's native half remains unusable on a session-0 service.
- The `/dsh/` reverse proxy and the panel's own workspace UI — the form lives in DSH, as chosen.
- Changing the `ctx.fs`/`ctx.subprocess` contract, or the doors' request/response shapes beyond the resolution step.
