# Rotate a token, and say its name in the audit

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** an operator can replace a permanent token in one confirmed action (same scope, new secret,
old dead immediately), and the audit names the token that made a call instead of showing a bare id.

**Architecture:** rotation is a store operation that removes the old record and inserts the new one in
**one** atomic write, so there is no window with both or neither; the HTTP route returns the new
plaintext once and audits the replacement as a creation plus a revocation. The audit table receives the
operator's token list, resolves each line's `token_id` to the token's name (short id fallback) and gains
a token filter; the App fetches the list beside the devices and audit, and a failed token fetch leaves
the table on ids rather than blanking the page.

**Tech Stack:** Rust (`summrise-console`), React + TS (`device/console/web`).

**Spec:** the permanent-token design and round 11's attribution
(`docs/superpowers/plans/2026-10-26-token-usage-and-attribution.md`).

## Global Constraints

- **Rotation is one save.** `Tokens::rotate` builds the next map (old removed, new inserted), writes it
  once, and only then adopts it in memory. A write failure returns `Storage` and leaves both the file
  and memory unchanged: either the old token still works or the new one does, never a half rotation.
- **The new token has the old scope**: same name, device, tools, destructive consent; new id, new
  `srm_` secret, new `created_at_ms`, `last_used_ms: None`, new hash. The count is unchanged, so the
  cap is not checked. The plaintext is returned once and appears nowhere else.
- **Old is dead on success.** The old record is gone from the file before the response is built; the
  next request with it is 401. A failed write leaves it working (nothing changed).
- **Audit, no new kind.** A rotation records `token` for the new record (with the consent detail) and
  `token_revoke` for the old (detail `rotated`), each with its device. No schema change beyond the
  existing `token_id` field.
- **Names are a convenience, ids are the fact.** The audit table resolves `token_id` through the
  operator's list; an unknown/revoked id shows its short id. A failed `GET /tokens` must not fail the
  App's load: the table falls back to ids.
- **The token filter is over the window.** Its options are the tokens that appear in the lines the page
  holds (named when the list knows them), plus every token when the list is present and used? No —
  only the ids present, so the control stays short; "All tokens" clears it. The filter composes with
  device/tool/status.
- English; `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **Atomicity.** A failed save leaves the old token valid and the file unchanged; a success leaves
   exactly one record (the new) and the old id absent; the count stays the same.
2. **One plaintext.** The rotate response is the only place the new value appears; audit/log lines
   carry the new id only; the old hash is gone.
3. **Scope preserved.** The new record's tools/consent/device/name equal the old; created/last-used are
   fresh; the UI shows the same name with a new created date.
4. **The audit reads like a story.** Rotating writes a creation and a revocation, both naming the same
   device; the table shows the token's name and filters by it; an id it cannot resolve is short, not
   blank.
5. **The App survives a token-fetch failure.** Devices and audit still render, the token cell shows
   ids, the filter degrades to "All tokens".

---

### Task 1: The rotation

**Files:**
- Modify: `device/console/src/tokens.rs` (+ store tests)
- Modify: `device/console/src/http.rs` (route + handler + audit; integration tests in `tests/console.rs`)

**Interfaces:**
- `Tokens::rotate(&mut self, id: &str, now_ms: u64) -> Result<Option<(TokenMeta, String)>, TokenError>`:
  `None` when no such id; otherwise the new record and its plaintext; one `save`; on error
  `TokenError::Storage` and no state change.
- `POST /tokens/:id/rotate` (operator session) → 201 `{id, name, device_id, tools,
  allow_destructive, created_at_ms, token}` (the create response shape); 404 `no_such_token`; 500 on
  storage. Audit: `token` (new device, `allow_destructive=true|false`) and `token_revoke` (old
  device, detail `rotated`).
- Tests: rotate returns a new `srm_`/id with the same scope; the old hash is gone, the new one is
  present; the file reloads to one record; the count/`MAX_TOKENS` unchanged; a write failure (unix
  read-only dir) leaves the old token valid and the list unchanged; unknown id → None/404; after
  rotation the old token's `/mcp` call is 401 and the new one succeeds; audit has the two lines.

- [ ] **Step 1: Store tests first**
- [ ] **Step 2: Endpoint/audit tests first**
- [ ] **Step 3: Implement, run the console gates, commit**
  `console: a token can be rotated in one atomic replacement`.

### Task 2: The page

**Files:**
- Modify: `device/console/web/src/api.ts` (+ test): `rotateToken(id)`
- Modify: `device/console/web/src/App.tsx` + `App.test.tsx`: fetch `api.tokens()` in probe/load beside
  devices/audit, tolerate its failure, pass `tokens` to every `AuditTable`
- Modify: `device/console/web/src/AuditTable.tsx` + `.test.tsx` + `auditFilter.ts` (+ tests): the
  `token` filter, name resolution, token cell
- Modify: `device/console/web/src/ConnectCard.tsx` + `.test.tsx`: the Rotate action

**Interfaces:**
- `api.rotateToken(id: string): Promise<Created>` — `POST /tokens/<id>/rotate`, returns the created
  shape (token included).
- `AuditTable` gains `tokens: TokenMeta[]`; the token cell shows `name` or `shortId(id)` or `—`; the
  token filter's options are the distinct `token_id`s in `lines`, labelled by name when known; the
  line key already includes the token id.
- `AuditFilter` gains `token: string` (`""` = all); `filterAudit` compares `line.token_id`.
- ConnectCard: the token row gets **Rotate**; a confirm `Modal` says the old token stops working and
  the new value is shown once; on confirm `api.rotateToken(id)`, then the created view (the same
  one-time token + config + copy) and a refreshed list; failure shows the console's sentence.
- Tests: App renders with tokens and with a rejected `tokens`; AuditTable names/short fallback/filter;
  ConnectCard rotate confirm → call → new token shown once → list refreshed; cancel sends nothing;
  the create/revoke paths unchanged; api path/method.

- [ ] **Step 1: API + component tests first**
- [ ] **Step 2: Implement, run the web gates, commit**
  `console: the audit names tokens, and a token can be rotated from the list`.

### Task 3: The docs

**Files:**
- Modify: `docs/CONNECT-MCP.md` (Scope and revocation: the Rotate action, what it preserves and what
  it kills; the audit's name/fallback)

- [ ] Commit `docs: rotation and the audit's token names`.

## Self-review

- Spec: the operator's rotation need and the round-11 attribution finished as names + filter.
- Review Focus: atomicity, one plaintext, scope, story, degradation.
- Deliberate: no per-token history page (the filter on the existing table is enough), no expiry, no
  bulk rotation.

## Review rulings (2026-10-26)

- Rotation is one save of the whole map (old removed, new inserted); a failed write returns before
  memory is adopted, so the old token keeps working and the file is unchanged. Tested on a read-only
  directory, in memory and after reload.
- The audit is a creation plus a revocation (`token` with the consent, `token_revoke` with `rotated`),
  both with the device and the 201 the caller saw; no new kind.
- The App's token fetch is caught to an empty list: devices/audit render, cells and filter options fall
  back to short ids, and no error is surfaced; a rejected fetch is tested.
- The audit filter's options are the distinct ids in the window (name when known, short id otherwise),
  sorted by label with "All tokens" first; the line key and expansion are untouched.
- Rotation at the cap succeeds and the count stays at the cap.
