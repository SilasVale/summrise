//! The signer's command line: a key file in, one signed document out (design §4.4, §6).
//!
//! These tests run the real binary. The last one constructs the same key pair from the seed it
//! wrote and hands the document to the launcher's `parse_pubkey`/`verify_signature` — the path a
//! device actually uses — so the CLI is measured against the consumer, not against itself.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ed25519_dalek::{Signer, SigningKey};
use summrise_release::{Channel, ReleaseDocument, Revocations};

const ARTIFACT: &[u8] = b"the release bytes";
const ARTIFACT_SHA256: &str = "86c0ea10853cdebc340b41ccd1a151ff282ec570bad37f52fa2321dd34b7479f";

fn seed_hex() -> String {
    "07".repeat(32)
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};

        let nonce = RandomState::new().build_hasher().finish();
        let path = std::env::temp_dir().join(format!(
            "summrise-release-{label}-{}-{nonce:016x}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("the test directory is created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .expect("the test key's mode is set");
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

fn write_key(path: &Path, mode: u32) {
    fs::write(path, format!("{}\n", seed_hex())).expect("the key file is written");
    set_mode(path, mode);
}

fn write_artifact(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("summrise-device.exe");
    fs::write(&path, ARTIFACT).expect("the artifact is written");
    path
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_summrise-release"))
}

fn sign_command(key: &Path, artifact: &Path, out: &Path) -> Command {
    let mut command = cli();
    command
        .arg("sign")
        .arg("--key-file")
        .arg(key)
        .arg("--artifact")
        .arg(artifact)
        .arg("--version")
        .arg("2.0.3")
        .arg("--channel")
        .arg("stable")
        .arg("--url")
        .arg("https://example.test/summrise-device.exe")
        .arg("--min-version")
        .arg("2.0.0")
        .arg("--out")
        .arg(out);
    command
}

fn public_hex_from_the_seed() -> String {
    // Construct the key pair the test wrote to disk, independently of the signer's own seed parser.
    signing_key()
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn run(command: &mut Command) -> Output {
    command.output().expect("the CLI runs")
}

#[test]
fn the_cli_signs_a_document_the_launcher_verifies() {
    let dir = TempDir::new("sign");
    let key = dir.path().join("release.key");
    write_key(&key, 0o600);
    let artifact = write_artifact(&dir);
    let out = dir.path().join("release.json");

    let output = run(&mut sign_command(&key, &artifact, &out));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: ReleaseDocument =
        serde_json::from_slice(&fs::read(&out).expect("the document is written"))
            .expect("the document parses");

    assert_eq!(document.version, "2.0.3");
    assert_eq!(document.channel, Channel::Stable);
    assert_eq!(document.sha256, ARTIFACT_SHA256);
    assert_eq!(
        document.artifact_url,
        "https://example.test/summrise-device.exe"
    );
    assert_eq!(document.min_version, "2.0.0");
    assert!(document.issued_at_ms > 0, "the document is timestamped");
    assert!(
        document
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "the digest is lowercase hex: {}",
        document.sha256
    );

    // The seed is never a log line, on either stream.
    let seed = seed_hex();
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&seed));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&seed));

    // The device's own path: `parse_pubkey` on the same key pair, then the launcher's
    // `verify_signature` over exactly `"{version}\n{channel}\n{sha256}"`.
    let public = summrise_launch::parse_pubkey(&public_hex_from_the_seed())
        .expect("the same key pair parses as a device key");
    assert_eq!(summrise_release::verify(&document, &public), Ok(()));

    let sidecar = summrise_launch::Sidecar {
        version: document.version.clone(),
        channel: document.channel.as_str().to_owned(),
        sha256: document.sha256.clone(),
        signature: document.artifact_signature.clone(),
    };
    assert_eq!(
        summrise_launch::verify_signature(&sidecar, &public),
        Ok(()),
        "the launcher accepts the artifact signature the CLI wrote"
    );
    assert_eq!(
        document.artifact_signature,
        hex_encode(
            &signing_key()
                .sign(summrise_release::artifact_message(&document).as_bytes())
                .to_bytes()
        ),
        "artifactSignature is the stager's three-part message"
    );
    assert_eq!(
        document.signature,
        hex_encode(
            &signing_key()
                .sign(summrise_release::canonical_message(&document).as_bytes())
                .to_bytes()
        ),
        "signature covers the whole policy"
    );
}

#[cfg(unix)]
#[test]
fn the_cli_refuses_a_key_file_that_others_can_read() {
    let dir = TempDir::new("key-mode");
    let key = dir.path().join("release.key");
    write_key(&key, 0o644);
    let artifact = write_artifact(&dir);
    let out = dir.path().join("release.json");
    fs::write(&out, b"do not touch").expect("the sentinel document is written");

    let output = run(&mut sign_command(&key, &artifact, &out));

    assert_eq!(
        output.status.code(),
        Some(1),
        "a refused signing is an operational failure"
    );
    assert!(!output.status.success(), "a 0644 key file must be refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("0600"),
        "the error names the required mode: {stderr}"
    );
    assert!(
        stderr.contains("0644"),
        "the error names the actual mode: {stderr}"
    );
    assert_eq!(
        fs::read(&out).expect("the sentinel is readable"),
        b"do not touch",
        "a refusal writes nothing"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&seed_hex()));
}

#[test]
fn the_cli_refuses_a_missing_flag() {
    let output = run(cli().arg("sign"));

    assert_eq!(
        output.status.code(),
        Some(2),
        "a usage error is exit code 2: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("--key-file"));
}

fn revoke_command(key: &Path, out: &Path) -> Command {
    let mut command = cli();
    command
        .arg("revoke")
        .arg("--key-file")
        .arg(key)
        .arg("--out")
        .arg(out);
    command
}

fn read_revocations(path: &Path) -> Revocations {
    serde_json::from_slice(&fs::read(path).expect("the revocations list is written"))
        .expect("the revocations list parses")
}

#[test]
fn the_cli_revoke_round_trips_and_appends() {
    let dir = TempDir::new("revoke-round-trip");
    let key = dir.path().join("release.key");
    write_key(&key, 0o600);
    let artifact = write_artifact(&dir);
    let out = dir.path().join("release.json");
    let output = run(&mut sign_command(&key, &artifact, &out));
    assert!(output.status.success(), "the fixture document signs");

    let list_path = dir.path().join("revocations.json");
    let mut command = revoke_command(&key, &list_path);
    command
        .arg("--document")
        .arg(&out)
        .arg("--reason")
        .arg("boot loop on Windows 11");
    let output = run(&mut command);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let first = read_revocations(&list_path);
    assert!(first.issued_at_ms > 0, "the list is timestamped");
    assert_eq!(first.revoked.len(), 1);
    assert_eq!(first.revoked[0].version, "2.0.3");
    assert_eq!(first.revoked[0].sha256, ARTIFACT_SHA256);
    assert_eq!(
        first.revoked[0].reason.as_deref(),
        Some("boot loop on Windows 11")
    );
    assert_eq!(
        first.verify(&signing_key().verifying_key()),
        Ok(()),
        "the release key verifies the CLI's list"
    );
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&list_path).expect("the list is readable"))
            .expect("the list is JSON");
    assert_eq!(value["revoked"][0]["version"], serde_json::json!("2.0.3"));
    assert!(
        value.get("issued_at_ms").is_none(),
        "the wire spelling is camelCase: {value}"
    );

    // Appending reads the existing list and writes the same path atomically. The digest is given
    // in uppercase on purpose: the signed bytes require lowercase hex, so the CLI normalizes it.
    let second_sha = "AB".repeat(32);
    let mut command = revoke_command(&key, &list_path);
    command
        .arg("--from")
        .arg(&list_path)
        .arg("--version")
        .arg("2.0.4")
        .arg("--sha256")
        .arg(&second_sha)
        .arg("--reason")
        .arg("rebuilt with the installer fix");
    let output = run(&mut command);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let second = read_revocations(&list_path);
    assert_eq!(second.revoked.len(), 2, "the existing entry is kept");
    assert_eq!(second.revoked[0], first.revoked[0]);
    assert_eq!(second.revoked[1].version, "2.0.4");
    assert_eq!(second.revoked[1].sha256, "ab".repeat(32));
    assert_eq!(second.verify(&signing_key().verifying_key()), Ok(()));

    // A duplicate pair is refused, and the file is left exactly as it was.
    let before = fs::read(&list_path).expect("the list is readable");
    let mut command = revoke_command(&key, &list_path);
    command
        .arg("--from")
        .arg(&list_path)
        .arg("--version")
        .arg("2.0.4")
        .arg("--sha256")
        .arg(&second_sha);
    let output = run(&mut command);
    assert!(!output.status.success(), "a duplicate must be refused");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("appears more than once") && stderr.contains("2.0.4"),
        "the refusal names the duplicate: {stderr}"
    );
    assert_eq!(
        fs::read(&list_path).expect("the list is still readable"),
        before,
        "a refused append writes nothing"
    );
}

#[test]
fn the_cli_revoke_can_start_a_list_without_a_document() {
    let dir = TempDir::new("revoke-explicit");
    let key = dir.path().join("release.key");
    write_key(&key, 0o600);
    let list_path = dir.path().join("revocations.json");
    let sha256 = "CD".repeat(32);

    let mut command = revoke_command(&key, &list_path);
    command
        .arg("--version")
        .arg("2.0.3")
        .arg("--sha256")
        .arg(&sha256)
        .arg("--reason")
        .arg("withdrawn before rollout");
    let output = run(&mut command);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list = read_revocations(&list_path);
    assert_eq!(list.revoked.len(), 1);
    assert_eq!(list.revoked[0].version, "2.0.3");
    assert_eq!(list.revoked[0].sha256, "cd".repeat(32));
    assert_eq!(
        list.revoked[0].reason.as_deref(),
        Some("withdrawn before rollout")
    );
    assert_eq!(list.verify(&signing_key().verifying_key()), Ok(()));
}

#[cfg(unix)]
#[test]
fn the_cli_revoke_refuses_a_key_file_that_others_can_read() {
    let dir = TempDir::new("revoke-key-mode");
    let key = dir.path().join("release.key");
    write_key(&key, 0o644);
    let list_path = dir.path().join("revocations.json");

    let mut command = revoke_command(&key, &list_path);
    command
        .arg("--version")
        .arg("2.0.3")
        .arg("--sha256")
        .arg("ab".repeat(32));
    let output = run(&mut command);

    assert!(!output.status.success(), "a 0644 key file must be refused");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("0600") && stderr.contains("0644"),
        "the error names both modes: {stderr}"
    );
    assert!(
        !list_path.exists(),
        "a refused revoke must not write a list"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&seed_hex()));
}

#[test]
fn a_bad_revoke_command_line_is_a_usage_error() {
    let dir = TempDir::new("revoke-usage");
    let key = dir.path().join("release.key");
    write_key(&key, 0o600);
    let out = dir.path().join("revocations.json");
    let sha256 = "ab".repeat(32);
    let document = dir.path().join("release.json");

    for extra in [
        vec![],
        vec![OsString::from("--version"), OsString::from("2.0.3")],
        vec![OsString::from("--sha256"), OsString::from(&sha256)],
        vec![
            OsString::from("--document"),
            document.as_os_str().to_owned(),
            OsString::from("--version"),
            OsString::from("2.0.3"),
            OsString::from("--sha256"),
            OsString::from(&sha256),
        ],
        vec![
            OsString::from("--version"),
            OsString::from("2.0"),
            OsString::from("--sha256"),
            OsString::from(&sha256),
        ],
        vec![
            OsString::from("--version"),
            OsString::from("2.0.3"),
            OsString::from("--sha256"),
            OsString::from("not-a-digest"),
        ],
        vec![OsString::from("--unknown")],
    ] {
        let mut command = revoke_command(&key, &out);
        command.args(extra);
        let output = run(&mut command);
        assert_eq!(
            output.status.code(),
            Some(2),
            "a bad revoke line is a usage error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!out.exists(), "a usage error writes nothing");
    }
}
