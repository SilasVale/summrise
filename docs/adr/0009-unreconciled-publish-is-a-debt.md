# ADR 0009 — an unreconciled publish is a debt the next publish must answer for

Date: 2026-09-14 · Status: Adopted

## Context

`scripts/publish-release.sh --skip-reconcile` publishes to the CDN when no GitHub
release exists yet to audit against. The dual-builder audit (`scripts/lib/release-audit.sh`)
is the only check that the CDN artifact and the GitHub artifact came from the same
source, and it cannot run before the tag exists — so a first publish is legitimately
allowed to skip it, PROVIDED the audit happens afterwards.

Measured on 2026-09-14 (round 122): the newest GitHub release in `SilasVale/vale` was
**v1.2.361**, while the CDN served 1.2.361, 1.2.362, 1.2.363 and 1.2.364. Three shipped
versions had no release, no tag, and therefore no audit — and nothing anywhere recorded
it. The post-publish checklist's "[2] cut the tag via the API" step is printed text, so
no part of the flow could fail because that step never happened. `--skip-reconcile` was
not the exception; it was the only path recent releases took.

## Decision

A publish that skips the reconcile RECORDS the debt as a file, and every publish REFUSES
while that file is non-empty:

* the ledger is `docs/agents/release-reconcile.txt` — one line per version, tracked in
  git so the debt survives this machine and appears in review;
* `--skip-reconcile` appends to it (idempotently — the count of versions owing a
  reconcile must stay answerable) and the ledger rides the release commit;
* the gate runs before any build, pack or deploy and exits 1, naming the pending versions
  and both ways out;
* `--acknowledge-unreconciled` lets a run proceed anyway and ADDS to the ledger rather
  than clearing it;
* the debt is settled by the existing `--audit-only <ver>` path, which clears that version
  once the audit passes.

## Rejected options

* **Automate the tag/release creation inside the publish.** It would turn this machinery
  into a publisher of public releases — precisely the action the charter reserves for the
  user — and it would hide a manual step that exists because tag pushes intermittently
  time out on this network. Revisit only with an explicit decision on that reservation.
* **Keep the state only on the CDN (next to `version.json`).** A file that exists only
  where the publish puts it cannot refuse a publish that has not run yet.
* **Warn instead of refuse.** That is the defect being fixed: round 122 closed two checks
  in the audit itself whose failure the caller printed as success. A warning is not a gate.

## Consequences and measurements

* Externally observable metric: the number of pending versions (the ledger's non-comment
  lines) and the age of the oldest. Target 0; failure criterion a pending version older
  than one publish cycle.
* The gate fails closed: a non-empty ledger with no acknowledgement stops the publish.
* Cost: one extra flag to remember when a first publish is genuinely intended.
* **Deletion criterion**: delete this ledger, its gate and this ADR when the reconcile leg
  is CODE — a required CI follow-up that audits the artifact after the tag — rather than a
  checklist item. At that point the debt cannot accumulate, and this machinery is scaffolding.
