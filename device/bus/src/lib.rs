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

/// The journal and its live fan-out. One writer, one sequence.
pub struct Bus {
    dir: PathBuf,
    journal: Mutex<Journal>,
    segment_bytes: u64,
    max_bytes: u64,
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
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let envelope = Envelope {
            v: SCHEMA_VERSION,
            plane,
            seq,
            ts: chrono::Utc::now().to_rfc3339(),
            kind: kind.to_owned(),
            subject,
            trace: None,
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
}
