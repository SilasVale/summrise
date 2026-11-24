//! The console's own record: an append-only sequence of JSON lines, kept in SQLite.
//!
//! **ONE ROW PER LINE, ONE SEQUENCE, ONE FILE.** The audit and the denials log used to be text
//! files with one rotated generation; they are now two records (`audit.db`, `denials.db`) opened
//! through `summrise-record`. A line's moment is its `ts_ms`, which is what a day read selects by;
//! retention prunes the oldest rows under a byte budget, so a flood can shorten history but can
//! never split it across files a reader has to stitch together.
//!
//! The interface the rest of the console sees is unchanged: newest-first reads, a predicate applied
//! during the scan, and the `NewestSince` facts a day read states (lines examined, exhausted,
//! bounded, oldest moment).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use summrise_record::Record;

/// What a reader can prove about the source it reads: name, size, span and the configured cap.
#[derive(Debug, Serialize)]
pub(crate) struct SourceFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) oldest_ts_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) newest_ts_ms: Option<u64>,
    pub(crate) max_bytes: u64,
    /// No older generations exist: one record holds every retained line.
    pub(crate) rotations: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rotated: Option<RotatedFacts>,
}

/// Kept for the source answer's shape; the record has no predecessor.
#[derive(Debug, Serialize)]
pub(crate) struct RotatedFacts {
    pub(crate) file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) oldest_ts_ms: Option<u64>,
}

/// The three facts a day read states, plus the lines it found.
pub(crate) struct NewestSince {
    pub(crate) lines: Vec<Value>,
    /// How many lines the scan examined, including the one that proved the cut.
    pub(crate) lines_read: usize,
    /// The scan reached the record's first retained line without finding one older than the cut.
    pub(crate) exhausted: bool,
    /// The scan stopped at its line bound before reaching either the cut or the first line.
    pub(crate) bounded: bool,
    /// The oldest line the scan counted, when it counted one.
    pub(crate) oldest_ts_ms: Option<u64>,
}

/// One append-only line record.
pub(crate) struct JsonLines {
    db: PathBuf,
    record: Record,
    max_bytes: u64,
    append: tokio::sync::Mutex<()>,
}

impl JsonLines {
    /// The record beside `path` (`audit.jsonl` → `audit.db`), capped at `max_bytes`.
    pub(crate) fn new(path: PathBuf, max_bytes: u64) -> JsonLines {
        let db = path.with_extension("db");
        let record = Record::open_file(&db, max_bytes).unwrap_or_else(|error| {
            panic!("the console record {} cannot open: {error}", db.display())
        });
        import_legacy(&record, &db, max_bytes);
        JsonLines {
            db,
            record,
            max_bytes,
            append: tokio::sync::Mutex::new(()),
        }
    }

    /// What this reader is reading, and how far back it goes.
    pub(crate) async fn source(&self) -> SourceFacts {
        let _reading = self.append.lock().await;
        let bytes = std::fs::metadata(&self.db).ok().map(|meta| meta.len());
        let (oldest, newest) = self.record.span().unwrap_or((None, None));
        SourceFacts {
            file: Some(file_name_of(&self.db)),
            bytes,
            oldest_ts_ms: oldest.map(|ts| ts.max(0) as u64),
            newest_ts_ms: newest.map(|ts| ts.max(0) as u64),
            max_bytes: self.max_bytes,
            rotations: 0,
            rotated: None,
        }
    }

    /// Append one line. `false` means the line was not written (and has been reported).
    pub(crate) async fn append<T: Serialize>(&self, entry: &T) -> bool {
        let value = match serde_json::to_value(entry) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("summrise-console: could not serialize a JSONL line: {error}");
                return false;
            }
        };
        let json = value.to_string();
        let ts_ms = value
            .get("ts_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(i64::MAX as u64) as i64;
        let _writing = self.append.lock().await;
        match self.record.append(ts_ms, json) {
            Ok(_) => true,
            Err(error) => {
                eprintln!(
                    "summrise-console: could not append to {}: {error}",
                    self.db.display()
                );
                false
            }
        }
    }

    /// The newest `limit` lines, newest first.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.newest_blocking(limit)
    }

    /// The newest `limit` lines for a caller that cannot await.
    pub(crate) fn newest_blocking(&self, limit: usize) -> Vec<Value> {
        if limit == 0 {
            return Vec::new();
        }
        match self.record.newest(limit) {
            Ok(rows) => rows.iter().map(|row| parse(&row.json)).collect(),
            Err(error) => {
                eprintln!(
                    "summrise-console: could not read {}: {error}",
                    self.db.display()
                );
                Vec::new()
            }
        }
    }

    /// The newest `limit` lines that `matches` accepts, newest first.
    ///
    /// The predicate is applied to each row in turn, so another machine's newer traffic cannot push
    /// this machine's lines out of the window.
    pub(crate) async fn newest_matching<F>(&self, limit: usize, matches: F) -> Vec<Value>
    where
        F: Fn(&Value) -> bool,
    {
        if limit == 0 {
            return Vec::new();
        }
        let _reading = self.append.lock().await;
        let mut lines = Vec::new();
        let mut before = None;
        while lines.len() < limit {
            let (rows, more) = match self.record.window(before, 256) {
                Ok(page) => page,
                Err(error) => {
                    eprintln!(
                        "summrise-console: could not read {}: {error}",
                        self.db.display()
                    );
                    break;
                }
            };
            if rows.is_empty() {
                break;
            }
            let last = rows.last().map(|row| row.seq);
            for row in &rows {
                let Some(value) = parse_opt(&row.json) else {
                    continue;
                };
                if matches(&value) {
                    lines.push(value);
                    if lines.len() == limit {
                        return lines;
                    }
                }
            }
            if !more {
                break;
            }
            before = last;
        }
        lines
    }

    /// Lines at or after `since_ms`, newest first, scanned from the newest retained row.
    ///
    /// The scan stops at the first line older than the cut, at the record's first retained row, or
    /// after `max_lines` lines — whichever comes first.
    pub(crate) async fn newest_since(&self, since_ms: u64, max_lines: usize) -> NewestSince {
        let _reading = self.append.lock().await;
        let mut lines = Vec::new();
        let mut lines_read = 0;
        let mut bounded = false;
        let mut stopped_on_older = false;
        let mut before = None;
        'scan: loop {
            let (rows, more) = match self.record.window(before, 256) {
                Ok(page) => page,
                Err(error) => {
                    eprintln!(
                        "summrise-console: could not read {}: {error}",
                        self.db.display()
                    );
                    break;
                }
            };
            if rows.is_empty() {
                break;
            }
            let last = rows.last().map(|row| row.seq);
            for row in &rows {
                if lines_read == max_lines {
                    bounded = true;
                    break 'scan;
                }
                lines_read += 1;
                let Some(value) = parse_opt(&row.json) else {
                    continue;
                };
                let older = value
                    .get("ts_ms")
                    .and_then(Value::as_u64)
                    .is_some_and(|ts_ms| ts_ms < since_ms);
                if older {
                    stopped_on_older = true;
                    break 'scan;
                }
                lines.push(value);
            }
            if !more {
                break;
            }
            before = last;
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
}

/// Import the pre-record files (`audit.jsonl` and its one rotation) once, then mark it done.
///
/// **THE OLD LINES ARE NOT THROWN AWAY.** A console upgraded from the JSONL index still has
/// `audit.jsonl` (and possibly `audit.1.jsonl`) beside the new `audit.db`; the first open reads
/// them, keeps each line's `ts_ms`, and writes one marker in the record's `meta` — so a crash
/// mid-import retries, and a second open does nothing. A record that already has rows is
/// authoritative: the marker says `skipped:record-not-empty` and the files stay on disk.
fn import_legacy(record: &Record, db: &Path, max_bytes: u64) {
    if record.meta("jsonl_import").ok().flatten().is_some() {
        return;
    }
    let current = db.with_extension("jsonl");
    let rotated = db.with_extension("1.jsonl");
    let mut entries: Vec<(i64, String)> = Vec::new();
    for path in [&rotated, &current] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let ts_ms = value
                .get("ts_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(i64::MAX as u64) as i64;
            entries.push((ts_ms, line.to_owned()));
        }
    }
    let marker = if record.last_seq() > 0 {
        json_marker("skipped", "record-not-empty")
    } else if entries.is_empty() {
        json_marker("files", "0")
    } else {
        let _ = max_bytes;
        let imported = entries.len();
        if record.append_many(&entries).is_ok() {
            json_marker("rows", &imported.to_string())
        } else {
            json_marker("error", "append-failed")
        }
    };
    let _ = record.set_meta("jsonl_import", &marker);
}

fn json_marker(key: &str, value: &str) -> String {
    serde_json::json!({
        key: value,
        "at": chrono::Utc::now().to_rfc3339(),
    })
    .to_string()
}

fn parse(json: &str) -> Value {
    parse_opt(json).unwrap_or(Value::Null)
}

fn parse_opt(json: &str) -> Option<Value> {
    match serde_json::from_str(json) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("summrise-console: skipping an unreadable line: {error}");
            None
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
