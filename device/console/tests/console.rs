//! The console's HTTP surface, driven in-process: no socket is bound for the HTTP routes and every
//! temporary file lives under this test binary's own directory. The device channel is a real
//! WebSocket, so those tests bind an ephemeral loopback listener and drive it with a fake device.

use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use summrise_console::{AccessConfig, Config, Console, LoginCode, ACCESS_HEADER};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};
use tower::ServiceExt;

const OPERATOR_TOKEN: [u8; 32] = [0x11; 32];
const GRANT_SEED: [u8; 32] = [0x22; 32];

/// The throwaway Access team the unit tests under `src/access.rs` already use: this file signs its
/// own assertions with the same fixture key, so a page test meets the verifier the deployment runs.
const ACCESS_AUDIENCE: &str = "test-audience-tag";
const ACCESS_ISSUER: &str = "https://test-team.cloudflareaccess.com";
const ACCESS_KEY_PEM: &str = include_str!("fixtures/access-test-key.pem");
const ACCESS_JWKS: &str = include_str!("fixtures/access-test-jwks.json");
const START_MS: u64 = 1_000_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const MINUTE_MS: u64 = 60 * 1000;
const ENROLLMENT_TTL_MS: u64 = 15 * MINUTE_MS;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A uniquely named directory under the system temp directory, removed when dropped.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "summrise-console-test-{}-{unique}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the test directory is creatable");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Fixture {
    _dir: TempDir,
    config: Config,
    console: Console,
    clock: Arc<AtomicU64>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        Self::with_options(label, Duration::from_secs(30), None)
    }

    /// A console whose `/mcp` wait is short enough for a test to observe the timeout.
    fn with_timeout(label: &str, mcp_timeout: Duration) -> Self {
        Self::with_options(label, mcp_timeout, None)
    }

    /// A console whose audit rotates after a few lines instead of 8 MiB.
    fn with_audit_cap(label: &str, audit_max_bytes: u64) -> Self {
        Self::with_options(label, Duration::from_secs(30), Some(audit_max_bytes))
    }

    fn with_options(label: &str, mcp_timeout: Duration, audit_max_bytes: Option<u64>) -> Self {
        let dir = TempDir::new(label);
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data directory is creatable");

        let token_file = dir.join("operator.token");
        write_key(&token_file, &OPERATOR_TOKEN);
        let grant_key_file = dir.join("grant.key");
        write_key(&grant_key_file, &GRANT_SEED);

        let config = Config {
            operator_token_file: token_file,
            grant_key_file,
            data,
            bind: std::net::Ipv4Addr::LOCALHOST.into(),
            port: 0,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            access: None,
            mesh_broker: None,
        };
        let clock = Arc::new(AtomicU64::new(START_MS));
        let reader = clock.clone();
        let console = match audit_max_bytes {
            Some(cap) => Console::load_with_clock_timeout_and_audit_cap(
                &config,
                move || reader.load(Ordering::Relaxed),
                mcp_timeout,
                cap,
            ),
            None => Console::load_with_clock_and_timeout(
                &config,
                move || reader.load(Ordering::Relaxed),
                mcp_timeout,
            ),
        }
        .expect("the fixture console loads");
        Self {
            _dir: dir,
            console,
            clock,
            config,
        }
    }

    /// A console with a mesh broker configured: the readiness route's live case.
    fn with_mesh_broker(label: &str, broker: &str) -> Self {
        let mut fixture = Self::new(label);
        let mut config = fixture.config.clone();
        config.mesh_broker = Some(broker.to_owned());
        let reader = fixture.clock.clone();
        fixture.console = Console::load_with_clock(&config, move || reader.load(Ordering::Relaxed))
            .expect("the console reloads with the broker");
        fixture.config = config;
        fixture
    }

    /// A console with Cloudflare Access configured the way the deployment has it: three facts and a
    /// pinned JWKS on disk. The key that signs the tests' assertions is the one the JWKS names.
    fn with_access(label: &str) -> Self {
        let mut fixture = Self::new(label);
        let jwks = fixture._dir.join("access-jwks.json");
        fs::write(&jwks, ACCESS_JWKS).expect("the pinned JWKS is written");
        let mut config = fixture.config.clone();
        config.access = Some(AccessConfig {
            jwks,
            audience: ACCESS_AUDIENCE.to_owned(),
            issuer: ACCESS_ISSUER.to_owned(),
        });
        let reader = fixture.clock.clone();
        fixture.console = Console::load_with_clock(&config, move || reader.load(Ordering::Relaxed))
            .expect("the console reloads with Access");
        fixture.config = config;
        fixture
    }

    fn app(&self) -> Router {
        self.console.router()
    }

    fn now_ms(&self) -> u64 {
        self.clock.load(Ordering::Relaxed)
    }

    fn advance_ms(&self, milliseconds: u64) {
        self.clock.fetch_add(milliseconds, Ordering::Relaxed);
    }

    /// Reload from the same files with the same injected clock: a restart must not move time.
    fn restart(&self) -> Console {
        let reader = self.clock.clone();
        Console::load_with_clock(&self.config, move || reader.load(Ordering::Relaxed))
            .expect("the console reloads from its own files")
    }

    async fn authenticate(&self) -> String {
        authenticate_on(&self.console).await
    }

    /// `POST /enroll`, asserting the operator session is enough and returning the code.
    async fn enroll(&self, session: &str) -> (String, u64) {
        let (status, body) =
            send(self.app(), post_json_bearer("/enroll", session, json!({}))).await;
        assert_eq!(status, StatusCode::OK, "enrollment: {body}");
        let code = body["code"]
            .as_str()
            .expect("an enrollment code is a string")
            .to_owned();
        let expires_at_ms = body["expires_at_ms"]
            .as_u64()
            .expect("an enrollment code has an expiry");
        (code, expires_at_ms)
    }

    /// Register a device end to end: operator session, one-time code, signed registration.
    async fn register(&self, key: &SigningKey) -> (String, String) {
        let session = self.authenticate().await;
        let (code, _) = self.enroll(&session).await;
        let (status, body) = self.register_with_code(key, &code, self.now_ms()).await;
        assert_eq!(status, StatusCode::OK, "registration: {body}");
        let id = body["id"].as_str().expect("an id").to_owned();
        let token = body["token"].as_str().expect("a device token").to_owned();
        (id, token)
    }

    async fn register_with_code(
        &self,
        key: &SigningKey,
        code: &str,
        ts_ms: u64,
    ) -> (StatusCode, Value) {
        send(
            self.app(),
            post_json("/devices", registration_body(key, code, ts_ms)),
        )
        .await
    }

    async fn create_grant(&self, session: &str, device_id: &str, expires_in_s: u64) -> Value {
        self.create_grant_with_tools(session, device_id, &["files.read"], expires_in_s)
            .await
    }

    async fn create_grant_with_tools(
        &self,
        session: &str,
        device_id: &str,
        tools: &[&str],
        expires_in_s: u64,
    ) -> Value {
        self.create_grant_with_flag(session, device_id, tools, expires_in_s, false)
            .await
    }

    async fn create_grant_with_flag(
        &self,
        session: &str,
        device_id: &str,
        tools: &[&str],
        expires_in_s: u64,
        allow_destructive: bool,
    ) -> Value {
        let (status, body) = send(
            self.app(),
            post_json_bearer(
                "/grants",
                session,
                json!({
                    "device_id": device_id,
                    "tools": tools,
                    "expires_in_s": expires_in_s,
                    "allow_destructive": allow_destructive,
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "grant: {body}");
        body
    }

    /// `POST /tokens`: the operator's permanent-token mint, with every field named.
    async fn create_token(
        &self,
        session: &str,
        device_id: &str,
        name: &str,
        tools: &[&str],
        allow_destructive: bool,
    ) -> (StatusCode, Value) {
        self.create_token_with_expiry(session, device_id, name, tools, allow_destructive, None)
            .await
    }

    /// `POST /tokens` with an explicit lifetime, or with the field absent entirely when `None`:
    /// permanence is the body's silence, not a sentinel.
    async fn create_token_with_expiry(
        &self,
        session: &str,
        device_id: &str,
        name: &str,
        tools: &[&str],
        allow_destructive: bool,
        expires_in_s: Option<u64>,
    ) -> (StatusCode, Value) {
        let mut body = json!({
            "name": name,
            "device_id": device_id,
            "tools": tools,
            "allow_destructive": allow_destructive,
        });
        if let Some(seconds) = expires_in_s {
            body["expires_in_s"] = json!(seconds);
        }
        send(self.app(), post_json_bearer("/tokens", session, body)).await
    }

    /// `POST /tokens` for one aggregate token: an explicit device list, no single device.
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
                json!({
                    "name": name,
                    "device_ids": device_ids,
                    "tools": tools,
                    "allow_destructive": false,
                }),
            ),
        )
        .await
    }

    /// `POST /tokens/:id/rotate`: replace one token's secret, keeping its scope.
    async fn rotate_token(&self, session: &str, id: &str) -> (StatusCode, Value) {
        send(
            self.app(),
            post_json_bearer(&format!("/tokens/{id}/rotate"), session, json!({})),
        )
        .await
    }
}

fn get(uri: &str, session: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().method("GET").uri(uri);
    if let Some(session) = session {
        request = request.header(header::AUTHORIZATION, format!("Bearer {session}"));
    }
    request.body(Body::empty()).expect("the request builds")
}

fn delete(uri: &str, session: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().method("DELETE").uri(uri);
    if let Some(session) = session {
        request = request.header(header::AUTHORIZATION, format!("Bearer {session}"));
    }
    request.body(Body::empty()).expect("the request builds")
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("the request builds")
}

/// `POST` with an empty body, as the page's best-effort logout sends it.
fn post(uri: &str, session: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().method("POST").uri(uri);
    if let Some(session) = session {
        request = request.header(header::AUTHORIZATION, format!("Bearer {session}"));
    }
    request.body(Body::empty()).expect("the request builds")
}

fn post_json_bearer(uri: &str, session: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::from(body.to_string()))
        .expect("the request builds")
}

async fn authenticate_on(console: &Console) -> String {
    let (status, body) = send(
        console.router(),
        post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the fixture token must authenticate"
    );
    body["token"].as_str().expect("a session token").to_owned()
}

async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, _headers, value) = send_with_headers(app, request).await;
    (status, value)
}

/// [`send`] with the response headers, for the one refusal that carries `Retry-After`.
async fn send_with_headers(
    app: Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app.oneshot(request).await.expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body is readable");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("the body is JSON")
    };
    (status, headers, value)
}

/// [`send_with_headers`] for a response that is not JSON: the operator page is HTML.
async fn send_text(
    app: Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app.oneshot(request).await.expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body is readable");
    (
        status,
        headers,
        String::from_utf8(bytes.to_vec()).expect("the page is UTF-8"),
    )
}

fn fixture_config(dir: &TempDir, token: Option<&Path>, grant_key: Option<&Path>) -> Config {
    Config {
        operator_token_file: token
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dir.join("operator.token")),
        grant_key_file: grant_key
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dir.join("grant.key")),
        data: dir.join("data"),
        bind: std::net::Ipv4Addr::LOCALHOST.into(),
        port: 0,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        access: None,
        mesh_broker: None,
    }
}

fn write_key(path: &Path, bytes: &[u8; 32]) {
    fs::write(path, format!("{}\n", hex(bytes))).expect("the key file is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("the test key is private");
    }
}

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public_hex(key: &SigningKey) -> String {
    hex(&key.verifying_key().to_bytes())
}

/// The exact body a device sends to register: a code, its clock, and a signature over
/// `register\n{id}\n{code}\n{ts_ms}`.
fn registration_body(key: &SigningKey, code: &str, ts_ms: u64) -> Value {
    let id = public_hex(key);
    registration_body_signed_by(key, &id, code, ts_ms)
}

fn registration_body_signed_by(signer: &SigningKey, id: &str, code: &str, ts_ms: u64) -> Value {
    let message = format!("register\n{id}\n{code}\n{ts_ms}");
    json!({
        "id": id,
        "public_key": public_hex(signer),
        "code": code,
        "ts_ms": ts_ms,
        "signature": hex(&signer.sign(message.as_bytes()).to_bytes()),
    })
}

#[tokio::test]
async fn a_wrong_operator_token_is_401() {
    let fixture = Fixture::new("wrong-token");

    let (status, body) = send(
        fixture.app(),
        post_json("/auth", json!({ "token": "00".repeat(32) })),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn the_operator_token_yields_a_session_that_expires_in_a_month() {
    let fixture = Fixture::new("right-token");

    let (status, body) = send(
        fixture.app(),
        post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let session = body["token"].as_str().expect("a session token");
    assert_eq!(session.len(), 64, "a session token is 32 bytes of hex");
    assert_ne!(session, hex(&OPERATOR_TOKEN), "a session is not the secret");
    assert_eq!(body["expires_at_ms"], json!(START_MS + 30 * DAY_MS));
}

#[tokio::test]
async fn an_expired_session_is_401() {
    let fixture = Fixture::new("expired-session");
    let session = fixture.authenticate().await;

    let (status, _) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "the session starts out valid");

    fixture.advance_ms(30 * DAY_MS);
    let (status, body) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn operator_routes_refuse_a_missing_or_malformed_session() {
    let fixture = Fixture::new("no-session");

    let (status, _) = send(fixture.app(), get("/fleet", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(fixture.app(), get("/fleet", Some("not-a-token"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(fixture.app(), get("/fleet", Some(&"ab".repeat(32)))).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a well-formed token that was never issued is not a session"
    );
}

#[tokio::test]
async fn logout_ends_only_the_session_that_presented_its_token() {
    let fixture = Fixture::new("logout-one-session");
    let first = fixture.authenticate().await;
    let second = fixture.authenticate().await;
    assert_ne!(first, second, "two sign-ins are two sessions");

    let (status, body) = send(fixture.app(), post("/logout", Some(&first))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null, "204 carries no body");

    let (status, body) = send(fixture.app(), get("/fleet", Some(&first))).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the logged-out token is gone"
    );
    assert_eq!(body, json!({ "error": "unauthorized" }));

    // A second logout is 401 because the extractor refuses the token: it was removed from the
    // in-memory list, not merely marked. A flag would answer 204 again here.
    let (status, _) = send(fixture.app(), post("/logout", Some(&first))).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the session left the list"
    );

    let (status, _) = send(fixture.app(), get("/fleet", Some(&second))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the other operator session still works"
    );
}

#[tokio::test]
async fn logout_without_a_session_is_401() {
    let fixture = Fixture::new("logout-no-session");

    let (status, body) = send(fixture.app(), post("/logout", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));

    let (status, _) = send(fixture.app(), post("/logout", Some("not-a-token"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn enrollment_requires_an_operator_session() {
    let fixture = Fixture::new("enroll-no-session");

    let (status, body) = send(fixture.app(), post_json("/enroll", json!({}))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test]
async fn enrollment_codes_are_stored_hashed_and_never_in_cleartext() {
    let fixture = Fixture::new("codes-hashed");
    let session = fixture.authenticate().await;
    let (code, expires_at_ms) = fixture.enroll(&session).await;

    let on_disk = fs::read_to_string(fixture.config.data.join("codes.json"))
        .expect("the codes file is readable");
    assert!(
        !on_disk.contains(&code),
        "the issued code must not be persisted: {on_disk}"
    );
    let digest = sha2::Sha256::digest(code.as_bytes());
    assert!(
        on_disk.contains(&hex(&digest)),
        "the file stores sha256(code) as its key: {on_disk}"
    );
    assert!(
        on_disk.contains(&format!("\"expires_at_ms\":{expires_at_ms}")),
        "the expiry stays alongside the hash: {on_disk}"
    );

    // A restart re-reads the hash, not the code, and the code still spends exactly once.
    let restarted = fixture.restart();
    let (status, body) = send(
        restarted.router(),
        post_json(
            "/devices",
            registration_body(&signing_key(0x87), &code, fixture.now_ms()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "registration: {body}");
}

#[cfg(unix)]
#[tokio::test]
async fn the_registry_and_enrollment_file_are_written_mode_0600() {
    let fixture = Fixture::new("private-files");
    let session = fixture.authenticate().await;
    fixture.enroll(&session).await;
    fixture.register(&signing_key(0x88)).await;

    for name in ["codes.json", "devices.json"] {
        let path = fixture.config.data.join(name);
        assert_eq!(
            mode_of(&path),
            0o600,
            "{name} holds a credential-grade secret and must not be readable by others"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_world_readable_codes_file_is_refused_at_startup() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("world-readable-codes");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    let data = dir.join("data");
    fs::create_dir_all(&data).expect("the data directory is creatable");
    let codes = data.join("codes.json");
    fs::write(&codes, "[]").expect("the codes fixture is written");
    fs::set_permissions(&codes, fs::Permissions::from_mode(0o644))
        .expect("the test can loosen the mode");
    let config = fixture_config(&dir, None, None);

    let error = Console::load(&config)
        .err()
        .expect("a world-readable codes file must not start");
    let message = error.to_string();

    assert!(
        message.contains(&codes.display().to_string()) && message.contains("0600"),
        "the error names the file and the required mode: {message}"
    );
}

#[cfg(unix)]
#[test]
fn a_world_readable_registry_is_refused_at_startup() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("world-readable-registry");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    let data = dir.join("data");
    fs::create_dir_all(&data).expect("the data directory is creatable");
    let registry = data.join("devices.json");
    fs::write(&registry, "[]").expect("the registry fixture is written");
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o644))
        .expect("the test can loosen the mode");
    let config = fixture_config(&dir, None, None);

    let error = Console::load(&config)
        .err()
        .expect("a world-readable registry must not start");
    let message = error.to_string();

    assert!(
        message.contains(&registry.display().to_string()) && message.contains("0600"),
        "the error names the file and the required mode: {message}"
    );
}

#[cfg(unix)]
#[test]
fn an_operator_token_readable_by_others_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("world-readable-operator-token");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    fs::set_permissions(&token, fs::Permissions::from_mode(0o644))
        .expect("the test can loosen the mode");
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let config = fixture_config(&dir, None, None);

    let error = Console::load(&config)
        .err()
        .expect("a world-readable operator token must not start");
    let message = error.to_string();

    assert!(
        message.contains("--operator-token-file")
            && message.contains(&token.display().to_string())
            && message.contains("0600"),
        "the error names the flag, the file and the required mode: {message}"
    );
}

#[cfg(unix)]
#[test]
fn a_new_data_directory_is_created_mode_0700() {
    let dir = TempDir::new("private-data-dir");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    let config = fixture_config(&dir, None, None);

    Console::load(&config).expect("the console creates its data directory");

    assert_eq!(
        mode_of(&config.data),
        0o700,
        "the directory that holds the registry and codes is not world-readable"
    );
}

#[test]
fn a_missing_operator_token_file_is_a_startup_error_naming_the_flag() {
    let dir = TempDir::new("missing-token");
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let missing = dir.join("does-not-exist");
    let config = fixture_config(&dir, Some(&missing), None);

    let error = Console::load(&config)
        .err()
        .expect("a missing token file must not start");
    let message = error.to_string();

    assert!(
        message.contains("--operator-token-file"),
        "the error names the flag: {message}"
    );
    assert!(
        message.contains(&missing.display().to_string()),
        "the error names the path: {message}"
    );
}

#[test]
fn a_missing_grant_key_file_is_a_startup_error_naming_the_flag() {
    let dir = TempDir::new("missing-grant-key");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let missing = dir.join("does-not-exist");
    let config = fixture_config(&dir, None, Some(&missing));

    let error = Console::load(&config)
        .err()
        .expect("a missing grant key must not start");
    let message = error.to_string();

    assert!(
        message.contains("--grant-key-file"),
        "the error names the flag: {message}"
    );
}

#[cfg(unix)]
#[test]
fn a_grant_key_file_that_is_readable_by_others_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("world-readable-grant-key");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    fs::set_permissions(&grant_key, fs::Permissions::from_mode(0o644))
        .expect("the test can loosen the mode");
    let config = fixture_config(&dir, None, None);

    let error = Console::load(&config)
        .err()
        .expect("a world-readable grant key must not start");
    let message = error.to_string();

    assert!(
        message.contains("--grant-key-file") && message.contains("0600"),
        "the error names the flag and the required mode: {message}"
    );
}

#[test]
fn a_malformed_operator_token_file_is_a_startup_error_naming_the_flag() {
    let dir = TempDir::new("bad-token");
    let token = dir.join("operator.token");
    fs::write(&token, "not hex\n").expect("the token fixture is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&token, fs::Permissions::from_mode(0o600))
            .expect("the malformed fixture is still private");
    }
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let config = fixture_config(&dir, None, None);

    let error = Console::load(&config)
        .err()
        .expect("a malformed token must not start");

    assert!(
        error.to_string().contains("--operator-token-file"),
        "the error names the flag: {error}"
    );
}

#[test]
fn the_grant_key_file_stays_parseable_after_a_restart() {
    let dir = TempDir::new("grant-key-reload");
    let token = dir.join("operator.token");
    write_key(&token, &OPERATOR_TOKEN);
    let grant_key = dir.join("grant.key");
    write_key(&grant_key, &GRANT_SEED);
    fs::create_dir_all(dir.join("data")).expect("the data directory is creatable");
    let config = fixture_config(&dir, None, None);

    Console::load(&config).expect("the fixture key loads on the first start");
    Console::load(&config).expect("the same key loads on a restart");
}

#[tokio::test]
async fn enrollment_codes_are_random_and_expire_after_fifteen_minutes() {
    let fixture = Fixture::new("enroll");
    let session = fixture.authenticate().await;

    let (code, expires_at_ms) = fixture.enroll(&session).await;
    assert_eq!(code.len(), 64, "an enrollment code is 32 bytes of hex");
    assert!(
        code.chars().all(|character| character.is_ascii_hexdigit()),
        "an enrollment code is hex: {code}"
    );
    assert_eq!(expires_at_ms, START_MS + ENROLLMENT_TTL_MS);

    let (other, _) = fixture.enroll(&session).await;
    assert_ne!(code, other, "every enrollment code is fresh randomness");

    fixture.advance_ms(ENROLLMENT_TTL_MS);
    let key = signing_key(0x80);
    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("unknown_code"));
}

#[tokio::test]
async fn at_most_sixteen_enrollment_codes_are_outstanding() {
    let fixture = Fixture::new("enroll-cap");
    let session = fixture.authenticate().await;

    for _ in 0..16 {
        fixture.enroll(&session).await;
    }

    let (status, body) = send(
        fixture.app(),
        post_json_bearer("/enroll", &session, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], json!("too_many_codes"));

    fixture.advance_ms(ENROLLMENT_TTL_MS);
    let (status, _) = send(
        fixture.app(),
        post_json_bearer("/enroll", &session, json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "expired codes stop counting toward the cap"
    );
}

#[tokio::test]
async fn a_spent_enrollment_code_is_refused_and_does_not_survive_a_restart() {
    let fixture = Fixture::new("spent-code");
    let key = signing_key(0x81);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let (unspent, _) = fixture.enroll(&session).await;

    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().expect("a device token").to_owned();
    assert_eq!(token.len(), 64, "a device token is 32 bytes of hex");
    let on_disk = fs::read_to_string(fixture.config.data.join("devices.json"))
        .expect("the registry is readable");
    assert!(
        on_disk.contains("token_hash"),
        "the registry stores the token's hash: {on_disk}"
    );
    assert!(
        !on_disk.contains(&token),
        "the registry never stores the device token itself"
    );

    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("unknown_code"));

    let restarted = fixture.restart();
    let (status, body) = send(
        restarted.router(),
        post_json("/devices", registration_body(&key, &code, fixture.now_ms())),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a restart must not resurrect a spent code"
    );
    assert_eq!(body["error"], json!("unknown_code"));

    let (status, _) = send(
        restarted.router(),
        post_json(
            "/devices",
            registration_body(&signing_key(0x86), &unspent, fixture.now_ms()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an unspent code survives a restart");
}

#[tokio::test]
async fn a_registration_with_an_unknown_code_is_403() {
    let fixture = Fixture::new("unknown-code");
    let key = signing_key(0x82);

    let (status, body) = fixture
        .register_with_code(&key, &"cd".repeat(32), fixture.now_ms())
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("unknown_code"));
}

#[tokio::test]
async fn a_registration_timestamp_outside_five_minutes_is_refused() {
    let fixture = Fixture::new("stale-timestamp");
    let key = signing_key(0x83);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let now = fixture.now_ms();

    for ts_ms in [now + 5 * MINUTE_MS + 1, now - 5 * MINUTE_MS - 1] {
        let (status, body) = fixture.register_with_code(&key, &code, ts_ms).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "ts_ms={ts_ms} must be outside the ±5 min window"
        );
        assert_eq!(body["error"], json!("stale_timestamp"));
    }

    // The refused attempts must not have spent the code.
    let (status, _) = fixture
        .register_with_code(&key, &code, now + 5 * MINUTE_MS)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "exactly five minutes of skew is inside the window"
    );
}

#[tokio::test]
async fn a_registration_signature_from_another_key_is_refused() {
    let fixture = Fixture::new("wrong-signature");
    let key = signing_key(0x84);
    let impostor = signing_key(0x85);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let now = fixture.now_ms();
    let id = public_hex(&key);

    let body = registration_body_signed_by(&impostor, &id, &code, now);
    // The public key is the device's; only the signature comes from the impostor.
    let mut body = body;
    body["public_key"] = json!(id);
    let (status, response) = send(fixture.app(), post_json("/devices", body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["error"], json!("bad_signature"));

    let (status, _) = fixture.register_with_code(&key, &code, now).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a refused signature must not spend the code"
    );
}

#[tokio::test]
async fn a_device_registers_under_its_own_public_key() {
    let fixture = Fixture::new("register");
    let key = signing_key(0x33);
    let public_key = public_hex(&key);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;

    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], json!(public_key));
    assert_eq!(body["public_key"], json!(public_key));
    assert_eq!(body["registered_at_ms"], json!(START_MS));
    assert_eq!(body["last_seen_ms"], json!(START_MS));
    assert_eq!(
        body["token"].as_str().expect("a device token").len(),
        64,
        "registration returns the device token once"
    );

    let (status, list) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("a list").len(), 1);
    assert_eq!(list[0]["id"], json!(public_key));
    assert_eq!(list[0]["last_seen_ms"], json!(START_MS));
    assert!(
        list[0].get("token").is_none() && list[0].get("token_hash").is_none(),
        "the token and its hash never leave the registry: {list}"
    );
}

#[tokio::test]
async fn re_registering_updates_last_seen_and_keeps_registered_at() {
    let fixture = Fixture::new("re-register");
    let key = signing_key(0x44);

    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let (status, _) = fixture.register_with_code(&key, &code, START_MS).await;
    assert_eq!(status, StatusCode::OK);

    fixture.advance_ms(5_000);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["registered_at_ms"], json!(START_MS));
    assert_eq!(body["last_seen_ms"], json!(START_MS + 5_000));

    let (_, list) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(
        list.as_array().expect("a list").len(),
        1,
        "no duplicate entry"
    );
}

#[tokio::test]
async fn an_id_that_is_not_the_public_key_is_400() {
    let fixture = Fixture::new("id-mismatch");
    let key = signing_key(0x55);
    let other = signing_key(0x56);
    let now = fixture.now_ms();

    let (status, body) = send(
        fixture.app(),
        post_json(
            "/devices",
            registration_body_signed_by(&key, &public_hex(&other), &"00".repeat(32), now),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("id_mismatch"));

    let (status, _) = send(
        fixture.app(),
        post_json(
            "/devices",
            json!({
                "id": "not-hex",
                "public_key": public_hex(&key),
                "code": "00".repeat(32),
                "ts_ms": now,
                "signature": "00".repeat(64),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_public_key_that_is_not_an_ed25519_point_is_400() {
    let fixture = Fixture::new("bad-key");

    let (status, body) = send(
        fixture.app(),
        post_json(
            "/devices",
            json!({
                "id": "zz".repeat(32),
                "public_key": "zz".repeat(32),
                "code": "00".repeat(32),
                "ts_ms": fixture.now_ms(),
                "signature": "00".repeat(64),
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("invalid_public_key"));
}

#[tokio::test]
async fn the_registry_is_persisted_atomically_and_survives_a_restart() {
    let fixture = Fixture::new("persist");
    let key = signing_key(0x66);

    let (_, _) = fixture.register(&key).await;

    let path = fixture.config.data.join("devices.json");
    assert!(
        path.is_file(),
        "the registry is written where the flag says"
    );
    assert!(
        !fixture.config.data.join("devices.json.tmp").exists(),
        "the atomic write leaves no temporary file behind"
    );

    let on_disk: Vec<Value> =
        serde_json::from_slice(&fs::read(&path).expect("the registry is readable"))
            .expect("the registry is JSON");
    assert_eq!(on_disk.len(), 1);
    assert_eq!(on_disk[0]["public_key"], json!(public_hex(&key)));

    let restarted = fixture.restart();
    let session = authenticate_on(&restarted).await;
    let (status, list) = send(restarted.router(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("a list").len(), 1);
}

fn decode_signature(hex_signature: &str) -> ed25519_dalek::Signature {
    let bytes: Vec<u8> = (0..hex_signature.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex_signature[index..index + 2], 16).expect("hex"))
        .collect();
    let array: [u8; 64] = bytes.try_into().expect("a 64-byte signature");
    ed25519_dalek::Signature::from_bytes(&array)
}

async fn register_fixture_device(fixture: &Fixture, key: &SigningKey) -> String {
    fixture.register(key).await.0
}

#[tokio::test]
async fn a_valid_grant_verifies_with_the_console_grant_key() {
    let fixture = Fixture::new("grant");
    let device_key = signing_key(0x77);
    let device_id = register_fixture_device(&fixture, &device_key).await;
    let session = fixture.authenticate().await;
    let tools = vec!["files.read".to_owned(), "terminal.create".to_owned()];

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": device_id,
                "tools": tools,
                "expires_in_s": 3_600,
                "allow_destructive": true,
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["grant"]["device_id"], json!(device_id));
    assert_eq!(body["grant"]["tools"], json!(tools));
    assert_eq!(body["grant"]["expires_at_ms"], json!(START_MS + 3_600_000));
    assert_eq!(
        body["grant"]["allow_destructive"],
        json!(true),
        "the console returns the fact it signed"
    );

    let nonce = body["grant"]["nonce"]
        .as_str()
        .expect("the grant carries the nonce it signed")
        .to_owned();
    assert_eq!(nonce.len(), 32, "sixteen bytes of hex: {nonce}");
    let canonical = summrise_console::canonical_message(&summrise_console::Grant {
        device_id: device_id.clone(),
        tools: tools.clone(),
        expires_at_ms: START_MS + 3_600_000,
        allow_destructive: true,
        nonce: nonce.clone(),
    });
    assert_eq!(
        canonical,
        format!(
            "grant\n{device_id}\n{}\n{}\ntrue\n{nonce}",
            tools.join(","),
            START_MS + 3_600_000
        ),
        "the signed document is the five-line message"
    );
    let signature = decode_signature(body["signature"].as_str().expect("a hex signature"));
    let grant_key = SigningKey::from_bytes(&GRANT_SEED);
    grant_key
        .verifying_key()
        .verify_strict(canonical.as_bytes(), &signature)
        .expect("the signature verifies over the canonical message");
}

#[tokio::test]
async fn a_grant_that_does_not_ask_for_destruction_gets_false() {
    let fixture = Fixture::new("grant-default-destructive");
    let device_id = register_fixture_device(&fixture, &signing_key(0x7e)).await;
    let session = fixture.authenticate().await;

    // No `allow_destructive` field at all: destructive consent must be off.
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    assert_eq!(grant["grant"]["allow_destructive"], json!(false));
    let canonical = summrise_console::canonical_message(&summrise_console::Grant {
        device_id,
        tools: vec!["files.read".to_owned()],
        expires_at_ms: START_MS + 60_000,
        allow_destructive: false,
        nonce: grant["grant"]["nonce"]
            .as_str()
            .expect("the grant carries a nonce")
            .to_owned(),
    });
    let signature = decode_signature(grant["signature"].as_str().expect("a hex signature"));
    SigningKey::from_bytes(&GRANT_SEED)
        .verifying_key()
        .verify_strict(canonical.as_bytes(), &signature)
        .expect("the signature covers the implicit `false`");
}

#[test]
fn the_canonical_message_is_exactly_the_agreed_string() {
    let tools = vec!["files.read".to_owned(), "terminal.create".to_owned()];
    assert_eq!(
        summrise_console::canonical_message(&summrise_console::Grant {
            device_id: "abc123".to_owned(),
            tools,
            expires_at_ms: 1_700_000_000_000,
            allow_destructive: false,
            nonce: String::new(),
        }),
        "grant\nabc123\nfiles.read,terminal.create\n1700000000000\nfalse"
    );
}

#[tokio::test]
async fn a_grant_for_an_unknown_device_is_404() {
    let fixture = Fixture::new("grant-unknown-device");
    let session = fixture.authenticate().await;

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": "ab".repeat(32),
                "tools": ["files.read"],
                "expires_in_s": 60,
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("unknown_device"));
}

#[tokio::test]
async fn a_grant_with_an_unknown_tool_is_400() {
    let fixture = Fixture::new("grant-unknown-tool");
    let device_id = register_fixture_device(&fixture, &signing_key(0x78)).await;
    let session = fixture.authenticate().await;

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": device_id,
                "tools": ["files.read", "shell.everything"],
                "expires_in_s": 60,
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("unknown_tool"));
}

#[tokio::test]
async fn a_grant_with_no_tools_is_400() {
    let fixture = Fixture::new("grant-no-tools");
    let device_id = register_fixture_device(&fixture, &signing_key(0x79)).await;
    let session = fixture.authenticate().await;

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({ "device_id": device_id, "tools": [], "expires_in_s": 60 }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], json!("empty_tools"));
}

#[tokio::test]
async fn an_expiry_over_seven_days_is_400_and_exactly_seven_days_is_fine() {
    let fixture = Fixture::new("grant-expiry");
    let device_id = register_fixture_device(&fixture, &signing_key(0x7a)).await;
    let session = fixture.authenticate().await;

    let over = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": device_id,
                "tools": ["files.read"],
                "expires_in_s": 7 * 24 * 60 * 60 + 1,
            }),
        ),
    )
    .await;
    assert_eq!(over.0, StatusCode::BAD_REQUEST);
    assert_eq!(over.1["error"], json!("expiry_too_long"));

    let at_the_limit = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": device_id,
                "tools": ["files.read"],
                "expires_in_s": 7 * 24 * 60 * 60,
            }),
        ),
    )
    .await;
    assert_eq!(at_the_limit.0, StatusCode::OK);
    assert_eq!(
        at_the_limit.1["grant"]["expires_at_ms"],
        json!(START_MS + 7 * 24 * 60 * 60 * 1000)
    );
}

#[tokio::test]
async fn grant_signing_requires_an_operator_session() {
    let fixture = Fixture::new("grant-no-session");
    let device_id = register_fixture_device(&fixture, &signing_key(0x7b)).await;

    let (status, _) = send(
        fixture.app(),
        post_json(
            "/grants",
            json!({ "device_id": device_id, "tools": ["files.read"], "expires_in_s": 60 }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// **EVERY ANSWER CARRIES THE BROWSER MITIGATIONS.** Measured on the live console before this
/// test: `x-frame-options`, `cache-control` and `content-type` — nothing about sniffing, referrers
/// or what the page may embed. The middleware that sets them is one layer, so one test reads them.
#[tokio::test]
async fn every_answer_carries_the_browser_mitigations() {
    let fixture = Fixture::new("security-headers");

    let (status, headers, _) = send_with_headers(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("x-frame-options").and_then(|v| v.to_str().ok()),
        Some("DENY")
    );
    assert_eq!(
        headers
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        headers.get("referrer-policy").and_then(|v| v.to_str().ok()),
        Some("no-referrer")
    );
    assert_eq!(
        headers
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok()),
        Some("frame-ancestors 'none'; object-src 'none'; base-uri 'self'")
    );
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "a cached page is yesterday's page"
    );
}

#[tokio::test]
async fn health_reports_status_device_count_and_uptime_without_auth() {
    let fixture = Fixture::new("health");

    let (status, body) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], json!("ok"));
    assert_eq!(body["devices"], json!(0));
    assert_eq!(body["online"], json!(0), "nothing is connected yet: {body}");
    assert_eq!(
        body["version"],
        json!(env!("CARGO_PKG_VERSION")),
        "the endpoint an operator checks answers which build is running"
    );
    assert!(
        body["uptime_s"].as_u64().is_some(),
        "uptime is a whole number of seconds: {body}"
    );

    register_fixture_device(&fixture, &signing_key(0x7c)).await;
    let (status, body) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["devices"], json!(1));
    // Registered is not connected: the count that answers "is anything up" stays 0 until a channel
    // opens, which is the distinction a known-device count cannot make.
    assert_eq!(body["online"], json!(0), "{body}");
}

/// **ONLINE IS THE QUESTION.** `devices` counts the registry and stays the same whether anything is
/// connected; `online` counts the channels that are open at this instant. This opens one.
#[tokio::test]
async fn health_counts_a_connected_device_as_online() {
    let fixture = Fixture::new("health-online");
    let server = TestServer::start(&fixture.console).await;
    let (_device_id, _token, _device) =
        connected_device(&fixture, &server, &signing_key(0x7b)).await;

    let (status, body) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["devices"], json!(1), "{body}");
    assert_eq!(
        body["online"],
        json!(1),
        "the channel the test opened is counted: {body}"
    );
}

/// **THE FIELD MOVES BOTH WAYS.** The incident of 2026-10-29 was a channel that went away; the test
/// above proves an open channel counts. This one proves a channel that drops stops counting — the
/// state an operator (or a supervisor) needs to see, and the one the old `devices` count could not
/// express.
#[tokio::test]
async fn health_stops_counting_a_device_whose_channel_closed() {
    let fixture = Fixture::new("health-offline");
    let server = TestServer::start(&fixture.console).await;
    let (_device_id, _token, device) =
        connected_device(&fixture, &server, &signing_key(0x7a)).await;

    let (_, body) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(body["online"], json!(1), "the channel is open: {body}");

    drop(device);

    // The close is asynchronous: the channel task notices the socket ended.
    let mut last = body;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (_, body) = send(fixture.app(), get("/health", None)).await;
        if body["online"] == json!(0) {
            assert_eq!(
                body["devices"],
                json!(1),
                "the device is still known: {body}"
            );
            return;
        }
        last = body;
    }
    panic!("the dropped channel still counts as online: {last}");
}

/// **THE OUTAGE THAT IS OVER IS STILL A FACT.** A fleet row says `online` again the moment a device
/// comes back; this is what lets it also say how long the device was gone.
#[tokio::test]
async fn a_reconnect_records_how_long_the_device_was_away() {
    let fixture = Fixture::new("outage-record");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token, device) = connected_device(&fixture, &server, &signing_key(0x78)).await;
    let session = fixture.authenticate().await;

    let (_, fleet) = send(fixture.app(), get("/fleet", Some(&session))).await;
    let listed = fleet
        .as_array()
        .expect("the fleet is a list")
        .iter()
        .find(|entry| entry["id"] == json!(device_id))
        .expect("the device is listed");
    assert!(
        listed["last_outage"].is_null(),
        "a device that has never gone away has no outage: {listed}"
    );

    drop(device);
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let _again = open_channel(&server, &device_id, &token)
        .await
        .expect("the device reconnects");

    let (_, fleet) = send(fixture.app(), get("/fleet", Some(&session))).await;
    let listed = fleet
        .as_array()
        .expect("the fleet is a list")
        .iter()
        .find(|entry| entry["id"] == json!(device_id))
        .expect("the device is still listed");
    let outage = &listed["last_outage"];
    assert!(
        outage["duration_ms"].as_u64().unwrap_or(0) >= 80,
        "the gap it was away is recorded: {listed}"
    );
    assert!(
        outage["started_ms"].as_u64().unwrap_or(0) > 0,
        "and when it began: {listed}"
    );
}

#[tokio::test]
async fn a_grant_shorter_than_a_minute_is_400_and_a_minute_is_fine() {
    let fixture = Fixture::new("grant-minimum-expiry");
    let device_id = register_fixture_device(&fixture, &signing_key(0x7d)).await;
    let session = fixture.authenticate().await;

    for expires_in_s in [0, 59] {
        let (status, body) = send(
            fixture.app(),
            post_json_bearer(
                "/grants",
                &session,
                json!({
                    "device_id": device_id,
                    "tools": ["files.read"],
                    "expires_in_s": expires_in_s,
                }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "expires_in_s={expires_in_s}"
        );
        assert_eq!(body["error"], json!("expiry_too_short"));
    }

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/grants",
            &session,
            json!({
                "device_id": device_id,
                "tools": ["files.read"],
                "expires_in_s": 60,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["grant"]["expires_at_ms"], json!(START_MS + 60_000));
}

// ---- the device channel and the operator's /mcp proxy -------------------------------------------

type DeviceSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The real listener the WebSocket tests need: axum's upgrade hands the socket to a task, which a
/// tower `oneshot` cannot do.
struct TestServer {
    addr: std::net::SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(console: &Console) -> Self {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("a loopback listener");
        let addr = listener.local_addr().expect("a bound address");
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let router = console.router();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("the test server serves");
        });
        Self {
            addr,
            shutdown: Some(shutdown),
            handle,
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.handle.abort();
    }
}

async fn open_channel(
    server: &TestServer,
    device_id: &str,
    token: &str,
) -> Result<DeviceSocket, Box<tungstenite::Error>> {
    let mut request = format!("ws://{}/devices/{}/channel", server.addr, device_id)
        .into_client_request()
        .expect("a websocket request");
    request.headers_mut().insert(
        tungstenite::http::header::AUTHORIZATION,
        format!("Bearer {token}").parse().expect("a header value"),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(Box::new)
}

fn http_status(error: &tungstenite::Error) -> Option<u16> {
    match error {
        tungstenite::Error::Http(response) => Some(response.status().as_u16()),
        _ => None,
    }
}

async fn next_message(socket: &mut DeviceSocket) -> WsMessage {
    tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("a frame arrives")
        .expect("the socket stays open")
        .expect("a websocket message")
}

async fn next_json(socket: &mut DeviceSocket) -> Value {
    let text = next_message(socket)
        .await
        .into_text()
        .expect("a text frame");
    serde_json::from_str(text.as_str()).expect("the frame is JSON")
}

async fn answer(socket: &mut DeviceSocket, frame: &Value, result: Value) {
    let reply = json!({ "id": frame["id"].clone(), "result": result });
    socket
        .send(WsMessage::Text(reply.to_string()))
        .await
        .expect("the device answers");
}

#[tokio::test]
async fn an_unregistered_device_cannot_open_a_channel() {
    let fixture = Fixture::new("channel-unregistered");
    let server = TestServer::start(&fixture.console).await;

    let error = open_channel(&server, &"ab".repeat(32), &"cd".repeat(32))
        .await
        .expect_err("an unregistered device is refused");

    assert_eq!(http_status(&error), Some(401));
}

#[tokio::test]
async fn a_channel_requires_the_device_token() {
    let fixture = Fixture::new("channel-token");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x90)).await;

    let wrong = open_channel(&server, &device_id, &"cd".repeat(32)).await;
    let error = wrong.expect_err("a wrong token is refused");
    assert_eq!(http_status(&error), Some(401));

    let (other_id, other_token) = fixture.register(&signing_key(0x99)).await;
    let foreign = open_channel(&server, &other_id, &token).await;
    let error = foreign.expect_err("a token only opens its own device's channel");
    assert_eq!(http_status(&error), Some(401));

    open_channel(&server, &device_id, &token)
        .await
        .expect("the issued token opens the channel");
    open_channel(&server, &other_id, &other_token)
        .await
        .expect("each device has its own token");
}

#[tokio::test]
async fn a_second_channel_replaces_the_first() {
    let fixture = Fixture::new("channel-replace");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x91)).await;

    let mut first = open_channel(&server, &device_id, &token)
        .await
        .expect("the first channel");
    let _second = open_channel(&server, &device_id, &token)
        .await
        .expect("the second channel");

    match next_message(&mut first).await {
        WsMessage::Close(Some(frame)) => {
            assert_eq!(
                frame.code,
                tungstenite::protocol::frame::coding::CloseCode::Again
            );
            assert_eq!(frame.reason, "replaced by a new channel");
        }
        other => panic!("the replaced channel must be closed with a reason, got {other:?}"),
    }
}

#[tokio::test]
async fn a_device_channel_carries_an_mcp_round_trip() {
    let fixture = Fixture::new("mcp-round-trip");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x92)).await;
    let mut device = open_channel(&server, &device_id, &token)
        .await
        .expect("the device channel");
    // Let the upgrade callback register the channel before the proxy looks for it.
    tokio::task::yield_now().await;

    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" });
    let response = json!({ "jsonrpc": "2.0", "id": 7, "result": { "tools": [] } });

    let proxy = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    );
    let device_side = async {
        // An answer nobody asked for is logged and dropped; it must not be mistaken for the reply.
        device
            .send(WsMessage::Text(
                json!({ "id": 999_999, "result": { "stray": true } }).to_string(),
            ))
            .await
            .expect("the stray frame is sent");
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, response.clone()).await;
    };

    let ((status, body), ()) = tokio::join!(proxy, device_side);

    assert_eq!(status, StatusCode::OK, "mcp: {body}");
    assert_eq!(body, response);
}

#[tokio::test]
async fn a_read_only_grant_refuses_a_destructive_tool_call() {
    let fixture = Fixture::new("mcp-tool-denied");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc1)).await;
    let session = fixture.authenticate().await;
    // The grant permits files.read only.
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": {
            "name": "files.write",
            "arguments": { "path": "denied-body-marker-3f1a" },
        },
    });

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "refusal: {body}");
    assert_eq!(body["jsonrpc"], json!("2.0"));
    assert_eq!(body["id"], json!(9), "the JSON-RPC id is echoed");
    assert_eq!(
        body["error"]["code"],
        json!(-32001),
        "an authorization refusal is a JSON-RPC server error"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("an error message")
            .contains("not permitted"),
        "the message names the refusal: {body}"
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(200), device.next())
            .await
            .is_err(),
        "a call the grant does not permit must never reach the device"
    );

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(403));
    assert_eq!(denied["device_id"], json!(device_id));
    assert_eq!(denied["tool"], json!("files.write"));
    assert!(
        !audit_text(&fixture).contains("denied-body-marker-3f1a"),
        "the audit records the tool, never its arguments"
    );
}

#[tokio::test]
async fn the_forwarded_grant_reverifies_over_the_canonical_bytes() {
    let fixture = Fixture::new("mcp-forwarded-grant");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc2)).await;
    let session = fixture.authenticate().await;
    let tools = vec!["files.read", "terminal.create"];
    let grant = fixture
        .create_grant_with_flag(&session, &device_id, &tools, 120, true)
        .await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": {} },
    });

    let proxy = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);

        // The device receives the verified grant and can reconstruct the bytes with nothing but
        // this frame: the same field names, the same order and the same separators the console signs.
        let forwarded = &frame["grant"];
        let facts = forwarded["grant"]
            .as_object()
            .expect("the granted facts are an object");
        assert_eq!(
            facts.len(),
            5,
            "exactly the five canonical fields: {facts:?}"
        );
        let delivered_nonce = facts["nonce"]
            .as_str()
            .expect("a nonce in the forwarded grant")
            .to_owned();
        let delivered_device = facts["device_id"]
            .as_str()
            .expect("a device id in the forwarded grant");
        let delivered_tools: Vec<String> = facts["tools"]
            .as_array()
            .expect("tools in the forwarded grant")
            .iter()
            .map(|tool| tool.as_str().expect("a tool name").to_owned())
            .collect();
        let delivered_expiry = facts["expires_at_ms"]
            .as_u64()
            .expect("an expiry in the forwarded grant");
        let delivered_consent = facts["allow_destructive"]
            .as_bool()
            .expect("a destructive-consent flag in the forwarded grant");
        assert_eq!(delivered_device, device_id);
        assert_eq!(delivered_tools, tools);
        assert_eq!(delivered_expiry, START_MS + 120_000);
        assert!(delivered_consent, "the operator's consent is forwarded");

        let canonical = format!(
            "grant\n{delivered_device}\n{}\n{delivered_expiry}\n{delivered_consent}\n{delivered_nonce}",
            delivered_tools.join(",")
        );
        let signature = decode_signature(
            forwarded["signature"]
                .as_str()
                .expect("a signature in the forwarded grant"),
        );
        SigningKey::from_bytes(&GRANT_SEED)
            .verifying_key()
            .verify_strict(canonical.as_bytes(), &signature)
            .expect("the forwarded grant re-verifies over exactly the canonical bytes");

        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 11, "result": { "ok": true } }),
        )
        .await;
    };

    let ((status, body), ()) = tokio::join!(proxy, device_side);
    assert_eq!(status, StatusCode::OK, "mcp: {body}");
    assert_eq!(body["result"], json!({ "ok": true }));
}

#[tokio::test]
async fn a_tool_call_without_a_tool_name_is_a_json_rpc_error() {
    let fixture = Fixture::new("mcp-tool-missing");
    let (device_id, _token) = fixture.register(&signing_key(0xc3)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": { "arguments": {} },
    });

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "refusal: {body}");
    assert_eq!(body["id"], json!(12));
    assert_eq!(body["error"]["code"], json!(-32602));

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(400));
    assert_eq!(denied["detail"], json!("invalid_tool_call"));
}

#[tokio::test]
async fn a_device_json_rpc_error_is_forwarded_as_200_unchanged() {
    let fixture = Fixture::new("mcp-error");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x9a)).await;
    let mut device = open_channel(&server, &device_id, &token)
        .await
        .expect("the device channel");
    tokio::task::yield_now().await;

    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "files.read",
        "params": { "path": "nope" },
    });
    let error = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "error": {
            "code": -32601,
            "message": "no such tool",
            "data": { "tool": "files.nope" },
        },
    });

    let proxy = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        let reply = json!({ "id": frame["id"].clone(), "error": error.clone() });
        device
            .send(WsMessage::Text(reply.to_string()))
            .await
            .expect("the device answers with an error envelope");
    };

    let ((status, body), ()) = tokio::join!(proxy, device_side);

    assert_eq!(
        status,
        StatusCode::OK,
        "a JSON-RPC error is an answer, not a gateway failure"
    );
    assert_eq!(
        body, error,
        "the JSON-RPC error body is forwarded unchanged"
    );
}

#[tokio::test]
async fn an_mcp_call_requires_an_operator_session() {
    let fixture = Fixture::new("mcp-no-session");

    let (status, body) = send(
        fixture.app(),
        post_json(
            "/mcp",
            json!({
                "device_id": "ab".repeat(32),
                "grant": {
                    "grant": {
                        "device_id": "ab".repeat(32),
                        "tools": ["files.read"],
                        "expires_at_ms": START_MS + 60_000,
                        "allow_destructive": false,
                    },
                    "signature": "00".repeat(64),
                },
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn an_mcp_call_to_an_unregistered_device_is_404() {
    let fixture = Fixture::new("mcp-unknown-device");
    let session = fixture.authenticate().await;
    let device_id = "ab".repeat(32);

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
                "grant": {
                    "grant": {
                        "device_id": device_id,
                        "tools": ["files.read"],
                        "expires_at_ms": START_MS + 60_000,
                        "allow_destructive": false,
                    },
                    "signature": "00".repeat(64),
                },
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "error": "unknown_device" }));
}

#[tokio::test]
async fn a_request_with_no_channel_waits_then_is_503_device_reconnecting() {
    let fixture = Fixture::new("mcp-reconnecting");
    let (device_id, _token) = fixture.register(&signing_key(0x93)).await;
    let session = fixture.authenticate().await;

    let started = std::time::Instant::now();
    let (status, headers, body) = send_with_headers(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        headers
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("1"),
        "a missing channel is safe to retry, and the header says so"
    );
    assert_eq!(
        body,
        json!({ "error": "device reconnecting", "retry_after_ms": 1000 }),
        "a missing channel is retryable, never a silent drop"
    );
    assert!(
        elapsed >= Duration::from_millis(1900),
        "the request waits for a replacement channel before giving up: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the wait is bounded at two seconds: {elapsed:?}"
    );

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(503));
    assert_eq!(denied["detail"], json!("device reconnecting"));
}

#[tokio::test]
async fn an_expired_grant_is_refused() {
    let fixture = Fixture::new("mcp-expired-grant");
    let (device_id, _token) = fixture.register(&signing_key(0x94)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    fixture.advance_ms(60_000);
    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
            "grant": grant,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({ "error": "grant_expired" }));
}

#[tokio::test]
async fn a_grant_that_names_another_device_is_refused() {
    let fixture = Fixture::new("mcp-foreign-grant");
    let (device_id, _token) = fixture.register(&signing_key(0x95)).await;
    let (other_id, _) = fixture.register(&signing_key(0x96)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &other_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
            "grant": grant,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({ "error": "grant_device_mismatch" }));
}

#[tokio::test]
async fn a_tampered_grant_is_refused() {
    let fixture = Fixture::new("mcp-tampered-grant");
    let (device_id, _token) = fixture.register(&signing_key(0x97)).await;
    let session = fixture.authenticate().await;
    let mut grant = fixture.create_grant(&session, &device_id, 60).await;
    grant["grant"]["tools"] = json!(["files.write"]);

    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
            "grant": grant,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({ "error": "invalid_grant_signature" }));
}

#[tokio::test]
async fn two_hundred_requests_against_a_wedged_device_stay_bounded_and_busy() {
    // The bounds the proxy promises: 64 frames may be queued for one socket, and no more than 32
    // requests may be in flight for one device. A wedged device cannot turn either into memory the
    // console holds without limit.
    const CHANNEL_CAP: u64 = 64;
    const IN_FLIGHT_CAP: usize = 32;

    let fixture = Fixture::with_timeout("mcp-bounded", Duration::from_millis(500));
    let server = TestServer::start(&fixture.console).await;
    // The device opens its channel and then never reads: the socket is wedged.
    let (device_id, _token, _device) =
        connected_device(&fixture, &server, &signing_key(0xc4)).await;
    let session = fixture.authenticate().await;

    let mut calls = Vec::new();
    for _ in 0..200 {
        let app = fixture.app();
        let request = post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        );
        calls.push(tokio::spawn(async move {
            send_with_headers(app, request).await
        }));
    }

    let mut busy = 0;
    let mut timed_out = 0;
    for call in calls {
        let (status, headers, body) = call.await.expect("the proxy task finishes");
        match status {
            StatusCode::SERVICE_UNAVAILABLE => {
                assert_eq!(
                    body,
                    json!({ "error": "device busy", "retry_after_ms": 250 }),
                    "an over-cap request is refused, not queued"
                );
                assert_eq!(
                    headers
                        .get(header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok()),
                    Some("1"),
                    "the busy refusal is retryable and says so in the header too"
                );
                busy += 1;
            }
            StatusCode::GATEWAY_TIMEOUT => {
                assert_eq!(body, json!({ "error": "device timeout" }));
                timed_out += 1;
            }
            other => panic!("unexpected status {other}: {body}"),
        }
    }
    assert_eq!(
        busy,
        200 - IN_FLIGHT_CAP,
        "the excess is refused rather than retained"
    );
    assert_eq!(
        timed_out, IN_FLIGHT_CAP,
        "only the capped in-flight requests wait for an answer"
    );

    let (status, health) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    let peak = health["max_queued_frames"]
        .as_u64()
        .expect("health reports the queue high-water mark");
    assert!(
        peak <= CHANNEL_CAP,
        "the channel depth never exceeds its cap: {peak} > {CHANNEL_CAP}"
    );
    assert!(
        peak <= IN_FLIGHT_CAP as u64,
        "the in-flight cap keeps the queue below the channel cap: {peak}"
    );
}

#[tokio::test]
async fn a_device_that_never_answers_is_a_504() {
    let fixture = Fixture::with_timeout("mcp-timeout", Duration::from_millis(100));
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x98)).await;
    let _device = open_channel(&server, &device_id, &token)
        .await
        .expect("the device channel");
    let session = fixture.authenticate().await;

    let mut outcome = None;
    for _ in 0..40 {
        let (status, body) = send(
            fixture.app(),
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                        "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
                }),
            ),
        )
        .await;
        if status != StatusCode::SERVICE_UNAVAILABLE {
            outcome = Some((status, body));
            break;
        }
        // The upgrade callback may not have registered the channel yet; retry the cheap way.
        tokio::task::yield_now().await;
    }

    let (status, body) = outcome.expect("the channel registers and then times out");
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body, json!({ "error": "device timeout" }));

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(504));
    assert_eq!(denied["detail"], json!("device timeout"));
}

// ---- the audit index --------------------------------------------------------------------------

/// Every audit line across the one rotation and the current file, oldest first.
fn audit_lines(fixture: &Fixture) -> Vec<Value> {
    jsonl_lines(fixture, "audit")
}

/// Every denial line across the one rotation and the current file, oldest first.
fn denial_lines(fixture: &Fixture) -> Vec<Value> {
    jsonl_lines(fixture, "denials")
}

/// Every line of one of the console's rotating JSONL files, oldest first.
fn jsonl_lines(fixture: &Fixture, stem: &str) -> Vec<Value> {
    let mut lines = Vec::new();
    for name in [format!("{stem}.1.jsonl"), format!("{stem}.jsonl")] {
        let path = fixture.config.data.join(name);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("{} is readable: {error}", path.display()),
        };
        for line in text.lines() {
            lines.push(serde_json::from_str(line).expect("a JSONL line is JSON"));
        }
    }
    lines
}

#[tokio::test]
async fn the_audit_route_requires_an_operator_session() {
    let fixture = Fixture::new("audit-no-session");

    let (status, body) = send(fixture.app(), get("/audit", None)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn the_audit_lists_the_newest_lines_first() {
    let fixture = Fixture::new("audit-newest-first");
    let session = fixture.authenticate().await;
    fixture.enroll(&session).await;
    let device_id = register_fixture_device(&fixture, &signing_key(0xa1)).await;
    fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(fixture.app(), get("/audit?limit=2", Some(&session))).await;

    assert_eq!(status, StatusCode::OK);
    let newest = body.as_array().expect("a list of audit lines");
    assert_eq!(newest.len(), 2);
    assert_eq!(newest[0]["kind"], json!("grant"));
    assert_eq!(newest[1]["kind"], json!("register"));
    assert_eq!(newest[0]["status"], json!(200));
    assert_eq!(newest[0]["ts_ms"], json!(START_MS));

    let (_, all) = send(fixture.app(), get("/audit", Some(&session))).await;
    let kinds: Vec<Value> = all
        .as_array()
        .expect("a list")
        .iter()
        .map(|line| line["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            json!("grant"),
            json!("register"),
            json!("enroll"),
            json!("auth"),
            json!("enroll"),
            json!("auth")
        ],
        "one line per operator action, newest first, sign-ins included"
    );
}

// ---- the denial index -------------------------------------------------------------------------

/// Write `count` denial lines to the current file, oldest first, without going through the writer.
fn seed_denials(fixture: &Fixture, count: usize) {
    let mut text = String::new();
    for index in 0..count {
        text.push_str(
            &json!({
                "ts_ms": START_MS + index as u64,
                "kind": "denied",
                "status": 401,
                "detail": format!("seeded-{index}"),
            })
            .to_string(),
        );
        text.push('\n');
    }
    fs::write(fixture.config.data.join("denials.jsonl"), text)
        .expect("the seeded denial file is writable");
}

#[tokio::test]
async fn the_denials_route_requires_an_operator_session() {
    let fixture = Fixture::new("denials-no-session");

    let (status, body) = send(fixture.app(), get("/denials", None)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthorized" }));
    let denied = denial_lines(&fixture)
        .pop()
        .expect("the refused read is itself a denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(401));
    assert_eq!(denied["detail"], json!("unauthorized"));
    assert!(
        audit_lines(&fixture).is_empty(),
        "an unauthenticated read never reaches the authenticated audit"
    );
}

#[tokio::test]
async fn the_denials_list_the_newest_lines_first() {
    let fixture = Fixture::new("denials-newest-first");

    let (status, _) = send(
        fixture.app(),
        post_json("/auth", json!({ "token": "00".repeat(32) })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(fixture.app(), get("/fleet", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let session = fixture.authenticate().await;
    let (status, body) = send(fixture.app(), get("/denials", Some(&session))).await;

    assert_eq!(status, StatusCode::OK);
    let newest = body.as_array().expect("a list of denial lines");
    assert_eq!(newest.len(), 2);
    assert_eq!(newest[0]["kind"], json!("denied"));
    assert_eq!(newest[0]["detail"], json!("unauthorized"));
    assert_eq!(newest[1]["detail"], json!("bad_operator_token"));
    assert_eq!(newest[0]["ts_ms"], json!(START_MS));
}

#[tokio::test]
async fn a_denials_limit_of_one_is_the_newest_line() {
    let fixture = Fixture::new("denials-limit-one");
    let session = fixture.authenticate().await;
    seed_denials(&fixture, 2);

    let (status, body) = send(fixture.app(), get("/denials?limit=1", Some(&session))).await;

    assert_eq!(status, StatusCode::OK);
    let lines = body.as_array().expect("a list of denial lines");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["detail"], json!("seeded-1"));
}

#[tokio::test]
async fn a_denials_zero_limit_is_empty() {
    let fixture = Fixture::new("denials-limit-zero");
    let session = fixture.authenticate().await;
    seed_denials(&fixture, 2);

    let (status, body) = send(fixture.app(), get("/denials?limit=0", Some(&session))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn a_malformed_denials_limit_is_a_json_refusal() {
    let fixture = Fixture::new("denials-bad-limit");
    let session = fixture.authenticate().await;

    let (status, body) = send(fixture.app(), get("/denials?limit=abc", Some(&session))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({ "error": "invalid_limit" }),
        "the console's JSON refusal, not axum's plain rejection"
    );
}

#[tokio::test]
async fn a_denials_limit_above_the_max_is_clamped() {
    let fixture = Fixture::new("denials-limit-clamp");
    let session = fixture.authenticate().await;
    seed_denials(&fixture, 1001);

    let (status, body) = send(fixture.app(), get("/denials?limit=100000", Some(&session))).await;

    assert_eq!(status, StatusCode::OK);
    let lines = body.as_array().expect("a list of denial lines");
    assert_eq!(lines.len(), 1000, "the index is not an unbounded read");
    assert_eq!(lines[0]["detail"], json!("seeded-1000"));
    assert_eq!(
        lines[999]["detail"],
        json!("seeded-1"),
        "the newest 1000 lines, not the oldest"
    );
}

#[tokio::test]
async fn a_missing_or_empty_denial_log_is_an_empty_list() {
    let fixture = Fixture::new("denials-empty");
    let session = fixture.authenticate().await;

    let (status, body) = send(fixture.app(), get("/denials", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "no denial file has been written yet");

    fs::write(fixture.config.data.join("denials.jsonl"), "").expect("an empty denial file");
    let (status, body) = send(fixture.app(), get("/denials", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn the_grant_audit_line_records_the_destructive_flag() {
    let fixture = Fixture::new("audit-grant-destructive");
    let session = fixture.authenticate().await;
    let device_id = register_fixture_device(&fixture, &signing_key(0xa3)).await;

    fixture.create_grant(&session, &device_id, 60).await;
    fixture
        .create_grant_with_flag(&session, &device_id, &["files.write"], 60, true)
        .await;

    let lines = audit_lines(&fixture);
    let grants: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("grant"))
        .collect();
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0]["detail"], json!("allow_destructive=false"));
    assert_eq!(grants[1]["detail"], json!("allow_destructive=true"));
}

/// The audit text across the one rotation and the current file, for "no body was logged" checks.
fn audit_text(fixture: &Fixture) -> String {
    let mut text = String::new();
    for name in ["audit.1.jsonl", "audit.jsonl"] {
        match fs::read_to_string(fixture.config.data.join(name)) {
            Ok(part) => text.push_str(&part),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("the audit file is readable: {error}"),
        }
    }
    text
}

/// Open a registered device's channel and wait until the upgrade callback has registered it.
async fn connected_device(
    fixture: &Fixture,
    server: &TestServer,
    key: &SigningKey,
) -> (String, String, DeviceSocket) {
    let (device_id, token) = fixture.register(key).await;
    let device = open_channel(server, &device_id, &token)
        .await
        .expect("the device channel");
    // The upgrade callback registers the channel in a spawned task; give it one turn.
    tokio::task::yield_now().await;
    (device_id, token, device)
}

#[tokio::test]
async fn an_operator_mcp_call_is_one_audit_line_without_its_payload() {
    let fixture = Fixture::new("audit-mcp");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xa2)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "files.read",
            "arguments": { "path": "request-body-marker-7c1f" },
        },
    });
    let response = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": { "content": "response-body-marker-9d2e" },
    });

    let proxy = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, response.clone()).await;
    };
    let ((status, body), ()) = tokio::join!(proxy, device_side);

    assert_eq!(status, StatusCode::OK, "mcp: {body}");
    assert_eq!(body, response);

    let lines = audit_lines(&fixture);
    let kinds: Vec<Value> = lines.iter().map(|line| line["kind"].clone()).collect();
    assert_eq!(
        kinds,
        vec![
            json!("auth"),
            json!("enroll"),
            json!("register"),
            json!("auth"),
            json!("grant"),
            json!("mcp")
        ],
        "one line per operator action"
    );
    let mcp = lines.last().expect("the mcp line");
    assert_eq!(mcp["status"], json!(200));
    assert_eq!(mcp["device_id"], json!(device_id));
    assert_eq!(mcp["ts_ms"], json!(START_MS));
    assert_eq!(
        mcp["tool"],
        json!("files.read"),
        "the audit names the tool a forwarded call used"
    );
    assert!(
        mcp.get("detail").is_none(),
        "a result needs no detail: {mcp}"
    );
    assert!(
        mcp.get("token_id").is_none(),
        "the operator route names no token: {mcp}"
    );

    let text = audit_text(&fixture);
    for marker in ["request-body-marker-7c1f", "response-body-marker-9d2e"] {
        assert!(
            !text.contains(marker),
            "the audit must never carry a payload body, found {marker}: {text}"
        );
    }
}

#[tokio::test]
async fn a_device_json_rpc_error_is_audited_by_code_not_by_body() {
    let fixture = Fixture::new("audit-mcp-error");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xa3)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "files.read",
            "arguments": { "path": "error-request-marker-4f8a" },
        },
    });
    let error = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "error": {
            "code": -32601,
            "message": "error-message-marker-6b3c",
            "data": { "detail": "error-data-marker-2e5d" },
        },
    });

    let proxy = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({ "device_id": device_id, "grant": grant, "request": request }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        let reply = json!({ "id": frame["id"].clone(), "error": error.clone() });
        device
            .send(WsMessage::Text(reply.to_string()))
            .await
            .expect("the device answers with an error envelope");
    };
    let ((status, body), ()) = tokio::join!(proxy, device_side);

    assert_eq!(status, StatusCode::OK, "a JSON-RPC error is an answer");
    assert_eq!(body, error);

    let lines = audit_lines(&fixture);
    let mcp = lines.last().expect("the mcp line");
    assert_eq!(mcp["kind"], json!("mcp"));
    assert_eq!(mcp["status"], json!(200));
    assert_eq!(mcp["tool"], json!("files.read"));
    assert_eq!(
        mcp["detail"],
        json!("jsonrpc error -32601"),
        "the numeric error code is allowed; the message and data are not"
    );

    let text = audit_text(&fixture);
    for marker in [
        "error-request-marker-4f8a",
        "error-message-marker-6b3c",
        "error-data-marker-2e5d",
    ] {
        assert!(
            !text.contains(marker),
            "the audit must never carry a payload body, found {marker}: {text}"
        );
    }
}

#[tokio::test]
async fn a_bad_operator_token_is_a_denial_line_not_an_audit_line() {
    let fixture = Fixture::new("denial-bad-operator-token");

    let (status, _) = send(
        fixture.app(),
        post_json("/auth", json!({ "token": "00".repeat(32) })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let denied = denial_lines(&fixture).pop().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(401));
    assert_eq!(denied["detail"], json!("bad_operator_token"));
    assert!(
        denied.get("device_id").is_none(),
        "a token refusal names no device: {denied}"
    );
    assert!(
        audit_lines(&fixture).is_empty(),
        "an unauthenticated refusal never reaches the authenticated audit"
    );
}

#[tokio::test]
async fn a_request_without_an_operator_session_is_a_denial_line() {
    let fixture = Fixture::new("denial-bad-session");

    let (status, _) = send(
        fixture.app(),
        post_json(
            "/mcp",
            json!({
                "device_id": "ab".repeat(32),
                "grant": {
                    "grant": {
                        "device_id": "ab".repeat(32),
                        "tools": ["files.read"],
                        "expires_at_ms": START_MS + 60_000,
                        "allow_destructive": false,
                    },
                    "signature": "00".repeat(64),
                },
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let denied = denial_lines(&fixture).pop().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(401));
    assert_eq!(denied["detail"], json!("unauthorized"));
    assert!(audit_lines(&fixture).is_empty());
}

#[tokio::test]
async fn a_spent_enrollment_code_is_a_denial_with_the_device_id() {
    let fixture = Fixture::new("denial-spent-code");
    let key = signing_key(0xa4);
    let session = fixture.authenticate().await;
    let (code, _) = fixture.enroll(&session).await;
    let (status, _) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = fixture
        .register_with_code(&key, &code, fixture.now_ms())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("unknown_code"));

    // The successful registration is an authenticated action; the replay proves no credential and
    // is a denial.
    let audit = audit_lines(&fixture);
    let kinds: Vec<Value> = audit.iter().map(|line| line["kind"].clone()).collect();
    assert_eq!(
        kinds,
        vec![json!("auth"), json!("enroll"), json!("register")],
        "the sign-in, then the actions it bought"
    );

    let denied = denial_lines(&fixture).pop().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(403));
    assert_eq!(denied["detail"], json!("unknown_code"));
    assert_eq!(
        denied["device_id"],
        json!(public_hex(&key)),
        "the denied registration names the key it tried to use"
    );
}

#[tokio::test]
async fn an_expired_grant_is_audited_as_denied_with_its_reason() {
    let fixture = Fixture::new("audit-expired-grant");
    let (device_id, _token) = fixture.register(&signing_key(0xa5)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    fixture.advance_ms(60_000);
    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
            "grant": grant,
                "request": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("grant_expired"));

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(403));
    assert_eq!(denied["detail"], json!("grant_expired"));
    assert_eq!(denied["device_id"], json!(device_id));
}

#[tokio::test]
async fn a_channel_with_a_bad_device_token_is_a_denial_line() {
    let fixture = Fixture::new("denial-bad-device-token");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token) = fixture.register(&signing_key(0xa6)).await;

    let error = open_channel(&server, &device_id, &"cd".repeat(32))
        .await
        .expect_err("a wrong device token is refused");
    assert_eq!(http_status(&error), Some(401));

    let denied = denial_lines(&fixture).pop().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(401));
    assert_eq!(denied["detail"], json!("bad_device_token"));
    assert_eq!(denied["device_id"], json!(device_id));
    assert_eq!(
        audit_lines(&fixture).len(),
        3,
        "the registration's sign-in, enroll and register lines are the audit lines"
    );
}

#[tokio::test]
async fn the_audit_cap_rotates_to_one_generation_and_discards_the_older_one() {
    const CAP: u64 = 200;
    let fixture = Fixture::with_audit_cap("audit-rotation", CAP);
    let session = fixture.authenticate().await;
    // An authenticated refusal for an unknown device is one audit line: forty of them are forty
    // actions without spending enrollment codes or waiting for a device.
    for _ in 0..40 {
        let (status, _) = send(
            fixture.app(),
            post_json_bearer(
                "/grants",
                &session,
                json!({
                    "device_id": "ab".repeat(32),
                    "tools": ["files.read"],
                    "expires_in_s": 60,
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let current = fixture.config.data.join("audit.jsonl");
    let rotated = fixture.config.data.join("audit.1.jsonl");
    let current_text = fs::read_to_string(&current).expect("the current audit file exists");
    let rotated_text = fs::read_to_string(&rotated).expect("the previous generation exists");
    assert!(
        !fixture.config.data.join("audit.2.jsonl").exists(),
        "one rotation keeps exactly one older generation"
    );

    for text in [&rotated_text, &current_text] {
        assert!(!text.is_empty(), "no generation is empty");
        assert!(
            (text.len() as u64) <= CAP,
            "a generation must not grow past the cap: {} > {CAP}",
            text.len()
        );
        for line in text.lines() {
            let value: Value =
                serde_json::from_str(line).expect("rotation never splits a line from its JSON");
            assert!(
                value["kind"] == json!("denied") || value["kind"] == json!("auth"),
                "a generation holds refusals and sign-ins, nothing else: {value}"
            );
        }
    }

    // Forty actions were written, but the cap keeps only the newest generations. `/audit` reads the
    // current file first and stitches the rotated generation behind it.
    let session = fixture.authenticate().await;
    let (status, body) = send(fixture.app(), get("/audit?limit=1000", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    let lines = body.as_array().expect("a list of audit lines");
    assert!(!lines.is_empty());
    assert!(
        lines.len() < 40,
        "the older generations are discarded, not accumulated: {} lines",
        lines.len()
    );
    // The newest line is the sign-in this read needed; the refusals sit behind it.
    assert_eq!(lines[0]["kind"], json!("auth"));
}

#[tokio::test]
async fn a_request_that_arrives_between_channels_waits_for_the_next_one() {
    let fixture = Fixture::new("channel-reconnect-wait");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0xb1)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id.clone(),
            "request": { "jsonrpc": "2.0", "id": 7, "method": "tools/list" },
        }),
    );
    let started = std::time::Instant::now();
    let proxy = tokio::spawn(async move { send(app, request).await });

    // No channel is live when the request arrives. The device dials back in well inside the
    // two-second bound, and the queued exchange runs on the channel that replaces it.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut device = open_channel(&server, &device_id, &token)
        .await
        .expect("the replacement channel");
    let frame = next_json(&mut device).await;
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 7, "result": { "ok": true } }),
    )
    .await;

    let (status, body) = proxy.await.expect("the proxy task finishes");
    assert_eq!(status, StatusCode::OK, "mcp: {body}");
    assert_eq!(body["result"], json!({ "ok": true }));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the new channel was found inside the bound: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_replaced_channel_fails_an_in_flight_request_as_502_without_retry_after() {
    let fixture = Fixture::new("channel-replaced-in-flight");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb2)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id.clone(),
            "request": { "jsonrpc": "2.0", "id": 7, "method": "tools/list" },
        }),
    );
    let started = std::time::Instant::now();
    let proxy = tokio::spawn(async move { send_with_headers(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["method"],
        json!("tools/list"),
        "the request really was written to the old channel"
    );

    let _replacement = open_channel(&server, &device_id, &token)
        .await
        .expect("the replacement channel");

    let (status, headers, body) = proxy.await.expect("the proxy task finishes");
    assert_eq!(
        status,
        StatusCode::BAD_GATEWAY,
        "a frame that was written and never answered is a gateway fault"
    );
    assert!(
        headers.get(header::RETRY_AFTER).is_none(),
        "the device may have executed it: the answer must not tell the client to retry"
    );
    assert_eq!(
        body,
        json!({ "error": "device disconnected" }),
        "an unanswered frame is not falsely retryable"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the in-flight request failed inside the bound: {:?}",
        started.elapsed()
    );

    let lines = audit_lines(&fixture);
    let denied = lines.last().expect("the denial line");
    assert_eq!(denied["kind"], json!("denied"));
    assert_eq!(denied["status"], json!(502));
    assert_eq!(denied["detail"], json!("device disconnected"));
}

#[tokio::test]
async fn a_channel_that_dies_mid_request_is_502() {
    let fixture = Fixture::new("channel-dies-in-flight");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb3)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id,
            "request": { "jsonrpc": "2.0", "id": 7, "method": "tools/list" },
        }),
    );
    let proxy = tokio::spawn(async move { send_with_headers(app, request).await });

    let _frame = next_json(&mut device).await;
    drop(device);

    let (status, headers, body) = tokio::time::timeout(Duration::from_secs(5), proxy)
        .await
        .expect("the proxy returns when its channel dies")
        .expect("the proxy task finishes");
    assert_eq!(
        status,
        StatusCode::BAD_GATEWAY,
        "a channel that closed is an unknown outcome, not a retryable outage"
    );
    assert!(
        headers.get(header::RETRY_AFTER).is_none(),
        "the outcome is unknown; no retry hint"
    );
    assert_eq!(body, json!({ "error": "device disconnected" }));
}

#[tokio::test]
async fn unauthenticated_denials_are_rated_and_cannot_rotate_the_audit() {
    const CAP: u64 = 200;
    let fixture = Fixture::with_audit_cap("denial-rate-limit", CAP);
    let session = fixture.authenticate().await;
    fixture.enroll(&session).await;

    for _ in 0..200 {
        let (status, _) = send(fixture.app(), get("/fleet", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // The authenticated index is untouched: one enroll line, no rotation, no denial text.
    let audit = audit_lines(&fixture);
    assert_eq!(
        audit.len(),
        2,
        "the sign-in and the enroll: authenticated actions, no denial"
    );
    assert_eq!(audit[0]["kind"], json!("auth"));
    assert_eq!(audit[1]["kind"], json!("enroll"));
    assert!(
        !fixture.config.data.join("audit.1.jsonl").exists(),
        "unauthenticated traffic must not rotate the audit"
    );
    assert!(!audit_text(&fixture).contains("unauthorized"));

    // The denial log holds the first window and suppresses the rest.
    let denials = denial_lines(&fixture);
    assert_eq!(
        denials.len(),
        100,
        "one window holds at most 100 lines; the summary arrives with the next window"
    );
    for line in &denials {
        assert_eq!(line["kind"], json!("denied"));
        assert_eq!(line["status"], json!(401));
        assert_eq!(line["detail"], json!("unauthorized"));
    }

    // The window rolls on the injected clock: one summary accounts for the 100 suppressed, then the
    // new window's first line is written.
    fixture.advance_ms(60_000);
    let (status, _) = send(fixture.app(), get("/fleet", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let denials = denial_lines(&fixture);
    assert_eq!(denials.len(), 102);
    let summary = &denials[100];
    assert_eq!(summary["kind"], json!("denied"));
    assert_eq!(summary["status"], json!(429));
    assert_eq!(summary["detail"], json!("suppressed 100"));
    assert_eq!(summary["ts_ms"], json!(START_MS + 60_000));
    assert_eq!(denials[101]["status"], json!(401));
}

// ---- the root and the Rust page ----------------------------------------------------------------

#[tokio::test]
async fn the_root_sends_a_bookmark_to_the_rust_page() {
    let fixture = Fixture::new("root-redirect");

    let (status, headers, _) = send_text(fixture.app(), get("/", None)).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/next"),
        "the old root points at the page that replaced it"
    );
}

#[tokio::test]
async fn the_served_tool_catalogue_is_the_registry() {
    // The page fetches /tools instead of carrying a copy; this is the route it fetches. The list must be
    // the device's own registry — 38 tools, same order, same sentences — or the grant form would offer
    // a surface the device does not have (or miss one it does).
    let fixture = Fixture::new("page-tools");
    let session = fixture.authenticate().await;
    let (status, tools) = send(fixture.app(), get("/tools", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    let served: Vec<&str> = tools
        .as_array()
        .expect("a list")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name"))
        .collect();
    let declared: Vec<&str> = summrise_core::registry::CAPABILITIES
        .iter()
        .flat_map(|capability| capability.tools)
        .map(|tool| tool.name)
        .collect();
    assert_eq!(served, declared, "the console serves the device's registry");
    for tool in tools.as_array().expect("a list") {
        let name = tool["name"].as_str().expect("a name");
        let expected = summrise_core::registry::tool(name).expect("the tool is in the registry");
        assert_eq!(tool["description"], expected.description, "{name}");
        assert_eq!(tool["destructive"], expected.destructive, "{name}");
    }

    // The Rust page carries no catalogue of its own: the device's `tools/list` is the only source,
    // and the server renders what it needs. Nothing to assert on the page, which is the point.
}

#[tokio::test]
async fn the_tool_catalogue_is_not_served_without_an_operator() {
    let fixture = Fixture::new("page-tools-gate");
    let (status, _) = send(fixture.app(), get("/tools", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_page_holds_no_secret_and_says_the_token_stays_in_memory() {
    let fixture = Fixture::new("page-secret");
    // The login page is what an unauthenticated visitor sees; the signed-in page is behind the cookie.
    let (status, _, page) = send_text(fixture.app(), get("/next", None)).await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        !page.contains(&hex(&OPERATOR_TOKEN)),
        "the operator token must never be embedded in the page"
    );
    // The SESSION token may be remembered in the tab (it expires and it is not the operator's
    // secret); the operator token may not be persisted anywhere, and nothing may be written to
    // localStorage or a cookie at all.
    for forbidden in [
        "localStorage.setItem",
        "localStorage.getItem",
        "document.cookie",
    ] {
        assert!(
            !page.contains(forbidden),
            "the page must not persist anything through {forbidden}"
        );
    }
    assert!(
        !page.contains("sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: operator"),
        "the operator token is never written to storage"
    );
}

#[tokio::test]
async fn the_session_probe_says_what_it_made_of_the_access_header() {
    let fixture = Fixture::new("session-access-report");

    let (status, body) = send(fixture.app(), get("/session", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["access"]["configured"], false);
    assert_eq!(body["access"]["header"], false);

    // A header that is present but cannot be verified must not read as "no header": the page's
    // sentence differs, and so does the thing the operator should do about it.
    let request = Request::builder()
        .method("GET")
        .uri("/session")
        .header("cf-access-jwt-assertion", "not-a-jwt")
        .body(Body::empty())
        .expect("the request builds");
    let (status, body) = send(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["access"]["configured"], false);
    assert_eq!(body["access"]["header"], true);
}

/// The page's gate is a cookie; Cloudflare Access is the identity in front of it. An operator the
/// edge already authenticated was never given a token to type, so a verified header must sign them
/// in — the token box is the break-glass path, not the front door.
#[tokio::test]
async fn a_verified_access_request_signs_the_page_in_without_a_token() {
    let fixture = Fixture::with_access("access-sign-in");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");

    let (status, headers, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("New enrollment code"),
        "the signed-in fleet, not the token box: {page}"
    );
    assert!(!page.contains("Operator token"), "no token box: {page}");
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("the answer sets the session it just minted");
    assert!(cookie.starts_with("summrise_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
}

/// A header that does not verify is not an identity: the token box stays, and nothing is minted for
/// whoever sent it.
#[tokio::test]
async fn an_access_header_that_does_not_verify_leaves_the_token_box() {
    let fixture = Fixture::with_access("access-refused");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, "not-a-jwt")
        .body(Body::empty())
        .expect("the request builds");

    let (status, headers, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Operator token"), "{page}");
    assert!(
        headers.get(header::SET_COOKIE).is_none(),
        "a refused identity mints nothing"
    );
}

/// Only a GET signs in: a cross-site form POST that the edge happens to see must not mint a
/// session, and a mutation still needs the session's own CSRF value.
#[tokio::test]
async fn a_post_carrying_only_an_access_header_mints_nothing() {
    let fixture = Fixture::with_access("access-post");
    let request = Request::builder()
        .method("POST")
        .uri("/next/fleet/device/name")
        .header(ACCESS_HEADER, access_token(60))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("csrf=00&name=renamed"))
        .expect("the request builds");

    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        headers.get(header::SET_COOKIE).is_none(),
        "a POST mints nothing"
    );
}

/// Signing out has to end the session Access itself holds: clearing our cookie alone would be
/// undone by the next request, which the edge would authenticate again.
#[tokio::test]
async fn signing_out_behind_access_ends_the_access_session() {
    let fixture = Fixture::with_access("access-sign-out");
    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let set = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII");
    let cookie = set.split(';').next().expect("the name=value pair");

    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", cookie)).await;
    let csrf = csrf_from(&page);

    let request = Request::builder()
        .method("POST")
        .uri("/next/logout")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}")))
        .expect("the request builds");
    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("https://test-team.cloudflareaccess.com/cdn-cgi/access/logout"),
        "our cookie is not the session that outlives us"
    );
}

/// `--login-link` prints `<page>?code=…` and promises two minutes' way in. The page has to spend
/// that code, or the only recovery left when Access misbehaves is a secret in a file on the host.
#[tokio::test]
async fn an_unknown_path_is_a_page_with_a_way_back() {
    let fixture = Fixture::new("not-found");
    // Unauthenticated: the 404 is the same page whoever asks, so a typo never becomes a login wall.
    let (status, headers, body) = send_text(fixture.app(), get("/next/no-such-page", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    assert!(body.contains("Not here"), "{body}");
    assert!(
        body.contains("href=\"/next\""),
        "the page has a way back: {body}"
    );
    assert!(
        !body.contains("no-such-page"),
        "the path is not echoed: {body}"
    );
}

#[tokio::test]
async fn a_login_link_code_signs_the_page_in_exactly_once() {
    let fixture = Fixture::new("login-link");
    let code = LoginCode {
        code: "a".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(&fixture.config.data, &code).expect("the code is written");
    let uri = format!("/next?code={}", code.code);

    let (status, headers, _) = send_text(fixture.app(), get(&uri, None)).await;

    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "a link signs in and redirects"
    );
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/next"),
        "the code leaves the URL with the redirect"
    );
    assert!(
        headers
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|cookie| cookie.starts_with("summrise_session=")),
        "the link mints the page's session"
    );

    // A link is one use: a second visit says so instead of minting another session.
    let (status, headers, page) = send_text(fixture.app(), get(&uri, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("not accepted"),
        "the card explains the dead link: {page}"
    );
    assert!(headers.get(header::SET_COOKIE).is_none());
}

/// A code that does not match is refused like an expired one — and the card still has the token
/// path under it, because a dead link must not be a dead end.
#[tokio::test]
async fn a_login_link_that_does_not_match_leaves_the_token_box() {
    let fixture = Fixture::new("login-link-refused");
    let code = LoginCode {
        code: "b".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(&fixture.config.data, &code).expect("the code is written");

    let (status, headers, page) =
        send_text(fixture.app(), get("/next?code=not-the-code", None)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Operator token"), "{page}");
    assert!(headers.get(header::SET_COOKIE).is_none());
}

/// The card says which of the two Access failures happened, because the page's sign-in now depends
/// on the header arriving: silence here is what used to look like a verification bug. A request
/// through the edge carries `cf-ray`, which is how the page knows Access was in the path at all.
#[tokio::test]
async fn the_card_says_when_access_did_not_identify_the_request() {
    let fixture = Fixture::with_access("card-access-missing");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header("cf-ray", "8a00000000000000-TEST")
        .body(Body::empty())
        .expect("the request builds");

    let (status, _, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Cloudflare Access did not identify this request"),
        "{page}"
    );
}

#[tokio::test]
async fn the_card_says_when_access_did_not_accept_the_request() {
    let fixture = Fixture::with_access("card-access-refused");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, "not-a-jwt")
        .body(Body::empty())
        .expect("the request builds");

    let (status, _, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Cloudflare Access did not accept this request"),
        "{page}"
    );
}

/// Without Access there is no edge to explain: the card must not invent one.
#[tokio::test]
async fn the_card_is_quiet_when_access_is_not_configured() {
    let fixture = Fixture::new("card-no-access");

    let (_, _, page) = send_text(fixture.app(), get("/next", None)).await;

    assert!(!page.contains("Cloudflare Access"), "{page}");
}

/// The operator who opens the console where no edge is in the path (the tailnet address, a smoke
/// run) is not being asked to sign in at Cloudflare: there is nothing there to sign in to.
#[tokio::test]
async fn the_card_is_quiet_on_a_direct_request_when_access_is_configured() {
    let fixture = Fixture::with_access("card-access-direct");

    let (_, _, page) = send_text(fixture.app(), get("/next", None)).await;

    assert!(
        !page.contains("Cloudflare Access did not identify"),
        "a direct request must not send the operator to an edge that is not in the path: {page}"
    );
}

/// The one operator string that reaches a filesystem is refused by the page itself: a traversal
/// never becomes a device round trip.
#[tokio::test]
async fn a_files_write_outside_the_root_is_refused_by_the_page() {
    let fixture = Fixture::new("files-traversal");
    let (device_id, _) = fixture.register(&signing_key(0xf7)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/files/{device_id}/write"),
        &[
            ("csrf", &csrf),
            ("path", "../windows/system32/x"),
            ("content", "x"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(
        location.contains("cannot%20contain"),
        "the page's own refusal, not the device's: {location}"
    );
}

/// A revoke form on device A's page must not destroy a token that belongs to device B.
#[tokio::test]
async fn a_token_is_only_revoked_from_a_page_that_can_reach_it() {
    let fixture = Fixture::new("token-scope");
    let (device_a, _) = fixture.register(&signing_key(0xf8)).await;
    let (device_b, _) = fixture.register(&signing_key(0xf9)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/tokens/{device_a}/mint"),
        &[("csrf", &csrf), ("name", "scoped")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);

    let (_, _, listed) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/tokens/{device_a}"), &cookie),
    )
    .await;
    let marker = "name=\"id\" value=\"";
    let start = listed.find(marker).expect("a revoke form") + marker.len();
    let end = start + listed[start..].find('"').expect("the id ends");
    let id = listed[start..end].to_owned();

    // Device B's page names a token it cannot reach: refused, and the token survives.
    let mut request = post_form(
        &format!("/next/tokens/{device_b}/revoke"),
        &[("csrf", &csrf), ("id", &id)],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, _, after) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/tokens/{device_a}"), &cookie),
    )
    .await;
    assert!(
        after.contains("scoped"),
        "the token is still there: {after}"
    );
}

/// `/session` is a probe a client can call with the page's cookie; before this it could only see a
/// bearer token, so `via: "session"` was unreachable and a signed-in page read as signed out.
#[tokio::test]
async fn the_session_probe_sees_a_page_cookie() {
    let fixture = Fixture::new("session-probe-cookie");
    let cookie = page_cookie(&fixture).await;
    let request = Request::builder()
        .method("GET")
        .uri("/session")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("the request builds");

    let (status, body) = send(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["via"], "session");
}

/// Every way in leaves an audit line naming the way, and nothing about the credential. The console
/// could not previously answer "did the edge deliver the header?" from its own record — a question
/// that cost an operator an hour when the page fell back to the token box.
#[tokio::test]
async fn each_way_in_writes_an_auth_line() {
    let fixture = Fixture::with_access("auth-audit");

    // 1. Cloudflare Access: a verified assertion and no cookie.
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");
    send_text(fixture.app(), request).await;

    // 2. The API's token exchange, whose session then reads the log.
    let session = fixture.authenticate().await;

    // 3. The page's token form.
    let _ = page_cookie(&fixture).await;

    // 4. A `--login-link` code.
    let code = LoginCode {
        code: "f".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(&fixture.config.data, &code).expect("the code is written");
    send_text(
        fixture.app(),
        get(&format!("/next?code={}", code.code), None),
    )
    .await;

    let (status, rows) = send(fixture.app(), get("/audit", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    let details: Vec<&str> = rows
        .as_array()
        .expect("the audit is a list")
        .iter()
        .filter(|row| row["kind"] == "auth")
        .filter_map(|row| row["detail"].as_str())
        .collect();

    for way in ["access", "token", "login-link"] {
        assert!(
            details.contains(&way),
            "a {way} sign-in is in the audit: {details:?}"
        );
    }
    // The row names the way in, never the secret it came with.
    let text = rows.to_string();
    assert!(!text.contains(&hex(&OPERATOR_TOKEN)), "{text}");
    assert!(!text.contains(&code.code), "{text}");
}

/// Presets are operator convenience the console stores, and the terminal page is where they are
/// used: saving one has to show up on the page that will open a session from it.
#[tokio::test]
async fn a_saved_connection_is_listed_on_the_terminal_page() {
    let fixture = Fixture::new("presets-list");
    let (device_id, _) = fixture.register(&signing_key(0xf2)).await;
    let cookie = page_cookie(&fixture).await;
    let page_uri = format!("/next/terminal/{device_id}");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let csrf = csrf_from(&page);

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/terminal/{device_id}/connections"),
            &[
                ("csrf", &csrf),
                ("name", "Edge box"),
                ("kind", "ssh"),
                ("target", "root@10.0.0.5:2222"),
            ],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "a save redirects back");

    let (status, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Edge box"), "{page}");
    assert!(page.contains("10.0.0.5"), "{page}");
}

/// Reachability is a question an operator asks the device ("can you see X:443?"), and the answer is
/// a summary in the page's notice slot.
#[tokio::test]
async fn the_system_page_can_test_reachability_from_the_device() {
    let fixture = Fixture::new("system-net-test");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe5)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/system/{device_id}/net"),
        &[("csrf", &csrf), ("host", "10.0.0.5"), ("port", "443")],
        &cookie,
    );
    let test = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("system.net_test"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["host"],
        json!("10.0.0.5")
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["port"], json!(443));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({
                // The device's own shape: the address that connected, not a boolean.
                "addresses": ["10.0.0.5:443"], "connected": "10.0.0.5:443", "latency_ms": 3, "error": null
            }).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, headers, _) = test.await.expect("the test finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(location.contains("net="), "{location}");
    assert!(location.contains("reachable"), "{location}");
    assert!(
        location.contains("connected%20to%2010.0.0.5%3A443"),
        "the summary names the address that answered: {location}"
    );
}

/// The form is on the page it acts on, next to the process tools.
#[tokio::test]
async fn the_system_page_offers_a_reachability_check() {
    let fixture = Fixture::new("system-net-form");
    let (device_id, _) = fixture.register(&signing_key(0xe4)).await;
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/system/{device_id}"), &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(&format!("/next/system/{device_id}/net")),
        "{page}"
    );
    assert!(page.contains("Test reachability"), "{page}");
}

/// A restart is behind a second click and reaches `host.restart`; the answer says the device is
/// coming back rather than pretending the page is unchanged.
#[tokio::test]
async fn the_system_page_can_restart_the_device() {
    let fixture = Fixture::new("system-restart");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe7)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/system/{device_id}/restart"),
        &[("csrf", &csrf)],
        &cookie,
    );
    let restart = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("host.restart"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "{\"restarting\":true}" }], "isError": false }
        }),
    )
    .await;

    let (status, headers, _) = restart.await.expect("the restart finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(
        location.contains("restart=asked"),
        "the page says the device is coming back: {location}"
    );
}

/// The restart is offered where the processes are, behind a disclosure, with its consequence said.
#[tokio::test]
async fn the_system_page_offers_the_restart_behind_a_disclosure() {
    let fixture = Fixture::new("system-restart-form");
    let (device_id, _) = fixture.register(&signing_key(0xe6)).await;
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/system/{device_id}"), &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(&format!("/next/system/{device_id}/restart")),
        "{page}"
    );
    assert!(page.contains("Restart now"), "{page}");
}

/// The device's missing-parent refusal tells the operator to use `files.mkdir`; the page has to be
/// able to do it, or it is pointing at a door that does not exist.
#[tokio::test]
async fn a_directory_can_be_created_from_the_files_page() {
    let fixture = Fixture::new("files-mkdir");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xea)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/files/{device_id}/mkdir"),
        &[("csrf", &csrf), ("path", "logs/2026"), ("current", "logs")],
        &cookie,
    );
    let create = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.mkdir"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["path"],
        json!("logs/2026")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "{}" }], "isError": false }
        }),
    )
    .await;

    let (status, headers, _) = create.await.expect("the mkdir finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/files/{device_id}?path=logs").as_str()),
        "the listing the operator was looking at comes back"
    );
}

/// The page's path rule covers the new folder too.
#[tokio::test]
async fn a_mkdir_path_outside_the_root_is_refused_by_the_page() {
    let fixture = Fixture::new("files-mkdir-traversal");
    let (device_id, _) = fixture.register(&signing_key(0xe9)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/files/{device_id}/mkdir"),
            &[("csrf", &csrf), ("path", "../escape")],
            &cookie,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(location.contains("cannot%20contain"), "{location}");
}

/// A watched target can be probed now, so a fresh status does not wait for the interval.
#[tokio::test]
async fn a_watched_target_can_be_probed_now() {
    let fixture = Fixture::new("monitor-probe-now");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe8)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/monitors/{device_id}/probe"),
        &[("csrf", &csrf), ("id", "target-1")],
        &cookie,
    );
    let probe = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("monitor.probe"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["id"], json!("target-1"));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({"ts_ms": 1, "ok": true, "latency_ms": 3}).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, headers, _) = probe.await.expect("the probe finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/monitors/{device_id}").as_str())
    );
}

/// An ssh preset that names a key on the device is like a serial one: it can open a session from the
/// rail without a credential typed in, because a key path is configuration and the key stays put.
#[tokio::test]
async fn an_ssh_preset_with_a_key_connects_from_the_rail() {
    let fixture = Fixture::new("preset-key-connect");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xec)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let page_uri = format!("/next/terminal/{device_id}");

    let mut request = post_form(
        &format!("{page_uri}/connections"),
        &[
            ("csrf", &csrf),
            ("name", "Edge"),
            ("kind", "ssh"),
            ("target", "root@10.0.0.5:2222"),
            ("key_path", "C:\\Users\\op\\.ssh\\id_ed25519"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let app = fixture.app();
    let request = get_with_cookie(&page_uri, &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "[]" }], "isError": false }
        }),
    )
    .await;
    let (_, _, body) = page.await.expect("the page finishes");
    let preset = first_preset_id(&body);
    assert!(
        body.contains(&format!("/connections/{preset}/connect")),
        "a key-bearing ssh preset connects from the rail: {body}"
    );

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("{page_uri}/connections/{preset}/connect"),
        &[("csrf", &csrf)],
        &cookie,
    );
    let connect = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("terminal.create"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["auth"]["kind"],
        json!("key"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["auth"]["path"],
        json!("C:\\Users\\op\\.ssh\\id_ed25519"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "id": "session-77", "kind": "ssh", "shell": "root@10.0.0.5" }).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, headers, _) = connect.await.expect("the connect finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("{page_uri}?attach=session-77").as_str())
    );
}

/// A key file belongs to the kind that uses one: a pty or serial preset that names one is a mistake,
/// refused where it is made.
#[tokio::test]
async fn a_key_file_on_a_pty_preset_is_refused() {
    let fixture = Fixture::new("preset-key-pty");
    let (device_id, _) = fixture.register(&signing_key(0xeb)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/terminal/{device_id}/connections"),
        &[
            ("csrf", &csrf),
            ("name", "Shell"),
            ("kind", "pty"),
            ("target", ""),
            ("key_path", "C:\\Users\\op\\.ssh\\id_ed25519"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(
        location.contains("only%20an%20ssh%20preset"),
        "the page says why: {location}"
    );
}

/// The id of the first preset the page rendered, read off its remove form's action.
fn first_preset_id(page: &str) -> String {
    let marker = "/connections/";
    let start = page.find(marker).expect("a preset row") + marker.len();
    page[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect()
}

/// Saving and forgetting are the two halves of the same control; a forgotten preset must leave the
/// page, not merely the file.
#[tokio::test]
async fn a_preset_can_be_forgotten_from_the_page() {
    let fixture = Fixture::new("presets-forget");
    let (device_id, _) = fixture.register(&signing_key(0xf3)).await;
    let cookie = page_cookie(&fixture).await;
    let page_uri = format!("/next/terminal/{device_id}");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let csrf = csrf_from(&page);

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("{page_uri}/connections"),
            &[
                ("csrf", &csrf),
                ("name", "Old box"),
                ("kind", "pty"),
                ("target", ""),
            ],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(page.contains("Old box"), "{page}");
    let preset = first_preset_id(&page);

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("{page_uri}/connections/{preset}/remove"),
            &[("csrf", &csrf)],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(!page.contains("Old box"), "the row is gone: {page}");
}

/// An ssh preset carries no credential, so "Use" fills the form and leaves the password blank —
/// that is the whole reason the preset may store host, user and port but nothing else.
#[tokio::test]
async fn an_ssh_preset_prefills_the_form_but_not_a_password() {
    let fixture = Fixture::new("presets-prefill");
    let (device_id, _) = fixture.register(&signing_key(0xf4)).await;
    let cookie = page_cookie(&fixture).await;
    let page_uri = format!("/next/terminal/{device_id}");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let csrf = csrf_from(&page);

    send_text(
        fixture.app(),
        post_form_cookie(
            &format!("{page_uri}/connections"),
            &[
                ("csrf", &csrf),
                ("name", "Edge box"),
                ("kind", "ssh"),
                ("target", "root@10.0.0.5:2222"),
            ],
            &cookie,
        ),
    )
    .await;
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let preset = first_preset_id(&page);

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("{page_uri}?preset={preset}"), &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("value=\"10.0.0.5\""), "{page}");
    assert!(page.contains("value=\"root\""), "{page}");
    assert!(page.contains("value=\"2222\""), "{page}");
    assert!(
        page.contains("option value=\"ssh\" selected"),
        "the kind follows the preset: {page}"
    );
    assert!(
        !page.contains("data-kind=\"ssh\" hidden"),
        "the prefilled group is visible without touching the select: {page}"
    );
    assert!(
        !page.contains("value=\"root\" type=\"password\""),
        "no password is ever prefilled: {page}"
    );
}

/// The password is the preset's deliberate omission, so connecting an ssh preset from the page has
/// to say so instead of failing at the device with a worse sentence.
#[tokio::test]
async fn connecting_an_ssh_preset_asks_for_the_password() {
    let fixture = Fixture::new("presets-ssh-connect");
    let (device_id, _) = fixture.register(&signing_key(0xf5)).await;
    let cookie = page_cookie(&fixture).await;
    let page_uri = format!("/next/terminal/{device_id}");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let csrf = csrf_from(&page);
    send_text(
        fixture.app(),
        post_form_cookie(
            &format!("{page_uri}/connections"),
            &[
                ("csrf", &csrf),
                ("name", "Edge box"),
                ("kind", "ssh"),
                ("target", "root@10.0.0.5"),
            ],
            &cookie,
        ),
    )
    .await;
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    let preset = first_preset_id(&page);

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("{page_uri}/connections/{preset}/connect"),
            &[("csrf", &csrf)],
            &cookie,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie(location, &cookie)).await;
    assert!(
        page.contains("needs its password typed on the page"),
        "{page}"
    );
}

/// The presets are mutations of the console's own file, so the page's CSRF rule covers them.
#[tokio::test]
async fn a_preset_cannot_be_saved_without_the_sessions_csrf() {
    let fixture = Fixture::new("presets-csrf");
    let (device_id, _) = fixture.register(&signing_key(0xf6)).await;
    let cookie = page_cookie(&fixture).await;

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/terminal/{device_id}/connections"),
            &[
                ("csrf", &"00".repeat(32)),
                ("name", "Nope"),
                ("kind", "pty"),
                ("target", ""),
            ],
            &cookie,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// `/health` is what a supervisor and a human check first; whether Access is configured is the fact
/// that explains why the page behaves the way it does, and it is not a secret.
#[tokio::test]
async fn health_says_whether_access_is_configured() {
    let plain = Fixture::new("health-no-access");
    let (status, body) = send(plain.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["access_configured"], false);

    let trusted = Fixture::with_access("health-access");
    let (status, body) = send(trusted.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["access_configured"], true);
}

/// The page names who signed the operator in. A person who was told to type a token they never had
/// deserves to see, on the page, that Access — not the token — opened it.
#[tokio::test]
async fn the_dashboard_names_access_as_the_signer() {
    let fixture = Fixture::with_access("dashboard-signer-access");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");

    let (status, _, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Signed in through Cloudflare Access as ")
            && page.contains("operator@example.test"),
        "the page names Access and the identity it verified: {page}"
    );
}

/// Without Access the sentence has to be the other one, because the token really did sign them in.
#[tokio::test]
async fn the_dashboard_says_the_token_signed_in_without_access() {
    let fixture = Fixture::new("dashboard-signer-token");
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Signed in with this browser's session"),
        "{page}"
    );
}

/// The explanation is not the landing page's privilege: a bookmarked deep link that lands signed out
/// gets the same sentence, because that is where an operator without a session actually arrives.
#[tokio::test]
async fn a_signed_out_deep_page_explains_the_edge_too() {
    let fixture = Fixture::with_access("deep-page-note");
    let request = Request::builder()
        .method("GET")
        .uri("/next/audit")
        .header("cf-ray", "8a00000000000000-TEST")
        .body(Body::empty())
        .expect("the request builds");

    let (status, _, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Cloudflare Access did not identify this request"),
        "{page}"
    );
}

/// The page mints a session for an Access-verified request with no cookie, so a browser that refuses
/// cookies would append one per page load. The file keeps the newest few, not everything since the
/// first deploy.
#[tokio::test]
async fn the_session_store_keeps_at_most_thirty_two_sessions() {
    let fixture = Fixture::new("session-cap");
    let mut newest = String::new();
    for _ in 0..=32 {
        let (status, headers, _) = send_text(
            fixture.app(),
            post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        newest = headers
            .get(header::SET_COOKIE)
            .expect("a session cookie")
            .to_str()
            .expect("a cookie is ASCII")
            .to_owned();
    }

    let text = fs::read_to_string(fixture.config.data.join("sessions.json"))
        .expect("the sessions file is written");
    let rows: Vec<Value> = serde_json::from_str(&text).expect("the file is JSON");
    assert_eq!(rows.len(), 32, "the store is bounded");

    let cookie = newest.split(';').next().expect("the name=value pair");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", cookie)).await;
    assert!(
        page.contains("New enrollment code"),
        "the newest session is the one kept: {page}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_session_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("session-mode");
    let _ = page_cookie(&fixture).await;

    let mode = fs::metadata(fixture.config.data.join("sessions.json"))
        .expect("the sessions file exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "a session hash is credential-grade");
}

/// Sign-out must mean something even in the tab whose cookie is gone: the edge's session is the one
/// that would sign it straight back in.
#[tokio::test]
async fn signing_out_without_a_session_still_ends_the_access_session() {
    let fixture = Fixture::with_access("access-sign-out-no-cookie");
    let request = Request::builder()
        .method("POST")
        .uri("/next/logout")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .expect("the request builds");

    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("https://test-team.cloudflareaccess.com/cdn-cgi/access/logout"),
        "the edge's session outlives ours"
    );
    let cleared = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok());
    assert!(
        cleared.is_some_and(|cookie| cookie.contains("Secure")),
        "the clear mirrors the cookie it clears: {cleared:?}"
    );
}

/// Without an edge there is nothing to sign out of: the tab goes home.
#[tokio::test]
async fn signing_out_without_access_goes_home() {
    let fixture = Fixture::new("sign-out-home");

    let (status, headers, _) = send_text(fixture.app(), post_form("/next/logout", &[])).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/next")
    );
}

/// A one-time code must not outlive itself in the recovery copy: minting a second link must not
/// leave the first code on disk, and spending one must clear a leftover copy from an older build.
#[tokio::test]
async fn a_login_code_leaves_no_recovery_copy() {
    let fixture = Fixture::new("login-code-backup");
    let data = &fixture.config.data;
    let first = LoginCode {
        code: "d".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    let second = LoginCode {
        code: "e".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(data, &first).expect("the first code is written");
    summrise_console::write_login_code(data, &second).expect("the second code is written");

    let backup = data.join("login.json.bak");
    assert!(
        !backup.exists(),
        "the previous code must not survive in a copy"
    );

    fs::write(&backup, "a leftover from an older build").expect("the leftover exists");
    let (status, _, _) = send_text(
        fixture.app(),
        get(&format!("/next?code={}", second.code), None),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(!backup.exists(), "spending the code removes the copy too");
}

/// `--login-link https://console.saisi.online` prints `<public-url>?code=…`. The bare root is the
/// URL that command would be given, and its redirect to the page must carry the code, not drop it.
#[tokio::test]
async fn the_root_redirect_keeps_the_query_a_login_link_carries() {
    let fixture = Fixture::new("root-query");

    let (status, headers, _) = send_text(fixture.app(), get("/?code=abc", None)).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/next?code=abc")
    );
}

/// The login box takes the one-time code too: `--login-link` says its code is for that box, and a
/// code pasted there used to be decoded as a 32-byte token and refused.
#[tokio::test]
async fn the_login_box_accepts_a_one_time_code() {
    let fixture = Fixture::new("login-box-code");
    let code = LoginCode {
        code: "c".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(&fixture.config.data, &code).expect("the code is written");

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("code", &code.code)]),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|cookie| cookie.starts_with("summrise_session=")),
        "a spent code buys the same session a token does"
    );
}

/// Two 64-hex parameters, two meanings: an enrollment code is shown on the page, a login code is
/// spent. A login link must never be printed as "Enrollment code".
#[tokio::test]
async fn a_login_code_in_the_url_is_not_shown_as_an_enrollment_code() {
    let fixture = Fixture::new("code-not-enrol");
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie("/next?code=deadbeef", &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains("Enrollment code:"), "{page}");
}

/// A refused sign-in says what the edge said, not only that the credential was wrong: the refusal
/// re-renders the card, and that card is where the operator found the missing sentence before.
#[tokio::test]
async fn the_refusal_card_carries_the_access_note() {
    let fixture = Fixture::with_access("refusal-note");
    let request = Request::builder()
        .method("POST")
        .uri("/next/auth")
        .header("cf-ray", "8a00000000000000-TEST")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("token=deadbeef"))
        .expect("the request builds");

    let (status, _, page) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        page.contains("Cloudflare Access did not identify this request"),
        "{page}"
    );
}

#[tokio::test]
async fn no_console_answer_is_cacheable() {
    // A cached page outlives a deploy and a cached `/session` outlives a login: both were possible
    // while these routes carried no `Cache-Control`.
    let fixture = Fixture::new("no-store");

    // The page is HTML, so it is read off the response rather than through the JSON helper.
    let response = fixture
        .app()
        .oneshot(get("/next", None))
        .await
        .expect("the page is served");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store"),
        "the page is the build, and a cached copy outlives a deploy"
    );

    for (uri, request) in [
        ("/session", get("/session", None)),
        ("/health", get("/health", None)),
    ] {
        let (status, headers, _) = send_with_headers(fixture.app(), request).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store"),
            "{uri} is an answer for one request"
        );
    }
}

#[tokio::test]
async fn the_fleet_namespace_is_the_operators_and_the_device_namespace_is_not() {
    let fixture = Fixture::new("fleet-split");
    let key = signing_key(0x11);
    let (id, _token) = fixture.register(&key).await;
    let session = fixture.authenticate().await;

    let (status, list) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list.as_array()
            .expect("a list")
            .iter()
            .any(|device| device["id"] == json!(id)),
        "the operator's list is under /fleet: {list}"
    );

    // `/devices` is the device namespace: a registration POST and a channel. An operator GET there
    // is not a route — and, more to the point, it is inside Cloudflare Access's bypass prefix, so
    // the page must never call it with an operator identity.
    let (status, _) = send(fixture.app(), get("/devices", Some(&session))).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let (status, renamed) = send(
        fixture.app(),
        post_json_bearer(
            &format!("/fleet/{id}/name"),
            &session,
            json!({ "name": "the renamed canary" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "rename: {renamed}");
    assert_eq!(renamed["name"], json!("the renamed canary"));

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/fleet/{id}"))
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .expect("the request builds");
    let (status, _) = send(fixture.app(), request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert!(
        list.as_array()
            .expect("a list")
            .iter()
            .all(|device| device["id"] != json!(id)),
        "the removed device is gone from the fleet list"
    );
}

#[tokio::test]
async fn a_saved_connection_is_per_device_and_carries_no_credential() {
    let fixture = Fixture::new("saved-connections");
    let key = signing_key(0x21);
    let (id, _token) = fixture.register(&key).await;
    let session = fixture.authenticate().await;

    let (status, saved) = send(
        fixture.app(),
        post_json_bearer(
            &format!("/fleet/{id}/connections"),
            &session,
            json!({
                "name": "the db host",
                "kind": "ssh",
                "host": "db.internal",
                "port": 22,
                "user": "operator"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "save: {saved}");
    let connection_id = saved["id"].as_str().expect("an id").to_owned();
    assert_eq!(saved["host"], json!("db.internal"));

    // A preset that cannot become a session is refused now, not when the operator is waiting for a
    // prompt: an ssh preset with no host is the shape the device would reject later.
    let (status, body) = send(
        fixture.app(),
        post_json_bearer(
            &format!("/fleet/{id}/connections"),
            &session,
            json!({ "name": "no host", "kind": "ssh", "port": 22, "user": "operator" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A device that does not exist has no list to read or write.
    let (status, _) = send(
        fixture.app(),
        get("/fleet/not-a-device/connections", Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, list) = send(
        fixture.app(),
        get(&format!("/fleet/{id}/connections"), Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("a list").len(), 1, "{list}");

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/fleet/{id}/connections/{connection_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .expect("the request builds");
    let (status, _) = send(fixture.app(), request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = send(
        fixture.app(),
        get(&format!("/fleet/{id}/connections"), Some(&session)),
    )
    .await;
    assert!(list.as_array().expect("a list").is_empty(), "{list}");
}

/// A real mesh broker on an ephemeral loopback port, for the readiness route.
struct TestBroker {
    url: String,
}

async fn start_broker() -> TestBroker {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a broker listener");
    let address = listener.local_addr().expect("the broker's address");
    tokio::spawn(async move {
        axum::serve(listener, summrise_mesh::Broker::new().router())
            .await
            .expect("the test broker serves");
    });
    TestBroker {
        url: format!("http://{address}"),
    }
}

#[tokio::test]
async fn the_mesh_view_is_absent_without_a_broker() {
    let fixture = Fixture::new("mesh-absent");
    let session = fixture.authenticate().await;
    let (status, body) = send(fixture.app(), get("/mesh", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["configured"], json!(false));
    assert_eq!(body["broker"], Value::Null);
}

#[tokio::test]
async fn the_readiness_route_asks_the_broker_about_a_registered_device() {
    let broker = start_broker().await;
    let fixture = Fixture::with_mesh_broker("mesh-ready", &broker.url);
    let key = SigningKey::from_bytes(&[0x33; 32]);
    let (device_id, _token) = fixture.register(&key).await;

    // The device id IS the mesh peer id: register the same key at the broker.
    let peer = summrise_mesh::Identity::from_private_bytes(key.to_bytes());
    summrise_mesh::Rendezvous::new(peer, Vec::new(), 30_000)
        .register(&broker.url)
        .await
        .expect("the device registers at the broker");

    let session = fixture.authenticate().await;
    let (status, body) = send(
        fixture.app(),
        get(&format!("/fleet/{device_id}/mesh"), Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["configured"], json!(true));
    assert_eq!(body["reachable"], json!(true), "{body}");
}

#[tokio::test]
async fn the_readiness_route_refuses_an_unregistered_device() {
    let broker = start_broker().await;
    let fixture = Fixture::with_mesh_broker("mesh-unregistered", &broker.url);
    let session = fixture.authenticate().await;
    let (status, _) = send(
        fixture.app(),
        get(&format!("/fleet/{}/mesh", "ab".repeat(32)), Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // And without an operator session the route is not a public probe.
    let (status, _) = send(
        fixture.app(),
        get(&format!("/fleet/{}/mesh", "ab".repeat(32)), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---- the standard MCP endpoint -------------------------------------------------------------------

/// Lowercase hex, as the grant header carries it.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn standard_request(
    device_id: &str,
    authorization: Option<&str>,
    content_type: Option<&str>,
    accept: Option<&str>,
    body: Body,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/mcp/{device_id}"));
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = accept {
        builder = builder.header(header::ACCEPT, value);
    }
    builder.body(body).expect("the request builds")
}

/// One standard MCP request with the grant in the header and the JSON-RPC body the endpoint reads.
fn post_standard(device_id: &str, grant: &Value, body: Value) -> Request<Body> {
    let bytes = serde_json::to_vec(grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));
    standard_request(
        device_id,
        Some(&authorization),
        Some("application/json"),
        Some("application/json, text/event-stream"),
        Body::from(body.to_string()),
    )
}

/// One standard MCP request with a permanent token in the header.
fn post_token(device_id: &str, token: &str, body: Value) -> Request<Body> {
    standard_request(
        device_id,
        Some(&format!("Bearer {token}")),
        Some("application/json"),
        Some("application/json, text/event-stream"),
        Body::from(body.to_string()),
    )
}

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
    let (status, created) = fixture
        .create_aggregate_token(session, "aggregate", ids, tools)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (
        created["id"].as_str().expect("a token id").to_owned(),
        created["token"]
            .as_str()
            .expect("the one-time secret")
            .to_owned(),
    )
}

fn ping_request() -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })
}

async fn answer_error(socket: &mut DeviceSocket, frame: &Value, error: Value) {
    let reply = json!({ "id": frame["id"].clone(), "error": error });
    socket
        .send(WsMessage::Text(reply.to_string()))
        .await
        .expect("the device answers");
}

#[tokio::test]
async fn a_standard_client_initializes_with_a_grant() {
    let fixture = Fixture::new("standard-initialize");
    let (device_id, _token) = fixture.register(&signing_key(0xd1)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "0" },
                },
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["protocolVersion"], json!("2025-06-18"));
    assert_eq!(
        body["result"]["serverInfo"]["name"],
        json!("summrise-console")
    );
    assert_eq!(
        body["result"]["serverInfo"]["version"],
        json!(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        body["result"]["capabilities"]["tools"]["listChanged"],
        json!(false)
    );
    assert!(
        body["result"]["instructions"]
            .as_str()
            .expect("instructions")
            .contains(device_id.as_str()),
        "{body}"
    );
}

#[tokio::test]
async fn an_initialize_is_not_an_audited_action() {
    let fixture = Fixture::new("standard-initialize-audit");
    let (device_id, _token) = fixture.register(&signing_key(0xd4)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let before = audit_lines(&fixture).len();

    let (status, _) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        audit_lines(&fixture).len(),
        before,
        "initialize is a local transport fact, not an operator action"
    );
}

#[tokio::test]
async fn an_initialized_notification_is_accepted_without_a_device_frame() {
    let fixture = Fixture::new("standard-initialized");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd2)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body, Value::Null);
    let frame = tokio::time::timeout(Duration::from_millis(100), device.next()).await;
    assert!(frame.is_err(), "a notification must not reach the device");
}

#[tokio::test]
async fn a_missing_or_malformed_grant_is_unauthorized_and_denied() {
    let fixture = Fixture::new("standard-unauthorized");
    let (device_id, _token) = fixture.register(&signing_key(0xd3)).await;

    for authorization in [None, Some("Bearer zz"), Some("Basic abcd")] {
        let (status, body) = send(
            fixture.app(),
            standard_request(
                &device_id,
                authorization,
                Some("application/json"),
                Some("application/json"),
                Body::from(ping_request().to_string()),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{authorization:?}: {body}"
        );
        assert_eq!(body["error"], json!("unauthorized"));
    }

    let denials = denial_lines(&fixture);
    assert_eq!(denials.len(), 3, "{denials:?}");
    assert!(denials.iter().all(|line| line["kind"] == json!("denied")));
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().all(|line| line["kind"] != json!("denied")),
        "a missing credential is a denial, not an audited action: {audit:?}"
    );
}

#[tokio::test]
async fn an_unknown_device_without_a_grant_is_unauthorized_not_not_found() {
    let fixture = Fixture::new("standard-unknown-unauthenticated");
    let unknown = "ab".repeat(32);

    let (status, body) = send(
        fixture.app(),
        standard_request(
            &unknown,
            None,
            Some("application/json"),
            Some("application/json"),
            Body::from(ping_request().to_string()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(
        body["error"],
        json!("unauthorized"),
        "the route must not confirm which ids are registered"
    );
}

#[tokio::test]
async fn a_valid_grant_for_a_removed_device_is_not_found_and_audited() {
    let fixture = Fixture::new("standard-removed-device");
    let (device_id, _token) = fixture.register(&signing_key(0xd5)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, _) = send(
        fixture.app(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/fleet/{device_id}"))
            .header(header::AUTHORIZATION, format!("Bearer {session}"))
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(
        fixture.app(),
        post_standard(&device_id, &grant, ping_request()),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], json!("unknown_device"));
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(
            |line| line["kind"] == json!("denied") && line["detail"] == json!("unknown_device")
        ),
        "the caller held a valid grant, so the missing device is an audited action: {audit:?}"
    );
}

#[tokio::test]
async fn a_bad_expired_or_mismatched_grant_is_forbidden_and_denied() {
    let fixture = Fixture::new("standard-grant-refusals");
    let (device_a, _token_a) = fixture.register(&signing_key(0xd6)).await;
    let (device_b, _token_b) = fixture.register(&signing_key(0xd7)).await;
    let session = fixture.authenticate().await;
    let grant_a = fixture.create_grant(&session, &device_a, 60).await;
    let grant_b = fixture.create_grant(&session, &device_b, 60).await;

    let mut tampered = grant_a.clone();
    tampered["signature"] = json!("00".repeat(64));
    let (status, body) = send(
        fixture.app(),
        post_standard(&device_a, &tampered, ping_request()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], json!("invalid_grant_signature"));

    let (status, body) = send(
        fixture.app(),
        post_standard(&device_a, &grant_b, ping_request()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], json!("grant_device_mismatch"));

    fixture.advance_ms(61_000);
    let (status, body) = send(
        fixture.app(),
        post_standard(&device_a, &grant_a, ping_request()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], json!("grant_expired"));

    let denials = denial_lines(&fixture);
    for code in [
        "invalid_grant_signature",
        "grant_device_mismatch",
        "grant_expired",
    ] {
        assert!(
            denials.iter().any(|line| line["detail"] == json!(code)),
            "{code} is missing from {denials:?}"
        );
    }
    let audit = audit_lines(&fixture);
    for code in [
        "invalid_grant_signature",
        "grant_device_mismatch",
        "grant_expired",
    ] {
        assert!(
            audit.iter().all(|line| line["detail"] != json!(code)),
            "{code} must not be in the audit: {audit:?}"
        );
    }
}

#[tokio::test]
async fn tools_list_is_filtered_to_the_grants_tools() {
    let fixture = Fixture::new("standard-tools-list");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd8)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "tools": [
            { "name": "files.read", "description": "read" },
            { "name": "files.write", "description": "write" },
        ]},
    });

    let http = send(
        fixture.app(),
        post_standard(&device_id, &grant, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, response.clone()).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["result"]["tools"],
        json!([{ "name": "files.read", "description": "read" }])
    );
}

#[tokio::test]
async fn a_tool_call_round_trips_through_the_channel() {
    let fixture = Fixture::new("standard-tool-call");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd9)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
    });
    let response = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "result": { "content": [{ "type": "text", "text": "hello" }] },
    });

    let http = send(
        fixture.app(),
        post_standard(&device_id, &grant, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, response.clone()).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, response);
}

#[tokio::test]
async fn a_tool_outside_the_grant_is_refused_and_the_device_sees_nothing() {
    let fixture = Fixture::new("standard-tool-denied");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xda)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "files.write", "arguments": { "path": "denied" } },
    });

    let (status, body) = send(fixture.app(), post_standard(&device_id, &grant, request)).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], json!(-32001));
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("not permitted"),
        "{body}"
    );
    let frame = tokio::time::timeout(Duration::from_millis(100), device.next()).await;
    assert!(frame.is_err(), "a refused call must not reach the device");

    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["tool"] == json!("files.write")
            && line["detail"] == json!("tool_not_permitted")),
        "{audit:?}"
    );
}

#[tokio::test]
async fn a_device_error_envelope_is_a_json_rpc_answer_at_http_200() {
    let fixture = Fixture::new("standard-device-error");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdb)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
    });
    let error = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "error": { "code": -32000, "message": "the device said no" },
    });

    let http = send(
        fixture.app(),
        post_standard(&device_id, &grant, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer_error(&mut device, &frame, error.clone()).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body, error,
        "a JSON-RPC error is an answer, not a transport failure"
    );
}

#[tokio::test]
async fn a_standard_call_is_one_audit_line_without_its_payload() {
    let fixture = Fixture::new("standard-audit");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdc)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let marker = "marker-standard-audit-7f3a";
    let request = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": marker } },
    });
    let response = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "result": { "content": [{ "type": "text", "text": "ok" }] },
    });

    let http = send(
        fixture.app(),
        post_standard(&device_id, &grant, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        answer(&mut device, &frame, response.clone()).await;
    };
    let ((status, _body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK);

    let audit = audit_lines(&fixture);
    let rows: Vec<_> = audit
        .iter()
        .filter(|line| line["kind"] == json!("mcp") && line["tool"] == json!("files.read"))
        .collect();
    assert_eq!(rows.len(), 1, "{audit:?}");
    let text = fs::read_to_string(fixture.config.data.join("audit.jsonl")).expect("the audit file");
    assert!(
        !text.contains(marker),
        "the audit must not carry the argument"
    );
}

#[tokio::test]
async fn a_wrong_content_type_is_refused() {
    let fixture = Fixture::new("standard-content-type");
    let (device_id, _token) = fixture.register(&signing_key(0xdd)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let bytes = serde_json::to_vec(&grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));

    let (status, body) = send(
        fixture.app(),
        standard_request(
            &device_id,
            Some(&authorization),
            Some("text/plain"),
            Some("application/json"),
            Body::from(ping_request().to_string()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");
    assert_eq!(body["error"], json!("unsupported_media_type"));

    // The media type is checked before the credential: without a grant the answer is still 415,
    // and no denial is written for a request that never presented one.
    let (status, _) = send(
        fixture.app(),
        standard_request(
            &device_id,
            None,
            Some("text/plain"),
            Some("application/json"),
            Body::from(ping_request().to_string()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(denial_lines(&fixture).is_empty());
}

#[tokio::test]
async fn a_get_on_the_standard_endpoint_is_method_not_allowed() {
    let fixture = Fixture::new("standard-get");
    let (device_id, _token) = fixture.register(&signing_key(0xe5)).await;

    let (status, _) = send(
        fixture.app(),
        Request::builder()
            .method("GET")
            .uri(format!("/mcp/{device_id}"))
            .header(header::ACCEPT, "text/event-stream")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;

    // The official client opens a standalone SSE stream after initialize and tolerates this; the
    // endpoint has nothing to say on such a stream.
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn an_accept_header_without_json_is_not_acceptable() {
    let fixture = Fixture::new("standard-accept");
    let (device_id, _token) = fixture.register(&signing_key(0xde)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let bytes = serde_json::to_vec(&grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));

    let (status, body) = send(
        fixture.app(),
        standard_request(
            &device_id,
            Some(&authorization),
            Some("application/json"),
            Some("text/event-stream"),
            Body::from(ping_request().to_string()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_ACCEPTABLE, "{body}");
    assert_eq!(body["error"], json!("not_acceptable"));
}

#[tokio::test]
async fn a_malformed_body_is_a_parse_error() {
    let fixture = Fixture::new("standard-parse-error");
    let (device_id, _token) = fixture.register(&signing_key(0xdf)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let bytes = serde_json::to_vec(&grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));

    let (status, body) = send(
        fixture.app(),
        standard_request(
            &device_id,
            Some(&authorization),
            Some("application/json"),
            Some("application/json"),
            Body::from("{"),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], json!(-32700));
    assert_eq!(body["id"], Value::Null);
}

#[tokio::test]
async fn a_batch_is_an_invalid_request() {
    let fixture = Fixture::new("standard-batch");
    let (device_id, _token) = fixture.register(&signing_key(0xe0)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let bytes = serde_json::to_vec(&grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));

    let (status, body) = send(
        fixture.app(),
        standard_request(
            &device_id,
            Some(&authorization),
            Some("application/json"),
            Some("application/json"),
            Body::from("[]"),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], json!(-32600));
}

#[tokio::test]
async fn an_unknown_method_is_method_not_found() {
    let fixture = Fixture::new("standard-unknown-method");
    let (device_id, _token) = fixture.register(&signing_key(0xe1)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/list" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32601));
    assert_eq!(body["id"], json!(5));
}

#[tokio::test]
async fn a_body_over_the_cap_is_refused() {
    let fixture = Fixture::new("standard-body-cap");
    let (device_id, _token) = fixture.register(&signing_key(0xe2)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let bytes = serde_json::to_vec(&grant).expect("the grant serializes");
    let authorization = format!("Bearer {}", hex_encode(&bytes));
    let body = Body::from(vec![b' '; 9 * 1024 * 1024]);

    let (status, answer) = send(
        fixture.app(),
        standard_request(
            &device_id,
            Some(&authorization),
            Some("application/json"),
            Some("application/json"),
            body,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{answer}");
    assert_eq!(answer["error"], json!("payload_too_large"));
}

#[tokio::test]
async fn an_offline_device_is_a_retryable_503() {
    let fixture = Fixture::new("standard-offline");
    let (device_id, _token) = fixture.register(&signing_key(0xe3)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], json!("device reconnecting"));
    assert_eq!(body["retry_after_ms"], json!(1_000));
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["detail"] == json!("device reconnecting")),
        "{audit:?}"
    );
}

#[tokio::test]
async fn a_tool_call_without_a_name_is_a_json_rpc_error_at_http_200() {
    let fixture = Fixture::new("standard-invalid-tool-call");
    let (device_id, _token) = fixture.register(&signing_key(0xe4)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let (status, body) = send(
        fixture.app(),
        post_standard(
            &device_id,
            &grant,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {} }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32602));
    let audit = audit_lines(&fixture);
    assert!(
        audit
            .iter()
            .any(|line| line["kind"] == json!("denied")
                && line["detail"] == json!("invalid_tool_call")),
        "{audit:?}"
    );
}

// ---- permanent API tokens -------------------------------------------------------------------------

/// The create response is the only plaintext; the list and the audit never carry it, and the file
/// keeps only its hash.
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

/// Removing a device takes the tokens that could reach it — including an aggregate token that
/// merely LISTS it, which its per-device page never shows, so nobody could revoke it by hand.
#[tokio::test]
async fn removing_a_device_revokes_the_aggregate_tokens_that_list_it() {
    let fixture = Fixture::new("aggregate-token-device-removal");
    let session = fixture.authenticate().await;
    let (a, _) = fixture.register(&signing_key(0x73)).await;
    let (b, _) = fixture.register(&signing_key(0x74)).await;
    let (status, created) = fixture
        .create_aggregate_token(&session, "all", &[a.clone(), b.clone()], &["files.read"])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/fleet/{a}"))
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .expect("the request builds");
    let (status, _) = send(fixture.app(), request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert!(
        list.as_array().expect("a list").is_empty(),
        "the aggregate token went with the device it could reach: {list}"
    );
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

#[tokio::test]
async fn an_aggregate_token_validates_every_device_in_its_scope() {
    let fixture = Fixture::new("aggregate-token-validate");
    let session = fixture.authenticate().await;
    let (device, _) = fixture.register(&signing_key(0x73)).await;
    let (status, _) = send(
        fixture.app(),
        post_json_bearer(
            "/tokens",
            &session,
            json!({ "name": "x", "device_ids": [], "tools": ["files.read"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "an empty scope is refused");

    let (status, _) = send(
        fixture.app(),
        post_json_bearer(
            "/tokens",
            &session,
            json!({ "name": "x", "device_ids": [device.clone(), device], "tools": ["files.read"] }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a duplicate device is refused"
    );

    let (status, _) = send(
        fixture.app(),
        post_json_bearer(
            "/tokens",
            &session,
            json!({ "name": "x", "device_ids": ["ff".repeat(32)], "tools": ["files.read"] }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an unknown device is refused"
    );
}

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
        aggregate_request(
            &token,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"]["method"], json!("tools/list"));
        answer(
            &mut device,
            &frame,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [{
                "name": "files.read",
                "inputSchema": {"type": "object", "properties": {}, "required": []}
            }]}}),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    let tool = &body["result"]["tools"][0];
    assert_eq!(tool["name"], "files.read");
    assert!(tool["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "device"));
}

#[tokio::test]
async fn an_aggregate_call_reaches_the_named_device_and_audits_it() {
    let fixture = Fixture::new("aggregate-call");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xa8)).await;
    let (b, _tb, mut db) = connected_device(&fixture, &server, &signing_key(0xa9)).await;
    let session = fixture.authenticate().await;
    let (token_id, token) =
        aggregate_token_for(&fixture, &session, &[a.clone(), b.clone()], &["files.read"]).await;

    let http = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": b.clone()}}
            }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut db).await;
        assert!(frame["mcp"]["params"]["arguments"].get("device").is_none());
        assert_eq!(frame["mcp"]["params"]["arguments"]["path"], json!("C:\\x"));
        answer(
            &mut db,
            &frame,
            json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [{"type": "text", "text": "ok"}]}}),
        )
        .await;
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
async fn an_aggregate_call_resolves_a_unique_name_and_strips_it() {
    let fixture = Fixture::new("aggregate-name");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xae)).await;
    let (b, _tb, mut db) = connected_device(&fixture, &server, &signing_key(0xaf)).await;
    let session = fixture.authenticate().await;
    let (status, renamed) = send(
        fixture.app(),
        post_json_bearer(
            &format!("/fleet/{b}/name"),
            &session,
            json!({ "name": "bench" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "rename: {renamed}");
    let (token_id, token) =
        aggregate_token_for(&fixture, &session, &[a, b.clone()], &["files.read"]).await;

    let http = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": "bench"}}
            }),
        ),
    );
    let device_side = async {
        let frame = next_json(&mut db).await;
        assert!(frame["mcp"]["params"]["arguments"].get("device").is_none());
        assert_eq!(frame["mcp"]["params"]["arguments"]["path"], json!("C:\\x"));
        answer(
            &mut db,
            &frame,
            json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [{"type": "text", "text": "ok"}]}}),
        )
        .await;
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
async fn an_out_of_scope_name_is_audited_as_requested() {
    let fixture = Fixture::new("aggregate-name-scope");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xb0)).await;
    let (c, _tc, _dc) = connected_device(&fixture, &server, &signing_key(0xb1)).await;
    let session = fixture.authenticate().await;
    let (status, renamed) = send(
        fixture.app(),
        post_json_bearer(
            &format!("/fleet/{c}/name"),
            &session,
            json!({ "name": "attic" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "rename: {renamed}");
    let (token_id, token) = aggregate_token_for(&fixture, &session, &[a], &["files.read"]).await;

    let (status, body) = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": "attic"}}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], json!(-32001));
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["device_id"] == json!("attic")
            && line["detail"] == json!("device_not_permitted")
            && line["token_id"] == json!(token_id)),
        "{audit:?}"
    );
}

#[tokio::test]
async fn an_ambiguous_name_is_refused_with_the_scope_error_and_audited_as_requested() {
    let fixture = Fixture::new("aggregate-name-ambiguous");
    let server = TestServer::start(&fixture.console).await;
    let (a, _ta, _da) = connected_device(&fixture, &server, &signing_key(0xb2)).await;
    let (b, _tb, _db) = connected_device(&fixture, &server, &signing_key(0xb3)).await;
    let session = fixture.authenticate().await;
    for id in [&a, &b] {
        let (status, renamed) = send(
            fixture.app(),
            post_json_bearer(
                &format!("/fleet/{id}/name"),
                &session,
                json!({ "name": "bench" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "rename: {renamed}");
    }
    let (token_id, token) = aggregate_token_for(&fixture, &session, &[a, b], &["files.read"]).await;

    let (status, body) = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": "bench"}}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], json!(-32001));
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["device_id"] == json!("bench")
            && line["detail"] == json!("device_not_permitted")
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
    let (token_id, token) = aggregate_token_for(&fixture, &session, &[a, b], &["files.read"]).await;

    let (status, body) = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "files.read", "arguments": {"path": "C:\\x", "device": c.clone()}}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["device_id"] == json!(c)
            && line["detail"] == json!("device_not_permitted")
            && line["token_id"] == json!(token_id)),
        "{audit:?}"
    );
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
    let (_id, token) = aggregate_token_for(
        &fixture,
        &session,
        std::slice::from_ref(&device_id),
        &["files.read"],
    )
    .await;

    let (status, _) = send(
        fixture.app(),
        post_token(
            &device_id,
            &token,
            json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}),
        ),
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
        aggregate_request(
            &token,
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn an_aggregate_token_initializes_without_a_device() {
    let fixture = Fixture::new("aggregate-initialize");
    let session = fixture.authenticate().await;
    let (device_id, _) = fixture.register(&signing_key(0xb1)).await;
    let (_id, token) = aggregate_token_for(
        &fixture,
        &session,
        std::slice::from_ref(&device_id),
        &["files.read"],
    )
    .await;

    let (status, body) = send(
        fixture.app(),
        aggregate_request(
            &token,
            json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["result"]["serverInfo"]["name"],
        json!("summrise-console")
    );
    assert!(body["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains(&device_id));
}

#[tokio::test]
async fn a_created_token_is_shown_once_and_only_its_hash_is_stored() {
    let fixture = Fixture::new("token-create");
    let (device_id, _token) = fixture.register(&signing_key(0xb0)).await;
    let session = fixture.authenticate().await;
    let now = fixture.now_ms();

    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "  laptop-dsh  ",
            &["files.read", "memory.search"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"]
        .as_str()
        .expect("the create response carries the token")
        .to_owned();
    assert!(plaintext.starts_with("srm_"), "{plaintext}");
    assert_eq!(plaintext.len(), 68, "srm_ plus 64 hex: {plaintext}");
    assert!(
        plaintext[4..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "the body is 64 lowercase hex: {plaintext}"
    );
    assert_eq!(created["name"], json!("laptop-dsh"), "the name is trimmed");
    assert_eq!(created["device_id"], json!(device_id));
    assert_eq!(created["tools"], json!(["files.read", "memory.search"]));
    assert_eq!(created["allow_destructive"], json!(false));
    assert_eq!(created["created_at_ms"], json!(now));
    let id = created["id"].as_str().expect("an id").to_owned();

    let (status, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let list = list.as_array().expect("a list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], json!(id));
    assert_eq!(list[0]["name"], json!("laptop-dsh"));
    assert_eq!(list[0]["device_id"], json!(device_id));
    assert_eq!(list[0]["tools"], json!(["files.read", "memory.search"]));
    assert!(
        list[0].get("token").is_none(),
        "the list never repeats the secret: {}",
        list[0]
    );
    assert!(
        list[0].get("hash").is_none(),
        "the list carries no hash: {}",
        list[0]
    );

    let on_disk = fs::read_to_string(fixture.config.data.join("tokens.json"))
        .expect("the tokens file is readable");
    let hash = hex_encode(&Sha256::digest(plaintext.as_bytes()));
    assert!(
        on_disk.contains(&hash),
        "the file keeps the hash: {on_disk}"
    );
    assert!(
        !on_disk.contains(&plaintext),
        "the file must never hold the token: {on_disk}"
    );
    let audit_on_disk =
        fs::read_to_string(fixture.config.data.join("audit.jsonl")).expect("the audit is readable");
    assert!(
        !audit_on_disk.contains(&plaintext),
        "no audit line carries the token: {audit_on_disk}"
    );
    assert!(
        audit_lines(&fixture)
            .iter()
            .any(|line| line["kind"] == json!("token") && line["device_id"] == json!(device_id)),
        "the mint is an audited operator action"
    );

    // Newest first, and it survives a restart.
    fixture.advance_ms(1_000);
    let (status, second) = fixture
        .create_token(&session, &device_id, "desktop", &["files.read"], true)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list[0]["id"], second["id"], "newest first: {list}");
    assert_eq!(list[1]["id"], json!(id));

    let restarted = fixture.restart();
    let session = authenticate_on(&restarted).await;
    let (status, list) = send(restarted.router(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list.as_array().expect("a list").len(),
        2,
        "a restart still lists the tokens: {list}"
    );
}

/// Rotation is one replacement: the old secret is refused at once, the new one reaches the same
/// device with the same scope, and the answer is the only place the new secret appears.
#[tokio::test]
async fn a_rotated_token_kills_the_old_one_and_keeps_the_scope() {
    let fixture = Fixture::new("token-rotate");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb8)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read", "memory.search"],
            true,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let old_plaintext = created["token"]
        .as_str()
        .expect("the create response carries the token")
        .to_owned();
    let old_id = created["id"].as_str().expect("an id").to_owned();

    fixture.advance_ms(5_000);
    let rotated_at = fixture.now_ms();
    let (status, rotated) = fixture.rotate_token(&session, &old_id).await;

    assert_eq!(status, StatusCode::CREATED, "{rotated}");
    let plaintext = rotated["token"]
        .as_str()
        .expect("the rotate response carries the new token")
        .to_owned();
    let id = rotated["id"].as_str().expect("the new id").to_owned();
    assert!(plaintext.starts_with("srm_"), "{plaintext}");
    assert_eq!(plaintext.len(), 68, "srm_ plus 64 hex: {plaintext}");
    assert_ne!(plaintext, old_plaintext, "a rotation mints a new secret");
    assert_ne!(id, old_id, "a rotation mints a new id");
    assert_eq!(rotated["name"], created["name"], "the name is kept");
    assert_eq!(
        rotated["device_id"], created["device_id"],
        "the device is kept"
    );
    assert_eq!(rotated["tools"], created["tools"], "the tools are kept");
    assert_eq!(
        rotated["allow_destructive"], created["allow_destructive"],
        "the destructive consent is kept"
    );
    assert_eq!(
        rotated["created_at_ms"],
        json!(rotated_at),
        "the new record is created now"
    );
    assert!(
        rotated.get("last_used_ms").is_none(),
        "the new record has never been used: {rotated}"
    );
    let mut rotated_fields: Vec<&String> = rotated
        .as_object()
        .expect("the rotate answer is an object")
        .keys()
        .collect();
    let mut created_fields: Vec<&String> = created
        .as_object()
        .expect("the create answer is an object")
        .keys()
        .collect();
    rotated_fields.sort();
    created_fields.sort();
    assert_eq!(
        rotated_fields, created_fields,
        "rotation answers in the create shape"
    );

    let (status, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let list = list.as_array().expect("a list");
    assert_eq!(
        list.len(),
        1,
        "rotation replaced the record instead of adding one: {list:?}"
    );
    assert_eq!(list[0]["id"], json!(id));
    assert!(
        list[0].get("token").is_none() && list[0].get("hash").is_none(),
        "the list stays metadata only: {}",
        list[0]
    );

    let (status, body) = send(
        fixture.app(),
        post_token(&device_id, &old_plaintext, ping_request()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the old token is dead at once: {body}"
    );
    assert_eq!(body, json!({ "error": "unauthorized" }));

    let request = json!({ "jsonrpc": "2.0", "id": 31, "method": "tools/list" });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        let token = &frame["token"];
        assert!(
            token["token_id"].as_str().is_some(),
            "the frame carries a token scope"
        );
        assert_eq!(
            token["scopes"],
            json!(["files.read", "memory.search"]),
            "the new token keeps the same tools"
        );
        assert_eq!(
            token["allow_destructive"],
            json!(true),
            "the new token keeps the destructive consent"
        );
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 31, "result": { "tools": [] } }),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "the new token works: {body}");

    let on_disk = fs::read_to_string(fixture.config.data.join("tokens.json"))
        .expect("the tokens file is readable");
    let old_hash = hex_encode(&Sha256::digest(old_plaintext.as_bytes()));
    let new_hash = hex_encode(&Sha256::digest(plaintext.as_bytes()));
    assert!(
        !on_disk.contains(&old_hash),
        "the old hash is gone from the file: {on_disk}"
    );
    assert!(
        on_disk.contains(&new_hash),
        "the new hash is on disk: {on_disk}"
    );
    assert!(
        !on_disk.contains(&plaintext) && !on_disk.contains(&old_plaintext),
        "the file never holds a secret: {on_disk}"
    );
    let records: Vec<Value> = serde_json::from_str(&on_disk).expect("the file is a JSON list");
    assert_eq!(records.len(), 1, "one record after rotation: {records:?}");
    assert_eq!(records[0]["id"], json!(id));

    let audit = audit_text(&fixture);
    assert!(
        !audit.contains(&plaintext) && !audit.contains(&old_plaintext),
        "no audit line carries either secret: {audit}"
    );
}

/// A rotation reads as a creation and a revocation in the audit: the replacement's consent and the
/// old record's withdrawal, both naming the device.
#[tokio::test]
async fn a_rotation_is_audited_as_a_creation_and_a_revocation() {
    let fixture = Fixture::new("token-rotate-audit");
    let (device_id, _token) = fixture.register(&signing_key(0xb9)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "laptop-dsh", &["files.write"], true)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id").to_owned();

    fixture.advance_ms(1_000);
    let (status, rotated) = fixture.rotate_token(&session, &id).await;
    assert_eq!(status, StatusCode::CREATED, "{rotated}");

    let lines = audit_lines(&fixture);
    let token_lines: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("token"))
        .collect();
    assert_eq!(
        token_lines.len(),
        2,
        "the mint and the rotation are both creations: {lines:?}"
    );
    let replacement = token_lines[1];
    assert_eq!(replacement["device_id"], json!(device_id));
    assert_eq!(
        replacement["detail"],
        json!("allow_destructive=true"),
        "the replacement's consent is recorded: {replacement}"
    );
    assert_eq!(replacement["status"], json!(201));

    let revocations: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("token_revoke"))
        .collect();
    assert_eq!(
        revocations.len(),
        1,
        "the old record is revoked in the same story: {lines:?}"
    );
    assert_eq!(revocations[0]["device_id"], json!(device_id));
    assert_eq!(revocations[0]["detail"], json!("rotated"));
    assert_eq!(revocations[0]["status"], json!(201));
    assert!(
        !audit_text(&fixture).contains("srm_"),
        "the audit never carries the secret"
    );
}

/// Rotating an id the console does not hold is the same typed 404 as revoking one.
#[tokio::test]
async fn rotating_an_unknown_token_is_404() {
    let fixture = Fixture::new("token-rotate-404");
    let session = fixture.authenticate().await;

    let (status, body) = fixture.rotate_token(&session, &"ee".repeat(32)).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body, json!({ "error": "no_such_token" }));
}

/// A token request reaches the device as a grant with exactly the token's tools and consent, a
/// sixty-second life and the console's signature — the device verifies it as any grant.
#[tokio::test]
async fn a_token_mints_a_sixty_second_grant_with_the_tokens_tools_and_consent() {
    let fixture = Fixture::new("token-mint");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb1)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read", "memory.search"],
            true,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().expect("the token").to_owned();

    let request = json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list" });
    let http = send(
        fixture.app(),
        post_token(&device_id, &token, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        let token = &frame["token"];
        assert!(
            token["token_id"].as_str().is_some(),
            "the frame carries a token scope"
        );
        assert_eq!(token["device_id"], json!(device_id));
        assert_eq!(
            token["scopes"],
            json!(["files.read", "memory.search"]),
            "the scope names exactly the token's tools"
        );
        assert_eq!(
            token["allow_destructive"],
            json!(true),
            "the consent is the token's, not the caller's"
        );
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 11, "result": { "tools": [
                { "name": "files.read", "description": "read" },
                { "name": "files.write", "description": "write" },
            ]}}),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["result"]["tools"],
        json!([{ "name": "files.read", "description": "read" }]),
        "the device's list is filtered to the token's tools: {body}"
    );
}

/// The token's tools are the grant's tools: a call outside them never reaches the device.
#[tokio::test]
async fn a_tool_outside_the_token_never_reaches_the_device() {
    let fixture = Fixture::new("token-tool-denied");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb2)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "reader", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().expect("the token").to_owned();

    let request = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": { "name": "files.write", "arguments": { "path": "denied.txt" } },
    });
    let (status, body) = send(fixture.app(), post_token(&device_id, &token, request)).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], json!(-32001));
    let frame = tokio::time::timeout(Duration::from_millis(100), device.next()).await;
    assert!(frame.is_err(), "a refused call must not reach the device");
    assert!(
        audit_lines(&fixture)
            .iter()
            .any(|line| line["kind"] == json!("denied")
                && line["tool"] == json!("files.write")
                && line["detail"] == json!("tool_not_permitted")),
        "the refusal is audited"
    );
}

/// A token-authenticated call is attributed to the record — the id, never the secret — and moves the
/// token's last use, which the operator's list shows.
#[tokio::test]
async fn a_token_call_moves_last_used_and_names_the_token_in_the_audit() {
    let fixture = Fixture::new("token-last-used");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb6)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read", "memory.search"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let id = created["id"].as_str().expect("an id").to_owned();

    fixture.advance_ms(5_000);
    let listed_at = fixture.now_ms();
    let list_request = json!({ "jsonrpc": "2.0", "id": 21, "method": "tools/list" });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, list_request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], list_request);
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 21, "result": { "tools": [] } }),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list[0]["last_used_ms"],
        json!(listed_at),
        "the list shows when the token was used: {list}"
    );

    // A tools/call carries the tool as well as the token.
    fixture.advance_ms(1_000);
    let call_at = fixture.now_ms();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, call.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], call);
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 22, "result": { "content": [] } }),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "{body}");

    let lines = audit_lines(&fixture);
    let mcp: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == json!("mcp"))
        .collect();
    assert_eq!(mcp.len(), 2, "one line per call: {lines:?}");
    assert_eq!(
        mcp[0]["token_id"],
        json!(id),
        "the tools/list line names the token: {}",
        mcp[0]
    );
    assert!(
        mcp[0].get("tool").is_none(),
        "tools/list names no tool: {}",
        mcp[0]
    );
    assert_eq!(
        mcp[1]["token_id"],
        json!(id),
        "the tools/call line names the token too: {}",
        mcp[1]
    );
    assert_eq!(mcp[1]["tool"], json!("files.read"));

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(
        list[0]["last_used_ms"],
        json!(call_at),
        "a second call moves the use even inside the save window: {list}"
    );

    let audit = audit_text(&fixture);
    assert!(
        !audit.contains("srm_") && !audit.contains(&plaintext),
        "the audit must never carry the secret: {audit}"
    );
}

/// A grant-authenticated call has no token record to name, so its audit line carries no token_id.
#[tokio::test]
async fn a_grant_call_has_no_token_id() {
    let fixture = Fixture::new("grant-no-token-id");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb7)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });

    let http = send(
        fixture.app(),
        post_standard(&device_id, &grant, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": [] } }),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "{body}");

    let lines = audit_lines(&fixture);
    let mcp = lines
        .iter()
        .find(|line| line["kind"] == json!("mcp"))
        .expect("the mcp line");
    assert!(
        mcp.get("token_id").is_none(),
        "a grant has no token record to name: {mcp}"
    );
}

/// An unknown token, a revoked one and one used against another device are one 401 on the wire; the
/// denial log keeps the difference, and none of them can rotate the audit.
#[tokio::test]
async fn an_unknown_revoked_or_mismatched_token_is_unauthorized() {
    let fixture = Fixture::new("token-unauthorized");
    let (device_a, _token_a) = fixture.register(&signing_key(0xb3)).await;
    let (device_b, _token_b) = fixture.register(&signing_key(0xb4)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_a, "laptop-dsh", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().expect("the token").to_owned();
    let id = created["id"].as_str().expect("an id").to_owned();

    let (status, body) = send(fixture.app(), post_token(&device_b, &token, ping_request())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body, json!({ "error": "unauthorized" }));
    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert!(
        list[0].get("last_used_ms").is_none(),
        "a foreign-device attempt is not a use: {list}"
    );

    let unknown = format!("srm_{}", "00".repeat(32));
    let (status, body) = send(
        fixture.app(),
        post_token(&device_a, &unknown, ping_request()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body, json!({ "error": "unauthorized" }));

    let (status, _) = send(
        fixture.app(),
        delete(&format!("/tokens/{id}"), Some(&session)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the operator revokes the token"
    );
    let (status, body) = send(fixture.app(), post_token(&device_a, &token, ping_request())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body, json!({ "error": "unauthorized" }));

    let denials = denial_lines(&fixture);
    assert!(
        denials.iter().any(
            |line| line["detail"] == json!("unknown_api_token") && line["status"] == json!(401)
        ),
        "the unknown/revoked credential is a denial line: {denials:?}"
    );
    assert!(
        denials
            .iter()
            .all(|line| line["detail"] != json!("api_token_device_mismatch")),
        "a live token used elsewhere is authenticated, so it is not a denial line"
    );
    let audit = audit_lines(&fixture);
    assert!(
        audit.iter().any(|line| line["kind"] == json!("denied")
            && line["detail"] == json!("api_token_device_mismatch")
            && line["device_id"] == json!(device_b)
            && line.get("token_id").is_none()),
        "the foreign-device attempt is in the operator's audit and names no token: {audit:?}"
    );

    for request in [
        get("/tokens", None),
        post_json("/tokens", json!({})),
        post_json(&format!("/tokens/{id}/rotate"), json!({})),
        delete(&format!("/tokens/{id}"), None),
    ] {
        let (status, _) = send(fixture.app(), request).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "management needs the operator"
        );
    }
}

/// Removing a device removes its tokens with it: no token outlives the machine it was scoped to.
#[tokio::test]
async fn removing_a_device_revokes_its_tokens() {
    let fixture = Fixture::new("token-device-removed");
    let (device_id, _channel) = fixture.register(&signing_key(0xb5)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "laptop-dsh", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/fleet/{device_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .expect("the request builds");
    let (status, _) = send(fixture.app(), request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([]), "the device's token is gone: {list}");
    assert!(
        audit_lines(&fixture)
            .iter()
            .any(|line| line["kind"] == json!("token_revoke")
                && line["device_id"] == json!(device_id)
                && line["detail"] == json!("device_removed")),
        "the removal is audited with the device"
    );
}

/// Every refusal is the console's typed code, not a prose error.
#[tokio::test]
async fn token_management_refusals_are_typed() {
    let fixture = Fixture::new("token-refusals");
    let (device_id, _token) = fixture.register(&signing_key(0xb5)).await;
    let session = fixture.authenticate().await;

    let long = "n".repeat(81);
    let cases = [
        ("   ", StatusCode::BAD_REQUEST, "name_empty"),
        (long.as_str(), StatusCode::BAD_REQUEST, "invalid_name"),
    ];
    for (name, expected, code) in cases {
        let (status, body) = fixture
            .create_token(&session, &device_id, name, &["files.read"], false)
            .await;
        assert_eq!(status, expected, "{code}: {body}");
        assert_eq!(body["error"], json!(code));
    }

    let (status, body) = fixture
        .create_token(&session, &device_id, "no-tools", &[], false)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("empty_tools"));

    let (status, body) = fixture
        .create_token(
            &session,
            &device_id,
            "unknown-tool",
            &["no.such.tool"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("unknown_tool"));

    let unknown_device = "cd".repeat(32);
    let (status, body) = fixture
        .create_token(&session, &unknown_device, "ghost", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], json!("unknown_device"));

    let (status, body) = send(
        fixture.app(),
        delete(&format!("/tokens/{}", "ee".repeat(32)), Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], json!("no_such_token"));

    for index in 0..64 {
        let (status, body) = fixture
            .create_token(
                &session,
                &device_id,
                &format!("client-{index}"),
                &["files.read"],
                false,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "token {index}: {body}");
    }
    let (status, body) = fixture
        .create_token(&session, &device_id, "one-too-many", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"], json!("too_many_tokens"));
}

/// A token minted with a lifetime reports the absolute date in its answer, in the list and after a
/// restart: the operator can see when it will stop working.
#[tokio::test]
async fn a_token_created_with_a_lifetime_reports_when_it_expires() {
    let fixture = Fixture::new("token-expiry-create");
    let (device_id, _token) = fixture.register(&signing_key(0xba)).await;
    let session = fixture.authenticate().await;
    let now = fixture.now_ms();
    let thirty_days_s = 30 * 24 * 3_600;
    let thirty_days_ms = thirty_days_s * 1_000;

    let (status, created) = fixture
        .create_token_with_expiry(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read"],
            false,
            Some(thirty_days_s),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created["expires_at_ms"],
        json!(now + thirty_days_ms),
        "the create answer carries the date"
    );

    let (status, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list[0]["expires_at_ms"],
        json!(now + thirty_days_ms),
        "the list shows the date: {list}"
    );

    let restarted = fixture.restart();
    let session = authenticate_on(&restarted).await;
    let (status, list) = send(restarted.router(), get("/tokens", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list[0]["expires_at_ms"],
        json!(now + thirty_days_ms),
        "a restart keeps the date: {list}"
    );
}

/// Absent is permanent and stays absent: a create without the field, its list row, and a rotation
/// all carry no expiry fact.
#[tokio::test]
async fn a_token_created_without_a_lifetime_is_permanent() {
    let fixture = Fixture::new("token-expiry-default");
    let (device_id, _token) = fixture.register(&signing_key(0xbb)).await;
    let session = fixture.authenticate().await;

    let (status, created) = fixture
        .create_token(&session, &device_id, "laptop-dsh", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert!(
        created.get("expires_at_ms").is_none(),
        "an absent lifetime is no fact: {created}"
    );

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert!(
        list[0].get("expires_at_ms").is_none(),
        "the list omits the absent fact: {list}"
    );

    let id = created["id"].as_str().expect("an id").to_owned();
    fixture.advance_ms(1_000);
    let (status, rotated) = fixture.rotate_token(&session, &id).await;
    assert_eq!(status, StatusCode::CREATED, "{rotated}");
    assert!(
        rotated.get("expires_at_ms").is_none(),
        "rotation keeps a permanent token permanent: {rotated}"
    );
}

/// At `now == expires_at_ms` the token is expired: 401, a denial detail naming expiry, no grant, no
/// last use and no audit line. The check comes before the scope check, so a foreign device sees the
/// same expiry refusal rather than a device mismatch.
#[tokio::test]
async fn an_expired_token_is_refused_as_expired_without_a_grant_or_an_audit() {
    // A short `/mcp` wait, so the pre-implementation run fails fast instead of waiting for a device
    // that was never meant to answer.
    let fixture = Fixture::with_timeout("token-expired", Duration::from_millis(200));
    let server = TestServer::start(&fixture.console).await;
    let (device_a, _token_a, mut device) =
        connected_device(&fixture, &server, &signing_key(0xbc)).await;
    let (device_b, _token_b) = fixture.register(&signing_key(0xbd)).await;
    let session = fixture.authenticate().await;
    let created_at = fixture.now_ms();
    let one_hour_s = 3_600;
    let one_hour_ms = one_hour_s * 1_000;

    let (status, created) = fixture
        .create_token_with_expiry(
            &session,
            &device_a,
            "laptop-dsh",
            &["files.read"],
            false,
            Some(one_hour_s),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().expect("the token").to_owned();
    let expires_at_ms = created_at + one_hour_ms;
    assert_eq!(created["expires_at_ms"], json!(expires_at_ms));

    fixture.advance_ms(one_hour_ms);
    assert_eq!(fixture.now_ms(), expires_at_ms, "the clock is at the edge");
    let audit_before = audit_lines(&fixture).len();
    let denials_before = denial_lines(&fixture).len();

    let (status, body) = send(fixture.app(), post_token(&device_a, &token, ping_request())).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an expired token is a credential failure: {body}"
    );
    assert_eq!(body, json!({ "error": "unauthorized" }));

    let (status, body) = send(fixture.app(), post_token(&device_b, &token, ping_request())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(
        body,
        json!({ "error": "unauthorized" }),
        "the wire answer does not distinguish the reasons"
    );

    let frame = tokio::time::timeout(Duration::from_millis(100), device.next()).await;
    assert!(frame.is_err(), "an expired token must not mint a grant");

    let denials: Vec<Value> = denial_lines(&fixture)
        .into_iter()
        .skip(denials_before)
        .collect();
    assert_eq!(
        denials.len(),
        2,
        "each attempt is a denial line: {denials:?}"
    );
    assert!(
        denials.iter().all(
            |line| line["detail"] == json!("expired_api_token") && line["status"] == json!(401)
        ),
        "the detail names expiry, even on the foreign device: {denials:?}"
    );
    assert_eq!(
        audit_lines(&fixture).len(),
        audit_before,
        "neither attempt touches the operator's audit"
    );

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert!(
        list[0].get("last_used_ms").is_none(),
        "an expired token is not marked used: {list}"
    );
    assert_eq!(
        list[0]["expires_at_ms"],
        json!(expires_at_ms),
        "the list still shows the date: {list}"
    );
}

/// One millisecond before expiry the token still mints a grant: the boundary belongs to the refusal.
#[tokio::test]
async fn a_token_works_one_millisecond_before_it_expires() {
    let fixture = Fixture::new("token-almost-expired");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xbe)).await;
    let session = fixture.authenticate().await;
    let created_at = fixture.now_ms();
    let one_hour_s = 3_600;
    let one_hour_ms = one_hour_s * 1_000;

    let (status, created) = fixture
        .create_token_with_expiry(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read"],
            false,
            Some(one_hour_s),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().expect("the token").to_owned();

    fixture.advance_ms(one_hour_ms - 1);
    assert_eq!(fixture.now_ms(), created_at + one_hour_ms - 1);
    let request = json!({ "jsonrpc": "2.0", "id": 41, "method": "tools/list" });
    let http = send(
        fixture.app(),
        post_token(&device_id, &token, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        let token = &frame["token"];
        assert!(
            token["token_id"].as_str().is_some(),
            "the frame carries a token scope"
        );
        assert_eq!(
            token["expires_at_ms"],
            json!(created_at + one_hour_ms),
            "the scope carries the token's own expiry"
        );
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 41, "result": { "tools": [] } }),
        )
        .await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);

    assert_eq!(
        status,
        StatusCode::OK,
        "one millisecond before expiry is valid: {body}"
    );
    assert_eq!(body["result"]["tools"], json!([]));
}

/// Rotation starts a fresh version of the old policy: a thirty-day token rotates to now plus thirty
/// days, not the old absolute date, and a permanent one stays permanent.
#[tokio::test]
async fn rotation_carries_the_tokens_lifetime() {
    let fixture = Fixture::new("token-rotate-expiry");
    let (device_id, _token) = fixture.register(&signing_key(0xbf)).await;
    let session = fixture.authenticate().await;
    let created_at = fixture.now_ms();
    let thirty_days_s = 30 * 24 * 3_600;
    let thirty_days_ms = thirty_days_s * 1_000;

    let (status, created) = fixture
        .create_token_with_expiry(
            &session,
            &device_id,
            "laptop-dsh",
            &["files.read"],
            false,
            Some(thirty_days_s),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id").to_owned();
    assert_eq!(created["expires_at_ms"], json!(created_at + thirty_days_ms));

    fixture.advance_ms(5_000);
    let rotated_at = fixture.now_ms();
    let (status, rotated) = fixture.rotate_token(&session, &id).await;
    assert_eq!(status, StatusCode::CREATED, "{rotated}");
    assert_eq!(rotated["created_at_ms"], json!(rotated_at));
    assert_eq!(
        rotated["expires_at_ms"],
        json!(rotated_at + thirty_days_ms),
        "the policy is thirty days from the rotation, not the old date"
    );

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(
        list[0]["expires_at_ms"],
        json!(rotated_at + thirty_days_ms),
        "the list shows the new date: {list}"
    );

    let (status, permanent) = fixture
        .create_token(&session, &device_id, "permanent", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{permanent}");
    let permanent_id = permanent["id"].as_str().expect("an id").to_owned();
    let (status, rotated_permanent) = fixture.rotate_token(&session, &permanent_id).await;
    assert_eq!(status, StatusCode::CREATED, "{rotated_permanent}");
    assert!(
        rotated_permanent.get("expires_at_ms").is_none(),
        "a permanent token stays permanent: {rotated_permanent}"
    );
}

/// A lifetime outside one hour to one year is the console's typed 400, and mints nothing.
#[tokio::test]
async fn a_token_expiry_outside_the_window_is_a_typed_400() {
    let fixture = Fixture::new("token-expiry-bounds");
    let (device_id, _token) = fixture.register(&signing_key(0xc0)).await;
    let session = fixture.authenticate().await;

    for (expires_in_s, code) in [(3_599, "expiry_too_short"), (31_536_001, "expiry_too_long")] {
        let (status, body) = fixture
            .create_token_with_expiry(
                &session,
                &device_id,
                "bounds",
                &["files.read"],
                false,
                Some(expires_in_s),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code}: {body}");
        assert_eq!(body["error"], json!(code));
    }

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list, json!([]), "a refused expiry mints nothing: {list}");
}

/// The manual canary for the design's real-client claim.
///
/// It is `#[ignore]`d because it waits for a process outside this test (an official MCP client) and
/// prints the endpoint and a short-lived grant for that process to use. Run it with
/// `--ignored --nocapture`, copy the two `CANARY ...` lines, point the client at them, then stop the
/// test. CI's `cargo test` skips it.
#[tokio::test]
#[ignore = "manual canary: waits for an external MCP client"]
async fn the_official_mcp_client_can_connect() {
    let fixture = Fixture::new("official-client-canary");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xf0)).await;
    let session = fixture.authenticate().await;
    let grant = fixture
        .create_grant_with_tools(&session, &device_id, &["files.read"], 300)
        .await;
    let grant_hex = hex_encode(&serde_json::to_vec(&grant).expect("the grant serializes"));

    println!(
        "CANARY URL http://127.0.0.1:{}/mcp/{device_id}",
        server.addr.port()
    );
    println!("CANARY GRANT {grant_hex}");

    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = device.next() => {
                let Some(Ok(WsMessage::Text(text))) = message else { break };
                let frame: Value = serde_json::from_str(&text).expect("a device frame");
                let id = frame["mcp"]["id"].clone();
                let result = match frame["mcp"]["method"].as_str() {
                    Some("tools/list") => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "tools": [{
                            "name": "files.read",
                            "description": "Read a file.",
                            "inputSchema": {
                                "type": "object",
                                "properties": { "path": { "type": "string" } },
                                "required": ["path"],
                            },
                        }]},
                    }),
                    Some("tools/call") => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": "canary-ok" }] },
                    }),
                    _ => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": "method not found" },
                    }),
                };
                let reply = json!({ "id": frame["id"].clone(), "result": result });
                device
                    .send(WsMessage::Text(reply.to_string()))
                    .await
                    .expect("the canary device answers");
            }
        }
    }
}

/// The manual UI canary: serves the real page with one registered device and prints the URL and the
/// operator token for a browser to sign in with. It is `#[ignore]`d because it waits for a process
/// outside this test; CI skips it.
#[tokio::test]
#[ignore = "manual UI canary: waits for a browser"]
async fn the_console_page_canary() {
    let fixture = Fixture::new("console-page-canary");
    // A real clock, so the card's expiry sentence is the one an operator would see rather than the
    // fixture's 1970 epoch.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after the epoch")
        .as_millis() as u64;
    fixture.clock.store(now, Ordering::Relaxed);
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token) = fixture.register(&signing_key(0xf1)).await;

    println!("UI CANARY URL http://127.0.0.1:{}/", server.addr.port());
    println!("UI CANARY DEVICE {device_id}");
    println!("UI CANARY TOKEN {}", hex(&OPERATOR_TOKEN));

    tokio::time::sleep(Duration::from_secs(300)).await;
}

/// A form body the page's sign-in and sign-out send.
fn post_form(uri: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body = fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("the request builds")
}

fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("the request builds")
}

/// The CSRF value the sign-out form carries.
fn csrf_from(page: &str) -> String {
    let marker = "name=\"csrf\" value=\"";
    let start = page.find(marker).expect("a CSRF field") + marker.len();
    page[start..].chars().take(64).collect()
}

/// A real Access assertion for the fixture team, expiring `seconds` from now. Signed with the same
/// throwaway key the pinned JWKS publishes, so `Access::verify` is what accepts it.
fn access_token(seconds: u64) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key".to_owned());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(ACCESS_KEY_PEM.as_bytes())
        .expect("the fixture key loads");
    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after the epoch")
        .as_secs()
        + seconds;
    let claims = json!({
        "aud": ACCESS_AUDIENCE,
        "iss": ACCESS_ISSUER,
        "exp": exp,
        "email": "operator@example.test",
    });
    jsonwebtoken::encode(&header, &claims, &key).expect("the token is signed")
}

#[tokio::test]
async fn the_rust_page_signs_in_with_a_cookie_and_signs_out_with_csrf() {
    let fixture = Fixture::new("rust-page-login");
    let token = hex(&OPERATOR_TOKEN);

    let (status, _, body) = send_text(fixture.app(), get("/next", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in"), "{body}");
    assert!(
        body.contains("autocomplete=\"off\""),
        "the token box must not invite a password manager: {body}"
    );
    assert!(
        !body.contains("Devices"),
        "the fleet is behind the cookie: {body}"
    );

    let (status, headers, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &token)])).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "a form sign-in redirects");
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .to_owned();
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(
        !cookie.contains("Secure"),
        "plain HTTP must not set Secure: {cookie}"
    );
    assert!(cookie.contains("Path=/next"), "{cookie}");
    assert!(cookie.contains("Max-Age=2592000"), "{cookie}");
    let cookie = cookie
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Devices"), "{body}");
    let csrf = csrf_from(&body);

    let mut request = post_form("/next/logout", &[("csrf", &csrf)]);
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, headers, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(headers
        .get(header::SET_COOKIE)
        .expect("a clearing cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .contains("Max-Age=0"));

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Sign in"),
        "the ended session is signed out: {body}"
    );
}

#[tokio::test]
async fn the_rust_page_refuses_a_wrong_token_and_a_wrong_csrf() {
    let fixture = Fixture::new("rust-page-refusals");

    let (status, _, body) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &"00".repeat(32))]),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Sign in"), "{body}");

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    let mut request = post_form("/next/logout", &[("csrf", &"00".repeat(32))]);
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a wrong CSRF ends nothing");

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Devices"),
        "the session survived the refusal: {body}"
    );
}

#[tokio::test]
async fn the_rust_page_lists_the_registered_fleet() {
    let fixture = Fixture::new("rust-page-fleet");
    let (device_id, _) = fixture.register(&signing_key(0xb4)).await;
    let token = hex(&OPERATOR_TOKEN);
    let (status, headers, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &token)])).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Devices"), "{body}");
    assert!(
        body.contains(&device_id),
        "the registered device is listed: {body}"
    );
    assert!(!body.contains("No devices have registered yet."), "{body}");
}

/// **PROGRESSIVE DISCLOSURE, MEASURED.** On the fleet the rail used to list ten device pages whose
/// hrefs all fall back to `/next` — icons that navigate to the page the reader is already on — and a
/// fleet row printed all eleven pages as one run of words, so the four that answer a daily job looked
/// exactly like the seven that do not. This pins both halves: one rail entry before a device is
/// chosen and eleven after, four links plus one disclosure in the row, and the other seven still
/// ordinary anchors inside it, so the page works with scripting off.
#[tokio::test]
async fn the_fleet_shows_only_the_pages_a_device_can_open() {
    let fixture = Fixture::new("fleet-progressive-disclosure");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb5)).await;
    let cookie = page_cookie(&fixture).await;

    let (status, _, fleet) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fleet.matches("class=\"rail-item").count(),
        1,
        "the fleet's rail is the fleet, not ten links back to itself: {fleet}"
    );
    assert!(fleet.contains("class=\"row-menu more-pages\""), "{fleet}");
    for page in ["Terminal", "Screen", "Files", "Connect"] {
        assert!(
            fleet.contains(&format!(">{page}</a>")),
            "the {page} link is in the row: {fleet}"
        );
    }
    let menu_at = fleet
        .find("class=\"menu-panel page-menu\"")
        .expect("the more-pages disclosure is in the row");
    let menu = &fleet[menu_at..];
    // "State" is a card on the System page now, and "Browser" is the Screen page's second view.
    for page in ["Runs", "System", "Memory", "Monitors", "Vitals"] {
        assert!(
            menu.contains(&format!(">{page}</a>")),
            "the {page} link is one click away: {fleet}"
        );
    }

    // The pages appear with the device: a device page keeps the whole rail.
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/vitals/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("host.facts"),
        "{frame}"
    );
    let facts = json!({
        "machine": "BOX",
        "os": { "name": "Windows", "version": "11" },
        "cpu": { "cores": 8 },
        "uptime_s": 120,
        "memory": { "total_bytes": 8_000_000_000u64, "available_bytes": 4_000_000_000u64 },
        "disks": [],
        "update": {
            "current": { "version": "0.3.112", "channel": "canary" },
            "previous": null,
            "pending": null,
            "last_swap": null,
            "repairs": []
        },
        "shell": { "present": false, "sha256": "", "bytes": 0 },
        "restarts": []
    });
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": facts.to_string() }], "isError": false } }),
    )
    .await;
    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.matches("class=\"rail-item").count(),
        10,
        "a device page lists every rail destination: {body}"
    );
}

#[tokio::test]
async fn the_rust_page_hides_the_fleet_after_the_session_expires() {
    let fixture = Fixture::new("rust-page-expiry");
    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    // The console's clock is injectable; a session lives a month from its mint.
    fixture
        .clock
        .store(START_MS + 30 * DAY_MS + 1, Ordering::Relaxed);

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Sign in"),
        "the expired cookie is signed out: {body}"
    );
    assert!(!body.contains("Devices"), "{body}");
}

/// The session cookie a page sign-in sets.
async fn page_cookie(fixture: &Fixture) -> String {
    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned()
}

#[tokio::test]
async fn the_page_cookie_is_secure_only_behind_https() {
    let fixture = Fixture::new("rust-page-secure");
    let mut request = post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]);
    request.headers_mut().insert(
        axum::http::HeaderName::from_static("x-forwarded-proto"),
        axum::http::HeaderValue::from_static("https"),
    );
    let (status, headers, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a cookie")
        .to_str()
        .expect("ASCII");
    assert!(cookie.contains("Secure"), "{cookie}");

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a cookie")
        .to_str()
        .expect("ASCII");
    assert!(
        !cookie.contains("Secure"),
        "plain HTTP must not set Secure: {cookie}"
    );
}

#[tokio::test]
async fn a_bad_page_token_leaves_the_same_denial_as_auth() {
    let fixture = Fixture::new("rust-page-denial");
    let session = fixture.authenticate().await;
    let (status, _, body) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &"00".repeat(32))]),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Sign in"), "{body}");

    let (status, denials) = send(fixture.app(), get("/denials", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    let newest = denials
        .as_array()
        .expect("a denial list")
        .last()
        .expect("a line");
    assert_eq!(newest["detail"], json!("bad_operator_token"));
}

#[tokio::test]
async fn the_page_logout_refuses_a_foreign_or_absent_csrf_and_a_missing_cookie() {
    let fixture = Fixture::new("rust-page-csrf");
    let a = page_cookie(&fixture).await;
    let b = page_cookie(&fixture).await;
    let (_, _, body) = send_text(fixture.app(), get_with_cookie("/next", &a)).await;
    let csrf_a = csrf_from(&body);

    let mut request = post_form("/next/logout", &[("csrf", &csrf_a)]);
    request
        .headers_mut()
        .insert(header::COOKIE, b.parse().expect("b"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a foreign CSRF ends nothing");

    let mut request = post_form("/next/logout", &[]);
    request
        .headers_mut()
        .insert(header::COOKIE, b.parse().expect("b"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an absent CSRF is 403, not 422"
    );

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/logout", &[("csrf", &csrf_a)]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .expect("a redirect")
            .to_str()
            .expect("ASCII"),
        "/next"
    );

    for cookie in [&a, &b] {
        let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Devices"), "the session survived: {body}");
    }
}

#[tokio::test]
async fn the_console_refuses_framing_on_the_page_and_the_sign_in() {
    let fixture = Fixture::new("rust-page-frame");
    for uri in ["/", "/next"] {
        let (status, headers, _) = send_text(fixture.app(), get(uri, None)).await;
        assert!(
            status == StatusCode::OK || status == StatusCode::SEE_OTHER,
            "{uri}: {status}"
        );
        assert_eq!(
            headers
                .get("x-frame-options")
                .expect("the header")
                .to_str()
                .expect("ASCII"),
            "DENY",
            "{uri}"
        );
    }
}

/// A phase-2 form post that also carries the page's session cookie.
fn post_form_cookie(uri: &str, fields: &[(&str, &str)], cookie: &str) -> Request<Body> {
    let mut request = post_form(uri, fields);
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    request
}

#[tokio::test]
async fn the_rust_page_renames_and_removes_a_device_with_its_csrf() {
    let fixture = Fixture::new("rust-page-mutate");
    let (device_id, _) = fixture.register(&signing_key(0xb4)).await;
    let cookie = page_cookie(&fixture).await;
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    let csrf = csrf_from(&page);

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/fleet/{device_id}/name"),
            &[("csrf", &csrf), ("name", "studio")],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "a rename redirects");
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert!(page.contains("studio"), "{page}");

    // A wrong CSRF is 403 and changes nothing.
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/fleet/{device_id}/name"),
            &[("csrf", &"00".repeat(32)), ("name", "hijacked")],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert!(
        page.contains("studio") && !page.contains("hijacked"),
        "{page}"
    );

    // An empty name is refused, not treated as "clear".
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/fleet/{device_id}/name"),
            &[("csrf", &csrf), ("name", "   ")],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send_text(
        fixture.app(),
        get_with_cookie("/next?error=invalid_name", &cookie),
    )
    .await;
    assert!(page.contains("invalid_name"), "{page}");
    assert!(page.contains("studio"), "{page}");

    // Remove: the row disappears and the console records it.
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/fleet/{device_id}/remove"),
            &[("csrf", &csrf)],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert!(!page.contains(&device_id), "the device row is gone: {page}");
    let (status, _, audit) =
        send_text(fixture.app(), get_with_cookie("/next/audit", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(audit.contains("device_remove"), "{audit}");
}

#[tokio::test]
async fn the_rust_page_mints_an_enrollment_code_once_and_gates_the_logs() {
    let fixture = Fixture::new("rust-page-enroll");
    let cookie = page_cookie(&fixture).await;
    let (_, _, page) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    let csrf = csrf_from(&page);

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form_cookie("/next/enroll", &[("csrf", &csrf)], &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .expect("a redirect")
        .to_str()
        .expect("ASCII");
    assert!(location.starts_with("/next?enroll="), "{location}");
    let code = location.trim_start_matches("/next?enroll=");
    assert_eq!(code.len(), 64, "the code is 32 bytes in hex");

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next?enroll={code}"), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Enrollment code:") && page.contains(code),
        "{page}"
    );

    // Wrong CSRF: no code, 403.
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie("/next/enroll", &[("csrf", &"00".repeat(32))], &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The logs are login-gated, then readable.
    for uri in ["/next/audit", "/next/denials"] {
        let (status, _, body) = send_text(fixture.app(), get(uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Sign in"), "{uri}: {body}");
        let (status, _, body) = send_text(fixture.app(), get_with_cookie(uri, &cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("Sign in"), "{uri}: {body}");
    }
}

/// Sign in through the page and return (cookie, csrf), the pair every `/next` mutation needs.
async fn sign_in_to_the_page(fixture: &Fixture) -> (String, String) {
    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "the page's sign-in redirects"
    );
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();
    let (_, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    let csrf = csrf_from(&body);
    (cookie, csrf)
}

fn post_json_cookie(uri: &str, body: Value, cookie: &str, csrf: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie);
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf", csrf);
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("the request builds")
}

#[tokio::test]
async fn the_terminal_page_is_gated_by_the_cookie_and_carries_the_device_and_csrf() {
    let fixture = Fixture::new("terminal-page");
    let (device_id, _token) = fixture.register(&signing_key(0xe1)).await;

    let (status, _, body) = send_text(
        fixture.app(),
        get(&format!("/next/terminal/{device_id}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Sign in"),
        "the page is behind the cookie: {body}"
    );

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/terminal/{device_id}"), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(&format!("data-device=\"{device_id}\"")),
        "{page}"
    );
    assert!(page.contains(&format!("data-csrf=\"{csrf}\"")), "{page}");
    assert!(
        page.contains("/assets/xterm.js"),
        "the terminal script: {page}"
    );
    assert!(
        page.contains("/assets/terminal.js"),
        "the page's glue: {page}"
    );

    // An unknown device is a 404, not a page that opens a shell somewhere else.
    let (status, _, _) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/terminal/{}", "00".repeat(32)), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_terminal_mutation_without_the_page_csrf_is_refused_before_the_device_hears_of_it() {
    let fixture = Fixture::new("terminal-csrf");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe2)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let (status, _, _) = send_text(
        fixture.app(),
        post_json_cookie(
            &format!("/next/terminal/{device_id}/create"),
            json!({ "kind": "pty" }),
            &cookie,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            next_json(&mut device)
        )
        .await
        .is_err(),
        "a refused create must not reach the device"
    );
    let _ = &mut device;
}

#[tokio::test]
async fn the_terminal_page_relays_create_and_read_with_the_cursor_it_was_given() {
    let fixture = Fixture::new("terminal-relay");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe3)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_json_cookie(
        &format!("/next/terminal/{device_id}/create"),
        json!({ "kind": "pty", "rows": 30, "cols": 100 }),
        &cookie,
        Some(&csrf),
    );
    let create = tokio::spawn(async move { send_with_headers(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"]["method"], json!("tools/call"), "{frame}");
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("terminal.create"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["kind"], json!("pty"));
    assert_eq!(frame["mcp"]["params"]["arguments"]["rows"], json!(30));
    assert_eq!(frame["mcp"]["params"]["arguments"]["cols"], json!(100));
    answer(
        &mut device,
        &frame,
        // The device's reply is the JSON-RPC envelope it would send an MCP client.
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "id": "session-1", "kind": "pty", "shell": "pwsh" }).to_string() }],
                "isError": false,
            },
        }),
    )
    .await;

    let (status, _, created) = create.await.expect("the create call finishes");
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["id"], json!("session-1"));
    assert_eq!(created["shell"], json!("pwsh"));

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/terminal/{device_id}/session-1/read?cursor=7"),
        &cookie,
    );
    let read = tokio::spawn(async move { send_with_headers(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("terminal.read"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["id"],
        json!("session-1")
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["cursor"], json!(7));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "data": "hello", "cursor": 12, "alive": true }).to_string() }],
                "isError": false,
            },
        }),
    )
    .await;
    let (status, _, body) = read.await.expect("the read call finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "data": "hello", "cursor": 12, "alive": true })
    );

    // A device refusal keeps its own sentence in the page's answer.
    let app = fixture.app();
    let request = post_json_cookie(
        &format!("/next/terminal/{device_id}/missing/write"),
        json!({ "data": "x" }),
        &cookie,
        Some(&csrf),
    );
    let write = tokio::spawn(async move { send_with_headers(app, request).await });
    let frame = next_json(&mut device).await;
    answer_error(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32001, "message": "terminal.write: no such session" },
        }),
    )
    .await;
    let (status, _, body) = write.await.expect("the write call finishes");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(
        body["error"],
        json!("terminal.write: no such session"),
        "the device's refusal is passed through: {body}"
    );
}

/// The operator's own scope is `["*"]`, and the proxy must read that as a wildcard: an exact
/// comparison here silently refused every grantless `tools/call` while the page had already stopped
/// signing grants.
#[tokio::test]
async fn an_operator_mcp_call_without_a_grant_reaches_the_device() {
    let fixture = Fixture::new("mcp-operator-scope");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe4)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id,
            "request": {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": "files.read", "arguments": { "path": "notes.txt" } }
            }
        }),
    );
    let call = tokio::spawn(async move { send_with_headers(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.read"),
        "the frame carries the call, not a refusal: {frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "content": [{ "type": "text", "text": "hello" }],
            "isError": false,
        }),
    )
    .await;

    let (status, _, body) = call.await.expect("the call finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["content"][0]["text"], json!("hello"));
}

#[tokio::test]
async fn the_files_page_lists_the_devices_root_and_marks_directories_as_links() {
    let fixture = Fixture::new("files-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe5)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/files/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.list"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["path"], json!(""));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "name": "notes.txt", "kind": "file", "len": 5 },
                    { "name": "logs", "kind": "directory", "len": 0 }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    let html = body;
    assert!(html.contains("notes.txt"), "{html}");
    assert!(html.contains("logs"), "{html}");
    assert!(
        html.contains(&format!("/next/files/{device_id}/read?path=notes.txt")),
        "a file is a read link: {html}"
    );
    assert!(
        html.contains(&format!("/next/files/{device_id}?path=logs")),
        "a directory is a listing link: {html}"
    );
    assert!(html.contains("Write a file"), "{html}");
}

#[tokio::test]
async fn the_files_page_shows_a_file_and_a_write_without_csrf_is_refused() {
    let fixture = Fixture::new("files-read-write");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe6)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/files/{device_id}/read?path=notes.txt"),
        &cookie,
    );
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.read"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": "hello from the file" }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the read page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("hello from the file"), "{body}");

    // A write without the page's CSRF value changes nothing and never reaches the device.
    let mut request = post_form(
        &format!("/next/files/{device_id}/write"),
        &[
            ("csrf", &"00".repeat(32)),
            ("path", "notes.txt"),
            ("content", "x"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            next_json(&mut device)
        )
        .await
        .is_err(),
        "a refused write must not reach the device"
    );

    // With it, the write is forwarded and the page returns to the file's directory.
    let mut request = post_form(
        &format!("/next/files/{device_id}/write"),
        &[
            ("csrf", &csrf),
            ("path", "logs/notes.txt"),
            ("content", "written"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let write = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.write"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["path"],
        json!("logs/notes.txt")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["content"],
        json!("written")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "path": "logs/notes.txt", "bytes": 7 }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, headers, _) = write.await.expect("the write finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        // The write says which file it replaced, so the page can acknowledge it once.
        Some(format!("/next/files/{device_id}?path=logs&wrote=logs/notes.txt").as_str())
    );
}

#[tokio::test]
async fn the_runs_page_lists_the_devices_history_and_a_run_is_forwarded() {
    let fixture = Fixture::new("runs-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe7)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/runs/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("runs.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "run-1", "command": "pwsh", "args": ["-NoProfile", "Get-Date"],
                      "started_at_ms": 1_793_000_000_000u64, "duration_ms": 120,
                      "exit_code": 0, "timed_out": false }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the runs page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Run a command") || body.contains("History"),
        "{body}"
    );
    assert!(body.contains("pwsh -NoProfile Get-Date"), "{body}");

    // One run: the form is forwarded unchanged and its output is what the page shows.
    let mut request = post_form(
        &format!("/next/runs/{device_id}/exec"),
        &[
            ("csrf", &csrf),
            ("command", "echo"),
            ("args", "hello\nfrom\nthe\npage"),
            ("timeout_ms", "5000"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let run = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("runs.exec"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["command"],
        json!("echo")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["args"],
        json!(["hello", "from", "the", "page"])
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["timeout_ms"],
        json!(5000)
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["shell"], json!(false));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({
                    "id": "run-2", "exit_code": 0, "timed_out": false, "duration_ms": 42,
                    "stdout": "hello\nfrom\nthe\npage\n", "stderr": "", "truncated": false,
                    "history_recorded": true
                }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = run.await.expect("the run finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("hello"), "{body}");
    assert!(body.contains("exit 0"), "{body}");
    assert!(body.contains("run-2"), "{body}");
}

/// **A REFUSED ACTION IS A PAGE.** The run form used to answer a device refusal with the raw
/// `{"error":"…"}` body: no chrome, no navigation, on the screen of someone who pressed Run.
#[tokio::test]
async fn a_device_refusal_of_a_run_is_rendered_as_a_page() {
    let fixture = Fixture::new("runs-refusal");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xda)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/runs/{device_id}/exec"),
        &[("csrf", &csrf), ("command", "pwsh"), ("args", "Get-Date")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let run = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("runs.exec"),
        "{frame}"
    );
    // A JSON-RPC error frame, sent whole: `answer` wraps its argument in a `result`, which is how
    // the console could not tell this test's refusal from a malformed run answer.
    device
        .send(WsMessage::Text(
            json!({
                "id": frame["id"].clone(),
                "error": { "error": { "code": -32000, "message": "the device refused that command" } }
            })
            .to_string(),
        ))
        .await
        .expect("the device refuses");
    let (status, _, body) = run.await.expect("the run finishes");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(
        body.contains("Refused"),
        "the page names what happened: {body}"
    );
    assert!(
        body.contains("the device refused that command"),
        "the device's sentence is on the page: {body}"
    );
    assert!(
        body.contains(&format!("/next/runs/{device_id}")),
        "the page offers the way back: {body}"
    );
    assert!(
        !body.trim_start().starts_with('{'),
        "the answer is a page, not a JSON body: {body}"
    );
}

/// The same shape, one handler further: a refused write is a page too, and it offers the files page
/// back rather than a JSON body with no way home.
#[tokio::test]
async fn a_device_refusal_of_a_write_is_rendered_as_a_page() {
    let fixture = Fixture::new("files-write-refusal");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdb)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/files/{device_id}/write"),
        &[("csrf", &csrf), ("path", "notes.txt"), ("content", "hello")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let write = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.write"),
        "{frame}"
    );
    device
        .send(WsMessage::Text(
            json!({
                "id": frame["id"].clone(),
                "error": { "error": { "code": -32000, "message": "the file is read-only" } }
            })
            .to_string(),
        ))
        .await
        .expect("the device refuses");
    let (status, _, body) = write.await.expect("the write finishes");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("Refused"), "{body}");
    assert!(body.contains("the file is read-only"), "{body}");
    assert!(body.contains(&format!("/next/files/{device_id}")), "{body}");
    assert!(!body.trim_start().starts_with('{'), "{body}");
}

#[tokio::test]
async fn a_run_without_the_page_csrf_is_refused_before_the_device_hears_of_it() {
    let fixture = Fixture::new("runs-csrf");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe8)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/runs/{device_id}/exec"),
        &[
            ("csrf", &"00".repeat(32)),
            ("command", "echo"),
            ("args", "x"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            next_json(&mut device)
        )
        .await
        .is_err(),
        "a refused run must not reach the device"
    );
}

#[tokio::test]
async fn the_state_page_shows_the_devices_own_document() {
    let fixture = Fixture::new("state-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe9)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    // The document lives on the System page now: the old address redirects there, and the device
    // is asked for nothing on the way.
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/state/{device_id}"), &cookie);
    let (status, headers, _body) = send_text(app, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a location");
    assert_eq!(location, format!("/next/system/{device_id}"));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), next_json(&mut device))
            .await
            .is_err(),
        "the redirect asks the device nothing"
    );
}

#[tokio::test]
async fn the_system_page_lists_processes_and_forwards_a_kill() {
    let fixture = Fixture::new("system-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xea)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/system/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("system.process_list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "pid": 4242, "name": "pwsh.exe", "memory_kb": 1024 }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    // The page then asks for the state document it prints below the process list.
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("state.get"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "health": { "status": "ok" } }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the system page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("pwsh.exe"), "{body}");
    assert!(body.contains("End a process"), "{body}");

    let mut request = post_form(
        &format!("/next/system/{device_id}/kill"),
        &[("csrf", &csrf), ("pid", "4242")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let kill = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("system.process_kill"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["pid"], json!(4242));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "killed": [4242], "failed": [] }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, headers, _) = kill.await.expect("the kill finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/system/{device_id}?killed=1&failed=0").as_str())
    );
}

#[tokio::test]
async fn the_memory_page_lists_entries_and_forwards_an_append() {
    let fixture = Fixture::new("memory-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xeb)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/memory/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "m1", "text": "the antenna is on COM3", "tags": ["hardware", "serial"] }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the memory page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("the antenna is on COM3"), "{body}");
    assert!(body.contains("hardware, serial"), "{body}");

    let mut request = post_form(
        &format!("/next/memory/{device_id}/append"),
        &[
            ("csrf", &csrf),
            ("text", "the key is in the drawer"),
            ("tags", "keys, office"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let append = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.append"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["text"],
        json!("the key is in the drawer")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["tags"],
        json!(["keys", "office"])
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "appended": true, "id": "m2" }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, headers, _) = append.await.expect("the append finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/memory/{device_id}").as_str())
    );
}

#[tokio::test]
async fn a_memory_delete_without_the_page_csrf_is_refused() {
    let fixture = Fixture::new("memory-csrf");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xec)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_form(
        &format!("/next/memory/{device_id}/delete"),
        &[("csrf", &"00".repeat(32)), ("id", "m1")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            next_json(&mut device)
        )
        .await
        .is_err(),
        "a refused delete must not reach the device"
    );
}

#[tokio::test]
async fn the_screen_page_renders_the_capture_as_a_data_url() {
    let fixture = Fixture::new("screen-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xed)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/screen/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("screen.capture"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({
                    "format": "png", "data": "QUJD", "width": 10, "height": 20
                }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the screen page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("src=\"data:image/png;base64,QUJD\""),
        "{body}"
    );
    assert!(body.contains("10×20"), "{body}");
}

#[tokio::test]
async fn the_monitors_page_lists_a_target_and_forwards_an_add() {
    let fixture = Fixture::new("monitors-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xee)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/monitors/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("monitor.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "tcp:10.0.0.1:443", "kind": "tcp", "host": "10.0.0.1", "port": 443,
                      "path": null, "expect": null, "interval_ms": 15000,
                      "summary": { "up_now": true, "since_ms": 1, "up_pct": 100, "drops": 0, "last_status": null } }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the monitors page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("10.0.0.1:443"), "{body}");
    assert!(body.contains("Watch a target"), "{body}");

    let mut request = post_form(
        &format!("/next/monitors/{device_id}/add"),
        &[
            ("csrf", &csrf),
            ("kind", "http"),
            ("host", "192.168.1.1"),
            ("port", "80"),
            ("interval_ms", "20000"),
            ("path", "/health"),
            ("expect", "ok"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let add = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("monitor.add"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["kind"], json!("http"));
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["host"],
        json!("192.168.1.1")
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["port"], json!(80));
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["path"],
        json!("/health")
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["expect"], json!("ok"));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "id": "http:192.168.1.1:80" }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, headers, _) = add.await.expect("the add finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/monitors/{device_id}").as_str())
    );
}

/// Editing from a filtered list keeps the filter: otherwise the entry the operator clicked can be
/// outside the unfiltered page, and the edit card silently does not appear.
#[tokio::test]
async fn editing_from_a_search_keeps_the_search() {
    let fixture = Fixture::new("memory-edit-filter");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd4)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/memory/{device_id}?q=needle"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.search"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!([
            { "id": "entry-1", "text": "a needle", "tags": [] }
        ]).to_string() }], "isError": false } }),
    )
    .await;
    let (_, _, body) = page.await.expect("the page finishes");
    assert!(
        body.contains("?edit=entry-1&amp;q=needle"),
        "the edit link carries the search: {body}"
    );

    // The form carries it too, so the save comes back to the same filtered list.
    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/memory/{device_id}/update"),
        &[
            ("csrf", &csrf),
            ("id", "entry-1"),
            ("text", "a needle"),
            ("tags", ""),
            ("q", "needle"),
        ],
        &cookie,
    );
    let update = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.update"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": "{}" }], "isError": false } }),
    )
    .await;
    let (status, headers, _) = update.await.expect("the update finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(
        location.contains("q=needle"),
        "the filter comes back: {location}"
    );
}

/// A refused action's sentence is on the page the browser lands on, not swallowed by it.
#[tokio::test]
async fn the_browser_page_shows_a_refused_action_sentence() {
    let fixture = Fixture::new("browser-error-shown");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd6)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/browser/{device_id}?error=the%20target%20is%20not%20there"),
        &cookie,
    );
    let page = tokio::spawn(async move { send_text(app, request).await });
    for name in ["browser.list", "browser.screenshot"] {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"]["params"]["name"], json!(name), "{frame}");
        let text = if name == "browser.list" {
            json!([{ "id": "target-1", "title": "example", "url": "https://example.com/", "type": "page" }]).to_string()
        } else {
            json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string()
        };
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": text }], "isError": false } }),
        )
        .await;
    }
    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("the target is not there"), "{body}");
}

/// A refused mkdir leaves a sentence and still lists the directory it was refused in.
#[tokio::test]
async fn a_refused_mkdir_still_shows_the_directory() {
    let fixture = Fixture::new("files-error-keeps-listing");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd5)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/files/{device_id}?error=that%20folder%20is%20not%20allowed"),
        &cookie,
    );
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("files.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!([
            { "name": "notes.txt", "kind": "file", "len": 3 }
        ]).to_string() }], "isError": false } }),
    )
    .await;
    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("that folder is not allowed"), "{body}");
    assert!(
        body.contains("notes.txt"),
        "the listing survives the error: {body}"
    );
}

/// The glue builds request paths itself, so the id it interpolates must be encoded: a device that
/// answers with `../<other-device>/session-3` must not turn the pane into a cross-device write.
#[tokio::test]
async fn the_terminal_glue_encodes_the_session_id() {
    let fixture = Fixture::new("terminal-glue-encoding");
    let (status, _, js) = send_text(fixture.app(), get("/assets/terminal.js", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        js.contains("encodeURIComponent(id)"),
        "the id is encoded before it becomes a path: {js}"
    );
}

/// The server does not rely on the glue: an id that is not one is refused before any device call.
#[tokio::test]
async fn a_terminal_id_that_is_not_one_is_refused() {
    let fixture = Fixture::new("terminal-id-guard");
    let (device_id, _) = fixture.register(&signing_key(0xd7)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_json_cookie(
        &format!("/next/terminal/{device_id}/session-1%2Fother/write"),
        json!({ "data": "x" }),
        &cookie,
        Some(&csrf),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("a header"),
    );
    let (status, _, _) = send_with_headers(fixture.app(), request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The read path takes the same id and refuses it the same way.
    let (status, _, _) = send_text(
        fixture.app(),
        get_with_cookie(
            &format!("/next/terminal/{device_id}/session-1%2Fother/read?cursor=0"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The browser pane pumps frames: a JSON frame route behind the session, and an image the script can
/// replace without re-rendering the page.
#[tokio::test]
async fn the_browser_pane_can_take_a_fresh_frame() {
    let fixture = Fixture::new("browser-frame");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xcf)).await;
    let cookie = page_cookie(&fixture).await;

    // The page renders one capture and the hook the script follows.
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/browser/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!([
            { "id": "target-1", "title": "example", "url": "https://example.com/", "type": "page" }
        ]).to_string() }], "isError": false } }),
    )
    .await;
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.screenshot"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string() }], "isError": false } }),
    )
    .await;
    let (_, _, body) = page.await.expect("the page finishes");
    assert!(body.contains("id=\"cast\""), "{body}");
    assert!(body.contains("data-target=\"target-1\""), "{body}");
    assert!(body.contains("/assets/browser.js"), "{body}");

    // The frame route takes the newest frame of the device's live stream, for the pump.
    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/browser/{device_id}/frame?target=target-1"),
        &cookie,
    );
    let frame_request = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.cast"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["target"],
        json!("target-1")
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!({ "format": "jpeg", "data": "/9j/AAAA", "width": 1280, "height": 800, "at_ms": 1_793_000_000_000u64 }).to_string() }], "isError": false } }),
    )
    .await;
    let (status, _, body) = frame_request.await.expect("the frame finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    let frame: Value = serde_json::from_str(&body).expect("a JSON frame");
    assert_eq!(frame["format"], json!("jpeg"));
    assert_eq!(frame["data"], json!("/9j/AAAA"));
    assert_eq!(frame["width"], json!(1280));
    assert_eq!(frame["stream"], json!(true), "the pane may poll faster");

    // Without a session it refuses before touching the device.
    let request = get(
        &format!("/next/browser/{device_id}/frame?target=target-1"),
        None,
    );
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A transient failure of the live stream is NOT an old device: it reaches the caller, which keeps
/// trying the stream, instead of silently downgrading the pane to screenshots for the tab's life.
#[tokio::test]
async fn a_transient_cast_failure_is_a_502_not_a_fallback() {
    let fixture = Fixture::new("browser-frame-transient");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc6)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/browser/{device_id}/frame?target=target-1"),
        &cookie,
    );
    let frame_request = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.cast"),
        "{frame}"
    );
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32603, "message": "browser.cast: the screencast reader stopped before a frame arrived" } }),
    )
    .await;

    // No screenshot is asked for: the failure is the answer.
    let (status, _, body) = frame_request.await.expect("the frame finishes");
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("screencast reader"), "{body}");
}

/// A device older than `browser.cast` still fills the pane: the console falls back to one
/// screenshot per poll rather than failing the frame.
#[tokio::test]
async fn the_frame_route_falls_back_to_a_screenshot_on_an_older_device() {
    let fixture = Fixture::new("browser-frame-fallback");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc8)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/browser/{device_id}/frame?target=target-1"),
        &cookie,
    );
    let frame_request = tokio::spawn(async move { send_text(app, request).await });

    // The old build has no browser.cast.
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.cast"),
        "{frame}"
    );
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32601, "message": "unknown tool: browser.cast" } }),
    )
    .await;

    // The console asks for a screenshot instead.
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.screenshot"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string() }], "isError": false } }),
    )
    .await;

    let (status, _, body) = frame_request.await.expect("the frame finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    let frame: Value = serde_json::from_str(&body).expect("a JSON frame");
    assert_eq!(frame["format"], json!("png"));
    assert_eq!(frame["data"], json!("QUJD"));
    assert_eq!(
        frame["stream"],
        json!(false),
        "the pane learns to slow down: {frame}"
    );
}

/// Once the pane knows the device has no stream, the console stops asking for one: `stream=0` goes
/// straight to a screenshot and makes one device call, not two.
#[tokio::test]
async fn a_pane_that_knows_there_is_no_stream_skips_the_ask() {
    let fixture = Fixture::new("browser-frame-nostream");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc7)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(
        &format!("/next/browser/{device_id}/frame?target=target-1&stream=0"),
        &cookie,
    );
    let frame_request = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.screenshot"),
        "no cast attempt at all: {frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string() }], "isError": false } }),
    )
    .await;

    let (status, _, body) = frame_request.await.expect("the frame finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    let frame: Value = serde_json::from_str(&body).expect("a JSON frame");
    assert_eq!(frame["stream"], json!(false));
}

/// The install card is the device's own account of its generations and its last swap: the console
/// shows it and keeps no copy.
#[tokio::test]
async fn the_vitals_page_shows_the_install_it_reads() {
    let fixture = Fixture::new("vitals-install");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd0)).await;
    let cookie = page_cookie(&fixture).await;
    let facts = json!({
        "machine": "BOX",
        "os": { "name": "Windows", "version": "11" },
        "cpu": { "cores": 8 },
        "uptime_s": 120,
        "memory": { "total_bytes": 8_000_000_000u64, "available_bytes": 4_000_000_000u64 },
        "disks": [],
        "update": {
            "current": { "version": "0.3.112", "channel": "canary" },
            "previous": { "version": "0.3.111", "channel": "canary" },
            "pending": null,
            "last_swap": {
                "outcome": "rolled_back",
                "version": "0.3.111",
                "at_ms": 1_790_990_000_000u64,
                "reason": "the new build did not answer /health"
            },
            "repairs": [{ "from": "previous", "at_ms": 1_790_990_300_000u64 }]
        },
        "shell": { "present": true, "sha256": "cd".repeat(32), "bytes": 1411072 },
        "restarts": [
            { "at_ms": 1_790_991_000_000u64, "version": "0.3.112", "channel": "canary", "pid": 4242 },
            { "at_ms": 1_790_990_000_000u64, "version": "0.3.111", "channel": "canary", "pid": 1111 }
        ]
    });

    let page = visit_vitals(&fixture, &cookie, &device_id, &mut device, &facts).await;

    assert!(page.contains("Install"), "{page}");
    assert!(page.contains("0.3.112"), "the current generation: {page}");
    assert!(page.contains("Previous"), "{page}");
    assert!(page.contains("0.3.111"), "the previous generation: {page}");
    assert!(
        page.contains("rolled back"),
        "the last swap's outcome: {page}"
    );
    assert!(
        page.contains("the new build did not answer /health"),
        "and the launcher's reason: {page}"
    );
    // The launcher's "from previous" is rendered as what happened, with the bytes that came back.
    assert!(
        page.contains("restored the previous generation"),
        "the repair says what it did: {page}"
    );
    assert!(
        page.contains("no generation"),
        "an absent pending says so: {page}"
    );
    assert!(
        page.contains("Tray") && page.contains(&"cd".repeat(32)),
        "the tray's digest is on the card, because the canary update does not carry it: {page}"
    );
    assert!(
        page.contains("Restarts"),
        "the boot journal is on the card: {page}"
    );
    assert!(
        page.contains("0.3.112") && page.contains("canary"),
        "a restart row names its release: {page}"
    );
}

/// Open the vitals page once, answering its `host.facts` call with `facts`, and return the page.
async fn visit_vitals(
    fixture: &Fixture,
    cookie: &str,
    device_id: &str,
    device: &mut DeviceSocket,
    facts: &Value,
) -> String {
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/vitals/{device_id}"), cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("host.facts"),
        "{frame}"
    );
    answer(
        device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": facts.to_string() }], "isError": false } }),
    )
    .await;
    page.await.expect("the page finishes").2
}

/// A token that already consents is never asked again: a device answering -32002 to it is refusing
/// for its own reasons, and the console must not stall the caller for the whole deadline.
#[tokio::test]
async fn a_consenting_token_is_not_asked_for_approval() {
    let fixture = Fixture::new("approval-consenting");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xca)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "trusted", &["terminal.create"], true)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 65,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let app = fixture.app();
    let request = post_token(&device_id, &plaintext, call.clone());
    let client = tokio::spawn(async move { send(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"], call, "{frame}");
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 65, "error": { "code": -32002, "message": "the device refuses this one" } }),
    )
    .await;

    let (status, body) = client.await.expect("the client finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32002), "{body}");

    // Nothing was held for the operator to answer.
    let cookie = page_cookie(&fixture).await;
    let (_, _, count) = send_text(
        fixture.app(),
        get_with_cookie("/next/approvals/count", &cookie),
    )
    .await;
    let count: Value = serde_json::from_str(&count).expect("a JSON count");
    assert_eq!(count["pending"], json!(0));
}

/// Facts with no totals are no reading: the trend does not record a zero it did not measure.
#[tokio::test]
async fn facts_without_a_reading_do_not_enter_the_trend() {
    let fixture = Fixture::new("vitals-no-reading");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc9)).await;
    let cookie = page_cookie(&fixture).await;
    let facts = json!({
        "machine": "BOX",
        "os": { "name": "Linux", "version": "6.5" },
        "cpu": { "cores": 4 },
        "uptime_s": 60,
        "disks": [],
        "update": { "current": null, "previous": null, "pending": null, "last_swap": null, "repairs": [] }
    });

    let first = visit_vitals(&fixture, &cookie, &device_id, &mut device, &facts).await;
    let second = visit_vitals(&fixture, &cookie, &device_id, &mut device, &facts).await;

    assert!(first.contains("One point so far"), "{first}");
    assert!(
        second.contains("One point so far"),
        "two visits without readings are still no point: {second}"
    );
}

/// An approved call whose device goes away before answering is UNKNOWN, never "needs approval":
/// the frame may have run, so a retry could run it twice.
#[tokio::test]
async fn an_approved_call_whose_device_disappears_is_unknown() {
    let fixture = Fixture::new("approval-unknown");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xcc)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 63,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let app = fixture.app();
    let request = post_token(&device_id, &plaintext, call.clone());
    let client = tokio::spawn(async move { send(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"], call, "{frame}");
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 63, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
    )
    .await;

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let id = wait_for_approval(&fixture, &cookie).await;

    let approve = {
        let app = fixture.app();
        let request = post_form_cookie(
            &format!("/next/approvals/{id}/approve"),
            &[("csrf", &csrf)],
            &cookie,
        );
        tokio::spawn(async move { send_text(app, request).await })
    };

    // The re-send arrives, and then the device is gone without answering it.
    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"], call, "{frame}");
    drop(device);

    let (status, _, _) = approve.await.expect("the approve finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, body) = client.await.expect("the client finishes");
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{body}");
    assert!(
        body.to_string().contains("unknown"),
        "the answer says the outcome is unknown: {body}"
    );

    let lines = audit_lines(&fixture);
    assert!(
        lines.iter().any(|line| {
            line["kind"] == "approval"
                && line["status"] == 504
                && line["detail"] == "approved, but the device did not answer"
        }),
        "the record names it: {lines:#?}"
    );
}

/// Removing a device refuses its held calls instead of leaving them on the dashboard.
#[tokio::test]
async fn removing_a_device_refuses_its_held_calls() {
    let fixture = Fixture::new("approval-device-removed");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xcb)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 64,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let app = fixture.app();
    let request = post_token(&device_id, &plaintext, call.clone());
    let client = tokio::spawn(async move { send(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"], call, "{frame}");
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 64, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
    )
    .await;

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let _ = wait_for_approval(&fixture, &cookie).await;

    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/fleet/{device_id}/remove"),
            &[("csrf", &csrf)],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, _, count) = send_text(
        fixture.app(),
        get_with_cookie("/next/approvals/count", &cookie),
    )
    .await;
    let count: Value = serde_json::from_str(&count).expect("a JSON count");
    assert_eq!(
        count["pending"],
        json!(0),
        "the removed device's call is gone"
    );

    let (_, body) = client.await.expect("the client finishes");
    assert_eq!(
        body["error"]["code"],
        json!(-32002),
        "the refused call keeps the device's sentence: {body}"
    );
}

/// A client that hangs up takes its held call with it: nothing sits on the dashboard waiting for a
/// request that no longer exists.
#[tokio::test]
async fn a_held_call_leaves_when_its_client_hangs_up() {
    let fixture = Fixture::new("approval-hangup");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xcd)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 62,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let app = fixture.app();
    let request = post_token(&device_id, &plaintext, call.clone());
    let client = tokio::spawn(async move { send(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"], call, "{frame}");
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 62, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
    )
    .await;

    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;
    let _ = wait_for_approval(&fixture, &cookie).await;

    // The client goes away without an answer.
    client.abort();

    let mut pending = json!(1);
    for _ in 0..200 {
        let (_, _, body) = send_text(
            fixture.app(),
            get_with_cookie("/next/approvals/count", &cookie),
        )
        .await;
        pending = serde_json::from_str::<Value>(&body)
            .map(|body| body["pending"].clone())
            .unwrap_or(json!(1));
        if pending == json!(0) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        pending,
        json!(0),
        "the hung-up call is not waiting for anyone"
    );

    // **AND THE RECORD CLOSES.** The ask was written when it started; a client that leaves without
    // an answer is not a refusal and must not read as one, but a question with no closing line is
    // indistinguishable from a console that lost the answer.
    let mut closing = None;
    for _ in 0..200 {
        closing = audit_lines(&fixture).into_iter().find(|line| {
            line["kind"] == json!("approval") && line["detail"] == json!("client left")
        });
        if closing.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let closing = closing.expect("the ask that was abandoned leaves a closing line");
    assert_eq!(closing["status"], json!(200));
    assert_eq!(closing["tool"], json!("terminal.create"));
    assert!(
        closing["token_id"].is_string(),
        "the closing line names the token: {closing}"
    );
}

/// Nobody answers: the gate's deadline ends the call, and the client gets the device's refusal.
/// Expiry fails CLOSED, as the legacy gate did — this is the one path a person never sees.
#[tokio::test(start_paused = true)]
async fn an_unanswered_approval_expires_closed() {
    let fixture = Fixture::new("approval-expiry");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xce)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, call.clone()),
    );

    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], call, "{frame}");
        answer_error(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 61, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
        )
        .await;
    };

    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;
    let watcher = async {
        let id = wait_for_approval(&fixture, &cookie).await;
        assert!(!id.is_empty());
        // Nobody presses anything; time passes.
        tokio::time::advance(std::time::Duration::from_secs(601)).await;
    };

    let ((status, body), (), ()) = tokio::join!(http, device_side, watcher);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32002), "{body}");

    let lines = audit_lines(&fixture);
    assert!(
        lines
            .iter()
            .any(|line| line["kind"] == "approval" && line["detail"] == "timed out"),
        "the record says it expired: {lines:#?}"
    );
}

/// The badge is on every signed-in page, and its count route answers only to a session.
#[tokio::test]
async fn every_page_carries_the_approvals_badge() {
    let fixture = Fixture::new("approvals-badge");
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(fixture.app(), get_with_cookie("/next/audit", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("id=\"approvals-badge\""), "{page}");
    assert!(page.contains("/assets/approvals.js"), "{page}");

    let (status, count) = send(fixture.app(), get("/next/approvals/count", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{count}");

    let (status, _, count) = send_text(
        fixture.app(),
        get_with_cookie("/next/approvals/count", &cookie),
    )
    .await;
    let count: Value = serde_json::from_str(&count).expect("a JSON count");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count["pending"], json!(0));

    let (status, _, js) = send_text(fixture.app(), get("/assets/approvals.js", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(js.contains("/next/approvals/count"), "{js}");
}

/// The vitals page keeps its own trend: one point per visit, in the console's memory, and it says
/// so rather than implying a sampler runs behind it.
#[tokio::test]
async fn the_vitals_page_keeps_a_trend_from_its_own_visits() {
    let fixture = Fixture::new("vitals-trend");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd1)).await;
    let cookie = page_cookie(&fixture).await;
    let facts = json!({
        "machine": "BOX",
        "os": { "name": "Windows", "version": "11" },
        "cpu": { "cores": 8 },
        "uptime_s": 120,
        "memory": { "total_bytes": 8_000_000_000u64, "available_bytes": 4_000_000_000u64 },
        "disks": [{ "name": "C:\\", "total_bytes": 1_000u64, "free_bytes": 400u64 }],
        "update": { "current": { "version": "0.3.1" } }
    });

    let first = visit_vitals(&fixture, &cookie, &device_id, &mut device, &facts).await;
    assert!(first.contains("One point so far"), "{first}");

    let second = visit_vitals(&fixture, &cookie, &device_id, &mut device, &facts).await;
    assert!(
        second.contains("class=\"spark\""),
        "the second visit draws the line: {second}"
    );
    assert!(second.contains("2 samples"), "{second}");
    assert!(second.contains("Not a sampler"), "{second}");
}

/// A destructive call a token did not consent to is held for the operator, and the request stays
/// open while they decide. Approving re-sends it with consent for THIS call only.
#[tokio::test]
async fn a_destructive_token_call_is_held_and_runs_when_approved() {
    let fixture = Fixture::new("approval-approved");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd3)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, call.clone()),
    );

    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], call, "{frame}");
        answer_error(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 31, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
        )
        .await;

        // The approved re-send: same call, consenting token.
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], call, "{frame}");
        assert_eq!(
            frame["token"]["allow_destructive"],
            json!(true),
            "the console consents for this call: {frame}"
        );
        answer(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 31, "result": { "content": [{ "type": "text", "text": "{\"id\":\"session-1\"}" }], "isError": false } }),
        )
        .await;
    };

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let operator = async {
        let id = wait_for_approval(&fixture, &cookie).await;
        // **THE CARD NAMES THE CREDENTIAL.** "Approve" answers one client's call, and two clients
        // can hold the same tool on the same machine; the audit always named the token and the
        // decision surface did not.
        let (_, _, dashboard) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
        assert!(
            dashboard.contains("asked by") && dashboard.contains("cautious"),
            "the held call names the token that asked: {dashboard}"
        );
        assert!(
            dashboard.contains(&format!(
                "token {}",
                created["id"].as_str().expect("a token id")
            )),
            "the name carries the id a record can be matched to: {dashboard}"
        );
        let (_, _, count) = send_text(
            fixture.app(),
            get_with_cookie("/next/approvals/count", &cookie),
        )
        .await;
        let count: Value = serde_json::from_str(&count).expect("a JSON count");
        assert_eq!(count["pending"], json!(1), "the badge counts the held call");
        let (status, _, _) = send_text(
            fixture.app(),
            post_form_cookie(
                &format!("/next/approvals/{id}/approve"),
                &[("csrf", &csrf)],
                &cookie,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        let (_, _, count) = send_text(
            fixture.app(),
            get_with_cookie("/next/approvals/count", &cookie),
        )
        .await;
        let count: Value = serde_json::from_str(&count).expect("a JSON count");
        assert_eq!(count["pending"], json!(0), "answered calls stop counting");
        id
    };

    let ((status, body), (), id) = tokio::join!(http, device_side, operator);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["result"]["content"][0]["text"],
        json!("{\"id\":\"session-1\"}")
    );

    // A second press on the same question is not reported as delivered: the call is gone.
    let (status, headers, _) = send_text(
        fixture.app(),
        post_form_cookie(
            &format!("/next/approvals/{id}/approve"),
            &[("csrf", &csrf)],
            &cookie,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a redirect");
    assert!(
        location.contains("no%20longer%20waiting"),
        "the second press says the call is gone: {location}"
    );
}

/// An approval is consent for a credential that exists NOW: the ninety seconds the operator had to
/// decide is long enough for a token to be revoked, and the device cannot know about a revocation
/// (it can only check an expiry). The approved call is refused, not re-sent.
#[tokio::test]
async fn an_approved_call_whose_token_was_revoked_is_refused() {
    let fixture = Fixture::new("approval-token-revoked");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd9)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let token_id = created["id"]
        .as_str()
        .or_else(|| created["token_meta"]["id"].as_str())
        .expect("the created token's id")
        .to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, call.clone()),
    );

    let device_side = async {
        let frame = next_json(&mut device).await;
        answer_error(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 41, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
        )
        .await;
        // Nothing is re-sent: the token is gone by the time the operator approves.
        let silent = tokio::time::timeout(
            std::time::Duration::from_millis(400),
            next_json(&mut device),
        )
        .await;
        assert!(
            silent.is_err(),
            "a revoked token's approval must not reach the device"
        );
    };

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let operator = async {
        let id = wait_for_approval(&fixture, &cookie).await;
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/tokens/{token_id}"))
            .header(header::AUTHORIZATION, format!("Bearer {session}"))
            .body(Body::empty())
            .expect("the request builds");
        let (status, _) = send(fixture.app(), request).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "the token is revoked");
        let (status, _, _) = send_text(
            fixture.app(),
            post_form_cookie(
                &format!("/next/approvals/{id}/approve"),
                &[("csrf", &csrf)],
                &cookie,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
    };

    let ((status, body), (), ()) = tokio::join!(http, device_side, operator);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["code"],
        json!(-32001),
        "the approval is refused with its own sentence: {body}"
    );
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("no longer valid")),
        "{body}"
    );
}

/// Refusing returns the device's own sentence: the call did not run.
#[tokio::test]
async fn a_refused_approval_returns_the_device_sentence() {
    let fixture = Fixture::new("approval-refused");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd2)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(
            &session,
            &device_id,
            "cautious",
            &["terminal.create"],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, call.clone()),
    );

    let device_side = async {
        let frame = next_json(&mut device).await;
        answer_error(
            &mut device,
            &frame,
            json!({ "jsonrpc": "2.0", "id": 32, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
        )
        .await;
    };

    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let operator = async {
        let id = wait_for_approval(&fixture, &cookie).await;
        let (status, _, _) = send_text(
            fixture.app(),
            post_form_cookie(
                &format!("/next/approvals/{id}/refuse"),
                &[("csrf", &csrf)],
                &cookie,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
    };

    let ((status, body), (), ()) = tokio::join!(http, device_side, operator);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32002), "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("destructive")),
        "{body}"
    );
}

/// The id of the first held call the dashboard lists, read off its approve form's action.
fn first_approval_id(body: &str) -> Option<String> {
    let start = body.find("/next/approvals/")? + "/next/approvals/".len();
    let rest = &body[start..];
    let end = rest.find('/')?;
    Some(rest[..end].to_owned())
}

/// Wait for the dashboard to list a held call, then answer with its id.
async fn wait_for_approval(fixture: &Fixture, cookie: &str) -> String {
    for _ in 0..200 {
        let (_, _, body) = send_text(fixture.app(), get_with_cookie("/next", cookie)).await;
        if let Some(id) = first_approval_id(&body) {
            return id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the dashboard never listed a held call");
}

/// A refused Access token leaves a line. "Was the edge's header there, and did the console accept
/// it?" must be answerable from the audit, not only from the card the browser shows.
#[tokio::test]
async fn a_refused_access_token_is_audited() {
    let fixture = Fixture::with_access("access-refusal-audited");
    let mut request = get("/next", None);
    request.headers_mut().insert(
        "cf-access-jwt-assertion",
        "not-a-jwt".parse().expect("a header value"),
    );
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);

    let lines = audit_lines(&fixture);
    let refused = lines
        .iter()
        .find(|line| line["kind"] == "auth" && line["status"] == 401)
        .unwrap_or_else(|| panic!("a refusal line: {lines:#?}"));
    assert_eq!(refused["detail"], "access refused");
}

/// A refused operator token leaves one too, and it never carries what was typed.
#[tokio::test]
async fn a_refused_operator_token_is_audited() {
    let fixture = Fixture::new("token-refusal-audited");
    let wrong = "0".repeat(64);
    let (status, _, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &wrong)])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let lines = audit_lines(&fixture);
    let refused = lines
        .iter()
        .find(|line| line["kind"] == "auth" && line["status"] == 401)
        .unwrap_or_else(|| panic!("a refusal line: {lines:#?}"));
    assert_eq!(refused["detail"], "token refused");
    assert!(
        !lines.iter().any(|line| line.to_string().contains(&wrong)),
        "the attempt is not what is recorded"
    );
}

/// A history row can be opened: `runs.get` answers the row, which is all the device keeps.
#[tokio::test]
async fn a_run_history_row_can_be_opened() {
    let fixture = Fixture::new("runs-get");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd8)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/runs/{device_id}?run=run-7"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("runs.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!([
                { "id": "run-7", "command": "pwsh", "args": ["-c", "1"], "cwd": "C:\\work",
                  "exit_code": 0, "timed_out": false, "duration_ms": 12,
                  "started_at_ms": 1793057500000u64, "stdout_bytes": 5, "stderr_bytes": 0 }
            ]).to_string() }], "isError": false }
        }),
    )
    .await;

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"]["params"]["name"], json!("runs.get"), "{frame}");
    assert_eq!(frame["mcp"]["params"]["arguments"]["id"], json!("run-7"));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({
                "id": "run-7", "command": "pwsh", "args": ["-c", "1"], "cwd": "C:\\work",
                "exit_code": 0, "timed_out": false, "duration_ms": 12,
                "started_at_ms": 1793057500000u64, "stdout_bytes": 5, "stderr_bytes": 0
            }).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("?run=run-7"),
        "the row links to itself: {body}"
    );
    assert!(body.contains("run-7"), "{body}");
    assert!(
        body.contains("C:\\work"),
        "the row's working directory: {body}"
    );
    assert!(
        body.contains("5 bytes"),
        "how much stdout the run produced: {body}"
    );
}

/// The memory page can search the text (not only filter a tag), which is what `memory.search` is for.
#[tokio::test]
async fn the_memory_page_searches_the_text() {
    let fixture = Fixture::new("memory-search");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdb)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/memory/{device_id}?q=needle"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.search"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["query"],
        json!("needle")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!([
                { "id": "entry-1", "text": "a needle in the store", "tags": ["probe"] }
            ]).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("a needle in the store"), "{body}");
}

/// An entry can be rewritten in place; the form is filled from the entry the list already returned.
#[tokio::test]
async fn the_memory_page_can_edit_an_entry() {
    let fixture = Fixture::new("memory-edit");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xda)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/memory/{device_id}?edit=entry-1"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!([
                { "id": "entry-1", "text": "the old fact", "tags": ["a", "b"] }
            ]).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Edit an entry"), "{body}");
    assert!(body.contains("the old fact"), "{body}");
    assert!(
        body.contains(&format!("/next/memory/{device_id}/update")),
        "{body}"
    );

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/memory/{device_id}/update"),
        &[
            ("csrf", &csrf),
            ("id", "entry-1"),
            ("text", "the new fact"),
            ("tags", "a, b"),
        ],
        &cookie,
    );
    let update = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.update"),
        "{frame}"
    );
    assert_eq!(frame["mcp"]["params"]["arguments"]["id"], json!("entry-1"));
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["text"],
        json!("the new fact")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["tags"],
        json!(["a", "b"]),
        "tags are the complete new list, split from the field"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "{}" }], "isError": false }
        }),
    )
    .await;
    let (status, _, _) = update.await.expect("the update finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
}

/// The whole store can be taken away as JSONL, which is what `memory.export` is for.
#[tokio::test]
async fn the_memory_page_can_export_the_store() {
    let fixture = Fixture::new("memory-export");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xd9)).await;
    let cookie = page_cookie(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/memory/{device_id}/export"), &cookie);
    let export = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("memory.export"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({
                // The device answers an envelope; the download is the JSONL inside it.
                "text": "{\"id\":\"1\",\"text\":\"kept\"}\n",
                "entries": 1,
                "truncated": true,
                "total_bytes": 27
            }).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, headers, body) = export.await.expect("the export finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("kept"), "{body}");
    assert!(
        !body.contains("truncated") && !body.contains("entries"),
        "the envelope is not the file: {body}"
    );
    assert!(
        headers
            .get("x-memory-truncated")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == "true"),
        "a truncated export says so in a header, since JSONL has nowhere else"
    );
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("json")),
        "the response names what it is"
    );
    assert!(
        headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("attachment")),
        "it downloads rather than renders"
    );
}

/// Navigation and shutdown were the two browser tools without a form; both now reach the device.
#[tokio::test]
async fn the_browser_page_can_navigate_and_shut_the_browser_down() {
    let fixture = Fixture::new("browser-navigate-shutdown");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdc)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/browser/{device_id}/navigate"),
        &[
            ("csrf", &csrf),
            ("target", "target-1"),
            ("url", "https://example.org/next"),
        ],
        &cookie,
    );
    let navigate = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.navigate"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["target"],
        json!("target-1")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["url"],
        json!("https://example.org/next")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "{}" }], "isError": false }
        }),
    )
    .await;
    let (status, _, _) = navigate.await.expect("the navigate finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/browser/{device_id}/shutdown"),
        &[("csrf", &csrf)],
        &cookie,
    );
    let shutdown = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.shutdown"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": "{\"stopped\":true}" }], "isError": false }
        }),
    )
    .await;
    let (status, _, _) = shutdown.await.expect("the shutdown finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
}

/// An expression runs in the device's target and its answer is on the page that follows; the console
/// keeps it in memory rather than in the URL.
#[tokio::test]
async fn the_browser_page_can_evaluate_and_shows_the_answer() {
    let fixture = Fixture::new("browser-eval");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xdd)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/browser/{device_id}/eval"),
        &[
            ("csrf", &csrf),
            ("target", "target-1"),
            ("expression", "document.title"),
        ],
        &cookie,
    );
    let eval = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.eval"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["target"],
        json!("target-1")
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["expression"],
        json!("document.title")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "type": "string", "value": "Example" }).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, _, _) = eval.await.expect("the eval finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);

    // The page that follows shows the expression and the answer.
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/browser/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!([
                { "id": "target-1", "title": "example", "url": "https://example.com/", "type": "page" }
            ]).to_string() }], "isError": false }
        }),
    )
    .await;
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.screenshot"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string() }], "isError": false }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the browser page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Last evaluation"), "{body}");
    assert!(body.contains("document.title"), "{body}");
    assert!(body.contains("Example"), "{body}");
    assert!(
        body.contains(&format!("/next/browser/{device_id}/eval")),
        "the form is there to run another: {body}"
    );
}

#[tokio::test]
async fn the_browser_page_lists_targets_captures_the_first_and_forwards_an_open() {
    let fixture = Fixture::new("browser-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xef)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/browser/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "target-1", "title": "example", "url": "https://example.com/", "type": "page" }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.screenshot"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["target"],
        json!("target-1")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "format": "png", "data": "QUJD", "bytes": 3 }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the browser page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("https://example.com/"), "{body}");
    assert!(
        body.contains("src=\"data:image/png;base64,QUJD\""),
        "{body}"
    );

    let mut request = post_form(
        &format!("/next/browser/{device_id}/open"),
        &[("csrf", &csrf), ("url", "https://example.org/")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let open = tokio::spawn(async move { send_text(fixture.app(), request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("browser.open"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["url"],
        json!("https://example.org/")
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({ "target": "target-2", "url": "https://example.org/" }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, headers, _) = open.await.expect("the open finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/browser/{device_id}").as_str())
    );
}

#[tokio::test]
async fn a_terminal_create_can_name_an_ssh_or_a_serial_target() {
    let fixture = Fixture::new("terminal-kinds");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xf0)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    // SSH: the page carries the password into the device's auth envelope and never into the URL.
    let mut request = post_json_cookie(
        &format!("/next/terminal/{device_id}/create"),
        json!({
            "kind": "ssh", "host": "10.0.0.5", "ssh_port": 2222, "user": "root",
            "password": "hunter2", "rows": 30, "cols": 90
        }),
        &cookie,
        Some(&csrf),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("a header"),
    );
    let app = fixture.app();
    let create = tokio::spawn(async move { send_with_headers(app, request).await });
    let frame = next_json(&mut device).await;
    let arguments = &frame["mcp"]["params"]["arguments"];
    assert_eq!(arguments["kind"], json!("ssh"), "{frame}");
    assert_eq!(arguments["host"], json!("10.0.0.5"));
    assert_eq!(arguments["port"], json!(2222));
    assert_eq!(arguments["user"], json!("root"));
    assert_eq!(arguments["auth"]["kind"], json!("password"));
    assert_eq!(arguments["auth"]["password"], json!("hunter2"));
    assert_eq!(arguments["rows"], json!(30));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "id": "session-9", "kind": "ssh", "shell": "root@10.0.0.5" }).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, _, body) = create.await.expect("the ssh create finishes");
    assert_eq!(status, StatusCode::OK, "{body}");

    // Serial: the port path and the baud, and no host or user.
    let mut request = post_json_cookie(
        &format!("/next/terminal/{device_id}/create"),
        json!({ "kind": "serial", "serial_port": "COM3", "baud": 9600 }),
        &cookie,
        Some(&csrf),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("a header"),
    );
    let app = fixture.app();
    let create = tokio::spawn(async move { send_with_headers(app, request).await });
    let frame = next_json(&mut device).await;
    let arguments = &frame["mcp"]["params"]["arguments"];
    assert_eq!(arguments["kind"], json!("serial"), "{frame}");
    assert_eq!(arguments["port"], json!("COM3"));
    assert_eq!(arguments["baud"], json!(9600));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "id": "session-10", "kind": "serial", "shell": "COM3 9600 8N1" }).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, _, body) = create.await.expect("the serial create finishes");
    assert_eq!(status, StatusCode::OK, "{body}");

    // SSH by key: the page names the key on the device and sends no password. A key path is
    // configuration, not a credential, so the preset and the form may hold it.
    let mut request = post_json_cookie(
        &format!("/next/terminal/{device_id}/create"),
        json!({
            "kind": "ssh", "host": "10.0.0.5", "ssh_port": 22, "user": "root",
            "key_path": "C:\\Users\\operator\\.ssh\\id_ed25519", "rows": 24, "cols": 80
        }),
        &cookie,
        Some(&csrf),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("a header"),
    );
    let app = fixture.app();
    let create = tokio::spawn(async move { send_with_headers(app, request).await });
    let frame = next_json(&mut device).await;
    let arguments = &frame["mcp"]["params"]["arguments"];
    assert_eq!(arguments["auth"]["kind"], json!("key"), "{frame}");
    assert_eq!(
        arguments["auth"]["path"],
        json!("C:\\Users\\operator\\.ssh\\id_ed25519"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "content": [{ "type": "text", "text": json!({ "id": "session-11", "kind": "ssh", "shell": "root@10.0.0.5" }).to_string() }], "isError": false }
        }),
    )
    .await;
    let (status, _, body) = create.await.expect("the key create finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// An ssh session needs one of the two: a password typed on the page, or a key path the device can
/// read. Saying so beats a device-side refusal with a worse sentence.
#[tokio::test]
async fn an_ssh_create_without_a_password_or_a_key_is_refused_by_the_page() {
    let fixture = Fixture::new("terminal-ssh-no-auth");
    let (device_id, _) = fixture.register(&signing_key(0xee)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let mut request = post_json_cookie(
        &format!("/next/terminal/{device_id}/create"),
        json!({ "kind": "ssh", "host": "10.0.0.5", "user": "root" }),
        &cookie,
        Some(&csrf),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("a header"),
    );
    let (status, _, body) = send_with_headers(fixture.app(), request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|sentence| sentence.contains("key path")),
        "{body}"
    );
}

/// The page offers the key field; without it the device's public-key auth was unreachable from the
/// console, which said so in the one place an operator would look for it.
#[tokio::test]
async fn the_terminal_page_offers_a_key_file() {
    let fixture = Fixture::new("terminal-key-field");
    let (device_id, _) = fixture.register(&signing_key(0xed)).await;
    let cookie = page_cookie(&fixture).await;

    let (status, _, page) = send_text(
        fixture.app(),
        get_with_cookie(&format!("/next/terminal/{device_id}"), &cookie),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("id=\"key_path\""), "{page}");
}

#[tokio::test]
async fn the_connect_page_mints_a_token_once_lists_it_and_revokes_it() {
    let fixture = Fixture::new("connect-page");
    let (device_id, _token) = fixture.register(&signing_key(0xf1)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let page_uri = format!("/next/tokens/{device_id}");
    let (status, _, body) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Mint a token"), "{body}");
    assert!(body.contains("No token names this device yet"), "{body}");

    // Mint: the page shows the secret once, along with the client configuration for this device.
    let mut request = post_form(
        &format!("{page_uri}/mint"),
        &[("csrf", &csrf), ("name", "dsh-aggregate")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, created) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(created.contains("srm_"), "{created}");
    assert!(
        created.contains(&format!("/mcp/{device_id}")),
        "the client config names this device's endpoint: {created}"
    );

    // The list reports it, and carries the id the revoke form posts.
    let (_, _, listed) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(listed.contains("dsh-aggregate"), "{listed}");
    let marker = "name=\"id\" value=\"";
    let start = listed.find(marker).expect("a revoke form") + marker.len();
    let end = start + listed[start..].find('"').expect("the id ends");
    let id = listed[start..end].to_owned();

    let mut request = post_form(
        &format!("{page_uri}/revoke"),
        &[("csrf", &csrf), ("id", &id)],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, after) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(!after.contains("dsh-aggregate"), "{after}");
    assert!(after.contains("No token names this device yet"), "{after}");
}

/// The consent is the operator's, not a default: a mint without the box ticked must produce a token
/// whose destructive calls the device refuses, and the list must say so.
#[tokio::test]
async fn a_minted_token_is_only_destructive_when_asked_for() {
    let fixture = Fixture::new("token-consent");
    let (device_id, _) = fixture.register(&signing_key(0xfa)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;
    let page_uri = format!("/next/tokens/{device_id}");

    let mut request = post_form(
        &format!("{page_uri}/mint"),
        &[("csrf", &csrf), ("name", "plain")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);

    let (_, _, listed) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(listed.contains("plain"), "{listed}");
    assert!(
        !listed.contains("destructive allowed"),
        "a plain mint has no destructive consent: {listed}"
    );

    let mut request = post_form(
        &format!("{page_uri}/mint"),
        &[
            ("csrf", &csrf),
            ("name", "armed"),
            ("allow_destructive", "on"),
        ],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);

    let (_, _, listed) = send_text(fixture.app(), get_with_cookie(&page_uri, &cookie)).await;
    assert!(listed.contains("armed"), "{listed}");
    assert!(
        listed.contains("destructive allowed"),
        "the ticked box is recorded on the row: {listed}"
    );
    assert!(
        listed.contains("holds the question for ninety seconds"),
        "the page says where the ninety-second hold lives — this page, not the device: {listed}"
    );
    assert!(
        listed.contains("refused by the device"),
        "the device refuses without consent; the page only holds the question: {listed}"
    );
}

#[tokio::test]
async fn a_token_mint_without_csrf_is_refused() {
    let fixture = Fixture::new("connect-csrf");
    let (device_id, _token) = fixture.register(&signing_key(0xf2)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;
    let mut request = post_form(
        &format!("/next/tokens/{device_id}/mint"),
        &[("csrf", &"00".repeat(32)), ("name", "sneaky")],
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_vitals_page_draws_one_sample_of_the_devices_own_numbers() {
    let fixture = Fixture::new("vitals-page");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xf3)).await;
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/vitals/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("host.facts"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!({
                    "machine": "DESKTOP-TEST",
                    "os": { "name": "Windows", "version": "10.0.22631" },
                    "cpu": { "cores": 12 },
                    "uptime_s": 7200,
                    "memory": { "total_bytes": 8u64 * 1024 * 1024 * 1024, "available_bytes": 4u64 * 1024 * 1024 * 1024 },
                    "disks": [{ "name": "C:\\", "total_bytes": 100u64 * 1024 * 1024 * 1024, "free_bytes": 25u64 * 1024 * 1024 * 1024 }],
                    "update": { "current": { "version": "0.3.65", "channel": "canary" } }
                }).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, body) = page.await.expect("the vitals page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("DESKTOP-TEST"), "{body}");
    assert!(body.contains("4.0 GiB used of 8.0 GiB"), "{body}");
    assert!(body.contains("C:\\"), "{body}");
    assert!(body.contains("0.3.65 (canary)"), "{body}");
    assert!(body.contains("width:50%"), "the memory bar is half: {body}");
}

#[tokio::test]
async fn the_conventional_favicon_path_answers_with_the_icon() {
    // A browser asks for /favicon.ico on every page it opens; a 404 there is a console error on
    // every page that does not declare an icon.
    let fixture = Fixture::new("favicon");
    let response = fixture
        .app()
        .oneshot(get("/favicon.ico", None))
        .await
        .expect("the icon is served");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
}

#[tokio::test]
async fn a_page_session_survives_a_console_restart() {
    // Every deploy used to sign the operator out, because sessions were memory only. A session is
    // written to `<data>/sessions.json` now, and the token itself never is.
    let fixture = Fixture::new("session-restart");
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let restarted = fixture.restart();
    let (status, _, body) = send_text(restarted.router(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the restarted console accepts the cookie"
    );
    assert!(body.contains("Devices"), "{body}");

    // The CSRF value is part of the session too, so a form rendered before the restart still works.
    let (status, _, _) = send_text(
        restarted.router(),
        post_form_cookie("/next/enroll", &[("csrf", &csrf)], &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    // The session file holds hashes, not tokens.
    let file =
        std::fs::read_to_string(fixture.config.data.join("sessions.json")).expect("the file");
    assert!(
        !file.contains(&cookie["summrise_session=".len()..]),
        "{file}"
    );
    assert!(file.contains("token_hash"), "{file}");
}

#[tokio::test]
async fn a_page_session_expires_after_its_lifetime() {
    let fixture = Fixture::new("session-expiry");
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    // One minute short of the month: the session still works.
    fixture.advance_ms(30 * 24 * 60 * 60 * 1000 - 60_000);
    let (status, _, _) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);

    // A minute past it: the page is the login card again.
    fixture.advance_ms(120_000);
    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in"), "{body}");
}

/// A session another browser left behind is visible in the rail; ending it there must close it on
/// the device, not merely hide the row.
#[tokio::test]
async fn ending_a_session_from_the_rail_closes_it_on_the_device() {
    let fixture = Fixture::new("terminal-rail-end");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe2)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    // The page's own request, answered with one live session.
    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/terminal/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });
    let frame = next_json(&mut device).await;
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "session-9", "kind": "pty", "started_at_ms": 1_791_000_000_000u64 }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;
    let (status, _, _) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);

    // The rail's form ends the session.
    let app = fixture.app();
    let request = post_form_cookie(
        &format!("/next/terminal/{device_id}/session-9/end"),
        &[("csrf", &csrf)],
        &cookie,
    );
    let end = tokio::spawn(async move { send_text(app, request).await });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("terminal.close"),
        "{frame}"
    );
    assert_eq!(
        frame["mcp"]["params"]["arguments"]["id"],
        json!("session-9"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{ "type": "text", "text": "{\"ok\":true}" }], "isError": false }
        }),
    )
    .await;

    let (status, headers, _) = end.await.expect("the end request finishes");
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/next/terminal/{device_id}").as_str()),
        "the page comes back after the session is closed"
    );
}

#[tokio::test]
async fn the_terminal_page_lists_the_devices_live_sessions() {
    let fixture = Fixture::new("terminal-rail");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xe1)).await;
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    let app = fixture.app();
    let request = get_with_cookie(&format!("/next/terminal/{device_id}"), &cookie);
    let page = tokio::spawn(async move { send_text(app, request).await });

    // The page asks the device for its sessions before it renders the rail.
    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("terminal.list"),
        "{frame}"
    );
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{ "type": "text", "text": json!([
                    { "id": "session-3", "kind": "pty", "started_at_ms": 1_791_000_000_000u64 },
                    { "id": "session-7", "kind": "ssh", "started_at_ms": 1_791_000_500_000u64 }
                ]).to_string() }],
                "isError": false
            }
        }),
    )
    .await;

    let (status, _, body) = page.await.expect("the page finishes");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("class=\"context-rail\""), "{body}");
    assert!(body.contains("?attach=session-3"), "{body}");
    assert!(body.contains("?attach=session-7"), "{body}");
    assert!(
        body.contains(&format!("/next/terminal/{device_id}/session-3/end")),
        "a session in the rail can be ended without opening it: {body}"
    );
    assert!(
        body.contains(&csrf),
        "the page still carries its CSRF value: {body}"
    );
}

#[tokio::test]
async fn the_fleet_shows_the_console_s_own_recent_activity() {
    let fixture = Fixture::new("fleet-activity");
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    // A device action lands in the audit log; the fleet page reads the same file `/audit` serves.
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie("/next/enroll", &[("csrf", &csrf)], &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Recent activity"), "the card is on the page");
    assert!(
        body.contains("enroll"),
        "the enrollment is one of the lines: {body}"
    );
}

#[tokio::test]
async fn the_audit_page_narrows_to_the_lines_the_filter_names() {
    let fixture = Fixture::new("audit-filter");
    let (cookie, csrf) = sign_in_to_the_page(&fixture).await;

    // One enrollment is one audit line, and it is the only line this console has made.
    let (status, _, _) = send_text(
        fixture.app(),
        post_form_cookie("/next/enroll", &[("csrf", &csrf)], &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    // No filter: the form and the line.
    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next/audit", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("id=\"f-kind\""),
        "the filter bar is on the page: {body}"
    );
    assert!(body.contains("enroll"), "{body}");

    // A kind that is not in the log: the empty state, not the same table.
    let (status, _, body) = send_text(
        fixture.app(),
        get_with_cookie("/next/audit?kind=tool", &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No line matches this filter"), "{body}");

    // The tool filter reads the detail too, so a narrowed view is never a dead end.
    let (status, _, body) = send_text(
        fixture.app(),
        get_with_cookie("/next/audit?kind=enroll", &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("enroll"), "{body}");
}

/// The filter offers every kind the console writes, so a line it can record is a line it can select.
#[tokio::test]
async fn the_audit_filter_offers_the_new_kinds() {
    let fixture = Fixture::new("audit-filter-kinds");
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    let approval = json!({
        "ts_ms": 1_793_060_000_000u64,
        "kind": "approval",
        "status": 200,
        "tool": "terminal.create",
        "detail": "asked"
    });
    let enroll = json!({
        "ts_ms": 1_793_059_000_000u64,
        "kind": "enroll",
        "status": 200,
        "tool": "memory.list",
        "detail": "page"
    });
    let path = fixture.config.data.join("audit.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(&format!("{approval}\n{enroll}\n"));
    std::fs::write(&path, text).expect("the audit line is written");

    let (status, _, body) = send_text(
        fixture.app(),
        get_with_cookie("/next/audit?kind=approval", &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("option value=\"approval\" selected"),
        "the filter can name it: {body}"
    );
    assert!(body.contains("terminal.create"), "{body}");
    assert!(
        !body.contains("memory.list"),
        "only the named kind's line: {body}"
    );
}

#[tokio::test]
async fn the_denials_page_narrows_with_the_same_filter() {
    let fixture = Fixture::new("denials-filter");
    let (cookie, _csrf) = sign_in_to_the_page(&fixture).await;

    // A request with no session is a denial line; one with a bad token is another.
    let (status, _, _) = send_text(
        fixture.app(),
        get_with_cookie("/next/audit", "summrise_session=not-a-cookie"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) =
        send_text(fixture.app(), get_with_cookie("/next/denials", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    // **ONE KIND, SO NO KIND FILTER.** The audit's twelve-option list used to appear here too,
    // offering eleven choices that cannot match a store whose every row is `denied`; the page
    // says what it is instead. The test that pinned the old selector is what let the lie ship.
    assert!(
        !body.contains("id=\"f-kind\""),
        "denials has one kind, so it must not offer a kind filter: {body}"
    );
    assert!(
        body.contains("refused before authentication"),
        "the page says which log it is: {body}"
    );
    assert!(
        body.contains("action=\"/next/denials\""),
        "the form posts back to the denials page: {body}"
    );
    assert!(
        !body.contains("id=\"f-device\""),
        "a denial has no device to filter by: {body}"
    );

    // The audit keeps the full kind list: it is the log with many kinds.
    let (status, _, audit) =
        send_text(fixture.app(), get_with_cookie("/next/audit", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        audit.contains("id=\"f-kind\""),
        "the audit still filters by kind: {audit}"
    );

    // A status class that cannot be in the log gives the empty state.
    let (status, _, body) = send_text(
        fixture.app(),
        get_with_cookie("/next/denials?status=5", &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No line matches this filter"), "{body}");
}
