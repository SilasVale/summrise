# The Security page: failed authentications the operator can see

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the operator can see the console's unauthenticated refusals — bad operator tokens, missing
or expired sessions, bad device tokens, failed registration proofs, unknown/revoked/expired API
tokens — instead of those lines living only in a file on the host.

**Architecture:** the denial log already exists (`denials.jsonl`, its own 2 MiB cap and one rotation,
rate-limited to 100 lines/min with a lazily written `suppressed N` summary). A new
`GET /denials?limit=` (operator session, same rules as `/audit`) reads it newest-first; the page adds a
**Security** rail section that renders the same table used for the audit with a denial-specific empty
sentence and a short explanation of the rate limit and the suppression line.

**Tech Stack:** Rust (`summrise-console`: `denials.rs`, `http.rs`), React + TS (`api.ts`, `Rail.tsx`,
`AuditTable.tsx`, `App.tsx`).

**Spec:** `docs/superpowers/specs/2026-10-26-credential-surface-threat-model.md` ("the denial log is
separate, rate-limited and capped"; the operator needs to see probing).

## Global Constraints

- **Operator-only.** `GET /denials` sits behind `OperatorSession`; unauthenticated callers get 401 and
  their own refusal goes to the denial log (the existing `refuse_as` path), not to the audit.
- **No secrets.** Denial details are the existing short codes (`bad_operator_token`,
  `unknown_api_token`, `expired_api_token`, `bad_device_token`, `suppressed N`, …); the page renders
  `detail` verbatim, never a token, and no new field carries one.
- **Same limits as the audit.** Default 100, maximum 1000, a malformed `limit` is the console's
  `invalid_limit` JSON 400 (not axum's plain rejection); newest first, current file before the
  rotated one.
- **The page is honest.** It says the log is rate-limited (100/minute), that a `suppressed N` line
  means N refusals were not written, and its empty state is "No failed authentications yet." — not
  the audit's sentence.
- **Additive UI.** The audit table gains an optional `empty` sentence (default unchanged); the audit
  page keeps its behaviour, tests and polling. The rail grows to twelve sections; the shell test's
  count and navigation list change with it.
- English; `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **The route is operator-only and read-only.** No mutation, no path parameter, no way to forge a
   line; the limit is clamped before the read; a bad limit is the JSON refusal.
2. **The log's guarantees survive the read.** The rate limit/cap/rotation are untouched; the read
   takes the append lock (`JsonLines::newest` does), so a concurrent write is not torn.
3. **The page says what it is.** The rate limit and suppression summary are explained in plain
   sentences; the empty state is denial-specific; the table's device/status filters work and the
   token column is empty (`—`) because denials carry no token id.
4. **The audit page is unchanged.** Its empty sentence, data, filters, polling and tests stay as they
   were; the new prop defaults to the old sentence.
5. **The shell is complete.** The Security rail button navigates, the page title/section is
   accessible, and the twelve-section test is updated deliberately (not by loosening the assertion).

---

### Task 1: The route

**Files:**
- Modify: `device/console/src/denials.rs` (+ tests)
- Modify: `device/console/src/http.rs` (route + handler + tests in `tests/console.rs`)

**Interfaces:**
- `Denials::newest(&self, limit: usize) -> Vec<Value>` → `self.file.newest(limit).await` (the method is
  async; mirror `Audit::newest`).
- `GET /denials` (OperatorSession) with the existing `AuditQuery` shape (or a renamed `LogQuery`; a
  rename must update `/audit` too) and the audit's `DEFAULT_AUDIT_LIMIT`/`MAX_AUDIT_LIMIT` rules.
  Response: `Json<Vec<Value>>`.
- Tests: unauthenticated 401 (and it appears in the denial log, not the audit); an operator gets the
  seeded lines newest first; `limit=1` returns one; `limit=0` empty; a malformed limit 400
  `invalid_limit`; a limit above the max is clamped; an empty/missing file `[]`.

- [ ] **Step 1: Tests first**
- [ ] **Step 2: Implement, gates, commit**
  `console: failed authentications are readable at /denials`.

### Task 2: The page

**Files:**
- Modify: `device/console/web/src/api.ts` (+ `api.test.ts`): `denials(limit?)`
- Modify: `device/console/web/src/AuditTable.tsx` (+ `.test.tsx`): `empty?: string` (default
  "No audit lines yet.")
- Create: `device/console/web/src/SecurityPage.tsx` (+ `.test.tsx`)
- Modify: `device/console/web/src/Rail.tsx` (`Page` union, `PAGE_LABELS`, `ICONS` — a shield path)
- Modify: `device/console/web/src/App.tsx` + `App.test.tsx`: fetch denials with the audit refresh,
  state, render the page, update the rail count/navigation tests and the `./api` mock.

**Interfaces:**
- `api.denials(limit = 200): Promise<AuditLine[]>` — `GET /denials?limit=`.
- `SecurityPage({ lines, devices, tokens }: { lines: AuditLine[]; devices: Device[];
  tokens: TokenMeta[] })`: an `<h2>Security</h2>`/section with a hint that the log is rate-limited
  (100/minute) and that `suppressed N` means N refusals were not written, then
  `<AuditTable lines={lines} devices={devices} tokens={tokens} empty="No failed authentications yet." />`.
- App: `denials` state, fetched in `refreshData` (`api.denials(AUDIT_WINDOW)`), passed to the new
  `page === "security"` branch; the rail's twelfth entry is `Security`.
- Tests: api path/limit; AuditTable's custom empty; SecurityPage renders lines/empty and the
  explanation; App's twelve sections, the Security navigation, and the page's table after a denial
  line is returned; the audit page's empty sentence stays "No audit lines yet.".

- [ ] **Step 1: Tests first**
- [ ] **Step 2: Implement, gates, commit**
  `console: the Security page shows failed authentications`.

### Task 3: The record

**Files:**
- Modify: `docs/superpowers/specs/2026-10-26-credential-surface-threat-model.md` (Controls: the denial
  log is surfaced read-only on the Security page; residual risk of probing is now observable)

- [ ] Commit `docs: the denial log is visible on the Security page`.

## Self-review

- Spec: the operator's inability to see probing; the threat model's denial-log control made visible.
- Review Focus: operator-only, log guarantees, honest page, audit unchanged, shell complete.
- Deliberate: read-only (no clear/export), no real-time alert/notification for denials this round, no
  cross-file search; the page is the existing table with the denial-specific sentence.

## Review rulings (2026-10-26)

- The route is read-only and operator-only; a malformed `limit` is the console's JSON 400, missing is
  100, above the max clamps to 1000, and `0` returns no lines. The 401 of an unauthenticated caller is
  itself a denial line, not an audit line.
- The denial log's cap, rotation and 100/minute window are untouched; the read takes `JsonLines`'s
  append lock, so a concurrent append is not torn.
- The page uses the existing table with a denial-specific empty sentence; the audit page passes no
  `empty` prop and keeps its behavior/tests. The rail's twelve-section shell test was updated
  deliberately.
- The threat model now records the Security page as the operator's view of probing, and that probing
  is observable but not alerted.
