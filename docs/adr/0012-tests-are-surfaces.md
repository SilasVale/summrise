# ADR 0012 — tests are surfaces; generated, vendored and retired trees are not

Date: 2026-09-14 · Status: Adopted

## Context

The ledger's "Never-examined surfaces" table (round 165) asks one question: **which
files has no round ever named?** `scripts/surface-coverage.mjs` is its instrument, and
the question only means something if the SCOPE is the set of files where a defect would
cost. Through round 235 that scope was ten SOURCE roots, and round 235 measured what it
left out: **154 of the repository's 415 source files, 37%, with no run ever saying so.**

Round 236 classified the 261 outside it, and the classification decided the question:

| category | files | is it a surface? |
|---|---|---|
| `gateway/test/`, `agent/tests/`, `index/test/`, the proxies' `test/`, the panel's `__tests__` | **~158** | **yes — and round 231 is why** |
| `gateway/public/code/` (the code-viewer mirror) | 41 | no — GENERATED |
| `agent/vendor-portable-pty/` | 12 | no — VENDORED |
| `agent/deploy/retired/` | 3 | no — RETIRED |
| `agent/resources/panel/` (built `panel.js`) | 3 | no — BUILD OUTPUT |

**The bulk of the omission was the one category this loop had already proved matters.**
Round 231 found 69 passing `api-relay` tests that CI ran none of, and the finding was
worth a round precisely because a test file nothing exercises is a surface where a
defect hides. Worse: `agent/tests/` contains **this loop's own instruments** —
`ledger_head.rs`, `adr_allocation.rs`, `gateway_code_contract.rs`, `module_map.rs` —
and `gateway/test/` contains the 872 tests the journal quotes every round. Neither had
ever been in scope, so the instrument had never counted the instruments.

## Decision

**Tests are surfaces and join the scope.** The directories added in round 236 are the
four test trees, the panel SPA's `src/`, `gateway/ui/`, `agent/vale-agent-npm/`,
`agent/deploy/`, `agent/scripts/` and `agent/vale-desktop-electron/`.

**Generated, vendored, retired and build-output trees are excluded, each with its
reason stated in the tool.** The exclusions are printed in every run, so "not in scope"
is a decision a reader can see rather than an omission they have to guess at — which is
round 235's lesson applied to itself.

## Rejected options

* **Keep the scope source-only.** Rejected because round 231 refutes it on this
  repository's own evidence: the most valuable finding of the last ten rounds was about
  tests nobody ran. A scope that excludes them cannot find the next one.
* **Include the generated mirror (`gateway/public/code/`).** Rejected because its
  obligation is already CHECKED and stronger than naming: round 211's test asserts the
  mirror matches what `src/` would publish, and round 219 measured that an edit to
  `gateway/src` trips it. Counting 41 generated copies would inflate the never-named
  list with files whose content no round should ever decide.
* **Include vendored third-party source.** Rejected because the round-165 question is
  about surfaces this project AUTHORS; a defect in `portable-pty` is upstream's, and
  naming it here would neither find nor fix it.
* **Exclude the panel SPA because it is "just UI".** Rejected — it is 63 files of
  TypeScript with 29 test files of its own, and `agent/CLAUDE.md` already carries a
  panel-rendering audit whose whole lesson was that hand-built galleries verify only the
  CSS you were thinking about.

## Consequences and measurements

* **Coverage: 355 of 358 non-excluded repository files (99%)**, printed by every run
  alongside `NOT in scope, by top-level directory` — so the denominator travels with the
  numerator and the two can never be read apart.
* **The never-named count moves 8 → 110**, which is the measurement doing its job: the
  scope now covers what the question is about, and 110 files have never been named by
  any round — dominated by the panel SPA's `hooks/`, `lib/` and `styles/`, and by
  `agent/deploy/`.
* **Failure criterion**: a file that IS a surface for round 165's question sitting
  silently outside the scope. That is exactly what round 235 found, and the ratio plus
  the printed exclusion list are what make it visible now.
* **Deletion criterion**: an `EXCLUDED` entry is removed when its reason stops holding —
  if the code-viewer mirror stops being generated, it becomes an authored surface and
  belongs in the scope; if `deploy/retired/` returns to service, likewise. The reason
  string is the criterion, which is why each entry carries one.
* **Cost, stated honestly**: 110 un-named files is a longer list than 8, and it is
  correct rather than alarming. Round 165's table closed its population item on a scope
  that could not have seen them; this ADR keeps that closure from being read as coverage
  of a set it never contained.
