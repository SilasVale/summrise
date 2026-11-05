//! The console's HTTP surface, driven in-process: no socket is bound for the HTTP routes and every
//! temporary file lives under this test binary's own directory. The device channel is a real
//! WebSocket, so those tests bind an ephemeral loopback listener and drive it with a fake device.

use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::ConnectInfo;
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
            trusted_proxy: None,
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

    /// A console that believes one reverse proxy's `X-Forwarded-User`: the tailnet deployment.
    fn with_trusted_proxy(label: &str, proxy: Ipv4Addr) -> Self {
        let mut fixture = Self::new(label);
        let mut config = fixture.config.clone();
        config.trusted_proxy = Some(proxy.into());
        let reader = fixture.clock.clone();
        fixture.console = Console::load_with_clock(&config, move || reader.load(Ordering::Relaxed))
            .expect("the console reloads with the trusted proxy");
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

/// The connection's peer address, exactly as axum's connect-info service inserts it before routing.
fn from_peer(request: Request<Body>, peer: Ipv4Addr) -> Request<Body> {
    let mut request = request;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((peer, 40_000))));
    request
}

/// The header a configured reverse proxy sets after it authenticated the operator.
fn forwarded_user(request: Request<Body>, login: &str) -> Request<Body> {
    let (mut parts, body) = request.into_parts();
    parts.headers.insert(
        "x-forwarded-user",
        login.parse().expect("the login is a header value"),
    );
    Request::from_parts(parts, body)
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
        trusted_proxy: None,
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
    assert_eq!(status, StatusCode::OK);
    // The console cookie is ours; without Access there is no edge session to end, and the answer
    // says so instead of inventing a URL.
    assert!(body["access_logout"].is_null(), "{body}");

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
    let (_device_id, _token, _device) = present_device(&fixture, &server, &signing_key(0x7b)).await;

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
    let (_device_id, _token, device) = present_device(&fixture, &server, &signing_key(0x7a)).await;

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

/// **A SOCKET IS NOT PRESENCE.** The live finding (2026-11-05): DESKTOP-14RJCR8 was offline on the
/// tailnet for 7+ minutes (`tailscale status` said "offline, last seen 7m ago") while `/fleet`
/// served `"online": true`, because the console asked whether a channel existed instead of whether
/// the device had said anything on it. A half-open socket to a sleeping machine kept claiming it was
/// there. This is the same shape with the clock under our hand: a channel that opened and sent no
/// frame is offline, and the console omits a moment it never observed.
#[tokio::test]
async fn a_channel_that_has_sent_no_frame_is_not_online() {
    let fixture = Fixture::new("presence-no-frame");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0x61)).await;
    let session = fixture.authenticate().await;
    let mut device = open_channel(&server, &device_id, &token)
        .await
        .expect("the device channel");

    // The upgrade callback registers the channel asynchronously. Wait for its join boundary rather
    // than guessing with a sleep: once the line is on disk, the socket exists and only a frame is
    // missing.
    wait_until_joined(&fixture, 1).await;
    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["online"],
        json!(false),
        "an open socket with no frame is not presence: {listed}"
    );
    assert!(
        listed.get("last_frame_ms").is_none(),
        "the console has never seen a frame, so it has no moment to serve: {listed}"
    );

    device
        .send(WsMessage::Text(
            json!({ "host": { "machine": "workshop" } }).to_string(),
        ))
        .await
        .expect("the host frame is sent");

    let listed = wait_until_online(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["last_frame_ms"],
        json!(START_MS),
        "the frame is the moment the console saw: {listed}"
    );
}

/// **SILENCE IS OFFLINE, AND THE MOMENT IS NOT ROUNDED AWAY.** A device that reported and then
/// stopped (the sleeping machine behind a half-open TCP connection) keeps its place in the channel
/// map. The row says offline after the presence window, keeps the exact moment it was last heard,
/// and does not explain silence as a reconnect: `last_attempt` describes a real reconnect and this
/// channel never had one.
#[tokio::test]
async fn a_silent_channel_is_offline_and_keeps_the_moment_it_was_last_heard() {
    let fixture = Fixture::new("presence-silent");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        present_device(&fixture, &server, &signing_key(0x62)).await;
    let session = fixture.authenticate().await;

    let heard = fleet_row(fixture.app(), &session, &device_id).await;
    assert_eq!(heard["online"], json!(true), "{heard}");
    let heard_at = heard["last_frame_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("a frame was seen, so the moment is served: {heard}"));

    // Seven minutes is the measured live silence; this test uses 200 s because it is past the
    // window and on the injected clock, so the numbers are deterministic.
    fixture.advance_ms(200_000);
    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["online"],
        json!(false),
        "a channel silent past the window is offline: {listed}"
    );
    assert_eq!(
        listed["last_frame_ms"],
        json!(heard_at),
        "the moment is unchanged, not moved to now: {listed}"
    );
    assert!(
        listed.get("last_attempt").is_none(),
        "silence is not a reconnect, so the attempt line has nothing to say: {listed}"
    );
    assert!(
        listed.get("last_outage").is_none(),
        "a silent channel has not closed, so there is no outage: {listed}"
    );

    let (_, health) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(health["online"], json!(0), "{health}");

    // A frame on the SAME socket brings it back without a reconnect.
    device
        .send(WsMessage::Text(
            json!({ "host": { "machine": "workshop" } }).to_string(),
        ))
        .await
        .expect("the host frame is sent");
    let listed = wait_until_online(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["last_frame_ms"],
        json!(START_MS + 200_000),
        "the new frame moves the moment forward: {listed}"
    );
}

/// **A REOPENED CHANNEL HAS AN EMPTY PRESENCE CLOCK.** The old socket's frames are not evidence for
/// the new one: a device can open a channel and go silent before its first report. The console still
/// remembers when it last heard the device, because that moment happened and is not a claim about
/// the new socket; `online` alone is decided by frames on the channel that is live now.
#[tokio::test]
async fn a_reopened_channel_is_offline_until_it_sends_a_frame() {
    let fixture = Fixture::new("presence-reopened");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token, device) = present_device(&fixture, &server, &signing_key(0x63)).await;
    let session = fixture.authenticate().await;
    let heard = fleet_row(fixture.app(), &session, &device_id).await;
    let heard_at = heard["last_frame_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("a frame was seen, so the moment is served: {heard}"));

    drop(device);
    wait_until_offline(fixture.app(), &session, &device_id).await;

    let mut again = open_channel(&server, &device_id, &token)
        .await
        .expect("the device reconnects");
    wait_until_joined(&fixture, 2).await;

    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["online"],
        json!(false),
        "the replacement socket has proved nothing yet: {listed}"
    );
    assert_eq!(
        listed["last_frame_ms"],
        json!(heard_at),
        "the last frame the console saw is a fact about the past, not about the new socket: {listed}"
    );

    fixture.advance_ms(1_000);
    again
        .send(WsMessage::Text(
            json!({ "host": { "machine": "workshop" } }).to_string(),
        ))
        .await
        .expect("the host frame is sent");
    let listed = wait_until_online(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed["last_frame_ms"],
        json!(START_MS + 1_000),
        "a frame on the new channel is what makes it online: {listed}"
    );
}

/// One device's row from `/fleet`, or a failure that shows the whole answer.
async fn fleet_row(app: Router, session: &str, device_id: &str) -> Value {
    let (status, fleet) = send(app, get("/fleet", Some(session))).await;
    assert_eq!(status, StatusCode::OK, "the fleet read: {fleet}");
    fleet
        .as_array()
        .expect("the fleet is a list")
        .iter()
        .find(|entry| entry["id"] == json!(device_id))
        .cloned()
        .unwrap_or_else(|| panic!("the device {device_id} is listed: {fleet}"))
}

/// Wait until the console has processed the dropped socket and the row says offline.
async fn wait_until_offline(app: Router, session: &str, device_id: &str) -> Value {
    for _ in 0..100 {
        let listed = fleet_row(app.clone(), session, device_id).await;
        if listed["online"] == json!(false) {
            return listed;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the dropped channel still counts as online");
}

/// Wait until a frame has been recorded and the row says online.
async fn wait_until_online(app: Router, session: &str, device_id: &str) -> Value {
    for _ in 0..100 {
        let listed = fleet_row(app.clone(), session, device_id).await;
        if listed["online"] == json!(true) {
            return listed;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the channel never carried a frame");
}

/// Wait until `joins` `device.joined` boundary lines are on disk.
///
/// The upgrade callback registers a socket in a spawned task, so a test that needs "the socket
/// exists but no frame has arrived" must wait for an observable boundary rather than a sleep. The
/// line is written by `open`, before the socket is served.
async fn wait_until_joined(fixture: &Fixture, joins: usize) {
    for _ in 0..100 {
        let seen = audit_lines(fixture)
            .iter()
            .filter(|line| line["tool"] == json!("device.joined"))
            .count();
        if seen >= joins {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the channel never wrote its device.joined line");
}

/// Wait until the reconnect's `device.joined` line is on disk and the row reads it back.
async fn wait_until_outage(app: Router, session: &str, device_id: &str) -> Value {
    for _ in 0..100 {
        let listed = fleet_row(app.clone(), session, device_id).await;
        if listed.get("last_outage").is_some() {
            return listed;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the reconnect never left an outage");
}

/// **THE OUTAGE THAT IS OVER IS STILL A FACT.** A fleet row says `online` again the moment a device
/// comes back; the pair of moments the console observed is what lets a menu line say "back 2h ago
/// after 41m" without opening the machine. The names are the duration and the return, because those
/// are the two a reader compares.
///
/// The two moments are audit lines now (`device.left`, `device.joined`), and the duration is their
/// difference on the console's clock. The test moves that clock between the close and the reopen so
/// the numbers are deterministic rather than a measured wall-clock race.
#[tokio::test]
async fn a_reconnect_records_how_long_the_device_was_away() {
    let fixture = Fixture::new("outage-record");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token, device) = present_device(&fixture, &server, &signing_key(0x78)).await;
    let session = fixture.authenticate().await;

    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert!(
        listed.get("last_outage").is_none(),
        "a device that has never gone away omits the key, rather than inventing a zero: {listed}"
    );

    let gone_at = fixture.now_ms();
    drop(device);
    // The outage starts when the console notices the close, not when the socket was dropped: wait
    // for the offline row, then measure the gap it stayed away.
    wait_until_offline(fixture.app(), &session, &device_id).await;
    fixture.advance_ms(80);
    let back_at = fixture.now_ms();

    let _again = open_channel(&server, &device_id, &token)
        .await
        .expect("the device reconnects");

    let listed = wait_until_outage(fixture.app(), &session, &device_id).await;
    let outage = listed
        .get("last_outage")
        .unwrap_or_else(|| panic!("the reconnect leaves an outage: {listed}"));
    assert_eq!(outage["away_ms"], json!(80), "{listed}");
    assert_eq!(outage["back_at_ms"], json!(back_at), "{listed}");
    assert_eq!(
        outage["back_at_ms"].as_u64().unwrap_or(0) - outage["away_ms"].as_u64().unwrap_or(0),
        gone_at,
        "the absence began when the console saw the close: {listed}"
    );

    // The two moments are audit lines, marked as the console's own facts rather than a tool call.
    // The initial connect, the close and the reopen are the three: the outage is the last two.
    let boundaries: Vec<Value> = audit_lines(&fixture)
        .into_iter()
        .filter(|line| line["kind"] == json!("device"))
        .collect();
    assert_eq!(boundaries.len(), 3, "{boundaries:?}");
    for line in &boundaries {
        assert_eq!(line["device_id"], json!(device_id));
        assert_eq!(line["caller"], json!("console"));
        assert_eq!(line["status"], json!(200));
        assert!(
            line.get("token_id").is_none(),
            "a channel boundary is not a client call: {line}"
        );
    }
    assert_eq!(boundaries[0]["tool"], json!("device.joined"));
    assert_eq!(boundaries[0]["ts_ms"], json!(START_MS));
    assert_eq!(boundaries[1]["tool"], json!("device.left"));
    assert_eq!(boundaries[1]["ts_ms"], json!(gone_at));
    assert_eq!(boundaries[2]["tool"], json!("device.joined"));
    assert_eq!(boundaries[2]["ts_ms"], json!(back_at));
}

/// **THE ABSENCE OUTLIVES THE CONSOLE THAT WATCHED IT.** The record is the audit, not a map in the
/// process: a console restarted mid-outage still reads the close the dead process wrote and the
/// open the new one wrote, and the pair is the interval between them.
#[tokio::test]
async fn an_absence_outlives_a_console_restart() {
    let fixture = Fixture::new("outage-audit-restart");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token, device) = present_device(&fixture, &server, &signing_key(0x79)).await;
    let session = fixture.authenticate().await;

    drop(device);
    wait_until_offline(fixture.app(), &session, &device_id).await;
    fixture.advance_ms(41 * MINUTE_MS);
    let _again = open_channel(&server, &device_id, &token)
        .await
        .expect("the device reconnects");

    let listed = wait_until_outage(fixture.app(), &session, &device_id).await;
    let outage = listed["last_outage"].clone();
    assert_eq!(outage["away_ms"], json!(41 * MINUTE_MS), "{listed}");

    let on_disk = fs::read_to_string(fixture.config.data.join("devices.json"))
        .expect("the device registry is readable");
    assert!(
        !on_disk.contains("last_outage") && !on_disk.contains("away_ms"),
        "the outage is not written to the device registry: {on_disk}"
    );
    let audit =
        fs::read_to_string(fixture.config.data.join("audit.jsonl")).expect("the audit is readable");
    assert!(
        audit.contains("\"tool\":\"device.left\"") && audit.contains("\"tool\":\"device.joined\""),
        "the two boundaries are audit lines: {audit}"
    );

    // A new process: the channel map is empty, the audit is not.
    let restarted = fixture.restart();
    let session = authenticate_on(&restarted).await;
    let listed = fleet_row(restarted.router(), &session, &device_id).await;
    assert_eq!(
        listed.get("last_outage"),
        Some(&outage),
        "the pair is read back from the audit, not from the dead process: {listed}"
    );
}

/// **THE AUDIT ALONE PROVES AN ABSENCE.** Seeded lines, no channel ever opened in this console: a
/// `device.left` with no return yet is not an absence; adding the `device.joined` makes it one.
#[tokio::test]
async fn a_channel_pair_in_the_audit_is_the_last_outage() {
    let fixture = Fixture::new("outage-audit-pair");
    let device_id = register_fixture_device(&fixture, &signing_key(0x7a)).await;
    let session = fixture.authenticate().await;
    let left_at = START_MS + 1_000;
    let joined_at = START_MS + 171 * MINUTE_MS;
    let left = json!({
        "ts_ms": left_at,
        "kind": "device",
        "device_id": device_id,
        "status": 200,
        "tool": "device.left",
        "caller": "console",
    });

    seed_audit_file(&fixture, "audit.jsonl", std::slice::from_ref(&left));
    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert!(
        listed.get("last_outage").is_none(),
        "a drop with no return is still away, so it is not a completed absence: {listed}"
    );

    let joined = json!({
        "ts_ms": joined_at,
        "kind": "device",
        "device_id": device_id,
        "status": 200,
        "tool": "device.joined",
        "caller": "console",
    });
    seed_audit_file(&fixture, "audit.jsonl", &[left, joined]);
    let listed = fleet_row(fixture.app(), &session, &device_id).await;
    assert_eq!(
        listed.get("last_outage"),
        Some(&json!({
            "away_ms": joined_at - left_at,
            "back_at_ms": joined_at,
        })),
        "the pair is the absence: {listed}"
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

/// Write `lines` to one audit generation (`audit.jsonl` or `audit.1.jsonl`), oldest first, without
/// going through the writer.
fn seed_audit_file(fixture: &Fixture, name: &str, lines: &[Value]) {
    let path = fixture.config.data.join(name);
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    fs::write(&path, text).expect("the seeded audit generation is written");
}

/// The audit window can be one machine's: a device filter is applied during the read, so another
/// machine's newer traffic cannot push this machine's lines out of the returned window.
#[tokio::test]
async fn the_audit_window_can_be_one_machines() {
    let fixture = Fixture::new("audit-one-machine");
    let session = fixture.authenticate().await;
    let alpha = "a1".repeat(32);
    let beta = "b2".repeat(32);

    // Alpha's six lines are the older generation; beta has written eight newer ones. A fleet read
    // capped at three stops inside beta's traffic, so a filter applied *after* the cap would return
    // nothing for alpha — exactly the bug the device read has to avoid.
    let alpha_lines: Vec<Value> = (0..6)
        .map(|index| {
            json!({
                "ts_ms": START_MS + index,
                "kind": "mcp",
                "device_id": alpha.as_str(),
                "status": 200,
                "tool": "terminal.list",
            })
        })
        .collect();
    let beta_lines: Vec<Value> = (0..8)
        .map(|index| {
            json!({
                "ts_ms": START_MS + 100 + index,
                "kind": "mcp",
                "device_id": beta.as_str(),
                "status": 200,
                "tool": "files.read",
            })
        })
        .collect();
    seed_audit_file(&fixture, "audit.1.jsonl", &alpha_lines);
    seed_audit_file(&fixture, "audit.jsonl", &beta_lines);

    // Without `device` the answer is the fleet's newest three, unchanged: beta's.
    let (status, body) = send(fixture.app(), get("/audit?limit=3", Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    let fleet = body.as_array().expect("a list of audit lines");
    assert_eq!(fleet.len(), 3);
    assert!(
        fleet.iter().all(|line| line["device_id"] == json!(beta)),
        "{fleet:?}"
    );

    // With `device`, alpha gets three of its own lines even though eight newer beta lines exist, and
    // they come newest first from the rotated generation.
    let (status, body) = send(
        fixture.app(),
        get(&format!("/audit?device={alpha}&limit=3"), Some(&session)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mine = body.as_array().expect("a list of audit lines");
    assert_eq!(mine.len(), 3, "{mine:?}");
    assert!(
        mine.iter().all(|line| line["device_id"] == json!(alpha)),
        "{mine:?}"
    );
    let times: Vec<u64> = mine
        .iter()
        .filter_map(|line| line["ts_ms"].as_u64())
        .collect();
    assert_eq!(times, vec![START_MS + 5, START_MS + 4, START_MS + 3]);

    // More than alpha has: the scan crosses the rotation and stops at what exists.
    let (_, body) = send(
        fixture.app(),
        get(&format!("/audit?device={alpha}&limit=100"), Some(&session)),
    )
    .await;
    assert_eq!(
        body.as_array().expect("a list").len(),
        6,
        "the rotation is read when the current file has fewer matches"
    );
}

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
///
/// **IT SENDS NO FRAME.** Since `online` means "a frame arrived recently", a socket alone is offline;
/// tests about answers and replacements want exactly that one variable. Use [`present_device`] when
/// the scenario depends on presence.
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

/// Open a registered device's channel, send the host frame a real device sends on connect, and wait
/// until the console calls it online.
///
/// The wait reads `/fleet` as an operator, so this helper creates a session; the auth line it writes
/// is why a test that counts the audit's every line keeps using [`connected_device`] instead.
async fn present_device(
    fixture: &Fixture,
    server: &TestServer,
    key: &SigningKey,
) -> (String, String, DeviceSocket) {
    let (device_id, token, mut device) = connected_device(fixture, server, key).await;
    device
        .send(WsMessage::Text(
            json!({ "host": { "machine": "workshop", "caps": [] } }).to_string(),
        ))
        .await
        .expect("the first host frame is sent");
    let session = fixture.authenticate().await;
    wait_until_online(fixture.app(), &session, &device_id).await;
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
            // The channel's own join: a console fact, not a call, and not what this test counts.
            json!("device"),
            json!("auth"),
            json!("grant"),
            json!("mcp")
        ],
        "one line per operator action, plus the channel boundary"
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

// ---- the root and the login link ----------------------------------------------------------------

#[tokio::test]
async fn the_root_is_the_react_workspace_unless_a_login_code_is_riding_on_it() {
    let fixture = Fixture::new("root-react");

    let (status, _, page) = send_text(fixture.app(), get("/", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("id=\"root\""),
        "the root is the React workspace"
    );

    // `--login-link` puts a one-time code in the query; the `/next` shim redeems it, so that
    // request must keep going there with the query intact.
    let (status, headers, _) = send_text(fixture.app(), get("/?code=one-time", None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/next?code=one-time")
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

    // The React page fetches this route; there is no second copy of the registry in the console.
}

#[tokio::test]
async fn the_tool_catalogue_is_not_served_without_an_operator() {
    let fixture = Fixture::new("page-tools-gate");
    let (status, _) = send(fixture.app(), get("/tools", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_react_page_is_served_as_one_embedded_html_file() {
    let fixture = Fixture::new("page-react");
    // `/app` is the React page during the migration; it is one file and carries no data of its own.
    let (status, _, page) = send_text(fixture.app(), get("/app", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("id=\"root\""));
    assert!(page.contains("<title>Summrise console"));
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

/// Cloudflare Access is the identity in front of the console. An operator the edge already
/// authenticated was never given a token to type, so a verified header on the login-link shim must
/// mint the same session cookie the React page reads.
#[tokio::test]
async fn a_verified_access_request_signs_in_without_a_token() {
    let fixture = Fixture::with_access("access-sign-in");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");

    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("the answer sets the session it just minted");
    assert!(cookie.starts_with("summrise_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
}

/// Signing out behind Access has to end the edge's session too: the page cannot, so a request that
/// carried an Access assertion is answered with the edge's logout URL. A token-or-cookie-only
/// sign-out gets `null` even on an Access-configured console: the edge never owned that session, and
/// Cloudflare's logout would be a dead end. Without Access there is no edge session at all.
#[tokio::test]
async fn signing_out_names_the_edge_logout_only_when_the_request_carried_access() {
    let with_edge = Fixture::with_access("sign-out-edge");
    let cookie = sign_in_cookie(&with_edge).await;
    let request = Request::builder()
        .method("POST")
        .uri("/logout")
        .header(header::COOKIE, &cookie)
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");
    let (status, _, body) = send_text(with_edge.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_str(&body).expect("the answer is JSON");
    let url = body["access_logout"]
        .as_str()
        .expect("the edge logout is named");
    assert!(url.ends_with("/cdn-cgi/access/logout"), "{url}");

    // The same console, the same Access configuration, but this sign-out carries a session alone: it
    // never came through the edge, so the answer must not name an edge session to end.
    let cookie = sign_in_cookie(&with_edge).await;
    let request = Request::builder()
        .method("POST")
        .uri("/logout")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .expect("the request builds");
    let (status, _, body) = send_text(with_edge.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_str(&body).expect("the answer is JSON");
    assert!(body["access_logout"].is_null(), "{body}");

    let plain = Fixture::new("sign-out-plain");
    let cookie = sign_in_cookie(&plain).await;
    let request = Request::builder()
        .method("POST")
        .uri("/logout")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .expect("the request builds");
    let (status, _, body) = send_text(plain.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_str(&body).expect("the answer is JSON");
    assert!(body["access_logout"].is_null(), "{body}");
}

/// The middleware mints only for GET `/next`; a POST carrying a verified Access header is not an
/// identity. The old page's test for this died with the page, and the guard it proved is still live.
#[tokio::test]
async fn a_post_carrying_only_an_access_header_mints_nothing() {
    let fixture = Fixture::with_access("access-post");
    let request = Request::builder()
        .method("POST")
        .uri("/next")
        .header(ACCESS_HEADER, access_token(60))
        .body(Body::empty())
        .expect("the request builds");

    let (_, headers, _) = send_text(fixture.app(), request).await;

    assert!(
        headers.get(header::SET_COOKIE).is_none(),
        "a POST must not mint a session"
    );
}

/// A header that does not verify is not an identity: the request goes to the React page signed out,
/// and nothing is minted for whoever sent it.
#[tokio::test]
async fn an_access_header_that_does_not_verify_signs_nothing_in() {
    let fixture = Fixture::with_access("access-refused");
    let request = Request::builder()
        .method("GET")
        .uri("/next")
        .header(ACCESS_HEADER, "not-a-jwt")
        .body(Body::empty())
        .expect("the request builds");

    let (status, headers, _) = send_text(fixture.app(), request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/")
    );
    assert!(
        headers.get(header::SET_COOKIE).is_none(),
        "a refused identity mints nothing"
    );
}

/// `--login-link` prints `<page>?code=…` and promises two minutes' way in. The page has to spend
/// that code, or the only recovery left when Access misbehaves is a secret in a file on the host.
#[tokio::test]
async fn an_unknown_path_is_a_plain_404() {
    let fixture = Fixture::new("not-found");
    // Unauthenticated: the 404 is the same answer whoever asks, so a typo never becomes a login wall.
    let (status, headers, body) = send_text(fixture.app(), get("/next/no-such-page", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        None,
        "there is no fallback page: {body}"
    );
    assert!(body.is_empty(), "the path is not echoed: {body}");
}

#[tokio::test]
async fn a_login_link_code_signs_in_exactly_once() {
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
        Some("/"),
        "the code leaves the URL with the redirect, and the React page is where it lands"
    );
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("the link mints a session");
    assert!(cookie.starts_with("summrise_session="), "{cookie}");
    let pair = cookie.split(';').next().expect("the name=value pair");

    // The session the link bought is an operator session, not just a cookie: the probe says so.
    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/session", pair)).await;
    assert_eq!(status, StatusCode::OK);
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(probe["authenticated"], json!(true), "{body}");

    // A link is one use: a second visit says so instead of minting another session.
    let (status, headers, _) = send_text(fixture.app(), get(&uri, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers.get(header::SET_COOKIE).is_none());
}

/// A code that does not match is refused like an expired one, and no session is minted.
#[tokio::test]
async fn a_login_link_that_does_not_match_is_refused() {
    let fixture = Fixture::new("login-link-refused");
    let code = LoginCode {
        code: "b".repeat(64),
        expires_at_ms: fixture.now_ms() + MINUTE_MS,
    };
    summrise_console::write_login_code(&fixture.config.data, &code).expect("the code is written");

    let (status, headers, body) =
        send_text(fixture.app(), get("/next?code=not-the-code", None)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("not accepted"), "{body}");
    assert!(headers.get(header::SET_COOKIE).is_none());
}

/// `/session` is a probe a client can call with the sign-in cookie; before this it could only see a
/// bearer token, so `via: "session"` was unreachable and a signed-in browser read as signed out.
#[tokio::test]
async fn the_session_probe_sees_a_sign_in_cookie() {
    let fixture = Fixture::new("session-probe-cookie");
    let cookie = sign_in_cookie(&fixture).await;
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
/// that cost an operator an hour when the old page fell back to the token box.
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

    // 3. A `--login-link` code.
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

/// A browser that signs in repeatedly must not grow the file without bound: the store keeps the
/// newest few, not everything since the first deploy.
#[tokio::test]
async fn the_session_store_keeps_at_most_thirty_two_sessions() {
    let fixture = Fixture::new("session-cap");
    let mut newest = String::new();
    for _ in 0..=32 {
        let (status, headers, _) = send_text(
            fixture.app(),
            post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
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
    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/session", cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(
        probe["authenticated"],
        json!(true),
        "the newest session is the one kept: {body}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_session_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("session-mode");
    let _ = sign_in_cookie(&fixture).await;

    let mode = fs::metadata(fixture.config.data.join("sessions.json"))
        .expect("the sessions file exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "a session hash is credential-grade");
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

#[tokio::test]
async fn no_console_answer_is_cacheable() {
    // A cached page outlives a deploy and a cached `/session` outlives a login: both were possible
    // while these routes carried no `Cache-Control`.
    let fixture = Fixture::new("no-store");

    // The page is HTML, so it is read off the response rather than through the JSON helper.
    let response = fixture
        .app()
        .oneshot(get("/", None))
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

    // The login-link shim is an answer too, and a cached redirect outlives the code it spends.
    let shim = fixture
        .app()
        .oneshot(get("/next", None))
        .await
        .expect("the shim is served");
    assert_eq!(shim.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        shim.headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
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
        present_device(&fixture, &server, &signing_key(0xa7)).await;
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

/// A grant on the standard route is a client's call with no token record. `token_id` cannot tell a
/// client from the console — the operator's own `/mcp` carries none either — so the caller is a
/// separate fact, and this is the case that proves it.
#[tokio::test]
async fn a_grant_authenticated_call_is_still_a_client() {
    let fixture = Fixture::new("grant-caller");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb8)).await;
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
    let line = lines
        .iter()
        .find(|line| line["kind"] == json!("mcp"))
        .expect("the mcp line");
    assert!(
        line.get("token_id").is_none(),
        "a grant has no token record to name: {line}"
    );
    assert_eq!(
        line["caller"],
        json!("client"),
        "the audience is the route, not the credential's token record: {line}"
    );
}

/// The console's own page calls `/mcp` on the operator session. That call is the console's, not the
/// AI's, and both the line and the live event say so.
#[tokio::test]
async fn a_console_call_names_the_console_on_its_line_and_event() {
    let fixture = Fixture::new("caller-console");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xb9)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "tools/call",
                        "params": { "name": "terminal.list", "arguments": {} },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    answer(&mut device, &frame, tool_result(json!({ "content": [] }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    read_until(&mut socket, &mut seen, "\"caller\":\"console\"").await;
    assert!(
        seen.contains("\"caller\":\"console\""),
        "the live event names the console: {seen}"
    );

    let lines = audit_lines(&fixture);
    let line = lines
        .iter()
        .find(|line| line["kind"] == json!("mcp"))
        .expect("the mcp line");
    assert_eq!(line["caller"], json!("console"), "{line}");
}

/// An AI client's call through `/mcp/:id` is the client's, on the line and on the event.
#[tokio::test]
async fn a_client_call_names_the_client_on_its_line_and_event() {
    let fixture = Fixture::new("caller-client");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xba)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "laptop-dsh", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let plaintext = created["token"].as_str().expect("the token").to_owned();

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "{body}");

    read_until(&mut socket, &mut seen, "\"caller\":\"client\"").await;
    assert!(
        seen.contains("\"caller\":\"client\""),
        "the live event names the client: {seen}"
    );

    let lines = audit_lines(&fixture);
    let line = lines
        .iter()
        .find(|line| line["kind"] == json!("mcp"))
        .expect("the mcp line");
    assert_eq!(line["caller"], json!("client"), "{line}");
}

/// The live event names the token record that made a client's call, not only the line read back from
/// `/audit`: the newest rows are the live ones, so a tokenless event would hide the token behind the
/// call exactly where the page looks first.
#[tokio::test]
async fn a_token_call_publishes_its_token_on_the_live_event() {
    let fixture = Fixture::new("token-live-event");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc3)).await;
    let session = fixture.authenticate().await;
    let (status, created) = fixture
        .create_token(&session, &device_id, "laptop-dsh", &["files.read"], false)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token_id = created["id"]
        .as_str()
        .expect("the token's record id")
        .to_owned();
    let plaintext = created["token"].as_str().expect("the token").to_owned();

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
    });
    let http = send(
        fixture.app(),
        post_token(&device_id, &plaintext, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    };
    let ((status, body), ()) = tokio::join!(http, device_side);
    assert_eq!(status, StatusCode::OK, "{body}");

    let needle = format!("\"token_id\":\"{token_id}\"");
    read_until(&mut socket, &mut seen, &needle).await;
    assert!(
        seen.contains(&needle),
        "the live event carries the token record that made the call: {seen}"
    );
    assert!(
        seen.contains("\"caller\":\"client\""),
        "the event still names the audience: {seen}"
    );
}

/// The operator's own `/mcp` call has no token record, and the live event must not invent one: a
/// `token_id` on a console row would name a credential that did not make the call.
#[tokio::test]
async fn a_console_call_publishes_no_token_on_the_live_event() {
    let fixture = Fixture::new("console-live-event");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0xc4)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "tools/call",
                        "params": { "name": "terminal.list", "arguments": {} },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    answer(&mut device, &frame, tool_result(json!({ "content": [] }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    read_until(&mut socket, &mut seen, "\"caller\":\"console\"").await;
    assert!(
        seen.contains("\"caller\":\"console\""),
        "the live event names the console: {seen}"
    );
    assert!(
        !seen.contains("\"token_id\""),
        "the console's own call carries no token record: {seen}"
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

// ---- the live catalogue and plugin-tool token scopes (capability hub B1/B2) --------------------

/// `GET /fleet/:id/tools` is a real `tools/list` over the device channel, under the operator's own
/// scope. The console adds only the source/capability classification; the device's list is the
/// list.
#[tokio::test]
async fn the_live_catalogue_reads_the_devices_own_tools_list() {
    let fixture = Fixture::new("live-catalogue");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        present_device(&fixture, &server, &signing_key(0xb6)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let uri = format!("/fleet/{device_id}/tools");
    let session_for_read = session.clone();
    let read = tokio::spawn(async move { send(app, get(&uri, Some(&session_for_read))).await });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"]["method"], json!("tools/list"), "{frame}");
    assert_eq!(
        frame["token"]["token_id"],
        json!("operator"),
        "the live read uses the operator's existing scope, not a second credential"
    );
    assert_eq!(frame["token"]["device_id"], json!(device_id));
    assert_eq!(frame["token"]["scopes"], json!(["*"]));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "tools": [
                {
                    "name": "terminal.create",
                    "description": "Start a session.",
                    "inputSchema": { "type": "object", "properties": {} },
                },
                {
                    "name": "hello.greet",
                    "description": "Greet someone.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "who": { "type": "string" } },
                        "required": ["who"],
                    },
                    "annotations": { "destructiveHint": false },
                },
            ]},
        }),
    )
    .await;

    let (status, body) = read.await.expect("the read finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "tools": [
            {
                "name": "terminal.create",
                "description": "Start a session.",
                "inputSchema": { "type": "object", "properties": {} },
                "source": "builtin",
                "capability": "terminal",
                "destructive": true,
            },
            {
                "name": "hello.greet",
                "description": "Greet someone.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "who": { "type": "string" } },
                    "required": ["who"],
                },
                "source": "plugin:hello",
                "capability": "hello",
                "destructive": false,
            },
        ]}),
        "the page's contract is name/description/inputSchema/source/capability/destructive"
    );
}

/// An empty advertisement is a 200 with `[]`: the page uses the empty list to detect an older
/// build, so it must not be an error.
#[tokio::test]
async fn the_live_catalogue_serves_an_empty_device_list_as_200() {
    let fixture = Fixture::new("live-catalogue-empty");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        present_device(&fixture, &server, &signing_key(0xb7)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let uri = format!("/fleet/{device_id}/tools");
    let session_for_read = session.clone();
    let read = tokio::spawn(async move { send(app, get(&uri, Some(&session_for_read))).await });
    let frame = next_json(&mut device).await;
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": [] } }),
    )
    .await;

    let (status, body) = read.await.expect("the read finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "tools": [] }));
}

/// A device with no channel is 503 with a sentence, never a cached or static list.
#[tokio::test]
async fn the_live_catalogue_is_503_when_the_device_is_offline() {
    let fixture = Fixture::new("live-catalogue-offline");
    let (device_id, _token) = fixture.register(&signing_key(0xb8)).await;
    let session = fixture.authenticate().await;

    let (status, body) = send(
        fixture.app(),
        get(&format!("/fleet/{device_id}/tools"), Some(&session)),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], json!("device reconnecting"));
    let message = body["message"]
        .as_str()
        .expect("the refusal has a sentence");
    assert!(message.contains("not connected"), "{message}");
    assert!(
        message.contains("connect"),
        "the sentence names the action: {message}"
    );
}

/// A device that takes the frame and does not answer inside the console's bound is 504.
#[tokio::test]
async fn the_live_catalogue_is_504_when_the_device_does_not_answer() {
    let fixture = Fixture::with_timeout("live-catalogue-timeout", Duration::from_millis(50));
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, _device) = present_device(&fixture, &server, &signing_key(0xb9)).await;
    let session = fixture.authenticate().await;

    let (status, body) = send(
        fixture.app(),
        get(&format!("/fleet/{device_id}/tools"), Some(&session)),
    )
    .await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{body}");
    assert_eq!(body["error"], json!("device timeout"));
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("did not answer")),
        "the refusal says what happened: {body}"
    );
}

/// An answer that is not a tool list is a 500 upstream fault, not an empty catalogue.
#[tokio::test]
async fn the_live_catalogue_is_500_when_the_device_answers_malformed() {
    let fixture = Fixture::new("live-catalogue-malformed");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        present_device(&fixture, &server, &signing_key(0xba)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let uri = format!("/fleet/{device_id}/tools");
    let session_for_read = session.clone();
    let read = tokio::spawn(async move { send(app, get(&uri, Some(&session_for_read))).await });
    let frame = next_json(&mut device).await;
    answer(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": "not a list" } }),
    )
    .await;

    let (status, body) = read.await.expect("the read finishes");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"], json!("malformed_tools"));
}

/// A token may name a plugin tool only when the target device's live `tools/list` advertises it.
/// The stored record is the existing shape; the plugin name is just another tool string.
#[tokio::test]
async fn a_token_can_scope_a_plugin_tool_the_device_advertises() {
    let fixture = Fixture::new("plugin-token-online");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _device_token, mut device) =
        present_device(&fixture, &server, &signing_key(0xd0)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let session_for_create = session.clone();
    let device_for_create = device_id.clone();
    let create = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/tokens",
                &session_for_create,
                json!({
                    "name": "hello",
                    "device_id": device_for_create,
                    "tools": ["files.read", "hello.greet"],
                    "allow_destructive": false,
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"]["method"], json!("tools/list"), "{frame}");
    assert_eq!(frame["token"]["token_id"], json!("operator"));
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "tools": [{
                "name": "hello.greet",
                "description": "Greet someone.",
                "inputSchema": { "type": "object", "properties": {} },
            }]},
        }),
    )
    .await;

    let (status, created) = create.await.expect("the create finishes");
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["tools"], json!(["files.read", "hello.greet"]));
    assert!(created["token"]
        .as_str()
        .expect("the plaintext is returned once")
        .starts_with("srm_"));

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list[0]["tools"], json!(["files.read", "hello.greet"]));

    // The stored shape did not change: the plugin name is a string in the existing `tools` list.
    let text = fs::read_to_string(fixture.config.data.join("tokens.json"))
        .expect("the token file is written");
    let stored: Value = serde_json::from_str(&text).expect("the token file is JSON");
    let mut keys: Vec<&str> = stored[0]
        .as_object()
        .expect("a stored record")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "allow_destructive",
            "created_at_ms",
            "device_id",
            "hash",
            "id",
            "name",
            "tools"
        ],
        "a plugin tool adds no field to the record: {stored}"
    );
}

/// Offline with a plugin tool is a 400 that names the action, not a token minted against a list
/// the console never read.
#[tokio::test]
async fn a_plugin_tool_is_refused_while_the_device_is_offline() {
    let fixture = Fixture::new("plugin-token-offline");
    let (device_id, _token) = fixture.register(&signing_key(0xd1)).await;
    let session = fixture.authenticate().await;

    let (status, body) = fixture
        .create_token(&session, &device_id, "hello", &["hello.greet"], false)
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("unknown_tool"));
    let message = body["message"]
        .as_str()
        .expect("the refusal has a sentence");
    assert!(message.contains("hello.greet"), "{message}");
    assert!(
        message.contains("connect the machine, then mint the token"),
        "the sentence names the action: {message}"
    );

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list, json!([]), "a refused mint writes nothing: {list}");
}

/// Online but not advertising the name is still a 400: the live list is the authority, so a typo
/// cannot become a token scope.
#[tokio::test]
async fn a_plugin_tool_the_device_does_not_advertise_is_refused() {
    let fixture = Fixture::new("plugin-token-unknown");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        present_device(&fixture, &server, &signing_key(0xd2)).await;
    let session = fixture.authenticate().await;

    let app = fixture.app();
    let session_for_create = session.clone();
    let device_for_create = device_id.clone();
    let create = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/tokens",
                &session_for_create,
                json!({
                    "name": "typo",
                    "device_id": device_for_create,
                    "tools": ["hello.nope"],
                    "allow_destructive": false,
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    assert_eq!(frame["mcp"]["method"], json!("tools/list"), "{frame}");
    answer(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "tools": [{
                "name": "hello.greet",
                "inputSchema": { "type": "object", "properties": {} },
            }]},
        }),
    )
    .await;

    let (status, body) = create.await.expect("the create finishes");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("unknown_tool"));
    assert_eq!(
        body["message"],
        json!("the device does not advertise hello.nope; connect it and try again")
    );

    let (_, list) = send(fixture.app(), get("/tokens", Some(&session))).await;
    assert_eq!(list, json!([]), "a refused mint writes nothing: {list}");
}

/// An aggregate token's scope is one list for every device, so v1 keeps it registry-only and says
/// plugin tools are per-device rather than minting a scope most of the fleet cannot honour.
#[tokio::test]
async fn an_aggregate_token_cannot_scope_a_plugin_tool() {
    let fixture = Fixture::new("plugin-token-aggregate");
    let (device_id, _token) = fixture.register(&signing_key(0xd3)).await;
    let session = fixture.authenticate().await;

    let (status, body) = fixture
        .create_aggregate_token(&session, "all", &[device_id], &["hello.greet"])
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("unknown_tool"));
    let message = body["message"]
        .as_str()
        .expect("the refusal has a sentence");
    assert!(
        message.contains("per-device"),
        "the sentence names the v1 rule: {message}"
    );
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

fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("the request builds")
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

/// The JSON sign-in is the page's door too: it sets the same page cookie, and a request carrying only
/// that cookie is an operator session. This is what keeps a bearer out of the page's JavaScript and a
/// reload from asking for the 64-character operator token again.
#[tokio::test]
async fn the_api_sign_in_sets_the_session_cookie_and_a_cookie_alone_is_a_session() {
    let fixture = Fixture::new("api-signin-cookie");
    let (status, headers, _) = send_text(
        fixture.app(),
        post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("the sign-in sets the page cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .to_owned();
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(
        cookie.contains("Path=/"),
        "the page and its JSON API share one cookie: {cookie}"
    );
    let pair = cookie
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    // No Authorization header in anything below: the cookie is the credential.
    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/session", &pair)).await;
    assert_eq!(status, StatusCode::OK);
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(probe["authenticated"], json!(true), "{body}");

    let (status, _, _) = send_text(fixture.app(), get_with_cookie("/fleet", &pair)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an operator route accepts the cookie alone"
    );

    let logout = Request::builder()
        .method("POST")
        .uri("/logout")
        .header(header::COOKIE, pair.as_str())
        .body(Body::empty())
        .expect("the request builds");
    let (status, headers, _) = send_text(fixture.app(), logout).await;
    assert_eq!(status, StatusCode::OK);
    let cleared = headers
        .get(header::SET_COOKIE)
        .expect("the sign-out clears the cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .to_owned();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    let (status, _, _) = send_text(fixture.app(), get_with_cookie("/fleet", &pair)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the ended session is not a session"
    );
}

/// The session cookie a JSON sign-in sets; the cookie is the credential every operator route takes.
async fn sign_in_cookie(fixture: &Fixture) -> String {
    let (status, headers, _) = send_text(
        fixture.app(),
        post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
async fn the_session_cookie_is_secure_only_behind_https() {
    let fixture = Fixture::new("session-cookie-secure");
    let mut request = post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) }));
    request.headers_mut().insert(
        axum::http::HeaderName::from_static("x-forwarded-proto"),
        axum::http::HeaderValue::from_static("https"),
    );
    let (status, headers, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a cookie")
        .to_str()
        .expect("ASCII");
    assert!(cookie.contains("Secure"), "{cookie}");

    let (status, headers, _) = send_text(
        fixture.app(),
        post_json("/auth", json!({ "token": hex(&OPERATOR_TOKEN) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
}

/// A token that did not consent to destruction is refused immediately, with a sentence that says
/// how to authorize it. There is no ninety-second hold for the operator to answer.
#[tokio::test]
async fn a_destructive_call_without_token_consent_is_refused_immediately() {
    let fixture = Fixture::new("approval-no-consent");
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
        "id": 66,
        "method": "tools/call",
        "params": { "name": "terminal.create", "arguments": { "kind": "pty" } },
    });
    let app = fixture.app();
    let request = post_token(&device_id, &plaintext, call.clone());
    let client = tokio::spawn(async move { send(app, request).await });

    let frame = next_json(&mut device).await;
    answer_error(
        &mut device,
        &frame,
        json!({ "jsonrpc": "2.0", "id": 66, "error": { "code": -32002, "message": "'terminal.create' is destructive and needs approval" } }),
    )
    .await;

    let (status, body) = client.await.expect("the client finishes");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["code"], json!(-32002), "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not allow destructive calls")),
        "{body}"
    );
}

/// A refused Access token leaves a line. "Was the edge's header there, and did the console accept
/// it?" must be answerable from the audit, not only from the sign-in page.
#[tokio::test]
async fn a_refused_access_token_is_audited() {
    let fixture = Fixture::with_access("access-refusal-audited");
    let mut request = get("/next", None);
    request.headers_mut().insert(
        "cf-access-jwt-assertion",
        "not-a-jwt".parse().expect("a header value"),
    );
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let lines = audit_lines(&fixture);
    let refused = lines
        .iter()
        .find(|line| line["kind"] == "auth" && line["status"] == 401)
        .unwrap_or_else(|| panic!("a refusal line: {lines:#?}"));
    assert_eq!(refused["detail"], "access refused");
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
async fn a_session_survives_a_console_restart() {
    // Every deploy used to sign the operator out, because sessions were memory only. A session is
    // written to `<data>/sessions.json` now, and the token itself never is.
    let fixture = Fixture::new("session-restart");
    let cookie = sign_in_cookie(&fixture).await;

    let restarted = fixture.restart();
    let (status, _, body) =
        send_text(restarted.router(), get_with_cookie("/session", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(
        probe["authenticated"],
        json!(true),
        "the restarted console accepts the cookie: {body}"
    );

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
async fn a_session_expires_after_its_lifetime() {
    let fixture = Fixture::new("session-expiry");
    let cookie = sign_in_cookie(&fixture).await;

    // One minute short of the month: the session still works.
    fixture.advance_ms(30 * 24 * 60 * 60 * 1000 - 60_000);
    let (_, _, body) = send_text(fixture.app(), get_with_cookie("/session", &cookie)).await;
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(probe["authenticated"], json!(true), "{body}");

    // A minute past it: the session is no longer an identity.
    fixture.advance_ms(120_000);
    let (_, _, body) = send_text(fixture.app(), get_with_cookie("/session", &cookie)).await;
    let probe: Value = serde_json::from_str(&body).expect("the probe is JSON");
    assert_eq!(probe["authenticated"], json!(false), "{body}");
}

/// A tailnet deployment trusts one address to say who the operator is. The same header from any
/// other peer is only a string, and the token is still the way in: the flag removes a password,
/// not the identity check.
#[tokio::test]
async fn a_trusted_proxy_names_the_operator_and_only_from_its_own_address() {
    const PROXY: Ipv4Addr = Ipv4Addr::new(100, 85, 163, 159);
    const ELSEWHERE: Ipv4Addr = Ipv4Addr::new(100, 79, 123, 140);
    let fixture = Fixture::with_trusted_proxy("proxy-identity", PROXY);

    // Through the proxy: no token, and the page is already signed in as the name it sent.
    let (status, body) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/session", None), "operator@example.com"),
            PROXY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], json!(true));
    assert_eq!(body["via"], json!("proxy"));
    assert_eq!(body["email"], json!("operator@example.com"));

    // A real route opens on the same word, not only the probe the page asks first.
    let (status, devices) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/fleet", None), "operator@example.com"),
            PROXY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        devices.as_array().map(Vec::is_empty),
        Some(true),
        "the proxy's identity passed the extractor, not just the probe"
    );

    // The same header from another peer proves nothing, even on a console that trusts a proxy.
    let (status, body) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/session", None), "operator@example.com"),
            ELSEWHERE,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["authenticated"],
        json!(false),
        "the address is what makes the claim, not the header"
    );

    let (status, _) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/fleet", None), "operator@example.com"),
            ELSEWHERE,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Without the flag the header is an ordinary string: a console that was never told to trust one
/// cannot be talked into trusting one.
#[tokio::test]
async fn a_console_without_a_trusted_proxy_does_not_believe_the_header() {
    const PROXY: Ipv4Addr = Ipv4Addr::new(100, 85, 163, 159);
    let fixture = Fixture::new("proxy-unconfigured");

    let (status, body) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/session", None), "operator@example.com"),
            PROXY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], json!(false));

    let (status, _) = send(
        fixture.app(),
        from_peer(
            forwarded_user(get("/fleet", None), "operator@example.com"),
            PROXY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// An empty name is not a person: a proxy that authenticated nobody says so by sending nothing.
#[tokio::test]
async fn an_empty_forwarded_user_is_not_an_identity() {
    const PROXY: Ipv4Addr = Ipv4Addr::new(100, 85, 163, 159);
    let fixture = Fixture::with_trusted_proxy("proxy-empty", PROXY);

    let (status, body) = send(
        fixture.app(),
        from_peer(forwarded_user(get("/session", None), "   "), PROXY),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], json!(false));
}

// ---- the terminal stream: the page's one open connection per session --------------------------

/// A raw HTTP/1.1 GET whose body is read as it arrives.
///
/// The SSE route never ends on its own while a session is live, so a tower `oneshot` (which waits
/// for the whole body) cannot see it; this is the smallest client that can, and it reads the same
/// socket the browser does.
async fn raw_get(server: &TestServer, path: &str, headers: &str, until: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("a connection to the test server");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\nConnection: close\r\n{headers}\r\n"
    );
    socket
        .write_all(request.as_bytes())
        .await
        .expect("the request is written");
    let mut seen: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, socket.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
            Ok(Ok(read)) => {
                seen.extend_from_slice(&chunk[..read]);
                if String::from_utf8_lossy(&seen).contains(until) {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&seen).into_owned()
}

fn bearer(session: &str) -> String {
    format!("Authorization: Bearer {session}\r\n")
}

/// One tool's text answer, as a device sends it: the MCP content envelope around JSON.
fn tool_result(value: Value) -> Value {
    json!({ "content": [{ "type": "text", "text": value.to_string() }], "isError": false })
}

/// The page subscribes once and each read's bytes arrive on that connection; the device sees the
/// same `terminal.read` it would have answered for a poll.
#[tokio::test]
async fn a_terminal_stream_pushes_each_read_down_one_connection() {
    let fixture = Fixture::new("terminal-stream-output");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x31)).await;
    let session = fixture.authenticate().await;

    let path = format!("/terminal/{device_id}/session-1/stream?from=7");
    let auth = bearer(&session);
    let stream = raw_get(&server, &path, &auth, "hello-stream");
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"]["params"]["name"], json!("terminal.read"));
        assert_eq!(frame["mcp"]["params"]["arguments"]["cursor"], json!(7));
        assert_eq!(
            frame["mcp"]["params"]["arguments"]["wait_ms"],
            json!(20_000)
        );
        answer(
            &mut device,
            &frame,
            tool_result(json!({ "data": "hello-stream", "cursor": 19, "alive": true })),
        )
        .await;
        // The stream reads again at once; leaving it parked is what a quiet session looks like.
        let _ = tokio::time::timeout(Duration::from_secs(2), next_json(&mut device)).await;
    };
    let (body, ()) = tokio::join!(stream, device_side);

    assert!(body.contains("event: output"), "{body}");
    assert!(body.contains("hello-stream"), "{body}");
    assert!(body.contains("\"cursor\":19"), "{body}");
}

/// A device from before `wait_ms` refuses the field once; the stream drops it and keeps going, and
/// a read with no wait is paced so the console does not spin against it.
#[tokio::test]
async fn a_stream_drops_wait_ms_for_a_device_that_refuses_it() {
    let fixture = Fixture::new("terminal-stream-nowait");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x32)).await;
    let session = fixture.authenticate().await;

    let path = format!("/terminal/{device_id}/session-1/stream");
    let auth = bearer(&session);
    let stream = raw_get(&server, &path, &auth, "after-the-refusal");
    let device_side = async {
        let first = next_json(&mut device).await;
        assert!(first["mcp"]["params"]["arguments"]["wait_ms"].is_number());
        device
            .send(WsMessage::Text(
                json!({
                    "id": first["id"].clone(),
                    "error": { "code": -32602, "message": "unknown field `wait_ms`" },
                })
                .to_string(),
            ))
            .await
            .expect("the refusal is sent");
        let second = next_json(&mut device).await;
        assert!(
            second["mcp"]["params"]["arguments"]["wait_ms"].is_null(),
            "a refusal must not be followed by the same field: {second}"
        );
        answer(
            &mut device,
            &second,
            tool_result(json!({ "data": "after-the-refusal", "cursor": 17, "alive": true })),
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(2), next_json(&mut device)).await;
    };
    let (body, ()) = tokio::join!(stream, device_side);

    assert!(body.contains("event: output"), "{body}");
    assert!(body.contains("after-the-refusal"), "{body}");
}

/// The session's end is a fact the stream carries, with the device's own exit facts.
#[tokio::test]
async fn a_stream_reports_the_end_of_a_session() {
    let fixture = Fixture::new("terminal-stream-end");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x33)).await;
    let session = fixture.authenticate().await;

    let path = format!("/terminal/{device_id}/session-1/stream");
    let auth = bearer(&session);
    let stream = raw_get(&server, &path, &auth, "event: end");
    let device_side = async {
        let frame = next_json(&mut device).await;
        answer(
            &mut device,
            &frame,
            tool_result(json!({ "data": "", "cursor": 3, "alive": false, "exit_code": 3 })),
        )
        .await;
    };
    let (body, ()) = tokio::join!(stream, device_side);

    assert!(body.contains("event: end"), "{body}");
    assert!(body.contains("\"exit_code\":3"), "{body}");
}

/// A refused read is the page's fault sentence, on the same connection.
#[tokio::test]
async fn a_stream_carries_a_refused_read_as_a_fault() {
    let fixture = Fixture::new("terminal-stream-fault");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x34)).await;
    let session = fixture.authenticate().await;

    let path = format!("/terminal/{device_id}/session-1/stream");
    let auth = bearer(&session);
    let stream = raw_get(&server, &path, &auth, "no such session");
    let device_side = async {
        let frame = next_json(&mut device).await;
        device
            .send(WsMessage::Text(
                json!({
                    "id": frame["id"].clone(),
                    "error": { "code": -32000, "message": "no such session" },
                })
                .to_string(),
            ))
            .await
            .expect("the refusal is sent");
    };
    let (body, ()) = tokio::join!(stream, device_side);

    assert!(body.contains("event: fault"), "{body}");
    assert!(body.contains("no such session"), "{body}");
}

/// The stream is an operator route: no session is no stream.
#[tokio::test]
async fn a_terminal_stream_needs_an_operator_session() {
    let fixture = Fixture::new("terminal-stream-auth");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token) = fixture.register(&signing_key(0x35)).await;

    let path = format!("/terminal/{device_id}/session-1/stream");
    let body = raw_get(&server, &path, "", "401").await;
    assert!(body.starts_with("HTTP/1.1 401"), "{body}");
}

/// A device that names the stream capability is followed, not polled: the console sends one
/// subscribe frame, the ack is the first read, and every later chunk arrives unprompted.
#[tokio::test]
async fn a_stream_follows_a_device_that_advertises_it() {
    let fixture = Fixture::new("terminal-stream-follow");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x36)).await;
    // The capability arrives in the host frame, the same way a new build announces it.
    device
        .send(WsMessage::Text(
            json!({ "host": { "machine": "workshop", "caps": ["terminal.stream"] } }).to_string(),
        ))
        .await
        .expect("the host frame is sent");
    tokio::task::yield_now().await;

    let session = fixture.authenticate().await;
    let path = format!("/terminal/{device_id}/session-1/stream?from=4");
    let auth = bearer(&session);
    let body = raw_get(&server, &path, &auth, "pushed-chunk");
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["subscribe"]["session_id"], json!("session-1"));
        assert_eq!(frame["subscribe"]["from"], json!(4));
        assert_eq!(frame["mcp"]["params"]["name"], json!("terminal.read"));
        assert_eq!(frame["mcp"]["params"]["arguments"]["cursor"], json!(4));
        let sub = frame["id"].clone();
        answer(
            &mut device,
            &frame,
            tool_result(json!({ "data": "hello-ack", "cursor": 9, "alive": true })),
        )
        .await;
        device
            .send(WsMessage::Text(
                json!({
                    "term": { "sub": sub, "data": "pushed-chunk", "cursor": 21, "alive": true }
                })
                .to_string(),
            ))
            .await
            .expect("the pushed frame is sent");
        // The client leaves after reading; the console's unsubscribe is the next thing it hears.
        let _ = tokio::time::timeout(Duration::from_secs(2), next_json(&mut device)).await;
    };
    let (body, ()) = tokio::join!(body, device_side);

    assert!(body.contains("event: output"), "{body}");
    assert!(body.contains("hello-ack"), "{body}");
    assert!(body.contains("pushed-chunk"), "{body}");
}

/// Without the capability the console keeps the parked-read pump: the page cannot tell the
/// difference, and an old device never sees a frame it cannot parse.
#[tokio::test]
async fn a_stream_polls_a_device_that_does_not_advertise_it() {
    let fixture = Fixture::new("terminal-stream-polls");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x37)).await;
    let session = fixture.authenticate().await;

    let path = format!("/terminal/{device_id}/session-1/stream?from=0");
    let auth = bearer(&session);
    let body = raw_get(&server, &path, &auth, "polled-chunk");
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert!(
            frame["subscribe"].is_null(),
            "a device without the capability must not be sent a subscribe frame: {frame}"
        );
        assert_eq!(frame["mcp"]["params"]["name"], json!("terminal.read"));
        answer(
            &mut device,
            &frame,
            tool_result(json!({ "data": "polled-chunk", "cursor": 12, "alive": true })),
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(1), next_json(&mut device)).await;
    };
    let (body, ()) = tokio::join!(body, device_side);

    assert!(body.contains("event: output"), "{body}");
    assert!(body.contains("polled-chunk"), "{body}");
}

// ---- the live activity stream: a person reads what the AI did --------------------------------

/// Read an open SSE connection until `needle` appears (or five seconds pass).
async fn read_until(socket: &mut tokio::net::TcpStream, seen: &mut String, needle: &str) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if seen.contains(needle) {
            return;
        }
        match tokio::time::timeout_at(deadline, socket.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => return,
            Ok(Ok(read)) => seen.push_str(&String::from_utf8_lossy(&chunk[..read])),
        }
    }
}

/// Like [`read_until`], but only after `from`: one capture's event, not the previous capture's.
///
/// The stream is one connection for the page's life, so a needle that was already consumed once
/// would otherwise satisfy every later wait.
async fn read_until_after(
    socket: &mut tokio::net::TcpStream,
    seen: &mut String,
    needle: &str,
    from: usize,
) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if seen.get(from..).is_some_and(|tail| tail.contains(needle)) {
            return;
        }
        match tokio::time::timeout_at(deadline, socket.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => return,
            Ok(Ok(read)) => seen.push_str(&String::from_utf8_lossy(&chunk[..read])),
        }
    }
}

/// One SSE event's JSON body by name: the last one in the buffer.
///
/// The stream carries `event: <name>\ndata: <json>\n\n`; a test that needs the started frame's id
/// parses the body rather than matching a field inside a buffer that also holds the snapshot.
fn last_event(seen: &str, name: &str) -> Value {
    let marker = format!("event: {name}\ndata: ");
    let start = seen
        .rfind(&marker)
        .unwrap_or_else(|| panic!("the buffer holds a {name} event: {seen}"))
        + marker.len();
    let rest = &seen[start..];
    let end = rest.find('\n').unwrap_or(rest.len());
    serde_json::from_str(&rest[..end])
        .unwrap_or_else(|error| panic!("the {name} event's data is JSON: {error}: {seen}"))
}

/// The newest `evidence.shot` path in an SSE buffer, without pulling a JSON parser into a test that
/// only needs the one string.
fn last_shot(seen: &str) -> Option<String> {
    let marker = "\"shot\":\"";
    let start = seen.rfind(marker)? + marker.len();
    let rest = &seen[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// A response that is not JSON: an image's exact bytes and headers.
async fn send_bytes(
    app: Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app.oneshot(request).await.expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body is readable")
        .to_vec();
    (status, headers, bytes)
}

/// A 1×1 PNG, as the device's `screen.capture` answers: 68 bytes, valid chunks, a real signature.
const ONE_PIXEL_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

/// The bytes behind [`ONE_PIXEL_PNG`], for byte-for-byte comparisons without a base64 dependency.
fn one_pixel_png() -> Vec<u8> {
    let text = "89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082";
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16).expect("the fixture is lowercase hex")
        })
        .collect()
}

/// One successful `screen.capture` from the fake device: the real channel carries the JSON-RPC
/// envelope, whose `result` is the MCP content list.
fn screen_capture_result(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{
                "type": "text",
                "text": json!({
                    "format": "png",
                    "data": ONE_PIXEL_PNG,
                    "width": 1,
                    "height": 1,
                }).to_string()
            }],
            "isError": false
        }
    })
}

async fn open_events(server: &TestServer, session: &str) -> tokio::net::TcpStream {
    use tokio::io::AsyncWriteExt;
    let mut socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("a connection to the test server");
    let request = format!(
        "GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n{}\r\n",
        bearer(session)
    );
    socket
        .write_all(request.as_bytes())
        .await
        .expect("the request is written");
    socket
}

#[tokio::test]
async fn the_activity_stream_opens_with_a_humanised_snapshot() {
    let fixture = Fixture::new("activity-snapshot");
    let server = TestServer::start(&fixture.console).await;
    let session = fixture.authenticate().await;
    fixture.enroll(&session).await;
    register_fixture_device(&fixture, &signing_key(0x51)).await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    assert!(seen.contains("event: snapshot"), "{seen}");
    assert!(
        seen.contains("A machine registered"),
        "the snapshot is sentences, not codes: {seen}"
    );
    assert!(seen.contains("\"sentence\""), "{seen}");
}

#[tokio::test]
async fn a_line_written_after_the_connection_arrives_live() {
    let fixture = Fixture::new("activity-live");
    let server = TestServer::start(&fixture.console).await;
    let session = fixture.authenticate().await;
    let device_id = register_fixture_device(&fixture, &signing_key(0x52)).await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;
    assert!(
        !seen.contains("Issued a grant"),
        "the grant is not written yet: {seen}"
    );

    fixture.create_grant(&session, &device_id, 60).await;
    read_until(&mut socket, &mut seen, "Issued a grant").await;

    assert!(seen.contains("event: activity"), "{seen}");
    assert!(seen.contains("Issued a grant"), "{seen}");
}

#[tokio::test]
async fn a_proxied_call_reaches_the_stream_as_a_sentence() {
    let fixture = Fixture::new("activity-call");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x53)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 3,
                        "method": "tools/call",
                        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    read_until(&mut socket, &mut seen, "Read `notes.txt`").await;
    assert!(seen.contains("Read `notes.txt`"), "{seen}");
    assert!(seen.contains("\"ok\":true"), "{seen}");
}

/// A call that is still waiting is announced before its answer, and the announcement is not a line
/// in the audit: the record holds finished calls, while the band is what says one is running.
#[tokio::test]
async fn a_held_call_is_announced_before_its_answer_and_never_audited() {
    let fixture = Fixture::new("activity-started");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x5a)).await;
    let session = fixture.authenticate().await;
    // The standard route is the AI client's: the started frame names `client`, not `console`.
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;
    let audited_before = audit_lines(&fixture).len();

    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": "files.read", "arguments": { "path": "held.txt" } },
    });
    let call_device = device_id.clone();
    let call_grant = grant.clone();
    let app = fixture.app();
    let call =
        tokio::spawn(
            async move { send(app, post_standard(&call_device, &call_grant, request)).await },
        );

    let frame = next_json(&mut device).await;
    read_until(&mut socket, &mut seen, "event: started").await;

    let started = last_event(&seen, "started");
    assert_eq!(started["caller"], json!("client"), "{started}");
    assert_eq!(started["started"]["device_id"], json!(device_id));
    assert_eq!(started["started"]["tool"], json!("files.read"));
    assert_eq!(started["started"]["at_ms"], json!(fixture.now_ms()));
    let call_id = started["started"]["id"]
        .as_str()
        .expect("the started frame names the call")
        .to_owned();
    assert!(!call_id.is_empty(), "{started}");
    assert!(
        !seen.contains("event: activity"),
        "the activity frame waits for the device's answer: {seen}"
    );

    // The announcement is a live frame, not an audit line: the file has not grown while the call is
    // held, and it never gains a `started` line.
    assert_eq!(
        audit_lines(&fixture).len(),
        audited_before,
        "the started frame must never be written to the audit"
    );

    answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    read_until(&mut socket, &mut seen, "event: finished").await;
    let finished = last_event(&seen, "finished");
    assert_eq!(finished["finished"]["id"], json!(call_id));
    let activity_at = seen
        .find("event: activity")
        .unwrap_or_else(|| panic!("the answer is the activity frame: {seen}"));
    let finished_at = seen
        .rfind("event: finished")
        .unwrap_or_else(|| panic!("the clear follows the answer: {seen}"));
    assert!(
        activity_at < finished_at,
        "the finished frame follows the reply: {seen}"
    );

    let lines = audit_lines(&fixture);
    assert_eq!(lines.len(), audited_before + 1, "{lines:?}");
    let text = audit_text(&fixture);
    for word in ["started", "finished"] {
        assert!(
            !text.contains(word),
            "a {word} frame is live, never an audit line: {text}"
        );
    }
}

/// A `finished` frame that never arrives cannot leave a running call forever. The snapshot applies
/// the proxy's own ninety-second cut, so a reconnect cannot resurrect the entry either.
#[tokio::test]
async fn a_started_entry_older_than_the_cut_is_dropped() {
    let fixture = Fixture::new("activity-started-cut");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x5b)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let call_device = device_id.clone();
    let call_session = session.clone();
    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &call_session,
                json!({
                    "device_id": call_device,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 12,
                        "method": "tools/call",
                        "params": { "name": "files.read", "arguments": { "path": "held.txt" } },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    read_until(&mut socket, &mut seen, "event: started").await;
    let call_id = last_event(&seen, "started")["started"]["id"]
        .as_str()
        .expect("the started frame names the call")
        .to_owned();

    // A page that reconnects while the call is fresh sees it in the snapshot: the registry is the
    // in-flight call, not the audit file (which holds nothing until the answer).
    let mut fresh = open_events(&server, &session).await;
    let mut fresh_seen = String::new();
    read_until(&mut fresh, &mut fresh_seen, "event: snapshot").await;
    let snapshot = last_event(&fresh_seen, "snapshot");
    assert!(
        snapshot["running"]
            .as_array()
            .expect("the snapshot carries the calls still running")
            .iter()
            .any(|entry| entry["id"] == json!(call_id)),
        "a fresh page is told the call is running: {snapshot}"
    );

    // Past the proxy's cut a dropped `finished` may not leave the entry running forever.
    fixture.advance_ms(90_001);
    let mut stale = open_events(&server, &session).await;
    let mut stale_seen = String::new();
    read_until(&mut stale, &mut stale_seen, "event: snapshot").await;
    let snapshot = last_event(&stale_seen, "snapshot");
    assert_eq!(
        snapshot["running"],
        json!([]),
        "an entry older than the proxy's cut is dropped: {snapshot}"
    );

    answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_proxied_call_carries_its_evidence_to_the_stream() {
    let fixture = Fixture::new("activity-evidence");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x54)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 4,
                        "method": "tools/call",
                        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    answer(&mut device, &frame, tool_result(json!({ "content": "hi" }))).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    // The row's proof rides on the event itself: the call's own arguments, bounded at proxy time and
    // never written to the audit file. The path the caller sent is what the row will open to.
    read_until(&mut socket, &mut seen, "\"evidence\"").await;
    assert!(seen.contains("\"evidence\""), "{seen}");
    assert!(
        seen.contains("\"evidence\":{\"path\":\"notes.txt\"}"),
        "the live event carries the arguments the caller sent: {seen}"
    );
}

/// A screen capture is evidence, not only a sentence: the row names a path, the same operator session
/// that reads the page can open it, and the bytes are the device's own PNG.
#[tokio::test]
async fn a_screen_capture_carries_a_shot_the_operator_can_open() {
    let fixture = Fixture::new("activity-shot");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x58)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let from = seen.len();
    let app = fixture.app();
    let call_device = device_id.clone();
    let call_session = session.clone();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &call_session,
                json!({
                    "device_id": call_device,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 5,
                        "method": "tools/call",
                        "params": { "name": "screen.capture", "arguments": {} },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    assert_eq!(
        frame["mcp"]["params"]["name"],
        json!("screen.capture"),
        "{frame}"
    );
    answer(&mut device, &frame, screen_capture_result(5)).await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK);

    read_until_after(&mut socket, &mut seen, "\"shot\"", from).await;
    let shot = last_shot(&seen).unwrap_or_else(|| panic!("the event carries a shot: {seen}"));
    let prefix = format!("/activity/shot/{device_id}/");
    assert!(shot.starts_with(&prefix), "{shot}");
    assert!(
        shot.len() > prefix.len(),
        "the path names an image id: {shot}"
    );
    assert!(
        seen.get(from..).is_some_and(
            |event| event.contains(&format!("\"captured_at_ms\":{}", fixture.now_ms()))
        ),
        "the event says when the frame was taken: {seen}"
    );

    // The operator's own session is the gate: with it, the exact bytes and a private, short cache.
    let (status, headers, bytes) = send_bytes(fixture.app(), get(&shot, Some(&session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, max-age=60")
    );
    assert_eq!(bytes, one_pixel_png(), "the served image is the device's");

    // Without a credential the route must not hand out the image at all.
    let (anonymous, _, _) = send_bytes(fixture.app(), get(&shot, None)).await;
    assert!(
        matches!(anonymous, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
        "an unauthenticated shot is refused, not a broken image: {anonymous}"
    );
}

/// Four frames per device is the bound, and the fifth capture evicts the first one's id. The evicted
/// id stays a 404 with a JSON error rather than a broken image.
#[tokio::test]
async fn five_screen_captures_keep_four_and_evict_the_first() {
    let fixture = Fixture::new("activity-shot-eviction");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x59)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let mut shots = Vec::new();
    for capture in 0..5u64 {
        let from = seen.len();
        let app = fixture.app();
        let request = post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": &device_id,
                "request": {
                    "jsonrpc": "2.0",
                    "id": 10 + capture,
                    "method": "tools/call",
                    "params": { "name": "screen.capture", "arguments": {} },
                },
            }),
        );
        let call = tokio::spawn(async move { send(app, request).await });

        let frame = next_json(&mut device).await;
        answer(&mut device, &frame, screen_capture_result(10 + capture)).await;
        let (status, _) = call.await.expect("the call finished");
        assert_eq!(status, StatusCode::OK);

        read_until_after(&mut socket, &mut seen, "\"shot\"", from).await;
        shots.push(last_shot(&seen).unwrap_or_else(|| panic!("capture {capture}: {seen}")));
    }
    assert_eq!(shots.len(), 5);

    let (evicted, body) = send(fixture.app(), get(&shots[0], Some(&session))).await;
    assert_eq!(
        evicted,
        StatusCode::NOT_FOUND,
        "the first frame was evicted"
    );
    assert!(
        body.get("error").and_then(Value::as_str).is_some(),
        "an evicted id is a JSON error, never a broken image: {body}"
    );
    for shot in &shots[1..] {
        let (status, _, bytes) = send_bytes(fixture.app(), get(shot, Some(&session))).await;
        assert_eq!(status, StatusCode::OK, "{shot}");
        assert_eq!(bytes, one_pixel_png(), "{shot}");
    }
}

/// A device refusal is HTTP 200 with a JSON-RPC error body, so the status alone would make a refused
/// call look green. The live event carries the device's own reason and says it was not a success.
#[tokio::test]
async fn a_device_refusal_reaches_the_stream_as_a_failure_with_its_reason() {
    let fixture = Fixture::new("activity-refusal");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, _token, mut device) =
        connected_device(&fixture, &server, &signing_key(0x57)).await;
    let session = fixture.authenticate().await;

    let mut socket = open_events(&server, &session).await;
    let mut seen = String::new();
    read_until(&mut socket, &mut seen, "event: snapshot").await;

    let app = fixture.app();
    let call = tokio::spawn(async move {
        send(
            app,
            post_json_bearer(
                "/mcp",
                &session,
                json!({
                    "device_id": device_id,
                    "request": {
                        "jsonrpc": "2.0",
                        "id": 8,
                        "method": "tools/call",
                        "params": { "name": "files.read", "arguments": { "path": "notes.txt" } },
                    },
                }),
            ),
        )
        .await
    });

    let frame = next_json(&mut device).await;
    answer_error(
        &mut device,
        &frame,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "error": { "code": -32001, "message": "tool not permitted by grant" }
        }),
    )
    .await;
    let (status, _) = call.await.expect("the call finished");
    assert_eq!(status, StatusCode::OK, "a JSON-RPC error stays HTTP 200");

    read_until(&mut socket, &mut seen, "\"ok\":false").await;
    assert!(
        seen.contains("\"ok\":false"),
        "the refusal is a failure: {seen}"
    );
    assert!(
        seen.contains("not permitted"),
        "the device's own reason reaches the page: {seen}"
    );
    assert!(
        seen.contains("\"live\":true"),
        "a proxied call is a live event, not an audit row: {seen}"
    );
}
