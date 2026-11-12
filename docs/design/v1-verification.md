# v1 verification sweep

What the Linux build host can prove about `hub/v1`, run once before the Windows handoff. HEAD at the
time of the sweep: `e61ff51` (the clippy fix below is included). Every command ran from the repository
root unless it says otherwise.

| # | Check | Command | Result |
|---|---|---|---|
| 1 | Tests, every shipped crate | `cargo test --manifest-path device/Cargo.toml -p summrise-core -p summrise-bus -p summrise-plugin-host -p summrise-console-page -p summrise-device -p summrise-launch -p summrise-mesh -p summrise-release -p summrise-console -p summrise-ui -p summrise-winsec -p summrise-winsys -p summrise-shell` | **1 487 passed, 0 failed, 4 ignored**, exit 0 |
| 2 | Clippy, same list | the same `-p` list with `--all-targets -- -D warnings` | clean, exit 0 |
| 3 | Formatting | `cargo fmt --manifest-path device/Cargo.toml --all -- --check` | clean |
| 4 | Dependency gate | `node scripts/check-architecture.mjs` | 14 packages, 22 edges, **0 upward** |
| 5 | The gate's own test | `node scripts/check-architecture.mjs --self-test` | clean accepted; `core -> bus` rejected |
| 6 | The page is not stale | `bash scripts/build-web.sh --check` | ok — the committed `dist/index.html` is the built page |
| 7 | Page tests | `cd device/console/web && npx vitest run` | 70 files, **1 182 passed** |
| 8 | Page types | `cd device/console/web && npx tsc --noEmit` | clean |
| 9 | The product loop, real binaries | `bash scripts/smoke-local.sh` (also a CI step in the device job) | **22 PASS**, `SMOKE OK` |
| 10 | The update path | `bash scripts/rehearse-release.sh --version 0.3.994 --out /tmp/v1-sweep-rehearsal` | five `ok` lines, including *the device accepts the signed document*; installer built; **PASS in 32 s** |

## What the sweep itself found

- **A real CI failure:** `device/console/src/audit.rs` had `map_or(false, …)`, which clippy rejects
  under `-D warnings`; the console lib was the only crate the CI clippy list could not finish. Fixed
  as `is_some_and` in `e61ff51`, and the full list is green.
- **One flake, not a regression:** the page suite reported 5 failures the first time it ran
  *concurrently with the cargo suite* on this box, and passed 1 182/1 182 on two clean runs. The
  failing files then were timing-sensitive; the suite is not run in parallel with the Rust build in
  CI (different jobs, different machines). Worth remembering if a page test fails alone under load.

## What this machine cannot prove

The Windows handoff:

- the four `+crt-static` exes link and the NSIS installer compiles on a Windows runner;
- a silent install/uninstall leaves the service and the tray correct;
- `summrise-shell.exe --check-window` opens `/panel` in an interactive session when the console is
  down, and the console when it is up;
- Authenticode signing of the four exes.

Linux-side proxies already exist for the parts that are portable: `cargo xwin check` and
`--target x86_64-pc-windows-gnu` build the workspace, the shell's 24 tests pass, and the rehearsal
above exercises the publish and the device's acceptance of the signed document.

## Re-running it

The ten commands are the sweep; nothing else is needed. The one prerequisite is a built workspace
(`cargo build --manifest-path device/Cargo.toml --workspace`); `smoke-local.sh` and
`rehearse-release.sh` build what they need themselves.
