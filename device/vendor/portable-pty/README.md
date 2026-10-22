# portable-pty, vendored and patched

This is `portable-pty` 0.8.1 from crates.io, copied from `agent/vendor-portable-pty` (the old
agent's copy) so the device workspace does not depend on a tree the two-runtime migration will
retire. It is MIT-licensed; `LICENSE.md` is the upstream license.

**Why 0.8.1 and not 0.9.0.** The ConPTY flag bug is still present in the newest published release.
`portable-pty` 0.9.0's `src/win/psuedocon.rs` passes

```rust
PSUEDOCONSOLE_INHERIT_CURSOR | PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE
```

to `CreatePseudoConsole`. `PSEUDOCONSOLE_WIN32_INPUT_MODE` (0x4) makes ConPTY deliver console output
back to the host as Win32 input records; PSReadLine treats the OSC 633 terminator as a keystroke and
renders a `>>` continuation prompt after every prompt (the report is microsoft/vscode#236841).
node-pty, which VS Code uses, passes only `INHERIT_CURSOR`. A `[patch.crates-io]` on 0.9 would have
nothing to patch to, so the patched 0.8.1 tree is the dependency.

## The two patches

1. `src/win/psuedocon.rs`: `CreatePseudoConsole` is called with `PSEUDOCONSOLE_RESIZE_QUIRK` only.
   The `PSEUDOCONSOLE_WIN32_INPUT_MODE` flag is removed and the reason is recorded at the call site.
2. `src/win/conpty.rs`: `try_clone_reader` returns a `ConPtyReader` that uses `PeekNamedPipe` before
   reading. Windows anonymous pipes cannot be put in non-blocking mode, and the ConPTY output pipe
   never reports EOF while the HPCON lives, so a plain blocking `ReadFile` would hang the reader
   thread after the shell exits. The patched reader returns `WouldBlock` when no data is buffered,
   and the capability's reader thread polls it.

`src/lib.rs` also carries a crate-root `#![allow(warnings)]` / `#![allow(clippy::all)]`: the device
workspace's gate is `-D warnings`, and these are upstream's lints (including the obsolete
`cargo-clippy` cfg feature the 0.8 tree still uses), not the device's to fix. A path dependency is
compiled by clippy with the workspace's flags, so without this the gate would fail on vendored code.

Everything else is upstream. Do not edit `src/` here; re-copy the old agent's vendor tree if the
upstream base moves.
