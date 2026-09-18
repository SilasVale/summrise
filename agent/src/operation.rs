//! The device's OPERATION TIMELINE — the terminal audit trail and the browser
//! action feed merged into one ordered record of what the AI actually did.
//!
//! ## Why this exists
//!
//! The two records live in different files, are written by different producers,
//! and until now could only be read separately:
//!
//! | | terminal | browser |
//! |---|---|---|
//! | file | `{sessions}/<sid>.jsonl` | `{pwout}/actions.jsonl` |
//! | reader | `session_log::events_of` | `evidence::recent_actions` |
//! | route | `/api/sessions/<sid>` | `/api/browser/actions` |
//!
//! But an AI's actual work crosses both constantly — it opens a session, runs a
//! command, checks a web UI in the browser, runs another command. Nothing in the
//! product could show that as one story, and nothing could save it as one
//! memory record. This module is that one story.
//!
//! ## Device-level, not session-level
//!
//! Deliberately NOT scoped to a session. The embedded browser is a
//! DEVICE-level resource with no session ownership (`actions.jsonl` carries no
//! session id), and the AI's "one operation" is genuinely cross-session: it may
//! drive the browser with no terminal open at all. Scoping this to a session
//! would either drop those actions or require inventing an attribution the data
//! does not support. Session ids ride along as an ATTRIBUTE of the terminal
//! events instead.
//!
//! ## The unit rule
//!
//! Ordering is by `ts_ms`, NEVER by `ts`. The two feeds stamp `ts` in different
//! units — seconds for the terminal trail, milliseconds for the browser feed —
//! so sorting on it interleaves them wrongly while looking ordered. Every record
//! here carries the timestamp under the explicit name, and a record lacking it is
//! DROPPED rather than assumed, because guessing a unit is exactly the silent
//! error this module exists to avoid.

use serde_json::{json, Value};
use std::path::Path;

/// Terminal event kinds that belong on an operation timeline.
///
/// Output chunks and session-lifecycle chatter are excluded on purpose: this is
/// the story of what was DONE, not a transcript. The full text stays available
/// through `terminal_read`, which is where a reader who wants it should go.
const OPERATION_KINDS: &[&str] = &[
    "command/start",
    "command/end",
    "goal",
    "plan",
    "approval",
    "control",
];

/// Read and merge the two feeds, oldest first, most recent `limit` entries.
///
/// `since_ms` filters both feeds; pass 0 for everything. Unknown or missing
/// timestamps drop the record (see the module header).
pub(crate) fn merged_operation(
    sessions_dir: &Path,
    evidence_dir: &Path,
    since_ms: u64,
    limit: usize,
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    out.extend(terminal_events(sessions_dir, since_ms));
    out.extend(browser_actions(evidence_dir, since_ms));
    // Stable sort on the explicit millisecond stamp. `sort_by_key` is stable, so
    // two records in the same millisecond keep their feed order rather than
    // shuffling between polls.
    out.sort_by_key(|e| e["ts_ms"].as_u64().unwrap_or(0));
    if out.len() > limit {
        out.drain(..out.len() - limit);
    }
    out
}

/// One session's events, mapped onto the operation shape.
/// ONE terminal record, mapped onto the wire shape.
///
/// EXTRACTED IN ROUND 66 so the allowlist can be asserted against
/// `agent/tests/fixtures/run-event.json` without a data dir — the same reason `summary_of` was split out of
/// `summary` in round 65.
///
/// THIS MAPPING IS AN ALLOWLIST, and that is the whole risk: a field missing here is dropped SILENTLY, with every
/// other test still green, and the panel renders a row with a hole in it. `run_id` was the first field to make that
/// trip and it had a test of its own; this function is what lets the fixture pin ALL of them, on both feeds.
pub fn terminal_row(v: &Value, sid: &str, ts_ms: u64) -> Value {
    let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    json!({
        "source": "terminal",
        "ts_ms": ts_ms,
        "session": sid,
        "kind": kind,
        "seq": v.get("seq").cloned().unwrap_or(Value::Null),
        "command": v.get("command").cloned().unwrap_or(Value::Null),
        "text": v.get("text").cloned().unwrap_or(Value::Null),
        "status": v.get("status").cloned().unwrap_or(Value::Null),
        "exit_code": v.get("exit_code").cloned().unwrap_or(Value::Null),
        "duration_ms": v.get("duration_ms").cloned().unwrap_or(Value::Null),
        "intent": v.get("intent").cloned().unwrap_or(Value::Null),
        "considered": v.get("considered").cloned().unwrap_or(Value::Null),
        "plan_step": v.get("plan_step").cloned().unwrap_or(Value::Null),
        // The run this event belonged to, when the client declared one. Carried through the merge because grouping
        // by run is the whole point of recording it.
        "run_id": v.get("run_id").cloned().unwrap_or(Value::Null),
    })
}

fn terminal_events(sessions_dir: &Path, since_ms: u64) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        // The sid is the file stem — the same identity the audit routes use.
        let Some(sid) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // A file untouched since `since_ms` cannot contain a newer event, so
        // skip it without reading. This is what keeps the endpoint cheap on a
        // device with hundreds of retained sessions.
        if since_ms > 0 {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    let mtime_ms = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    // One second of slack: an event at the boundary must not be
                    // dropped because its file's mtime rounded down.
                    if mtime_ms + 1000 < since_ms {
                        continue;
                    }
                }
            }
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in contents.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if !OPERATION_KINDS.contains(&kind) {
                continue;
            }
            // Milliseconds, explicitly. A record without it is dropped rather
            // than assumed — see the module header.
            let Some(ts_ms) = v.get("ts_ms").and_then(|t| t.as_u64()) else {
                continue;
            };
            if ts_ms < since_ms {
                continue;
            }
            out.push(terminal_row(&v, sid, ts_ms));
        }
    }
    out
}

/// Browser actions, mapped onto the same shape.
///
/// `recent_actions` reads newest-first and caps, so it is asked for a generous
/// slice and the filter/sort happens here — the cap is a read optimisation, not
/// the timeline's ordering rule.
/// ONE browser action, mapped onto the SAME wire shape — the other half of the allowlist, and the half where a
/// missing key is easiest to miss because the browser has no session or sequence to leave a visible hole.
pub fn browser_row(a: &Value, ts_ms: u64) -> Value {
    json!({
        "source": "browser",
        "ts_ms": ts_ms,
        "session": Value::Null,
        "kind": "action",
        "script": a.get("script").cloned().unwrap_or(Value::Null),
        // NO `text` AND NO `status` ON THIS FEED, and that is the shape the panel already reads: its row parser asks
        // for both on EVERY row and gets `undefined` here, which its own default turns into null. Adding them would
        // be a behaviour change dressed as a refactor — the extraction in round 66 is exact.
        "exit_code": a.get("exit_code").cloned().unwrap_or(Value::Null),
        "duration_ms": a.get("duration_ms").cloned().unwrap_or(Value::Null),
        "timed_out": a.get("timed_out").cloned().unwrap_or(Value::Null),
        "screenshots": a.get("screenshots").cloned().unwrap_or(Value::Null),
        // Same allowlist rule as the terminal side: a browser action's run is dropped here unless it is named. Null
        // for actions from producers that carry no run.
        "run_id": a.get("run_id").cloned().unwrap_or(Value::Null),
    })
}

fn browser_actions(evidence_dir: &Path, since_ms: u64) -> Vec<Value> {
    crate::evidence::recent_actions(evidence_dir, OPERATION_READ_CAP)
        .into_iter()
        .filter_map(|a| {
            let ts_ms = a.get("ts_ms")?.as_u64()?;
            if ts_ms < since_ms {
                return None;
            }
            Some(browser_row(&a, ts_ms))
        })
        .collect()
}

/// How many browser actions to pull before filtering. Bounded so one request
/// cannot walk an arbitrarily long feed.
const OPERATION_READ_CAP: usize = 500;

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("vale-op-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let sess = base.join("sessions");
        let ev = base.join("pwout");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::create_dir_all(&ev).unwrap();
        (sess, ev)
    }

    /// Write a terminal session file with the given events.
    fn write_session(dir: &Path, sid: &str, events: &[Value]) {
        let body: String = events
            .iter()
            .map(|e| format!("{e}\n"))
            .collect::<Vec<_>>()
            .join("");
        std::fs::write(dir.join(format!("{sid}.jsonl")), body).unwrap();
    }

    /// THE POINT OF THE MODULE: both feeds come back as ONE ordered timeline.
    #[test]
    fn merges_both_feeds_in_time_order() {
        let (sess, ev) = dirs("merge");
        write_session(
            &sess,
            "term-a",
            &[
                json!({"seq":1,"ts_ms":1000,"kind":"command/start","command":"ls","intent":"look"}),
                json!({"seq":2,"ts_ms":3000,"kind":"command/end","exit_code":0,"duration_ms":50}),
            ],
        );
        crate::evidence::append_action_line(
            &ev,
            2000,
            &json!({"script":"mcp: browser_navigate url=http://x","exit_code":0}),
        );

        let tl = merged_operation(&sess, &ev, 0, 100);
        let shape: Vec<(i64, &str)> = tl
            .iter()
            .map(|e| (e["ts_ms"].as_i64().unwrap(), e["source"].as_str().unwrap()))
            .collect();
        assert_eq!(
            shape,
            vec![(1000, "terminal"), (2000, "browser"), (3000, "terminal")],
            "the browser action must land BETWEEN the two terminal events"
        );
        // The session is an ATTRIBUTE of terminal events, null for browser ones.
        assert_eq!(tl[0]["session"], "term-a");
        assert!(tl[1]["session"].is_null());
        // And the reasoning survives the merge.
        assert_eq!(tl[0]["intent"], "look");
    }

    /// THE TRAP: sorting on `ts` instead of `ts_ms`.
    ///
    /// The feeds stamp `ts` in different units. This asserts the merge is immune
    /// — a terminal event and a browser action written in the same instant must
    /// come back adjacent, which sorting on the raw `ts` could not achieve.
    #[test]
    fn a_browser_action_and_a_terminal_event_in_the_same_instant_stay_together() {
        let (sess, ev) = dirs("units");
        let same = 1_700_000_000_123u64;
        // Terminal: `ts` is SECONDS, derived from the same instant.
        write_session(
            &sess,
            "term-b",
            &[
                json!({"seq":1,"ts":same/1000,"ts_ms":same,"kind":"command/start","command":"deploy"}),
            ],
        );
        // Browser: the writer stamps both names in MILLISECONDS.
        crate::evidence::append_action_line(&ev, same, &json!({"script":"mcp: click"}));

        let tl = merged_operation(&sess, &ev, 0, 100);
        assert_eq!(tl.len(), 2);
        let stamps: Vec<u64> = tl.iter().map(|e| e["ts_ms"].as_u64().unwrap()).collect();
        assert_eq!(
            stamps,
            vec![same, same],
            "both records describe the SAME instant; a merge that sorted on the \
             raw `ts` would put the browser action ~50 years away"
        );
    }

    /// Transcript noise is excluded — this is what was DONE, not a dump.
    #[test]
    fn output_chunks_and_lifecycle_chatter_are_excluded() {
        let (sess, ev) = dirs("noise");
        write_session(
            &sess,
            "term-c",
            &[
                json!({"seq":1,"ts_ms":1000,"kind":"status","status":"opened"}),
                json!({"seq":2,"ts_ms":1100,"kind":"command/start","command":"ls"}),
                json!({"seq":3,"ts_ms":1200,"kind":"output","text":"a\nb\nc"}),
                json!({"seq":4,"ts_ms":1300,"kind":"command/end","exit_code":0}),
            ],
        );
        let tl = merged_operation(&sess, &ev, 0, 100);
        let kinds: Vec<&str> = tl.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["command/start", "command/end"]);
    }

    /// A record with no millisecond stamp is DROPPED, never assumed.
    ///
    /// Guessing a unit is the exact silent error this module exists to avoid, so
    /// the failure mode is a missing row rather than a wrong position.
    #[test]
    fn a_terminal_event_without_ts_ms_is_dropped_rather_than_assumed() {
        let (sess, ev) = dirs("noms");
        write_session(
            &sess,
            "term-d",
            &[
                // Older-format event: `ts` only (seconds).
                json!({"seq":1,"ts":1_700_000_000,"kind":"command/start","command":"old"}),
                json!({"seq":2,"ts":1_700_000_001,"ts_ms":1_700_000_001_000u64,"kind":"command/start","command":"new"}),
            ],
        );
        let tl = merged_operation(&sess, &ev, 0, 100);
        assert_eq!(
            tl.len(),
            1,
            "the unstamped record must not be placed by guess"
        );
        assert_eq!(tl[0]["command"], "new");
    }

    /// `since_ms` filters both feeds, and the limit keeps the NEWEST entries.
    #[test]
    fn since_filters_both_feeds_and_limit_keeps_the_newest() {
        let (sess, ev) = dirs("since");
        write_session(
            &sess,
            "term-e",
            &[
                json!({"seq":1,"ts_ms":1000,"kind":"command/start","command":"old"}),
                json!({"seq":2,"ts_ms":5000,"kind":"command/start","command":"new"}),
            ],
        );
        crate::evidence::append_action_line(&ev, 1000, &json!({"script":"old-browser"}));
        crate::evidence::append_action_line(&ev, 6000, &json!({"script":"new-browser"}));

        let tl = merged_operation(&sess, &ev, 4000, 100);
        let cmds: Vec<&str> = tl
            .iter()
            .map(|e| {
                e["command"]
                    .as_str()
                    .or_else(|| e["script"].as_str())
                    .unwrap()
            })
            .collect();
        assert_eq!(
            cmds,
            vec!["new", "new-browser"],
            "since_ms applies to BOTH feeds"
        );

        // Limit keeps the NEWEST, not the oldest.
        let tl = merged_operation(&sess, &ev, 0, 2);
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0]["ts_ms"].as_u64().unwrap(), 5000);
        assert_eq!(tl[1]["ts_ms"].as_u64().unwrap(), 6000);
    }

    /// Missing directories are an empty timeline, not an error: a device that has
    /// never opened a browser has no actions file, and that is normal.
    #[test]
    fn absent_directories_yield_an_empty_timeline() {
        let missing = std::path::Path::new("/nonexistent-vale-op-probe");
        assert!(merged_operation(missing, missing, 0, 10).is_empty());
    }

    /// THE ALLOWLIST TRAP, pinned on BOTH feeds.
    ///
    /// The two mappings below name every field explicitly, so a producer that
    /// starts writing a new one is silently dropped from the timeline with every
    /// existing test still green — the same "added on one side, reader never
    /// told" shape as the module map and the gateway's NOT_EXPOSED contract.
    /// THE RUNS WIRE FORMAT, pinned from this end (round 66) — the same contract the session row and the monitor
    /// row have.
    ///
    /// `agent/tests/fixtures/run-event.json` is read by this test and by the panel's `runs` test. The two feeds are
    /// separate ALLOWLISTS over one row shape, and the device's own comment says the risk out loud: "a field missing
    /// here is dropped silently with every other test still green". `run_id` got a test of its own first; this pins
    /// every field, on both feeds, and asserts that everything the PANEL reads is on the list for the feed it reads
    /// it from.
    #[test]
    fn run_event_fixture_matches_both_allowlists() {
        let fixture: Value = serde_json::from_str(include_str!("../tests/fixtures/run-event.json"))
            .expect("fixture parses");
        let list = |key: &str| -> Vec<String> {
            fixture[key]
                .as_array()
                .unwrap_or_else(|| panic!("{key} is a list"))
                .iter()
                .map(|v| v.as_str().expect("string").to_string())
                .collect()
        };
        let keys_of =
            |v: &Value| -> Vec<String> { v.as_object().expect("object").keys().cloned().collect() };

        let terminal = terminal_row(&fixture["example_terminal"], "term-1", 1789700000123);
        let browser = browser_row(&fixture["example_browser"], 1789700001500);

        // The row carries the RECORD's fields as the source record had them, which is the other half of the mapping:
        // an allowlist that reads a key the producer never writes sends nulls forever.
        assert_eq!(terminal["command"], "vale status");
        assert_eq!(terminal["plan_step"], 2);
        assert_eq!(terminal["run_id"], "run-1000-abc123");
        assert_eq!(browser["script"], "mcp: click");
        assert_eq!(browser["timed_out"], false);
        assert_eq!(
            browser["run_id"], "run-1000-abc123",
            "the browser half of the same run"
        );

        for (label, row, allowlist, required) in [
            (
                "terminal",
                &terminal,
                list("terminal_keys"),
                list("required_by_panel"),
            ),
            (
                "browser",
                &browser,
                list("browser_keys"),
                list("required_by_panel_browser"),
            ),
        ] {
            let mut keys = keys_of(row);
            let mut promised = allowlist.clone();
            keys.sort();
            promised.sort();
            assert_eq!(
                keys, promised,
                "the {label} row the device builds and the fixture promises have drifted apart"
            );
            for k in &required {
                assert!(
                    allowlist.contains(k),
                    "the panel reads `{k}` from a {label} row, which this feed does not carry"
                );
            }
        }
    }

    /// `run_id` is the first field to make that trip, so it is the one pinned.
    #[test]
    fn run_id_survives_the_merge_on_both_feeds() {
        let (sess, ev) = dirs("runid");
        write_session(
            &sess,
            "term-r",
            &[json!({
                "seq":1,"ts_ms":1000,"kind":"command/start","command":"deploy",
                "run_id":"run-1000-abc123"
            })],
        );
        crate::evidence::append_action_line(
            &ev,
            2000,
            &json!({"script":"mcp: click","run_id":"run-1000-abc123"}),
        );

        let tl = merged_operation(&sess, &ev, 0, 100);
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0]["run_id"], "run-1000-abc123", "terminal half");
        assert_eq!(tl[1]["run_id"], "run-1000-abc123", "browser half");
        // The WHOLE point: the two rows can now be grouped as one execution.
        assert_eq!(tl[0]["run_id"], tl[1]["run_id"]);
    }

    /// An event with no run is `null`, not absent and not a sentinel string.
    ///
    /// A reader must be able to tell "this was never attributed to a run" from
    /// "this was attributed to a run" — collapsing them would let an unattributed
    /// command be silently folded into whichever run happened to precede it,
    /// fabricating an attribution the data does not support.
    #[test]
    fn an_unattributed_event_carries_a_null_run_id_not_a_sentinel() {
        let (sess, ev) = dirs("norun");
        write_session(
            &sess,
            "term-n",
            &[json!({"seq":1,"ts_ms":1000,"kind":"command/start","command":"ls"})],
        );
        crate::evidence::append_action_line(&ev, 2000, &json!({"script":"mcp: click"}));

        for e in merged_operation(&sess, &ev, 0, 100) {
            assert!(
                e["run_id"].is_null(),
                "unattributed must be null, got {:?}",
                e["run_id"]
            );
        }
    }
}
