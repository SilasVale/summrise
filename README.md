# Summrise

The Summrise device runtime: a Windows agent that supervises itself, updates from signed releases,
and offers its capabilities to a local operator, a mesh peer, or a console session.

This repository is the clean start for the runtime that replaces the pre-1.3 `agent/`. The old tree
lives in `summrise-legacy`; nothing here depends on it.

## The shape

```
device/
  core/      pure decisions: the state document, grants, sessions, update selection, the registry
  launch/    the supervisor and the only writer of the app binary (SCM service, install, swap, repair)
  device/    the app: capabilities, local HTTP/MCP, the console channel, the mesh service
  mesh/      pinned-QUIC sessions, the broker and relay, STUN candidates
  release/   signed release documents, channels, revocation lists, the signer
  console/   the control plane: operator sessions, enrollment codes, grants, the device channel, audit
  winsec/    the Windows security helpers (protected DACLs), shared by launch and device
```

Two runtimes, one control plane:

- **`summrise-launch`** starts, restarts, updates and repairs the app. It is the only process that
  writes `<install>/summrise-device.exe`, so an update cannot race the process it replaces.
- **`summrise-device`** is the product. It owns the state document, executes tools, and serves MCP
  over loopback, the console channel, and the mesh with one authorization check.
- **`summrise-console`** brokers: it enrolls devices, signs grants offline-verifiable by the device,
  and carries operator traffic without being able to forge a device's answer.

## Build and test

Rust 1.98.1 (`rust-toolchain.toml`).

```bash
cargo test --manifest-path device/Cargo.toml --workspace
cargo fmt --manifest-path device/Cargo.toml --all -- --check
cargo clippy --manifest-path device/Cargo.toml --workspace --all-targets -- -D warnings
```

Windows is the target: the lifecycle suite only runs there (`ci.yml`'s `device-windows` job), and a
Linux `cargo test` proves the portable half. `cargo xwin check --workspace --target
x86_64-pc-windows-msvc` compiles the Windows-only paths from Linux.

## The documents

- `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` — the design.
- `docs/superpowers/specs/2026-10-21-remote-access-two-client-classes-design.md` — who may reach a device.
- `docs/superpowers/specs/2026-10-21-two-runtime-canary-runbook.md` — how to install and verify it.
- `docs/superpowers/specs/2026-10-21-phase-5-cutover-plan.md` — when the old tree goes away.

## License

See `LICENSE`.

## Where to read next

The design is a record, not a wiki page: each document states a decision and what would overturn it.

| Question | Document |
|---|---|
| why two runtimes, and what each owns | `docs/superpowers/specs/2026-10-20-the-two-runtime-architecture-design.md` |
| where the console's TLS lives | `2026-10-22-the-console-transport-decision.md` |
| why there is no desktop shell | `2026-10-22-the-desktop-shell-decision.md` |
| how a canary is run, step by step | `2026-10-21-two-runtime-canary-runbook.md` |
| what the canary actually proved | `2026-10-22-the-canary-observation-report.md` |
| can the old runtime be retired | `2026-10-22-the-cutover-verdict.md` (verdict: not yet, and what would make it yes) |
