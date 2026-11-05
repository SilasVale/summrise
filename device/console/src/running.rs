//! Calls the console is still waiting on, for the live stream's "running" line.
//!
//! A proxied call is published to the audit only after the device answers, so the record cannot say
//! one is running. This registry can: it is filled when the call begins and cleared when the proxy
//! stops waiting. **It is not the audit**: nothing here is written to a file, and a restart forgets
//! every entry, as it forgets every call that was in flight.
//!
//! The page's own copy is bounded by the proxy's cut. The snapshot applies the same bound here, so a
//! reconnect cannot resurrect a call whose `finished` frame was dropped, and a call that outlives
//! the proxy's own wait stops being "running" either way.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::audit::Caller;

/// One call as the proxy announced it.
pub(crate) struct Started {
    id: String,
    at_ms: u64,
    caller: Caller,
    device_id: Option<String>,
    tool: String,
}

impl Started {
    /// The call's id, which is what a `finished` frame names.
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// The `started` object of the frame. The caller is top-level in the frame, because the band
    /// filters on it; a snapshot's `running` entry carries it beside the same facts.
    pub(crate) fn value(&self) -> Value {
        let mut value = json!({
            "id": self.id,
            "tool": self.tool,
            "at_ms": self.at_ms,
        });
        if let Some(device_id) = &self.device_id {
            value["device_id"] = json!(device_id);
        }
        value
    }

    fn running_value(&self) -> Value {
        let mut value = self.value();
        value["caller"] = json!(self.caller);
        value
    }
}

/// The in-flight calls, newest last.
pub(crate) struct Running {
    cut_ms: u64,
    next_id: AtomicU64,
    entries: Mutex<Vec<Started>>,
}

impl Running {
    /// A registry whose entries live at most `cut`: the proxy's own bound on a call.
    pub(crate) fn new(cut: Duration) -> Running {
        Running {
            cut_ms: cut.as_millis() as u64,
            next_id: AtomicU64::new(1),
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Record one call and return the facts the frame publishes.
    pub(crate) fn start(
        &self,
        now_ms: u64,
        caller: Caller,
        device_id: Option<&str>,
        tool: &str,
    ) -> Started {
        let id = format!("call-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let entry = Started {
            id,
            at_ms: now_ms,
            caller,
            device_id: device_id.map(str::to_owned),
            tool: tool.to_owned(),
        };
        let started = Started {
            id: entry.id.clone(),
            at_ms: entry.at_ms,
            caller: entry.caller,
            device_id: entry.device_id.clone(),
            tool: entry.tool.clone(),
        };
        let mut entries = lock(&self.entries);
        entries.retain(|entry| !expired(entry, now_ms, self.cut_ms));
        entries.push(entry);
        started
    }

    /// Forget a call: its `finished` frame arrived, or its proxy stopped waiting.
    pub(crate) fn finish(&self, id: &str) {
        lock(&self.entries).retain(|entry| entry.id != id);
    }

    /// The calls still running at `now_ms`, oldest first, after dropping the ones past the cut.
    pub(crate) fn current(&self, now_ms: u64) -> Vec<Value> {
        let mut entries = lock(&self.entries);
        entries.retain(|entry| !expired(entry, now_ms, self.cut_ms));
        entries.iter().map(Started::running_value).collect()
    }
}

/// Whether an entry is older than the proxy's own cut: a `finished` frame that never arrived.
fn expired(entry: &Started, now_ms: u64, cut_ms: u64) -> bool {
    now_ms.saturating_sub(entry.at_ms) > cut_ms
}

/// Lock the registry, ignoring poisoning: a panic in one call must not stop the stream from
/// clearing the rest. The entries are replaced rather than repaired.
fn lock(entries: &Mutex<Vec<Started>>) -> std::sync::MutexGuard<'_, Vec<Started>> {
    entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
