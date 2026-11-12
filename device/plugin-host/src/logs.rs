//! The plugin log: one bounded file per namespace, and the operator's to read (design §8.2).
//!
//! A plugin's stderr is copied here line by line, and the supervisor writes the facts only it knows
//! (started, exited, restarting, failed). The plugin's stdout is not logged as a rule: it is
//! JSON-RPC, the session consumes it, and a log that repeats every frame is a log nobody reads. The
//! session logs the exceptions — a malformed line, a notification, an answer nobody waited for —
//! because those are the facts a plugin author needs when their server is not being called.
//!
//! The file is capped at [`MAX_LOG_BYTES`] with one rotation (`...log.1`). A plugin that writes in a
//! loop cannot fill the disk, and `plugins.logs` reads the current file only, so the answer is
//! bounded by the cap rather than by how long the plugin has been running.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

/// The largest a plugin log may become before it rotates.
pub const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

/// How many lines `plugins.logs` returns when the caller names no count.
pub const DEFAULT_LOG_LINES: usize = 200;

/// The most lines a caller may ask for; a log read is a read, not an archive.
pub const MAX_LOG_LINES: usize = 2_000;

/// Where a plugin's log lines go.
///
/// The host builds one per plugin and hands clones to the session, the stderr copier and the
/// supervisor, so every writer lands in the same file without coordination beyond the file itself.
#[derive(Clone)]
pub struct LogSink {
    target: Arc<dyn Target>,
}

/// What a [`LogSink`] writes through; the two implementations are a file and a test's memory.
trait Target: Send + Sync {
    fn write_line(&self, line: &str);
}

impl LogSink {
    /// A sink that appends to `path`, rotating at [`MAX_LOG_BYTES`].
    pub fn to_file(path: impl Into<PathBuf>) -> LogSink {
        LogSink {
            target: Arc::new(FileTarget { path: path.into() }),
        }
    }

    /// A sink that keeps its lines in memory, for the tests.
    #[cfg(test)]
    pub fn to_memory(lines: Arc<Mutex<Vec<String>>>) -> LogSink {
        LogSink {
            target: Arc::new(MemoryTarget { lines }),
        }
    }

    /// Write one fact, as one or more lines.
    ///
    /// A message that carries its own newlines is split so the file stays line-oriented; a caller
    /// that passes a whole stderr chunk does not create one unreadable mega-line.
    pub fn line(&self, text: impl AsRef<str>) {
        for line in text.as_ref().lines() {
            // A blank line in a stderr chunk is spacing, not output; the log keeps only facts.
            if !line.is_empty() {
                self.target.write_line(line);
            }
        }
    }
}

struct FileTarget {
    path: PathBuf,
}

impl Target for FileTarget {
    fn write_line(&self, line: &str) {
        append(&self.path, line, MAX_LOG_BYTES);
    }
}

#[cfg(test)]
struct MemoryTarget {
    lines: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
impl Target for MemoryTarget {
    fn write_line(&self, line: &str) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(line.to_owned());
        }
    }
}

/// Append `line` to `path`, rotating `path` to `path.1` when the write would pass `cap`.
///
/// Best effort by design: a plugin must not fail to start because its log directory is read-only.
/// The caller's facts are still on stderr and in the state, and a lost log line is not a lost call.
fn append(path: &Path, line: &str, cap: u64) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let existing = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let incoming = line.len() as u64 + 1;
    if existing > 0 && existing + incoming > cap {
        rotate(path);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

/// Move the current log to `<name>.1`, replacing the previous generation.
fn rotate(path: &Path) {
    let rotated = rotated_path(path);
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, &rotated);
}

/// `plugin-hello.log` -> `plugin-hello.log.1`.
fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

/// The last `lines` lines of the log at `path`, oldest first.
///
/// A missing file is an empty log, not an error: a plugin that has not printed anything yet has no
/// output, and the page shows "No output yet." for that. Lines are bounded by [`MAX_LOG_LINES`].
pub fn tail(path: &Path, lines: usize) -> io::Result<Vec<String>> {
    let lines = lines.min(MAX_LOG_LINES);
    if lines == 0 {
        return Ok(Vec::new());
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut tail: Vec<String> = text.lines().rev().take(lines).map(str::to_owned).collect();
    tail.reverse();
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::TempDir;

    #[test]
    fn lines_append_and_tail_returns_the_last_ones_oldest_first() {
        let dir = TempDir::new("plugin-log-append");
        let path = dir.join("logs").join("plugin-hello.log");
        let sink = LogSink::to_file(&path);

        for line in ["one", "two", "three"] {
            sink.line(line);
        }

        assert_eq!(
            tail(&path, 2).expect("the log is readable"),
            ["two", "three"],
            "the tail keeps the newest lines in reading order"
        );
        assert_eq!(tail(&path, 200).unwrap().len(), 3);
        assert!(tail(&path, 0).unwrap().is_empty());
    }

    #[test]
    fn a_missing_log_is_empty_not_an_error() {
        let dir = TempDir::new("plugin-log-missing");
        assert!(tail(&dir.join("logs").join("plugin-none.log"), 10)
            .expect("a missing log is empty")
            .is_empty());
    }

    #[test]
    fn a_message_with_newlines_stays_line_oriented() {
        let dir = TempDir::new("plugin-log-multiline");
        let path = dir.join("plugin-hello.log");
        LogSink::to_file(&path).line("first\nsecond\n\nthird\n");

        assert_eq!(
            tail(&path, 10).unwrap(),
            ["first", "second", "third"],
            "a stderr chunk becomes lines, not one record with embedded newlines"
        );
    }

    #[test]
    fn the_log_rotates_once_at_its_cap_and_keeps_the_previous_generation() {
        let dir = TempDir::new("plugin-log-rotate");
        let path = dir.join("plugin-hello.log");
        let rotated = rotated_path(&path);

        append(&path, "old", 10);
        append(&path, "past-the-cap", 10);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "past-the-cap\n",
            "the current log starts again at the write that crossed the cap"
        );
        assert_eq!(
            fs::read_to_string(&rotated).unwrap(),
            "old\n",
            "the previous generation survives as .1"
        );

        append(&path, "again", 10);
        append(&path, "once-more", 10);
        assert_eq!(
            fs::read_to_string(&rotated).unwrap(),
            "again\n",
            "a second rotation replaces .1 rather than growing an archive"
        );
    }

    #[test]
    fn a_memory_sink_keeps_what_was_written() {
        let lines = Arc::new(Mutex::new(Vec::new()));
        LogSink::to_memory(Arc::clone(&lines)).line("a\nb");

        assert_eq!(*lines.lock().unwrap(), ["a", "b"]);
    }
}
