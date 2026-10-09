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
    /// What `spawn_detached` answers: `None` = still running when the grace window closed,
    /// `Some(exit)` = the child exited with that code, and `spawn_fails` = the spawn itself failed.
    spawn_answer: Option<Option<i32>>,
    spawn_fails: bool,
}

/// ONE SPELLING FOR EVERY PATH THE FAKE HOLDS OR IS ASKED FOR.
///
/// The fixtures are Windows paths (`D:\Summrise\etc\config.yaml`) and the suite runs on Linux,
/// where `Path::join` produces `/`. Without this, the SAME file was two keys: a fixture registered
/// under `D:\Summrise\etc` and looked up as `D:\Summrise/etc` — so a decision that reads a path
/// built by `Path::join` saw a file that was not there, and a ported case failed for a reason that
/// has nothing to do with the decision. Normalising on BOTH sides is what makes the fake answer the
/// way a real filesystem does.
fn key(p: &Path) -> PathBuf {
    PathBuf::from(p.display().to_string().replace('/', "\\"))
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
    /// Paths that RESIST a write — a LOCKED file, an AV hold or a denied HKLM ACL.
    ///
    /// Scoped to a path rather than a global flag because the two branches are driven in the SAME
    /// run: `uninstall --purge-data` removes the program dir SUCCESSFULLY and then cannot remove the
    /// data dir, and `setup` replaces the exe while leaving everything else alone. A global
    /// "removals fail" would make the survivor check fire on the first path and never reach the
    /// second branch at all — which is the branch the case is about.
    pub locked: BTreeSet<PathBuf>,
    pub temp: PathBuf,
    /// What [`Host::package_dir`] answers: the npm package this CLI was launched out of. A real
    /// string rather than a temp dir, because every path derived from it is asserted verbatim.
    pub package_dir: String,
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
            package_dir: "C:\\npm\\node_modules\\summrise-agent".to_string(),
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
            .insert(key(Path::new(path)), body.to_vec());
        self
    }

    pub fn with_file_at(self, path: &str, at_ms: i64, body: &str) -> Self {
        let p = key(Path::new(path));
        self.files
            .borrow_mut()
            .insert(p.clone(), body.as_bytes().to_vec());
        self.mtimes.borrow_mut().insert(p, at_ms);
        self
    }

    pub fn with_dir(self, path: &str) -> Self {
        self.dirs.borrow_mut().insert(key(Path::new(path)));
        self
    }

    /// A path that resists a write: a copy ONTO it fails, and a `Remove-Item` naming it (or an
    /// ancestor of it) changes nothing. This is the locked file every retry loop and every survivor
    /// check in the CLI exists for.
    pub fn with_locked(mut self, path: &str) -> Self {
        let k = key(Path::new(path));
        self.locked.insert(k);
        self
    }

    /// The bytes the fake holds for a path, for a case that has to look at what was written.
    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(&key(Path::new(path))).cloned()
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

    /// What [`Host::spawn_detached`] answers. The default is "still alive", which is the ordinary
    /// case; a case opts into a failure to reach the two branches that exist for one.
    pub fn spawn_answer(self, answer: io::Result<Option<i32>>) -> Self {
        let mut st = self.state.borrow_mut();
        match answer {
            Ok(v) => st.spawn_answer = Some(v),
            Err(_) => st.spawn_fails = true,
        }
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

impl FakeHost {
    /// Would a write to — or a removal of — `target` have to touch something held open?
    ///
    /// **BOTH DIRECTIONS, BECAUSE THE TWO CALLERS ASK DIFFERENT QUESTIONS OF ONE FACT.**
    ///
    /// * a `copy_file` ONTO a locked path is `EBUSY` (`target` IS, or is under, the lock);
    /// * a `Remove-Item -Recurse` OF a tree that CONTAINS a locked file cannot complete either
    ///   (`target` CONTAINS the lock) — which is the real shape of a locked data dir: the file the
    ///   agent still holds open lives inside the tree being deleted, not at its root.
    ///
    /// A predicate that only looked one way made the data dir's survivor branch unreachable: the
    /// removal named `…\Summrise` while the lock was on `…\Summrise\sessions\s1` inside it, so the
    /// fake removed the whole tree and the verb truthfully reported the purge had worked.
    fn is_locked(&self, target: &Path) -> bool {
        let t = key(target).display().to_string();
        self.locked.iter().any(|l| {
            let ls = l.display().to_string();
            t == ls || t.starts_with(&format!("{ls}\\")) || ls.starts_with(&format!("{t}\\"))
        })
    }

    /// Whatever single-quoted paths a `Remove-Item` in this argv names are gone afterwards —
    /// unless the path is [`FakeHost::locked`], which is how a case drives the LOCKED file the
    /// survivor branches exist for.
    fn apply_removals(&self, argv: &[String]) {
        for a in argv {
            if !a.contains("Remove-Item") {
                continue;
            }
            for path in quoted_paths(a) {
                let k = key(Path::new(&path));
                if self.is_locked(&k) {
                    continue;
                }
                self.files.borrow_mut().retain(|p, _| {
                    !p.display()
                        .to_string()
                        .starts_with(&format!("{k}\\", k = k.display()))
                });
                self.files.borrow_mut().remove(&k);
                self.dirs.borrow_mut().retain(|p| {
                    !p.display()
                        .to_string()
                        .starts_with(&format!("{k}\\", k = k.display()))
                });
                self.dirs.borrow_mut().remove(&k);
            }
        }
    }
}

/// Every `'…'` literal in a PowerShell one-liner. Enough for the removal scripts this suite drives,
/// and deliberately not a parser: a fake that parsed PowerShell would be a second implementation of
/// the thing under test.
fn quoted_paths(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = script;
    while let Some(i) = rest.find('\'') {
        let after = &rest[i + 1..];
        match after.find('\'') {
            Some(j) => {
                out.push(after[..j].to_string());
                rest = &after[j + 1..];
            }
            None => break,
        }
    }
    out
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
        match self.files.borrow().get(&key(path)) {
            Some(b) => String::from_utf8(b.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not utf8")),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        }
    }

    fn write_bytes(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.files.borrow_mut().insert(key(path), data.to_vec());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        // A REAL FILESYSTEM'S ANSWER, NOT A KEY LOOKUP: a directory exists when a file is under it.
        // The strict form made `exists("<install>")` false on a fixture that had staged the exe
        // inside it, so every "did it survive" check and every "are the sources there" check read
        // differently here than on a device.
        let k = key(path);
        if self.files.borrow().contains_key(&k) || self.dirs.borrow().contains(&k) {
            return true;
        }
        // BOTH separators: the fixtures are Windows paths and the suite runs on Linux, so the
        // separator here is the DATA's rather than the host's.
        let base = format!("{k}\\", k = k.display());
        let under = |p: &PathBuf| p.display().to_string().starts_with(&base);
        self.files.borrow().keys().any(under) || self.dirs.borrow().iter().any(under)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(&key(path))
    }

    fn file_size(&self, path: &Path) -> Option<u64> {
        self.files.borrow().get(&key(path)).map(|b| b.len() as u64)
    }

    fn mtime_ms(&self, path: &Path) -> Option<i64> {
        self.mtimes.borrow().get(&key(path)).copied()
    }

    fn sha256_file(&self, path: &Path) -> Option<String> {
        self.files
            .borrow()
            .get(&key(path))
            .map(|b| crate::sha256::sha256_bytes(b))
    }

    fn mkdirs(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let files = self.files.borrow();
        let dirs = self.dirs.borrow();
        let k = key(path);
        for p in files.keys().chain(dirs.iter()) {
            if p.parent() == Some(k.as_path()) {
                out.push(p.clone());
            }
        }
        Ok(out)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        // A LOCKED DESTINATION IS `EBUSY`. This is the condition `copy_with_retry`'s twelve attempts
        // and both "the file stayed locked" sentences exist for, and without it the fake answered
        // "copied" for every destination it had never been given — so the retry loop could not be
        // entered at all.
        if self.is_locked(to) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the file is locked",
            ));
        }
        let existing = self.files.borrow().get(&key(from)).cloned();
        match existing {
            Some(b) => {
                self.files.borrow_mut().insert(key(to), b);
                Ok(())
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files.borrow_mut().remove(&key(path));
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
        let answer = {
            let mut st = self.state.borrow_mut();
            st.runs.push(argv.to_vec());
            let i = st.run_calls;
            st.run_calls += 1;
            if st.run_script.is_empty() {
                None
            } else {
                Some(
                    st.run_script
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| st.run_script.last().cloned().unwrap_or_default()),
                )
            }
        };
        // THE FAKE HONOURS THE REMOVALS IT IS ASKED TO PERFORM. `uninstall`, `setup` and the swap all
        // delete through `Remove-Item … '<path>'`, and a fake that only RECORDED the request made
        // every "did it survive" check answer "yes" — so the survivor branch, which is the branch
        // those commands exist to get right, could not be reached at all.
        self.apply_removals(argv);
        answer.unwrap_or_default()
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
                // `key()`, like every other path this fake holds. Storing the destination RAW made
                // the SAME file two keys: the fetch wrote `…/summrise-comp-0/cloudflared.exe` (a
                // Linux-joined path) while `exists`/`file_size`/`sha256_file` looked it up
                // normalised — so a component whose digest MATCHED was reported as never staged
                // and the whole verified-fetch path was unreachable in the suite.
                self.files.borrow_mut().insert(key(dest), b);
                true
            }
            None => false,
        }
    }

    fn spawn_detached(&self, argv: &[String], _grace_ms: u64) -> io::Result<Option<i32>> {
        let mut st = self.state.borrow_mut();
        st.runs.push(argv.to_vec());
        st.effects
            .push(format!("spawn_detached:{}", argv.join(" ")));
        if st.spawn_fails {
            return Err(io::Error::new(io::ErrorKind::NotFound, "spawn failed"));
        }
        // The default is "still running", which is the ordinary case and the one a case has to opt
        // OUT of to reach the two failure branches.
        Ok(st.spawn_answer.flatten())
    }

    fn package_dir(&self) -> String {
        self.package_dir.clone()
    }
}
