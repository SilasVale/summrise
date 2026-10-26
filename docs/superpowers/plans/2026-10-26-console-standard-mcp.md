# The console's standard MCP endpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `POST /mcp/<device-id>`, a standard streamable-HTTP MCP endpoint authenticated by the console's signed grant, which forwards `tools/list` and `tools/call` through the existing device channel.

**Architecture:** Split the custom `/mcp` proxy into `verify_presented`, `ensure_registered` and `forward`; add a small pure `console/src/mcp.rs` for the grant header, media types, the initialize answer and the tool filter; add one handler in `console/src/http.rs` that verifies the grant **before** reading the body and logs pre-authentication refusals to `denials.jsonl`. The device, the grant format, the channel, the audit schema and the custom `/mcp` route do not change.

**Tech Stack:** Rust 1.98.1, axum 0.7, tokio, serde_json, the console's existing integration harness.

**Spec:** `docs/superpowers/specs/2026-10-26-console-standard-mcp-design.md`

## Global Constraints

- **English only** in code, comments, commits and docs; the operator conversation is Chinese.
- **The grant is the bearer:** lowercase hex of the `GrantEnvelope` JSON bytes, in
  `Authorization: Bearer <hex>`. No new credential store, no base64 dependency.
- **The custom `/mcp` must not change:** same body, same statuses, same audit rows, same order. Its
  existing 80 integration tests are the regression.
- **One authorization path:** the standard route calls the same `verify_presented`,
  `ensure_registered` and `forward` the custom route calls. No second copy of the tool-scope or
  timeout logic.
- **The audit/denial split is a security invariant** (`audit.rs`): a request that fails
  authentication never reaches `audit.jsonl`. Before a verified grant, refusals go to `denials.jsonl`;
  after it, outcomes go to `audit.jsonl`.
- **Verify the grant before the device lookup and before buffering the body.** An anonymous caller
  must not learn which ids are registered, and must not make the console hold 8 MiB per request.
- **No device change, no channel change, no grant change, no audit-schema change.**
- **No SSE and no session id in this change**; responses are `application/json`.
- **The 8 MiB body cap is the device `/mcp` cap.** The custom route keeps axum's 2 MiB.
- Rust 1.98.1; the CI commands are in `.github/workflows/ci.yml`'s `device` job and are run through
  `scripts/check.sh`.

## Review Focus

- An anonymous caller with an unknown device id gets `401`, never `404`: the route must not be a
  registry oracle.
- A client sends `initialize` with a protocol version newer than the console speaks: the answer must
  name `2025-06-18` (the version it will use), not echo the client's version and pretend.
- A client sends only `Accept: text/event-stream`: `406`, not a hang.
- A grant for another device, or an expired one: `403` in `denials.jsonl`, and the device channel
  receives nothing.
- A `tools/call` outside the grant: JSON-RPC `-32001` at HTTP 403, one `denied` row in
  `audit.jsonl`, no frame.
- A device that is offline: both `tools/list` and `tools/call` answer the same retryable `503` the
  custom route answers, not a 200 with an empty tool list or a hung request.
- A body over 8 MiB with a valid grant: `413`; without a valid grant: `401` before the body is read.
- A device `error` envelope: HTTP 200 with the body unchanged, because it is a JSON-RPC answer.

---

### Task 1: Split the shared authorize and forward path

**Files:**
- Modify: `device/console/src/http.rs:1133-1315` (the current `proxy_mcp` and its helpers)
- Test: the existing `device/console/tests/console.rs` suite

**Interfaces:**
- Produces:
  - `fn verify_presented(app: &AppState, device_id: &str, grant: &GrantEnvelope) -> Result<(), ApiError>` — signature, device binding, expiry; no logging.
  - `fn ensure_registered(app: &AppState, device_id: &str) -> Result<(), ApiError>` — `unknown_device` (404) or `Ok`.
  - `async fn forward(app: &AppState, device_id: &str, grant: &GrantEnvelope, audit_id: Option<&str>, request: Value, invalid_params_status: StatusCode, filter: Option<&[String]>) -> Response` — tool scope, wait, channel, audit, reply mapping. `invalid_params_status` is 400 on the custom route and 200 on the standard one.
- `proxy_mcp` keeps its order and rows: `ensure_registered` → `deny` on failure, `verify_presented` → `deny` on failure, then `forward`.

- [ ] **Step 1: Run the existing suite to pin the baseline**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console`
Expected: `41 passed` (unit) and `80 passed` (integration).

- [ ] **Step 2: Add `verify_presented` without changing any status or message**

Move the `grants::verify` match out of `proxy_mcp`. Map the three refusals to the existing `ApiError`
codes exactly: `invalid_grant_signature`, `grant_device_mismatch`, `grant_expired` (all 403). The
function does not record anything.

- [ ] **Step 3: Add `ensure_registered`**

Move the `lock(&app.devices).contains(&device_id)` check out of `proxy_mcp` and return
`ApiError::not_found("unknown_device")` when it fails.

- [ ] **Step 4: Add `forward`**

Move the body of `proxy_mcp` from `let request_id = ...` to the final `match answer` unchanged. The
only addition is the filter, after `Ok(Ok(Reply::Result(mut result)))` and before the audit row,
where `is_tools_list` is read from `request` before it is moved into `channels.request`:

```rust
if let Some(allowed) = filter {
    if is_tools_list {
        mcp::filter_tools(&mut result, allowed);
    }
}
```

`forward` does not verify the grant and does not check the registry; its caller has.

- [ ] **Step 5: Make `proxy_mcp` call the three pieces in its existing order**

```rust
async fn proxy_mcp(
    State(app): State<AppState>,
    _: OperatorSession,
    Json(body): Json<McpProxy>,
) -> Response {
    let audit_id = audit_device_id(&body.device_id);
    if let Err(error) = ensure_registered(&app, &body.device_id) {
        return app.deny(audit_id.as_deref(), error).await.into_response();
    }
    if let Err(error) = verify_presented(&app, &body.device_id, &body.grant) {
        return app.deny(audit_id.as_deref(), error).await.into_response();
    }
    forward(
        &app,
        &body.device_id,
        &body.grant,
        audit_id.as_deref(),
        body.request,
        StatusCode::BAD_REQUEST,
        None,
    )
    .await
}
```

- [ ] **Step 6: Run the suite and confirm it is unchanged**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console`
Expected: the same `41 passed` and `80 passed`; no test edits.

- [ ] **Step 7: Commit**

```bash
git add device/console/src/http.rs
git commit -m "console: one authorize-and-forward path for every MCP-shaped route"
```

---

### Task 2: The pure protocol helpers

**Files:**
- Create: `device/console/src/mcp.rs`
- Modify: `device/console/src/lib.rs` (add `mod mcp;`)
- Modify: `device/console/src/hex.rs` (expose arbitrary-length decoding)
- Test: inline `#[cfg(test)]` in `mcp.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn grant_from_headers(headers: &HeaderMap) -> Option<GrantEnvelope>` — parses `Authorization: Bearer <lowercase hex of the envelope JSON>`; anything else is `None`.
  - `pub(crate) fn content_type_is_json(headers: &HeaderMap) -> bool`.
  - `pub(crate) fn accept_allows_json(headers: &HeaderMap) -> bool` — absent `Accept` is allowed; `*/*` and `application/*` are allowed; `text/event-stream` alone is not.
  - `pub(crate) fn initialize_result(device_id: &str, version: &str) -> Value`.
  - `pub(crate) fn filter_tools(response: &mut Value, allowed: &[String])` — retains only tools whose `name` is in `allowed` in `response["result"]["tools"]`; a missing or non-array `tools` is a no-op.
  - `pub(crate) fn jsonrpc_result(id: Value, result: Value) -> Value`.
  - `pub(crate) fn jsonrpc_error_body(id: Value, code: i64, message: &str) -> Value`.
- Consumes: `summrise_core::grant::GrantEnvelope`; `crate::hex::decode_bytes`.

- [ ] **Step 1: Write the failing unit tests**

```rust
#[test]
fn a_grant_header_round_trips_through_hex() {
    let envelope = /* a hand-built GrantEnvelope */;
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", crate::hex::encode(&bytes)).parse().unwrap(),
    );
    assert_eq!(grant_from_headers(&headers), Some(envelope));
}

#[test]
fn a_wrong_scheme_or_odd_hex_is_not_a_grant() { /* "Basic {hex}", "Bearer 0", "Bearer zz" */ }

#[test]
fn only_the_allowed_tools_survive_the_filter() {
    let mut response = json!({ "result": { "tools": [
        { "name": "files.read" }, { "name": "files.write" }
    ]}});
    filter_tools(&mut response, &["files.read".to_owned()]);
    assert_eq!(response["result"]["tools"], json!([{ "name": "files.read" }]));
}

#[test]
fn a_client_that_wants_only_sse_is_not_served_json() { /* Accept: text/event-stream -> false */ }

#[test]
fn the_initialize_answer_names_the_device_and_the_console() { /* version, serverInfo, instructions */ }
```

- [ ] **Step 2: Run the unit tests to prove they fail**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --lib`
Expected: FAIL — `mcp` does not exist / the functions are missing.

- [ ] **Step 3: Implement the module and expose the hex decoder**

In `hex.rs`, add `pub(crate) fn decode_bytes(text: &str) -> Option<Vec<u8>>` that calls the existing
private decoder. In `mcp.rs`, implement the five functions with the exact signatures above.
`initialize_result` returns:

```json
{
  "protocolVersion": "2025-06-18",
  "capabilities": { "tools": { "listChanged": false } },
  "serverInfo": { "name": "summrise-console", "version": "<version>" },
  "instructions": "This endpoint reaches one Summrise device. The tools it lists are the grant's tools; every call is audited."
}
```

- [ ] **Step 4: Run the unit tests to prove they pass**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --lib`
Expected: the new tests pass and the existing 41 still pass.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/mcp.rs device/console/src/hex.rs device/console/src/lib.rs
git commit -m "console: the standard MCP end of the wire, as pure functions"
```

---

### Task 3: The route, the grant check and the local methods

**Files:**
- Modify: `device/console/src/http.rs` (the router and the new handler)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Consumes: `mcp::grant_from_headers`, `mcp::content_type_is_json`, `mcp::accept_allows_json`,
  `mcp::initialize_result`, `mcp::jsonrpc_result`, `mcp::jsonrpc_error_body`; `verify_presented`,
  `ensure_registered` (Task 1).
- Produces: `POST /mcp/:id`.

- [ ] **Step 1: Write the failing tests**

Add the test helper first:

```rust
fn hex_encode(bytes: &[u8]) -> String { /* lowercase hex */ }

fn post_standard(device_id: &str, grant: &Value, body: Value) -> Request<Body> {
    let grant = serde_json::to_vec(grant).expect("the grant serializes");
    Request::builder()
        .method("POST")
        .uri(format!("/mcp/{device_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {}", hex_encode(&grant)))
        .body(Body::from(body.to_string()))
        .expect("the request builds")
}
```

Then:

```rust
#[tokio::test]
async fn a_standard_client_initializes_with_a_grant() {
    // register a device, mint a 60 s grant, POST initialize
    // assert 200; result.protocolVersion == "2025-06-18";
    // serverInfo.name == "summrise-console"; capabilities.tools.listChanged == false;
    // instructions names the device id.
}

#[tokio::test]
async fn an_initialized_notification_is_accepted_without_a_device_frame() {
    // real TestServer + open channel; POST notifications/initialized
    // assert 202; no device frame within 100 ms.
}

#[tokio::test]
async fn a_missing_or_malformed_grant_is_unauthorized() {
    // no Authorization -> 401 {"error":"unauthorized"}; the denial file has the row.
    // "Bearer zz" -> 401; no audit row.
}

#[tokio::test]
async fn an_unknown_device_without_a_grant_is_unauthorized_not_not_found() {
    // a 64-hex id that was never registered, no grant -> 401 (the registry oracle stays closed).
}

#[tokio::test]
async fn a_valid_grant_for_a_removed_device_is_not_found_and_audited() {
    // register, mint, remove the device (POST /fleet/:id is the operator route), then use the
    // grant: the signature and the device binding still verify, the registry lookup fails -> 404;
    // the audit row is denied/unknown_device.
}

#[tokio::test]
async fn a_bad_expired_or_mismatched_grant_is_forbidden_and_denied() {
    // tamper the signature -> 403 invalid_grant_signature
    // advance the clock 61 s -> 403 grant_expired
    // mint for another device -> 403 grant_device_mismatch
    // every row is in denials.jsonl, none in audit.jsonl, and no frame reaches the device.
}
```

- [ ] **Step 2: Run the tests to prove they fail**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --test console a_standard_client`
Expected: FAIL — 404 from the router (the route does not exist).

- [ ] **Step 3: Implement the handler and the route**

Add `const MAX_STANDARD_MCP_BODY_BYTES: usize = 8 * 1024 * 1024;` next to the other route constants.
Add the route beside the custom one:

```rust
.route("/mcp", post(proxy_mcp))
.route("/mcp/:id", post(standard_mcp))
```

The handler takes the whole request so it can verify the grant before touching the body:

```rust
async fn standard_mcp(
    State(app): State<AppState>,
    Path(device_id): Path<String>,
    request: axum::extract::Request,
) -> Response
```

Its order, with the exact statuses from the spec:

1. `let (parts, body) = request.into_parts();` — keep `parts.headers` and the body.
2. wrong content type → `415 {"error":"unsupported_media_type"}`;
3. `Accept` without JSON → `406 {"error":"not_acceptable"}`;
4. no/malformed grant header → `app.refuse(audit_id.as_deref(), ApiError::unauthorized()).await`
   (the denial log);
5. `verify_presented` failure → `app.refuse(audit_id.as_deref(), error).await` (the denial log);
6. `ensure_registered` failure → `app.deny(audit_id.as_deref(), error).await` (the audit log);
7. read the body: `axum::body::to_bytes(body, MAX_STANDARD_MCP_BODY_BYTES)`; an over-cap body is
   `413 {"error":"payload_too_large"}`;
8. parse: a non-object or missing `jsonrpc`/`method` → `400` `-32600`; malformed JSON → `400`
   `-32700`;
9. no `id` → `202 Accepted` (a notification; `notifications/initialized` is the one with meaning, the
   rest are dropped);
10. `initialize` → local `jsonrpc_result(id, mcp::initialize_result(&device_id, &app.version))`;
11. `ping` → local `jsonrpc_result(id, json!({}))`;
12. anything else → `200` `-32601` (Task 4 adds `tools/list` and `tools/call`).

- [ ] **Step 4: Run the tests and the existing suite**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --test console`
Expected: the new tests pass; all 80 existing integration tests still pass.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: a standard MCP endpoint per device, behind the same grant"
```

---

### Task 4: Forward `tools/list` and `tools/call`

**Files:**
- Modify: `device/console/src/http.rs` (the two match arms)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Consumes: `forward` (Task 1), `mcp::filter_tools` (Task 2).
- Produces: the two methods the standard client actually uses.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn tools_list_is_filtered_to_the_grants_tools() {
    // channel open; grant allows files.read only
    // body: {"jsonrpc":"2.0","id":1,"method":"tools/list"}
    // device receives frame; assert frame["mcp"] == the request
    // answer: {"jsonrpc":"2.0","id":1,"result":{"tools":[
    //   {"name":"files.read"},{"name":"files.write"}]}}
    // assert 200; body["result"]["tools"] == [{"name":"files.read"}]
}

#[tokio::test]
async fn a_tool_call_round_trips_through_the_channel() {
    // grant allows files.read; call it; device answer is a JSON-RPC result
    // assert 200 and body == the device's answer
}

#[tokio::test]
async fn a_tool_outside_the_grant_is_refused_and_the_device_sees_nothing() {
    // grant allows files.read; call files.write
    // assert 403, error.code == -32001, error.message contains "not permitted"
    // assert no frame arrives within 100 ms; the audit row is denied/files.write.
}
```

- [ ] **Step 2: Run the tests to prove they fail**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --test console tools_list_is_filtered`
Expected: FAIL — 200 `-32601` (method not found) before the arms exist.

- [ ] **Step 3: Add the two arms**

```rust
"tools/list" => {
    forward(
        &app,
        &device_id,
        &grant,
        audit_id.as_deref(),
        request,
        StatusCode::OK,
        Some(&grant.grant.tools),
    )
    .await
}
"tools/call" => {
    forward(
        &app,
        &device_id,
        &grant,
        audit_id.as_deref(),
        request,
        StatusCode::OK,
        None,
    )
    .await
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --test console`
Expected: the new tests pass; the 80 existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: the standard endpoint lists the grant's tools and calls them"
```

---

### Task 5: Media types, bounds and malformed requests

**Files:**
- Modify: `device/console/src/http.rs` (only if a test exposes a wrong status)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: the refusal contract the spec's table names.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn a_wrong_content_type_is_refused() { /* text/plain -> 415 */ }

#[tokio::test]
async fn an_accept_header_without_json_is_not_acceptable() { /* text/event-stream -> 406 */ }

#[tokio::test]
async fn a_malformed_body_is_a_parse_error() { /* "{" -> 400, error.code == -32700 */ }

#[tokio::test]
async fn a_batch_is_an_invalid_request() { /* [] -> 400, error.code == -32600 */ }

#[tokio::test]
async fn an_unknown_method_is_method_not_found() { /* resources/list -> 200, -32601 */ }

#[tokio::test]
async fn a_body_over_the_cap_is_refused() {
    // a 9 MiB body with a valid grant -> 413
}

#[tokio::test]
async fn an_offline_device_is_a_retryable_503() {
    // register, mint, no channel; tools/list -> 503, body["error"] == "device reconnecting",
    // and the audit row is denied.
}

#[tokio::test]
async fn a_standard_call_is_one_audit_line_without_its_payload() {
    // tools/call with a unique argument marker; read <data>/audit.jsonl
    // assert exactly one kind == "mcp", tool == "files.read", and no marker in the file
}
```

- [ ] **Step 2: Run the tests to prove they fail where the code is missing**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console --test console a_wrong_content_type`
Expected: PASS if Task 3 implemented the checks; FAIL for any status that does not match the spec.
Fix the handler, not the test.

- [ ] **Step 3: Make the statuses match the spec exactly**

The reference table is in `docs/superpowers/specs/2026-10-26-console-standard-mcp-design.md` § "Bounds
and errors". Do not add a new error shape; reuse `ApiError`, `jsonrpc_error_body` and `failure_for`.

- [ ] **Step 4: Run the suite**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console`
Expected: every test passes.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: the standard endpoint refuses what it cannot serve, by name"
```

---

### Task 6: The client document

**Files:**
- Create: `docs/CONNECT-MCP.md`
- Modify: `README.md` (the documents list)

**Interfaces:**
- Consumes: the `/mcp/<id>` route and the hex grant header.
- Produces: the operator's copy-paste path.

- [ ] **Step 1: Write the document**

It must contain, in this order:

1. what the endpoint is (one device; the grant is the credential; the page's `/mcp` is not this);
2. minting a grant with `POST /grants` (the exact body and the session header), and hex-encoding its
   JSON;
3. a DSH `streamable-http` configuration block with the URL, the `Authorization: Bearer <hex>` header,
   and the two `CF-Access-*` headers only when the service-token policy is chosen;
4. the Access step: one application for `console.example.com/mcp/*`, **bypass** or service token, and
   never the operator application (an Access service token is a full operator in this console);
5. the security facts: TTL 60 s–7 days, tool scope, `allow_destructive`, no revocation before expiry,
   never put the grant in a URL;
6. the limits: one device per endpoint, no SSE, `tools/list` needs the device online, the 8 MiB body
   cap.

- [ ] **Step 2: Link it from the README's document list**

One line beside the design documents.

- [ ] **Step 3: Commit**

```bash
git add docs/CONNECT-MCP.md README.md
git commit -m "docs: how an AI client reaches one device through the console"
```

---

### Task 7: Verify and open the PR

**Files:** none.

- [ ] **Step 1: Format**

Run: `cargo fmt --manifest-path device/Cargo.toml --all -- --check`
Expected: exit 0.

- [ ] **Step 2: Lints**

Run: `cargo clippy --manifest-path device/Cargo.toml -p summrise-console --all-targets -- -D warnings`
Expected: exit 0.

- [ ] **Step 3: The console suite**

Run: `cargo test --manifest-path device/Cargo.toml -p summrise-console`
Expected: `41 passed` unit, `80 + N passed` integration.

- [ ] **Step 4: The portable CI job**

Run: `scripts/check.sh --fast`
Expected: format and lints for every crate, exit 0. (The full test run is CI's job; the console is the
only crate this change touches.)

- [ ] **Step 5: Push the branch and open the PR**

Use the GitHub API path the repository already uses (a tarball of `main` + blobs → tree → commit →
ref → PR; direct `git push` is not available through the proxy). The PR body carries:

- what changed and why (the parity-sweep gap, the one-route-per-audience rule, the audit/denial split);
- the exact test counts and the commands;
- the two things not done (no SSE, no session id) and why;
- the canary plan (the official `StreamableHTTPClientTransport` against a local console, then the
  deployed console), and the Access application the deployment needs.

- [ ] **Step 6: Watch CI**

Wait for the `device` job on the PR head; a red step is fixed in a new commit on the same branch, not
by editing the PR description.

---

## Self-Review

**Spec coverage:** § "The shape" → Tasks 3–4; § "The credential" → Tasks 2–3; § "Authentication,
ordering, and which log" → Task 1 (shared pieces) and Task 3 (ordering/logging tests); § "The protocol"
table → Tasks 3–4; § "The one authorization path" → Task 1; § "What does not change" → Task 1's
regression and the global constraints; § "Bounds and errors" → Task 5; § "Tests" → Tasks 3–5;
§ "Rollout" → Tasks 6–7.

**Type consistency:** `verify_presented` and `ensure_registered` return `Result<(), ApiError>`;
`forward` takes `Option<&[String]>` and returns `Response`; `grant_from_headers` returns
`Option<GrantEnvelope>`; `filter_tools` takes `&mut Value` and `&[String]`; `jsonrpc_result` and
`jsonrpc_error_body` return `Value`.

**Review Focus:** each line has a test: unknown-id 401 (Task 3), protocol version (Task 3), SSE 406
(Task 5), wrong/expired grant denials (Task 3), out-of-scope audit (Task 4), offline 503 (Task 5),
body cap (Task 5), device error (Task 4's round trip).

**Proportion:** the plan names the exact tests and statuses; it does not reproduce the handler's body,
which the tests and the spec determine.
