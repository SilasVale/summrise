# Permanent API tokens for the console (plan B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** a console-issued, device- and tool-scoped, revocable API token that an AI client can keep in
a fixed MCP configuration forever; the console validates it and mints a short-lived grant per request,
so the device protocol and the offline verification are unchanged.

**Architecture:** a token is `srm_` + 64 lowercase hex (32 bytes), shown once; the console stores only
its SHA-256. The standard MCP endpoint accepts either the existing grant envelope (hex of its JSON) or
a token (`srm_` prefix); for a token it checks the record and the path's device, signs a 60-second
grant with the token's tools and destructive consent, and then runs the existing grant path. The
console page creates, lists and revokes tokens; the device never sees a token.

**Tech Stack:** Rust (`summrise-console`: `tokens.rs`, `http.rs`, `mcp.rs`, `grants.rs`), React 19 +
TypeScript (`device/console/web`), the existing store/atomic-write patterns.

**Spec:** the operator's decision "按照 B 来做" (2026-10-26); `docs/CONNECT-MCP.md`.

## Global Constraints

- **Token secrecy.** `srm_` + 32 bytes of `OsRng`, 64 lowercase hex. Only `sha256(token)` (hex) is
  persisted; the plaintext is returned **once**, in the create response, and never in a list, an audit
  line, a log, an error, or a device frame. A revoked token is removed from the store and its next
  request is 401.
- **Scope.** A token names one device, a non-empty tool set from the registry, and
  `allow_destructive` (the same signed consent a grant carries). The `/mcp/:id` path's device must
  equal the token's; a mismatch is 401, not 403-with-detail. A valid token causes a **60-second**
  grant signed with exactly those tools and that consent; the device verifies it as any grant.
- **The grant path is untouched.** A bearer that is not `srm_…` is parsed and verified exactly as
  today. No device change, no new device route, no change to `Grant`/`GrantEnvelope`.
- **Operator-only management.** `GET/POST/DELETE /tokens` require the same operator session as
  `/grants`; the endpoints live behind the same Access application as the rest of the API.
- **Limits.** At most 64 tokens; name 1–80 characters (trimmed); tools non-empty and every name in
  `summrise_core::registry`; the device must exist. Validation errors are typed `ApiError`s with the
  console's existing codes (`empty_tools`, `unknown_tool`, `unknown_device`, `name_empty`,
  `too_many_tokens`, `no_such_token`).
- **The token is a bearer.** The UI says it once and says so: treat it like a password; revoke is the
  only withdrawal. No expiry (revocation replaces it), and the create response is the only time it is
  visible.
- English. `cargo test -p summrise-console`, clippy, fmt; `npm run typecheck`, `npm test`,
  `./scripts/build-web.sh` (exit code checked, bundle committed).

## Review Focus

1. **At rest and in transit.** The store file contains hashes only; `GET /tokens` has no hash or
   token; the create response is the only plaintext; audit lines never carry it; the minted grant is
   the only thing the device sees.
2. **Scope is enforced before the device.** Wrong device → 401; unknown/revoked → 401; a tool outside
   the token never reaches the device (the grant names only the token's tools); `allow_destructive`
   is the token's, not the caller's.
3. **The grant path is unchanged.** Existing grant clients behave exactly as before; the token branch
   is an additional bearer type, not a rewrite.
4. **Management is honest.** The create form shows the token once with a copy action and a warning;
   the list shows name/device/tools/created (never the token); revoke asks for confirmation and the
   next request fails; the empty state explains the feature.
5. **The mint is cheap and bounded.** A 60-second grant per request; no token state on the device; no
   extra round trip beyond the existing proxy.

---

### Task 1: The token store and the endpoint

**Files:**
- Create: `device/console/src/tokens.rs` (store + tests)
- Modify: `device/console/src/http.rs` (AppState, routes, handlers, tests)
- Modify: `device/console/src/mcp.rs` (the bearer split: grant vs token)
- Modify: `device/console/src/lib.rs` (the module)
- Modify: `device/console/src/config.rs` only if the data path needs no change (it does not)

**Interfaces:**
- `Token { id: String, name: String, device_id: String, tools: Vec<String>, allow_destructive: bool,
  created_at_ms: u64, hash: String }`; `hash` is `sha256(token)` as 64 lowercase hex.
- `Tokens` registry: `load(data_dir)`, `list() -> Vec<TokenMeta>` (no hash), `create(...) ->
  (TokenMeta, String)` (the plaintext token once), `revoke(id) -> bool`, `find_by_hash(hash) ->
  Option<Token>`, `save()` atomic (`<data>/tokens.json`, temp + rename like `devices.json`).
- HTTP:
  - `GET /tokens` (operator) → `Json<Vec<TokenMeta>>`, newest first.
  - `POST /tokens` (operator) `{name, device_id, tools, allow_destructive}` → 201
    `{id, name, device_id, tools, allow_destructive, created_at_ms, token}`.
  - `DELETE /tokens/:id` (operator) → 204 or 404 `no_such_token`.
  - `/mcp/:id`: read the bearer; `srm_` → hash, look up, device check, `grants::sign(&app.grant_key,
    &id, token.tools, 60, token.allow_destructive, (app.now)())`; on any failure `unauthorized` (audit
    denial). Otherwise the existing `grant_from_headers` path.
- AppState: `tokens: Arc<Mutex<tokens::Tokens>>`.

- [ ] **Step 1: Store tests first**

Mint returns a `srm_` token of 68 chars (64 hex after the prefix), the file has the hash and not the
token, list has no hash/token, revoke removes it and `find_by_hash` returns None, the cap refuses the
65th, names/tools/devices are validated. Atomic write: a crash/temp file leaves the old file readable
(the existing store tests show the pattern).

- [ ] **Step 2: The routes and the bearer split**

Add the three handlers with `OperatorSession`; the create validates and saves; `DELETE` 404s. In
`standard_mcp`, split the `Authorization` reading: a helper returns either `BearerKind::Grant(env)` or
`BearerKind::Token(String)`; the token branch mints the grant before `ensure_registered` (which still
applies) and then proceeds through the existing code. Audit denials for token failures.

- [ ] **Step 3: Endpoint tests**

Using the console's test server: create a token (201, `srm_`, list shows metadata only, file has the
hash); a token request to `/mcp/:id` with a mock device channel arrives as a grant with exactly the
token's tools/consent and a ~60s expiry; wrong device, unknown token, revoked token → 401; the
existing grant path still works; the create response contains the token only once (the list never
does); the cap/name/tool/device errors are the typed codes.

- [ ] **Step 4: Verify and commit**

`cargo test -p summrise-console`, clippy, fmt. Commit:
`console: permanent API tokens mint a short grant per request`.

### Task 2: The console page's token UI

**Files:**
- Modify: `device/console/web/src/api.ts` (+ `api.test.ts`): `tokens()`, `createToken()`, `revokeToken()`
- Modify: `device/console/web/src/ConnectCard.tsx` (+ `.test.tsx`), `app.css` if a class is new
- Modify: `device/console/web/src/App.test.tsx` only if a shared mock needs the new methods

**Interfaces:**
- `api.tokens(): Promise<TokenMeta[]>`, `api.createToken({name, device_id, tools,
  allow_destructive}): Promise<{token: string} & TokenMeta>`, `api.revokeToken(id): Promise<void>`
  (DELETE `/tokens/<id>`).
- `TokenMeta = {id, name, device_id, tools, allow_destructive, created_at_ms}`.
- ConnectCard gains two modes: **Session grant** (today's path) and **Permanent token**:
  - name input (placeholder "laptop-dsh"), the tool selection (reuse the card's current list and
    All/None), the destructive checkbox, **Create token**;
  - on success: the token in a `<code>`/`<pre>` with a **Copy** button, the config snippet with
    `Authorization: Bearer <token>`, and a warning that it is shown once and is a password;
  - a **Existing tokens** list for this device: name, created date, tool count, **Revoke**
    (confirmation via `Modal`), refresh after revoke; no token value ever appears in the list.
- The card's session-grant behavior is unchanged.

- [ ] **Step 1: API tests first**

`api.test.ts`: `GET /tokens`, `POST /tokens` body, `DELETE /tokens/<id>` path/method. The existing
mock in `App.test.tsx` needs `tokens`/`createToken`/`revokeToken` (the App renders the card).

- [ ] **Step 2: Component tests first**

ConnectCard: the two tabs; create with a name/tools/consent sends the exact body and shows the token
once + config + copy; the list renders metadata (no `srm_`), revoke opens the confirm and sends the
delete, then re-lists; a create refusal shows the console's sentence; the session-grant path still
mints a grant.

- [ ] **Step 3: Implement and verify**

`npm run typecheck`, `npm test`, build with the exit code checked, bundle committed. Commit:
`console: permanent token creation and revocation beside the session grant`.

### Task 3: The documentation

**Files:**
- Modify: `docs/CONNECT-MCP.md` (a "Permanent tokens" section after "Mint a grant")

- [ ] **Step 1: Write it**

Explain: create in the console device card → **AI client** → **Permanent token**; the token is
`srm_…`, shown once, a bearer; the header is `Authorization: Bearer srm_…`; the console validates it,
checks the device, and mints a 60-second grant to the device, so the device protocol is unchanged;
revoke in the same card and the next request fails. Add the fixed-config DSH snippet (literal token in
a secret/env, not committed).

- [ ] **Step 2: Commit**

`docs: the permanent token path through the console`.

## Self-review

- Spec coverage: the operator's permanent-token requirement, as the scoped/revocable model agreed.
- Review Focus: hash/show-once, scope, unchanged grant path, management UX, cheap mint.
- Deliberate limits: no token expiry (revocation replaces it), no per-token audit name (the audit
  records the call and the device), the existing Access app covers `/tokens` and `/mcp/*`.
