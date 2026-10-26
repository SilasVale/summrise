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
use summrise_console::{Config, Console};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};
use tower::ServiceExt;

const OPERATOR_TOKEN: [u8; 32] = [0x11; 32];
const GRANT_SEED: [u8; 32] = [0x22; 32];
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
        send(
            self.app(),
            post_json_bearer(
                "/tokens",
                session,
                json!({
                    "name": name,
                    "device_id": device_id,
                    "tools": tools,
                    "allow_destructive": allow_destructive,
                }),
            ),
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
async fn the_operator_token_yields_a_session_that_expires_in_24_hours() {
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
    assert_eq!(body["expires_at_ms"], json!(START_MS + DAY_MS));
}

#[tokio::test]
async fn an_expired_session_is_401() {
    let fixture = Fixture::new("expired-session");
    let session = fixture.authenticate().await;

    let (status, _) = send(fixture.app(), get("/fleet", Some(&session))).await;
    assert_eq!(status, StatusCode::OK, "the session starts out valid");

    fixture.advance_ms(DAY_MS);
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

    let canonical = summrise_console::canonical_message(&summrise_console::Grant {
        device_id: device_id.clone(),
        tools: tools.clone(),
        expires_at_ms: START_MS + 3_600_000,
        allow_destructive: true,
    });
    assert_eq!(
        canonical,
        format!(
            "grant\n{device_id}\n{}\n{}\ntrue",
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

#[tokio::test]
async fn health_reports_status_device_count_and_uptime_without_auth() {
    let fixture = Fixture::new("health");

    let (status, body) = send(fixture.app(), get("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], json!("ok"));
    assert_eq!(body["devices"], json!(0));
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
            4,
            "exactly the four canonical fields: {facts:?}"
        );
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
            "grant\n{delivered_device}\n{}\n{delivered_expiry}\n{delivered_consent}",
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
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let started = std::time::Instant::now();
    let (status, headers, body) = send_with_headers(
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
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let mut calls = Vec::new();
    for _ in 0..200 {
        let app = fixture.app();
        let request = post_json_bearer(
            "/mcp",
            &session,
            json!({
                "device_id": device_id,
                "grant": grant,
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
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let mut outcome = None;
    for _ in 0..40 {
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
            json!("enroll")
        ],
        "one line per operator action, newest first"
    );
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
            json!("enroll"),
            json!("register"),
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
    assert_eq!(kinds, vec![json!("enroll"), json!("register")]);

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
        2,
        "the registration's enroll and register lines are the only audit lines"
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
            assert_eq!(value["kind"], json!("denied"));
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
    assert_eq!(lines[0]["kind"], json!("denied"));
}

#[tokio::test]
async fn a_request_that_arrives_between_channels_waits_for_the_next_one() {
    let fixture = Fixture::new("channel-reconnect-wait");
    let server = TestServer::start(&fixture.console).await;
    let (device_id, token) = fixture.register(&signing_key(0xb1)).await;
    let session = fixture.authenticate().await;
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id.clone(),
            "grant": grant,
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
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id.clone(),
            "grant": grant,
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
    let grant = fixture.create_grant(&session, &device_id, 60).await;

    let app = fixture.app();
    let request = post_json_bearer(
        "/mcp",
        &session,
        json!({
            "device_id": device_id,
            "grant": grant,
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
    assert_eq!(audit.len(), 1, "one authenticated action, one audit line");
    assert_eq!(audit[0]["kind"], json!("enroll"));
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

// ---- the operator page ------------------------------------------------------------------------

#[tokio::test]
async fn the_operator_page_is_served_as_html() {
    let fixture = Fixture::new("page-served");

    let (status, headers, page) = send_text(fixture.app(), get("/", None)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html")),
        "the operator page is HTML, not JSON: {headers:?}"
    );
    assert!(
        page.contains("<!doctype html>") && page.contains("<html"),
        "the response is the page itself: {page}"
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

    // And the page really does call it: a page that silently fell back to a stale copy would pass
    // every other test here.
    let (status, _, page) = send_text(fixture.app(), get("/", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("/tools"),
        "the page asks the console for the catalogue"
    );
}

#[tokio::test]
async fn the_tool_catalogue_is_not_served_without_an_operator() {
    let fixture = Fixture::new("page-tools-gate");
    let (status, _) = send(fixture.app(), get("/tools", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
fn the_page_is_the_built_bundle_and_needs_nothing_from_outside() {
    let page = include_str!("../web/dist/index.html");

    assert!(
        page.contains("id=\"root\""),
        "the served page is the React bundle's mount point"
    );
    assert!(
        !page.contains("device-cards"),
        "the hand-written page is not what ships any more"
    );
    assert!(
        page.contains("Connect an AI client"),
        "the built page carries the AI-client card; rebuild it with scripts/build-web.sh when its sources change"
    );
    // The bundle carries XML namespace URIs and React's documentation links in strings; neither is a
    // request. What would be a request is a load, so that is what this looks for.
    for needle in [
        "src=\"http",
        "href=\"http",
        "url(http",
        "@import",
        "fetch(\"http",
        "import(\"http",
    ] {
        assert!(
            !page.contains(needle),
            "the page must not load {needle} — the console serves it alone"
        );
    }
}

#[tokio::test]
async fn the_page_holds_no_secret_and_says_the_token_stays_in_memory() {
    let fixture = Fixture::new("page-secret");
    let (status, _, page) = send_text(fixture.app(), get("/", None)).await;
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
    let minted_at = fixture.now_ms();

    let request = json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list" });
    let http = send(
        fixture.app(),
        post_token(&device_id, &token, request.clone()),
    );
    let device_side = async {
        let frame = next_json(&mut device).await;
        assert_eq!(frame["mcp"], request);
        let grant: summrise_console::Grant =
            serde_json::from_value(frame["grant"]["grant"].clone())
                .expect("the frame carries a grant");
        assert_eq!(grant.device_id, device_id);
        assert_eq!(
            grant.tools,
            vec!["files.read".to_owned(), "memory.search".to_owned()],
            "the grant names exactly the token's tools"
        );
        assert!(
            grant.allow_destructive,
            "the consent is the token's, not the caller's"
        );
        assert_eq!(
            grant.expires_at_ms,
            minted_at + 60_000,
            "a minted grant lives sixty seconds"
        );
        let message = summrise_console::canonical_message(&grant);
        let signature =
            decode_signature(frame["grant"]["signature"].as_str().expect("a signature"));
        SigningKey::from_bytes(&GRANT_SEED)
            .verifying_key()
            .verify_strict(message.as_bytes(), &signature)
            .expect("the device can verify the minted grant offline");
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
