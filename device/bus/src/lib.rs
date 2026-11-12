//! The device's event bus: one journal, one sequence, subscribers with cursors.
//!
//! S1 of `docs/design/architecture-of-record-2026-11-12.md`: the control plane is a single append-only
//! journal with a monotonic `seq`, written and fsynced before an event is published, and read back by
//! subscribers from a cursor. The data plane is not here yet.

use std::collections::VecDeque;
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
    path: PathBuf,
    file: Mutex<File>,
    seq: AtomicU64,
    tx: broadcast::Sender<Arc<Envelope>>,
}

impl Bus {
    /// Open (or create) a journal at `path`, recovering the last durable `seq` and truncating a torn tail.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let last = recover(&path)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Ok(Self {
            path,
            file: Mutex::new(file),
            seq: AtomicU64::new(last),
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
            let mut file = self
                .file
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            file.write_all(&line)?;
            file.sync_all()?;
        }
        let _ = self.tx.send(Arc::new(envelope.clone()));
        Ok(envelope)
    }

    /// A subscriber from `from_seq` inclusive: the journal's tail first, then live events, deduplicated
    /// by `seq`.
    pub fn subscribe(&self, from_seq: u64) -> io::Result<Subscription> {
        let rx = self.tx.subscribe();
        let replay = read_from(&self.path, from_seq)?;
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

/// The last durable sequence, truncating a torn tail to its last complete line.
fn recover(path: &Path) -> io::Result<u64> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    }
    let mut last_seq = 0;
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
    if valid_end < bytes.len() {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(valid_end as u64)?;
        file.sync_all()?;
    }
    Ok(last_seq)
}

fn read_from(path: &Path, from: u64) -> io::Result<Vec<Envelope>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut events = Vec::new();
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
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_journal(tag: &str) -> PathBuf {
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
        dir.join("events.jsonl")
    }

    #[test]
    fn publishing_assigns_a_monotonic_sequence() {
        let path = temp_journal("monotonic");
        let bus = Bus::open(&path).unwrap();
        assert_eq!(bus.publish("a", Value::Null).unwrap().seq, 1);
        assert_eq!(bus.publish("b", Value::Null).unwrap().seq, 2);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        let first: Envelope = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first.v, SCHEMA_VERSION);
        assert_eq!(first.seq, 1);
        assert_eq!(first.kind, "a");
    }

    #[test]
    fn reopening_continues_the_sequence() {
        let path = temp_journal("reopen");
        {
            let bus = Bus::open(&path).unwrap();
            bus.publish("a", Value::Null).unwrap();
            bus.publish("b", Value::Null).unwrap();
        }
        let bus = Bus::open(&path).unwrap();
        assert_eq!(bus.last_seq(), 2);
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
    }

    #[tokio::test]
    async fn replay_from_a_cursor_then_live() {
        let path = temp_journal("replay");
        let bus = Bus::open(&path).unwrap();
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
        let path = temp_journal("torn");
        {
            let bus = Bus::open(&path).unwrap();
            bus.publish("a", Value::Null).unwrap();
            bus.publish("b", Value::Null).unwrap();
        }
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"v\":1,\"plane\":\"control\"").unwrap();
        drop(file);
        let bus = Bus::open(&path).unwrap();
        assert_eq!(bus.publish("c", Value::Null).unwrap().seq, 3);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text
            .lines()
            .all(|line| serde_json::from_str::<Envelope>(line).is_ok()));
    }

    #[tokio::test]
    async fn subscribing_past_the_end_is_live_only() {
        let path = temp_journal("past");
        let bus = Bus::open(&path).unwrap();
        bus.publish("a", Value::Null).unwrap();
        let mut subscription = bus.subscribe(99).unwrap();
        bus.publish("b", Value::Null).unwrap();
        let event = subscription.next().await.unwrap();
        assert_eq!(event.seq, 2);
    }
}
