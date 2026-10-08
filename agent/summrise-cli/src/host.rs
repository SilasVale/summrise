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
}
