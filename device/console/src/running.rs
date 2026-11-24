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
    /// The call's bounded arguments, as the proxy already builds them for the finished row. They are
    /// what lets a page scope a running call to a session (`evidence.session`), and they never carry
    /// the device's answer: the proxy announces before the wait.
    evidence: Option<Value>,
    /// The permanent token record that authenticated a client's call, by id, never the secret.
    token_id: Option<String>,
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
        if let Some(evidence) = &self.evidence {
            value["evidence"] = evidence.clone();
        }
        if let Some(token_id) = &self.token_id {
            value["token_id"] = json!(token_id);
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
        evidence: Option<Value>,
        token_id: Option<&str>,
    ) -> Started {
        let id = format!("call-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token_id = token_id.map(str::to_owned);
        let entry = Started {
            id,
            at_ms: now_ms,
            caller,
            device_id: device_id.map(str::to_owned),
            tool: tool.to_owned(),
            evidence: evidence.clone(),
            token_id: token_id.clone(),
        };
        let started = Started {
            id: entry.id.clone(),
            at_ms: entry.at_ms,
            caller: entry.caller,
            device_id: entry.device_id.clone(),
            tool: entry.tool.clone(),
            evidence,
            token_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Running {
        Running::new(Duration::from_secs(90))
    }

    #[test]
    fn a_started_call_names_the_evidence_and_the_token_it_was_made_with() {
        let running = registry();
        let started = running.start(
            1_000,
            Caller::Client,
            Some("device-1"),
            "terminal.write",
            Some(json!({ "session": "session-1", "input": "npm test" })),
            Some("token-1"),
        );
        let value = started.value();
        assert_eq!(
            value["evidence"],
            json!({ "session": "session-1", "input": "npm test" })
        );
        assert_eq!(value["token_id"], json!("token-1"));
    }

    #[test]
    fn the_snapshot_carries_the_same_evidence_beside_the_caller() {
        let running = registry();
        running.start(
            1_000,
            Caller::Client,
            Some("device-1"),
            "terminal.write",
            Some(json!({ "session": "session-1", "input": "npm test" })),
            Some("token-1"),
        );
        let current = running.current(1_000);
        assert_eq!(current[0]["caller"], json!("client"));
        assert_eq!(
            current[0]["evidence"],
            json!({ "session": "session-1", "input": "npm test" })
        );
        assert_eq!(current[0]["token_id"], json!("token-1"));
    }

    #[test]
    fn a_started_call_without_evidence_or_token_names_neither() {
        let running = registry();
        let started = running.start(
            1_000,
            Caller::Console,
            Some("device-1"),
            "terminal.list",
            None,
            None,
        );
        let value = started.value();
        assert!(value.get("evidence").is_none(), "{value}");
        assert!(value.get("token_id").is_none(), "{value}");
        let current = running.current(1_000);
        assert!(current[0].get("evidence").is_none(), "{}", current[0]);
        assert!(current[0].get("token_id").is_none(), "{}", current[0]);
    }
}
