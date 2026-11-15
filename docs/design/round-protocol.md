# Round protocol (for the long product-polish goal)

One round = one backlog item delivered end to end. A round never ends with code that has not been
planned, tested, verified on 9VH when it is visible there, recorded and pushed.

## Before touching code

1. Pick the top item from `docs/design/product-backlog.md`.
2. Write the plan to `docs/superpowers/plans/round-NNN-<slug>.md` in the writing-plans format:
   goal, files, interfaces, tasks; every task has a failing test first, the run that proves it fails,
   the implementation, the run that proves it passes, and the commit.
3. If the item is visual, say in the plan exactly which screenshot will prove it and where it lands.

## While executing

- TDD: test first, watch it fail, minimal implementation, watch it pass.
- Keep `node scripts/check-architecture.mjs` green and the existing suites green.
- One deploy to 9VH per round unless the plan says why; screenshots only when pixels changed.
- If a step turns out wrong, fix the plan first, then the code.

## After

- Evidence in `docs/design/evidence/` (screenshot or command output) when the item is observable.
- `docs/design/program-ledger.md` entry naming what changed, how it was proven, what stayed open.
- Update the backlog (done / dropped / reprioritised) and commit; push through `scripts/push-via-relay.sh`.

## Every tenth round: pruning round

No new features. Delete or simplify what does not earn its place, reorder the backlog, and edit the
goal objective if the product has moved. Fewer, better surfaces beat more surface.

## Quality bar

Judged like a consumer product, not a debug console: clarity, consistency, restraint, accessibility
(contrast, focus, keyboard), honest empty/loading/error states, fast first paint, no dead ends.
A weak feature is removed, not decorated.
