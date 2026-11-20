//! Real offline CLI checks: trusted artifact bytes, strict metadata and bounded input.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Output};

use ed25519_dalek::{Signer, SigningKey};
use rand_core::{OsRng, RngCore};
use serde_json::Value;
use summrise_release::{verify_artifact_file, ArtifactVerificationError, Channel};

const VERSION: &str = "2.0.987";
const ARTIFACT: &[u8] = b"a disposable signed artifact, not an installer format fixture";
const MAX_ARTIFACT: u64 = 128 * 1024 * 1024;
const MAX_SIDECAR: usize = 64 * 1024;

struct Fixture {
    dir: PathBuf,
    artifact: PathBuf,
    sidecar: PathBuf,
    key_file: PathBuf,
    public_key: String,
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_summrise-release"));
    // No production seed/key environment participates in a fixture.
    command.env_clear();
    command
}

fn run(command: &mut Command) -> Output {
    command.output().expect("the real release CLI starts")
}

fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    if expected != 0 {
        assert!(
            output.stdout.is_empty(),
            "a refusal must not report verification"
        );
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "summrise verify artifact {} {:016x}",
            std::process::id(),
            OsRng.next_u64()
        ));
        fs::create_dir(&dir).expect("a private test directory is created");
        let key_file = dir.join("ephemeral test seed");
        fs::write(&key_file, "17".repeat(32)).expect("a fresh seed file is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let artifact = dir.join("SummriseSetup-2.0.987-canary.exe");
        let sidecar = dir.join("SummriseSetup-2.0.987-canary.sidecar.json");
        fs::write(&artifact, ARTIFACT).unwrap();
        let public_key = summrise_release::hex_encode(
            SigningKey::from_bytes(&[0x17; 32])
                .verifying_key()
                .as_bytes(),
        );
        let fixture = Self {
            dir,
            artifact,
            sidecar,
            key_file,
            public_key,
        };
        fixture.sign(VERSION, "canary");
        fixture
    }

    fn sign(&self, version: &str, channel: &str) {
        let result = run(cli()
            .arg("sidecar")
            .arg("--key-file")
            .arg(&self.key_file)
            .arg("--artifact")
            .arg(&self.artifact)
            .arg("--version")
            .arg(version)
            .arg("--channel")
            .arg(channel)
            .arg("--out")
            .arg(&self.sidecar));
        assert_exit(&result, 0);
        for stream in [&result.stdout, &result.stderr] {
            assert!(!String::from_utf8_lossy(stream).contains(&"17".repeat(32)));
        }
    }

    fn verify(&self) -> Command {
        let mut command = cli();
        command
            .arg("verify-artifact")
            .arg("--artifact")
            .arg(&self.artifact)
            .arg("--sidecar")
            .arg(&self.sidecar)
            .arg("--public-key")
            .arg(&self.public_key)
            .arg("--version")
            .arg(VERSION)
            .arg("--channel")
            .arg("canary");
        command
    }

    fn metadata(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.sidecar).unwrap()).unwrap()
    }

    fn write_metadata(&self, value: &Value) {
        fs::write(&self.sidecar, serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn entries(&self) -> Vec<PathBuf> {
        let mut entries: Vec<_> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        entries
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the exact directory this fixture created is removed.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn valid_signed_bytes_verify_without_writing_any_file() {
    let fixture = Fixture::new();
    let before = fixture.entries();
    let sidecar = fs::read(&fixture.sidecar).unwrap();
    let output = run(&mut fixture.verify());
    assert_exit(&output, 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("verified"));
    assert_eq!(fs::read(&fixture.artifact).unwrap(), ARTIFACT);
    assert_eq!(fs::read(&fixture.sidecar).unwrap(), sidecar);
    assert_eq!(fixture.entries(), before);
}

#[test]
fn artifact_tampering_and_a_device_sidecar_for_different_bytes_are_rejected() {
    for bytes in [
        b"tampered setup".as_slice(),
        b"different device binary".as_slice(),
    ] {
        let fixture = Fixture::new();
        fs::write(&fixture.artifact, bytes).unwrap();
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn a_valid_signature_from_a_different_key_is_rejected() {
    let fixture = Fixture::new();
    let wrong_public = summrise_release::hex_encode(
        SigningKey::from_bytes(&[0x18; 32])
            .verifying_key()
            .as_bytes(),
    );
    let output = run(cli()
        .arg("verify-artifact")
        .arg("--artifact")
        .arg(&fixture.artifact)
        .arg("--sidecar")
        .arg(&fixture.sidecar)
        .arg("--public-key")
        .arg(wrong_public)
        .arg("--version")
        .arg(VERSION)
        .arg("--channel")
        .arg("canary"));
    assert_exit(&output, 1);
}

#[test]
fn an_edited_signature_or_unsigned_metadata_is_rejected() {
    for (field, value) in [
        ("signature", "00".repeat(64)),
        ("sha256", "00".repeat(32)),
        ("version", "2.0.988".to_owned()),
        ("channel", "stable".to_owned()),
    ] {
        let fixture = Fixture::new();
        let mut sidecar = fixture.metadata();
        sidecar[field] = value.into();
        fixture.write_metadata(&sidecar);
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn a_valid_sidecar_for_another_version_or_channel_is_rejected() {
    for (version, channel) in [("2.0.988", "canary"), (VERSION, "stable")] {
        let fixture = Fixture::new();
        fixture.sign(version, channel);
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn malformed_duplicate_unknown_and_trailing_sidecar_fields_are_rejected() {
    let fixture = Fixture::new();
    let valid = fs::read_to_string(&fixture.sidecar).unwrap();
    let duplicate = valid.replacen('{', "{\"version\":\"2.0.987\",", 1);
    let unknown = valid.replacen('{', "{\"publicKey\":\"mirror supplied\",", 1);
    for bytes in [
        b"not json".to_vec(),
        b"{}".to_vec(),
        duplicate.into_bytes(),
        unknown.into_bytes(),
        format!("{valid} {{}}").into_bytes(),
        vec![0xff, 0xfe],
        b"".to_vec(),
    ] {
        fs::write(&fixture.sidecar, bytes).unwrap();
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn malformed_digest_signature_version_and_channel_are_rejected() {
    for (field, value) in [
        ("sha256", "xyz"),
        ("signature", "xyz"),
        ("version", "2.0.987\ncanary"),
        ("version", "v2.0.987"),
        ("channel", "nightly"),
    ] {
        let fixture = Fixture::new();
        let mut sidecar = fixture.metadata();
        sidecar[field] = value.into();
        fixture.write_metadata(&sidecar);
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn missing_and_non_regular_inputs_are_operational_failures() {
    for artifact_missing in [true, false] {
        let fixture = Fixture::new();
        let path = if artifact_missing {
            &fixture.artifact
        } else {
            &fixture.sidecar
        };
        fs::remove_file(path).unwrap();
        assert_exit(&run(&mut fixture.verify()), 1);
        fs::create_dir(path).unwrap();
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn empty_artifacts_and_oversize_files_are_rejected() {
    let fixture = Fixture::new();
    fs::write(&fixture.artifact, []).unwrap();
    fixture.sign(VERSION, "canary");
    assert_exit(&run(&mut fixture.verify()), 1);

    let fixture = Fixture::new();
    File::options()
        .write(true)
        .open(&fixture.artifact)
        .unwrap()
        .set_len(MAX_ARTIFACT + 1)
        .unwrap();
    assert_exit(&run(&mut fixture.verify()), 1);

    let fixture = Fixture::new();
    let mut padded = fs::read(&fixture.sidecar).unwrap();
    padded.resize(MAX_SIDECAR, b' ');
    fs::write(&fixture.sidecar, &padded).unwrap();
    assert_exit(&run(&mut fixture.verify()), 0);
    padded.push(b' ');
    fs::write(&fixture.sidecar, &padded).unwrap();
    assert_exit(&run(&mut fixture.verify()), 1);
}

#[test]
fn signed_uppercase_digest_spelling_is_preserved_during_verification() {
    let fixture = Fixture::new();
    let mut sidecar = fixture.metadata();
    let uppercase = sidecar["sha256"].as_str().unwrap().to_ascii_uppercase();
    let message = format!("{VERSION}\ncanary\n{uppercase}");
    let signature = SigningKey::from_bytes(&[0x17; 32]).sign(message.as_bytes());
    sidecar["sha256"] = uppercase.into();
    sidecar["signature"] = summrise_release::hex_encode(&signature.to_bytes()).into();
    fixture.write_metadata(&sidecar);
    assert_exit(&run(&mut fixture.verify()), 0);
}

#[test]
fn matching_app_bytes_renamed_as_setup_still_verify_the_generic_format() {
    let fixture = Fixture::new();
    let app = fixture.dir.join("summrise-device.exe");
    fs::rename(&fixture.artifact, &app).unwrap();
    fs::rename(&app, &fixture.artifact).unwrap();
    // The existing signed message has no artifact kind or path. This is a deliberate limit.
    assert_exit(&run(&mut fixture.verify()), 0);
}

#[test]
fn malformed_missing_duplicate_unknown_and_secret_seed_cli_options_are_usage_errors() {
    let fixture = Fixture::new();
    let base: Vec<String> = vec![
        "--artifact".into(),
        fixture.artifact.to_str().unwrap().into(),
        "--sidecar".into(),
        fixture.sidecar.to_str().unwrap().into(),
        "--public-key".into(),
        fixture.public_key.clone(),
        "--version".into(),
        VERSION.into(),
        "--channel".into(),
        "canary".into(),
    ];
    for index in (0..base.len()).step_by(2) {
        let mut missing = base.clone();
        missing.drain(index..index + 2);
        assert_exit(&run(cli().arg("verify-artifact").args(&missing)), 2);
        let mut duplicate = base.clone();
        duplicate.extend_from_slice(&base[index..index + 2]);
        assert_exit(&run(cli().arg("verify-artifact").args(&duplicate)), 2);
    }
    for (index, value) in [(5, "abcd"), (5, "gg"), (7, "v2.0.987"), (9, "nightly")] {
        let mut invalid = base.clone();
        invalid[index] = value.into();
        assert_exit(&run(cli().arg("verify-artifact").args(&invalid)), 2);
    }
    for extra in [
        vec!["--unknown", "value"],
        vec!["--key-file", "seed path"],
        vec!["positional"],
        vec!["--sidecar"],
    ] {
        assert_exit(
            &run(cli().arg("verify-artifact").args(&base).args(extra)),
            2,
        );
    }
    assert_exit(&run(cli().arg("verify-artifact")), 2);
    let help = run(cli().arg("verify-artifact").arg("--help"));
    assert_exit(&help, 0);
    assert!(String::from_utf8_lossy(&help.stdout).contains("--public-key"));
}

#[cfg(unix)]
#[test]
fn non_unicode_path_is_accepted_and_non_unicode_text_is_a_usage_error() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let mut fixture = Fixture::new();
    let path = fixture.dir.join(OsString::from_vec(vec![b's', 0xff]));
    fs::rename(&fixture.artifact, &path).unwrap();
    fixture.artifact = path;
    assert_exit(&run(&mut fixture.verify()), 0);
    let output = run(fixture.verify().arg(OsString::from_vec(vec![0xff])));
    assert_exit(&output, 2);
}

#[test]
fn library_authenticates_metadata_before_using_facts_or_opening_the_artifact() {
    let fixture = Fixture::new();
    let key = SigningKey::from_bytes(&[0x17; 32]).verifying_key();
    let mut metadata = fixture.metadata();
    metadata["version"] = "2.0.988".into();
    fixture.write_metadata(&metadata);
    fs::remove_file(&fixture.artifact).unwrap();
    assert!(matches!(
        verify_artifact_file(
            &fixture.artifact,
            &fixture.sidecar,
            &key,
            VERSION,
            Channel::Canary
        ),
        Err(ArtifactVerificationError::Signature)
    ));

    fs::write(&fixture.artifact, ARTIFACT).unwrap();
    fixture.sign("2.0.988", "canary");
    fs::remove_file(&fixture.artifact).unwrap();
    assert!(matches!(
        verify_artifact_file(
            &fixture.artifact,
            &fixture.sidecar,
            &key,
            VERSION,
            Channel::Canary
        ),
        Err(ArtifactVerificationError::VersionMismatch { .. })
    ));
    assert!(matches!(
        verify_artifact_file(
            &fixture.artifact,
            &fixture.sidecar,
            &key,
            "v2.0.987",
            Channel::Canary
        ),
        Err(ArtifactVerificationError::InvalidExpectedVersion)
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_as_non_regular_inputs() {
    use std::os::unix::fs::symlink;
    for artifact_link in [true, false] {
        let fixture = Fixture::new();
        let path = if artifact_link {
            &fixture.artifact
        } else {
            &fixture.sidecar
        };
        let target = fixture.dir.join("symlink target");
        fs::rename(path, &target).unwrap();
        symlink(&target, path).unwrap();
        assert_exit(&run(&mut fixture.verify()), 1);
    }
}

#[test]
fn artifact_at_the_exact_128_mib_limit_verifies() {
    let fixture = Fixture::new();
    File::options()
        .write(true)
        .open(&fixture.artifact)
        .unwrap()
        .set_len(MAX_ARTIFACT)
        .unwrap();
    fixture.sign(VERSION, "canary");
    assert_exit(&run(&mut fixture.verify()), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn a_file_truncated_during_the_actual_cli_read_is_rejected_as_changed_length() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let fixture = Fixture::new();
    File::options()
        .write(true)
        .open(&fixture.artifact)
        .unwrap()
        .set_len(MAX_ARTIFACT)
        .unwrap();
    fixture.sign(VERSION, "canary");
    let mut child = fixture
        .verify()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let descriptors = PathBuf::from(format!("/proc/{}/fd", child.id()));
    let deadline = Instant::now() + Duration::from_secs(10);
    // Observe the real reader's handle, then change the same file while it hashes 128 MiB.
    loop {
        let opened = fs::read_dir(&descriptors)
            .map(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    fs::read_link(entry.path()).is_ok_and(|path| path == fixture.artifact)
                })
            })
            .unwrap_or(false);
        if opened {
            break;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "the CLI exited before opening its artifact"
        );
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the real verifier did not open the test artifact within ten seconds");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    File::options()
        .write(true)
        .open(&fixture.artifact)
        .unwrap()
        .set_len(MAX_ARTIFACT - 1)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_exit(&output, 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("changed length"));
}
