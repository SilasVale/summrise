//! The record's change feed: envelopes over `record.db`, and the live fan-out.
//!
//! **THE RECORD IS THE STORE; THIS CRATE IS THE BUS.** Rows live in `summrise-record`; the bus
//! gives them the envelope shape, a sequence, a replay-then-live subscription, and the reset a
//! cursor below the retained window earns. A publish is one record insert (one transaction); a
//! read is one query. The segmented journal of earlier versions is imported once by [`Bus::open`],
//! keeping each row's sequence and moment.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use summrise_record::{Record, Row};
use tokio::sync::broadcast;

/// The wire shape's version. A reader that does not know it must refuse the row, not guess.
pub const SCHEMA_VERSION: u8 = 1;

/// The name of the database inside the directory a [`Bus`] is opened on.
pub const RECORD_FILE: &str = summrise_record::FILE;

/// Kept for callers that asked for a segment size; the record has no segments.
pub const DEFAULT_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;

/// The default retention budget: the bytes of retained envelope JSON.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Kept for callers that asked for a snapshot cadence; the record needs no snapshots.
pub const DEFAULT_SNAPSHOT_EVERY: u64 = 1_000;

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
    record: Record,
    /// Serializes sequence assignment with the insert, so two publishers cannot claim one sequence.
    publish: Mutex<()>,
    tx: broadcast::Sender<Arc<Envelope>>,
}

impl Bus {
    /// Open (or create) the record in `dir`, at `dir/record.db`.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with(dir, DEFAULT_SEGMENT_BYTES, DEFAULT_MAX_BYTES)
    }

    /// Open with an explicit retention budget. `segment_bytes` is accepted for callers that still
    /// pass it; the record has no segments.
    pub fn open_with(
        dir: impl AsRef<Path>,
        segment_bytes: u64,
        max_bytes: u64,
    ) -> io::Result<Self> {
        Self::open_with_snapshot(dir, segment_bytes, max_bytes, DEFAULT_SNAPSHOT_EVERY)
    }

    /// Open with a snapshot cadence. The cadence is ignored: a record needs no snapshots.
    pub fn open_with_snapshot(
        dir: impl AsRef<Path>,
        _segment_bytes: u64,
        max_bytes: u64,
        _snapshot_every: u64,
    ) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let record = Record::open(&dir, max_bytes)?;
        import_legacy(&dir, &record)?;
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            dir,
            record,
            publish: Mutex::new(()),
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

    /// Publish a traced event with an explicit moment (the import path needs it).
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

    /// Give the envelope its sequence, store it, and fan it out after the commit.
    fn append(&self, mut envelope: Envelope) -> io::Result<Envelope> {
        let _sequence = self
            .publish
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let seq = self.record.last_seq() + 1;
        envelope.seq = seq;
        let json = serde_json::to_string(&envelope)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.record.insert(&Row {
            seq,
            ts_ms: moment_ms(&envelope.ts),
            json,
        })?;
        let _ = self.tx.send(Arc::new(envelope.clone()));
        Ok(envelope)
    }

    /// A subscriber from `from_seq` inclusive: the retained tail first, then live events,
    /// deduplicated by `seq`.
    pub fn subscribe(&self, from_seq: u64) -> io::Result<Subscription> {
        let rx = self.tx.subscribe();
        let replay: Vec<Envelope> = self
            .record
            .read_from(from_seq)?
            .iter()
            .map(envelope_of)
            .collect::<io::Result<_>>()?;
        let last = replay.last().map(|event| event.seq).unwrap_or(0);
        Ok(Subscription {
            replay: replay.into(),
            rx,
            last,
        })
    }

    /// The rows whose moment falls in `[from_ms, to_ms)`, newest first, at most `limit`, and
    /// whether the range held more.
    pub fn read_between(
        &self,
        from_ms: u64,
        to_ms: u64,
        limit: usize,
    ) -> io::Result<(Vec<Envelope>, bool)> {
        let (rows, more) = self.record.between(from_ms, to_ms, limit)?;
        Ok((
            rows.iter().map(envelope_of).collect::<io::Result<_>>()?,
            more,
        ))
    }

    /// The last durable sequence. Zero when the record is empty.
    pub fn last_seq(&self) -> u64 {
        self.record.last_seq()
    }

    /// The first sequence still retained. Zero when the record is empty.
    pub fn oldest_seq(&self) -> u64 {
        self.record.oldest_seq()
    }

    /// The newest `limit` retained rows, newest first.
    pub fn scan_newest(&self, limit: usize) -> io::Result<Vec<Envelope>> {
        self.record.newest(limit)?.iter().map(envelope_of).collect()
    }

    /// A page of the record, newest first: up to `limit` retained rows older than `before_seq`,
    /// and whether a retained row lies below the page.
    pub fn read_window(
        &self,
        before_seq: Option<u64>,
        limit: usize,
    ) -> io::Result<(Vec<Envelope>, bool)> {
        let (rows, more) = self.record.window(before_seq, limit)?;
        Ok((
            rows.iter().map(envelope_of).collect::<io::Result<_>>()?,
            more,
        ))
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

    /// Copy the record to `path` (`VACUUM INTO`).
    pub fn backup_to(&self, path: impl AsRef<Path>) -> io::Result<()> {
        self.record.backup_to(path)
    }

    /// `PRAGMA integrity_check`.
    pub fn check(&self) -> io::Result<()> {
        self.record.check()
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

fn moment_ms(ts: &str) -> i64 {
    DateTime::parse_from_rfc3339(ts)
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(0)
}

fn envelope_of(row: &Row) -> io::Result<Envelope> {
    serde_json::from_str(&row.json)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Import a device's pre-record journal (`*.seg`) once, then mark it done.
///
/// The old rows keep their `seq` and their moment; the marker is written after the rows, so a crash
/// mid-import retries from scratch and a second open does nothing. A record that already has rows
/// is authoritative: the marker says `skipped:record-not-empty` and the segments stay on disk. A
/// torn last line is skipped, not fatal.
fn import_legacy(dir: &Path, record: &Record) -> io::Result<()> {
    if record.meta("journal_import")?.is_some() {
        return Ok(());
    }
    let segments = segment_files(dir);
    let marker = if record.last_seq() > 0 {
        serde_json::json!({ "skipped": "record-not-empty" }).to_string()
    } else if segments.is_empty() {
        serde_json::json!({ "files": 0, "rows": 0 }).to_string()
    } else {
        let mut imported = 0u64;
        for path in &segments {
            for envelope in envelopes_in(path) {
                let json = serde_json::to_string(&envelope)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                record.insert(&Row {
                    seq: envelope.seq,
                    ts_ms: moment_ms(&envelope.ts),
                    json,
                })?;
                imported += 1;
            }
        }
        serde_json::json!({
            "files": segments.len(),
            "rows": imported,
            "at": chrono::Utc::now().to_rfc3339(),
        })
        .to_string()
    };
    record.set_meta("journal_import", &marker)
}

/// The `*.seg` files in `dir`, in journal order.
fn segment_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("seg"))
        .collect();
    files.sort();
    files
}

/// The envelopes a segment holds, in file order; anything after the first unreadable line is
/// dropped with it, because a torn tail is where the crash was.
fn envelopes_in(path: &Path) -> Vec<Envelope> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map_while(|line| serde_json::from_str::<Envelope>(line).ok())
        .filter(|envelope| envelope.seq > 0)
        .collect()
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

    fn envelope_line(seq: u64, ts: &str, kind: &str) -> String {
        serde_json::to_string(&json!({
            "v": 1,
            "plane": "control",
            "seq": seq,
            "ts": ts,
            "type": kind,
            "payload": { "i": seq },
        }))
        .unwrap()
    }

    #[test]
    fn an_old_journal_is_imported_once() {
        let dir = temp_dir("import");
        let journal = dir.join("journal");
        std::fs::create_dir_all(&journal).unwrap();
        let lines = [
            envelope_line(1, "2026-11-13T10:00:00Z", "boot"),
            envelope_line(2, "2026-11-13T10:00:01Z", "call.started"),
        ];
        std::fs::write(journal.join("000001.seg"), lines.join("\n") + "\n").unwrap();

        let bus = Bus::open(&journal).unwrap();
        assert_eq!(bus.last_seq(), 2, "the imported rows keep their sequence");
        assert_eq!(bus.oldest_seq(), 1);
        assert_eq!(bus.scan_newest(2).unwrap()[0].kind, "call.started");
        assert!(bus.reset(1).is_none(), "nothing was lost to retention");
        drop(bus);

        let bus = Bus::open(&journal).unwrap();
        assert_eq!(
            bus.scan_newest(10).unwrap().len(),
            2,
            "a second open does not duplicate"
        );
    }

    #[test]
    fn a_torn_segment_imports_the_whole_lines_it_holds() {
        let dir = temp_dir("import-torn");
        let journal = dir.join("journal");
        std::fs::create_dir_all(&journal).unwrap();
        let text = format!(
            "{}\n{}\n{{\"v\":1,\"seq\":3",
            envelope_line(1, "2026-11-13T10:00:00Z", "a"),
            envelope_line(2, "2026-11-13T10:00:01Z", "b"),
        );
        std::fs::write(journal.join("000001.seg"), text).unwrap();
        let bus = Bus::open(&journal).unwrap();
        assert_eq!(bus.last_seq(), 2);
        assert_eq!(bus.scan_newest(10).unwrap().len(), 2);
    }

    #[test]
    fn imported_rows_keep_their_moment() {
        let dir = temp_dir("import-moment");
        let journal = dir.join("journal");
        std::fs::create_dir_all(&journal).unwrap();
        let text = format!(
            "{}\n",
            envelope_line(1, "1970-01-01T00:00:10Z", "call.finished")
        );
        std::fs::write(journal.join("000001.seg"), text).unwrap();
        let bus = Bus::open(&journal).unwrap();
        let (rows, _) = bus
            .read_between(
                moment("1970-01-01T00:00:00Z"),
                moment("1970-01-01T00:00:20Z"),
                10,
            )
            .unwrap();
        assert_eq!(rows.len(), 1, "the imported row sits at its own moment");
    }

    #[test]
    fn an_existing_record_is_not_merged_with_old_segments() {
        let dir = temp_dir("import-existing");
        let journal = dir.join("journal");
        std::fs::create_dir_all(&journal).unwrap();
        {
            let bus = Bus::open(&journal).unwrap();
            bus.publish("live", Value::Null).unwrap();
        }
        let text = format!("{}\n", envelope_line(5, "2026-11-13T10:00:00Z", "stale"));
        std::fs::write(journal.join("000001.seg"), text).unwrap();
        let bus = Bus::open(&journal).unwrap();
        assert_eq!(bus.last_seq(), 1, "the record is authoritative");
        assert_eq!(bus.scan_newest(10).unwrap()[0].kind, "live");
    }

    #[test]
    fn check_answers_ok() {
        let dir = temp_dir("check");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", Value::Null).unwrap();
        bus.check().expect("a fresh record is intact");
    }
}
