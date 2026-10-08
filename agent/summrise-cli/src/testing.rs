//! A [`Host`] assembled from literals, for the ported suite.
//!
//! The TypeScript suite stubs the platform by writing a fake `curl` onto `PATH` and pointing the
//! env vars at temp directories. That works, and it is also why the seam it relies on is implicit:
//! nothing in the test names the effect it is standing in for. Here every effect is a method, the
//! fake records what it was asked to do, and a test asserts on the DECISION rather than on a file
//! that happens to exist afterwards.

use crate::host::{Host, RunResult, Tri};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
struct State {
    now_ms: i64,
    runs: Vec<Vec<String>>,
    /// Answers handed out in order for `run`, then the last one repeats.
    run_script: Vec<RunResult>,
    run_calls: usize,
    effects: Vec<String>,
    sleeps: Vec<u64>,
}

/// The fake. Every field is public because a test IS the caller here.
#[derive(Default, Clone)]
pub struct FakeHost {
    pub env: BTreeMap<String, String>,
    pub hostname: String,
    pub registry: BTreeMap<String, String>,
    pub process_running: BTreeMap<String, Tri>,
    /// URL -> body, for `http_get`.
    pub http: BTreeMap<String, String>,
    /// URL -> bytes, for `http_download`.
    pub downloads: BTreeMap<String, Vec<u8>>,
    /// When set, `reg_write` reports failure (an unelevated / refused HKLM write).
    pub reg_write_fails: bool,
    pub temp: PathBuf,
    // Interior mutability: the trait takes `&self` (the real host does its own locking), so a fake
    // that a test can WRITE through — `http_download` writes the destination file — needs cells.
    files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    mtimes: RefCell<BTreeMap<PathBuf, i64>>,
    state: RefCell<State>,
}

impl FakeHost {
    pub fn new() -> Self {
        FakeHost {
            hostname: "device".to_string(),
            temp: PathBuf::from("/tmp/summrise-cli-fake"),
            ..Default::default()
        }
    }

    pub fn with_env(mut self, k: &str, v: &str) -> Self {
        self.env.insert(k.to_string(), v.to_string());
        self
    }

    pub fn with_hostname(mut self, name: &str) -> Self {
        self.hostname = name.to_string();
        self
    }

    pub fn with_file(self, path: &str, body: &str) -> Self {
        self.with_bytes(path, body.as_bytes())
    }

    pub fn with_bytes(self, path: &str, body: &[u8]) -> Self {
        self.files
            .borrow_mut()
            .insert(PathBuf::from(path), body.to_vec());
        self
    }

    pub fn with_file_at(self, path: &str, at_ms: i64, body: &str) -> Self {
        let p = PathBuf::from(path);
        self.files
            .borrow_mut()
            .insert(p.clone(), body.as_bytes().to_vec());
        self.mtimes.borrow_mut().insert(p, at_ms);
        self
    }

    pub fn with_dir(self, path: &str) -> Self {
        self.dirs.borrow_mut().insert(PathBuf::from(path));
        self
    }

    /// The bytes the fake holds for a path, for a case that has to look at what was written.
    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(Path::new(path)).cloned()
    }

    pub fn with_reg(mut self, name: &str, value: &str) -> Self {
        self.registry.insert(name.to_string(), value.to_string());
        self
    }

    pub fn with_http(mut self, url: &str, body: &str) -> Self {
        self.http.insert(url.to_string(), body.to_string());
        self
    }

    pub fn with_download(mut self, url: &str, bytes: &[u8]) -> Self {
        self.downloads.insert(url.to_string(), bytes.to_vec());
        self
    }

    pub fn with_process(mut self, image: &str, t: Tri) -> Self {
        self.process_running.insert(image.to_string(), t);
        self
    }

    pub fn script_runs(self, results: Vec<RunResult>) -> Self {
        let mut st = self.state.borrow_mut();
        st.run_script = results;
        st.run_calls = 0;
        drop(st);
        self
    }

    pub fn set_now(&self, ms: i64) {
        self.state.borrow_mut().now_ms = ms;
    }

    /// Every argv the decisions asked for, in order.
    pub fn runs(&self) -> Vec<Vec<String>> {
        self.state.borrow().runs.clone()
    }

    /// The named effects, in order — `reg_write:DataDir=D:\Summrise`, `taskkill:electron.exe`, …
    pub fn effects(&self) -> Vec<String> {
        self.state.borrow().effects.clone()
    }

    pub fn sleeps(&self) -> Vec<u64> {
        self.state.borrow().sleeps.clone()
    }
}

impl Host for FakeHost {
    fn env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn hostname(&self) -> String {
        self.hostname.clone()
    }

    fn now_ms(&self) -> i64 {
        self.state.borrow().now_ms
    }

    fn sleep_ms(&self, ms: u64) {
        let mut st = self.state.borrow_mut();
        st.sleeps.push(ms);
        st.now_ms += ms as i64;
    }

    fn temp_dir(&self) -> PathBuf {
        self.temp.clone()
    }

    fn make_temp_dir(&self, prefix: &str) -> io::Result<PathBuf> {
        let n = self.state.borrow().effects.len();
        let p = self.temp.join(format!("{prefix}{n}"));
        self.dirs.borrow_mut().insert(p.clone());
        Ok(p)
    }

    fn read_string(&self, path: &Path) -> io::Result<String> {
        match self.files.borrow().get(path) {
            Some(b) => String::from_utf8(b.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not utf8")),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        }
    }

    fn write_bytes(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), data.to_vec());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path)
    }

    fn file_size(&self, path: &Path) -> Option<u64> {
        self.files.borrow().get(path).map(|b| b.len() as u64)
    }

    fn mtime_ms(&self, path: &Path) -> Option<i64> {
        self.mtimes.borrow().get(path).copied()
    }

    fn sha256_file(&self, path: &Path) -> Option<String> {
        self.files
            .borrow()
            .get(path)
            .map(|b| crate::sha256::sha256_bytes(b))
    }

    fn mkdirs(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let files = self.files.borrow();
        let dirs = self.dirs.borrow();
        for p in files.keys().chain(dirs.iter()) {
            if p.parent() == Some(path) {
                out.push(p.clone());
            }
        }
        Ok(out)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        let existing = self.files.borrow().get(from).cloned();
        match existing {
            Some(b) => {
                self.files.borrow_mut().insert(to.to_path_buf(), b);
                Ok(())
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }

    fn reg_read(&self, name: &str) -> Option<String> {
        self.registry.get(name).cloned()
    }

    fn reg_write(&self, name: &str, value: &str) -> bool {
        self.state
            .borrow_mut()
            .effects
            .push(format!("reg_write:{name}={value}"));
        if self.reg_write_fails {
            return false;
        }
        true
    }

    fn run(&self, argv: &[String], _timeout_ms: Option<u64>) -> RunResult {
        let mut st = self.state.borrow_mut();
        st.runs.push(argv.to_vec());
        let i = st.run_calls;
        st.run_calls += 1;
        if st.run_script.is_empty() {
            return RunResult::default();
        }
        st.run_script
            .get(i)
            .cloned()
            .unwrap_or_else(|| st.run_script.last().cloned().unwrap_or_default())
    }

    fn process_running(&self, image: &str) -> Tri {
        self.process_running
            .get(image)
            .copied()
            .unwrap_or(Tri::Unknown)
    }

    fn remove_tree(&self, path: &Path) -> RunResult {
        self.state
            .borrow_mut()
            .effects
            .push(format!("remove_tree:{}", path.display()));
        RunResult {
            status: Some(0),
            ..Default::default()
        }
    }

    fn taskkill_image(&self, image: &str) -> RunResult {
        self.state
            .borrow_mut()
            .effects
            .push(format!("taskkill:{image}"));
        RunResult {
            status: Some(0),
            ..Default::default()
        }
    }

    fn http_get(&self, url: &str, _timeout_secs: u64) -> Option<String> {
        self.state
            .borrow_mut()
            .effects
            .push(format!("http_get:{url}"));
        self.http.get(url).cloned()
    }

    fn http_download(&self, url: &str, dest: &Path, _timeout_secs: u64) -> bool {
        self.state
            .borrow_mut()
            .effects
            .push(format!("http_download:{url}"));
        match self.downloads.get(url).cloned() {
            Some(b) => {
                self.files.borrow_mut().insert(dest.to_path_buf(), b);
                true
            }
            None => false,
        }
    }
}
