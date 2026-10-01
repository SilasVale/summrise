# What must break when you change these fixtures

JSON has no comment syntax, so the three fixtures in this directory cannot carry their own mutation proofs the way every
other gate in this repository does (landing 4b moved those into the file each one names). **Read the row before you rename a
field**: a fixture field is read by BOTH ends, and the proof is that each end notices on its own.

## `agent/tests/fixtures/approval-grants.json`

**MUTATION:** rename a member the panel mirror reads

**RESULT:** both sides fail

## `agent/tests/fixtures/embedded-bridge.json`

**MUTATION:** rename `fwd` to `forward`

**RESULT:** shell + panel fail

## `agent/tests/fixtures/session-row.json`

**MUTATION:** rename `idle_ms` to `idleMs`

**RESULT:** device + panel fail


## `gateway/wasm/fixtures/stream-corpus.json`

NOT IN THIS DIRECTORY, and it is here because the rule is about the FILE TYPE rather than the folder: the streaming
half's corpus is JSON, so it cannot carry its own proof either. It is the expectation set for
`gateway/wasm/src/stream.rs`'s `the_typescript_corpus`, and every case in it was produced by the SHIPPING TypeScript
class (`gateway/wasm/oracle.mjs` drives it).

**MUTATION:** drop one character from one case's `expected.events` — `message_start` → `message_star` in the case named
`plain text, stop`.

**RESULT:** measured, `cargo test --manifest-path gateway/wasm/Cargo.toml`:
`plain text, stop: the two streams part at byte 19`, then the two tails from the first differing byte — the Rust side
saying `t\ndata: {\"type\":\"message_start\"...` and the fixture saying `\ndata: {\"type\":\"message_start\"...`. The byte
is reported rather than only the verdict, because a stream that parts at 19 and one that parts at 2,000 are different
findings.
