//! Real shell entry points with loopback HTTP and the release crate's signer/verifier.
//! Breaks caught: trusting changed bytes, falling back on non-404, claiming trust without a key,
//! signing before the last setup mutation, and silently omitting a required installer.
#![cfg(unix)]

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::SigningKey;

const VERSION: &str = "2.0.987";
const QUALIFIED: &str = "/SummriseSetup-2.0.987-canary.exe";
const LEGACY: &str = "/SummriseSetup-2.0.987.exe";
const SETUP: &[u8] = b"fixture setup bytes (not an actual Windows installer)";

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "summrise installer pipeline {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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

#[derive(Clone)]
struct Reply {
    status: u16,
    body: Vec<u8>,
    declared_length: Option<usize>,
    chunked: bool,
    location: Option<String>,
}
impl Reply {
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            declared_length: None,
            chunked: false,
            location: None,
        }
    }
    fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(Vec::new())
        }
    }
}

struct Mirror {
    url: String,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Mirror {
    fn new(files: Option<PathBuf>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(HashMap::<String, Reply>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (r, seen, stopping) = (routes.clone(), requests.clone(), stop.clone());
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                seen.lock().unwrap().push(path.clone());
                let response = r.lock().unwrap().get(&path).cloned().unwrap_or_else(|| {
                    files
                        .as_ref()
                        .and_then(|root| fs::read(root.join(path.trim_start_matches('/'))).ok())
                        .map(Reply::ok)
                        .unwrap_or_else(|| Reply::status(404))
                });
                let mut header = if response.chunked {
                    format!("HTTP/1.1 {} Fixture\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n", response.status)
                } else {
                    format!(
                        "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
                        response.status,
                        response.declared_length.unwrap_or(response.body.len())
                    )
                };
                if let Some(location) = response.location {
                    header.push_str(&format!("Location: {location}\r\n"));
                }
                header.push_str("\r\n");
                let _ = stream.write_all(header.as_bytes());
                if response.chunked {
                    let _ = write!(stream, "{:x}\r\n", response.body.len());
                    let _ = stream.write_all(&response.body);
                    let _ = stream.write_all(b"\r\n0\r\n\r\n");
                } else {
                    let _ = stream.write_all(&response.body);
                }
            }
        });
        Self {
            url,
            routes,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn set(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_owned(), reply);
    }
    fn saw(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == path)
            .count()
    }
}
impl Drop for Mirror {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.thread.take().unwrap().join().unwrap();
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn tool() -> &'static str {
    env!("CARGO_BIN_EXE_summrise-release")
}
fn clean(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("NO_PROXY", "*")
}
fn text(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
fn expect_code(out: &Output, code: i32) {
    assert_eq!(out.status.code(), Some(code), "{}", text(out));
}
fn run_tool(args: &[&str]) -> Output {
    clean(&mut Command::new(tool()))
        .args(args)
        .output()
        .unwrap()
}
fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn key(dir: &TempDir) -> (PathBuf, String) {
    let path = dir.join("ephemeral launcher seed");
    fs::write(&path, "4d".repeat(32)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let public = SigningKey::from_bytes(&[0x4d; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (path, public)
}
fn sidecar(dir: &TempDir, seed: &Path, bytes: &[u8], version: &str, channel: &str) -> Vec<u8> {
    let artifact = dir.join("artifact to sign.exe");
    let out = dir.join("signed artifact.sidecar.json");
    fs::write(&artifact, bytes).unwrap();
    expect_code(
        &run_tool(&[
            "sidecar",
            "--key-file",
            seed.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--version",
            version,
            "--channel",
            channel,
            "--out",
            out.to_str().unwrap(),
        ]),
        0,
    );
    fs::read(out).unwrap()
}
fn sidecar_path(installer: &str) -> String {
    format!("{}.sidecar.json", installer.strip_suffix(".exe").unwrap())
}

struct Fixture {
    dir: TempDir,
    mirror: Mirror,
    seed: PathBuf,
    public: String,
}
impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new();
        let mirror = Mirror::new(None);
        let (seed, public) = key(&dir);
        let artifact = dir.join("device.exe");
        let document = dir.join("release.json");
        fs::write(&artifact, b"normal device artifact").unwrap();
        expect_code(
            &run_tool(&[
                "sign",
                "--key-file",
                seed.to_str().unwrap(),
                "--artifact",
                artifact.to_str().unwrap(),
                "--version",
                VERSION,
                "--channel",
                "canary",
                "--url",
                &format!("{}/device.exe", mirror.url),
                "--min-version",
                "0.1.0",
                "--out",
                document.to_str().unwrap(),
            ]),
            0,
        );
        mirror.set(
            "/release-canary.json",
            Reply::ok(fs::read(document).unwrap()),
        );
        mirror.set("/device.exe", Reply::ok(b"normal device artifact".to_vec()));
        mirror.set(
            "/summrise-device-2.0.987-canary.sidecar.json",
            Reply::ok(sidecar(
                &dir,
                &seed,
                b"normal device artifact",
                VERSION,
                "canary",
            )),
        );
        Self {
            dir,
            mirror,
            seed,
            public,
        }
    }
    fn setup(&self, path: &str, bytes: &[u8]) {
        self.mirror.set(path, Reply::ok(bytes.to_vec()));
        self.mirror.set(
            &sidecar_path(path),
            Reply::ok(sidecar(&self.dir, &self.seed, bytes, VERSION, "canary")),
        );
    }
    fn wrapper(&self) -> Command {
        let mut cmd = Command::new("/bin/bash");
        clean(&mut cmd)
            .env("TMPDIR", &self.dir.0)
            .arg(repo().join("scripts/verify-release.sh"))
            .arg(VERSION)
            .arg("--mirror")
            .arg(&self.mirror.url);
        cmd
    }
    fn trusted(&self) -> Command {
        let mut cmd = self.wrapper();
        cmd.args(["--installer-pubkey", &self.public, "--release-tool", tool()]);
        cmd
    }
}

// Replace only curl's version output; every HTTP request still uses the actual host curl.
fn with_curl_version(fixture: &Fixture, cmd: &mut Command, version: &str) {
    let bin = fixture.dir.join("curl version tools");
    fs::create_dir_all(&bin).unwrap();
    write_executable(
        &bin.join("curl"),
        r#"#!/bin/bash
if [[ "${1:-}" == -q && "${2:-}" == --version ]]; then
    printf '%s\n' "$CURL_FIXTURE_VERSION"
    exit 0
fi
exec /usr/bin/curl "$@"
"#,
    );
    cmd.env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CURL_FIXTURE_VERSION", version);
}

#[test]
fn unsupported_or_unparseable_curl_never_fetches_installer_even_without_key() {
    for version in [
        "curl 7.88.1 (fixture)",
        "curl 8.3.0 (fixture)",
        "unknown curl build",
        "curl 8.4.bad (fixture)",
    ] {
        for trusted in [false, true] {
            let fixture = Fixture::new();
            fixture.setup(QUALIFIED, SETUP);
            fixture.setup(LEGACY, SETUP);
            let mut command = if trusted {
                fixture.trusted()
            } else {
                fixture.wrapper()
            };
            with_curl_version(&fixture, &mut command, version);
            let out = command.output().unwrap();
            expect_code(&out, 1);
            assert!(text(&out).contains("curl 8.4.0"), "{}", text(&out));
            assert_eq!(fixture.mirror.saw(QUALIFIED), 0);
            assert_eq!(fixture.mirror.saw(LEGACY), 0);
            assert_eq!(fixture.mirror.saw(&sidecar_path(QUALIFIED)), 0);
        }
    }
}

#[test]
fn curl_minimum_and_newer_versions_use_real_download_verification() {
    for version in ["curl 8.4.0 (fixture)", "curl 9.0.0 (fixture)"] {
        let fixture = Fixture::new();
        fixture.setup(QUALIFIED, SETUP);
        let mut command = fixture.trusted();
        with_curl_version(&fixture, &mut command, version);
        let out = command.output().unwrap();
        expect_code(&out, 0);
        assert!(text(&out).contains("trusted publisher-signed"));
        assert_eq!(fixture.mirror.saw(QUALIFIED), 1);
        assert_eq!(fixture.mirror.saw(&sidecar_path(QUALIFIED)), 1);
    }
}

#[test]
fn chunked_oversize_sidecar_is_rejected_by_actual_curl() {
    let fixture = Fixture::new();
    fixture.setup(QUALIFIED, SETUP);
    fixture.setup(LEGACY, SETUP);
    fixture.mirror.set(
        &sidecar_path(QUALIFIED),
        Reply {
            chunked: true,
            ..Reply::ok(vec![b' '; 65537])
        },
    );
    let out = fixture.trusted().output().unwrap();
    expect_code(&out, 1);
    assert!(
        text(&out).contains("cannot download SummriseSetup-2.0.987-canary.sidecar.json"),
        "{}",
        text(&out)
    );
    assert_eq!(fixture.mirror.saw(LEGACY), 0);
}

#[test]
fn required_mode_needs_a_caller_key_before_fetching() {
    let fixture = Fixture::new();
    let out = fixture
        .wrapper()
        .args(["--require-installer", "--release-tool", tool()])
        .output()
        .unwrap();
    expect_code(&out, 2);
    assert!(text(&out).contains("--installer-pubkey"), "{}", text(&out));
    assert_eq!(fixture.mirror.saw("/release-canary.json"), 0);
}

#[test]
fn required_mode_needs_a_working_tool_before_fetching() {
    let fixture = Fixture::new();
    let out = fixture
        .wrapper()
        .args([
            "--require-installer",
            "--installer-pubkey",
            &fixture.public,
            "--release-tool",
            "/missing release tool",
        ])
        .output()
        .unwrap();
    expect_code(&out, 2);
    assert!(text(&out).contains("--release-tool"), "{}", text(&out));
    assert_eq!(fixture.mirror.saw("/release-canary.json"), 0);
}

#[test]
fn qualified_setup_uses_the_real_verifier_with_environment_key_and_space_paths() {
    let fixture = Fixture::new();
    fixture.setup(QUALIFIED, SETUP);
    let copied_tool = fixture.dir.join("release tool with spaces");
    fs::copy(tool(), &copied_tool).unwrap();
    let out = fixture
        .wrapper()
        .env("SUMMRISE_LAUNCHER_PUBKEY", &fixture.public)
        .arg("--release-tool")
        .arg(copied_tool)
        .arg("--require-installer")
        .output()
        .unwrap();
    expect_code(&out, 0);
    assert!(
        text(&out).contains("trusted publisher-signed"),
        "{}",
        text(&out)
    );
    assert_eq!(fixture.mirror.saw(QUALIFIED), 1, "no full-size probes");
    assert_eq!(fixture.mirror.saw(LEGACY), 0);
}

#[test]
fn only_qualified_404_selects_the_matching_legacy_pair() {
    let fixture = Fixture::new();
    fixture.setup(LEGACY, SETUP);
    let out = fixture
        .trusted()
        .arg("--require-installer")
        .output()
        .unwrap();
    expect_code(&out, 0);
    assert!(text(&out).contains("trusted publisher-signed"));
    assert_eq!(fixture.mirror.saw(QUALIFIED), 1);
    assert_eq!(fixture.mirror.saw(LEGACY), 1);
    assert_eq!(fixture.mirror.saw(&sidecar_path(LEGACY)), 1);
    assert_eq!(fixture.mirror.saw(&sidecar_path(QUALIFIED)), 0);
}

#[test]
fn qualified_http_errors_and_redirect_never_select_legacy() {
    for status in [403, 500, 302] {
        let fixture = Fixture::new();
        fixture.setup(LEGACY, SETUP);
        let mut reply = Reply::status(status);
        reply.location = Some(format!("{}{}", fixture.mirror.url, LEGACY));
        fixture.mirror.set(QUALIFIED, reply);
        let out = fixture.trusted().output().unwrap();
        expect_code(&out, 1);
        assert_eq!(
            fixture.mirror.saw(LEGACY),
            0,
            "HTTP {status}: {}",
            text(&out)
        );
    }
}

#[test]
fn qualified_corruption_and_untrusted_metadata_never_select_legacy() {
    for problem in [
        "bytes",
        "signature",
        "version",
        "channel",
        "json",
        "missing-sidecar",
        "device-sidecar",
    ] {
        let fixture = Fixture::new();
        fixture.setup(QUALIFIED, SETUP);
        fixture.setup(LEGACY, SETUP);
        match problem {
            "bytes" => fixture
                .mirror
                .set(QUALIFIED, Reply::ok(b"tampered setup".to_vec())),
            "signature" => {
                let mut value: serde_json::Value = serde_json::from_slice(&sidecar(
                    &fixture.dir,
                    &fixture.seed,
                    SETUP,
                    VERSION,
                    "canary",
                ))
                .unwrap();
                value["signature"] = serde_json::Value::String("00".repeat(64));
                fixture.mirror.set(
                    &sidecar_path(QUALIFIED),
                    Reply::ok(serde_json::to_vec(&value).unwrap()),
                );
            }
            "version" => fixture.mirror.set(
                &sidecar_path(QUALIFIED),
                Reply::ok(sidecar(
                    &fixture.dir,
                    &fixture.seed,
                    SETUP,
                    "2.0.986",
                    "canary",
                )),
            ),
            "channel" => fixture.mirror.set(
                &sidecar_path(QUALIFIED),
                Reply::ok(sidecar(
                    &fixture.dir,
                    &fixture.seed,
                    SETUP,
                    VERSION,
                    "stable",
                )),
            ),
            "json" => fixture
                .mirror
                .set(&sidecar_path(QUALIFIED), Reply::ok(b"{broken".to_vec())),
            "missing-sidecar" => fixture
                .mirror
                .set(&sidecar_path(QUALIFIED), Reply::status(404)),
            "device-sidecar" => fixture.mirror.set(
                &sidecar_path(QUALIFIED),
                Reply::ok(sidecar(
                    &fixture.dir,
                    &fixture.seed,
                    b"normal device artifact",
                    VERSION,
                    "canary",
                )),
            ),
            _ => unreachable!(),
        }
        let out = fixture.trusted().output().unwrap();
        expect_code(&out, 1);
        assert_eq!(fixture.mirror.saw(LEGACY), 0, "{problem}: {}", text(&out));
        assert!(!text(&out).contains("trusted publisher-signed"));
    }
}

#[test]
fn incomplete_or_oversized_downloads_fail_without_fallback() {
    for (is_sidecar, declared, body) in [
        (false, 128 * 1024 * 1024 + 1, Vec::new()),
        (false, SETUP.len() + 30, SETUP.to_vec()),
        (true, 64 * 1024 + 1, vec![b' '; 64 * 1024 + 1]),
        (true, 2000, b"truncated sidecar".to_vec()),
    ] {
        let fixture = Fixture::new();
        fixture.setup(QUALIFIED, SETUP);
        fixture.setup(LEGACY, SETUP);
        let path = if is_sidecar {
            sidecar_path(QUALIFIED)
        } else {
            QUALIFIED.to_owned()
        };
        fixture.mirror.set(
            &path,
            Reply {
                declared_length: Some(declared),
                ..Reply::ok(body)
            },
        );
        let out = fixture.trusted().output().unwrap();
        expect_code(&out, 1);
        assert_eq!(fixture.mirror.saw(LEGACY), 0);
    }
}

#[test]
fn optional_absence_and_no_key_are_honest_partial_checks() {
    let fixture = Fixture::new();
    let out = fixture.trusted().output().unwrap();
    expect_code(&out, 0);
    assert!(text(&out).contains("not served"));
    let required = fixture
        .trusted()
        .arg("--require-installer")
        .output()
        .unwrap();
    expect_code(&required, 1);
    fixture.setup(QUALIFIED, SETUP);
    let no_key = fixture.wrapper().output().unwrap();
    expect_code(&no_key, 0);
    assert!(
        text(&no_key).contains("installer trust was not checked"),
        "{}",
        text(&no_key)
    );
    assert!(!text(&no_key).contains("trusted publisher-signed"));
    assert_eq!(fixture.mirror.saw(&sidecar_path(QUALIFIED)), 0);
}

struct PublishFixture {
    dir: TempDir,
    mirror: Mirror,
    public: String,
    seed: PathBuf,
}
impl PublishFixture {
    fn new(nsis: bool, authenticode: bool) -> Self {
        let dir = TempDir::new();
        for path in [
            "scripts",
            "device/installer",
            "tools",
            "out",
            "mirror",
            "home",
        ] {
            fs::create_dir_all(dir.join(path)).unwrap();
        }
        for name in ["publish-canary.bash", "verify-release.sh"] {
            fs::copy(
                repo().join("scripts").join(name),
                dir.join("scripts").join(name),
            )
            .unwrap();
        }
        let (seed, public) = key(&dir);
        // Build tools are disposable fixtures; release signing and verification stay real.
        write_executable(
            &dir.join("tools/cargo"),
            r#"#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  build) mkdir -p target/release; cp "$REAL_RELEASE_TOOL" target/release/summrise-release ;;
  xwin) mkdir -p target/fixture/release
    for name in launch device console shell helper; do printf 'fixture %s app' "$name" > "target/fixture/release/summrise-$name.exe"; done ;;
  *) exit 92 ;;
esac
"#,
        );
        if nsis {
            write_executable(
                &dir.join("tools/makensis"),
                r#"#!/usr/bin/env bash
set -euo pipefail
for arg; do
  case "$arg" in -DVERSION=*) version="${arg#*=}" ;; -DCHANNEL=*) channel="${arg#*=}" ;; esac
done
printf 'fixture unsigned setup' > "SummriseSetup-$version-$channel.exe"
"#,
            );
        }
        if authenticode {
            fs::write(
                dir.join("fixture certificate.pfx"),
                b"not a real certificate",
            )
            .unwrap();
            write_executable(&dir.join("tools/signtool"), "#!/usr/bin/env bash\nset -euo pipefail\nprintf '+authenticode-fixture' >> \"${@: -1}\"\n");
        }
        // A host may have fallback NSIS outside PATH. Hide only that external discovery for the
        // no-NSIS fixture, so the actual publisher's absence branch runs on any test host.
        fs::write(
            dir.join("mask external nsis.bash"),
            r#"function [() {
  if builtin [ "$#" -eq 3 ] && builtin [ "$1" = -x ]; then
    case "$2" in /tmp/nsis/*|/c/*|*/tools/nsis/*) return 1 ;; esac
  fi
  builtin [ "$@"
}
"#,
        )
        .unwrap();
        let mirror = Mirror::new(Some(dir.join("mirror")));
        mirror.set(
            "/health",
            Reply::ok(br#"{"version":"2.0.987","devices":0}"#.to_vec()),
        );
        Self {
            dir,
            mirror,
            public,
            seed,
        }
    }
    fn run(&self, authenticode: bool) -> Output {
        let mut cmd = Command::new("/bin/bash");
        clean(&mut cmd)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.dir.join("tools").display()),
            )
            .env("HOME", self.dir.join("home"))
            .env("BASH_ENV", self.dir.join("mask external nsis.bash"))
            .env("REAL_RELEASE_TOOL", tool())
            .env("SUMMRISE_LAUNCHER_PUBKEY", &self.public)
            .env("SUMMRISE_RELEASE_PUBKEY", &self.public)
            .env("SUMMRISE_CONSOLE_GRANT_KEY", "ephemeral-test-only-grant")
            .arg(self.dir.join("scripts/publish-canary.bash"))
            .args([
                "--version",
                VERSION,
                "--channel",
                "canary",
                "--target",
                "fixture",
            ])
            .arg("--mirror")
            .arg(&self.mirror.url)
            .arg("--console")
            .arg(&self.mirror.url)
            .arg("--launcher-key")
            .arg(&self.seed)
            .arg("--release-key")
            .arg(&self.seed)
            .arg("--out")
            .arg(self.dir.join("out"))
            .arg("--mirror-dir")
            .arg(self.dir.join("mirror"));
        if authenticode {
            cmd.env(
                "SUMMRISE_SIGN_PFX",
                self.dir.join("fixture certificate.pfx"),
            );
        }
        cmd.output().unwrap()
    }
}

#[test]
fn publisher_attests_final_setup_bytes_and_mirrors_the_pair() {
    let fixture = PublishFixture::new(true, true);
    let out = fixture.run(true);
    expect_code(&out, 0);
    let name = QUALIFIED.trim_start_matches('/');
    let sidecar_name = sidecar_path(QUALIFIED).trim_start_matches('/').to_owned();
    let artifact = fixture.dir.join("out").join(name);
    assert_eq!(
        fs::read(&artifact).unwrap(),
        b"fixture unsigned setup+authenticode-fixture"
    );
    assert_eq!(
        fs::read(fixture.dir.join("mirror").join(name)).unwrap(),
        fs::read(&artifact).unwrap()
    );
    let attestation = fixture.dir.join("out").join(&sidecar_name);
    assert_eq!(
        fs::read(fixture.dir.join("mirror").join(sidecar_name)).unwrap(),
        fs::read(&attestation).unwrap()
    );
    expect_code(
        &run_tool(&[
            "verify-artifact",
            "--artifact",
            artifact.to_str().unwrap(),
            "--sidecar",
            attestation.to_str().unwrap(),
            "--public-key",
            &fixture.public,
            "--version",
            VERSION,
            "--channel",
            "canary",
        ]),
        0,
    );
    assert!(text(&out).contains("trusted publisher-signed"));
}

#[test]
fn publisher_without_certificate_attests_bytes_but_reports_unsigned() {
    let fixture = PublishFixture::new(true, false);
    let out = fixture.run(false);
    expect_code(&out, 0);
    assert!(text(&out).contains("Authenticode: unsigned"));
    assert!(fixture
        .dir
        .join("out")
        .join(sidecar_path(QUALIFIED).trim_start_matches('/'))
        .is_file());
    assert_eq!(
        fs::read(
            fixture
                .dir
                .join("out")
                .join(QUALIFIED.trim_start_matches('/'))
        )
        .unwrap(),
        b"fixture unsigned setup"
    );
}

#[test]
fn publisher_without_nsis_emits_neither_setup_nor_attestation() {
    let fixture = PublishFixture::new(false, false);
    let out = fixture.run(false);
    expect_code(&out, 0);
    assert!(text(&out).contains("makensis was not found"));
    assert!(!fixture
        .dir
        .join("out")
        .join(QUALIFIED.trim_start_matches('/'))
        .exists());
    assert!(!fixture
        .dir
        .join("out")
        .join(sidecar_path(QUALIFIED).trim_start_matches('/'))
        .exists());
    assert!(!text(&out).contains("trusted publisher-signed"));
}

#[test]
fn publisher_requires_served_installer_only_when_it_built_one() {
    let fixture = PublishFixture::new(true, false);
    fixture.mirror.set(QUALIFIED, Reply::status(404));
    fixture.mirror.set(LEGACY, Reply::status(404));
    let out = fixture.run(false);
    expect_code(&out, 1);
    assert!(text(&out).contains("not served"));
}
