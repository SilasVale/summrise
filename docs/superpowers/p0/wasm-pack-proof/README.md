# wasm-pack, working in this repository

P0's third deliverable, and **a proof of the toolchain only**. Nothing here is imported by the panel,
the console, or any worker; `scripts/build.sh` does not know this directory exists. It exists so that
"Rust compiles to wasm here" is a measurement instead of a belief, and it can be deleted the moment
P2 has real crates.

## The command

```bash
cd docs/superpowers/p0/wasm-pack-proof
WASM_OPT=$(command -v wasm-opt) ./build.sh
```

which runs, in full:

```bash
wasm-pack build --target web --no-opt
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
  pkg/wasm_pack_proof_bg.wasm -o pkg/wasm_pack_proof_bg.wasm
```

**Prerequisites, and where each one came from on this box** (none of them was present at the start):

| tool | version | how it got here |
|---|---|---|
| `wasm-pack` | 0.15.0 | `cargo install wasm-pack --locked` — **crates.io works; GitHub releases do not** |
| `wasm-bindgen` | 0.2.129 | already installed; the crate pins `=0.2.129` so wasm-pack uses it |
| `wasm-opt` | binaryen 132 | `npm i binaryen` — wasm-pack's own copy cannot be fetched (below) |
| `wasm32-unknown-unknown` | — | already installed for 1.98.1; **now declared** in `rust-toolchain.toml` here |

## The numbers, measured 2026-09-28

| file | raw | gzipped |
|---|---|---|
| `wasm_pack_proof_bg.wasm` as emitted by wasm-pack | 22,800 | 8,894 |
| `wasm_pack_proof.js` (the `--target web` glue) | 6,960 | 2,089 |
| `wasm_pack_proof_bg.wasm` after `wasm-opt -Oz` | 15,662 | 7,016 |

**And it is importable, which is the part a build log cannot tell you.** Loaded in a real Chromium
from the emitted glue, `add(19, 23)` returned `42` and `echo("hello")` returned `"wasm:hello"`, with
no page errors and these exports: `add`, `default`, `echo`, `initSync`.

## Two things this cost, both of them worth knowing before P2

### `wasm-pack build` alone HANGS on this box, and it is not the compile

Plain `wasm-pack build --target web` compiled the crate in **8 seconds** and then sat for **14 minutes**
holding `~/.cache/.wasm-pack/.wasm-opt-*.lock`. It was fetching binaryen from
`https://github.com/WebAssembly/binaryen/releases/download/version_117/…`, and GitHub releases are
unusable here — measured the same hour, a 20 MB asset was still crawling after 25 minutes and never
finished. `strings ~/.cargo/bin/wasm-pack | grep binaryen` shows that URL hardcoded and **there is no
`WASM_PACK_*` environment variable** to point it at a local binary.

So the working form passes `--no-opt` and runs wasm-opt as a separate step. `--mode no-install` is the
other escape hatch, but it is not needed: wasm-pack found the `wasm-bindgen` on `PATH` (because the
crate pins the version that is installed) and never tried to download one.

### The two wasm-opt flags are a property of the CODE, not of the toolchain

The spike's note says *"wasm-opt 132 refuses Rust 1.98's output without `--enable-bulk-memory
--enable-nontrapping-float-to-int`"*. **That is true of the spike's module and false of this one**,
and the difference is measurable rather than a matter of opinion:

| module | `wasm-opt -Oz` with no flags |
|---|---|
| the spike's `spike_rust_landing_bg.wasm` | **refused** — `[wasm-validator error in function 1] unexpected false: memory.copy operations require bulk memory operations [--enable-bulk-memory-opt]` |
| this crate's `wasm_pack_proof_bg.wasm` | **accepted**, rc=0, empty output, byte-identical to the flagged run (15,662 both ways) |

Both modules declare the same target features — `wasm-opt --print-features` reports
`--enable-nontrapping-float-to-int` and `--enable-bulk-memory` for each. What differs is whether the
code *contains* a `memory.copy`: the spike's does (it moves strings and a particle buffer), this
crate's does not (`add` is an `i32` add, `echo` formats five bytes).

**The operating consequence is the opposite of "these flags are for big crates":** whether you need
them depends on what the code happens to compile to, which changes with every edit, and a build that
omits them passes until the day someone adds a `String`. Pass them unconditionally — proven harmless
when unneeded, and required when not.
