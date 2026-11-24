# The Aggregate MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One standard MCP endpoint (`POST /mcp/all`) that reaches every device an aggregate token names, with a required `device` argument and an ordinary per-device grant per call.

**Architecture:** Extend the console's token record with an optional device list, create the token through `/tokens`, then add the aggregate route beside `/mcp/:id`. The route resolves the device, strips the argument, mints a 60-second grant for that device and reuses the existing channel proxy. Device protocol, grant verification and destructive consent are unchanged.

**Tech Stack:** Rust (axum, serde_json, tokio), the console's existing token/device/channel modules, React + TypeScript for the operator UI.

**Spec:** `docs/superpowers/specs/2026-10-27-aggregate-mcp.md`

## Global Constraints

- The repository is written in English; talk to the operator in Chinese.
- No change to the device protocol, the grant envelope, or `summrise-core`'s tool registry.
- Existing device tokens must deserialize and behave exactly as before; the new field is optional and skipped when absent.
- Every forwarded line audits the **actual** device id and the token id; pre-authentication refusals go to the denial log.
- A credential must never widen its scope: the grant is signed from the stored record, not from the request.
- A `tools/list` with no reachable allowed device is a `503` with a retry hint, never an empty tool list.
- The target test commands are `cargo test -p summrise-console`, `cargo clippy -p summrise-console --all-targets -- -D warnings`, `pnpm test` in `device/console/web`, and the Windows build with `cargo xwin build -p summrise-console --target x86_64-pc-windows-msvc --release`.
- Follow the existing test style in `device/console/tests/console.rs` and the unit tests inside each module.

## Review Focus

1. A device name shared by two allowed devices must be refused as ambiguous, not resolved by list order.
2. The `device` argument must be removed before the frame leaves the console; a fake device asserting its arguments sees no `device` key.
3. An aggregate token presented to `/mcp/<id>` must be refused; a device token presented to `/mcp/all` must be refused.
4. An aggregate token whose device list gets a device removed later must stay valid for the remaining devices and remain inert for the removed one.
5. A `tools/list` when every allowed device is offline must answer `503` with a retry hint, not a `200` with an empty list.
6. The audit line for an aggregate call must carry the resolved device id, not the aggregate token's empty `device_id`.

---

### Task 1: The token scope

**Files:**
- Modify: `device/console/src/tokens.rs`
- Test: `device/console/src/tokens.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  ```rust
  pub(crate) enum TokenScope {
      Device(String),
      Devices(Vec<String>),
  }

  pub(crate) struct Draft {
      pub name: String,
      pub scope: TokenScope,
      pub tools: Vec<String>,
      pub allow_destructive: bool,
      pub expires_in_s: Option<u64>,
  }

  // Token and TokenMeta gain:
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device_ids: Option<Vec<String>>,

  pub(crate) enum TokenError {
      // existing variants ...
      EmptyDevices,
      DuplicateDevice,
  }

  pub(crate) fn create(
      &mut self,
      draft: Draft,
      known_devices: impl Fn(&str) -> bool,
      now_ms: u64,
  ) -> Result<(TokenMeta, String), TokenError>
  ```

- [ ] **Step 1: Write the failing tests**

In `tokens.rs`'s test module (which already has `tempdir` and a `draft` helper), add:

```rust
fn aggregate(ids: &[&str]) -> Draft {
    Draft {
        name: "all".into(),
        scope: TokenScope::Devices(ids.iter().map(|id| (*id).to_owned()).collect()),
        tools: vec!["files.read".into()],
        allow_destructive: false,
        expires_in_s: None,
    }
}

#[test]
fn an_aggregate_token_records_every_device_and_never_a_single_device() {
    let data = tempdir("aggregate-scope");
    let mut tokens = Tokens::load(&data).expect("an empty store loads");
    let (meta, secret) = tokens
        .create(aggregate(&["a", "b", "c"]), |_| true, 1_000)
        .expect("an aggregate token is mintable");
    assert!(is_token(&secret));
    assert_eq!(meta.device_id, "");
    assert_eq!(meta.device_ids, Some(vec!["a".into(), "b".into(), "c".into()]));
}

#[test]
fn an_aggregate_token_needs_one_known_unique_device() {
    let data = tempdir("aggregate-validate");
    let mut tokens = Tokens::load(&data).expect("an empty store loads");
    assert!(matches!(
        tokens.create(aggregate(&[]), |_| true, 0),
        Err(TokenError::EmptyDevices)
    ));
    assert!(matches!(
        tokens.create(aggregate(&["a", "a"]), |_| true, 0),
        Err(TokenError::DuplicateDevice)
    ));
    assert!(matches!(
        tokens.create(aggregate(&["z"]), |_| false, 0),
        Err(TokenError::UnknownDevice)
    ));
}

#[test]
fn rotating_an_aggregate_token_keeps_its_scope() {
    let data = tempdir("aggregate-rotate");
    let mut tokens = Tokens::load(&data).expect("an empty store loads");
    let (meta, _) = tokens
        .create(aggregate(&["a", "b"]), |_| true, 0)
        .expect("an aggregate token is mintable");
    let (rotated, _) = tokens.rotate(&meta.id, 1).expect("rotation succeeds");
    assert_eq!(rotated.device_ids, Some(vec!["a".into(), "b".into()]));
    assert_eq!(rotated.device_id, "");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p summrise-console tokens:: -- --nocapture`
Expected: compile errors for `TokenScope`, `Draft.scope`, `Tokens::empty_for_test` (add a test-only constructor if absent, or use `Tokens::load` on a temp dir), and the new error variants.

- [ ] **Step 3: Implement the scope**

- Add `device_ids: Option<Vec<String>>` to `Token` and `TokenMeta`; `Token::meta` copies it.
- Replace `Draft.device_id` with `Draft.scope`.
- In `create`, build `(device_id, device_ids)` from the scope, validate the list (non-empty, no duplicates, every id in `known_devices`), then store both.
- Map the errors in `http.rs`'s `create_token` (Task 2).
- Update every existing `Draft { device_id: ... }` construction in tests and `http.rs` to `scope: TokenScope::Device(...)`.
- Update `Tokens::create` call sites to pass a `Fn(&str) -> bool`; the HTTP handler resolves the registry once, and tests pass `|_| true` or a narrower predicate.
- `rotate` clones the record, so it keeps both fields.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p summrise-console tokens::`
Expected: all token tests pass, including the existing device-token tests.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/tokens.rs
git commit -m "console: an aggregate token names a list of devices"
```

---

### Task 2: The operator API

**Files:**
- Modify: `device/console/src/http.rs` (the `create_token` handler and its `CreateToken`)
- Test: `device/console/tests/console.rs` (the token section near `a_created_token_is_shown_once...`)

**Interfaces:**
- Consumes: `TokenScope`, `Tokens::create(draft, known_devices, now)` from Task 1.
- Produces: `POST /tokens` accepts exactly one of:
  ```json
  { "name": "x", "device_id": "<id>", "tools": ["files.read"] }
  { "name": "x", "device_ids": ["<id>", "<id>"], "tools": ["files.read"] }
  ```
  and answers with the existing `CreatedToken` shape, whose `meta` now carries `device_ids`.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn an_aggregate_token_is_created_and_listed_without_a_secret() {
    let fixture = Fixture::new("aggregate-token-create");
    let session = fixture.authenticate().await;
    let (a, _) = fixture.register(&signing_key(0x70)).await;
    let (b, _) = fixture.register(&signing_key(0x71)).await;
    let (status, created) = fixture
        .create_aggregate_token(&session, "all", &[a.clone(), b.clone()], &["files.read"])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["device_ids"], json!([a.clone(), b.clone()]));
    assert_eq!(created["device_id"], json!(""));
    assert!(created["token"].as_str().unwrap().starts_with("srm_"));

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list[0]["device_ids"], json!([a, b]));
    assert!(list[0].get("token").is_none());
}

#[tokio::test]
async fn a_token_create_requires_exactly_one_scope() {
    let fixture = Fixture::new("aggregate-token-scope");
    let session = fixture.authenticate().await;
    let (device, _) = fixture.register(&signing_key(0x72)).await;
    let body = json!({ "name": "x", "tools": ["files.read"] });
    let (status, _) = send(fixture.app(), post_json_bearer("/tokens", &session, body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let body = json!({
        "name": "x", "device_id": device, "device_ids": [device], "tools": ["files.read"]
    });
    let (status, _) = send(fixture.app(), post_json_bearer("/tokens", &session, body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
```

Add this helper next to the existing token helpers in `impl Fixture`:

```rust
async fn create_aggregate_token(
    &self,
    session: &str,
    name: &str,
    device_ids: &[String],
    tools: &[&str],
) -> (StatusCode, Value) {
    send(
        self.app(),
        post_json_bearer(
            "/tokens",
            session,
            json!({ "name": name, "device_ids": device_ids, "tools": tools,
                    "allow_destructive": false }),
        ),
    )
    .await
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p summrise-console --test console aggregate_token -- --nocapture`
Expected: the create request is rejected because `CreateToken` still requires `device_id`.

- [ ] **Step 3: Implement the API**

- `CreateToken`:
  ```rust
  struct CreateToken {
      name: String,
      #[serde(default)]
      device_id: Option<String>,
      #[serde(default)]
      device_ids: Option<Vec<String>>,
      tools: Vec<String>,
      #[serde(default)]
      allow_destructive: bool,
      #[serde(default)]
      expires_in_s: Option<u64>,
  }
  ```
- Exactly one scope or `ApiError::bad_request("device_required")` / `("ambiguous_scope")`.
- Build `TokenScope::Device(id)` or `TokenScope::Devices(ids)`.
- Build the known set from `lock(&app.devices).list()`; pass it to `Tokens::create`.
- Map `EmptyDevices`/`DuplicateDevice` to `bad_request("empty_devices")`/`("duplicate_device")`; `UnknownDevice` stays `not_found("unknown_device")`.
- The audit line: `audit_device_id(&request.device_id)` only for a device scope; None for an aggregate; keep the existing `consent_detail`.
- Update the test fixture helpers if a token-create helper assumes `device_id`; keep the device-token tests passing.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p summrise-console --test console token`
Expected: old and new token tests pass.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: mint a token for several devices"
```

---

### Task 3: The aggregate protocol helpers

**Files:**
- Modify: `device/console/src/mcp.rs`
- Test: `device/console/src/mcp.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: `serde_json::Value`.
- Produces:
  ```rust
  pub(crate) fn aggregate_initialize_result(devices: &[(String, String)], version: &str) -> Value
  pub(crate) fn add_device_argument(result: &mut Value, devices: &[(String, String)])
  ```
  `devices` is `(id, name)` sorted by id. `add_device_argument` requires `result["tools"]` to be an array; every tool's `inputSchema` becomes an object with `properties.device` and `required` containing `device`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_aggregate_initialize_names_every_device() {
    let value = aggregate_initialize_result(
        &[("a".into(), "ALPHA".into()), ("b".into(), "BETA".into())],
        "0.3.43",
    );
    assert_eq!(value["serverInfo"]["name"], "summrise-console");
    let text = value["instructions"].as_str().unwrap();
    assert!(text.contains("a (ALPHA)"));
    assert!(text.contains("b (BETA)"));
    assert!(text.contains("device"));
}

#[test]
fn every_tool_gets_a_required_device_argument() {
    let mut result = json!({"tools": [
        {"name": "files.read", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}
    ]});
    add_device_argument(&mut result, &[("a".into(), "ALPHA".into())]);
    let schema = &result["tools"][0]["inputSchema"];
    assert_eq!(schema["properties"]["device"]["type"], "string");
    assert!(schema["required"].as_array().unwrap().iter().any(|v| v == "device"));
    assert_eq!(schema["properties"]["path"]["type"], "string");
}

#[test]
fn injection_tolerates_a_tool_without_a_schema() {
    let mut result = json!({"tools": [{"name": "x"}]});
    add_device_argument(&mut result, &[("a".into(), "ALPHA".into())]);
    assert_eq!(result["tools"][0]["inputSchema"]["properties"]["device"]["type"], "string");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p summrise-console mcp::`
Expected: `aggregate_initialize_result` and `add_device_argument` do not exist.

- [ ] **Step 3: Implement the helpers**

- `aggregate_initialize_result` returns the MCP initialize shape: `protocolVersion` `2025-06-18`, `capabilities.tools.listChanged = false`, `serverInfo { name: "summrise-console", version }`, and `instructions` containing the `id (name)` list and the sentence "Every tool takes a required `device` argument: the device id, or a unique device name."
- `add_device_argument` ensures `inputSchema.type = "object"`, `properties` and `required` objects/arrays exist, and sets:
  ```json
  "device": {
    "type": "string",
    "description": "The device to act on: an id, or a name unique in this token's scope."
  }
  ```
  and adds `device` to `required` (deduplicated). It preserves the tool's existing properties and required entries and does not add an `enum`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p summrise-console mcp::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add device/console/src/mcp.rs
git commit -m "console: the aggregate tool surface names its devices"
```

---

### Task 4: The aggregate route

**Files:**
- Modify: `device/console/src/http.rs` (route table, `standard_mcp`'s neighbours, `mint_*`, `forward`)
- Test: `device/console/src/http.rs` (unit) and `device/console/tests/console.rs` (integration)

**Interfaces:**
- Consumes: Tasks 1-3; `Channels::online`, `DeviceRegistry::list`, `grants::sign`.
- Produces:
  ```rust
  // POST /mcp/all
  async fn aggregate_mcp(State(app): State<AppState>, request: axum::extract::Request) -> Response

  enum DeviceResolution {
      Resolved(String),
      OutOfScope,
      Ambiguous,
  }

  fn resolve_aggregate_device(
      allowed: &[String],
      devices: &[(String, String)], // every registered device as (id, name)
      requested: &str,
  ) -> DeviceResolution

  /// Remove `params.arguments.device` and return it; `Err(())` when it is missing or not a string.
  fn take_aggregate_device(request: &mut Value) -> Result<String, ()>

  fn mint_aggregate_grant(app: &AppState, device_id: &str, token: &str) -> Result<MintedGrant, TokenRefusal>

  /// The part of `forward` that produces a reply, split out so the aggregate can add the
  /// `device` property to a `tools/list` answer before it becomes a response.
  async fn proxy_reply(
      app: &AppState,
      device_id: &str,
      grant: &GrantEnvelope,
      token_id: Option<&str>,
      request: Value,
      invalid_params_status: StatusCode,
      filter: Option<&[String]>,
  ) -> Result<Reply, Box<Response>>
  ```

- [ ] **Step 1: Write the failing unit tests**

In `http.rs`'s test module:

```rust
#[test]
fn a_device_name_resolves_only_within_the_scope() {
    let devices = vec![
        ("a".to_string(), "ALPHA".to_string()),
        ("b".to_string(), "BETA".to_string()),
        ("c".to_string(), "GAMMA".to_string()),
    ];
    let allowed = vec!["a".to_string(), "b".to_string()];
    assert!(matches!(
        resolve_aggregate_device(&allowed, &devices, "b"),
        DeviceResolution::Resolved(id) if id == "b"
    ));
    assert!(matches!(
        resolve_aggregate_device(&allowed, &devices, "BETA"),
        DeviceResolution::Resolved(id) if id == "b"
    ));
    assert!(matches!(
        resolve_aggregate_device(&allowed, &devices, "GAMMA"),
        DeviceResolution::OutOfScope
    ));
    assert!(matches!(
        resolve_aggregate_device(&allowed, &devices, "nope"),
        DeviceResolution::OutOfScope
    ));
}

#[test]
fn a_shared_device_name_is_ambiguous() {
    let devices = vec![
        ("a".to_string(), "SAME".to_string()),
        ("b".to_string(), "SAME".to_string()),
    ];
    assert!(matches!(
        resolve_aggregate_device(&["a".into(), "b".into()], &devices, "SAME"),
        DeviceResolution::Ambiguous
    ));
}

#[test]
fn the_aggregate_strips_the_device_before_forwarding() {
    let mut request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": "a"}}});
    assert_eq!(take_aggregate_device(&mut request), Ok("a".to_string()));
    assert!(request["params"]["arguments"].get("device").is_none());
    assert_eq!(request["params"]["arguments"]["path"], "C:\\x");
}
```

- [ ] **Step 2: Run the unit tests to verify they fail**

Run: `cargo test -p summrise-console http::`
Expected: the new functions do not exist.

- [ ] **Step 3: Implement the resolution, the route and the grant**

- Route: add `.route("/mcp/all", post(aggregate_mcp))` beside `/mcp/:id`.
- `resolve_aggregate_device`:
  1. exact id in `allowed` → `Resolved(id)`;
  2. exact name among the **allowed** registered devices → exactly one is `Resolved(id)`, more than one is `Ambiguous`, none is `OutOfScope` (a name that exists only outside the scope is a scope refusal, not a resolution).
- `take_aggregate_device(&mut Value) -> Result<String, ()>` removes `params.arguments.device` and returns it when it is a non-empty string; missing/wrong type is `Err(())`.
- `mint_aggregate_grant`: reuse the lookup/expiry part of `mint_token_grant` (extract `live_token(app, token) -> Result<Token, TokenRefusal>` if that keeps both readable); require `device_ids: Some(ids)` containing `device_id`, then sign with the token's tools/consent and mark used.
- `mint_token_grant` gains a check that the record is **not** an aggregate (`device_ids.is_none()`); an aggregate token on `/mcp/:id` is `ForeignDevice` (401).
- `proxy_reply`: move the body of `forward` up to the `Ok(Ok(reply))` audit into it; `forward` becomes a thin wrapper. Keep the audit and filter behaviour exactly.
- `aggregate_mcp`:
  - Same content-type/accept checks as `standard_mcp`.
  - Bearer must be `BearerKind::Token`; a grant is `401` (`refuse_as` "aggregate_requires_token").
  - Parse JSON-RPC exactly as `standard_mcp`; notifications → 202.
  - `initialize` → `aggregate_initialize_result` with the token's `(id, name)` list (registered devices only; names from `DeviceRegistry` or `channels.name`).
  - `ping` → `{}`.
  - `tools/list`: token must be aggregate; choose the first allowed id for which `app.channels.online(id)`; if none, `failure_for(RequestError::Offline).into_response()`; mint a grant for it; `proxy_reply(..., Some(&token.tools))`; on `Reply::Result(mut value)` call `add_device_argument(&mut value, &devices)` and return 200; `Reply::Error` returns 200 as the standard route does.
  - `tools/call`: `take_aggregate_device`; `Err` → 400 JSON-RPC -32602 "tools/call requires params.arguments.device"; resolve; `OutOfScope`/`Ambiguous` → 403 JSON-RPC -32001 "device not permitted by token" with `data: {"device": requested}`, audited with `record_facts(kind:"denied", device_id: requested, status: FORBIDDEN, detail:"device_not_permitted", token_id: Some(id))`; `Resolved(id)` → `ensure_registered`, mint, `forward(..., None)`.
  - Other methods → -32601.
  - The aggregate's `tools/list` audit is the existing `mcp` line for the chosen device.

- [ ] **Step 4: Write the failing integration tests**

Add these helpers beside the standard-MCP tests:

```rust
fn aggregate_request(token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/mcp/all")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        .body(Body::from(body.to_string()))
        .expect("the aggregate request builds")
}

async fn aggregate_token_for(
    fixture: &Fixture,
    session: &str,
    ids: &[String],
    tools: &[&str],
) -> (String, String) {
    let (status, created) = fixture.create_aggregate_token(session, "aggregate", ids, tools).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (
        created["id"].as_str().expect("a token id").to_owned(),
        created["token"].as_str().expect("the one-time secret").to_owned(),
    )
}
```

Then:

```rust
#[tokio::test]
async fn an_aggregate_token_lists_tools_with_a_required_device() {
    let fixture = Fixture::new("aggregate-list");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _device_token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xa7)).await;
    let session = fixture.authenticate().await;
    let (_id, token) = aggregate_token_for(&fixture, &session, &[device_id], &["files.read"]).await;

    let http = send(
        fixture.app(),
        aggregate_request(&token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"]["method"], json!("tools/list"));
        answer(&mut device, &frame, json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [{
            "name": "files.read",
            "inputSchema": {"type": "object", "properties": {}, "required": []}
        }]}}))
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    let tool = &body["result"]["tools"][0];
    assert_eq!(tool["name"], "files.read");
    assert!(tool["inputSchema"]["required"].as_array().unwrap().iter().any(|v| v == "device"));
}

#[tokio::test]
async fn an_aggregate_call_reaches_the_named_device_and_audits_it() {
    let fixture = Fixture::new("aggregate-call");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xa8)).await;
    let (b, _tb, mut db) = connected_device(&fixture, &server, &signing_key(0xa9)).await;
    let session = fixture.authenticate().await;
    let (token_id, token) = aggregate_token_for(
        &fixture, &session, &[a.clone(), b.clone()], &["files.read"],
    ).await;

    let http = send(
        fixture.app(),
        aggregate_request(&token, json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": b.clone()}}
        })),
    );
    let device_side = async {
        let frame = next_json(&mut db).await;
        assert!(frame["mcp"]["params"]["arguments"].get("device").is_none());
        assert_eq!(frame["mcp"]["params"]["arguments"]["path"], json!("C:\\x"));
        answer(&mut db, &frame, json!({"jsonrpc": "2.0", "id": 2,
            "result": {"content": [{"type": "text", "text": "ok"}]}})).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("mcp")
            && line["device_id"] == json!(b)
            && line["token_id"] == json!(token_id)),
        "{audit:?}"
    );
}

#[tokio::test]
async fn an_aggregate_token_cannot_reach_another_device() {
    let fixture = Fixture::new("aggregate-scope");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xaa)).await;
    let (b, _tb, _db) = connected_device(&fixture, &server, &signing_key(0xab)).await;
    let (c, _tc, _dc) = connected_device(&fixture, &server, &signing_key(0xac)).await;
    let session = fixture.authenticate().await;
    let (_id, token) = aggregate_token_for(&fixture, &session, &[a, b], &["files.read"]).await;

    let (status, _) = send(
        fixture.app(),
        aggregate_request(&token, json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": c}}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_device_token_is_refused_on_the_aggregate_route() {
    let fixture = Fixture::new("aggregate-device-token");
    let session = fixture.authenticate().await;
    let (device_id, _) = fixture.register(&signing_key(0xad)).await;
    let (_, created) = fixture
        .create_token(&session, &device_id, "one", &["files.read"], false)
        .await;
    let token = created["token"].as_str().expect("the secret").to_owned();

    let (status, _) = send(
        fixture.app(),
        aggregate_request(&token, json!({"jsonrpc": "2.0", "id": 4, "method": "ping"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_aggregate_token_is_refused_on_a_device_route() {
    let fixture = Fixture::new("aggregate-on-device");
    let session = fixture.authenticate().await;
    let (device_id, _) = fixture.register(&signing_key(0xae)).await;
    let (_id, token) = aggregate_token_for(&fixture, &session, &[device_id.clone()], &["files.read"]).await;

    let (status, _) = send(
        fixture.app(),
        post_token(&device_id, &token, json!({"jsonrpc": "2.0", "id": 5, "method": "ping"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_empty_aggregate_list_is_a_retryable_503() {
    let fixture = Fixture::new("aggregate-offline");
    let session = fixture.authenticate().await;
    let (device_id, _) = fixture.register(&signing_key(0xaf)).await;
    let (_id, token) = aggregate_token_for(&fixture, &session, &[device_id], &["files.read"]).await;

    let (status, _) = send(
        fixture.app(),
        aggregate_request(&token, json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn an_aggregate_token_initializes_without_a_device() {
    let fixture = Fixture::new("aggregate-initialize");
    let session = fixture.authenticate().await;
    let (device_id, _) = fixture.register(&signing_key(0xb0)).await;
    let (_id, token) = aggregate_token_for(&fixture, &session, &[device_id.clone()], &["files.read"]).await;

    let (status, body) = send(
        fixture.app(),
        aggregate_request(&token, json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["serverInfo"]["name"], "summrise-console");
    assert!(body["result"]["instructions"].as_str().unwrap().contains(&device_id));
}
```

- [ ] **Step 5: Run the integration tests to verify they fail**

Run: `cargo test -p summrise-console --test console aggregate`
Expected: the helpers/route are missing or the assertions fail.

- [ ] **Step 6: Implement the route until the tests pass**

Run the same command after the implementation; expected PASS.

- [ ] **Step 7: Commit**

```bash
git add device/console/src/http.rs device/console/tests/console.rs
git commit -m "console: one MCP endpoint that reaches every named device"
```

---

### Task 5: Documentation and the full gate

**Files:**
- Modify: `docs/CONNECT-MCP.md`
- Modify: `device/README.md` (the MCP section)
- Modify: `docs/QUICKSTART.md` (the token section)

**Interfaces:**
- Consumes: the finished endpoint.
- Produces: one documented aggregate entry:
  ```json
  {"mcpServers": {"summrise": {
    "type": "http",
    "url": "https://console.saisi.online/mcp/all",
    "headers": {"Authorization": "Bearer srm_..."}
  }}}
  ```
  and the security sentence: the token's device list is its scope; a call naming a device outside it is refused; every call still gets a per-device sixty-second grant.

- [ ] **Step 1: Update the docs**

- `CONNECT-MCP.md`: add "One endpoint for several devices" before the per-device token section; explain the `device` argument, id-or-unique-name, the scope refusal, and the create request:
  ```http
  POST /tokens
  Authorization: Bearer <operator session>
  {"name": "dsh", "device_ids": ["<id>", "<id>"], "tools": ["..."], "allow_destructive": true}
  ```
  Keep the per-device endpoint as the stricter option.
- `device/README.md`: the console's token section says one device; add the aggregate shape and the `/mcp/all` route.
- `QUICKSTART.md`: the token card text and the fact that an aggregate token is shown once.

- [ ] **Step 2: Run the full gates**

Run:
```bash
cargo fmt --all -- --check
cargo test -p summrise-console
cargo clippy -p summrise-console --all-targets -- -D warnings
cargo xwin check -p summrise-console --target x86_64-pc-windows-msvc --all-targets
(cd device/console/web && pnpm test && pnpm run build)
```
Expected: every command exits 0; the web suite reports all tests passed; the build reports the current bundle (no web change is expected in this plan).

- [ ] **Step 3: Commit**

```bash
git add docs/CONNECT-MCP.md device/README.md docs/QUICKSTART.md
git commit -m "docs: one MCP endpoint for several devices"
```

---

## Follow-up (not this plan)

The per-device `ConnectCard` remains the one-device surface. A fleet-level token form (a card on the Security page that lists aggregate tokens and lets the operator pick several devices) is the next UI step; until it exists, the operator mints an aggregate token through `POST /tokens` with their operator session. The API is the contract; the form is presentation.
