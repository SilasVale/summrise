//! `state`: the implementation of `summrise_core::capabilities::state`. The document is the device's
//! own store; this module is the readiness answer and the event tail the page draws.

use serde_json::{json, Map, Value};

use crate::mcp::ToolError;
use crate::store::{Store, EVENT_TAIL};
use summrise_core::capabilities::state::CAPABILITY;

/// Whether `tool` is one this capability declares.
pub fn owns(tool: &str) -> bool {
    CAPABILITY
        .tools
        .iter()
        .any(|declared| declared.name == tool)
}

/// Every tool this capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    CAPABILITY.tools.iter().map(|tool| tool.name)
}

/// Read the device's own state document.
pub fn call(
    tool: &str,
    _arguments: Map<String, Value>,
    store: &Store,
    version: &str,
    bus: Option<&summrise_bus::Bus>,
) -> Result<String, ToolError> {
    match tool {
        "state.get" => {
            // A poisoned document is the failure this tool exists to make visible, so it is a
            // successful call carrying `poisoned: true` and a null state, not an error the console
            // would render as "the device is offline".
            let (state, health) = match store.load() {
                Ok(state) => (
                    json!(state),
                    json!({
                        "status": "ok",
                        "version": version,
                        "pid": std::process::id(),
                        "poisoned": false,
                    }),
                ),
                Err(error) => (
                    Value::Null,
                    json!({
                        "status": "degraded",
                        "version": version,
                        "pid": std::process::id(),
                        "poisoned": true,
                        "reason": format!("state: {error}"),
                    }),
                ),
            };
            // Best-effort: the document still answers when the log cannot be read, but the field is
            // `null` — "the device cannot say" — not `[]`, which would claim it recorded nothing.
            let events = match store.tail_events(EVENT_TAIL) {
                Ok(events) => json!(events),
                Err(_) => Value::Null,
            };
            // The record's facts are the device page's answer to "is my history here": where it
            // lives, how big it is, and the sequence bounds. `null` on a process with no record,
            // never an invented zero.
            let record = match bus.and_then(|bus| bus.facts().ok()) {
                Some(facts) => json!(facts),
                None => Value::Null,
            };
            Ok(json!({
                "state": state,
                "health": health,
                "events": events,
                "record": record,
                // A recorded digest is not a measurement. This is the file the supervisor will boot
                // next, hashed now, so a baseline whose sidecar and bytes disagree cannot hide
                // behind its own metadata.
                "baseline": baseline_facts(&state),
            })
            .to_string())
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the state capability does not declare this tool"
        ))),
    }
}

/// The `baseline/current` pair as the disk holds it: the path the state document names, the digest
/// it records, and the digest of the file actually there.
///
/// `matches` is false when either side is missing — an unreadable file is not a match, and a
/// document that names no baseline has nothing to compare (`null`).
fn baseline_facts(state: &Value) -> Value {
    let Some(baseline) = state.get("baseline") else {
        return Value::Null;
    };
    let Some(path) = baseline.get("exe").and_then(Value::as_str) else {
        return Value::Null;
    };
    let recorded = baseline.get("sha256").and_then(Value::as_str);
    let file = sha256_of_file(std::path::Path::new(path)).ok();
    let matches = match (&file, recorded) {
        (Some(file), Some(recorded)) => file.eq_ignore_ascii_case(recorded),
        _ => false,
    };
    json!({
        "path": path,
        "recorded": recorded,
        "file": file,
        "matches": matches,
    })
}

/// The sha256 of a file, read in chunks so a large binary is not held in memory.
fn sha256_of_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_facts_compare_the_file_not_only_the_record() {
        let dir = std::env::temp_dir().join(format!("state-baseline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the test dir");
        let exe = dir.join("summrise-device.exe");
        std::fs::write(&exe, b"the installed bytes").expect("the fixture writes");
        let digest = sha256_of_file(&exe).expect("the fixture hashes");

        let state = json!({
            "baseline": { "exe": exe, "sha256": digest.clone() }
        });
        let facts = baseline_facts(&state);
        assert_eq!(facts["matches"], true, "{facts}");
        assert_eq!(facts["file"], digest);

        std::fs::write(&exe, b"a different build").expect("the file changes");
        let facts = baseline_facts(&state);
        assert_eq!(facts["matches"], false, "{facts}");
        assert_ne!(facts["file"], digest);

        assert_eq!(
            baseline_facts(&json!({})),
            Value::Null,
            "no baseline has nothing to compare"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
