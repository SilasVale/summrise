# Iteration loop — protocol

The standing mandate is the session goal; this file is HOW it is carried out.
Read it with `docs/CHARTER.md` (what counts as success) and
`docs/agents/iteration-coverage.md` (what has been looked at, what is open)
before every round. Keep it short enough to be read every round.

## User input — the top of the queue

The user's ideas are the loop's top-priority work and its anchor against drift
(round 100: "aurora" was the loop interpreting a request rather than following
one). They arrive written in `docs/agents/ideas.md`, or said in the session — in
which case the loop APPENDS them to that inbox so they survive the round.

Each entry gets either a round of its own, or an explicit reasoned verdict in the
round log, with evidence. It is never dropped silently and never reinterpreted
into something else. `docs/CHARTER.md` is the user's alone: the loop may only
propose changes to it.

The user may pause the loop at any time, and changing direction costs nothing —
the journal plus these files are the resume anchor.

## Roles — readers may be many, the writer is exactly one

| Role | Who | Hard constraints |
|---|---|---|
| Scout | 2–4 read-only `subagent`s, one per uncovered surface | no edits, no commits, no file writes, no spawning further children |
| Verifier | a DIFFERENT agent than the finder | reproduces the finding or falsifies it; mutation test when the claim is "this guard matters" |
| Designer | one agent (or the writer) | two designs compared, `prototype` when cheap, ADR draft with rejected options |
| Writer | the main agent, holder of the goal | the only one who edits, commits, runs gates, and writes the journal |

Why the split: the writer's own tests have asserted the wrong field twice
(round 89) and mutation testing found "tests proving nothing" three times
(round 95). A finding is a CLAIM until a different agent reproduces it.

### Scout prompt contract (self-contained — a scout cannot see the session)

- the surface boundary (files/routes/tools in scope) and what is out of scope
- what counts as a finding, and the evidence bar: a repro command, a failing test, or a mutation
- output schema: `surface | file:line | severity | evidence | shape | already-known round | suggested fix`
- de-duplication: grep the journal and the ledger FIRST; a known item is reported as `already-known`, never re-reported
- stable finding IDs (`gateway F2`, `panel F1`, `memory F5` …) so rounds 20 rounds later can still refer to them

## Discovery track

1. **Uncovered surface** — confirm by grep that no round ever mentioned this file/route/tool, then fan out scouts.
2. **Promise vs implementation** — every check a docstring/README/comment claims, verified to actually run (round 95 found a documented suffix allowlist that had never executed at dial time).
3. **Pairs** — register/rename, add/update, import/export, save/load, list/get, enable/disable: one path validates, the other does not. The repo's most frequent defect shape.
4. **Red-first tests / mutation** — disable the guard, the test MUST fail; a test that cannot fail proves nothing.
5. **Live and device drift** — `cmp` deployed assets, mirror parity, authenticated real probes; the probe proves itself first (a missing `-X POST` once produced a false 404).

Priority order: credential/authorization > data loss or silent wrong answer >
promise-vs-implementation > test gap > duplication and cleanup.

De-duplication and rotation: grep before working; an open item may only be
CLOSED, never re-reported. Two consecutive rounds on one surface with no
finding means rotate.

## Design track

Authority is by **verifiability**, not ownership:

- internal structure and anything tests can cover → decide alone;
- a contract change it can verify itself (migration path, dual-accept period, rollback point, release + device regression) → do it, verify it, ship it;
- anything it cannot verify (a Windows exe it cannot run, a dark device, a published version that cannot be recalled, someone else's machine, when to close a deprecation window) → propose and wait;
- loosening a security boundary → always propose, always recorded.

Rules of engagement are `docs/solid-program.md`'s: behavior-preserving moves,
**no speculative generality (a new seam needs an existing consumer)**, respect
the no-split verdicts in `docs/ARCHITECTURE.md`.

Every structural decision: an ADR with rejected options, a **deletion
criterion**, a measurable indicator and a failure/rollback condition; update
the boundary verdict in `docs/ARCHITECTURE.md` in the same commit; a new design
must refute or explicitly supersede the older ADR.

Design signals: the same fix shape appearing a second time (= missing single
owner), a pair defect, one logic hand-written at N sites, a seam no test can
see, an incident post-mortem, field feedback.

Pull skills when they fit: `codebase-design`, `domain-modeling`, `prototype`,
`grilling`, `writing-for-agents`.

## Contract list — check before touching a promise

| Consumer | Promised behavior | How it breaks | Migration |
|---|---|---|---|
| published tgz + devices in the field | CLI verbs/args, install layout, boot-task arguments | rename or remove a verb/arg | dual-accept + rollback point + device regression |
| AI clients (`/mcp` tool surface) | tool names and parameters | rename/remove a tool | keep an alias for one release |
| console / panel / extension / Electron | route semantics, fields, path leaf names | tightening or renaming | ADR 0008-style migration + marker |
| `settings.json` holders | relay token scope | tightening the scope | ADR 0007-style dual-accept + flag |

## Round record (before ending a round)

1. `agent/AGENTS.md`: replace the `Last updated` line and append the round entry — date, round N, one-line verdict, commits, numbered points with evidence and tests, gate numbers, deploy/verify state, `STILL OPEN`.
2. `docs/agents/iteration-coverage.md`: update the surface row and open items.
3. Every 10 rounds: the design review from `docs/CHARTER.md`.

Journal prose is English; it is the only memory that survives rounds.
