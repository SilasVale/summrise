//! The app folds the stager's `swap.request.json` into the state document; the CLI never writes it.
//!
//! `--stage`/`--check-update` leave durable files (the staged pair and the request) and no state
//! write. This test drives the real binary over a data directory whose `state.json` the CLI never
//! touched: the app records `staged` at start from the request, then `applied` when the supervisor's
//! result appears, and the request staying on disk does not flip the document back to `staged`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A directory removed when the test ends, unique across parallel test binaries.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "summrise-device-swap-ingest-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the fixture directory is creatable");
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

/// A real device process that is killed and reaped when the test ends, including on a panic.
struct Running {
    child: Child,
    stderr: PathBuf,
}

impl Running {
    fn start(dir: &TempDir) -> Running {
        let stderr = dir.join("device.err");
        let log = File::create(&stderr).expect("the log is created");
        let child = Command::new(env!("CARGO_BIN_EXE_summrise-device"))
            .arg("--install")
            .arg(dir.join("install"))
            .arg("--data")
            .arg(dir.join("data"))
            .arg("--port")
            .arg("0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .expect("the device binary starts");
        Running { child, stderr }
    }

    fn stderr_text(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Wait until the state document satisfies `predicate`, or fail with the device's stderr.
    fn wait_for_state(
        &mut self,
        dir: &TempDir,
        what: &str,
        predicate: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the child's status is readable")
            {
                panic!(
                    "the device exited ({status}) before it recorded {what}\n--- stderr ---\n{}",
                    self.stderr_text()
                );
            }
            if let Ok(text) = fs::read_to_string(dir.join("data").join("state.json")) {
                if let Ok(state) = serde_json::from_str::<serde_json::Value>(&text) {
                    if predicate(&state) {
                        return state;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the device did not record {what} within 10s\n--- stderr ---\n{}",
            self.stderr_text()
        );
    }

    fn stop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("the child's status is readable")
            .is_none()
        {
            self.child.kill().expect("the app is killable");
        }
        self.child.wait().expect("the app is reaped");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(true)
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The request the CLI writes, exactly as `summrise_device::update::request_swap` writes it.
fn write_request(install: &Path, version: &str, requested_at_ms: u64) {
    fs::create_dir_all(install).expect("the install directory is creatable");
    fs::write(
        install.join("swap.request.json"),
        serde_json::json!({
            "requested_at_ms": requested_at_ms,
            "version": version,
            "sha256": "b1946ac92492d2347c6235b4d2611184",
        })
        .to_string(),
    )
    .expect("the request is written");
}

/// The supervisor's answer, exactly as `summrise-launch` writes it.
fn write_result(install: &Path, outcome: &str, version: &str, at_ms: u64) {
    fs::write(
        install.join("swap.result.json"),
        serde_json::json!({
            "outcome": outcome,
            "version": version,
            "at_ms": at_ms,
        })
        .to_string(),
    )
    .expect("the result is written");
}

#[test]
fn the_app_records_the_request_as_staged_and_then_the_result_as_applied() {
    let dir = TempDir::new("staged-applied");
    let install = dir.join("install");
    // The stager already ran while no app was serving: the request is on disk before the start.
    write_request(&install, "2.0.4", 1_000);

    let mut device = Running::start(&dir);

    let staged = device.wait_for_state(&dir, "the stage", |state| {
        state["update"]["phase"] == "staged" && state["update"]["to"] == "2.0.4"
    });
    assert_eq!(
        staged["update"]["outcome"],
        serde_json::Value::Null,
        "a pending stage has no outcome yet: {staged}"
    );

    write_result(&install, "applied", "2.0.4", 1_001);
    let applied = device.wait_for_state(&dir, "the applied result", |state| {
        state["update"]["outcome"] == "applied" && state["update"]["phase"] == "idle"
    });
    assert_eq!(applied["update"]["to"], "2.0.4");
    assert_eq!(applied["update"]["lastResultAtMs"], 1_001);

    // The request file is still on disk (the supervisor removes it; this test does not). Repeated
    // polls must not turn the finished update back into `staged`.
    std::thread::sleep(Duration::from_millis(2_500));
    let after = fs::read_to_string(dir.join("data").join("state.json"))
        .expect("the state document is readable");
    let after: serde_json::Value = serde_json::from_str(&after).expect("the document is JSON");
    assert_eq!(
        after["update"]["outcome"], "applied",
        "a request the document already names is not recorded twice: {after}"
    );
    assert_eq!(
        after["update"]["phase"], "idle",
        "the finished swap is not downgraded to staged by a lingering request: {after}"
    );

    device.stop();
}
