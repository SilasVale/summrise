# summrise-workspace-dsh

DSH's filesystem and subprocess seams backed by a summrise agent: a workspace's files and processes live on a
**workspace host** reached over SSH, while DSH itself runs on the device. The agent owns the connection (which
saved connection serves which path) and the credentials; this package only forwards paths.

## Why this exists, when DSH ships an official one

**It does.** `@deepseek-ai/dsh-ssh`, `@deepseek-ai/dsh-fs-ssh`, `@deepseek-ai/dsh-subprocess-ssh` and
`@deepseek-ai/dsh-sandbox-ssh` are published, and the newest coherent set is **0.2.0-rc.1** — the same version
this deployment runs (their `latest` dist-tag is stale at 0.1.6-alpha.1; install by version or `@next`).

**And it cannot run here, by an explicit check rather than a documented preference.** Measured 2026-09-30 by
reading the published package and by experiment on `desktop-14rjcr8`:

```
@deepseek-ai/dsh-ssh/lib/index.js:46
	if (process.platform !== "linux" && process.platform !== "darwin") throw new Error("SSH runtime requires a POSIX client");
```

This deployment's DSH host is **Windows**, so that line throws while the provider activates. The failure is
**silent**: the same file catches its own startup rejection (`this.ready = this.start(); this.ready.catch(…)`),
so nothing reaches DSH's startup log or error log, and the stock local filesystem keeps answering `ctx.fs`. The
discriminator that proved it was a path that exists only on the Linux host:

```
workspace/create {path: "/srv"}  ->  ENOENT: no such file or directory, realpath 'D:\srv'
```

So: on a **POSIX** harness host the official stack is the better choice (multiplexed OpenSSH connection, a
TLS-secured socket per stream, helper leases and digest verification — all more than this package does). On a
Windows host it is not an option at all.

Two further things a reader of the official README needs, both read from the packages:

- `dsh-fs-ssh` declares `static inject = ["ssh", "sandboxPolicy"]`, so a profile must provide a `sandboxPolicy`
  alongside it — mounting the provider alone is not enough.
- `dsh-ssh` wants an **OpenSSH alias** (its `host` field names one), an absolute remote `node`, the installed
  `helper` and its `helperHash`.

## What is still needed on a POSIX host, and what is not

Three patches to DSH live in `patches/`, because DSH assumes a workspace is on the machine it runs on:

| patch | what it fixes | needed on a POSIX harness host? |
|---|---|---|
| `dsh-workspace-registry.mjs` | the attach validation resolves a workspace's path through the HOST's realpath instead of `ctx.fs` | **yes** — the workspace is still remote |
| `dsh-directory-picker.mjs` | the in-app directory picker lists through `node:fs` instead of `ctx.fs` | **yes** — same reason |
| `dsh-workspace-path.mjs` | `fullyQualifiedWorkspacePath` asks the HOST platform what "absolute" means | **no** — on POSIX it already answers `posix.isAbsolute` |

All three are anchor-based, idempotent by marker, and re-applied by `start-dsh.ps1` on every launch (a DSH
upgrade replaces the packages, so this is where they come back). Each REFUSES rather than half-applying.

## Verified, on the device

- A request carrying **only a path** ran `uname -a` on the workspace host and returned its kernel string, and
  `pwd` returned `/home/zhengsaisi/summrise`; an unregistered path was refused `workspace/unknown-path`; and
  unregistering the mapping made the same call change its answer.
- A file written on the Linux host was read back **byte-identically** through the workspace, and the workspace
  reported `hostname 61-83`, `id -u 1001`, `uname -r 5.4.0-60-generic` — the real machine, not an emulation.
- DSH's own "add workspace" dialog lists that host's real directories and creates a workspace from a picked
  folder (`workspace/create` → `created:true`, then `session/create` → `ok:true`).
