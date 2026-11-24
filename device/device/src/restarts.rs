//! This app's own boot journal: when it came up, and from which release.
//!
//! Why a restart happened belongs to the launcher — `swap.result.json` and the state document's
//! install facts answer "which generation" and "was it rolled back". This file answers the other
//! half: WHEN did this device last come up, and how often lately. A device that restarts every few
//! minutes is a different problem from one running the wrong generation, and a console can show
//! both only if the device keeps the first.
//!
//! The journal is bounded ([`MAX_RECORDS`]) and rewritten whole through a temporary sibling, like
//! every other list this device keeps: a torn line would make the facts unreadable, and the facts
//! are what an operator uses to decide whether to touch the machine.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file under `<data>`.
pub const FILE: &str = "restarts.jsonl";

/// How many boots are remembered. Enough for a pattern (a crash loop shows within minutes), small
/// enough that the file and the facts stay cheap.
pub const MAX_RECORDS: usize = 50;

/// How many the facts carry: the console shows a week of ordinary restarts, not the whole journal.
pub const IN_FACTS: usize = 10;

/// One boot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restart {
    /// When this process started, in Unix milliseconds.
    pub at_ms: u64,
    /// The release version it is running, from the launcher's argv (or the crate version standalone).
    pub version: String,
    /// The release digest, or empty for a standalone start that verified nothing.
    pub sha256: String,
    /// The release channel the sidecar named.
    pub channel: String,
    /// This process's pid, so a log line can be tied to a boot.
    pub pid: u32,
}

/// Append one boot, dropping the oldest when the journal is full.
pub fn record(data: &Path, entry: &Restart) -> std::io::Result<()> {
    let path = data.join(FILE);
    let mut lines: Vec<String> = match std::fs::read_to_string(&path) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    lines.push(line);
    if lines.len() > MAX_RECORDS {
        let excess = lines.len() - MAX_RECORDS;
        lines.drain(..excess);
    }
    let mut body = lines.join("\n");
    body.push('\n');
    let (temporary, mut file) = crate::fsutil::create_new_sibling(&path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// The most recent `limit` boots, newest first. An unreadable journal is empty: the facts must not
/// fail because a log line is damaged.
pub fn recent(data: &Path, limit: usize) -> Vec<Restart> {
    let Ok(text) = std::fs::read_to_string(data.join(FILE)) else {
        return Vec::new();
    };
    let mut entries: Vec<Restart> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A uniquely named directory under the system temp directory, removed when dropped.
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> TempDir {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "summrise-restarts-test-{}-{unique}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the test directory is creatable");
            TempDir { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn entry(at_ms: u64) -> Restart {
        Restart {
            at_ms,
            version: "0.3.127".to_owned(),
            sha256: "ab".repeat(32),
            channel: "canary".to_owned(),
            pid: 42,
        }
    }

    #[test]
    fn the_journal_is_newest_first_and_bounded() {
        let dir = TempDir::new("restarts-bound");
        for at in 1..=(MAX_RECORDS as u64 + 5) {
            record(dir.path(), &entry(at)).expect("the journal is writable");
        }
        let history = recent(dir.path(), IN_FACTS);
        assert_eq!(history.len(), IN_FACTS);
        assert_eq!(
            history[0].at_ms,
            MAX_RECORDS as u64 + 5,
            "the newest boot is first"
        );
        assert!(
            history.windows(2).all(|pair| pair[0].at_ms > pair[1].at_ms),
            "the order is the journal's, newest first: {history:?}"
        );

        let all = recent(dir.path(), MAX_RECORDS + 100);
        assert_eq!(
            all.len(),
            MAX_RECORDS,
            "the journal keeps a bounded history"
        );
        assert_eq!(
            all.last().map(|entry| entry.at_ms),
            Some(6),
            "oldest evicted"
        );
    }

    #[test]
    fn a_missing_or_damaged_journal_reads_as_empty() {
        let dir = TempDir::new("restarts-damaged");
        assert!(recent(dir.path(), IN_FACTS).is_empty(), "missing is empty");
        std::fs::write(dir.path().join(FILE), b"not json\n{}\n")
            .expect("the fixture journal is writable");
        assert!(
            recent(dir.path(), IN_FACTS).is_empty(),
            "damaged lines are skipped, not fatal"
        );
    }
}
