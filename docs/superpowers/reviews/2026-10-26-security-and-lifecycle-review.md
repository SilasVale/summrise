# Review: the Security page, test isolation and supervised restarts (rounds 18–20)

> **Scope:** `git diff 34e143b...4ff5b9c` — the Security page and `GET /denials`, the timeout tests'
> isolation fix, and the supervised panel-enrollment restart. Two independent subagents ran the
> **Standards** (AGENTS.md + Fowler baseline) and **Spec** (the round plans and the round-19/20 commit
> bodies) axes read-only; this file records what was fixed and what was deliberately left.
>
> **Dropped before merge:** a planned Processes page (`docs/superpowers/plans/2026-10-26-processes-page.md`)
> was built and then discarded. Its premise — that `system.process_list/kill` had no console surface —
> was stale: `SystemPage` has listed and killed processes with a named confirmation since `e7ac7cb`.
> Shipping a second surface would be duplication; the plan commit was reset off the branch and the
> worktree deleted. The lesson is recorded here rather than in a shipped doc: **check the premise
> against the tree before planning a surface.**

## Standards

| Finding | Disposition |
|---|---|
| **Hard:** `AuditTable`'s filtered-empty sentence stayed "No audit lines match." on Security | Fixed: `emptyFiltered` (default unchanged) and Security's "No failed authentication matches this filter."; tested |
| The token filter shows a control that cannot match when no line carries a token | Fixed: the filter is rendered only when the window has a token id; tested both ways |
| `supervised` inferred from `--release-sha`, although the digest is caller-supplied argv | Fixed: the launcher marks its child `SUMMRISE_SUPERVISED=1`, the app reads that mark (`Config.supervised`), and a config test proves absent/`0` are not supervised |
| Design-record drift: the panel-enrollment plan and the launcher manual said a manual restart was always required | Fixed: both say supervised = automatic, standalone = manual |
| `router_with`'s seven arguments repeated in four test helpers | Fixed: `Fixture::app_with(helper, restart)` is the one place; the three helpers delegate |
| The denial fixture is copied in `App.test.tsx` and `SecurityPage.test.tsx`; `AuditTable` is still named for the audit; `long_child` repeats its Unix/Windows copy shape | Deferred: each is a local test fixture or a name that still describes the table's origin; extracting or renaming would couple tests or churn imports without changing behaviour |

## Spec

| Finding | Disposition |
|---|---|
| The Security page's filtered-empty state was the audit's | Fixed (above) |
| The standalone test used a no-op hook, so an unconditional restart call would still pass | Fixed: the success test passes a recorder and asserts **0** calls |
| "Only after the link exists" was asserted after the response, not at call time | Fixed: the supervised test's hook checks `console.json` **when it runs** and records that it existed |
| The hook ran before the answer was built; the 2 s delay was the only tie to the page | Fixed order: the answer is built before the hook. The body's flush is not observable from the handler, so the two-second delay remains the stated mechanism (in the code comment), not a guarantee; a response that takes longer than that is outside this design |
| Round 19: no missing requirement | — |

## Verification

- Device: **467 tests** (+1 supervised-mark test) and 4 integration; `cargo xwin check` for
  `summrise-device` and `summrise-launch` tests; clippy `-D warnings` 0; fmt 0.
- Web: **304 tests / 30 files**; typecheck 0; bundle 736 KB and content-current.
- No conflict markers; the two axes' reports above are the basis for every disposition.
