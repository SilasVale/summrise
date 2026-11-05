//! The operator-action index at `<data>/audit.jsonl`.
//!
//! One line per **authenticated** action: `{"ts_ms","kind","device_id?","status","detail?","tool?","token_id?","caller?"}`.
//! `kind` is a closed set — `enroll`, `register`, `grant`, `token`, `token_revoke`, `auth`, `mcp`,
//! `device` for a channel's own boundary, and `denied` for a refusal of a request whose caller
//! proved a credential. `status` is the HTTP status the caller saw (200 for a channel boundary,
//! which has no HTTP exchange), `detail` is a short reason: the console's wire error code for a
//! refusal, or the device's JSON-RPC error code when it answered with one. `tool` is the JSON-RPC
//! tool name for a `tools/call`, and only the name — except on a `device` line, where it names the
//! boundary (`device.joined` / `device.left`): the arguments are the operator's data and never reach
//! this file. `token_id` is the permanent token *record's* id when a token authenticated an MCP call
//! — never the `srm_` secret; the grant path, the operator's own `/mcp` route and every non-token
//! action write no field.
//!
//! `caller` is the **audience** of a proxied call, and it is not `token_id`: the operator session on
//! `/mcp` and the terminal stream are the console's own reads, `/mcp/:id` and `/mcp/all` are an AI
//! client's calls, and a grant on the standard route is a client with no token record at all. Only a
//! call the console proxied writes the field; an operator action (a grant, a token, a sign-in) has
//! no caller, and a line written before this field existed has none either — the page treats both
//! as "not a client's call", never as one. A channel boundary writes `console`: it is a fact the
//! console observed, and it must never be counted as a client's call.
//!
//! A request that fails authentication never reaches this file. Those refusals are an
//! unauthenticated caller's to generate at will, so they live in [`crate::denials`], rate-limited
//! and capped on their own; an unauthenticated flood therefore cannot rotate the audit history
//! away. Registration is the one proof outside an operator session: a registration that passes its
//! code and signature is an authenticated action, and a failed one is a denial.
//!
//! **No payload body is ever written.** The JSON-RPC request and response are the operator's data;
//! the index records that the exchange happened and whether it succeeded, never what it contained.
//! The one exception the design allows is the numeric error code, which is a fact about the exchange
//! and not its content.
//!
//! The file is capped at [`AUDIT_MAX_BYTES`] with one rotation to `audit.1.jsonl`; the discipline is
//! [`JsonLines`]'s.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::jsonl::JsonLines;

/// The largest the audit file may become before it is rotated to `audit.1.jsonl`.
pub(crate) const AUDIT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// How many of the audit's newest lines an absence may be derived from.
///
/// A device that has been away for hours leaves its two lines at the head of the file until its
/// neighbours push them back; 2,000 lines is days of one machine's traffic and hours of a busy
/// fleet's. The bound is the cost of `/fleet`: one read, one pass, no growing map.
pub(crate) const OUTAGE_WINDOW: usize = 2000;

/// The tool name a channel's close writes. A boundary is not a tool call: the name is the marker
/// the page recognises, and the line's `kind` is `device`, never `mcp`.
pub(crate) const DEVICE_LEFT: &str = "device.left";

/// The tool name a channel's open writes.
pub(crate) const DEVICE_JOINED: &str = "device.joined";

/// The current file's name under the data directory.
const AUDIT_FILE: &str = "audit.jsonl";

/// Who made a call the console proxied.
///
/// The route is the fact: `/mcp` is the operator's own page on the operator session, and
/// `/terminal/:device/:session/stream` reads for that page; `/mcp/:id` and `/mcp/all` are an AI
/// client that presented a grant or a token. It is deliberately not derived from `token_id`: a grant
/// on the standard route is a client with no token record, and the operator's `/mcp` carries none
/// either, so the token id alone cannot tell the two audiences apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Caller {
    /// The console itself: the operator page's `/mcp` calls, and the terminal stream's reads.
    Console,
    /// An AI client holding a grant or a permanent token.
    Client,
}

/// One audit line.
///
/// The absence of a body field is the design: the struct cannot carry one even by accident.
#[derive(Serialize)]
pub(crate) struct Entry<'a> {
    /// The console's clock when the action finished, in Unix milliseconds.
    pub(crate) ts_ms: u64,
    /// `enroll`, `register`, `grant`, `token`, `token_revoke`, `auth`, `mcp`, `device` or `denied`.
    pub(crate) kind: &'static str,
    /// The device the action names, in canonical lowercase hex, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) device_id: Option<&'a str>,
    /// The HTTP status the caller saw; `200` on a channel boundary, which has no caller.
    pub(crate) status: u16,
    /// Why, for a refusal or a device error: a short code, never a body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<&'a str>,
    /// The tool a `tools/call` named, or the boundary a `device` line names (`device.left` /
    /// `device.joined`). The name is an authorization fact; the arguments are a body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool: Option<&'a str>,
    /// The permanent token record that authenticated this call, by id — never the secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token_id: Option<&'a str>,
    /// The audience of a proxied call, or `console` on a channel boundary. Absent on operator
    /// actions and on lines older than the field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) caller: Option<Caller>,
}

/// One absence the audit proves: the moment the device's channel came back, and how long it was
/// gone. The two names are the two a fleet row compares — the duration and the return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Absence {
    pub(crate) away_ms: u64,
    pub(crate) back_at_ms: u64,
}

/// The append-only audit index; the denial log reuses [`Entry`] as its line shape.
///
/// Cheap to clone: the console's `Inner` and the channel registry hold the same file and the same
/// live stream, so a channel boundary is appended by the process that saw it, not queued through a
/// second writer.
#[derive(Clone)]
pub(crate) struct Audit {
    file: std::sync::Arc<JsonLines>,
    /// Every line as it is appended, for the live activity stream.
    ///
    /// A broadcast and not a queue: a page that falls behind is told it lagged and asks for the file's
    /// newest lines again, instead of the console holding a per-subscriber backlog nobody bounded.
    live: tokio::sync::broadcast::Sender<Value>,
}

/// How many appended lines the live stream may carry before a slow subscriber is told it lagged.
pub(crate) const LIVE_AUDIT_CAPACITY: usize = 256;

impl Audit {
    /// The audit file under `data`, capped at `max_bytes`.
    pub(crate) fn new(data: &Path, max_bytes: u64) -> Audit {
        let (live, _) = tokio::sync::broadcast::channel(LIVE_AUDIT_CAPACITY);
        Audit {
            file: std::sync::Arc::new(JsonLines::new(data.join(AUDIT_FILE), max_bytes)),
            live,
        }
    }

    /// Append one line.
    ///
    /// The live stream is published by the caller, not here: the sentence a person reads needs the
    /// call's arguments, which this file must never hold. [`Audit::publish`] is the one door.
    pub(crate) async fn append(&self, entry: &Entry<'_>) -> bool {
        self.file.append(entry).await
    }

    /// Publish one live event (already humanised) to every open `/events`. No subscriber is fine: the
    /// stream is for whoever is watching, and nobody watching is not an error.
    pub(crate) fn publish(&self, value: Value) {
        let _ = self.live.send(value);
    }

    /// Follow the audit as it is written. Subscribe BEFORE reading the newest lines: a line written
    /// between the two would otherwise be in neither the snapshot nor the stream.
    pub(crate) fn follow(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.live.subscribe()
    }

    /// The newest `limit` lines, newest first.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.file.newest(limit).await
    }

    /// The newest `limit` lines that name `device`, newest first.
    ///
    /// The filter is applied during the read, not to [`Audit::newest`]'s window: another machine's
    /// newer traffic must not shorten this machine's day.
    pub(crate) async fn newest_for(&self, device: &str, limit: usize) -> Vec<Value> {
        self.file
            .newest_matching(limit, |line| {
                line.get("device_id").and_then(Value::as_str) == Some(device)
            })
            .await
    }

    /// Every device's newest completed absence, from the newest [`OUTAGE_WINDOW`] lines.
    ///
    /// The file is the record, not this process's memory: a console restarted while a device was
    /// away reads the close the dead process wrote and the open this one wrote, and the pair is the
    /// interval between them. The read is bounded and the map is built after it returns, so the
    /// file's writer is not held while a fleet response is serialised.
    pub(crate) async fn last_outages(&self) -> HashMap<String, Absence> {
        outages_from(&self.file.newest(OUTAGE_WINDOW).await)
    }
}

/// The completed absences in `lines`, newest line first.
///
/// A pair is a `device.joined` and the `device.left` immediately before it: the newest such join is
/// the return the fleet row reports. A join with another join before it is not an absence — the
/// channel was replaced while the device stayed up — and a left with no join after it is a device
/// that is still away. Ties are ordered by the line's position in the file, so a left appended after
/// a join it happened before (the two channel tasks race the writer) still pairs with that join.
fn outages_from(lines: &[Value]) -> HashMap<String, Absence> {
    // The boundary events per device: (ts_ms, joined, index in `lines`, newest first).
    let mut events: HashMap<&str, Vec<(u64, bool, usize)>> = HashMap::new();
    for (order, line) in lines.iter().enumerate() {
        let Some(device) = line.get("device_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(ts_ms) = line.get("ts_ms").and_then(Value::as_u64) else {
            continue;
        };
        let joined = match line.get("tool").and_then(Value::as_str) {
            Some(DEVICE_JOINED) => true,
            Some(DEVICE_LEFT) => false,
            _ => continue,
        };
        events
            .entry(device)
            .or_default()
            .push((ts_ms, joined, order));
    }

    let mut absences = HashMap::new();
    for (device, mut events) in events {
        // Oldest first. At one timestamp the later line in the file is the later event: `order` is
        // newest-first, so a smaller index is newer.
        events.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.2.cmp(&left.2)));
        let Some(newest) = events.iter().rposition(|event| event.1) else {
            continue;
        };
        if newest == 0 {
            continue;
        }
        let (joined_ts, _, _) = events[newest];
        let (left_ts, joined_before, _) = events[newest - 1];
        if joined_before {
            continue;
        }
        absences.insert(
            device.to_owned(),
            Absence {
                away_ms: joined_ts.saturating_sub(left_ts),
                back_at_ms: joined_ts,
            },
        );
    }
    absences
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "abababababababababababababababababababababababababababababababab";

    /// One boundary line, exactly as the channel registry writes it.
    fn boundary(device: &str, event: &str, ts_ms: u64) -> Value {
        serde_json::json!({
            "ts_ms": ts_ms,
            "kind": "device",
            "device_id": device,
            "status": 200,
            "tool": event,
            "caller": "console",
        })
    }

    /// A directory this test owns, removed with it.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "summrise-audit-unit-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the test directory is creatable");
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Write lines oldest first, as the writer appends them.
    fn write_audit(dir: &Path, lines: &[Value]) {
        let mut text = String::new();
        for line in lines {
            text.push_str(&line.to_string());
            text.push('\n');
        }
        std::fs::write(dir.join(AUDIT_FILE), text).expect("the audit file is writable");
    }

    #[test]
    fn a_left_and_the_joined_after_it_are_one_absence() {
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 1_000),
            boundary(DEVICE, DEVICE_LEFT, 400),
        ];
        assert_eq!(
            outages_from(&lines).get(DEVICE),
            Some(&Absence {
                away_ms: 600,
                back_at_ms: 1_000,
            })
        );
    }

    #[test]
    fn a_left_with_no_joined_is_still_away_and_proves_nothing() {
        let lines = vec![boundary(DEVICE, DEVICE_LEFT, 400)];
        assert!(outages_from(&lines).is_empty());
    }

    #[test]
    fn a_join_that_replaced_a_live_channel_is_not_an_absence() {
        // Two joins and no left between: the device was up the whole time.
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 2_000),
            boundary(DEVICE, DEVICE_JOINED, 1_000),
        ];
        assert!(outages_from(&lines).is_empty());
    }

    #[test]
    fn the_newest_completed_pair_is_the_one_reported() {
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 3_000),
            boundary(DEVICE, DEVICE_LEFT, 2_000),
            boundary(DEVICE, DEVICE_JOINED, 1_000),
            boundary(DEVICE, DEVICE_LEFT, 500),
        ];
        assert_eq!(
            outages_from(&lines).get(DEVICE),
            Some(&Absence {
                away_ms: 1_000,
                back_at_ms: 3_000,
            })
        );
    }

    #[test]
    fn a_left_written_after_the_join_it_precedes_still_pairs() {
        // The close's append and the reopen's race the writer; the timestamps are the ordering, so a
        // joined line above the left it followed is still one absence.
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 1_000),
            boundary(DEVICE, DEVICE_LEFT, 400),
        ];
        assert_eq!(
            outages_from(&lines).get(DEVICE),
            Some(&Absence {
                away_ms: 600,
                back_at_ms: 1_000,
            })
        );
    }

    #[test]
    fn two_devices_do_not_borrow_each_others_boundaries() {
        let other = "cd".repeat(32);
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 900),
            boundary(&other, DEVICE_JOINED, 500),
            boundary(&other, DEVICE_LEFT, 400),
        ];
        let absences = outages_from(&lines);
        assert_eq!(
            absences.get(&other),
            Some(&Absence {
                away_ms: 100,
                back_at_ms: 500,
            })
        );
        assert_eq!(
            absences.get(DEVICE),
            None,
            "this device has no left in the window"
        );
    }

    #[tokio::test]
    async fn only_the_newest_window_of_lines_is_consulted() {
        let dir = TempDir::new("window");
        let mut lines = vec![
            boundary(DEVICE, DEVICE_LEFT, 100),
            boundary(DEVICE, DEVICE_JOINED, 200),
        ];
        // Push the pair behind the window: a newer line for another device is still a line.
        for index in 0..OUTAGE_WINDOW {
            lines.push(serde_json::json!({
                "ts_ms": 1_000 + index as u64,
                "kind": "mcp",
                "device_id": "cd".repeat(32),
                "status": 200,
                "tool": "files.read",
            }));
        }
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);
        assert!(
            audit.last_outages().await.is_empty(),
            "a pair older than the newest {OUTAGE_WINDOW} lines is not read"
        );

        // Leave the pair exactly inside the window: 2 boundary lines and 1,998 fillers are 2,000.
        write_audit(&dir.0, &lines[..OUTAGE_WINDOW]);
        assert_eq!(
            audit.last_outages().await.get(DEVICE),
            Some(&Absence {
                away_ms: 100,
                back_at_ms: 200,
            })
        );
    }
}
