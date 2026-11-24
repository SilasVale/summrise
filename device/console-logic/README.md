# console-logic — the console page's decisions, in Rust

The console page is React for rendering; its **pure decisions are Rust**, compiled to one wasm module
and called through a thin TypeScript seam (`device/console/web/src/logic.ts`). This crate is the
seam's other end: no DOM, no fetch, no clock of its own.

## What lives here

The first family is formatting: `short_id` (the fleet's 12-character id), `age` (the one "seen 3h
ago" vocabulary) and `retry_sentence` (the sentence a reconnected link gets). Each function is a
**mirror** of the TypeScript it replaced, including JS's UTF-16 `slice` and `Math.round` semantics,
because a boundary that only matches on the easy input is a guess.

## Build

```bash
./build.sh          # wasm-pack, wasm-opt -Oz, glue + bytes into the page, freshness stamp
```

The artifact is **committed**: `device/console/web/src/wasm/console_logic.js`, `.d.ts`, the `.wasm`,
and `console_logic_bg.ts` (the same bytes base64-encoded into the single HTML the console embeds).
`build.sh` requires `wasm-opt`; an unoptimized module is 33 KB against 20.9 KB, and nothing downstream
re-derives these bytes.

## The gate

`scripts/check-console-logic.mjs` needs only Node and runs in CI's page job via
`scripts/build-web.sh --check`. It re-derives the source stamp and decodes the inlined base64 back to
the committed wasm; a source edit with no rebuild, or a hand-edited base64, fails. The Rust tests
(`cargo test --manifest-path device/console-logic/Cargo.toml`) pin the JS-parity cases.
