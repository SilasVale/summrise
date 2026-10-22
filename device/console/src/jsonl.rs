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
    ///
    /// A line that cannot be parsed is skipped and reported: a crash can leave a partial last line,
    /// and a damaged line must not make the rest of the file unreadable.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        if limit == 0 {
            return Vec::new();
        }

        let _reading = self.append.lock().await;
        let mut lines = self.read_newest(&self.path, limit).await;
        if lines.len() < limit {
            let older = self.read_newest(&self.rotated, limit - lines.len()).await;
            lines.extend(older);
        }
        lines
    }

    /// The newest `limit` lines of one file, newest first.
    async fn read_newest(&self, path: &Path, limit: usize) -> Vec<Value> {
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
            match serde_json::from_str(line) {
                Ok(value) => lines.push(value),
                Err(error) => eprintln!(
                    "summrise-console: skipping an unreadable line in {}: {error}",
                    path.display()
                ),
            }
        }
        lines
    }
}
