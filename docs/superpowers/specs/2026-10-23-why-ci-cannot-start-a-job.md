# Why CI cannot start a job — the diagnosis, not the guess

> **Status: diagnosis, 2026-10-23.** Every workflow on `SilasVale/summrise` has failed at job start
> since 2026-10-22T11:06Z: `steps: []`, `runner_id: 0`, and the run's log archive is an empty zip (22
> bytes). That shape is not a build failure — no job ever began. This document records the experiment
> that located the cause and the four ways out.

## What was ruled out

| Suspect | Test | Result |
|---|---|---|
| workflows are invalid | they ran green earlier the same day | ruled out |
| Actions disabled for the repo | `GET /repos/…/actions/permissions` | `enabled: true`, `allowed_actions: all` |
| workflow token permissions | `GET /repos/…/actions/permissions/workflow` | `default_workflow_permissions: read` — fine for a workflow that only reads |
| the account is blocked | **a one-step probe workflow pushed to the account's public repo** (`summrise-canary`, `runs-on: ubuntu-latest`, `echo`) | **job started and succeeded** (`success`) |
| billing readable through the API | `GET /users/SilasVale/settings/billing/actions` | `404` — the token is not the billing scope, so this could not be read |

## What the experiment shows

Actions starts jobs for this **account** — the public probe proves the account, the token and GitHub's
scheduler are fine. The same account cannot start a job in **`SilasVale/summrise`, which is private**.

That is the signature of private-repository Actions minutes being used up, or a spending limit of zero:
on a personal Free plan the 2,000 monthly minutes apply to private repositories only, and public
repositories are free. The session's usage fits — many `ci.yml` runs (two Windows jobs each) and
several ~25-minute installer builds, over one day.

## The four ways out

1. **Billing** (`Settings → Billing → Actions`): add or fix the payment method, or raise the spending
   limit above zero. Immediate, and restores the hosted runners the workflows already name.
2. **Wait for the reset**: personal accounts reset included minutes on the billing-cycle anniversary.
   Costs nothing; costs a month of no CI.
3. **Make the repository public**: public repositories get free hosted minutes. A product decision, not
   a maintenance one — the source becomes readable.
4. **A self-hosted runner on a canary machine**: unlimited minutes and no billing, at the cost of a
   resident service on an operator's box, a Rust toolchain (~1 GB) and NSIS on that machine, and the
   operational rules a self-hosted runner implies (it must never be offered to a public repository).
   **Not installed**: it changes a machine the operator uses, so it needs their word.

Until one of these happens, a release is assembled from a local build — which is what `v0.3.7` and
`v0.3.8` did, with the installer run on a real machine before the version was called released. The
tag's own `installer` run stays red, and that red is honest: it is this, not the workflow.
