//! One serving app per data directory.
//!
//! Two `run`s on one `<data>` are two writers of `state.json` with no cross-process lock between
//! them. The app therefore takes `<data>/device.lock` before it writes or serves anything: the
//! second process must exit non-zero (naming the data directory and the holder's pid) rather than
//! race the first, and the lock must be released when the first exits so a later start succeeds.
//!
//! The test drives the real binary: an in-process lock guard would prove the syscall, not the
//! startup path the operator actually runs.

#![cfg(unix)]

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
            "summrise-device-lock-{}-{}-{label}",
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

    fn path(&self) -> &Path {
        &self.0
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
    label: &'static str,
}

impl Running {
    fn start(dir: &TempDir, label: &'static str) -> Running {
        let stderr = File::create(dir.join(&format!("{label}.err"))).expect("the log is created");
        let child = Command::new(env!("CARGO_BIN_EXE_summrise-device"))
            .arg("--install")
            .arg(dir.join("install"))
            .arg("--data")
            .arg(dir.join("data"))
            .arg("--port")
            .arg("0")
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("the device binary starts");
        Running { child, label }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn stderr(&self, dir: &TempDir) -> String {
        fs::read_to_string(dir.join(&format!("{}.err", self.label))).unwrap_or_default()
    }

    /// Wait until the child has recorded its own pid in `state.json`, or fail with its stderr.
    fn wait_until_serving(&mut self, dir: &TempDir, expected_pid: Option<u32>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the child's status is readable")
            {
                panic!(
                    "the device exited ({status}) before it served\n--- stderr ---\n{}",
                    self.stderr(dir)
                );
            }
            if let Ok(text) = fs::read_to_string(dir.join("data").join("state.json")) {
                if let Ok(state) = serde_json::from_str::<serde_json::Value>(&text) {
                    let pid = state["service"]["pid"].as_u64();
                    if expected_pid.is_none() || pid == expected_pid.map(u64::from) {
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the device did not record its start in state.json within 10s\n--- stderr ---\n{}",
            self.stderr(dir)
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

#[test]
fn a_second_run_on_the_same_data_directory_exits_non_zero_and_the_lock_is_released() {
    let dir = TempDir::new("two-runs");

    let mut first = Running::start(&dir, "first");
    let first_pid = first.id();
    first.wait_until_serving(&dir, None);

    let mut second = Running::start(&dir, "second");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = second
            .child
            .try_wait()
            .expect("the second's status is readable")
        {
            break status;
        }
        if started.elapsed() > Duration::from_secs(1) {
            panic!(
                "a second `run` did not refuse the data directory within one second\n--- stderr ---\n{}",
                second.stderr(&dir)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        !status.success(),
        "a second app on one data directory must exit non-zero, got {status}"
    );
    let message = second.stderr(&dir);
    assert!(
        message.contains(&dir.join("data").display().to_string()),
        "the refusal names the data directory: {message}"
    );
    assert!(
        message.contains(&first_pid.to_string()),
        "the refusal names the process that holds the lock: {message}"
    );

    first.stop();

    let mut third = Running::start(&dir, "third");
    let third_pid = third.id();
    third.wait_until_serving(&dir, Some(third_pid));
    assert!(
        third
            .child
            .try_wait()
            .expect("the third's status")
            .is_none(),
        "the lock is released when the first app exits; a later start serves\n--- stderr ---\n{}",
        third.stderr(&dir)
    );
    third.stop();
}
