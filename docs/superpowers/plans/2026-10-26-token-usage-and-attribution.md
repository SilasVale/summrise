# Which token did that? Usage and attribution for permanent tokens

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** an operator with several permanent tokens can see whether each is still in use and which
token made any audited MCP call, so revocation is an informed act instead of a guess.

**Architecture:** the token record gains `last_used_ms`, updated when a token authenticates an MCP
call; the write is throttled (once a minute per token) so a busy client does not rewrite the file on
every request. The audit line gains `token_id` for calls that a token authenticated — an additive
optional field; the grant path writes none. The console page shows the last use in the token list and
the token's short id in the audit table.

**Tech Stack:** Rust (`summrise-console`: `tokens.rs`, `http.rs`, `audit.rs`, `denials.rs`), React +
TS (`device/console/web`: `api.ts`, `ConnectCard.tsx`, `AuditTable.tsx`).

**Spec:** the permanent-token design (`docs/superpowers/plans/2026-10-26-permanent-api-tokens.md`);
the operator's need to know which credential is live.

## Global Constraints

- **`token_id` is the record id, not the secret.** The id is already in `GET /tokens`; the token
  (the `srm_` value) never appears in an audit line, a denial line, a log or a response beyond the
  create response.
- **The audit field is additive.** `Entry.token_id: Option<&'a str>` with
  `skip_serializing_if = "Option::is_none"`; existing lines stay valid; the grant path and every
  non-token action write no field. The `kind` set does not change.
- **Attribution is per MCP line.** `forward` writes the `token_id` on the `mcp` line for both
  `tools/list` and `tools/call` when the bearer was a token; the operator-session route
  (`proxy_mcp`) writes none; a refusal before the token resolves writes none.
- **`last_used_ms` is best-effort.** Every successful token exchange updates it in memory; the file
  is written only when the previous persisted value is absent or at least 60 seconds old (one
  constant). A write failure must not fail the call: the in-memory fact stays, the next due use
  tries again. The list shows the value; a reload may lag by under a minute, and that is stated.
- **No expiry, no new credential, no new endpoint.** The token model is unchanged; this round only
  observes it.
- English; `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **Attribution cannot leak the secret.** The audit carries the id only; a grep of the audit/denial
   writers for the plaintext token finds none; the UI renders the id, never `srm_`.
2. **The field is optional and additive.** A grant-authenticated call and every older action have no
   `token_id`; the table tolerates its absence; old audit files load.
3. **`last_used_ms` moves only on a successful exchange.** Unknown/revoked/foreign tokens, a refused
   grant signing, and the operator route do not touch it. A failed file write leaves memory updated
   and returns the call's result.
4. **The throttle is one fact, one write.** Two calls within a minute write once and keep the newest
   in memory; the next call after the window writes again. A reload within the window shows the last
   persisted value, as documented.
5. **The UI answers the question.** The token list shows a date or "never used"; the audit table's
   token cell shows a short id for token lines and nothing otherwise; the existing filters still
   work and line keys stay stable.

---

### Task 1: The backend fact

**Files:**
- Modify: `device/console/src/tokens.rs` (+ tests)
- Modify: `device/console/src/http.rs` (+ integration tests in `tests/console.rs`)
- Modify: `device/console/src/audit.rs` (`Entry.token_id`, doc)
- Modify: `device/console/src/denials.rs` (the `Entry` construction gains `token_id: None`)

**Interfaces:**
- `Token.last_used_ms: Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`);
  `TokenMeta.last_used_ms: Option<u64>` (same skip); `Token::meta` copies it.
- `Tokens::mark_used(&mut self, hash: &str, now_ms: u64) -> bool`: find by hash, set
  `last_used_ms = Some(now)`, save only when the previous persisted value is `None` or
  `now - previous >= 60_000`, ignore a save error (memory updated, return `true`), return `false`
  when no record matches. `const LAST_USED_SAVE_MS: u64 = 60_000`.
- `audit::Entry.token_id: Option<&'a str>` (skip none); `record_tool(kind, device, status, detail,
  tool, token_id)`; `record(...)` passes `None`; `refuse_as` and the two direct `Entry` sites pass
  `None`.
- `mint_token_grant(...) -> Result<MintedGrant, TokenRefusal>` with
  `struct MintedGrant { envelope: GrantEnvelope, token_id: String }`; after `grants::sign` succeeds,
  `lock(&app.tokens).mark_used(&hash, now)` and return the id. The standard route carries
  `Option<&str>` into `forward`; the custom route passes `None`.
- `forward(..., token_id: Option<&str>)` writes it on every `record_tool` call.

- [ ] **Step 1: Store tests first**

`last_used_ms` starts `None`; after `mark_used` it is `Some(now)` and the list shows it; a second call
within 60 s updates memory but the file still holds the first value; a call at +60 s writes the newer
value; a hash that matches nothing returns `false` and changes no record; an old file without the
field loads (`None`); a reload after a persisted use shows it.

- [ ] **Step 2: Endpoint/audit tests first**

Create a token, call `tools/list` with it (the mock device), then: `GET /tokens` shows
`last_used_ms`; the audit `mcp` line has `token_id` equal to the created id and the tool; the same
call with a grant has no `token_id`; the foreign-device refusal has none; the operator `/mcp` route
has none. The audit line's JSON has no `srm_` substring.

- [ ] **Step 3: Implement and verify**

`cargo test -p summrise-console`, clippy `--all-targets -- -D warnings`, fmt. Commit:
`console: a token says when it was last used and which calls it made`.

### Task 2: The page's facts

**Files:**
- Modify: `device/console/web/src/api.ts` (`TokenMeta.last_used_ms?`, `AuditLine.token_id?`)
- Modify: `device/console/web/src/ConnectCard.tsx` + `.test.tsx`
- Modify: `device/console/web/src/AuditTable.tsx` + `.test.tsx` (+ `auditFilter.ts` only if a key needs it)

**Interfaces:**
- `TokenMeta` gains `last_used_ms?: number | null`; `AuditLine` gains `token_id?: string | null`.
- ConnectCard's token row shows `last used <local date>` or `never used` in the muted line; the
  existing name/date/tools/consent/Revoke stay.
- AuditTable's token cell shows `shortId(token_id)` (the formatter already imported) or `—`; its
  `lineKeys` includes `token_id ?? ""` so two otherwise identical lines with different tokens stay
  distinct; the existing filters and expansion behaviour are unchanged.

- [ ] **Step 1: Tests first**

ConnectCard: a token with `last_used_ms` renders the date; without it, "never used"; a create/list
refresh shows the value from the API (mock). AuditTable: a line with `token_id` shows its short id; a
line without shows `—`; the filter/expansion tests still pass; two lines identical except token ids
get distinct keys (the row expansion survives a re-render).

- [ ] **Step 2: Implement and verify**

`npm run typecheck`, `npm test`, `./scripts/build-web.sh` (exit code checked), bundle committed.
Commit: `console: the token list says when it was last used; the audit names the token`.

## Self-review

- Spec: the operator's ability to revoke an unused/leaked token knowingly, and to attribute an audit
  line to the credential that made it.
- Review Focus: no secret in the audit, additive field, last-used only on success, one write per
  window, the page's two answers.
- Deliberate: no per-token call counts or series (the audit is the log), no expiry, no filter by
  token this round (the table's cell and the existing device/tool/status filters answer the common
  question; a token filter is a follow-up if it is asked for).

## Review rulings (2026-10-26)

- `mark_used` measures the save window from the **persisted** value (a private `#[serde(skip)]`
  mirror), not the newest in-memory use; a failed write leaves memory updated and the next due call
  retries. A successful write syncs the mirror for every record, because the file is the whole list.
- The audit's `token_id` is the record id only; the plaintext is never passed to `Entry`, and the
  integration test greps both audit files for `srm_` and for the value.
- The token route passes the id on both `tools/list` and `tools/call`; the grant path, the custom
  operator route and the foreign-device refusal carry none. Malformed tool calls are audited with
  the id when the bearer was a token.
- The page's audit token cell shows the short id or an em dash; the row key includes `token_id`, so
  two otherwise identical lines stay distinct and an expanded row survives a poll.
- `CONNECT-MCP.md` now states the list's last-use fact and the audit attribution.
