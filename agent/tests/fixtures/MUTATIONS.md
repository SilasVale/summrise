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

