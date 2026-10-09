//! EVERY PLATFORM EFFECT THIS CLI HAS, BEHIND ONE TRAIT.
//!
//! The TypeScript reached its effects through module-level `spawnSync`/`fs` calls, and the suite
//! stubbed them by writing a fake `curl` onto `PATH` and pointing `HOME`-shaped env vars at temp
//! dirs — clever, and the reason the seam it needs is implicit. Here the seam is explicit: the
//! decisions take `&dyn Host`, and every test drives them on Linux with [`crate::testing::FakeHost`].
//!
//! **THERE IS NO "RUN THIS COMMAND LINE" METHOD, AND THAT IS THE DESIGN.** `ps()` in the TypeScript
//! existed because a PowerShell script had to be passed as ONE argv element, and the `psArgv`
//! incident is what happens when a shell re-parses it. A trait that offers only `run(argv)` — plus
//! two NAMED platform operations for the Windows commands that are `cmd.exe` builtins and take no
//! argv (`rmdir`, `taskkill`) — makes the defect unrepresentable rather than merely tested for.
//! The oracle's two source-scanning cases ("every ps() script is passed as argv", "every
//! interpolated value sits inside a double-quoted region of the cmd line") are REPLACED by this
//! construction; the port's report names them.

use std::io;
use std::path::{Path, PathBuf};

/// What a child process answered. `status: None` means the process could not be run at all (missing
/// program, refused spawn) — which is a different fact from any exit code, and the callers that
/// report "UNKNOWN" rather than a verdict depend on it staying that way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunResult {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RunResult {
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }
}

/// Every effect. See the module doc for why there is no command-line method.
pub trait Host {
    // ── environment ────────────────────────────────────────────────────────────
    fn env(&self, name: &str) -> Option<String>;
    fn hostname(&self) -> String;
    fn now_ms(&self) -> i64;
    fn sleep_ms(&self, ms: u64);
    fn temp_dir(&self) -> PathBuf;
    /// `mkdtempSync(path.join(os.tmpdir(), prefix))`: a NEW empty directory, never a reused one.
    fn make_temp_dir(&self, prefix: &str) -> io::Result<PathBuf>;

    // ── files ──────────────────────────────────────────────────────────────────
    fn read_string(&self, path: &Path) -> io::Result<String>;
    fn write_bytes(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn file_size(&self, path: &Path) -> Option<u64>;
    /// Modification time in milliseconds since the epoch, or `None` when it could not be read.
    fn mtime_ms(&self, path: &Path) -> Option<i64>;
    /// sha256 of a file's bytes, lowercase hex. Read whole — the electron runtime is 115 MB and the
    /// TypeScript does the same (`sha256File`), so the two sides hash the same bytes the same way.
    fn sha256_file(&self, path: &Path) -> Option<String>;
    fn mkdirs(&self, path: &Path) -> io::Result<()>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    // ── registry (HKLM\SOFTWARE\Summrise\Agent) ────────────────────────────────
    /// One value of `HKLM\SOFTWARE\Summrise\Agent`, or `None` when absent/unreadable.
    fn reg_read(&self, name: &str) -> Option<String>;
    /// Write one value and VERIFY IT (the TypeScript's `regWrite` returns whether the write took,
    /// and `setup` reports a failure rather than printing a success it did not check).
    fn reg_write(&self, name: &str, value: &str) -> bool;

    // ── processes ──────────────────────────────────────────────────────────────
    /// Run a program with argv. NO SHELL, ever.
    fn run(&self, argv: &[String], timeout_ms: Option<u64>) -> RunResult;
    /// Three answers, because two of them are not the same: `tasklist` failing leaves stdout empty,
    /// and reading that as "not running" is a claim of ABSENCE from a failed READ.
    fn process_running(&self, image: &str) -> Tri;
    /// `cmd /c rmdir /s /q <path>` — `rmdir` is a cmd.exe BUILTIN and takes no argv, which is the
    /// one reason the TypeScript needed a shell at all. Named, so the shell cannot spread.
    fn remove_tree(&self, path: &Path) -> RunResult;
    /// `taskkill /F /IM <image>` — argv, unlike `rmdir`.
    fn taskkill_image(&self, image: &str) -> RunResult;

    // ── the release host ───────────────────────────────────────────────────────
    /// HTTP GET of a URL's body, or `None` when it could not be read. The TypeScript shells out to
    /// `curl` for the proxy/TLS store the rest of the CLI relies on; this is the same decision with
    /// the transport behind the trait.
    fn http_get(&self, url: &str, timeout_secs: u64) -> Option<String>;
    /// `curl -fsSL -m <timeout> -o <dest> <url>`: an HTTP error is a FAILURE, never a 404 page
    /// written to disk.
    fn http_download(&self, url: &str, dest: &Path, timeout_secs: u64) -> bool;

    // ── starting something that must outlive us ────────────────────────────────
    /// Start a program and RETURN WITHOUT WAITING FOR IT.
    ///
    /// `Err` is the spawn itself failing (the binary is missing) — `tunnel start`'s "FAILED to
    /// start cloudflared". `Ok(Some(code))` is the child EXITING inside `grace_ms`, which is the
    /// failure the TypeScript learned to watch for after its old form reported success for a
    /// cloudflared that died on a bad config. `Ok(None)` is "still alive when the window closed",
    /// which is the only thing this can honestly claim: whether it STAYS up is a separate question
    /// the caller asks with [`Host::process_running`].
    fn spawn_detached(&self, argv: &[String], grace_ms: u64) -> io::Result<Option<i32>>;

    // ── the npm package this CLI IS ────────────────────────────────────────────
    /// The package root the CLI runs from — the TypeScript's `__dirname/..`.
    ///
    /// It is a HOST fact and not a constant, because the shipped layout and a test's layout are
    /// different: on a device it is the directory above the running `bin\summrise.exe`, and in a
    /// source tree it is the sibling `summrise-agent-npm` the parity harness and this crate's own
    /// tests drive. The exe, the launcher and the desktop shell's sources are all found through it.
    fn package_dir(&self) -> String;
}

/// Yes / no / could-not-ask. Ported from `processRunning`'s three-way return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    Yes,
    No,
    Unknown,
}

impl Tri {
    /// `true`/`false` only when the probe actually answered — `None` when it did not, so a caller
    /// cannot accidentally render "could not read" as a verdict.
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Tri::Yes => Some(true),
            Tri::No => Some(false),
            Tri::Unknown => None,
        }
    }
}

/// The real machine. `Host` for everything that actually runs.
#[derive(Debug, Default, Clone)]
pub struct RealHost;

impl RealHost {
    pub fn new() -> Self {
        RealHost
    }
}

impl Host for RealHost {
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "device".to_string())
    }

    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn temp_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }

    fn make_temp_dir(&self, prefix: &str) -> io::Result<PathBuf> {
        let base = std::env::temp_dir();
        for n in 0..10_000u32 {
            let p = base.join(format!("{prefix}{}-{n}", std::process::id()));
            match std::fs::create_dir(&p) {
                Ok(()) => return Ok(p),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free temp dir",
        ))
    }

    fn read_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_bytes(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        std::fs::write(path, data)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn file_size(&self, path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.len())
    }

    fn mtime_ms(&self, path: &Path) -> Option<i64> {
        let m = std::fs::metadata(path).ok()?;
        let t = m.modified().ok()?;
        Some(
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        )
    }

    fn sha256_file(&self, path: &Path) -> Option<String> {
        crate::sha256::sha256_file(path).ok()
    }

    fn mkdirs(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(path)? {
            out.push(e?.path());
        }
        Ok(out)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn reg_read(&self, name: &str) -> Option<String> {
        // `reg query HKLM\SOFTWARE\Summrise\Agent /v <name>` then `REG_SZ\s+(.+)` on the line that
        // names the value. On a non-Windows box `reg` does not exist at all, which the TypeScript
        // treats the same way: fall through to the default.
        let out = std::process::Command::new("reg")
            .args(["query", "HKLM\\SOFTWARE\\Summrise\\Agent", "/v", name])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        crate::paths::parse_reg_sz(&text, name)
    }

    fn reg_write(&self, name: &str, value: &str) -> bool {
        std::process::Command::new("reg")
            .args([
                "add",
                "HKLM\\SOFTWARE\\Summrise\\Agent",
                "/v",
                name,
                "/t",
                "REG_SZ",
                "/d",
                value,
                "/f",
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn run(&self, argv: &[String], timeout_ms: Option<u64>) -> RunResult {
        let _ = timeout_ms; // the decisions that need a bound pass their own `curl -m`
        let Some((prog, rest)) = argv.split_first() else {
            return RunResult::default();
        };
        match std::process::Command::new(prog).args(rest).output() {
            Ok(o) => RunResult {
                status: o.status.code(),
                stdout: String::from_utf8_lossy(&o.stdout).to_string(),
                stderr: String::from_utf8_lossy(&o.stderr).to_string(),
            },
            Err(_) => RunResult::default(),
        }
    }

    fn process_running(&self, image: &str) -> Tri {
        let argv: Vec<String> = vec![
            "tasklist".into(),
            "/FI".into(),
            format!("IMAGENAME eq {image}"),
        ];
        let r = self.run(&argv, None);
        if r.status != Some(0) {
            return Tri::Unknown;
        }
        if r.stdout.to_lowercase().contains(&image.to_lowercase()) {
            Tri::Yes
        } else {
            Tri::No
        }
    }

    fn remove_tree(&self, path: &Path) -> RunResult {
        self.run(
            &[
                "cmd".into(),
                "/d".into(),
                "/s".into(),
                "/c".into(),
                "rmdir".into(),
                "/s".into(),
                "/q".into(),
                path.to_string_lossy().to_string(),
            ],
            None,
        )
    }

    fn taskkill_image(&self, image: &str) -> RunResult {
        self.run(
            &[
                "taskkill".into(),
                "/F".into(),
                "/IM".into(),
                image.to_string(),
            ],
            None,
        )
    }

    fn http_get(&self, url: &str, timeout_secs: u64) -> Option<String> {
        let r = self.run(
            &[
                "curl".into(),
                "-s".into(),
                "-m".into(),
                timeout_secs.to_string(),
                url.to_string(),
            ],
            Some(timeout_secs * 1000 + 2000),
        );
        if r.status != Some(0) || r.stdout.is_empty() {
            return None;
        }
        Some(r.stdout)
    }

    fn http_download(&self, url: &str, dest: &Path, timeout_secs: u64) -> bool {
        let r = self.run(
            &[
                "curl".into(),
                "-fsSL".into(),
                "-m".into(),
                timeout_secs.to_string(),
                "-o".into(),
                dest.to_string_lossy().to_string(),
                url.to_string(),
            ],
            Some(timeout_secs * 1000 + 2000),
        );
        r.status == Some(0) && dest.exists() && dest.metadata().map(|m| m.len()).unwrap_or(0) > 0
    }

    fn spawn_detached(&self, argv: &[String], grace_ms: u64) -> io::Result<Option<i32>> {
        use std::process::{Command, Stdio};
        let Some((prog, rest)) = argv.split_first() else {
            return Ok(None);
        };
        let mut cmd = Command::new(prog);
        cmd.args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // THE TUNNEL MUST OUTLIVE THE CONSOLE THAT LAUNCHED IT. Node's `detached: true` did this
        // on the TypeScript side; on Windows the same fact is a creation flag, and without it a
        // `summrise tunnel start` from a console that then closes takes cloudflared with it.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = cmd.spawn()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(grace_ms);
        loop {
            if let Some(st) = child.try_wait()? {
                // -1 for a signal, which is what the TypeScript's `code === null ? -1 : code` says.
                return Ok(Some(st.code().unwrap_or(-1)));
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn package_dir(&self) -> String {
        // THE SHIPPED LAYOUT FIRST: `<package>\bin\summrise.exe` -> `<package>`. See
        // [`package_dir_from`], which holds the rule and is what the test drives — the layout npm
        // installs cannot be stated here, because `current_exe()` in a test is the test binary.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(root) = package_dir_from(&exe) {
                return root;
            }
        }
        // ...and otherwise the source tree this crate is built in, which is where the parity
        // harness, `cargo test` and the release scripts run it from.
        format!("{}/../summrise-agent-npm", env!("CARGO_MANIFEST_DIR"))
    }
}

/// WHERE THIS CLI'S PACKAGE IS, GIVEN THE PATH OF THE RUNNING EXE — `None` when neither the exe's own
/// directory nor its parent is a package (a `target/debug/` build, or any other loose copy).
///
/// **IT IS A FUNCTION OF THE PATH SO THE SHIPPED LAYOUT CAN BE TESTED.** `RealHost::package_dir`
/// asks `std::env::current_exe()`, and in a test that is the test binary under `target/debug/deps/` —
/// a path no staging can turn into a package. Taking the path as an argument makes "the layout npm
/// installs" a case a test can state, which matters because this is the ONE thing about the cutover
/// (landing 4b) that no other instrument on this box can check: get it wrong on a device and `setup`
/// looks for `summrise-agent.exe` in a directory that does not exist.
///
/// BOTH LEVELS ARE TRIED, NEAREST FIRST, and the second is not decoration: npm's `bin` lives in
/// `bin/` (`package.json`'s `bin` points at `bin/summrise.exe`), while the agent exe and the launcher
/// sit at the package ROOT — so a staging change that moves the CLI up one level keeps working
/// instead of falling through to the SOURCE-TREE fallback, which on a device is a path that does not
/// exist. Only a directory holding `package.json` counts, so an unrelated `bin/` cannot match.
pub fn package_dir_from(exe: &Path) -> Option<String> {
    let mut dir = exe.parent();
    for _ in 0..2 {
        let d = dir?;
        if d.join("package.json").is_file() {
            return Some(d.to_string_lossy().to_string());
        }
        dir = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory that removes itself. The unique suffix is the process id plus a counter, because
    /// tests in one binary run on several threads and a shared path would let two of them race.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let d = std::env::temp_dir().join(format!(
                "summrise-cli-pkgdir-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("a scratch dir must be creatable");
            Scratch(d)
        }

        fn file(&self, rel: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"{}").unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// THE SHIPPED LAYOUT, WHICH IS WHAT THE CUTOVER MADE LOAD-BEARING.
    ///
    /// `package.json`'s `bin` is `bin/summrise.exe`, so on a device this is the exe's real path. If
    /// this case fails, `setup` on every device resolves the package to the source-tree fallback and
    /// reports `summrise-agent.exe missing from package: <a path that does not exist>` — a fresh
    /// install that cannot stage the agent.
    ///
    /// MUTATION: change the loop to try only the exe's own directory (`for _ in 0..1`) → RESULT:
    /// fails with "the shipped layout must resolve to the package root".
    #[test]
    fn the_shipped_bin_layout_resolves_to_the_package_root() {
        let s = Scratch::new();
        s.file("package.json");
        s.file("bin/summrise.exe");
        let exe = s.0.join("bin").join("summrise.exe");
        assert_eq!(
            package_dir_from(&exe).as_deref(),
            Some(s.0.to_string_lossy().as_ref()),
            "the shipped layout must resolve to the package root: npm's bin is \
             `<package>/bin/summrise.exe` and everything else the CLI stages (summrise-agent.exe, \
             summrise-launch.exe, the desktop shell) is found through it"
        );
    }

    /// AND THE ROOT LAYOUT, because the other two exes live there and a staging change that puts the
    /// CLI beside them must not silently stop resolving.
    #[test]
    fn an_exe_at_the_package_root_resolves_too() {
        let s = Scratch::new();
        s.file("package.json");
        let exe = s.0.join("summrise-cli.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        assert_eq!(
            package_dir_from(&exe).as_deref(),
            Some(s.0.to_string_lossy().as_ref()),
            "an exe staged at the package root must resolve to that root"
        );
    }

    /// A CARGO BUILD IS NOT A PACKAGE, and this is the case that keeps the fallback honest: if
    /// `package_dir_from` answered for `target/debug/`, the source-tree fallback would never run and
    /// the parity harness and the crate's own tests would read a package directory that is not there.
    #[test]
    fn a_loose_build_resolves_to_nothing() {
        let s = Scratch::new();
        s.file("debug/summrise-cli");
        assert_eq!(
            package_dir_from(&s.0.join("debug").join("summrise-cli")),
            None,
            "a directory with no package.json above it is not a package — the caller must fall back \
             to the source tree rather than invent one"
        );
    }
}
