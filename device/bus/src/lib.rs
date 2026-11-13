//! The record's change feed: one durable row per event, one sequence, one writer.
//!
//! **THE RECORD IS SQLITE, AND THE FEED IS ITS TABLE.** A publish is one transaction that inserts
//! the envelope and prunes older rows under the retention budget; a reader is one query. The
//! process is the only writer, so the sequence is monotonic without coordination, and a crash
//! leaves the last committed transaction — not a torn line.
//!
//! Subscribers get the replay they asked for and then live rows. A cursor below the oldest retained
//! row is a [`Reset`]: the caller learns exactly what retention took, instead of silently reading a
//! gap.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::DateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

/// The wire shape's version. A reader that does not know it must refuse the row, not guess.
pub const SCHEMA_VERSION: u8 = 1;

/// The name of the database inside the directory a [`Bus`] is opened on.
pub const RECORD_FILE: &str = "record.db";

/// Kept for callers that asked for a segment size; the record has no segments.
pub const DEFAULT_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;

/// The default retention budget: the bytes of retained envelope JSON.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Kept for callers that asked for a snapshot cadence; the record needs no snapshots.
pub const DEFAULT_SNAPSHOT_EVERY: u64 = 1_000;

/// SQLite errors become the `io::Result` this API already returns.
trait Db<T> {
    fn db(self) -> io::Result<T>;
}

impl<T> Db<T> for rusqlite::Result<T> {
    fn db(self) -> io::Result<T> {
        self.map_err(io::Error::other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    Control,
    Data,
}

/// One event: the unit the record stores and subscribers receive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub plane: Plane,
    pub seq: u64,
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<Value>,
    #[serde(default)]
    pub payload: Value,
}

/// What a subscriber lost to retention when its cursor fell behind the oldest retained row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reset {
    /// The last sequence that is gone: everything up to it was pruned.
    pub snapshot_seq: u64,
    /// The first lost sequence, inclusive.
    pub lost_from: u64,
    /// The last lost sequence, inclusive.
    pub lost_to: u64,
}

/// The record and its live fan-out. One writer, one sequence.
pub struct Bus {
    dir: PathBuf,
    record: Mutex<Connection>,
    max_bytes: u64,
    seq: AtomicU64,
    tx: broadcast::Sender<Arc<Envelope>>,
}

impl Bus {
    /// Open (or create) the record in `dir`, at `dir/record.db`.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with(dir, DEFAULT_SEGMENT_BYTES, DEFAULT_MAX_BYTES)
    }

    /// Open with an explicit retention budget. `segment_bytes` is accepted for callers that still
    /// pass it; the record has no segments. Tests use small budgets.
    pub fn open_with(
        dir: impl AsRef<Path>,
        segment_bytes: u64,
        max_bytes: u64,
    ) -> io::Result<Self> {
        Self::open_with_snapshot(dir, segment_bytes, max_bytes, DEFAULT_SNAPSHOT_EVERY)
    }

    /// Open with a snapshot cadence. The cadence is ignored: a record needs no snapshots, and the
    /// parameter stays so an older caller's intent is visible rather than silently dropped.
    pub fn open_with_snapshot(
        dir: impl AsRef<Path>,
        _segment_bytes: u64,
        max_bytes: u64,
        _snapshot_every: u64,
    ) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(RECORD_FILE);
        let record = Connection::open(&path).db()?;
        record
            .execute_batch(
                "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS feed (
                 seq    INTEGER PRIMARY KEY,
                 ts_ms  INTEGER NOT NULL,
                 json   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');",
            )
            .db()?;
        let last: i64 = record
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM feed", [], |row| {
                row.get(0)
            })
            .db()?;
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            dir,
            record: Mutex::new(record),
            max_bytes,
            seq: AtomicU64::new(last.max(0) as u64),
            tx,
        })
    }

    /// The directory the record lives in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Publish one control-plane event.
    pub fn publish(&self, kind: &str, payload: Value) -> io::Result<Envelope> {
        self.publish_on(Plane::Control, kind, None, payload)
    }

    /// Publish one event on `plane`.
    pub fn publish_on(
        &self,
        plane: Plane,
        kind: &str,
        subject: Option<Value>,
        payload: Value,
    ) -> io::Result<Envelope> {
        self.publish_traced(plane, kind, subject, None, payload)
    }

    /// Publish a control-plane event whose `trace.call` names the call it belongs to.
    pub fn publish_traced(
        &self,
        plane: Plane,
        kind: &str,
        subject: Option<Value>,
        call: Option<&str>,
        payload: Value,
    ) -> io::Result<Envelope> {
        self.publish_traced_at(
            plane,
            kind,
            subject,
            call,
            &chrono::Utc::now().to_rfc3339(),
            payload,
        )
    }

    /// Publish a traced event with an explicit moment.
    ///
    /// The import path (an old record rewritten into this one) needs it: a row's moment is the
    /// moment it happened, not the moment it was migrated.
    pub fn publish_traced_at(
        &self,
        plane: Plane,
        kind: &str,
        subject: Option<Value>,
        call: Option<&str>,
        ts: &str,
        payload: Value,
    ) -> io::Result<Envelope> {
        let envelope = Envelope {
            v: SCHEMA_VERSION,
            plane,
            seq: 0,
            ts: ts.to_owned(),
            kind: kind.to_owned(),
            subject,
            trace: call.map(|call| serde_json::json!({ "call": call })),
            payload,
        };
        self.append(envelope)
    }

    /// Insert one envelope, prune under the budget, and fan it out after the commit.
    fn append(&self, mut envelope: Envelope) -> io::Result<Envelope> {
        let mut record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let transaction = record.transaction().db()?;
        let seq = self.seq.load(Ordering::SeqCst) + 1;
        envelope.seq = seq;
        let json = serde_json::to_string(&envelope)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let ts_ms = moment_ms(&envelope.ts);
        transaction
            .execute(
                "INSERT INTO feed (seq, ts_ms, json) VALUES (?1, ?2, ?3)",
                params![seq as i64, ts_ms, json],
            )
            .db()?;
        prune(&transaction, self.max_bytes)?;
        transaction.commit().db()?;
        self.seq.store(seq, Ordering::SeqCst);
        let _ = self.tx.send(Arc::new(envelope.clone()));
        Ok(envelope)
    }

    /// A subscriber from `from_seq` inclusive: the retained tail first, then live events,
    /// deduplicated by `seq`.
    pub fn subscribe(&self, from_seq: u64) -> io::Result<Subscription> {
        let rx = self.tx.subscribe();
        let replay = {
            let record = self
                .record
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mut statement = record
                .prepare("SELECT json FROM feed WHERE seq >= ?1 ORDER BY seq ASC")
                .db()?;
            let rows = statement
                .query_map(params![from_seq as i64], |row| row.get::<_, String>(0))
                .db()?;
            let mut events = Vec::new();
            for row in rows {
                events.push(parse_row(&row.map_err(io::Error::other)?)?);
            }
            events
        };
        let last = replay.last().map(|event| event.seq).unwrap_or(0);
        Ok(Subscription {
            replay: replay.into(),
            rx,
            last,
        })
    }

    /// The rows whose moment falls in `[from_ms, to_ms)`, newest first, at most `limit` rows, and
    /// whether the range held more rows than the answer.
    pub fn read_between(
        &self,
        from_ms: u64,
        to_ms: u64,
        limit: usize,
    ) -> io::Result<(Vec<Envelope>, bool)> {
        if limit == 0 || to_ms <= from_ms {
            return Ok((Vec::new(), false));
        }
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut statement = record
            .prepare(
                "SELECT json FROM feed WHERE ts_ms >= ?1 AND ts_ms < ?2 ORDER BY seq DESC LIMIT ?3",
            )
            .db()?;
        let rows = statement
            .query_map(
                params![from_ms as i64, to_ms as i64, (limit + 1) as i64],
                |row| row.get::<_, String>(0),
            )
            .db()?;
        let mut events = Vec::new();
        for row in rows {
            events.push(parse_row(&row.map_err(io::Error::other)?)?);
        }
        let more = events.len() > limit;
        events.truncate(limit);
        Ok((events, more))
    }

    /// The last durable sequence. Zero when the record is empty.
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// The first sequence still retained. Zero when the record is empty.
    pub fn oldest_seq(&self) -> u64 {
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        record
            .query_row("SELECT MIN(seq) FROM feed", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .flatten()
            .map(|seq| seq.max(0) as u64)
            .unwrap_or(0)
    }

    /// The newest `limit` retained rows, newest first.
    pub fn scan_newest(&self, limit: usize) -> io::Result<Vec<Envelope>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut statement = record
            .prepare("SELECT json FROM feed ORDER BY seq DESC LIMIT ?1")
            .db()?;
        let rows = statement
            .query_map(params![limit as i64], |row| row.get::<_, String>(0))
            .db()?;
        let mut events = Vec::new();
        for row in rows {
            events.push(parse_row(&row.map_err(io::Error::other)?)?);
        }
        Ok(events)
    }

    /// A page of the record, newest first: up to `limit` retained rows older than `before_seq`
    /// (from the newest row when it is `None`), and whether a retained row lies below the page.
    pub fn read_window(
        &self,
        before_seq: Option<u64>,
        limit: usize,
    ) -> io::Result<(Vec<Envelope>, bool)> {
        if limit == 0 {
            return Ok((Vec::new(), false));
        }
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let events = match before_seq {
            Some(before) => {
                let mut statement = record
                    .prepare("SELECT json FROM feed WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2")
                    .db()?;
                let rows = statement
                    .query_map(params![before as i64, (limit + 1) as i64], |row| {
                        row.get::<_, String>(0)
                    })
                    .db()?;
                collect_rows(rows)?
            }
            None => {
                let mut statement = record
                    .prepare("SELECT json FROM feed ORDER BY seq DESC LIMIT ?1")
                    .db()?;
                let rows = statement
                    .query_map(params![(limit + 1) as i64], |row| row.get::<_, String>(0))
                    .db()?;
                collect_rows(rows)?
            }
        };
        let more = events.len() > limit;
        let mut events = events;
        events.truncate(limit);
        Ok((events, more))
    }

    /// The lost range for a subscriber whose cursor is `from`, or `None` when that cursor is still
    /// inside the retained window (or the record is empty).
    pub fn reset(&self, from: u64) -> Option<Reset> {
        let oldest = self.oldest_seq();
        if oldest == 0 || from >= oldest {
            return None;
        }
        Some(Reset {
            snapshot_seq: oldest - 1,
            lost_from: from,
            lost_to: oldest - 1,
        })
    }

    /// Copy the record to `path` (`VACUUM INTO`): a consistent snapshot a backup can ship.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        record
            .execute("VACUUM INTO ?1", params![path.as_ref().to_string_lossy()])
            .db()?;
        Ok(())
    }

    /// `PRAGMA integrity_check`: `Ok` only when SQLite answers `ok`.
    pub fn check(&self) -> io::Result<()> {
        let record = self
            .record
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let answer: String = record
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .db()?;
        if answer == "ok" {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, answer))
        }
    }
}

/// A subscriber's cursor: record rows, then live events.
pub struct Subscription {
    replay: std::collections::VecDeque<Envelope>,
    rx: broadcast::Receiver<Arc<Envelope>>,
    last: u64,
}

impl Subscription {
    /// Every event the subscription can hand over right now, without waiting for one more.
    pub fn try_next(&mut self) -> Option<Envelope> {
        if let Some(event) = self.replay.pop_front() {
            self.last = event.seq;
            return Some(event);
        }
        match self.rx.try_recv() {
            Ok(event) => {
                if event.seq <= self.last {
                    return self.try_next();
                }
                self.last = event.seq;
                Some((*event).clone())
            }
            Err(_) => None,
        }
    }

    pub async fn next(&mut self) -> Option<Envelope> {
        if let Some(event) = self.replay.pop_front() {
            self.last = event.seq;
            return Some(event);
        }
        loop {
            match self.rx.recv().await {
                Ok(event) => {
                    if event.seq <= self.last {
                        continue;
                    }
                    self.last = event.seq;
                    return Some((*event).clone());
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

/// The moment of a row in Unix milliseconds; a timestamp this reader cannot parse is stored as 0
/// and stays out of every time range rather than being guessed into one.
fn moment_ms(ts: &str) -> i64 {
    DateTime::parse_from_rfc3339(ts)
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(0)
}

fn parse_row(json: &str) -> io::Result<Envelope> {
    serde_json::from_str(json).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn collect_rows<F>(rows: rusqlite::MappedRows<'_, F>) -> io::Result<Vec<Envelope>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>,
{
    let mut events = Vec::new();
    for row in rows {
        events.push(parse_row(&row.map_err(io::Error::other)?)?);
    }
    Ok(events)
}

/// Delete the oldest rows while the stored bytes exceed `max_bytes`, always leaving the newest row.
fn prune(transaction: &rusqlite::Transaction<'_>, max_bytes: u64) -> io::Result<()> {
    loop {
        let rows: i64 = transaction
            .query_row("SELECT COUNT(*) FROM feed", [], |row| row.get(0))
            .db()?;
        if rows <= 1 {
            return Ok(());
        }
        let bytes: i64 = transaction
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(json)), 0) FROM feed",
                [],
                |row| row.get(0),
            )
            .db()?;
        if (bytes as u64) <= max_bytes {
            return Ok(());
        }
        transaction
            .execute(
                "DELETE FROM feed WHERE seq = (SELECT MIN(seq) FROM feed)",
                [],
            )
            .db()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "summrise-bus-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn publishing_assigns_a_monotonic_sequence() {
        let dir = temp_dir("monotonic");
        let bus = Bus::open(&dir).unwrap();
        assert_eq!(bus.publish("a", Value::Null).unwrap().seq, 1);
        assert_eq!(bus.publish("b", Value::Null).unwrap().seq, 2);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
        assert_eq!(bus.last_seq(), 3);
        let newest = bus.scan_newest(3).unwrap();
        assert_eq!(
            newest.iter().map(|event| event.seq).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        assert_eq!(newest[0].kind, "c");
    }

    #[test]
    fn reopening_continues_the_sequence() {
        let dir = temp_dir("reopen");
        {
            let bus = Bus::open(&dir).unwrap();
            bus.publish("a", Value::Null).unwrap();
            bus.publish("b", Value::Null).unwrap();
        }
        let bus = Bus::open(&dir).unwrap();
        assert_eq!(bus.last_seq(), 2);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
    }

    #[tokio::test]
    async fn replay_from_a_cursor_then_live() {
        let dir = temp_dir("replay");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..5 {
            bus.publish("e", json!({ "i": index })).unwrap();
        }
        let mut subscription = bus.subscribe(3).unwrap();
        assert_eq!(subscription.next().await.unwrap().seq, 3);
        assert_eq!(subscription.next().await.unwrap().seq, 4);
        assert_eq!(subscription.next().await.unwrap().seq, 5);
        bus.publish("live", Value::Null).unwrap();
        assert_eq!(subscription.next().await.unwrap().seq, 6);
    }

    #[test]
    fn a_traced_publish_carries_the_call() {
        let dir = temp_dir("traced");
        let bus = Bus::open(&dir).unwrap();
        let envelope = bus
            .publish_traced(
                Plane::Control,
                "call.started",
                None,
                Some("call-7"),
                json!({}),
            )
            .unwrap();
        assert_eq!(envelope.trace, Some(json!({ "call": "call-7" })));
        assert_eq!(
            bus.scan_newest(1).unwrap()[0].trace,
            Some(json!({ "call": "call-7" }))
        );
    }

    #[tokio::test]
    async fn subscribing_past_the_end_is_live_only() {
        let dir = temp_dir("past-end");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", Value::Null).unwrap();
        let mut subscription = bus.subscribe(100).unwrap();
        assert!(subscription.try_next().is_none());
        bus.publish("b", Value::Null).unwrap();
        assert_eq!(subscription.next().await.unwrap().seq, 2);
    }

    #[test]
    fn retention_deletes_the_oldest_rows() {
        let dir = temp_dir("retention");
        let bus = Bus::open_with(&dir, 0, 400).unwrap();
        for index in 0..10 {
            bus.publish("e", json!({ "i": index, "pad": "x".repeat(40) }))
                .unwrap();
        }
        assert_eq!(bus.last_seq(), 10);
        assert!(
            bus.oldest_seq() > 1,
            "retention kept the budget: {}",
            bus.oldest_seq()
        );
        assert!(bus.oldest_seq() < 10, "retention left the newest rows");
    }

    #[test]
    fn reset_names_the_lost_range() {
        let dir = temp_dir("reset");
        let bus = Bus::open_with(&dir, 0, 300).unwrap();
        for index in 0..20 {
            bus.publish("e", json!({ "i": index, "pad": "x".repeat(40) }))
                .unwrap();
        }
        let oldest = bus.oldest_seq();
        assert!(oldest > 1);
        let reset = bus.reset(1).expect("the cursor is below the window");
        assert_eq!(reset.lost_from, 1);
        assert_eq!(reset.lost_to, oldest - 1);
        assert!(bus.reset(oldest).is_none());
    }

    #[test]
    fn scan_newest_beyond_the_end_answers_every_row() {
        let dir = temp_dir("scan-all");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", Value::Null).unwrap();
        bus.publish("b", Value::Null).unwrap();
        assert_eq!(bus.scan_newest(10).unwrap().len(), 2);
        assert!(bus.scan_newest(0).unwrap().is_empty());
    }

    #[test]
    fn read_window_stops_at_the_limit_and_says_there_is_more() {
        let dir = temp_dir("window");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..5 {
            bus.publish("e", json!({ "i": index })).unwrap();
        }
        let (page, more) = bus.read_window(None, 2).unwrap();
        assert_eq!(
            page.iter().map(|event| event.seq).collect::<Vec<_>>(),
            [5, 4]
        );
        assert!(more);
        let (page, more) = bus.read_window(Some(4), 2).unwrap();
        assert_eq!(
            page.iter().map(|event| event.seq).collect::<Vec<_>>(),
            [3, 2]
        );
        assert!(more);
        let (page, more) = bus.read_window(Some(2), 2).unwrap();
        assert_eq!(page.iter().map(|event| event.seq).collect::<Vec<_>>(), [1]);
        assert!(!more);
        assert!(bus.read_window(None, 0).unwrap().0.is_empty());
    }

    #[test]
    fn read_window_before_a_seq_excludes_it_and_everything_newer() {
        let dir = temp_dir("window-before");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..4 {
            bus.publish("e", json!({ "i": index })).unwrap();
        }
        let (page, _) = bus.read_window(Some(3), 10).unwrap();
        assert_eq!(
            page.iter().map(|event| event.seq).collect::<Vec<_>>(),
            [2, 1]
        );
    }

    impl Bus {
        fn publish_at(&self, ts: &str, kind: &str, payload: Value) -> io::Result<Envelope> {
            let envelope = Envelope {
                v: SCHEMA_VERSION,
                plane: Plane::Control,
                seq: 0,
                ts: ts.to_owned(),
                kind: kind.to_owned(),
                subject: None,
                trace: None,
                payload,
            };
            self.append(envelope)
        }
    }

    fn moment(ts: &str) -> u64 {
        DateTime::parse_from_rfc3339(ts).unwrap().timestamp_millis() as u64
    }

    #[test]
    fn read_between_includes_the_start_and_excludes_the_end() {
        let dir = temp_dir("between");
        let bus = Bus::open(&dir).unwrap();
        bus.publish_at("2026-11-13T10:00:00Z", "a", Value::Null)
            .unwrap();
        bus.publish_at("2026-11-13T10:00:01Z", "b", Value::Null)
            .unwrap();
        bus.publish_at("2026-11-13T10:00:02Z", "c", Value::Null)
            .unwrap();
        let (rows, more) = bus
            .read_between(
                moment("2026-11-13T10:00:00Z"),
                moment("2026-11-13T10:00:02Z"),
                10,
            )
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert!(!more);
    }

    #[test]
    fn read_between_stops_at_the_limit_and_says_more_lie_older() {
        let dir = temp_dir("between-limit");
        let bus = Bus::open(&dir).unwrap();
        for minute in 0..5 {
            bus.publish_at(&format!("2026-11-13T10:00:0{minute}Z"), "e", Value::Null)
                .unwrap();
        }
        let (rows, more) = bus
            .read_between(
                moment("2026-11-13T10:00:00Z"),
                moment("2026-11-13T10:01:00Z"),
                2,
            )
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(more);
    }

    #[test]
    fn read_between_of_an_empty_or_reversed_range_answers_nothing() {
        let dir = temp_dir("between-empty");
        let bus = Bus::open(&dir).unwrap();
        bus.publish_at("2026-11-13T10:00:00Z", "a", Value::Null)
            .unwrap();
        let start = moment("2026-11-13T10:00:00Z");
        assert!(bus.read_between(start, start, 10).unwrap().0.is_empty());
        assert!(bus.read_between(start + 1, start, 10).unwrap().0.is_empty());
        assert!(bus.read_between(start, start + 1, 0).unwrap().0.is_empty());
    }

    #[test]
    fn a_backup_is_a_readable_record() {
        let dir = temp_dir("backup");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", json!({ "ok": true })).unwrap();
        bus.publish("b", Value::Null).unwrap();
        let backup = temp_dir("backup-copy");
        bus.backup_to(backup.join(RECORD_FILE)).unwrap();
        let copy = Bus::open(&backup).unwrap();
        assert_eq!(copy.last_seq(), 2);
        assert_eq!(copy.scan_newest(2).unwrap()[0].kind, "b");
    }

    #[test]
    fn check_answers_ok() {
        let dir = temp_dir("check");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", Value::Null).unwrap();
        bus.check().expect("a fresh record is intact");
    }
}
