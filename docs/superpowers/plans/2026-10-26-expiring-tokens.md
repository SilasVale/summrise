# Expiring tokens, permanent by default

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** a team can give a permanent token an optional lifetime at creation (the default stays
"never"), see the date, and have an expired token refused like any other bad credential; rotation
keeps the token's lifetime policy.

**Architecture:** the token record gains `expires_at_ms: Option<u64>`. Creation takes an optional
`expires_in_s` (1 hour to 365 days); `None` stays permanent. The `/mcp` token branch checks expiry
before it mints a grant; rotation carries the old lifetime (old expiry − old creation) onto the new
record, or keeps `None`. The console page offers the choice and shows the date; the docs stop saying
every token is permanent.

**Tech Stack:** Rust (`summrise-console`: `tokens.rs`, `http.rs`), React + TS (`ConnectCard.tsx`).

**Spec:** the operator's permanent-token requirement (default permanent) and the team scenarios in
`docs/CONNECT-MCP.md`.

## Global Constraints

- **Permanent is the default and stays so.** An absent `expires_in_s` (or `null`) means no expiry; the
  UI's initial choice is **Never**. Existing token files (no field) load as permanent.
- **The window is 1 hour to 365 days.** `3600..=31_536_000` seconds; a shorter or longer value is a
  typed 400 (`expiry_too_short`/`expiry_too_long`), a unit error, not a token. `now + s*1000`, saturating.
- **An expired token is a credential failure.** The `/mcp` token branch finds the record, then checks
  `now >= expires_at_ms` before the device match or any grant signing, and refuses with a denial-log
  detail `expired_api_token` (401 on the wire). It never mints a grant, never marks last use, never
  touches the audit.
- **Rotation preserves the policy.** The new record's expiry is `now + (old.expires_at_ms −
  old.created_at_ms)` when the old had one, `None` otherwise; the old creation/expiry pair is the
  duration, not the new record's absolute time.
- **The list shows the fact.** `TokenMeta.expires_at_ms`; the row says `expires <date>` or `never
  expires` beside its tools and last use; the one-time created view is unchanged (the list refresh
  shows the date).
- **No audit kind change, no new endpoint.** The create response/list carry the field; the deny path
  keeps its existing shape.
- English; `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **Default permanence.** An old file, a create without the field, and a rotation of a permanent
   token all produce `None`; no migration reinterprets a permanent token as expiring.
2. **The boundary.** `now == expires_at_ms` is expired; one millisecond earlier is valid; the 1 h and
   365 d bounds are inclusive at the edges and the adjacent values are refused.
3. **Expiry before scope.** An expired foreign token is refused as expired, not as a device mismatch,
   and neither path mints or audits; the denial detail is `expired_api_token`.
4. **Rotation's arithmetic.** A 30-day-old token created with 90 days rotates to now + 90 days (the
   policy), not the old absolute date and not a fresh but different duration; a permanent stays
   permanent.
5. **The page is honest.** "Never" omits the field; the list's date is local, and the wording matches
   the backend (expires vs never expires); the create form's bounds are the server's.

---

### Task 1: The backend fact

**Files:**
- Modify: `device/console/src/tokens.rs` (+ store tests)
- Modify: `device/console/src/http.rs` (the create request/handler, the `/mcp` check, rotate, tests in
  `tests/console.rs`)

**Interfaces:**
- `Token.expires_at_ms: Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`);
  `TokenMeta` same; `Token::meta` copies it; `Draft.expires_in_s: Option<u64>`.
- `const MIN_TOKEN_EXPIRY_S: u64 = 3_600;` `const MAX_TOKEN_EXPIRY_S: u64 = 31_536_000;`
- `TokenError::{ExpiryTooShort, ExpiryTooLong}`; `CreateToken.expires_in_s: Option<u64>`
  (`serde(default)`); handler maps to `expiry_too_short`/`expiry_too_long` 400.
- `create`: after name/tools/device/cap checks, validate the window and compute
  `expires_at_ms = expires_in_s.map(|s| now_ms.saturating_add(s.saturating_mul(1_000)))`.
- `rotate`: lifetime = `old.expires_at_ms.map(|e| e.saturating_sub(old.created_at_ms))`; new
  `expires_at_ms = lifetime.map(|d| now_ms.saturating_add(d))`.
- `mint_token_grant`: after `find_by_hash`, if `expires_at_ms.is_some_and(|e| now >= e)` return
  `TokenRefusal::Unauthenticated("expired_api_token")` (the existing denial path).
- Tests: store (bounds, default None, old file), integration (expired 401 + denials detail; a valid
  one just before expiry; rotate preserves; create/list round-trip).

- [ ] **Step 1: Store tests first**
- [ ] **Step 2: Endpoint tests first**
- [ ] **Step 3: Implement, gates, commit**
  `console: a token can expire, and rotation keeps its lifetime`.

### Task 2: The page's choice

**Files:**
- Modify: `device/console/web/src/api.ts` (+ `api.test.ts`): `TokenMeta.expires_at_ms?`,
  `createToken` input `expires_in_s?: number`
- Modify: `device/console/web/src/ConnectCard.tsx` + `.test.tsx`
- Modify: `device/console/web/src/app.css` only if a class is new

**Interfaces:**
- The token form gains an **Expires** select: `Never` (0, default), `30 days`, `90 days`, `365 days`;
  the create body includes `expires_in_s` only when not Never.
- The token row says `expires <local date>` or `never expires` after the tools/consent and before or
  after `last used`; the existing facts stay.
- The create/rotate/list flows otherwise unchanged; the rotate confirmation names the lifetime
  preservation ("keeps ... its expiry" when it has one is a welcome sentence).
- Tests: Never omits the field; 30/365 send the seconds; the row renders both states; the existing
  create/rotate tests stay green.

- [ ] **Step 1: Tests first**
- [ ] **Step 2: Implement, gates, commit**
  `console: the token form offers an expiry, never by default`.

### Task 3: The docs

**Files:**
- Modify: `docs/CONNECT-MCP.md` (Scope and revocation: permanent by default, optional 1 h–365 d,
  expired = 401 `expired_api_token`, rotation keeps the lifetime)
- Modify: `docs/QUICKSTART.md` (the one sentence that says a token has no expiry)

- [ ] Commit `docs: a token is permanent unless it was given a lifetime`.

## Self-review

- Spec: the team's expiring credential with the operator's permanent default preserved.
- Review Focus: default, boundary, expiry-before-scope, rotation arithmetic, honest UI.
- Deliberate: no post-creation edit of a token's expiry (rotate with the policy; revoke/re-create for a
  different one), no expiry extension endpoint, no audit kind.

## Review rulings (2026-10-26)

- The expiry check sits after the record lookup and before the device match, signing, marking and
  audit; an expired foreign token is refused as expired (`expired_api_token`), and no grant is minted.
- `None` is permanent everywhere: create, rotate, list, reload and an old file without the field.
- Rotation measures the old **policy** (expiry − creation) and applies it from the rotation instant;
  a permanent token stays permanent.
- The UI's Never omits `expires_in_s` (the key is absent, not null); the row treats absent and null as
  `never expires`; the rotate dialog explains the lifetime is counted from now for expiring tokens.
- The tests pin the inclusive 1 h/365 d bounds, the adjacent refusals, `now == expiry` as expired and
  `expiry − 1 ms` as valid.
