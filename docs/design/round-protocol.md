# Round protocol (long product-polish goal)

One round = one backlog item delivered end to end. The session is the **controller**: it plans and
dispatches; it does not write feature code itself.

## 1. Plan (controller, before any code)

- Pick the top item from `docs/design/product-backlog.md`.
- Write `docs/superpowers/plans/round-NNN-<slug>.md` in the writing-plans format: goal, files,
  interfaces, tasks; each task has the failing test, the run that proves it fails, the implementation
  step, the run that proves it passes, and the commit.
- State the proof: which screenshot or command output will show the item working on 9VH.

## 2. Dispatch (controller)

- One fresh **implementer** subagent per task, given the task brief, the interfaces from earlier
  tasks, and the report contract; never parallel implementers.
- After each task, one **reviewer** subagent gets the brief, the report and the diff; it returns spec
  compliance and quality verdicts. The controller fixes nothing itself: findings go back to the
  implementer or a fresh fix subagent.
- At the end of the round, one whole-round reviewer on the most capable available model.
- Model choice is explicit in every dispatch: cheap for transcription, standard for integration,
  strongest for the final review.

## 3. Integrate and prove (controller)

- Keep `node scripts/check-architecture.mjs` and the existing suites green.
- At most one deploy to 9VH per round; screenshots only when pixels changed. Evidence goes in
  `docs/design/evidence/`.
- Ledger entry in `docs/design/program-ledger.md`: what changed, how it was proven, what stayed open.
- Update the backlog (done / dropped / reprioritised), commit, and push through
  `scripts/push-via-relay.sh`.

## Every tenth round: pruning round

No new features. Delete or simplify what does not earn its place, reorder the backlog, and edit the
goal objective if the product has moved. Fewer, better surfaces beat more surface.

## Quality bar

Judged like a consumer product, not a debug console: clarity, consistency, restraint, accessibility
(contrast, focus, keyboard), honest empty/loading/error states, fast first paint, no dead ends.
A weak feature is removed, not decorated.
