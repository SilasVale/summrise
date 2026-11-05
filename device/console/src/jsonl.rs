//! One append-only JSONL file with a size cap and exactly one rotation.
//!
//! Shared by the audit index and the denial log, which both need the same discipline: one line per
//! append under a lock, `flush` before the caller continues, and rotation to `<stem>.1.jsonl` when
//! the next line would cross the cap. One generation is kept, so disk use is at most about twice the
//! cap and older lines are discarded by design. If rotation fails the line is still appended: a
//! growing file is a worse warning than a missing one.
//!
//! A failed write is reported to stderr and returns `false`; neither index is allowed to turn a
//! completed action into a failed one.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

/// The newest lines above a cut, and what reading them cost.
///
/// The three facts are not the lines: a day read has to state when its coverage starts (the file's
/// oldest line) and whether the line bound stopped it, and those are properties of the scan rather
/// than of any line in it.
pub(crate) struct NewestSince {
    /// The lines at or after the cut, newest first.
    pub(crate) lines: Vec<Value>,
    /// How many lines the scan examined, including the one that proved the cut.
    pub(crate) lines_read: usize,
    /// The scan reached the file's first line without finding one older than the cut.
    pub(crate) exhausted: bool,
    /// The scan stopped at its line bound before reaching either the cut or the file's start.
    pub(crate) bounded: bool,
    /// The oldest line the scan counted, when it counted one.
    pub(crate) oldest_ts_ms: Option<u64>,
}

/// One append-only JSONL file.
pub(crate) struct JsonLines {
    path: PathBuf,
    rotated: PathBuf,
    max_bytes: u64,
    append: tokio::sync::Mutex<()>,
}

impl JsonLines {
    /// The file at `path`, capped at `max_bytes`.
    pub(crate) fn new(path: PathBuf, max_bytes: u64) -> JsonLines {
        JsonLines {
            rotated: path.with_extension("1.jsonl"),
            path,
            max_bytes,
            append: tokio::sync::Mutex::new(()),
        }
    }

    /// Append one line. `false` means the line was not written (and has been reported).
    pub(crate) async fn append<T: Serialize>(&self, entry: &T) -> bool {
        let mut line = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("summrise-console: could not serialize a JSONL line: {error}");
                return false;
            }
        };
        line.push('\n');

        let _writing = self.append.lock().await;
        if let Some(parent) = self.path.parent() {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                eprintln!(
                    "summrise-console: could not create the directory {}: {error}",
                    parent.display()
                );
                return false;
            }
        }

        if let Err(error) = self.rotate_if_needed(line.len() as u64).await {
            eprintln!(
                "summrise-console: could not rotate {}: {error}",
                self.path.display()
            );
        }

        let result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?;
            file.write_all(line.as_bytes()).await?;
            // `write_all` returns once the bytes reach the blocking task, and `flush` waits for
            // the OS write, so a line is on disk before the response leaves.
            file.flush().await
        }
        .await;

        if let Err(error) = result {
            eprintln!(
                "summrise-console: could not append to {}: {error}",
                self.path.display()
            );
            return false;
        }
        true
    }

    /// Rotate the current file to `<stem>.1.jsonl` when the next line would cross the cap.
    async fn rotate_if_needed(&self, incoming: u64) -> std::io::Result<()> {
        let current = match tokio::fs::metadata(&self.path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if current == 0 || current.saturating_add(incoming) <= self.max_bytes {
            return Ok(());
        }

        match tokio::fs::remove_file(&self.rotated).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        tokio::fs::rename(&self.path, &self.rotated).await
    }

    /// The newest `limit` lines, newest first, across the current file and its one rotation.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.newest_matching(limit, |_| true).await
    }

    /// The newest `limit` lines that `matches` accepts, newest first, across the current file and its
    /// one rotation.
    ///
    /// The predicate is applied **while the file is scanned**, not to a pre-cut window: a read that
    /// asks for one machine's lines must not have them pushed out by another machine's newer traffic.
    pub(crate) async fn newest_matching<F>(&self, limit: usize, matches: F) -> Vec<Value>
    where
        F: Fn(&Value) -> bool,
    {
        if limit == 0 {
            return Vec::new();
        }

        let _reading = self.append.lock().await;
        let mut lines = self.read_newest_matching(&self.path, limit, &matches).await;
        if lines.len() < limit {
            let older = self
                .read_newest_matching(&self.rotated, limit - lines.len(), &matches)
                .await;
            lines.extend(older);
        }
        lines
    }

    /// Lines at or after `since_ms`, newest first, scanned backwards from the current file's end.
    ///
    /// The scan stops at the first line older than the cut, or at the file's first line, or after
    /// `max_lines` lines — whichever comes first. It deliberately does not read the rotated
    /// generation: a day whose start is older than the current file is partial, and the caller
    /// states `since` instead of stitching two generations into a whole the page cannot verify.
    ///
    /// The cost is reported as lines examined, not bytes read: the file is opened once, like every
    /// other read of this index, and the scan is the part the day sets its bound on.
    pub(crate) async fn newest_since(&self, since_ms: u64, max_lines: usize) -> NewestSince {
        let _reading = self.append.lock().await;
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return NewestSince {
                    lines: Vec::new(),
                    lines_read: 0,
                    exhausted: true,
                    bounded: false,
                    oldest_ts_ms: None,
                }
            }
            Err(error) => {
                eprintln!(
                    "summrise-console: could not read {}: {error}",
                    self.path.display()
                );
                return NewestSince {
                    lines: Vec::new(),
                    lines_read: 0,
                    exhausted: true,
                    bounded: false,
                    oldest_ts_ms: None,
                };
            }
        };

        let text = String::from_utf8_lossy(&bytes);
        let mut lines = Vec::new();
        let mut lines_read = 0;
        let mut bounded = false;
        let mut stopped_on_older = false;
        for line in text.lines().rev() {
            if lines_read == max_lines {
                bounded = true;
                break;
            }
            lines_read += 1;
            match serde_json::from_str::<Value>(line) {
                Ok(value) => {
                    let older = value
                        .get("ts_ms")
                        .and_then(Value::as_u64)
                        .is_some_and(|ts_ms| ts_ms < since_ms);
                    if older {
                        stopped_on_older = true;
                        break;
                    }
                    lines.push(value);
                }
                Err(error) => eprintln!(
                    "summrise-console: skipping an unreadable line in {}: {error}",
                    self.path.display()
                ),
            }
        }

        NewestSince {
            oldest_ts_ms: lines
                .last()
                .and_then(|line| line.get("ts_ms").and_then(Value::as_u64)),
            lines,
            lines_read,
            exhausted: !stopped_on_older && !bounded,
            bounded,
        }
    }

    /// The newest `limit` lines of one file that `matches` accepts, newest first.
    ///
    /// A line that cannot be parsed is skipped and reported: a crash can leave a partial last line,
    /// and a damaged line must not make the rest of the file unreadable.
    async fn read_newest_matching<F>(&self, path: &Path, limit: usize, matches: &F) -> Vec<Value>
    where
        F: Fn(&Value) -> bool,
    {
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => {
                eprintln!(
                    "summrise-console: could not read {}: {error}",
                    path.display()
                );
                return Vec::new();
            }
        };

        let text = String::from_utf8_lossy(&bytes);
        let mut lines = Vec::new();
        for line in text.lines().rev() {
            if lines.len() == limit {
                break;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(value) => {
                    if matches(&value) {
                        lines.push(value);
                    }
                }
                Err(error) => eprintln!(
                    "summrise-console: skipping an unreadable line in {}: {error}",
                    path.display()
                ),
            }
        }
        lines
    }
}
