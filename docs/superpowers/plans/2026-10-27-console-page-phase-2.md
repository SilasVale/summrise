# The Console Page's Second Rust Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `/next` an operator page that changes the fleet, not only displays it: rename/remove a device, mint an enrollment code, and read the audit/denial logs — all through the `/next`-scoped cookie and a CSRF-checked form.

**Architecture:** Phase 2 stays inside the `/next` namespace, because the phase-1 cookie is `Path=/next` and cannot reach `/fleet/...`. Each mutation is a form post to a `/next/...` route that reuses the existing registry/audit/denial code and the session's CSRF value; each read is a server-rendered table. No React page, API route or wire shape changes.

**Tech Stack:** Rust 2021, axum 0.7, maud, the existing `summrise-console` crate and its in-process test harness.

**Spec:** `docs/superpowers/specs/2026-10-27-console-page-in-rust.md`
**Prior plan:** `docs/superpowers/plans/2026-10-27-console-page-phase-1.md`

## Global Constraints

- Rust only; no new dependency (the phase-1 `maud` covers this).
- The `/` React page and every API route keep their current behaviour and wire shapes.
- Every `/next` mutation is a POST with the session's CSRF value; absent/wrong/foreign → 403, never a state change.
- The cookie stays `Path=/next; HttpOnly; SameSite=Lax; Max-Age=86400`; `Secure` iff `X-Forwarded-Proto: https`.
- Page HTML goes through `maud`; no `format!`-built HTML.
- The console suite stays green: `cargo test -p summrise-console` from `device/`.
- Deploy version: `0.3.49`.

## Review Focus

- **A mutation with the wrong CSRF** (absent / foreign / stale) changes nothing and returns 403; the session survives.
- **A rename with an empty or over-long name** is refused with a visible error, not a 500 or a silent no-op; the old name survives.
- **Removing a device** stops its channel and its row disappears from `/next`; the audit line records it.
- **The enrollment code** is shown once, is single-use, and expires; a second `/next/enroll` mints a new one.
- **The audit/denial tables** are read-only and bounded (the existing `DEFAULT_LOG_LIMIT`/`MAX_LOG_LIMIT`), and unauthenticated `/next/audit` is the login page, never the log.

---

### Task 1: Rename and remove a device from `/next`

**Files:**
- Modify: `device/console/src/http.rs` (the routes and the handlers)
- Modify: `device/console/src/http/page.rs` (the row's forms)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Consumes: `cookie_session`, `session_cookie`, the `NextLogout` CSRF pattern, `device_views`, `DeviceRegistry::rename/remove` (whatever the existing `/fleet/:id/name`/`DELETE /fleet/:id` call), `refuse_as`/`ApiError` if useful.
- Produces:
  - `POST /next/fleet/:id/name` (form `csrf`, `name`) → 303 `/next`; 403 on CSRF; 400/422 with the form and an error on an empty name.
  - `POST /next/fleet/:id/remove` (form `csrf`) → 303 `/next`; 403 on CSRF.
  - The `/next` row's forms and a one-line error area.

- [ ] **Step 1: Write the failing tests** — a rename changes the row; a rename with the wrong CSRF is 403 and the name survives; an empty rename is refused with the error text; a remove removes the row.

- [ ] **Step 2: Run them to see the 404s** (`cargo test -p summrise-console --test console the_rust_page`).

- [ ] **Step 3: Implement the two handlers and the row forms**, reusing the existing registry calls and the phase-1 CSRF comparison.

- [ ] **Step 4: Run the tests green; the whole suite; clippy.**

- [ ] **Step 5: Commit.**

### Task 2: Mint an enrollment code from `/next`

**Files:** `device/console/src/http.rs`, `device/console/src/http/page.rs`, `device/console/tests/console.rs`.

**Interfaces:**
- Consumes: the existing `codes`/`EnrollmentCodes` issue path and the `POST /enroll` logic.
- Produces: `POST /next/enroll` (form `csrf`) → 303 `/next?code=<code>`; the page shows the code once with the expiry; 403 on CSRF.

- [ ] **Step 1: The failing test** — a CSRF-valid enroll renders the code once and the code is single-use; a wrong CSRF is 403 and mints nothing.
- [ ] **Step 2: Run it (fails).**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Green; suite; clippy.**
- [ ] **Step 5: Commit.**

### Task 3: Read the audit and denial logs from `/next`

**Files:** `device/console/src/http.rs`, `device/console/src/http/page.rs`, `device/console/tests/console.rs`.

**Interfaces:**
- Consumes: `Audit`/`Denials` and the existing `/audit`/`/denials` handlers' read logic and limits.
- Produces: `GET /next/audit` and `GET /next/denials`, login-gated by the cookie, server-rendered tables bounded by `DEFAULT_LOG_LIMIT`/`MAX_LOG_LIMIT`.

- [ ] **Step 1: The failing tests** — without the cookie both are the login page; with it they show the audit/denial rows; the limit is bounded.
- [ ] **Step 2: Run (fails).**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Green; suite; clippy.**
- [ ] **Step 5: Commit.**

### Task 4: Ship 0.3.49

- [ ] Build the Windows console (`SUMMRISE_BUILD_VERSION=0.3.49`), deploy to `C:\SummriseConsole` on 9vh92bj, restart `SummriseConsole`, verify `/health` 0.3.49, `/next` login, the three device rows, the rename/remove/enroll/audit on the live fleet; `/` hash unchanged.
- [ ] Run the new `/next` legs of `scripts/console-smoke.mjs` against the live console.

## Self-Review

- **Spec coverage:** the fleet actions (rename/remove), enrollment and the audit/denials of the spec's phase 1/2 are here; the connect card/tokens and the terminal/vitals stay in later slices.
- **Cookie path:** every new route is under `/next`, so the phase-1 cookie reaches it; the phase-2 review recommendation ("mirror the needed endpoints under `/next`") is the chosen path.
- **Review Focus:** each line has a test in Tasks 1-3 except the live `/` hash (Task 4).
