//! The Windows lifecycle, against the real binaries (design §4.1, §4.4, §8).
//!
//! The unit tests prove the decisions; this file proves the *effects*: a temp install with a
//! verified `baseline\current\` generation, the launcher supervising the app, the app answering
//! `/api/health` and `/api/state`, a real ConPTY session created, typed at and closed through
//! `/mcp`, a real headless Chrome listed, opened, evaluated, navigated, captured, closed and
//! stopped through the same endpoint, the launcher restoring a deleted binary from `current`, a real
//! update staged by `summrise-device --stage` and applied by the running supervisor, a validly-
//! signed update that cannot serve being rolled back to the running generation, and the lane's
//! signed `release.json` fetched over loopback and applied — with the canary and downgrade documents
//! a stable device must refuse. It then proves the SCM service mode once through `sc.exe`, the
//! product installer's seed/migration and token minting, and the per-install supervisor lock.
//!
//! **A test that needs the SCM or the product service is `#[ignore]`d, not conditionally skipped**
//! (I8): the CI Windows job runs each by exact name with `--ignored`, so deleting the CI step turns
//! the proof red instead of silently green.
//!
//! The update path is the product path (fix design F1/F3/F8): the app writes the canonical staged
//! pair and `swap.request.json`, the supervisor validates it, stops its child, swaps, probes
//! `/api/health` twice and writes `swap.result.json`. `--apply` is operator-only; one test drives it
//! against a console install (never the SCM service) to prove its rollback result carries the
//! failed artifact's digest, the same fact the app records from the product path.
//!
//! ── THE FIXTURE KEYS, AND WHY THE BUILD, NOT THE TEST, CARRIES THEM ─────────────────────────────
//!
//! Both binaries read keys through `option_env!` at COMPILE time, on purpose: a key a running
//! service could replace would make the signature check optional. So the test cannot inject a key
//! into a shipped binary — it has to be built with one. The two pairs below are RFC 8032's test
//! vectors 1 and 2, which are publicly documented, are nobody's release key, and are the only keys
//! this file ever signs with. They are **different keys on purpose (C1)**: the launcher key owns the
//! seed sidecar and the baseline pair; the release key owns release documents and the staged update
//! sidecar the app writes from one. The contract is:
//!
//!   * the CI Windows job builds with `SUMMRISE_LAUNCHER_PUBKEY` set to
//!     [`FIXTURE_LAUNCHER_PUBKEY_HEX`] and `SUMMRISE_RELEASE_PUBKEY` set to
//!     [`FIXTURE_RELEASE_PUBKEY_HEX`], then runs this file;
//!   * the launcher's embedded launcher key has a public const, so it is compared directly;
//!   * the device has no such const (it is a binary), so its release key is compared by BEHAVIOUR:
//!     [`require_fixture_key`] makes it verify a release-key signature, and the lifecycle then
//!     stages and applies a release-key-signed release end to end. A build with another key fails at
//!     the first signature check, and a build with no key fails by name.
//!
//! No fixture is optional: a missing key or a missing binary is a panic with a message, because a
//! green run that skipped the product is the one result this repository refuses.
//!
//! ── WHAT THIS FILE DOES NOT COVER ───────────────────────────────────────────────────────────────
//!
//! The browser lane's shutdown proof is the test's, not the MCP surface's: `browser.list` is lazy
//! by design (a call after `shutdown` starts a second Chrome), and no verb exposes the pid. The
//! lane therefore reads the `chrome.exe` command lines with `Get-CimInstance` and requires every
//! process naming the fixture's unique profile directory to be gone. A real SCM *restart* after a
//! supervisor crash is not triggered here (the restart policy is read back with `sc qfailure`
//! instead): forcing it would mean crashing the SCM's own service under the runner. The swap's
//! time-of-check/time-of-use re-hash and the path-injection refusals are unit tests in the crates.

#![cfg(windows)]

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signer, SigningKey};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};

use summrise_core::manifest::Manifest;
use summrise_core::{SessionKind, State, VerdictOutcome, SCHEMA_VERSION};
use summrise_launch::{
    app_exe, app_pid_file, baseline_dir, baseline_exe, baseline_sidecar, ensure_baseline, flat_exe,
    flat_sidecar, generation_exe, generation_sidecar, hex_encode, install_token, load_sidecar,
    save_sidecar, sha256_bytes, sha256_file, staged_exe, staged_sidecar, swap_request, swap_result,
    BaselineInstall, Sidecar, SigError, SwapOutcome, SwapResult, LAUNCHER_PUBKEY_HEX, PREVIOUS_DIR,
};

/// The SCM service `sc.exe` creates for the one service-mode proof. Deliberately NOT
/// `SummriseLaunch`: that name belongs to the product's installer, and this test proves that a
/// service registered by something else can still run `summrise-launch service`.
const SERVICE_TEST_NAME: &str = "SummriseDeviceTest";

/// The service name the product installer registers. It is the launcher's fixed name; the test
/// spells it once so `sc qfailure` queries the same service the installer configured.
const SERVICE_NAME_FOR_PRODUCT: &str = "SummriseLaunch";

/// RFC 8032 §7.1 test vector 1's seed — the **launcher** key: the seed sidecar and the baseline pair.
const FIXTURE_LAUNCHER_SEED_HEX: &str =
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
/// The public half of [`FIXTURE_LAUNCHER_SEED_HEX`]; CI compiles it into the launcher as
/// `SUMMRISE_LAUNCHER_PUBKEY`.
const FIXTURE_LAUNCHER_PUBKEY_HEX: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

/// RFC 8032 §7.1 test vector 2's seed — the **release** key: release documents and the staged
/// sidecar the app writes from one. Deliberately a different key from the launcher's: using one key
/// for both was how C1 hid.
const FIXTURE_RELEASE_SEED_HEX: &str =
    "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
/// The public half of [`FIXTURE_RELEASE_SEED_HEX`]; CI compiles it into the device and the launcher
/// as `SUMMRISE_RELEASE_PUBKEY`.
const FIXTURE_RELEASE_PUBKEY_HEX: &str =
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";

/// The version of the installed baseline generation, and the version the launcher passes to the app.
const BASELINE_VERSION: &str = "0.1.0";
/// The version of the update that must apply.
const UPDATED_VERSION: &str = "0.2.0";
/// The version of the validly-signed artifact (`cmd.exe`) that cannot serve and must roll back.
const ROLLBACK_VERSION: &str = "0.3.0";
/// The version a signed release document names, and the lane must apply end to end.
const RELEASE_VERSION: &str = "0.4.0";
/// The version on the channel a stable device must not install, even when it is signed.
const CANARY_VERSION: &str = "0.5.0";
/// The version below the installed release: a signed, stable document that is still a downgrade.
const DOWNGRADE_VERSION: &str = "0.3.0";

/// How long a freshly started launcher may take to bring the app's health endpoint up. The
/// supervisor itself allows 10 s per probe; this leaves room for the port to be released.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a stopped launcher may take to take its app child with it.
const CHILD_STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// The line the ConPTY lane types into a real console. It has to reach the console at least twice:
/// the console echoes the typed command once and prints the command's output once (see
/// [`console_printed`]).
const CONPTY_MARKER: &str = "SUMMRISE_CONPTY_OK";
/// How long a fresh console gets to run the typed command and print its output.
const CONPTY_TIMEOUT: Duration = Duration::from_secs(20);
/// How often the ConPTY lane polls the console's output while it waits for the marker.
const CONPTY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The title the browser lane loads in a `data:` document and reads back through `browser.eval`.
const BROWSER_TITLE: &str = "SUMMRISE_CDP_OK";
/// How long a closed tab may remain in `browser.list`; Chrome's target list lags a close a moment.
const BROWSER_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a stopped browser's processes get to exit before the lane calls them leftovers.
const BROWSER_STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// The Windows Chrome executables the app discovers, in the capability's order, after
/// `SUMMRISE_CHROME`. The lane checks the same list so its failure names every path it searched.
const WIN_CHROME_CANDIDATES: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

static NEXT: AtomicU32 = AtomicU32::new(0);

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The fixture key
// ─────────────────────────────────────────────────────────────────────────────────────────────────

fn decode_hex32(hex: &str) -> [u8; 32] {
    assert_eq!(hex.len(), 64, "a fixture key is 32 bytes of hex: {hex:?}");
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .unwrap_or_else(|_| panic!("the fixture key is not hex: {hex:?}"));
    }
    bytes
}

fn launcher_signing_key() -> SigningKey {
    SigningKey::from_bytes(&decode_hex32(FIXTURE_LAUNCHER_SEED_HEX))
}

fn release_signing_key() -> SigningKey {
    SigningKey::from_bytes(&decode_hex32(FIXTURE_RELEASE_SEED_HEX))
}

/// Was a launcher key supplied at build time at all? `option_env!` is this crate's compile-time
/// seam, the same one the launcher uses, so this is the test's view of the launcher's own key.
fn launcher_key_was_configured() -> bool {
    option_env!("SUMMRISE_LAUNCHER_PUBKEY").is_some()
}

/// Is the launcher's embedded key the launcher fixture key? A build with a *different* key is a
/// misconfiguration, not a skip: the seed sidecars below would be refused and the failure would look
/// like a product bug.
fn fixture_key_is_embedded() -> bool {
    LAUNCHER_PUBKEY_HEX.eq_ignore_ascii_case(FIXTURE_LAUNCHER_PUBKEY_HEX)
}

/// Panic unless THIS build can verify the two fixtures' signatures — and prove both binaries carry
/// the fixture key each one needs, not merely a key that some other configuration produced.
///
/// The launcher is checked directly through its public const for the launcher key. The device is a
/// binary with no library target, so its release key has no const this test can import; the
/// equivalent fact is behavioural: a release document signed with the release key must fail at the
/// *digest* check, which is reachable only after a successful signature check with that key.
fn require_fixture_key() {
    assert!(
        launcher_key_was_configured(),
        "this launcher was built without a launcher key, so it cannot verify a seed sidecar. \
         Rebuild with SUMMRISE_LAUNCHER_PUBKEY={FIXTURE_LAUNCHER_PUBKEY_HEX} and \
         SUMMRISE_RELEASE_PUBKEY={FIXTURE_RELEASE_PUBKEY_HEX} (the CI Windows job sets two \
         different keys)."
    );
    assert!(
        fixture_key_is_embedded(),
        "the launcher embedded {LAUNCHER_PUBKEY_HEX}, which is not the launcher fixture key \
         {FIXTURE_LAUNCHER_PUBKEY_HEX}. Rebuild with \
         SUMMRISE_LAUNCHER_PUBKEY={FIXTURE_LAUNCHER_PUBKEY_HEX}."
    );
    assert_eq!(
        hex_encode(launcher_signing_key().verifying_key().as_bytes()),
        FIXTURE_LAUNCHER_PUBKEY_HEX,
        "the launcher fixture seed and public key have drifted apart"
    );
    assert_eq!(
        hex_encode(release_signing_key().verifying_key().as_bytes()),
        FIXTURE_RELEASE_PUBKEY_HEX,
        "the release fixture seed and public key have drifted apart"
    );
    assert_ne!(
        FIXTURE_LAUNCHER_PUBKEY_HEX, FIXTURE_RELEASE_PUBKEY_HEX,
        "the two key seams must use different fixture keys, or this lane cannot see C1"
    );

    // The device's half of the release-key seam. The paths are deliberately fresh and the manifest
    // is signed over bytes the artifact does not contain, so the ONLY way to reach the digest
    // refusal is a successful signature check with the release public key.
    let dir = TempDir::new("device-key");
    let artifact = dir.join("artifact.exe");
    let manifest_path = dir.join("release.json");
    let install = dir.join("install");
    fs::write(&artifact, b"an artifact the manifest does not describe")
        .expect("write the key probe's artifact");
    let manifest = signed_manifest(
        UPDATED_VERSION,
        "stable",
        b"the bytes the manifest describes",
        BASELINE_VERSION,
    );
    write_manifest(&manifest_path, &manifest);

    let output = Command::new(device_exe())
        .arg("--stage")
        .arg(&artifact)
        .arg(&manifest_path)
        .arg("--install")
        .arg(&install)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("cannot run summrise-device --stage: {error}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "the device must refuse the key probe with the usual staging failure; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("does not hash to the manifest's sha256"),
        "the device did not verify a release-key signature: it stopped before the digest check. \
         Rebuild the device with SUMMRISE_RELEASE_PUBKEY={FIXTURE_RELEASE_PUBKEY_HEX}. \
         stderr was:\n{stderr}"
    );
    assert!(
        !staged_exe(&install).exists(),
        "a refused stage must not write the canonical pair"
    );
}

/// The operator-only `--apply` test drives a console install, not the product's SCM service. If the
/// service is installed, `--apply` would stop and start *that* install instead of the fixture, so
/// the precondition is asserted rather than silently skipped: the CI runner has no product service,
/// and a host that does must not run this test by accident.
fn require_no_product_service() {
    let query = sc(&["query", SERVICE_NAME_FOR_PRODUCT]);
    assert!(
        !query.status.success(),
        "this test proves the console-install `--apply` path; {SERVICE_NAME_FOR_PRODUCT} is \
         installed on this host, and the operator path would touch it instead of the fixture. Run \
         this test on a host without the product service (the CI Windows runner)."
    );
}

/// The sidecar the launcher accepts as a **seed/baseline**: a launcher-key signature over exactly
/// `version\nchannel\nsha256` (design §4.4). This is the same shape the app's stager writes; the
/// test crafts the baseline's directly because an installer, not the app, writes the first
/// generation.
fn signed_sidecar(version: &str, channel: &str, sha256: &str) -> Sidecar {
    let message = format!("{version}\n{channel}\n{sha256}");
    let signature = launcher_signing_key().sign(message.as_bytes());
    Sidecar {
        version: version.to_owned(),
        channel: channel.to_owned(),
        sha256: sha256.to_owned(),
        signature: hex_encode(&signature.to_bytes()),
    }
}

/// The sidecar an **update** carries: signed by the release key, the one the app verified the
/// manifest with. The launcher's staged check must accept this and refuse the launcher-key shape.
fn release_signed_sidecar(version: &str, channel: &str, sha256: &str) -> Sidecar {
    let message = format!("{version}\n{channel}\n{sha256}");
    let signature = release_signing_key().sign(message.as_bytes());
    Sidecar {
        version: version.to_owned(),
        channel: channel.to_owned(),
        sha256: sha256.to_owned(),
        signature: hex_encode(&signature.to_bytes()),
    }
}

/// The manifest `summrise-device --stage` consumes (design §6): the same signed message, plus the
/// minimum version the stager applies after the signature and digest. Signed with the release key,
/// because that is the key the device trusts for updates.
fn signed_manifest(version: &str, channel: &str, bytes: &[u8], min_version: &str) -> Manifest {
    let sha256 = sha256_bytes(bytes);
    let message = format!("{version}\n{channel}\n{sha256}");
    let signature = release_signing_key().sign(message.as_bytes());
    Manifest {
        version: version.to_owned(),
        channel: channel.to_owned(),
        sha256,
        signature: hex_encode(&signature.to_bytes()),
        min_version: min_version.to_owned(),
    }
}

fn write_manifest(path: &Path, manifest: &Manifest) {
    let document = serde_json::to_vec_pretty(manifest).expect("a release manifest serializes");
    fs::write(path, document)
        .unwrap_or_else(|error| panic!("cannot write the manifest {}: {error}", path.display()));
}

/// The release document `summrise-device --check-update` fetches (design §4.4, §6): the policy the
/// device verifies, the artifact's URL and the stager's older three-part signature.
///
/// The product parses this shape with `deny_unknown_fields` and `rename_all = "camelCase"`, so a
/// field spelled differently here is a refusal by name rather than a fact both sides quietly
/// dropped. Signing it here (instead of calling the release tool) is deliberate: the lane must
/// exercise the downloader's verifier, not a document the verifier's own crate produced.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseFixture {
    version: String,
    channel: String,
    sha256: String,
    artifact_url: String,
    min_version: String,
    issued_at_ms: u64,
    artifact_signature: String,
    signature: String,
}

fn signed_release(
    version: &str,
    channel: &str,
    artifact_url: &str,
    bytes: &[u8],
    min_version: &str,
) -> ReleaseFixture {
    let sha256 = sha256_bytes(bytes);
    let issued_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    // The device verifies two signatures: the policy message over every fact it acts on, and the
    // stager's older three-part artifact message. The lane signs both with the release key — the
    // key the device embeds and the launcher's staged check must use.
    let artifact_message = format!("{version}\n{channel}\n{sha256}");
    let policy_message =
        format!("{version}\n{channel}\n{sha256}\n{artifact_url}\n{min_version}\n{issued_at_ms}");
    let artifact_signature = release_signing_key().sign(artifact_message.as_bytes());
    let signature = release_signing_key().sign(policy_message.as_bytes());
    ReleaseFixture {
        version: version.to_owned(),
        channel: channel.to_owned(),
        sha256,
        artifact_url: artifact_url.to_owned(),
        min_version: min_version.to_owned(),
        issued_at_ms,
        artifact_signature: hex_encode(&artifact_signature.to_bytes()),
        signature: hex_encode(&signature.to_bytes()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// Paths the test drives
// ─────────────────────────────────────────────────────────────────────────────────────────────────

fn launcher_exe() -> &'static str {
    env!("CARGO_BIN_EXE_summrise-launch")
}

/// `%SystemRoot%\System32\<name>`, so a test never depends on PATH ordering.
fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(root).join("System32").join(name)
}

/// The `summrise-device.exe` that `cargo build --workspace` produced.
///
/// An integration test's own executable lives in `target\<profile>\deps\`, while binaries live one
/// directory up in `target\<profile>\`. Both are checked — `current_exe()`'s directory first, as the
/// brief says, then its parent — and `CARGO_BIN_EXE_summrise-launch`'s directory covers a custom
/// `CARGO_TARGET_DIR`. A missing binary is a build-order error and says so, because a test that
/// silently skips the product is the green this repository refuses.
fn device_exe() -> PathBuf {
    let test_exe = std::env::current_exe().expect("resolve this test's own path");
    let test_dir = test_exe.parent().expect("the test binary has a directory");
    let mut candidates = vec![
        test_dir.join("summrise-device.exe"),
        test_dir.join("..").join("summrise-device.exe"),
    ];
    if let Some(bin_dir) = Path::new(launcher_exe()).parent() {
        candidates.push(bin_dir.join("summrise-device.exe"));
    }
    if let Some(found) = candidates.iter().find(|candidate| candidate.is_file()) {
        return found.clone();
    }
    let looked = candidates
        .iter()
        .map(|candidate| format!("  {}", candidate.display()))
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "cannot find summrise-device.exe. Looked in:\n{looked}\n\
         The lifecycle test drives the built binary, not a test harness, so run\n\
         \n    cargo build --manifest-path device/Cargo.toml --workspace\n\
         \nbefore `cargo test`. The CI Windows job does that first."
    )
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// A small HTTP client: the app speaks HTTP, so the test does too
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// `GET <path>` on loopback, returning `(status, body)`. `Connection: close` keeps the read simple:
/// every response here carries `Content-Length` (a `Json` body), so no chunked decoder is needed.
fn http_get(port: u16, path: &str, token: Option<&str>) -> Result<(u16, String), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("connect 127.0.0.1:{port}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("set the read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("set the write timeout: {error}"))?;

    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}Connection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write the request: {error}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("read the response: {error}"))?;
    let response = String::from_utf8_lossy(&raw);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("the response has no header/body separator: {response:?}"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| format!("the response has no status line: {head:?}"))?;
    Ok((status, body.to_owned()))
}

/// One `/api/health` probe, as a document.
fn probe_health(port: u16) -> Result<Value, String> {
    match http_get(port, "/api/health", None)? {
        (200, body) => serde_json::from_str(&body)
            .map_err(|error| format!("health is not JSON ({error}): {body:?}")),
        (status, body) => Err(format!("/api/health answered {status}: {body:?}")),
    }
}

/// Read and judge the health document against the release version the launcher passed. Returns the
/// pid so a restart can be told from the old app.
fn assert_health(health: &Value, expected_version: &str) -> u32 {
    assert_eq!(
        health["status"].as_str(),
        Some("ok"),
        "the health document: {health}"
    );
    assert_eq!(
        health["version"].as_str(),
        Some(expected_version),
        "health must report the release version from the launcher's argv: {health}"
    );
    let pid = health["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("health has no numeric pid: {health}"));
    assert!(
        pid > 0 && pid <= u32::MAX as u64,
        "health's pid is a process id: {health}"
    );
    pid as u32
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The MCP round-trip: rmcp's own client against the installed app
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// Drive the installed app's `/mcp` endpoint the way a product client does: rmcp's streamable-HTTP
/// client with the install token as its bearer, one `tools/list`, one `files.mkdir` and one
/// `files.write` + `files.read`.
///
/// The fixture deletes nothing and creates no files tree: a fresh data directory is the point. The
/// audit file is asserted from the server's side as six lines — a begin and an end per call, paired
/// by id — so the write is the record of a call that went through `authorize`, not a test shortcut.
fn mcp_files_round_trip(fixture: &Fixture) {
    let workspace = fixture.data.join("workspace");
    assert!(
        workspace.is_dir(),
        "the app creates its files root at startup, before the first MCP call: {}",
        workspace.display()
    );
    assert!(
        !workspace.join("notes").exists(),
        "a fresh install has no files tree for the client to inherit"
    );

    let config = StreamableHttpClientTransportConfig::with_uri(format!(
        "http://127.0.0.1:{}/mcp",
        fixture.port
    ))
    .auth_header(fixture.token.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build the MCP client's runtime");

    runtime.block_on(async {
        let client = ()
            .serve(StreamableHttpClientTransport::from_config(config))
            .await
            .expect("rmcp's client completes the MCP handshake with the installed app");

        let tools = client.list_all_tools().await.expect("tools/list answers");
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        let registry: Vec<&str> = summrise_core::registry::CAPABILITIES
            .iter()
            .flat_map(|capability| capability.tools)
            .map(|tool| tool.name)
            .collect();
        assert_eq!(
            names, registry,
            "the installed app advertises the registry's tools"
        );

        let made = client
            .call_tool(
                CallToolRequestParams::new("files.mkdir").with_arguments(
                    json!({ "path": "notes" })
                        .as_object()
                        .expect("the mkdir arguments are an object")
                        .clone(),
                ),
            )
            .await
            .expect("files.mkdir is answered");
        assert_eq!(
            made.is_error,
            Some(false),
            "files.mkdir succeeded: {made:?}"
        );
        assert!(
            workspace.join("notes").is_dir(),
            "the client's mkdir created the directory inside the startup-created root"
        );

        let content = "written through MCP on Windows";
        let written = client
            .call_tool(
                CallToolRequestParams::new("files.write").with_arguments(
                    json!({ "path": "notes/lane.txt", "content": content })
                        .as_object()
                        .expect("the write arguments are an object")
                        .clone(),
                ),
            )
            .await
            .expect("files.write is answered");
        assert_eq!(
            written.is_error,
            Some(false),
            "files.write succeeded: {written:?}"
        );

        let read = client
            .call_tool(
                CallToolRequestParams::new("files.read").with_arguments(
                    json!({ "path": "notes/lane.txt" })
                        .as_object()
                        .expect("the read arguments are an object")
                        .clone(),
                ),
            )
            .await
            .expect("files.read is answered");
        assert_eq!(read.is_error, Some(false), "files.read succeeded: {read:?}");
        let read_back = read
            .content
            .iter()
            .find_map(|block| block.as_text())
            .map(|text| text.text.as_str());
        assert_eq!(
            read_back,
            Some(content),
            "the bytes read back are the bytes written"
        );

        client
            .cancel()
            .await
            .expect("the MCP client closes cleanly");
    });

    // Two lines per call: the `begin` line is written before the capability runs, the `end` line
    // after it. Three calls (mkdir, write, read) leave six lines whose ids pair in call order.
    let audit = fs::read_to_string(fixture.data.join("events.jsonl"))
        .expect("the MCP calls appended to the audit file");
    let lines: Vec<Value> = audit
        .lines()
        .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
        .collect();
    assert_eq!(lines.len(), 6, "three calls, two lines each: {lines:?}");
    for (pair, tool) in lines
        .chunks(2)
        .zip(["files.mkdir", "files.write", "files.read"])
    {
        let (begin, end) = (&pair[0], &pair[1]);
        assert_eq!(begin["event"], "begin", "{lines:?}");
        assert_eq!(begin["tool"], tool, "{lines:?}");
        assert_eq!(begin["decision"], "allow", "{lines:?}");
        assert_eq!(end["event"], "end", "{lines:?}");
        assert_eq!(end["ok"], true, "{lines:?}");
        assert_eq!(
            begin["id"], end["id"],
            "the begin and end lines name one call: {lines:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The ConPTY round-trip: the same installed app, a real console typed at and read back
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// A tool call's arguments, in the map shape rmcp's client takes.
fn call_arguments(arguments: Value) -> serde_json::Map<String, Value> {
    arguments
        .as_object()
        .expect("the call arguments are a JSON object")
        .clone()
}

/// The text one tool call returned, whether it succeeded or was refused.
fn tool_text(result: &CallToolResult) -> &str {
    result
        .content
        .iter()
        .find_map(|block| block.as_text())
        .map(|text| text.text.as_str())
        .unwrap_or_else(|| panic!("the tool returned a text block: {result:?}"))
}

/// The JSON document one successful tool call returned.
fn tool_json(result: &CallToolResult) -> Value {
    let text = tool_text(result);
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("the tool's text is JSON ({error}): {text:?}"))
}

/// Every audit line on disk, in call order.
fn audit_lines(path: &Path) -> Vec<Value> {
    let audit = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("the audit file {} is readable: {error}", path.display()));
    audit
        .lines()
        .map(|line| serde_json::from_str(line).expect("an audit line is JSON"))
        .collect()
}

/// Assert that `tool` was audited as one allowed call that succeeded: a `begin` line with the call
/// id, and the matching `end` line.
fn assert_audited(lines: &[Value], tool: &str) {
    let begin = lines
        .iter()
        .find(|line| line["event"] == "begin" && line["tool"] == tool)
        .unwrap_or_else(|| panic!("{tool} was audited with a begin line: {lines:?}"));
    assert_eq!(begin["decision"], "allow", "{tool} was allowed: {begin}");
    let end = lines
        .iter()
        .find(|line| line["event"] == "end" && line["id"] == begin["id"])
        .unwrap_or_else(|| panic!("{tool}'s begin line has an end line: {lines:?}"));
    assert_eq!(end["ok"], true, "{tool} succeeded: {end}");
    assert!(
        end.get("error_kind").is_none(),
        "{tool}'s success carries no error kind: {end}"
    );
}

/// Whether the command reached the console: the marker appears at least twice in the raw stream.
///
/// The first occurrence is the console's echo of the typed line; the second is `cmd.exe`'s output.
/// There is no layout to match between them, because there is no stable delimiter: ConPTY echoes
/// the input and then repaints with cursor-position escapes, so the output can be preceded by
/// `ESC[?25l` rather than a newline —
/// `...>echo SUMMRISE_CONPTY_OK\r\nESC[?25lSUMMRISE_CONPTY_OKESC[7;1H...`. The test therefore
/// asserts the observable — the shell ran the command and its output reached the console — by
/// counting occurrences in the raw stream, not a frame.
fn console_printed(text: &str, marker: &str) -> bool {
    text.matches(marker).count() >= 2
}

/// The ids `/api/state` lists as pseudo-terminal sessions.
fn state_pty_ids(fixture: &Fixture) -> Vec<String> {
    let (status, body) =
        http_get(fixture.port, "/api/state", Some(&fixture.token)).expect("query /api/state");
    assert_eq!(status, 200, "the state document is answered: {body}");
    let state = State::parse(&body).unwrap_or_else(|error| {
        panic!("GET /api/state did not answer with a state document ({error}): {body}")
    });
    state
        .sessions
        .iter()
        .filter(|row| row.kind == SessionKind::Pty)
        .map(|row| row.id.clone())
        .collect()
}

/// Drive a real ConPTY through the installed app's `/mcp`: create a `cmd.exe` console session, type
/// one command into it, read the console's own output back with the cursor until it appears, resize
/// it, then close it — proving the session appears in and disappears from `/api/state`, and that a
/// read of the closed id is a typed refusal rather than an empty answer.
///
/// This is the same keyed, non-elevated app and the same rmcp client the files round-trip uses. The
/// marker assertion is what makes it a *console* proof rather than a wire proof: the console echoes
/// the typed `echo SUMMRISE_CONPTY_OK` once and the shell prints `SUMMRISE_CONPTY_OK` once, so the
/// second occurrence can only come from a console that ran the command.
fn mcp_conpty_round_trip(fixture: &Fixture) {
    // The audit is cumulative: remember where the files round-trip stopped so this section asserts
    // exactly the lines its own calls added.
    let audited_before = audit_lines(&fixture.data.join("events.jsonl")).len();

    let config = StreamableHttpClientTransportConfig::with_uri(format!(
        "http://127.0.0.1:{}/mcp",
        fixture.port
    ))
    .auth_header(fixture.token.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build the ConPTY client's runtime");
    // One program path, resolved through `%SystemRoot%` rather than PATH ordering: the capability
    // takes a program, not a command line with arguments.
    let shell = system32("cmd.exe").to_string_lossy().into_owned();

    runtime.block_on(async {
        let client = ()
            .serve(StreamableHttpClientTransport::from_config(config))
            .await
            .expect("rmcp's client completes the MCP handshake with the installed app");

        let created = client
            .call_tool(
                CallToolRequestParams::new("terminal.create").with_arguments(call_arguments(
                    json!({ "kind": "pty", "shell": shell, "rows": 24, "cols": 80 }),
                )),
            )
            .await
            .expect("terminal.create is answered");
        assert_eq!(
            created.is_error,
            Some(false),
            "terminal.create started a console: {created:?}"
        );
        let created = tool_json(&created);
        let id = created["id"]
            .as_str()
            .unwrap_or_else(|| panic!("create returned a session id: {created}"))
            .to_owned();
        assert!(id.starts_with("session-"), "the id is a session id: {id:?}");
        assert_eq!(
            created["kind"].as_str(),
            Some("pty"),
            "the create reports the kind of session it started: {created}"
        );
        assert_eq!(
            created["shell"].as_str(),
            Some(shell.as_str()),
            "the create reports the program it started: {created}"
        );

        // `create` writes the row before it returns, so a reader of the state document must see it
        // now — this is the row `/api/state` lists, not a test-side note.
        assert_eq!(
            state_pty_ids(fixture),
            vec![id.clone()],
            "the new session is the only pty row in state.sessions"
        );

        let command = format!("echo {CONPTY_MARKER}\r\n");
        let written = client
            .call_tool(
                CallToolRequestParams::new("terminal.write")
                    .with_arguments(call_arguments(json!({ "id": id, "data": command }))),
            )
            .await
            .expect("terminal.write is answered");
        assert_eq!(
            written.is_error,
            Some(false),
            "terminal.write reached the console: {written:?}"
        );
        assert_eq!(
            tool_json(&written)["written"].as_u64(),
            Some(command.len() as u64),
            "the whole command line was written"
        );

        // Read from the cursor the previous read returned. Each answer is only the console output
        // produced since that cursor, so the accumulated text is the live stream; a stub that
        // replayed a canned string with a frozen cursor could not grow it line by line.
        let deadline = Instant::now() + CONPTY_TIMEOUT;
        let mut cursor = 0u64;
        let mut output = String::new();
        loop {
            let read = client
                .call_tool(
                    CallToolRequestParams::new("terminal.read")
                        .with_arguments(call_arguments(json!({ "id": id, "cursor": cursor }))),
                )
                .await
                .expect("terminal.read is answered");
            assert_eq!(
                read.is_error,
                Some(false),
                "terminal.read answered the live session: {read:?}"
            );
            let read = tool_json(&read);
            let data = read["data"]
                .as_str()
                .unwrap_or_else(|| panic!("read returned text: {read}"));
            let next = read["cursor"]
                .as_u64()
                .unwrap_or_else(|| panic!("read returned a cursor: {read}"));
            assert!(
                next >= cursor,
                "the console's cursor never moves backwards: {cursor} -> {next}"
            );
            output.push_str(data);
            cursor = next;
            // The marker must reach the console twice: the echo of the typed line and cmd.exe's
            // output. ConPTY repaints with cursor escapes instead of stable line breaks, so the
            // occurrence count in the raw stream is the observable, not a layout.
            if console_printed(&output, CONPTY_MARKER) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the console never showed {CONPTY_MARKER:?} twice (the echo and the output) \
                 within {}s (cursor {cursor}); the raw output was:\n{output:?}",
                CONPTY_TIMEOUT.as_secs()
            );
            tokio::time::sleep(CONPTY_POLL_INTERVAL).await;
        }

        let resized = client
            .call_tool(
                CallToolRequestParams::new("terminal.resize")
                    .with_arguments(call_arguments(json!({ "id": id, "rows": 40, "cols": 120 }))),
            )
            .await
            .expect("terminal.resize is answered");
        assert_eq!(
            resized.is_error,
            Some(false),
            "terminal.resize was accepted: {resized:?}"
        );
        let resized = tool_json(&resized);
        assert_eq!(
            resized["rows"].as_u64(),
            Some(40),
            "the resize applied: {resized}"
        );
        assert_eq!(
            resized["cols"].as_u64(),
            Some(120),
            "the resize applied: {resized}"
        );

        let closed = client
            .call_tool(
                CallToolRequestParams::new("terminal.close")
                    .with_arguments(call_arguments(json!({ "id": id }))),
            )
            .await
            .expect("terminal.close is answered");
        assert_eq!(
            closed.is_error,
            Some(false),
            "terminal.close stopped the console: {closed:?}"
        );
        assert_eq!(
            tool_json(&closed)["closed"],
            true,
            "the close reports success"
        );

        // `close` removes the row before it returns, so the document no longer lists the session.
        let remaining = state_pty_ids(fixture);
        assert!(
            remaining.is_empty(),
            "the closed session is gone from state.sessions: {remaining:?}"
        );

        // The id is not reused: the same operation on it is the capability's typed unknown-session
        // refusal, which the MCP surface reports as a tool error, not a successful empty read.
        let refused = client
            .call_tool(
                CallToolRequestParams::new("terminal.read")
                    .with_arguments(call_arguments(json!({ "id": id }))),
            )
            .await
            .expect("the read of a closed id is answered");
        assert_eq!(
            refused.is_error,
            Some(true),
            "a read of the closed id is refused: {refused:?}"
        );
        let refusal = tool_text(&refused);
        assert!(
            refusal.contains(&id) && refusal.contains("closed"),
            "the refusal names the session and the reason: {refusal:?}"
        );

        client
            .cancel()
            .await
            .expect("the MCP client closes cleanly");
    });

    // Two lines per call. The section's own lines must include the create and close pairs (the
    // write and resize pairs too); the refused read is the one end line carrying an error kind.
    let lines = audit_lines(&fixture.data.join("events.jsonl"));
    let added = lines.get(audited_before..).unwrap_or_else(|| {
        panic!(
            "the audit file kept the earlier lines: {} lines, expected at least {audited_before}",
            lines.len()
        )
    });
    for tool in [
        "terminal.create",
        "terminal.write",
        "terminal.resize",
        "terminal.close",
    ] {
        assert_audited(added, tool);
    }
    assert!(
        added.iter().any(|line| line["event"] == "end"
            && line["ok"] == false
            && line["error_kind"] == "refused"),
        "the refused read is audited as a refusal: {added:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The browser round-trip: the same installed app, a real headless Chrome driven over `/mcp`
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The Chrome the app will launch, or a panic naming every path that was searched.
///
/// The app discovers Chrome itself (`SUMMRISE_CHROME`, then the conventional Windows paths), so this
/// checks the same list before the first browser call: a missing browser is a failed lane with the
/// search in its message, never a skip. The environment variable is read, not set — the launcher
/// started earlier inherited it, so setting it here could disagree with the process under test.
fn require_chrome() -> PathBuf {
    let configured = std::env::var_os("SUMMRISE_CHROME").map(PathBuf::from);
    if let Some(path) = configured.as_ref().filter(|path| path.is_file()) {
        return path.clone();
    }
    for candidate in WIN_CHROME_CANDIDATES {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return path;
        }
    }
    let configured = configured.as_ref().map_or_else(
        || "SUMMRISE_CHROME is not set".to_owned(),
        |path| format!("SUMMRISE_CHROME={}", path.display()),
    );
    let looked = WIN_CHROME_CANDIDATES
        .iter()
        .map(|path| format!("  {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "the Windows lifecycle needs a real Chrome for its browser section, and the app \
         discovers it from the same list: {configured} names no file. Looked at:\n{looked}\n\
         The runner installs Chrome at {}; set SUMMRISE_CHROME for another executable. \
         This lane does not skip.",
        WIN_CHROME_CANDIDATES[0]
    )
}

/// The pids of `chrome.exe` processes whose command line names `profile`.
///
/// The MCP surface exposes no pid, and `browser.list` would start a *second* browser after a
/// `shutdown` (the registry is lazy by design), so the "process gone" proof has to come from
/// Windows. The needle is this fixture's unique profile directory, so another test's Chrome — or
/// the operator's — cannot match. A check that cannot run is a failure, not a skip.
fn chrome_processes_naming(profile: &Path) -> Vec<u32> {
    let powershell = system32(r"WindowsPowerShell\v1.0\powershell.exe");
    let script = "$ErrorActionPreference = 'Stop'; \
                  Get-CimInstance Win32_Process -Filter \"Name = 'chrome.exe'\" | \
                  Where-Object { $_.CommandLine -and \
                  $_.CommandLine.Contains($env:SUMMRISE_CHROME_PROFILE) } | \
                  ForEach-Object { $_.ProcessId }";
    let output = Command::new(&powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SUMMRISE_CHROME_PROFILE", profile)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", powershell.display()));
    assert!(
        output.status.success(),
        "the process check failed; {} exited with {}:\n{}",
        powershell.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(|pid| {
            pid.parse::<u32>()
                .unwrap_or_else(|_| panic!("the process check printed {pid:?}, not a pid"))
        })
        .collect()
}

/// Wait until no Chrome process names `profile`; returns the pids left at the deadline.
fn wait_for_chrome_exit(profile: &Path, timeout: Duration) -> Vec<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = chrome_processes_naming(profile);
        if left.is_empty() || Instant::now() >= deadline {
            return left;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The value of one standard-base64 alphabet character.
fn base64_value(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("the screenshot is not standard base64: {:?}", byte as char),
    }
}

/// The first `bytes` bytes a standard base64 string decodes to.
///
/// The lane asserts the decoded PNG magic rather than a matching base64 prefix, so a wrong
/// encoding cannot pass by sharing a constant with the encoder.
fn base64_prefix(data: &str, bytes: usize) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(bytes);
    for chunk in data.as_bytes().chunks(4) {
        if decoded.len() >= bytes {
            break;
        }
        assert_eq!(chunk.len(), 4, "base64 is whole four-character groups");
        let group = (u32::from(base64_value(chunk[0])) << 18)
            | (u32::from(base64_value(chunk[1])) << 12)
            | (u32::from(base64_value(chunk[2])) << 6)
            | u32::from(base64_value(chunk[3]));
        decoded.push((group >> 16) as u8);
        decoded.push((group >> 8) as u8);
        decoded.push(group as u8);
    }
    decoded.truncate(bytes);
    decoded
}

/// The `page` targets in a `browser.list` answer. Chrome also lists component background pages, so
/// counting every entry would count something the client did not open.
fn listed_pages(targets: &Value) -> Vec<&Value> {
    targets
        .as_array()
        .unwrap_or_else(|| panic!("browser.list returns an array of targets: {targets}"))
        .iter()
        .filter(|target| target["type"] == "page")
        .collect()
}

/// Drive the browser capability through the installed app's `/mcp`: start the real Chrome, list its
/// single page, open a `data:` document, read its title back, navigate it to `about:blank` and read
/// `location.href`, capture a PNG, close the tab, and shut the browser down.
///
/// The process check is the lane's because no MCP verb exposes the pid (see
/// [`chrome_processes_naming`]). It runs before `shutdown` to prove it can see the browser, and
/// after it to prove shutdown left nothing behind — the same evidence the Linux capability tests
/// read from `/proc`.
fn mcp_browser_round_trip(fixture: &Fixture) {
    let chrome = require_chrome();
    let profile = fixture.data.join("chrome-profile");
    let audited_before = audit_lines(&fixture.data.join("events.jsonl")).len();

    let config = StreamableHttpClientTransportConfig::with_uri(format!(
        "http://127.0.0.1:{}/mcp",
        fixture.port
    ))
    .auth_header(fixture.token.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build the browser client's runtime");
    let url = format!("data:text/html,<title>{BROWSER_TITLE}</title>");

    runtime.block_on(async {
        let client = ()
            .serve(StreamableHttpClientTransport::from_config(config))
            .await
            .expect("rmcp's client completes the MCP handshake with the installed app");

        // A fresh headless Chrome has one page target. Chrome may also list component background
        // pages; those are not tabs and the tab count ignores them.
        let listed = client
            .call_tool(CallToolRequestParams::new("browser.list"))
            .await
            .expect("browser.list is answered");
        assert_eq!(
            listed.is_error,
            Some(false),
            "browser.list starts {} and lists its page: {listed:?}",
            chrome.display()
        );
        let targets = tool_json(&listed);
        let pages = listed_pages(&targets);
        assert_eq!(
            pages.len(),
            1,
            "a fresh headless Chrome lists one page target: {targets}"
        );
        let startup_page = pages[0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("the startup page has a target id: {targets}"))
            .to_owned();
        assert!(!startup_page.is_empty(), "the target id is not empty");

        let opened = client
            .call_tool(
                CallToolRequestParams::new("browser.open")
                    .with_arguments(call_arguments(json!({ "url": url }))),
            )
            .await
            .expect("browser.open is answered");
        assert_eq!(
            opened.is_error,
            Some(false),
            "browser.open loaded a data: document: {opened:?}"
        );
        let opened = tool_json(&opened);
        assert_eq!(
            opened["url"].as_str(),
            Some(url.as_str()),
            "open reports the URL it loaded: {opened}"
        );
        let target = opened["target"]
            .as_str()
            .unwrap_or_else(|| panic!("open returned a target id: {opened}"))
            .to_owned();
        assert!(!target.is_empty(), "the opened target id is not empty");
        assert_ne!(target, startup_page, "the opened tab is a new target");

        // The tab count reflects the open: the startup page plus the document just opened.
        let listed = client
            .call_tool(CallToolRequestParams::new("browser.list"))
            .await
            .expect("browser.list is answered");
        assert_eq!(
            listed.is_error,
            Some(false),
            "browser.list after open: {listed:?}"
        );
        let targets = tool_json(&listed);
        let pages = listed_pages(&targets);
        assert_eq!(
            pages.len(),
            2,
            "the opened tab joins the startup page: {targets}"
        );
        assert!(
            pages
                .iter()
                .any(|page| page["id"].as_str() == Some(target.as_str())),
            "the opened target is listed: {targets}"
        );

        let evaluated = client
            .call_tool(
                CallToolRequestParams::new("browser.eval").with_arguments(call_arguments(json!({
                    "target": target,
                    "expression": "document.title",
                }))),
            )
            .await
            .expect("browser.eval is answered");
        assert_eq!(
            evaluated.is_error,
            Some(false),
            "browser.eval read the title: {evaluated:?}"
        );
        let title = tool_json(&evaluated);
        assert_eq!(title["type"].as_str(), Some("string"), "{title}");
        assert_eq!(
            title["value"].as_str(),
            Some(BROWSER_TITLE),
            "open waited for the document whose title this is: {title}"
        );

        let navigated = client
            .call_tool(
                CallToolRequestParams::new("browser.navigate").with_arguments(call_arguments(
                    json!({ "target": target, "url": "about:blank" }),
                )),
            )
            .await
            .expect("browser.navigate is answered");
        assert_eq!(
            navigated.is_error,
            Some(false),
            "browser.navigate reached about:blank: {navigated:?}"
        );
        let navigated = tool_json(&navigated);
        assert_eq!(navigated["navigated"], true, "{navigated}");
        assert_eq!(
            navigated["url"].as_str(),
            Some("about:blank"),
            "{navigated}"
        );

        let evaluated = client
            .call_tool(
                CallToolRequestParams::new("browser.eval").with_arguments(call_arguments(json!({
                    "target": target,
                    "expression": "location.href",
                }))),
            )
            .await
            .expect("browser.eval is answered");
        assert_eq!(
            evaluated.is_error,
            Some(false),
            "browser.eval read the new URL: {evaluated:?}"
        );
        let href = tool_json(&evaluated);
        assert_eq!(
            href["value"].as_str(),
            Some("about:blank"),
            "navigate committed before eval ran: {href}"
        );

        let captured = client
            .call_tool(
                CallToolRequestParams::new("browser.screenshot")
                    .with_arguments(call_arguments(json!({ "target": target }))),
            )
            .await
            .expect("browser.screenshot is answered");
        assert_eq!(
            captured.is_error,
            Some(false),
            "browser.screenshot captured the page: {captured:?}"
        );
        let captured = tool_json(&captured);
        assert_eq!(captured["format"].as_str(), Some("png"), "{captured}");
        let data = captured["data"]
            .as_str()
            .unwrap_or_else(|| panic!("the screenshot carries base64 data: {captured}"));
        assert!(!data.is_empty(), "the screenshot is not empty");
        assert_eq!(
            base64_prefix(data, 4),
            vec![0x89, b'P', b'N', b'G'],
            "the base64 decodes to the PNG signature; it starts {:?}",
            &data[..data.len().min(24)]
        );
        assert!(
            captured["bytes"].as_u64().unwrap_or(0) > 100,
            "a real screenshot is more than 100 bytes: {captured}"
        );

        let closed = client
            .call_tool(
                CallToolRequestParams::new("browser.close")
                    .with_arguments(call_arguments(json!({ "target": target }))),
            )
            .await
            .expect("browser.close is answered");
        assert_eq!(
            closed.is_error,
            Some(false),
            "browser.close closed the tab: {closed:?}"
        );
        assert_eq!(tool_json(&closed)["closed"], true, "the close succeeded");

        // Chrome's target list can lag the close by a moment; wait for the target to disappear and
        // for the startup page to be the only page left.
        let deadline = Instant::now() + BROWSER_CLOSE_TIMEOUT;
        loop {
            let listed = client
                .call_tool(CallToolRequestParams::new("browser.list"))
                .await
                .expect("browser.list is answered");
            assert_eq!(
                listed.is_error,
                Some(false),
                "browser.list after close: {listed:?}"
            );
            let targets = tool_json(&listed);
            let pages = listed_pages(&targets);
            if pages.len() == 1
                && !pages
                    .iter()
                    .any(|page| page["id"].as_str() == Some(target.as_str()))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the closed target is gone from browser.list within {}s: {targets}",
                BROWSER_CLOSE_TIMEOUT.as_secs()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Before stopping it, prove the process check can see the browser: a shutdown that "leaves
        // nothing" must not pass by watching for something that was never found in the first place.
        let running = chrome_processes_naming(&profile);
        assert!(
            !running.is_empty(),
            "the process check finds the Chrome the app started for {}; running pids: {running:?}",
            profile.display()
        );

        let stopped = client
            .call_tool(CallToolRequestParams::new("browser.shutdown"))
            .await
            .expect("browser.shutdown is answered");
        assert_eq!(
            stopped.is_error,
            Some(false),
            "browser.shutdown stopped the browser: {stopped:?}"
        );
        assert_eq!(
            tool_json(&stopped)["stopped"],
            true,
            "shutdown reports it stopped a running browser"
        );

        client
            .cancel()
            .await
            .expect("the MCP client closes cleanly");
    });

    // `shutdown` drops the browser, which kills and reaps it; a renderer can outlive that by a
    // moment, so wait rather than sample once.
    let left = wait_for_chrome_exit(&profile, BROWSER_STOP_TIMEOUT);
    assert!(
        left.is_empty(),
        "browser.shutdown leaves no chrome.exe naming {}; still running: {left:?}",
        profile.display()
    );

    let lines = audit_lines(&fixture.data.join("events.jsonl"));
    let added = lines.get(audited_before..).unwrap_or_else(|| {
        panic!(
            "the audit file kept the earlier lines: {} lines, expected at least {audited_before}",
            lines.len()
        )
    });
    for tool in [
        "browser.list",
        "browser.open",
        "browser.eval",
        "browser.navigate",
        "browser.screenshot",
        "browser.close",
        "browser.shutdown",
    ] {
        assert_audited(added, tool);
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// A small release server: the lane fetches over real HTTP, so the test serves over real HTTP
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// One path and its bytes. The content type is part of the response because a document and a binary
/// are different things; the downloader itself does not read it.
struct ReleaseRoute {
    path: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

/// A loopback HTTP/1.1 server for one test phase.
///
/// It binds `127.0.0.1:0`, so parallel tests never collide, and the base URL is handed to `routes`
/// before the thread starts: a release document names its artifact as an absolute URL on this same
/// origin, which is exactly the shape a device fetches from a CDN. One request per connection
/// (`Connection: close`) is all `reqwest` needs here, and the thread is joined in `Drop` — a panic
/// above still stops it.
struct ReleaseServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ReleaseServer {
    fn start<F>(routes: F) -> ReleaseServer
    where
        F: FnOnce(&str) -> Vec<ReleaseRoute>,
    {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind the release server to a loopback port");
        listener
            .set_nonblocking(true)
            .expect("poll the release server's listener");
        let port = listener
            .local_addr()
            .expect("the release server's port is known")
            .port();
        let routes = routes(&format!("http://127.0.0.1:{port}"));
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !server_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => serve_release_request(stream, &routes),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        ReleaseServer {
            port,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for ReleaseServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Read one request head, answer it, and close. A write that fails is a client that already went
/// away; that is not the server's failure to report.
fn serve_release_request(mut stream: TcpStream, routes: &[ReleaseRoute]) {
    // `reqwest` connects with a blocking socket assumption; make sure a nonblocking listener did
    // not hand one down on a platform where the mode is inherited.
    let _ = stream.set_nonblocking(false);
    if stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .is_err()
    {
        return;
    }

    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 16 * 1024 {
            return;
        }
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }

    let text = String::from_utf8_lossy(&request);
    let target = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    let path = target.split('?').next().unwrap_or(target);
    let (status, content_type, body) = match routes.iter().find(|route| route.path == path) {
        Some(route) => ("200 OK", route.content_type, route.body.as_slice()),
        None => ("404 Not Found", "text/plain", b"not found".as_slice()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The install the test drives
// ─────────────────────────────────────────────────────────────────────────────────────────────────

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    data: PathBuf,
    port: u16,
    token: String,
    baseline_sha: String,
    launcher: Option<Child>,
    last_stderr: Option<PathBuf>,
    runs: u32,
}

impl Fixture {
    /// A unique temp root with `install/` and `data/`, a free loopback port, and a token. Nothing is
    /// copied yet; [`Fixture::prepare_baseline`] is the "verified install" step.
    fn new(tag: &str) -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "summrise-lifecycle-{}-{nanos}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let install = root.join("install");
        let data = root.join("data");
        fs::create_dir_all(&install).expect("create the install directory");
        fs::create_dir_all(&data).expect("create the data directory");
        Fixture {
            root,
            install,
            data,
            port: free_port(),
            token: mint_token(),
            baseline_sha: String::new(),
            launcher: None,
            last_stderr: None,
            runs: 0,
        }
    }

    /// What an installer does after a verified download: the `current` generation pair (the app
    /// binary plus its signed sidecar), the same bytes as the app, and the install token. `current`
    /// is the generation the self-repair trusts (fix design F2); the launcher's rotated copies
    /// (previous, pending, staged) are not boot candidates. `previous` is deliberately absent, as it
    /// is after a fresh install.
    fn prepare_baseline(&mut self) {
        let device = device_exe();
        let baseline = baseline_exe(&self.install);
        fs::create_dir_all(baseline.parent().expect("the baseline has a parent"))
            .expect("create the current generation directory");
        fs::copy(&device, &baseline).expect("copy the app into the current generation");
        let sha = sha256_file(&baseline).expect("hash the baseline binary");
        let sidecar = signed_sidecar(BASELINE_VERSION, "stable", &sha);
        save_sidecar(&baseline_sidecar(&self.install), &sidecar)
            .expect("write the signed baseline sidecar");
        fs::copy(&device, app_exe(&self.install)).expect("install the app binary");
        // 32 random-ish bytes as 64 hex characters: fixture entropy, not a secret. It guards one
        // loopback port for one test, and hashing the clock + pid keeps the dependency list to what
        // the product already uses (the product's token is minted by the installer, not here).
        fs::write(self.data.join("token"), format!("{}\n", self.token))
            .expect("mint the install token");
        self.baseline_sha = sha;
    }

    fn start_launcher(&mut self) {
        self.runs += 1;
        let (stdout, stderr) = self.log_files("launcher");
        let child = Command::new(launcher_exe())
            .arg("run")
            .arg("--install")
            .arg(&self.install)
            .arg("--data")
            .arg(&self.data)
            .arg("--port")
            .arg(self.port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start {}: {error}", launcher_exe()));
        self.launcher = Some(child);
    }

    fn log_files(&mut self, kind: &str) -> (File, File) {
        let logs = self.root.join("logs");
        fs::create_dir_all(&logs).expect("create the log directory");
        let stem = format!("{kind}-{}", self.runs);
        let stdout = File::create(logs.join(format!("{stem}.out"))).expect("create the stdout log");
        let stderr = File::create(logs.join(format!("{stem}.err"))).expect("create the stderr log");
        self.last_stderr = Some(logs.join(format!("{stem}.err")));
        (stdout, stderr)
    }

    fn stderr_text(&self) -> String {
        self.last_stderr
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default()
    }

    /// Poll `/api/health` until TWO CONSECUTIVE 200s report `expected_version` and `status:"ok"`
    /// (fix design F4). One answer is not proof: the old app can still be answering the port while a
    /// swap starts the new one. The failure message carries the launcher's stderr, which is where a
    /// refused sidecar or a `NeedsHuman` restore says why.
    fn wait_healthy_version(&self, expected_version: &str) -> Value {
        let deadline = Instant::now() + HEALTH_TIMEOUT;
        let mut consecutive = 0u8;
        // Deferred: every branch below assigns it before the deadline check reads it.
        let mut last;
        loop {
            match probe_health(self.port) {
                Ok(document)
                    if document["status"].as_str() == Some("ok")
                        && document["version"].as_str() == Some(expected_version) =>
                {
                    consecutive = consecutive.saturating_add(1);
                    if consecutive >= 2 {
                        assert_health(&document, expected_version);
                        return document;
                    }
                    last = format!("one matching answer; waiting for a second: {document}");
                }
                Ok(document) => {
                    consecutive = 0;
                    last = format!("health does not report {expected_version:?}: {document}");
                }
                Err(error) => {
                    consecutive = 0;
                    last = error;
                }
            }
            if Instant::now() >= deadline {
                panic!(
                    "the device did not answer /api/health twice for version {expected_version} on \
                     port {} within {}s: {last}\n--- launcher stderr ---\n{}",
                    self.port,
                    HEALTH_TIMEOUT.as_secs(),
                    self.stderr_text()
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// The launcher owns the app child through a kill-on-close job, so killing the launcher must take
    /// the app with it. Waiting for the port to go quiet is what makes the next launcher's port bind
    /// meaningful (and is the only assertion that can see an orphaned child).
    fn wait_health_down(&self) {
        let deadline = Instant::now() + CHILD_STOP_TIMEOUT;
        while Instant::now() < deadline {
            match http_get(self.port, "/api/health", None) {
                Err(_) => return,
                Ok(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        panic!(
            "the app still answers on port {} after its launcher was stopped: the killed \
             supervisor left its child behind (the kill-on-close job did not hold)\n\
             --- launcher stderr ---\n{}",
            self.port,
            self.stderr_text()
        );
    }

    /// Kill the supervisor and prove its child went with it — the lifecycle's "service stopped".
    /// The exit is waited for with a deadline: a supervisor that survives `kill` is a failure to
    /// report, not a place for the test to hang.
    fn stop_launcher(&mut self) {
        if let Some(mut child) = self.launcher.take() {
            // `kill` on an already-exited child is an error in recent Rust; that is not a failure.
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
            }
            if wait_for_child(&mut child, Duration::from_secs(10)).is_none() {
                panic!(
                    "the supervisor did not exit within 10s of being killed\n\
                     --- launcher stderr ---\n{}",
                    self.stderr_text()
                );
            }
        }
        self.wait_health_down();
    }

    /// A swap must not take its own supervisor down: the supervisor keeps running, and the app it
    /// now owns is the one that answers health.
    fn assert_launcher_running(&mut self) {
        let problem = match self.launcher.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => None,
                Ok(Some(status)) => Some(format!("exited with {status}")),
                Err(error) => Some(format!("could not be polled: {error}")),
            },
            None => Some("was never started".to_owned()),
        };
        if let Some(problem) = problem {
            panic!(
                "the supervisor {problem}; a swap is performed by the running supervisor, not by \
                 its exit\n--- launcher stderr ---\n{}",
                self.stderr_text()
            );
        }
    }

    fn app_pid(&self) -> Option<u32> {
        fs::read_to_string(app_pid_file(&self.data))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// A failure path can leave an app behind a dead supervisor. Stop it by the pid file the
    /// supervisor wrote. The image filter makes a recycled pid harmless: only `summrise-device.exe`
    /// is killed. `taskkill` itself is waited for with a deadline so cleanup cannot become the hang.
    fn kill_app_from_pid_file(&self) {
        if let Some(pid) = self.app_pid() {
            let mut killer = match Command::new(system32("taskkill.exe"))
                .args([
                    "/F",
                    "/FI",
                    "IMAGENAME eq summrise-device.exe",
                    "/FI",
                    &format!("PID eq {pid}"),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(killer) => killer,
                Err(_) => return,
            };
            if wait_for_child(&mut killer, Duration::from_secs(10)).is_none() {
                let _ = killer.kill();
                let _ = killer.wait();
            }
        }
    }

    fn kill_launcher_if_alive(&mut self) {
        if let Some(mut child) = self.launcher.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Fixture {
    /// Best-effort cleanup on the failure path; the passing test calls the explicit methods first and
    /// asserts that the directory is gone, so a leak is a failure rather than a warning.
    fn drop(&mut self) {
        self.kill_launcher_if_alive();
        self.kill_app_from_pid_file();
        for _ in 0..20 {
            match fs::remove_dir_all(&self.root) {
                Ok(()) => return,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        eprintln!(
            "warning: the fixture directory survived cleanup: {}",
            self.root.display()
        );
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
    listener.local_addr().expect("the port is known").port()
}

fn mint_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let entropy = format!(
        "summrise-lifecycle-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    sha256_bytes(entropy.as_bytes())
}

/// Copy a release artifact under the fixture root, outside the install, so the stager reads it and
/// the install's only writes are the canonical pair. Returns the artifact's path.
fn copy_release(root: &Path, name: &str, source: &Path) -> PathBuf {
    let releases = root.join("releases");
    fs::create_dir_all(&releases).expect("create the releases directory");
    let destination = releases.join(name);
    fs::copy(source, &destination).unwrap_or_else(|error| {
        panic!(
            "cannot copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    });
    destination
}

/// Run the real product stager: `summrise-device --stage <artifact> <manifest> --install <install>`.
///
/// The stager writes the canonical pair, writes `swap.request.json`, waits up to 60 s for the
/// supervisor's answer and returns the 0/3/4 exit code. `--apply` is not involved.
fn stage(install: &Path, artifact: &Path, manifest: &Path) -> Output {
    Command::new(device_exe())
        .arg("--stage")
        .arg(artifact)
        .arg(manifest)
        .arg("--install")
        .arg(install)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("cannot run summrise-device --stage: {error}"))
}

/// Run the real product's release lane: `summrise-device --check-update <url> --install <install>
/// --data <data> --channel <channel>`. The downloader fetches the signed document and the artifact
/// from the URL, stages the pair, writes `swap.request.json`, and waits for the supervisor.
fn check_update(install: &Path, data: &Path, url: &str, channel: &str) -> Output {
    Command::new(device_exe())
        .arg("--check-update")
        .arg(url)
        .arg("--install")
        .arg(install)
        .arg("--data")
        .arg(data)
        .arg("--channel")
        .arg(channel)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("cannot run summrise-device --check-update: {error}"))
}

/// Wait for `child` with a deadline; `None` means it was still running when the deadline passed.
fn wait_for_child(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Run `command` with its stdout/stderr redirected to files and wait for it with a deadline.
///
/// `Command::output()` waits for the *pipes* to close, not just the process. `--apply` on a console
/// install leaves the restored app running by design, and that child inherits the launcher's stdio:
/// a captured pipe therefore never closes and the test hangs for as long as the app runs. Files do
/// not have that property, and they keep both the launcher's and the app's logs for the failure
/// message. The deadline is the second half: the launcher must exit, or the test fails with what it
/// printed.
fn run_captured(
    mut command: Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> (ExitStatus, String, String) {
    let stdout_file = File::create(stdout_path)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", stdout_path.display()));
    let stderr_file = File::create(stderr_path)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", stderr_path.display()));
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap_or_else(|error| panic!("cannot spawn the launcher: {error}"));

    let Some(status) = wait_for_child(&mut child, timeout) else {
        let _ = child.kill();
        let _ = child.wait();
        let stdout = fs::read_to_string(stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
        panic!(
            "the launcher did not exit within {}s. The restored console app inherits these files, \
             so the app log and the launcher log are both below.\n--- stdout ---\n{stdout}\n\
             --- stderr ---\n{stderr}",
            timeout.as_secs()
        );
    };
    let stdout = fs::read_to_string(stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    (status, stdout, stderr)
}

/// The supervisor's answer file, parsed with the launcher's own type.
fn read_swap_result(install: &Path) -> SwapResult {
    let path = swap_result(install);
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("the supervisor did not write {}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "{} is not a swap result ({error}): {:?}",
            path.display(),
            String::from_utf8_lossy(&bytes)
        )
    })
}

/// The supervisor writes the result before it removes the request, so a reader that has just seen
/// the result can briefly still see the request. Wait for the contract rather than race it.
fn wait_request_removed(install: &Path) {
    let path = swap_request(install);
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.exists() {
        if Instant::now() >= deadline {
            panic!(
                "the supervisor did not remove {} after answering the request",
                path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Delete `path`, retrying while Windows still has the image mapped. Ten seconds is far beyond the
/// teardown that follows a terminated process; a failure after that really is a file we cannot move.
fn remove_file_when_released(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match fs::remove_file(path) {
            Ok(()) => return,
            Err(error) => {
                if Instant::now() >= deadline {
                    panic!("cannot delete {}: {error}", path.display());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The lifecycle
// ─────────────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_device_is_installed_supervised_repaired_and_updated() {
    require_fixture_key();

    let mut fixture = Fixture::new("lifecycle");
    fixture.prepare_baseline();
    let baseline_sha = fixture.baseline_sha.clone();

    // 1. A verified install starts from `baseline\current` (F2), and health reports the release
    //    version the supervisor passed on the child's argv (F4/F6), twice in a row.
    fixture.start_launcher();
    let first = fixture.wait_healthy_version(BASELINE_VERSION);
    let pid = assert_health(&first, BASELINE_VERSION);

    // 2. The install token is the only way to the state document (design §9.1, §4.3 rule 1).
    let (status, body) = http_get(fixture.port, "/api/state", None).expect("query /api/state");
    assert_eq!(
        status, 401,
        "without the install token /api/state must be 401, not {status}: {body}"
    );
    let (status, body) =
        http_get(fixture.port, "/api/state", Some(&fixture.token)).expect("query /api/state");
    assert_eq!(status, 200, "with the install token: {body}");
    let state = State::parse(&body).unwrap_or_else(|error| {
        panic!("GET /api/state did not answer with a state document ({error}): {body}")
    });
    assert_eq!(state.schema, SCHEMA_VERSION, "the document's schema");
    assert_eq!(
        Path::new(&state.install.dir),
        fixture.install.as_path(),
        "the document names the install it runs from"
    );
    assert_eq!(
        state.install.version, BASELINE_VERSION,
        "the document's version is the release the supervisor booted, not the crate version"
    );
    assert_eq!(
        state.baseline.version, BASELINE_VERSION,
        "the baseline facts follow the same release"
    );
    assert_eq!(
        state.baseline.sha256, baseline_sha,
        "the document names the digest of the current generation"
    );
    assert_eq!(state.service.pid, pid, "the document names the live pid");

    // 3. The MCP surface through the installed app: rmcp's own client lists the registry's tools
    //    and round-trips one file, and the server's audit file records exactly those two calls.
    //    Keyed and elevated-free: no service is started for this.
    mcp_files_round_trip(&fixture);

    // 4. The first session capability through the same app: a real ConPTY. `cmd.exe` is typed at
    //    over `/mcp`, its own output is read back with the cursor, the session is resized and
    //    closed, and `/api/state` lists it for exactly its life. Also keyed and elevated-free.
    mcp_conpty_round_trip(&fixture);

    // 5. The browser capability through the same app: a real headless Chrome starts, its one page
    //    is listed, a `data:` document is opened and its title read, the tab is navigated to
    //    `about:blank` and read back, a PNG is captured, the tab is closed, and the browser is
    //    stopped with no `chrome.exe` left naming the fixture's profile. Same keyed, elevated-free
    //    app; a browserless runner fails by name rather than skipping.
    mcp_browser_round_trip(&fixture);

    // 6. Stop the supervisor: the kill-on-close job must take the app with it, or the next start
    //    would find the port held by an orphan (which `stop_launcher` itself asserts).
    fixture.stop_launcher();
    let app = app_exe(&fixture.install);

    // 7. Self-repair: delete the app; the launcher's start-up step copies `current` back before
    //    starting anything (F2 — the process that repairs the app cannot be the app). Windows
    //    releases an exited process's image a moment after the port closes, so a sharing violation
    //    here is the teardown, not a product failure; retry it rather than flake. `previous` may be
    //    absent at this point (a fresh install has only `current`), which is why nothing asserts it.
    remove_file_when_released(&app);
    assert!(!app.exists(), "the app binary really is gone");
    fixture.start_launcher();
    let second = fixture.wait_healthy_version(BASELINE_VERSION);
    let second_pid = assert_health(&second, BASELINE_VERSION);
    assert_ne!(
        second_pid, pid,
        "health is answered by the restarted app, not the killed one"
    );
    assert_eq!(
        fs::read(&app).expect("read the restored app"),
        fs::read(baseline_exe(&fixture.install)).expect("read the current generation"),
        "the launcher restored the current generation byte for byte"
    );
    assert_eq!(
        sha256_file(&app).expect("hash the restored app"),
        baseline_sha,
        "the restored app is the baseline the sidecar names"
    );
    let log = fixture.stderr_text();
    assert!(
        log.contains("restored"),
        "the launcher says it restored the app; its stderr was:\n{log}"
    );

    // I3: the repair is a machine record, not a stderr sentence. The launcher wrote one line; the
    // app consumed it at start into `lastVerdict` and `events.jsonl`, and remembered the offset.
    let repair_log = fixture.install.join("repair.log");
    let repairs = fs::read_to_string(&repair_log).unwrap_or_else(|error| {
        panic!(
            "the launcher did not write {}: {error}",
            repair_log.display()
        )
    });
    assert_eq!(
        repairs.lines().count(),
        1,
        "one deleted-exe repair is one repair record: {repairs}"
    );
    let record: Value = serde_json::from_str(repairs.trim()).expect("the repair record is JSON");
    assert_eq!(record["kind"], "repaired");
    assert_eq!(record["from"], "baseline");
    assert_eq!(
        record["sha256"], baseline_sha,
        "the record names the bytes that were restored: {record}"
    );
    let (status, body) = http_get(fixture.port, "/api/state", Some(&fixture.token))
        .expect("query /api/state after the repair");
    assert_eq!(status, 200, "the repaired device serves its state: {body}");
    let repaired = State::parse(&body).expect("the app writes the state document");
    assert_eq!(
        repaired.last_verdict.outcome,
        VerdictOutcome::Repaired,
        "the repair is the last verdict: {:?}",
        repaired.last_verdict
    );
    assert_eq!(repaired.last_verdict.detail, "repaired from baseline");
    assert!(
        repaired.repair_offset > 0,
        "the document records how far it consumed the repair log"
    );
    let events = fs::read_to_string(fixture.data.join("events.jsonl"))
        .expect("the device's event log is written");
    assert!(
        events
            .lines()
            .any(|line| line.contains("\"event\":\"repaired_from_baseline\"")),
        "the event log carries the same repair the verdict names: {events}"
    );

    // 8. A real update, end to end (F8). The artifact and its manifest are signed with the fixture
    //    key; the REAL device stager verifies them, writes `<install>\summrise-device.new.exe` and
    //    its sidecar, writes the request and waits for the supervisor. The supervisor validates the
    //    pair, swaps, proves version 0.2.0 through two health probes and commits the generation.
    //    The binary bytes do not change: the release version comes from the argv and the committed
    //    sidecar (F6), so this is an update of the signed facts, not a different executable.
    let artifact = copy_release(&fixture.root, "summrise-device-0.2.0.exe", &device_exe());
    let artifact_bytes = fs::read(&artifact).expect("read the 0.2.0 artifact");
    let manifest = signed_manifest(UPDATED_VERSION, "stable", &artifact_bytes, BASELINE_VERSION);
    let manifest_path = fixture.root.join("releases").join("0.2.0.json");
    write_manifest(&manifest_path, &manifest);

    let output = stage(&fixture.install, &artifact, &manifest_path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "the real stage + supervisor swap must apply {UPDATED_VERSION}; stdout was:\n{stdout}\n\
         stderr was:\n{stderr}\n--- launcher stderr ---\n{}",
        fixture.stderr_text()
    );
    assert!(
        stdout.contains(&format!("applied {UPDATED_VERSION}")),
        "the stager reports the supervisor's outcome; stdout was:\n{stdout}"
    );
    fixture.assert_launcher_running();
    let result = read_swap_result(&fixture.install);
    assert_eq!(
        result.outcome,
        SwapOutcome::Applied,
        "the supervisor's result: {result:?}"
    );
    assert_eq!(
        result.version.as_deref(),
        Some(UPDATED_VERSION),
        "the result names the applied release: {result:?}"
    );
    wait_request_removed(&fixture.install);
    assert_eq!(
        fs::read(staged_exe(&fixture.install)).expect("read the staged artifact the app wrote"),
        artifact_bytes,
        "the app staged the 0.2.0 artifact under the canonical name"
    );
    assert_eq!(
        load_sidecar(&staged_sidecar(&fixture.install))
            .expect("read the staged sidecar the app wrote")
            .version,
        UPDATED_VERSION,
        "the app wrote the canonical staged pair for the new release"
    );
    let after_update = fixture.wait_healthy_version(UPDATED_VERSION);
    assert_health(&after_update, UPDATED_VERSION);
    assert_eq!(
        load_sidecar(&baseline_sidecar(&fixture.install))
            .expect("read the committed current sidecar")
            .version,
        UPDATED_VERSION,
        "the committed `current` generation is 0.2.0"
    );
    assert_eq!(
        sha256_file(&baseline_exe(&fixture.install)).expect("hash the committed current binary"),
        sha256_file(&artifact).expect("hash the 0.2.0 artifact"),
        "`current` holds the artifact that was staged"
    );
    assert_eq!(
        load_sidecar(&generation_sidecar(&fixture.install, PREVIOUS_DIR))
            .expect("read the previous sidecar")
            .version,
        BASELINE_VERSION,
        "the generation this update replaced is `previous` and names 0.1.0"
    );
    assert_eq!(
        sha256_file(&generation_exe(&fixture.install, PREVIOUS_DIR))
            .expect("hash the previous binary"),
        baseline_sha,
        "`previous` holds the 0.1.0 bytes"
    );

    // 9. A rollback, end to end. cmd.exe is a real PE binary with a real digest, so the signature and
    //    digest checks pass and the failure is the only thing left: no `/api/health` within the probe
    //    window. The supervisor must answer `rolled_back` (exit 3), keep serving 0.2.0, and leave the
    //    committed generation untouched.
    let broken_artifact = copy_release(&fixture.root, "cmd-0.3.0.exe", &system32("cmd.exe"));
    let broken_bytes = fs::read(&broken_artifact).expect("read the 0.3.0 artifact");
    let broken_manifest =
        signed_manifest(ROLLBACK_VERSION, "stable", &broken_bytes, BASELINE_VERSION);
    let broken_manifest_path = fixture.root.join("releases").join("0.3.0.json");
    write_manifest(&broken_manifest_path, &broken_manifest);

    let output = stage(&fixture.install, &broken_artifact, &broken_manifest_path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(3),
        "a validly-signed update that cannot serve must exit 3 (rolled_back); stdout was:\n\
         {stdout}\nstderr was:\n{stderr}\n--- launcher stderr ---\n{}",
        fixture.stderr_text()
    );
    assert!(
        stdout.contains(&format!("rolled_back {ROLLBACK_VERSION}")),
        "the stager reports the rollback; stdout was:\n{stdout}"
    );
    fixture.assert_launcher_running();
    let result = read_swap_result(&fixture.install);
    assert_eq!(
        result.outcome,
        SwapOutcome::RolledBack,
        "the supervisor's result: {result:?}"
    );
    assert_eq!(
        result.version.as_deref(),
        Some(ROLLBACK_VERSION),
        "the result names the release that failed: {result:?}"
    );
    assert_eq!(
        result.sha256.as_deref(),
        Some(broken_manifest.sha256.as_str()),
        "the result carries the digest the launcher verified, so the app can remember the \
         artifact without reading a file that may be gone: {result:?}"
    );
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("health")),
        "the rollback says the candidate could not answer health: {result:?}"
    );

    let after_rollback = fixture.wait_healthy_version(UPDATED_VERSION);
    assert_health(&after_rollback, UPDATED_VERSION);
    // I3: the rollback verdict is `rolled back from <the candidate>`, not the refusal's phantom
    // "repaired from baseline".
    let (status, body) = http_get(fixture.port, "/api/state", Some(&fixture.token))
        .expect("query /api/state after the rollback");
    assert_eq!(
        status, 200,
        "the rolled-back device serves its state: {body}"
    );
    let rolled_back = State::parse(&body).expect("the app writes the state document");
    assert_eq!(
        rolled_back.last_verdict.detail,
        format!("rolled back from {ROLLBACK_VERSION}"),
        "the last verdict names the release that failed: {:?}",
        rolled_back.last_verdict
    );
    assert_eq!(
        rolled_back.last_verdict.outcome,
        VerdictOutcome::NeedsAttention,
        "a rollback needs attention; it is not a clean start"
    );
    let remembered = rolled_back
        .rolled_back
        .iter()
        .find(|artifact| artifact.version == ROLLBACK_VERSION)
        .unwrap_or_else(|| {
            panic!(
                "the rollback is remembered by digest: {:?}",
                rolled_back.rolled_back
            )
        });
    assert_eq!(
        remembered.sha256, broken_manifest.sha256,
        "the remembered digest is the artifact the launcher rolled back"
    );
    assert!(
        remembered.at_ms > 0,
        "the remembered rollback names when the supervisor answered: {remembered:?}"
    );
    assert_eq!(
        load_sidecar(&baseline_sidecar(&fixture.install))
            .expect("read the current sidecar after the rollback")
            .version,
        UPDATED_VERSION,
        "the rollback does not promote the previous generation over a working current one"
    );
    assert_eq!(
        fs::read(app_exe(&fixture.install)).expect("read the running app after the rollback"),
        artifact_bytes,
        "the running exe is byte-identical to the 0.2.0 artifact"
    );
    assert_eq!(
        load_sidecar(&generation_sidecar(&fixture.install, PREVIOUS_DIR))
            .expect("read the previous sidecar after the rollback")
            .version,
        BASELINE_VERSION,
        "0.1.0 is still the fallback generation"
    );

    // 10. Stop everything and prove the temp tree is gone. The supervisor owns the app child through
    //    its job, so stopping it is enough; the pid-file kill is the Drop net for a failure path.
    fixture.stop_launcher();
    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}

/// The operator-only `--apply` path runs the same swap transaction, so its rollback result must
/// carry the failed artifact's digest and the restored app must remember it. No SCM service is
/// installed here: `--apply` stops a console owner (none is running), leaves the restored child in
/// place, and the app records the result at its start — the operator's "console install is down"
/// case, without the service manager.
#[test]
fn an_operator_apply_rollback_records_the_failed_digest() {
    require_fixture_key();
    require_no_product_service();

    let mut fixture = Fixture::new("apply-rollback");
    fixture.prepare_baseline();

    let broken = copy_release(&fixture.root, "cmd-apply-0.3.0.exe", &system32("cmd.exe"));
    let broken_bytes = fs::read(&broken).expect("read the broken artifact");
    let broken_sha = sha256_bytes(&broken_bytes);
    let sidecar = release_signed_sidecar(ROLLBACK_VERSION, "stable", &broken_sha);
    let sidecar_path = fixture.root.join("apply-0.3.0.json");
    save_sidecar(&sidecar_path, &sidecar).expect("write the staged sidecar");

    // I3: the state document remembers this exact artifact failing here. `--apply` must warn and
    // audit, and must not block — the operator is the last resort.
    let remembered_at = 1_700_000_000_000u64;
    fs::write(
        fixture.data.join("state.json"),
        json!({
            "schema": 1,
            "install": {
                "dir": fixture.install.display().to_string(),
                "version": BASELINE_VERSION,
                "channel": "stable",
            },
            "baseline": {
                "version": BASELINE_VERSION,
                "sha256": fixture.baseline_sha,
                "exe": baseline_exe(&fixture.install).display().to_string(),
            },
            "service": {
                "state": "stopped",
                "pid": 0,
                "startedAt": "2023-11-14T22:13:20Z",
            },
            "lastVerdict": {
                "ts": "2023-11-14T22:13:20Z",
                "outcome": "needs_attention",
                "detail": "rolled back from 0.3.0",
            },
            "sessions": [],
            "update": {"phase": "idle", "to": null},
            "rolledBack": [{
                "sha256": broken_sha,
                "version": ROLLBACK_VERSION,
                "atMs": remembered_at,
            }],
            "repairOffset": 0,
        })
        .to_string(),
    )
    .expect("the rollback memory fixture is writable");

    // `--apply` on a console install leaves the restored app running by design (ManualOwner::finish
    // with no service). That child inherits the launcher's stdio, so `Command::output()` would wait
    // for a pipe that never closes — the six-minute CI hang. Files plus a deadline keep the
    // launcher's and the app's logs and fail fast instead.
    let logs = fixture.root.join("logs");
    fs::create_dir_all(&logs).expect("create the log directory");
    let stdout_path = logs.join("apply.out");
    let stderr_path = logs.join("apply.err");
    let mut command = Command::new(launcher_exe());
    command
        .arg("--apply")
        .arg(&broken)
        .arg(&sidecar_path)
        .arg("--install")
        .arg(&fixture.install)
        .arg("--data")
        .arg(&fixture.data)
        .arg("--port")
        .arg(fixture.port.to_string());
    let (status, stdout, stderr) = run_captured(
        command,
        &stdout_path,
        &stderr_path,
        Duration::from_secs(120),
    );
    // The restored app inherits this file; `wait_healthy_version` reads it on failure, so its
    // message carries the app's log as well as the launcher's.
    fixture.last_stderr = Some(stderr_path.clone());
    assert_eq!(
        status.code(),
        Some(1),
        "--apply reports a rollback as its failure code; stdout was:\n{stdout}\nstderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("rolled back"),
        "--apply says the candidate rolled back; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("was rolled back on this device") && stderr.contains(&broken_sha),
        "--apply names the remembered digest it is overriding; stderr was:\n{stderr}"
    );
    let audit = fs::read_to_string(fixture.data.join("events.jsonl"))
        .expect("the operator override is audited");
    let override_line = audit
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line["event"] == "operator_apply_rolled_back")
        .unwrap_or_else(|| panic!("no operator override in the audit log:\n{audit}"));
    assert_eq!(override_line["sha256"], json!(broken_sha));
    assert_eq!(override_line["version"], ROLLBACK_VERSION);
    assert_eq!(override_line["rolled_back_at_ms"], remembered_at);

    let result = read_swap_result(&fixture.install);
    assert_eq!(result.outcome, SwapOutcome::RolledBack);
    assert_eq!(
        result.sha256.as_deref(),
        Some(broken_sha.as_str()),
        "the operator path's result carries the candidate's digest: {result:?}"
    );

    let after = fixture.wait_healthy_version(BASELINE_VERSION);
    assert_health(&after, BASELINE_VERSION);
    let (status, body) = http_get(fixture.port, "/api/state", Some(&fixture.token))
        .expect("query /api/state after the operator rollback");
    assert_eq!(status, 200, "the restored app serves its state: {body}");
    let state = State::parse(&body).expect("the app writes the state document");
    let remembered = state
        .rolled_back
        .iter()
        .find(|artifact| artifact.version == ROLLBACK_VERSION)
        .unwrap_or_else(|| {
            panic!(
                "the --apply rollback is remembered by digest: {:?}",
                state.rolled_back
            )
        });
    assert_eq!(
        remembered.sha256, broken_sha,
        "the restored app remembers the artifact the operator's apply rolled back"
    );
    assert!(
        remembered.at_ms > 0,
        "the remembered rollback names when the supervisor answered: {remembered:?}"
    );

    fixture.kill_app_from_pid_file();
    fixture.wait_health_down();
    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}

/// The release lane end to end (design §4.4, §6): `summrise-device --check-update` fetches one
/// signed document and one artifact from a real loopback HTTP server, the running supervisor swaps
/// the pair, and `baseline\current` becomes the release the document names. The same lane must then
/// refuse a genuine canary document on a stable device and a signed downgrade, before either one can
/// stage anything.
///
/// The channel is `stable` throughout. The document is signed with the fixture key, so what is on
/// trial is the downloader's verifier, the digest check, the stager, and the supervisor's swap — no
/// step is short-circuited by a test-side helper. All three phases run against one supervisor, which
/// is what makes "health still 0.4.0" and "`baseline\current` unchanged" statements about the same
/// running device rather than about three fresh ones.
#[test]
fn the_lane_fetches_verifies_and_applies_a_signed_release() {
    require_fixture_key();

    let mut fixture = Fixture::new("release-lane");
    fixture.prepare_baseline();
    fixture.start_launcher();
    fixture.wait_healthy_version(BASELINE_VERSION);

    // 1. A signed 0.4.0 release applies end to end. The artifact is this build's own device binary:
    //    the release version is a signed fact from the document and the sidecar, not a property of
    //    the executable's bytes (F6), so the lane can prove the whole path without a second build.
    let artifact = fs::read(device_exe()).expect("read the device binary as the release artifact");
    let artifact_sha = sha256_bytes(&artifact);
    let server = ReleaseServer::start(|base| {
        let document = signed_release(
            RELEASE_VERSION,
            "stable",
            &format!("{base}/artifact.exe"),
            &artifact,
            BASELINE_VERSION,
        );
        vec![
            ReleaseRoute {
                path: "/release.json",
                content_type: "application/json",
                body: serde_json::to_vec(&document).expect("the release document serializes"),
            },
            ReleaseRoute {
                path: "/artifact.exe",
                content_type: "application/octet-stream",
                body: artifact.clone(),
            },
        ]
    });

    let output = check_update(
        &fixture.install,
        &fixture.data,
        &server.url("/release.json"),
        "stable",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "the real --check-update must fetch, verify and stage {RELEASE_VERSION}, and the running \
         supervisor must apply it; stdout was:\n{stdout}\nstderr was:\n{stderr}\n\
         --- launcher stderr ---\n{}",
        fixture.stderr_text()
    );
    assert!(
        stdout.contains(&format!("applied {RELEASE_VERSION}")),
        "the mode reports the supervisor's outcome; stdout was:\n{stdout}"
    );
    fixture.assert_launcher_running();
    let result = read_swap_result(&fixture.install);
    assert_eq!(
        result.outcome,
        SwapOutcome::Applied,
        "the supervisor's result for the fetched release: {result:?}"
    );
    assert_eq!(
        result.version.as_deref(),
        Some(RELEASE_VERSION),
        "the result names the release the document signed: {result:?}"
    );
    wait_request_removed(&fixture.install);
    assert_eq!(
        fs::read(staged_exe(&fixture.install))
            .expect("read the staged artifact the downloader wrote"),
        artifact,
        "the bytes the lane downloaded are the bytes the stager committed"
    );
    let health = fixture.wait_healthy_version(RELEASE_VERSION);
    assert_health(&health, RELEASE_VERSION);
    assert_eq!(
        load_sidecar(&baseline_sidecar(&fixture.install))
            .expect("read baseline\\current\\baseline.json")
            .version,
        RELEASE_VERSION,
        "baseline\\current\\baseline.json names the release the lane applied"
    );
    assert_eq!(
        sha256_file(&baseline_exe(&fixture.install)).expect("hash baseline\\current"),
        artifact_sha,
        "`current` holds the artifact whose digest the document signed"
    );
    assert_eq!(
        load_sidecar(&generation_sidecar(&fixture.install, PREVIOUS_DIR))
            .expect("read the previous sidecar")
            .version,
        BASELINE_VERSION,
        "the generation the lane replaced is `previous` and names {BASELINE_VERSION}"
    );
    drop(server);

    // What "nothing changes" will be measured against: the committed generation and the last
    // supervisor answer, both from the 0.4.0 apply above.
    let current_sidecar = fs::read(baseline_sidecar(&fixture.install))
        .expect("read baseline\\current\\baseline.json");
    let current_sha = sha256_file(&baseline_exe(&fixture.install)).expect("hash baseline\\current");
    let last_result =
        fs::read(swap_result(&fixture.install)).expect("read the applied swap result");

    // 2. A signed 0.5.0 canary document is genuine, but it is not this stable device's release.
    //    The policy answer comes before the artifact request, so there is no swap request and no
    //    supervisor answer: exit 4, a message that names the channel, and the last applied result
    //    (not a refusal result) is still on disk. The `/artifact.exe` route is deliberately absent:
    //    a downloader that fetched it would answer 404 and exit 1, which this assertion would see.
    let server = ReleaseServer::start(|base| {
        let document = signed_release(
            CANARY_VERSION,
            "canary",
            &format!("{base}/artifact.exe"),
            &artifact,
            BASELINE_VERSION,
        );
        vec![ReleaseRoute {
            path: "/release.json",
            content_type: "application/json",
            body: serde_json::to_vec(&document).expect("the release document serializes"),
        }]
    });
    let output = check_update(
        &fixture.install,
        &fixture.data,
        &server.url("/release.json"),
        "stable",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(4),
        "a canary document on a stable device is a refusal (4), not a failed attempt (1); \
         stdout was:\n{stdout}\nstderr was:\n{stderr}\n--- launcher stderr ---\n{}",
        fixture.stderr_text()
    );
    assert!(
        stderr.contains("does not accept the release's channel"),
        "the refusal must name the channel, not a transport or staging failure; stderr was:\n{stderr}"
    );
    assert_eq!(
        read_swap_result(&fixture.install).outcome,
        SwapOutcome::Applied,
        "a document refused before staging asks the supervisor nothing, so the last answer is still \
         the 0.4.0 apply"
    );
    assert_eq!(
        fs::read(swap_result(&fixture.install)).expect("read the swap result after the refusal"),
        last_result,
        "a pre-stage refusal must not replace the supervisor's last answer"
    );
    assert!(
        !swap_request(&fixture.install).exists(),
        "a refused document must not leave a swap request behind"
    );
    fixture.assert_launcher_running();
    fixture.wait_healthy_version(RELEASE_VERSION);
    assert_eq!(
        fs::read(baseline_sidecar(&fixture.install))
            .expect("read baseline\\current\\baseline.json after the refusal"),
        current_sidecar,
        "a refused canary release leaves baseline\\current\\baseline.json untouched"
    );
    assert_eq!(
        sha256_file(&baseline_exe(&fixture.install))
            .expect("hash baseline\\current after the refusal"),
        current_sha,
        "a refused canary release leaves the current generation untouched"
    );
    drop(server);

    // The state document is what the version policy compares against. It must name the release the
    // supervisor booted, or the downgrade below would be measured against the wrong fact.
    let (status, body) =
        http_get(fixture.port, "/api/state", Some(&fixture.token)).expect("query /api/state");
    assert_eq!(status, 200, "the running app answers /api/state: {body}");
    let state = State::parse(&body).expect("the app writes the state document");
    assert_eq!(
        state.install.version, RELEASE_VERSION,
        "the state document names {RELEASE_VERSION}, so the next document is a downgrade"
    );

    // 3. A signed, stable 0.3.0 document is a downgrade while the state names 0.4.0. The version
    //    policy must refuse it before the artifact is fetched; nothing may change.
    let server = ReleaseServer::start(|base| {
        let document = signed_release(
            DOWNGRADE_VERSION,
            "stable",
            &format!("{base}/artifact.exe"),
            &artifact,
            BASELINE_VERSION,
        );
        vec![ReleaseRoute {
            path: "/release.json",
            content_type: "application/json",
            body: serde_json::to_vec(&document).expect("the release document serializes"),
        }]
    });
    let output = check_update(
        &fixture.install,
        &fixture.data,
        &server.url("/release.json"),
        "stable",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(4),
        "a signed downgrade from {RELEASE_VERSION} to {DOWNGRADE_VERSION} is a refusal (4); \
         stdout was:\n{stdout}\nstderr was:\n{stderr}\n--- launcher stderr ---\n{}",
        fixture.stderr_text()
    );
    assert!(
        stderr.contains("not newer than the installed version"),
        "the refusal must name the version rule, not a transport or staging failure; stderr \
         was:\n{stderr}"
    );
    assert!(
        !swap_request(&fixture.install).exists(),
        "a downgrade must not leave a swap request behind"
    );
    fixture.assert_launcher_running();
    fixture.wait_healthy_version(RELEASE_VERSION);
    assert_eq!(
        fs::read(baseline_sidecar(&fixture.install))
            .expect("read baseline\\current\\baseline.json after the downgrade"),
        current_sidecar,
        "a refused downgrade leaves baseline\\current\\baseline.json untouched"
    );
    assert_eq!(
        sha256_file(&baseline_exe(&fixture.install))
            .expect("hash baseline\\current after the downgrade"),
        current_sha,
        "a refused downgrade leaves the current generation untouched"
    );
    assert_eq!(
        fs::read(swap_result(&fixture.install)).expect("read the swap result after the downgrade"),
        last_result,
        "a refused downgrade must not replace the supervisor's last answer"
    );
    drop(server);

    fixture.stop_launcher();
    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}

/// The unkeyed half of the key seam: a build without `SUMMRISE_*_PUBKEY` ships the all-zero
/// placeholder, and both binaries must refuse by *name* before they read anything an update could
/// have named. The CI Windows job's last step unsets the variables, rebuilds, and runs exactly this
/// test. It is `#[ignore]`d (I8): the keyed workspace run cannot prove it, and a green skip is not
/// proof. It also refuses to run against a keyed build, so the step cannot pass on the wrong binary.
#[test]
#[ignore = "requires the unkeyed rebuild; the CI job runs it by exact name with --ignored"]
fn a_build_without_a_release_key_refuses_updates_by_name() {
    assert!(
        !launcher_key_was_configured(),
        "this test proves the unkeyed boundary, but the build embedded SUMMRISE_LAUNCHER_PUBKEY. \
         The CI job rebuilds without it and runs this test alone (see the unkeyed step)."
    );
    assert!(
        matches!(
            summrise_launch::parse_pubkey(LAUNCHER_PUBKEY_HEX),
            Err(SigError::Placeholder)
        ),
        "an unkeyed launcher must refuse the placeholder key by name"
    );

    let dir = TempDir::new("placeholder");
    let staged = dir.join("summrise-device.new.exe");
    fs::write(&staged, b"an artifact nobody signed").expect("write the staged fixture");
    let sidecar = Sidecar {
        version: UPDATED_VERSION.to_owned(),
        channel: "stable".to_owned(),
        sha256: "0".repeat(64),
        signature: "0".repeat(128),
    };
    let sidecar_path = dir.join("sidecar.json");
    save_sidecar(&sidecar_path, &sidecar).expect("write the sidecar fixture");

    // The operator-only manual entry point still refuses a placeholder build by name (exit 2 is a
    // usage refusal, not a swap failure).
    let output = Command::new(launcher_exe())
        .arg("--apply")
        .arg(&staged)
        .arg(&sidecar_path)
        .arg("--install")
        .arg(dir.join("install"))
        .arg("--data")
        .arg(dir.join("data"))
        .arg("--port")
        .arg("18081")
        .stdin(Stdio::null())
        .output()
        .expect("run the unkeyed launcher");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unkeyed `--apply` must exit 2 (a usage refusal, not a swap failure); stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("placeholder release key"),
        "the refusal must say the key is the placeholder, not fail on the sidecar; \
         stderr was:\n{stderr}"
    );
    assert!(
        !dir.join("install").join("summrise-device.exe").exists(),
        "a refused update must not write anything"
    );

    // The app's own compile-time seam, through its hidden staging mode. It must refuse before it
    // reads the missing artifact: the message names the key, not the file. `--install` is present
    // because the staging command line itself must parse; the refusal is the key, not the usage.
    let output = Command::new(device_exe())
        .arg("--stage")
        .arg("missing-artifact.exe")
        .arg("missing-manifest.json")
        .arg("--install")
        .arg(dir.join("install"))
        .stdin(Stdio::null())
        .output()
        .expect("run the unkeyed device");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "an unkeyed `--stage` must fail; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("placeholder release key"),
        "the device's refusal must name the placeholder key before reading the artifact; \
         stderr was:\n{stderr}"
    );
}

/// The SCM service mode, once: a service created by `sc.exe` (not by the product's installer) runs
/// `summrise-launch service`, serves, and stops its app child when the SCM asks it to. `Drop` is
/// the `finally`: a panic still stops and deletes the service.
///
/// `#[ignore]`, not an early return (I8): the workspace run on Windows must not silently pass this
/// test, and the CI job runs it by exact name with `--ignored`. It needs an elevated shell
/// (GitHub's Windows runner is elevated).
#[test]
#[ignore = "registers a real SCM service; run elevated by exact name with --ignored"]
fn the_service_mode_serves_and_stops_through_the_scm() {
    require_fixture_key();

    let mut fixture = Fixture::new("scm");
    fixture.prepare_baseline();

    // `binPath=` needs the executable quoted when the path contains spaces; the value is passed as
    // one argv entry and `sc.exe` keeps the inner quotes. `start= auto` is the documented spelling
    // (the space is sc.exe's, not the shell's). `--service-name` is what makes the launcher's
    // dispatcher table key match the name `sc.exe` created: without it the table says
    // `SummriseLaunch` and `StartServiceCtrlDispatcher` refuses this service by name.
    let bin_path = format!(
        "\"{}\" service --service-name {SERVICE_TEST_NAME} --install \"{}\" --data \"{}\" --port {}",
        launcher_exe(),
        fixture.install.display(),
        fixture.data.display(),
        fixture.port
    );

    // A leftover from an interrupted run would fail `create` with 1073; removing it first makes the
    // test retryable on a runner that is reused. Both failures are ignored on purpose.
    let _ = sc(&["stop", SERVICE_TEST_NAME]);
    let _ = sc(&["delete", SERVICE_TEST_NAME]);
    std::thread::sleep(Duration::from_millis(500));

    let create = sc(&[
        "create",
        SERVICE_TEST_NAME,
        "binPath=",
        &bin_path,
        "start=",
        "auto",
    ]);
    assert!(
        create.status.success(),
        "sc create failed (this test needs an elevated shell):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );
    let mut service = ScmService { created: true };

    let start = sc(&["start", SERVICE_TEST_NAME]);
    assert!(
        start.status.success(),
        "sc start failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr)
    );

    let health = fixture.wait_healthy_version(BASELINE_VERSION);
    assert_health(&health, BASELINE_VERSION);
    let (status, body) =
        http_get(fixture.port, "/api/state", Some(&fixture.token)).expect("query /api/state");
    assert_eq!(status, 200, "the service's app answers /api/state: {body}");
    State::parse(&body).expect("the service's app writes the state document");

    // Stop through the SCM, not the console pid files: this is the path a device's service takes.
    let stop = sc(&["stop", SERVICE_TEST_NAME]);
    assert!(
        stop.status.success(),
        "sc stop failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stop.stdout),
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        service.wait_stopped(Duration::from_secs(30)),
        "the service did not reach STOPPED after sc stop"
    );
    // A CLEAN STOP MUST LOOK CLEAN. `ServiceSpecific(0)` reports Win32 1066
    // (ERROR_SERVICE_SPECIFIC_ERROR), the SCM counts that as a failure and runs the configured
    // recovery actions — the service an operator just stopped starts again by itself. The exit code
    // is the observable that decides it.
    assert_eq!(
        service.win32_exit_code(),
        Some(0),
        "a clean stop reports exit code 0, not a service-specific error"
    );
    // ...and it stays stopped: no recovery action may resurrect it inside the stop window.
    std::thread::sleep(Duration::from_secs(5));
    let later = sc(&["query", SERVICE_TEST_NAME]);
    assert!(
        String::from_utf8_lossy(&later.stdout)
            .to_ascii_uppercase()
            .contains("STOPPED"),
        "the recovered service was restarted by the SCM after a clean stop: {}",
        String::from_utf8_lossy(&later.stdout)
    );
    // The launcher's control handler kills the app child on the way down; the port going quiet is
    // the evidence, and it is what a restarting device needs.
    fixture.wait_health_down();

    assert!(
        service.delete(),
        "the service was deleted, not left behind for the runner"
    );

    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}

/// A loopback console that answers `POST /devices` with the id the device just signed with.
///
/// C1's Windows half needs a real enrollment: that mode is what mints `<data>/identity.key` and
/// `<data>/console-token.json`, and the test asserts the DACL of both against the shipped writer. The
/// token is fixture text, not a secret; the endpoint stops in `Drop` even when an assertion panics.
struct EnrollmentConsole {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl EnrollmentConsole {
    fn start() -> EnrollmentConsole {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind the enrollment console to a loopback port");
        listener
            .set_nonblocking(true)
            .expect("poll the enrollment console's listener");
        let port = listener
            .local_addr()
            .expect("the enrollment console's port is known")
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !server_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => serve_enrollment(stream),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        EnrollmentConsole {
            port,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for EnrollmentConsole {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Read one registration request and answer with its own id and a 64-hex fixture token.
fn serve_enrollment(mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    if stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .is_err()
    {
        return;
    }

    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 64 * 1024 {
            return;
        }
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }

    let head_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the header terminator was just found")
        + 4;
    let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < head_end + content_length {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
    let body = &request[head_end..request.len().min(head_end + content_length)];
    let id = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|body| body["id"].as_str().map(str::to_owned));
    let (status, response) = match id {
        Some(id) => (
            "200 OK",
            json!({"id": id, "token": "1".repeat(64)}).to_string(),
        ),
        None => (
            "400 Bad Request",
            json!({"error": "invalid_id"}).to_string(),
        ),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        response.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// The `icacls` reading of a protected path: SYSTEM and BUILTIN\Administrators have full control and
/// BUILTIN\Users is absent. A directory's inheritable ACEs print as `(OI)(CI)(F)`, so `(F)` is the
/// assertion rather than the exact text between the colon and the rights.
fn assert_protected_acl(path: &Path) {
    let output = Command::new("icacls")
        .arg(path)
        .output()
        .expect("icacls is an OS program");
    assert!(
        output.status.success(),
        "icacls {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        !text.to_ascii_lowercase().contains("builtin\\users"),
        "{} grants BUILTIN\\Users:\n{text}",
        path.display()
    );
    for principal in ["SYSTEM:", "BUILTIN\\Administrators:"] {
        let line = text
            .lines()
            .find(|line| line.contains(principal))
            .unwrap_or_else(|| panic!("{} has no {principal} entry:\n{text}", path.display()));
        assert!(
            line.contains("(F)"),
            "{}'s {principal} entry is not full control: {line}",
            path.display()
        );
    }
}

/// The product installer mints `<data>/token` once (review I9) and configures the SCM failure
/// actions (I4).
///
/// C1 is asserted on the same tree: enrollment writes `identity.key` and `console-token.json`, the
/// installer writes `token` and hardens `<data>`, and all four descriptors are read back — both by
/// `summrise_winsec` and by `icacls`, the operator-facing reader.
///
/// This owns the real `SummriseLaunch` service for a moment — the same elevation the SCM test
/// needs. `#[ignore]` and an explicit CI step, because a workspace run must not count a skipped
/// product installer as a pass (I8).
#[test]
#[ignore = "owns the real SummriseLaunch service; run elevated by exact name with --ignored"]
fn the_install_service_mints_the_install_token_once() {
    let dir = TempDir::new("install-token");
    let install = dir.join("install");
    let data = dir.join("data");
    fs::create_dir_all(&install).expect("create the install directory");
    fs::create_dir_all(&data).expect("create the data directory");

    // A current generation so a successfully started supervisor has an app to supervise. The
    // token, not the app, is the fact under test.
    let device = device_exe();
    fs::create_dir_all(baseline_exe(&install).parent().expect("baseline parent"))
        .expect("create the current generation directory");
    fs::copy(&device, baseline_exe(&install)).expect("copy the baseline");
    let sha = sha256_file(&baseline_exe(&install)).expect("hash the baseline");
    save_sidecar(
        &baseline_sidecar(&install),
        &signed_sidecar(BASELINE_VERSION, "stable", &sha),
    )
    .expect("write the signed baseline sidecar");
    fs::copy(&device, app_exe(&install)).expect("install the app binary");

    // C1: a real enrollment creates the identity and the console token, with the directory protected
    // before either is written. The installer below then mints the third credential in the same tree.
    let console = EnrollmentConsole::start();
    let enrolled = Command::new(device_exe())
        .arg("--enroll")
        .arg(console.url())
        .arg("a".repeat(64))
        .arg("--data")
        .arg(&data)
        .stdin(Stdio::null())
        .output()
        .expect("run the device's enrollment mode");
    assert!(
        enrolled.status.success(),
        "--enroll failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&enrolled.stdout),
        String::from_utf8_lossy(&enrolled.stderr)
    );
    let identity = data.join("identity.key");
    let console_token = data.join("console-token.json");
    assert!(
        identity.is_file(),
        "enrollment mints {}",
        identity.display()
    );
    assert!(
        console_token.is_file(),
        "enrollment writes {}",
        console_token.display()
    );

    let port = free_port();
    let _service = ProductService;
    // C3: the supervised app is armed through `<data>/console.json`; the service argv carries no
    // console fact because the SCM gives the installer no way to set its environment.
    let first = install_product_service(
        &install,
        &data,
        port,
        None,
        Some(("https://console.example.test", &console_token)),
    );
    assert!(
        first.status.success(),
        "--install-service failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    // I4/I8: the product installer, not `sc.exe`, configured the restart policy. `sc qfailure` is
    // the SCM's own reading of it; three restarts five seconds apart (and the one-hour reset).
    assert_failure_actions(SERVICE_NAME_FOR_PRODUCT, 3, 5000, 3600);

    // C3: the link lands beside the token, names the URL and token file the operator passed, and
    // the service argv names the same data directory the app will read it from.
    let link_path = data.join("console.json");
    let link: Value = serde_json::from_slice(&fs::read(&link_path).unwrap_or_else(|error| {
        panic!(
            "the installer did not write {}: {error}",
            link_path.display()
        )
    }))
    .expect("the console link is JSON");
    assert_eq!(link["url"], "https://console.example.test");
    assert_eq!(
        Path::new(link["token_file"].as_str().expect("token_file is a string")),
        console_token.as_path(),
        "the persisted token path is the absolute file the operator named"
    );
    let link_before = fs::read(&link_path).expect("read the console link");
    let command_line = service_command_line(SERVICE_NAME_FOR_PRODUCT);
    assert!(
        command_line.contains("--data") && command_line.contains(&data.display().to_string()),
        "the service argv must name the data directory the console link lives in: {command_line}"
    );
    assert!(
        command_line.contains(&format!("--port {port}")),
        "the service argv names the port the installer was given: {command_line}"
    );

    let token_path = install_token(&data);
    let first_bytes = fs::read(&token_path).unwrap_or_else(|error| {
        panic!(
            "the installer did not create {}: {error}",
            token_path.display()
        )
    });
    assert_eq!(
        first_bytes.len(),
        65,
        "the token is 32 bytes as 64 hex characters plus a newline: {:?}",
        String::from_utf8_lossy(&first_bytes)
    );
    assert!(
        first_bytes[..64]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "the token is lower-hex: {:?}",
        String::from_utf8_lossy(&first_bytes)
    );
    assert_eq!(first_bytes[64], b'\n', "the token ends with one newline");

    // C1: the directory and all three credentials are readable by SYSTEM and Administrators only.
    // The safe API's read-back and `icacls` (what an operator sees) are both asserted.
    for path in [&data, &identity, &token_path, &console_token] {
        summrise_winsec::verify(path)
            .unwrap_or_else(|error| panic!("{} is not protected: {error}", path.display()));
        assert_protected_acl(path);
    }

    // The read-back refuses a copy that was written without protection — the restored-backup or
    // hand-copied case the guard exists for. The copy is a fresh file under the test root, so it
    // inherits that directory's DACL rather than the protected file's.
    let unprotected = dir.join("console-token.unprotected.json");
    fs::write(
        &unprotected,
        fs::read(&console_token).expect("the protected token is readable"),
    )
    .expect("the unprotected copy is written");
    let refusal = summrise_winsec::verify(&unprotected)
        .expect_err("a copy without the protected DACL is refused");
    assert!(
        refusal.contains(&unprotected.display().to_string()),
        "the refusal names the path: {refusal}"
    );

    // Idempotence: the bytes must be the same after a second install. The second run deliberately
    // omits the console flags: re-running the installer must not disarm a working link.
    let second = install_product_service(&install, &data, port, None, None);
    assert!(
        second.status.success(),
        "the second --install-service failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        fs::read(&token_path).expect("read the token after the second install"),
        first_bytes,
        "an existing install token must be left byte-for-byte alone"
    );
    assert_eq!(
        fs::read(&link_path).expect("read the console link after the second install"),
        link_before,
        "a re-run without the console flags leaves an existing link in place"
    );
}

/// NEW-1: a fresh tree with no baseline is installable when the installer ships a sidecar.
///
/// The installer seeds `<install>\baseline\current\` from the app it downloaded plus
/// `--seed-sidecar`, verifies the signature with its embedded key and the sha256, then starts the
/// service; health must serve from that seeded generation.
#[test]
#[ignore = "owns the real SummriseLaunch service; run elevated by exact name with --ignored"]
fn the_installer_seeds_a_missing_baseline_from_the_shipped_sidecar() {
    require_fixture_key();

    let fixture = Fixture::new("install-seed");
    let app = app_exe(&fixture.install);
    fs::copy(device_exe(), &app).expect("stage the downloaded app the installer will seed from");
    let sha = sha256_file(&app).expect("hash the downloaded app");
    let seed = fixture.root.join("seed-baseline.json");
    save_sidecar(&seed, &signed_sidecar(BASELINE_VERSION, "stable", &sha))
        .expect("write the sidecar the installer ships");
    assert!(
        !baseline_exe(&fixture.install).exists(),
        "the fixture tree starts with no baseline generation"
    );

    // No seed and no flat pair: the installer must refuse before it opens the service manager, so
    // nothing is registered and the exit is a failure, not a service that starts into NeedsHuman.
    let refused =
        install_product_service(&fixture.install, &fixture.data, fixture.port, None, None);
    let refused_stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        !refused.status.success(),
        "an unseeded tree must not install a service:\nstdout: {}\nstderr: {refused_stderr}",
        String::from_utf8_lossy(&refused.stdout)
    );
    assert!(
        refused_stderr.contains("--seed-sidecar"),
        "the refusal must name the flag that would have seeded it: {refused_stderr}"
    );
    assert!(
        !sc(&["query", SERVICE_NAME_FOR_PRODUCT]).status.success(),
        "a refused install must not register {SERVICE_NAME_FOR_PRODUCT}"
    );

    let service = ProductService;
    let output = install_product_service(
        &fixture.install,
        &fixture.data,
        fixture.port,
        Some(&seed),
        None,
    );
    assert!(
        output.status.success(),
        "--install-service could not seed a baseline:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // `current` appeared with the generation names, and it is the pair the sidecar describes.
    assert_eq!(
        fs::read(baseline_exe(&fixture.install)).expect("read the seeded current binary"),
        fs::read(&app).expect("read the downloaded app"),
        "the seeded binary is the installer's downloaded app"
    );
    let current =
        load_sidecar(&baseline_sidecar(&fixture.install)).expect("read the seeded current sidecar");
    assert_eq!(current.version, BASELINE_VERSION);
    assert_eq!(current.sha256, sha);
    assert_eq!(
        sha256_file(&baseline_exe(&fixture.install)).expect("hash the seeded current binary"),
        sha,
        "the seeded generation hashes to its sidecar"
    );

    // And the service boots it: the seeded generation is a supervised app, not just two files.
    fixture.wait_healthy_version(BASELINE_VERSION);

    drop(service);
    fixture.wait_health_down();
    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}

/// C1, the refusal half: `--seed-sidecar` is the launcher key's job. A sidecar signed with the
/// release key (the tool the operator uses for release documents) must be refused by name, before
/// the service manager is opened, and the message must point at the launcher key rather than read
/// as a corrupt signature.
#[test]
#[ignore = "owns the real SummriseLaunch service; run elevated by exact name with --ignored"]
fn the_installer_refuses_a_seed_signed_with_the_release_key() {
    require_fixture_key();

    let fixture = Fixture::new("install-seed-release-key");
    let app = app_exe(&fixture.install);
    fs::copy(device_exe(), &app).expect("stage the downloaded app the installer will seed from");
    let sha = sha256_file(&app).expect("hash the downloaded app");
    let seed = fixture.root.join("seed-release.json");
    save_sidecar(
        &seed,
        &release_signed_sidecar(BASELINE_VERSION, "stable", &sha),
    )
    .expect("write a sidecar signed with the release key");

    let output = install_product_service(
        &fixture.install,
        &fixture.data,
        fixture.port,
        Some(&seed),
        None,
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a release-key-signed seed must not install:\nstdout: {}\nstderr: {stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("launcher key"),
        "the refusal must say which key owns the seed: {stderr}"
    );
    assert!(
        stderr.contains("SUMMRISE_LAUNCHER_PUBKEY"),
        "the refusal must name the build-time variable that carries the launcher key: {stderr}"
    );
    assert!(
        !baseline_exe(&fixture.install).exists(),
        "a refused seed must not create a current generation"
    );
    assert!(
        !sc(&["query", SERVICE_NAME_FOR_PRODUCT]).status.success(),
        "a refused install must not register {SERVICE_NAME_FOR_PRODUCT}"
    );

    let root = fixture.root.clone();
    drop(fixture);
    assert!(
        !root.exists(),
        "the temporary install tree survived cleanup: {}",
        root.display()
    );
}
/// NEW-1: the pre-generation flat pair (`baseline\summrise-device.exe` + `summrise-device.json`)
/// migrates into `baseline\current\` (renaming the sidecar to `baseline.json`) instead of leaving an
/// upgraded install with no bootable generation.
#[test]
fn the_installer_migrates_the_pre_generation_flat_baseline_pair() {
    require_fixture_key();

    let dir = TempDir::new("install-migrate");
    let install = dir.join("install");
    fs::create_dir_all(baseline_dir(&install)).expect("create the baseline directory");
    fs::copy(device_exe(), flat_exe(&install)).expect("write the flat binary");
    let sha = sha256_file(&flat_exe(&install)).expect("hash the flat binary");
    save_sidecar(
        &flat_sidecar(&install),
        &signed_sidecar(BASELINE_VERSION, "stable", &sha),
    )
    .expect("write the flat sidecar");

    let outcome = ensure_baseline(&install, None, FIXTURE_LAUNCHER_PUBKEY_HEX, false)
        .expect("a flat pair migrates without a seed sidecar");

    assert_eq!(outcome, BaselineInstall::Migrated);
    assert_eq!(
        sha256_file(&baseline_exe(&install)).expect("hash the migrated current binary"),
        sha,
        "`current` holds the flat pair's bytes"
    );
    let current =
        load_sidecar(&baseline_sidecar(&install)).expect("read the migrated current sidecar");
    assert_eq!(current.version, BASELINE_VERSION);
    assert_eq!(current.sha256, sha);
    assert!(
        flat_exe(&install).is_file() && flat_sidecar(&install).is_file(),
        "migration copies the flat pair; it does not delete it"
    );
}

/// NEW-2: one supervisor per install. A second `run` against the same install must exit nonzero
/// quickly and name the install, and the first supervisor must keep serving.
#[test]
fn a_second_supervisor_for_the_same_install_exits_nonzero() {
    require_fixture_key();

    let mut fixture = Fixture::new("single-instance");
    fixture.prepare_baseline();
    fixture.start_launcher();
    fixture.wait_healthy_version(BASELINE_VERSION);

    let started = Instant::now();
    let second = Command::new(launcher_exe())
        .arg("run")
        .arg("--install")
        .arg(&fixture.install)
        .arg("--data")
        .arg(&fixture.data)
        .arg("--port")
        .arg(fixture.port.to_string())
        .stdin(Stdio::null())
        .output()
        .expect("run a second supervisor for the same install");
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&second.stderr);

    assert!(
        !second.status.success(),
        "a second supervisor must exit nonzero instead of racing the first: {stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "the rejected supervisor exited after {elapsed:?}, not promptly"
    );
    assert!(
        stderr.contains("another supervisor"),
        "the refusal must name the other instance: {stderr}"
    );
    assert!(
        stderr.contains(&fixture.install.display().to_string()),
        "the refusal must name the install: {stderr}"
    );

    fixture.assert_launcher_running();
    fixture.wait_healthy_version(BASELINE_VERSION);
    fixture.stop_launcher();
}

fn install_product_service(
    install: &Path,
    data: &Path,
    port: u16,
    seed_sidecar: Option<&Path>,
    console: Option<(&str, &Path)>,
) -> std::process::Output {
    let mut command = Command::new(launcher_exe());
    command.arg("--install-service");
    if let Some(seed) = seed_sidecar {
        command.arg("--seed-sidecar").arg(seed);
    }
    if let Some((url, token_file)) = console {
        command
            .arg("--console-url")
            .arg(url)
            .arg("--console-token-file")
            .arg(token_file);
    }
    command
        .arg("--install")
        .arg(install)
        .arg("--data")
        .arg(data)
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .output()
        .expect("run --install-service")
}

/// The SCM's own reading of the service's binary path and arguments (`sc qc`).
fn service_command_line(service: &str) -> String {
    let output = sc(&["qc", service]);
    assert!(
        output.status.success(),
        "sc qc {service} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Assert the SCM's own view of a service's failure actions: `restarts` `RESTART` entries with
/// `delay_ms`, and the declared reset period in seconds. `sc.exe`'s output is English on the CI
/// runner; the values are the product fact, not the wording.
fn assert_failure_actions(service: &str, restarts: usize, delay_ms: u64, reset_seconds: u64) {
    let output = sc(&["qfailure", service]);
    assert!(
        output.status.success(),
        "sc qfailure {service} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_uppercase();
    assert_eq!(
        text.matches("RESTART").count(),
        restarts,
        "the SCM must hold {restarts} restart actions for {service}: {text}"
    );
    assert!(
        text.contains(&format!("{delay_ms} MILLISECONDS")),
        "each restart waits {delay_ms} ms: {text}"
    );
    assert!(
        text.contains(&format!("{reset_seconds}")),
        "the failure counter resets after {reset_seconds} seconds: {text}"
    );
}

/// The product service this test owns. `Drop` is the `finally`: a panic still uninstalls it.
struct ProductService;

impl Drop for ProductService {
    fn drop(&mut self) {
        let _ = Command::new(launcher_exe())
            .arg("--uninstall-service")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn sc(args: &[&str]) -> std::process::Output {
    Command::new(system32("sc.exe"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("cannot run sc.exe: {error}"))
}

/// The service `sc.exe` created. Dropping it stops and deletes the service, so the `finally` holds
/// even when an assertion above it panics.
struct ScmService {
    created: bool,
}

impl ScmService {
    /// The service's Win32 exit code as the SCM records it (`sc query`'s WIN32_EXIT_CODE line).
    fn win32_exit_code(&self) -> Option<u32> {
        let query = sc(&["query", SERVICE_TEST_NAME]);
        let text = String::from_utf8_lossy(&query.stdout);
        let line = text
            .lines()
            .find(|line| line.to_ascii_uppercase().contains("WIN32_EXIT_CODE"))?;
        // sc prints "WIN32_EXIT_CODE    : 0  (0x0)"; the first token after the colon is the code
        // and the parenthesised hex is sc's own restatement of it.
        line.split(':')
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    fn wait_stopped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let query = sc(&["query", SERVICE_TEST_NAME]);
            let text = String::from_utf8_lossy(&query.stdout).to_ascii_uppercase();
            if text.contains("STOPPED") {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Stop and delete the service; true when it is gone. Idempotent: deleting what is not there
    /// is success, so a panic before `sc create` cannot turn cleanup into a second failure.
    fn delete(&mut self) -> bool {
        if !self.created {
            return true;
        }
        let _ = sc(&["stop", SERVICE_TEST_NAME]);
        let _ = self.wait_stopped(Duration::from_secs(30));
        // A service that was just asked to stop can be "marked for deletion" (1072) for a moment.
        for _ in 0..20 {
            if sc(&["delete", SERVICE_TEST_NAME]).status.success() {
                self.created = false;
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }
}

impl Drop for ScmService {
    fn drop(&mut self) {
        if !self.delete() {
            eprintln!(
                "warning: could not delete the {SERVICE_TEST_NAME} service; the runner is not reused"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// The fixture's own temp directory
// ─────────────────────────────────────────────────────────────────────────────────────────────────

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "summrise-fixture-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the fixture temp directory");
        TempDir(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
