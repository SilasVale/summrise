//! The device's event bus: a segmented journal, one sequence, subscribers with cursors.
//!
//! S1 of `docs/design/architecture-of-record-2026-11-12.md`: the control plane is a single logical
//! journal with a monotonic `seq`, written and fsynced before an event is published, and read back by
//! subscribers from a cursor. S2 splits that journal into fixed-size segment files
//! (`000001.seg`, `000002.seg`, ...) so retention can delete the oldest segment without rewriting
//! sealed ones. The data plane is not here yet.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

/// The envelope's major schema. Additive changes stay at 1.
pub const SCHEMA_VERSION: u8 = 1;

/// How many published events a live subscriber may fall behind before it is told it lagged.
pub const CHANNEL_CAPACITY: usize = 1024;

/// The production segment size: 4 MiB. A segment is sealed once the next line would exceed it.
pub const DEFAULT_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;

/// The production retention ceiling: 64 MiB across all segments.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// How many published events apart the journal writes a snapshot.
pub const DEFAULT_SNAPSHOT_EVERY: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    Control,
    Data,
}

/// One event: the unit the journal stores and subscribers receive.
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
    /// The last sequence a snapshot could cover: everything up to it is gone.
    pub snapshot_seq: u64,
    /// The first lost sequence, inclusive.
    pub lost_from: u64,
    /// The last lost sequence, inclusive.
    pub lost_to: u64,
}

/// The journal and its live fan-out. One writer, one sequence.
pub struct Bus {
    dir: PathBuf,
    journal: Mutex<Journal>,
    segment_bytes: u64,
    max_bytes: u64,
    snapshot_every: u64,
    seq: AtomicU64,
    tx: broadcast::Sender<Arc<Envelope>>,
}

/// The active segment and the sizes of every live segment, under one writer's lock.
struct Journal {
    file: File,
    index: u64,
    size: u64,
    segments: BTreeMap<u64, u64>,
}

impl Bus {
    /// Open (or create) a segmented journal in `dir` with the production sizes, recovering the last
    /// durable `seq` and truncating a torn tail in the last segment.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with(dir, DEFAULT_SEGMENT_BYTES, DEFAULT_MAX_BYTES)
    }

    /// Open (or create) a segmented journal with explicit segment and retention sizes. Tests use
    /// small values; production goes through [`Bus::open`].
    pub fn open_with(
        dir: impl AsRef<Path>,
        segment_bytes: u64,
        max_bytes: u64,
    ) -> io::Result<Self> {
        Self::open_with_snapshot(dir, segment_bytes, max_bytes, DEFAULT_SNAPSHOT_EVERY)
    }

    /// Open (or create) a segmented journal with explicit sizes and snapshot cadence. A cadence of
    /// zero disables snapshots; tests otherwise use small values.
    pub fn open_with_snapshot(
        dir: impl AsRef<Path>,
        segment_bytes: u64,
        max_bytes: u64,
        snapshot_every: u64,
    ) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let (last_seq, mut segments) = recover(&dir)?;
        let index = segments.keys().next_back().copied().unwrap_or(1);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(segment_name(index)))?;
        let size = *segments.entry(index).or_insert(0);
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Ok(Self {
            dir,
            journal: Mutex::new(Journal {
                file,
                index,
                size,
                segments,
            }),
            segment_bytes,
            max_bytes,
            snapshot_every,
            seq: AtomicU64::new(last_seq),
            tx,
        })
    }

    /// Publish a control-plane event. The sequence is assigned, the line is appended and fsynced, and
    /// only then does it reach subscribers — a published `seq` is durable.
    pub fn publish(&self, kind: &str, payload: Value) -> io::Result<Envelope> {
        self.publish_on(Plane::Control, kind, None, payload)
    }

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
    ///
    /// The architecture's §4.2 is a pair: a call is `call.started` and then `call.finished` or
    /// `call.denied`, both carrying one `trace.call`, so a reader can always join them — and a
    /// proxied call joins the console's audit line on the same value. `None` is a plain publish,
    /// byte for byte what [`Bus::publish`] wrote before the field was used.
    pub fn publish_traced(
        &self,
        plane: Plane,
        kind: &str,
        subject: Option<Value>,
        call: Option<&str>,
        payload: Value,
    ) -> io::Result<Envelope> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let envelope = Envelope {
            v: SCHEMA_VERSION,
            plane,
            seq,
            ts: chrono::Utc::now().to_rfc3339(),
            kind: kind.to_owned(),
            subject,
            trace: call.map(|call| serde_json::json!({ "call": call })),
            payload,
        };
        let mut line = serde_json::to_vec(&envelope)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        {
            let mut journal = self
                .journal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if journal.size > 0
                && journal.size.saturating_add(line.len() as u64) > self.segment_bytes
            {
                self.rotate(&mut journal)?;
            }
            journal.file.write_all(&line)?;
            journal.file.sync_all()?;
            journal.size += line.len() as u64;
            let (index, size) = (journal.index, journal.size);
            if let Some(tracked) = journal.segments.get_mut(&index) {
                *tracked = size;
            }
        }
        // The event is durable before this point. Snapshots are derived data, so a failure to write
        // one must not fail a publish that already succeeded.
        let _ = self.write_snapshot(seq, &envelope.ts);
        let _ = self.tx.send(Arc::new(envelope.clone()));
        Ok(envelope)
    }

    /// A subscriber from `from_seq` inclusive: every segment's tail first, then live events,
    /// deduplicated by `seq`.
    pub fn subscribe(&self, from_seq: u64) -> io::Result<Subscription> {
        let rx = self.tx.subscribe();
        let indices: Vec<u64> = {
            let journal = self
                .journal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            journal.segments.keys().copied().collect()
        };
        let replay = read_from(&self.dir, &indices, from_seq)?;
        // The cursor is the last seq actually handed out, not `from_seq - 1`: a subscriber asking
        // past the end must still receive the next live event, not skip it.
        let last = replay.last().map(|event| event.seq).unwrap_or(0);
        Ok(Subscription {
            replay: replay.into(),
            rx,
            last,
        })
    }

    /// The last durable sequence. Zero when the journal is empty.
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// The first sequence still retained on disk. Zero when the journal is empty.
    pub fn oldest_seq(&self) -> u64 {
        let indices: Vec<u64> = {
            let journal = self
                .journal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            journal.segments.keys().copied().collect()
        };
        for index in indices {
            let Ok(bytes) = fs::read(self.dir.join(segment_name(index))) else {
                // Retention may delete a segment between the snapshot of the index list and this
                // read; the next retained segment still answers.
                continue;
            };
            for line in bytes.split(|byte| *byte == b'\n') {
                if line.is_empty() {
                    continue;
                }
                if let Ok(envelope) = serde_json::from_slice::<Envelope>(line) {
                    return envelope.seq;
                }
            }
        }
        0
    }

    /// The newest `limit` retained rows, newest first.
    pub fn scan_newest(&self, limit: usize) -> io::Result<Vec<Envelope>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let indices: Vec<u64> = {
            let journal = self
                .journal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            journal.segments.keys().copied().collect()
        };
        scan_newest_from(&self.dir, &indices, limit)
    }

    /// A page of the journal, newest first: up to `limit` retained rows older than `before_seq`
    /// (from the newest row when it is `None`), and whether a retained row lies below the page.
    ///
    /// The caller gets whole envelopes, because retention is the bus's fact and a summary is the
    /// tool's; the `more` flag is what lets a page stop asking, and it is found by reading exactly
    /// one row past the page rather than by walking the rest of the window.
    pub fn read_window(
        &self,
        before_seq: Option<u64>,
        limit: usize,
    ) -> io::Result<(Vec<Envelope>, bool)> {
        if limit == 0 {
            return Ok((Vec::new(), false));
        }
        let indices: Vec<u64> = {
            let journal = self
                .journal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            journal.segments.keys().copied().collect()
        };
        read_window_from(&self.dir, &indices, before_seq, limit)
    }

    /// The lost range for a subscriber whose cursor is `from`, or `None` when that cursor is still
    /// inside the retained window (or the journal is empty).
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

    /// Write `snapshot-<seq>.json` when this sequence is due one. An existing snapshot is never
    /// rewritten, and the file appears atomically: write the `.tmp` sibling, fsync, rename.
    fn write_snapshot(&self, seq: u64, ts: &str) -> io::Result<()> {
        if self.snapshot_every == 0 || !seq.is_multiple_of(self.snapshot_every) {
            return Ok(());
        }
        let path = self.dir.join(snapshot_name(seq));
        if path.exists() {
            return Ok(());
        }
        #[derive(Serialize)]
        struct Snapshot<'a> {
            v: u8,
            seq: u64,
            ts: &'a str,
        }
        let bytes = serde_json::to_vec(&Snapshot {
            v: SCHEMA_VERSION,
            seq,
            ts,
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let tmp = self.dir.join(format!("{}.tmp", snapshot_name(seq)));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &path)
    }

    /// Seal the active segment, start the next index, and prune old segments if retention demands it.
    fn rotate(&self, journal: &mut Journal) -> io::Result<()> {
        journal.file.sync_all()?;
        let index = journal.index + 1;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(segment_name(index)))?;
        let size = file.metadata()?.len();
        journal.file = file;
        journal.index = index;
        journal.size = size;
        journal.segments.insert(index, size);
        self.prune(journal);
        Ok(())
    }

    /// Delete the oldest segments while the total exceeds `max_bytes`, always leaving one.
    fn prune(&self, journal: &mut Journal) {
        while journal.segments.len() > 1 {
            let total: u64 = journal.segments.values().copied().sum();
            if total <= self.max_bytes {
                break;
            }
            let Some((&oldest, _)) = journal.segments.iter().next() else {
                break;
            };
            match fs::remove_file(self.dir.join(segment_name(oldest))) {
                Ok(()) => {
                    journal.segments.remove(&oldest);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    journal.segments.remove(&oldest);
                }
                Err(_) => break,
            }
        }
    }
}

/// A subscriber's cursor: journal rows, then live events.
pub struct Subscription {
    replay: VecDeque<Envelope>,
    rx: broadcast::Receiver<Arc<Envelope>>,
    last: u64,
}

impl Subscription {
    /// Every event the subscription can hand over right now: the replay it opened with and
    /// whatever is already queued live, without waiting for one more.
    ///
    /// Tests use it to read a journal after an action; a caller that must wait has [`Self::next`].
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

/// `000001.seg`, zero-padded to six digits.
fn segment_name(index: u64) -> String {
    format!("{index:06}.seg")
}

/// `snapshot-<seq>.json`.
fn snapshot_name(seq: u64) -> String {
    format!("snapshot-{seq}.json")
}

/// The index a segment file name carries, or `None` for anything else in the directory.
fn segment_index(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".seg")?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The segment indices present in `dir`, in journal order.
fn segment_indices(dir: &Path) -> io::Result<Vec<u64>> {
    let mut indices = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(index) = segment_index(name) {
            indices.push(index);
        }
    }
    indices.sort_unstable();
    Ok(indices)
}

/// Scan the segments in order for the last durable sequence, truncating a torn tail only in the last
/// one, and return each segment's live size.
fn recover(dir: &Path) -> io::Result<(u64, BTreeMap<u64, u64>)> {
    let indices = segment_indices(dir)?;
    let last = indices.last().copied();
    let mut last_seq = 0;
    let mut sizes = BTreeMap::new();
    for index in indices {
        let path = dir.join(segment_name(index));
        let mut bytes = Vec::new();
        File::open(&path)?.read_to_end(&mut bytes)?;
        let mut valid_end = 0;
        let mut offset = 0;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            offset += line.len();
            if line == b"\n" {
                valid_end = offset;
                continue;
            }
            match serde_json::from_slice::<Envelope>(line) {
                Ok(envelope) => {
                    last_seq = last_seq.max(envelope.seq);
                    valid_end = offset;
                }
                Err(_) => break,
            }
        }
        // Only the last segment may be rewritten; a sealed segment's torn tail is left untouched.
        if Some(index) == last && valid_end < bytes.len() {
            let file = OpenOptions::new().write(true).open(&path)?;
            file.set_len(valid_end as u64)?;
            file.sync_all()?;
            sizes.insert(index, valid_end as u64);
        } else {
            sizes.insert(index, bytes.len() as u64);
        }
    }
    Ok((last_seq, sizes))
}

/// One page of the journal, newest first: rows older than `before_seq` (all of them when it is
/// `None`), at most `limit`, plus whether a retained row lies below the page.
///
/// The walk is the same one [`scan_newest_from`] makes — segments newest-first, each walked from its
/// last line backwards — because a page is the tail of the record, and a reader that asked for twenty
/// rows must not parse a 64 MiB window to answer. `more` costs exactly one extra matching row: once the
/// page is full, the next row the walk meets is the proof that something lies below it, and the read
/// stops there rather than counting the rest.
fn read_window_from(
    dir: &Path,
    indices: &[u64],
    before_seq: Option<u64>,
    limit: usize,
) -> io::Result<(Vec<Envelope>, bool)> {
    let mut rows = Vec::new();
    for &index in indices.iter().rev() {
        let bytes = match fs::read(dir.join(segment_name(index))) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in bytes.split(|byte| *byte == b'\n').rev() {
            if line.is_empty() {
                continue;
            }
            let Ok(envelope) = serde_json::from_slice::<Envelope>(line) else {
                continue;
            };
            if before_seq.is_some_and(|before| envelope.seq >= before) {
                continue;
            }
            if rows.len() == limit {
                return Ok((rows, true));
            }
            rows.push(envelope);
        }
    }
    Ok((rows, false))
}

fn read_from(dir: &Path, indices: &[u64], from: u64) -> io::Result<Vec<Envelope>> {
    let mut events = Vec::new();
    for &index in indices {
        let bytes = match fs::read(dir.join(segment_name(index))) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Ok(envelope) = serde_json::from_slice::<Envelope>(line) else {
                continue;
            };
            if envelope.seq >= from {
                events.push(envelope);
            }
        }
    }
    Ok(events)
}

/// The newest `limit` rows across `indices`, newest first, in `limit` rows at most.
///
/// Segments are read from the newest index backwards, and each is walked from its last line to its
/// first, so a reader that only needs the tail never parses the bulk of a retention window.
fn scan_newest_from(dir: &Path, indices: &[u64], limit: usize) -> io::Result<Vec<Envelope>> {
    let mut newest = Vec::new();
    for &index in indices.iter().rev() {
        let bytes = match fs::read(dir.join(segment_name(index))) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in bytes.split(|byte| *byte == b'\n').rev() {
            if line.is_empty() {
                continue;
            }
            let Ok(envelope) = serde_json::from_slice::<Envelope>(line) else {
                continue;
            };
            newest.push(envelope);
            if newest.len() == limit {
                return Ok(newest);
            }
        }
    }
    Ok(newest)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn segments_in(dir: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("seg"))
            .collect();
        paths.sort();
        paths
    }

    fn read_segment(dir: &Path, index: u64) -> String {
        fs::read_to_string(dir.join(segment_name(index))).unwrap()
    }

    #[test]
    fn publishing_assigns_a_monotonic_sequence() {
        let dir = temp_dir("monotonic");
        let bus = Bus::open(&dir).unwrap();
        assert_eq!(bus.publish("a", Value::Null).unwrap().seq, 1);
        assert_eq!(bus.publish("b", Value::Null).unwrap().seq, 2);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
        let text = read_segment(&dir, 1);
        assert_eq!(text.lines().count(), 3);
        let first: Envelope = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first.v, SCHEMA_VERSION);
        assert_eq!(first.seq, 1);
        assert_eq!(first.kind, "a");
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
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        let mut subscription = bus.subscribe(3).unwrap();
        assert_eq!(subscription.next().await.unwrap().seq, 3);
        assert_eq!(subscription.next().await.unwrap().seq, 4);
        assert_eq!(subscription.next().await.unwrap().seq, 5);
        bus.publish("live", Value::Null).unwrap();
        let live = subscription.next().await.unwrap();
        assert_eq!(live.seq, 6);
        assert_eq!(live.kind, "live");
    }

    #[test]
    fn a_torn_tail_is_dropped_on_open() {
        let dir = temp_dir("torn");
        {
            let bus = Bus::open(&dir).unwrap();
            bus.publish("a", Value::Null).unwrap();
            bus.publish("b", Value::Null).unwrap();
        }
        let path = dir.join(segment_name(1));
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"v\":1,\"plane\":\"control\"").unwrap();
        drop(file);
        let bus = Bus::open(&dir).unwrap();
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
        let text = read_segment(&dir, 1);
        assert_eq!(text.lines().count(), 3);
        assert!(text
            .lines()
            .all(|line| serde_json::from_str::<Envelope>(line).is_ok()));
    }

    #[test]
    fn a_traced_publish_carries_the_call_on_the_wire_and_on_disk() {
        let dir = temp_dir("trace");
        let bus = Bus::open(&dir).unwrap();
        let envelope = bus
            .publish_traced(
                Plane::Control,
                "call.finished",
                None,
                Some("call-7"),
                Value::Null,
            )
            .unwrap();
        assert_eq!(
            envelope.trace,
            Some(serde_json::json!({ "call": "call-7" }))
        );
        let written: Envelope =
            serde_json::from_str(read_segment(&dir, 1).lines().next().unwrap()).unwrap();
        assert_eq!(written.trace, Some(serde_json::json!({ "call": "call-7" })));
        // A plain publish is unchanged: no trace written when the caller names none.
        assert_eq!(bus.publish("a", Value::Null).unwrap().trace, None);
    }

    #[tokio::test]
    async fn subscribing_past_the_end_is_live_only() {
        let dir = temp_dir("past");
        let bus = Bus::open(&dir).unwrap();
        bus.publish("a", Value::Null).unwrap();
        let mut subscription = bus.subscribe(99).unwrap();
        bus.publish("b", Value::Null).unwrap();
        let event = subscription.next().await.unwrap();
        assert_eq!(event.seq, 2);
    }

    #[test]
    fn rotation_creates_the_next_segment() {
        let dir = temp_dir("rotation");
        let bus = Bus::open_with(&dir, 200, 1 << 20).unwrap();
        for index in 0..5 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        let segments = segments_in(&dir);
        assert!(
            segments.len() >= 2,
            "expected a rotation, found {segments:?}"
        );
        assert!(dir.join(segment_name(1)).is_file());
        assert!(dir.join(segment_name(2)).is_file());
    }

    #[tokio::test]
    async fn replay_spans_two_segments() {
        let dir = temp_dir("span");
        let bus = Bus::open_with(&dir, 200, 1 << 20).unwrap();
        for index in 0..6 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            segments_in(&dir).len() >= 2,
            "expected at least two segments"
        );
        let mut subscription = bus.subscribe(1).unwrap();
        for seq in 1..=6 {
            assert_eq!(subscription.next().await.unwrap().seq, seq);
        }
    }

    #[test]
    fn retention_deletes_the_oldest_segment() {
        let dir = temp_dir("retention");
        let bus = Bus::open_with(&dir, 200, 300).unwrap();
        for index in 0..10 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            !dir.join(segment_name(1)).exists(),
            "retention should have deleted the oldest segment"
        );
        let segments = segments_in(&dir);
        assert!(!segments.is_empty(), "retention keeps at least one segment");
        // Retention runs after a rotation, so the sealed segments are bounded by `max_bytes` and the
        // active one may then grow by up to one segment before the next rotation.
        let total: u64 = segments
            .iter()
            .map(|path| fs::metadata(path).unwrap().len())
            .sum();
        assert!(
            total <= 300 + 200,
            "retention should bound the journal, found {total} bytes across {segments:?}"
        );
    }

    #[test]
    fn reopening_after_rotation_continues_the_sequence() {
        let dir = temp_dir("reopen-rotated");
        {
            let bus = Bus::open_with(&dir, 200, 1 << 20).unwrap();
            for index in 0..5 {
                bus.publish("e", serde_json::json!({ "i": index })).unwrap();
            }
            assert!(segments_in(&dir).len() >= 2, "expected a rotation");
        }
        let bus = Bus::open_with(&dir, 200, 1 << 20).unwrap();
        assert_eq!(bus.last_seq(), 5);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 6);
        let segments = segments_in(&dir);
        let mut seqs: Vec<u64> = Vec::new();
        for path in &segments {
            for line in fs::read_to_string(path).unwrap().lines() {
                seqs.push(serde_json::from_str::<Envelope>(line).unwrap().seq);
            }
        }
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=6).collect::<Vec<_>>(), "no seq is reused");
    }

    #[test]
    fn reset_names_the_lost_range() {
        let dir = temp_dir("reset");
        let bus = Bus::open_with(&dir, 200, 300).unwrap();
        for index in 0..10 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            !dir.join(segment_name(1)).exists(),
            "retention should have dropped the first segment"
        );

        let oldest = bus.oldest_seq();
        assert!(
            oldest > 1,
            "expected lost rows, oldest retained is {oldest}"
        );
        let reset = bus.reset(1).expect("reset(1) names the lost range");
        assert_eq!(reset.lost_from, 1);
        assert_eq!(reset.lost_to, reset.snapshot_seq);
        assert!(reset.snapshot_seq > 0);
        assert_eq!(reset.snapshot_seq, oldest - 1);

        assert!(
            bus.reset(oldest).is_none(),
            "a cursor inside the retained window is not lost"
        );
        assert!(bus.reset(oldest + 1).is_none());
    }

    #[test]
    fn a_snapshot_is_written_every_n_events() {
        let dir = temp_dir("snapshot");
        let bus = Bus::open_with_snapshot(&dir, 1 << 20, 1 << 20, 3).unwrap();
        for _ in 0..3 {
            bus.publish("e", Value::Null).unwrap();
        }
        let path = dir.join("snapshot-3.json");
        assert!(path.is_file(), "expected snapshot-3.json in {dir:?}");
        let first = fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(value["v"], 1);
        assert_eq!(value["seq"], 3);
        assert!(
            value["ts"].as_str().unwrap().contains('T'),
            "expected an rfc3339 timestamp, found {}",
            value["ts"]
        );
        assert!(
            !dir.join("snapshot-3.json.tmp").exists(),
            "the temporary file should have been renamed away"
        );

        for _ in 0..2 {
            bus.publish("e", Value::Null).unwrap();
        }
        assert!(!dir.join("snapshot-4.json").exists());
        assert!(!dir.join("snapshot-5.json").exists());
        assert_eq!(
            fs::read(&path).unwrap(),
            first,
            "the first snapshot must not be rewritten"
        );
    }

    #[test]
    fn scan_newest_answers_the_newest_rows_newest_first() {
        let dir = temp_dir("scan-newest");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..5 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        let seqs: Vec<u64> = bus
            .scan_newest(3)
            .unwrap()
            .iter()
            .map(|envelope| envelope.seq)
            .collect();
        assert_eq!(seqs, vec![5, 4, 3], "newest first, and only three");
    }

    #[test]
    fn scan_newest_beyond_the_end_answers_every_row() {
        let dir = temp_dir("scan-all");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..3 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        let seqs: Vec<u64> = bus
            .scan_newest(10)
            .unwrap()
            .iter()
            .map(|envelope| envelope.seq)
            .collect();
        assert_eq!(seqs, vec![3, 2, 1], "three rows, newest first");
    }

    #[test]
    fn scan_newest_of_zero_reads_nothing() {
        let dir = temp_dir("scan-zero");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..3 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            bus.scan_newest(0).unwrap().is_empty(),
            "a zero cap asks for no rows"
        );
    }

    #[test]
    fn read_window_stops_at_the_limit_and_says_there_is_more() {
        let dir = temp_dir("read-window-limit");
        // A small segment size makes the six rows span three segments, so the window is proved to
        // cross a rotation rather than to stay inside one file.
        let bus = Bus::open_with(&dir, 200, 1 << 20).unwrap();
        for index in 0..6 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            segments_in(&dir).len() >= 2,
            "expected the window to span segments"
        );

        let (rows, more) = bus.read_window(None, 3).unwrap();

        let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, vec![6, 5, 4], "the newest three rows, newest first");
        assert!(
            more,
            "rows 3..=1 are retained below the last row returned, so a page is not at the end"
        );
    }

    #[test]
    fn read_window_to_the_end_says_there_is_no_more() {
        let dir = temp_dir("read-window-end");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..3 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }

        let (rows, more) = bus.read_window(None, 10).unwrap();

        let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, vec![3, 2, 1], "every retained row, newest first");
        assert!(!more, "nothing is retained below the oldest row returned");
    }

    #[test]
    fn read_window_before_a_seq_excludes_it_and_everything_newer() {
        let dir = temp_dir("read-window-before");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..5 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }

        let (rows, more) = bus.read_window(Some(4), 10).unwrap();
        let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, vec![3, 2, 1], "`before_seq` is exclusive: {seqs:?}");
        assert!(!more, "the journal has nothing below seq 1");

        let (rows, more) = bus.read_window(Some(4), 2).unwrap();
        let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, vec![3, 2], "{seqs:?}");
        assert!(more, "seq 1 is retained below the page");

        let (rows, more) = bus.read_window(Some(1), 10).unwrap();
        assert!(
            rows.is_empty() && !more,
            "nothing is older than the oldest row: {rows:?} {more}"
        );
    }

    #[test]
    fn read_window_of_zero_answers_nothing() {
        let dir = temp_dir("read-window-zero");
        let bus = Bus::open(&dir).unwrap();
        for index in 0..3 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }

        let (rows, more) = bus.read_window(None, 0).unwrap();

        assert!(rows.is_empty(), "a zero cap asks for no rows");
        assert!(!more, "a page that returns no row claims no row below it");
    }

    #[tokio::test]
    async fn replay_from_within_the_window_still_works() {
        let dir = temp_dir("replay-window");
        let bus = Bus::open_with(&dir, 200, 300).unwrap();
        for index in 0..10 {
            bus.publish("e", serde_json::json!({ "i": index })).unwrap();
        }
        assert!(
            !dir.join(segment_name(1)).exists(),
            "retention should have dropped the first segment"
        );
        let oldest = bus.oldest_seq();
        assert!(
            oldest > 1,
            "expected lost rows, oldest retained is {oldest}"
        );

        let mut subscription = bus.subscribe(oldest).unwrap();
        for seq in oldest..=10 {
            assert_eq!(subscription.next().await.unwrap().seq, seq);
        }
        bus.publish("live", Value::Null).unwrap();
        assert_eq!(subscription.next().await.unwrap().seq, 11);
    }
}
