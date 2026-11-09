//! The operator-action index at `<data>/audit.jsonl`.
//!
//! One line per **authenticated** action: `{"ts_ms","kind","device_id?","status","detail?","tool?","token_id?","caller?"}`.
//! `kind` is a closed set — `enroll`, `register`, `grant`, `token`, `token_revoke`, `auth`, `mcp`,
//! `device` for a channel's own boundary, `poll` for a failed read the console's own poller made,
//! and `denied` for a refusal of a request whose caller proved a credential. A successful poll is
//! not a line at all: it is a read of the same screen the console already shows, and writing it
//! would make the record mostly the console talking to itself. A failed poll stays a line so the
//! operator can see it in the Activity Table, but `poll` is not the AI's work: it is counted apart
//! and excluded from every `failed` count. `status` is the console's outcome code: the HTTP status a
//! refusal produced, `200` for an answer that crossed as a success (a channel boundary, a JSON-RPC
//! error envelope), and `424` for a tool that answered with its own failure (`isError`, or a run
//! that exited non-zero) — the record's verdict, not always the code the caller's HTTP exchange
//! carried.
//! `detail` is a short reason: the console's wire error code for a refusal, or the device's
//! JSON-RPC error code when it answered with one, or a bounded tool reason. `tool` is the JSON-RPC
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

/// How many of the audit's newest lines `/fleet` reads: the absences and each machine's day both
/// come from this one window.
///
/// A device that has been away for hours leaves its two lines at the head of the file until its
/// neighbours push them back; 2,000 lines is days of one machine's traffic and hours of a busy
/// fleet's. The bound is the cost of `/fleet`: one read, one pass, no growing map.
pub(crate) const FLEET_WINDOW: usize = 2000;

/// How many lines the calendar-day read examines before it gives up and says so.
///
/// The measured record holds 28,056 lines over 14 days and 2,017 from one local midnight to 21:23,
/// so the bound is more than a day and a night of the busiest fleet observed. It exists because a
/// day read runs on every `/fleet` poll: 50,000 lines is the most one read may cost, and a file that
/// reaches it reports `bounded` rather than passing a partial day off as the whole day.
pub(crate) const DAY_SCAN_LIMIT: usize = 50_000;

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
    /// `enroll`, `register`, `grant`, `token`, `token_revoke`, `auth`, `mcp`, `device`, `poll`
    /// or `denied`. A `poll` line is a failed console poll; a successful one is not written.
    pub(crate) kind: &'static str,
    /// The device the action names, in canonical lowercase hex, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) device_id: Option<&'a str>,
    /// The console's outcome code: the status a refusal produced, `200` for an answer, or `424`
    /// for a tool-level failure that MCP carried inside a 200 result.
    pub(crate) status: u16,
    /// Why, for a refusal, a device error or a tool-level failure: a short reason, never a body.
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

/// What the newest audit window proves about devices' channel boundaries.
///
/// `last_outages` is the absence that ended; `last_lefts` is the silence that has not. The two are
/// read from the same window in one file read, so a fleet response cannot pair a return from one
/// generation with a close from another. `activity` is the same window's join/leave tally, so a
/// count and the absence it sits beside cannot come from two different reads either.
#[derive(Debug, Default)]
pub(crate) struct Boundaries {
    /// Each device's newest completed absence.
    pub(crate) last_outages: HashMap<String, Absence>,
    /// Each device's newest `device.left` that no `device.joined` has closed: the start of the
    /// silence it is still in. Omitted for a device whose newest boundary is a join.
    pub(crate) last_lefts: HashMap<String, u64>,
    /// How many joins and leaves each device had in this window.
    pub(crate) activity: HashMap<String, ChannelActivity>,
}

/// How many channel boundaries one device had in the fleet's newest window.
///
/// `joined` counts every `device.joined` and `left` every `device.left` in the same lines the
/// absence pair is derived from: the console's own record of how often it accepted the channel and
/// how often it wrote the close. A retry the console accepted is a join even when no left was
/// written (the live socket was replaced), so the two numbers are a tally of boundaries, not a
/// drop count. A device with no boundary in the window is absent from the map, never a zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChannelActivity {
    pub(crate) joined: u64,
    pub(crate) left: u64,
}

/// What one read of the newest audit window proves about the fleet: the channel boundaries.
///
/// One read serves every row, so two rows cannot pair boundaries from two different generations of
/// the file, and `/fleet` pays one window read whatever the fleet's size. The day is a separate
/// read ([`Audit::day`]): it starts at midnight, not at this window's oldest line.
#[derive(Debug, Default)]
pub(crate) struct FleetWindow {
    pub(crate) boundaries: Boundaries,
    /// How many lines the read returned, and the moments their two ends carry.
    pub(crate) read: WindowFacts,
}

/// What one window read held: its length and the span between its newest and oldest moments.
///
/// **THE COUNT IS THE READ'S OWN LENGTH, NEVER THE CAP.** [`FLEET_WINDOW`] is how many lines the
/// read asks for, not necessarily how many it returns: a file shorter than the cap is a smaller
/// window, and serving the cap beside a tally taken from fewer lines made the sentence's window
/// disagree with its own trace — the measured page said 2,000 lines beside a 1,000-line window
/// (task 76). The span is `newest_ts_ms - oldest_ts_ms` over the same lines, so a join/leave tally
/// can be read as a rate; a window whose newest or oldest line carries no `ts_ms` has no span to
/// state, and the page then says only the lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowFacts {
    /// The lines the read returned, at most [`FLEET_WINDOW`].
    pub(crate) lines: usize,
    /// The newest line's moment minus the oldest line's, when both carry one.
    pub(crate) span_ms: Option<u64>,
}

impl WindowFacts {
    /// The facts of one newest-first window read.
    fn of(lines: &[Value]) -> WindowFacts {
        let moment = |line: &Value| line.get("ts_ms").and_then(Value::as_u64);
        let newest = lines.first().and_then(moment);
        let oldest = lines.last().and_then(moment);
        WindowFacts {
            lines: lines.len(),
            span_ms: newest
                .zip(oldest)
                .map(|(newest, oldest)| newest.saturating_sub(oldest)),
        }
    }
}

/// What one calendar day holds, split by who can be proven to have made each line.
///
/// The four buckets and their failures are the page's: client calls, the console's own reads, failed
/// polls, and the lines no caller was recorded for. `client_calls` and `failed` are the two `/fleet`
/// serves; the rest are Activity's sentence, counted here once so the page and the fleet cannot use
/// two rules for the same day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DayBuckets {
    /// Lines a client made: `caller: "client"`, or a token with no caller.
    pub(crate) client_calls: u64,
    /// The client calls above whose exchange failed (status 400+, `ok: false`, or a JSON-RPC error).
    pub(crate) failed: u64,
    /// The console's own reads: `caller: "console"` and not a failed poll.
    pub(crate) console: u64,
    /// The console's reads that failed.
    pub(crate) console_failed: u64,
    /// Failed console polls: rows the table draws, no failure count includes them.
    pub(crate) poll: u64,
    /// The failed polls above (every poll in the audit is a failure: a successful one is not written).
    pub(crate) poll_failed: u64,
    /// Real lines with no caller and no token to prove a client made them.
    pub(crate) unattributed: u64,
    /// The unattributed lines that failed.
    pub(crate) unattributed_failed: u64,
}

/// One read of the audit for a local calendar day.
///
/// The numbers come from the day, not from the newest `FLEET_WINDOW` lines: a count that is derived
/// from a sliding window shrinks while the day is still running, because the window's oldest line is
/// newer than midnight (measured on DESKTOP-14RJCR8: `554 calls · 53 failed`, then half an hour later
/// `529 calls · 53 failed`). Starting at the day's own midnight fixes that rule.
///
/// **A COMPLETE DAY READ CANNOT DECREASE WHILE THE DAY RUNS.** New lines are appended after the ones
/// already counted and are at or after midnight, so the scan sees at least everything the previous
/// scan saw; only a rotation takes the day's earlier lines out of `audit.jsonl`, and that is reported
/// as `since` rather than served as a whole day. The line bound is the one other way the read can be
/// partial, and it says `bounded` when it happens.
#[derive(Debug, Default)]
pub(crate) struct DayRead {
    /// **THE MIDNIGHT THIS READ CUT AT, IN UNIX MILLISECONDS (the forty-fourth list).** The page's
    /// timeline groups its rows into days at the BROWSER's midnight and this read cuts at the CONSOLE
    /// HOST's, so the two agree only when both are the same zone — true of the operator's own machine
    /// and false for any other browser. The answer publishes the moment it used, so the page can cut
    /// its labels at the same one instead of inferring it from the reader's clock.
    pub(crate) day_start_ms: u64,
    /// The whole file's day, including lines that name no device (operator actions).
    pub(crate) all: DayBuckets,
    /// Each device's day. A device appears only when the day holds a line naming it.
    pub(crate) devices: HashMap<String, DayBuckets>,
    /// The local time the read's coverage starts at, when midnight is older than the file (a
    /// rotation, or a console that started after midnight). `None` means the scan reached midnight
    /// and the day is whole.
    pub(crate) since_ms: Option<u64>,
    /// The scan hit [`DAY_SCAN_LIMIT`] before midnight: the counts are the newest lines only.
    pub(crate) bounded: bool,
    /// Lines the scan examined; the read's cost, reported rather than guessed.
    pub(crate) lines_read: usize,
}

/// One read of the audit for a chosen time range, `[from_ms, to_ms)`.
///
/// The range is a slice of the **current file**, read by the day's backwards scan: newest first,
/// stopping at the first line older than `from_ms`, bounded at [`DAY_SCAN_LIMIT`]. A range that
/// starts before the current file does is partial, and a scan the line bound stopped before `from_ms`
/// is partial too; both are stated rather than padded.
#[derive(Debug, Default)]
pub(crate) struct RangeRead {
    /// The lines whose moment falls in `[from_ms, to_ms)`, newest first.
    pub(crate) lines: Vec<Value>,
    /// The moment the read's coverage starts at, when the current file starts after `from_ms` (a
    /// rotation, or a console that started later). `None` means the scan proved the range's start.
    pub(crate) since_ms: Option<u64>,
    /// The scan hit [`DAY_SCAN_LIMIT`] before reaching `from_ms`: the answer is the file's newest
    /// lines only.
    pub(crate) bounded: bool,
    /// Lines the scan examined; the read's cost, reported rather than guessed.
    pub(crate) lines_read: usize,
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

    /// The file this audit reads, its size and the moments its lines span, for the page's one
    /// source line.
    ///
    /// A fact about the reader, not about a window or a day: `/audit` answers lines and
    /// `/audit/day` answers a day, and neither can carry "which file is this" without making every
    /// answer about both.
    pub(crate) async fn source(&self) -> crate::jsonl::SourceFacts {
        self.file.source().await
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

    /// The channel boundaries every fleet row's absence reads, from ONE read of the newest
    /// [`FLEET_WINDOW`] lines, plus the lines and span that read actually held.
    ///
    /// The file is the record, not this process's memory: a console restarted while a device was
    /// away reads the close the dead process wrote and the open this one wrote, and the pair is the
    /// interval between them. The closing line also names the silence a still-away device is in,
    /// which is the newest evidence an offline row can be aged from. Both maps are built after the
    /// read returns, so the file's writer is not held while a fleet response is serialised.
    pub(crate) async fn fleet_window(&self) -> FleetWindow {
        let lines = self.file.newest(FLEET_WINDOW).await;
        FleetWindow {
            boundaries: boundaries_of(&lines),
            read: WindowFacts::of(&lines),
        }
    }

    /// The lines whose moment falls in `[from_ms, to_ms)`, newest first.
    ///
    /// The scan is [`Audit::day`]'s: the current file backwards, stopping at the first line older
    /// than `from_ms`, bounded by [`DAY_SCAN_LIMIT`]. A line exactly at `from_ms` belongs to the
    /// range; a line exactly at `to_ms` does not. The device filter is the caller's, applied after
    /// this read, exactly as `/audit/day?device=` selects a machine from the day it read.
    ///
    /// The current file only, deliberately: a range that starts before it is partial, and
    /// `since_ms` says where coverage starts instead of stitching the rotated generation into a
    /// whole the page cannot verify.
    pub(crate) async fn range(&self, from_ms: u64, to_ms: u64) -> RangeRead {
        self.range_with_limit(from_ms, to_ms, DAY_SCAN_LIMIT).await
    }

    /// [`Audit::range`] with the line bound injected, so a test can observe the bound without
    /// writing 50,000 lines.
    pub(crate) async fn range_with_limit(
        &self,
        from_ms: u64,
        to_ms: u64,
        limit: usize,
    ) -> RangeRead {
        let scan = self.file.newest_since(from_ms, limit).await;
        // The day read's rule, stated once: a scan that ran out of file or hit its bound never saw
        // a line older than `from_ms`, so its oldest line is where the coverage starts.
        let since_ms = if scan.exhausted || scan.bounded {
            scan.oldest_ts_ms.filter(|oldest| *oldest > from_ms)
        } else {
            None
        };
        RangeRead {
            lines: scan
                .lines
                .into_iter()
                // `to_ms` is exclusive. A line with no moment cannot be proven inside any range,
                // and is not served as if it were.
                .filter(|line| {
                    line.get("ts_ms")
                        .and_then(Value::as_u64)
                        .is_some_and(|ts_ms| ts_ms < to_ms)
                })
                .collect(),
            since_ms,
            bounded: scan.bounded,
            lines_read: scan.lines_read,
        }
    }

    /// The local calendar day's counts, from a backwards scan that stops at local midnight.
    ///
    /// This is the count `/fleet` and the hub's day sentence use: the day is the day, not whichever
    /// slice of it the newest window happens to hold. The scan is bounded by [`DAY_SCAN_LIMIT`] and
    /// by no I/O beyond the one file read every other audit reader pays.
    pub(crate) async fn day(&self, now_ms: u64) -> DayRead {
        self.day_with_limit(now_ms, DAY_SCAN_LIMIT).await
    }

    /// [`Audit::day`] with the line bound injected, so a test can observe the bound without writing
    /// 50,000 lines.
    pub(crate) async fn day_with_limit(&self, now_ms: u64, limit: usize) -> DayRead {
        let midnight_ms = local_day_start_ms(now_ms);
        let scan = self.file.newest_since(midnight_ms, limit).await;
        let mut all = DayBuckets::default();
        let mut devices: HashMap<String, DayBuckets> = HashMap::new();
        for line in &scan.lines {
            add_to_day(&mut all, line);
            if let Some(device) = line.get("device_id").and_then(Value::as_str) {
                add_to_day(devices.entry(device.to_owned()).or_default(), line);
            }
        }
        // A scan that ran out of file or hit its bound never saw a line older than midnight; its
        // oldest line is where the count starts, and the page says so instead of calling it the
        // whole day. A file whose first line is exactly midnight covers the whole day: there is no
        // today line it could have missed.
        let since_ms = if scan.exhausted || scan.bounded {
            scan.oldest_ts_ms.filter(|oldest| *oldest > midnight_ms)
        } else {
            None
        };
        DayRead {
            day_start_ms: midnight_ms,
            all,
            devices,
            since_ms,
            bounded: scan.bounded,
            lines_read: scan.lines_read,
        }
    }
}

/// The console host's local midnight at or before `now_ms`, in Unix milliseconds.
///
/// The page's timeline cuts at the browser's local midnight and the console runs in the operator's
/// timezone (the measured host and browser are both Asia/Shanghai), so the server's local day is the
/// same cut. A DST transition can make a midnight nonexistent; `earliest` takes the first instant of
/// the day that exists rather than inventing one.
pub(crate) fn local_day_start_ms(now_ms: u64) -> u64 {
    use chrono::{Local, TimeZone};

    let Some(now) = Local.timestamp_millis_opt(now_ms as i64).single() else {
        return 0;
    };
    let Some(midnight) = now.date_naive().and_hms_opt(0, 0, 0) else {
        return 0;
    };
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|midnight| midnight.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

/// Both boundary maps from one window of lines, so the two derivations share their read.
fn boundaries_of(lines: &[Value]) -> Boundaries {
    Boundaries {
        last_outages: outages_from(lines),
        last_lefts: open_lefts_from(lines),
        activity: activity_from(lines),
    }
}

/// Each device's join/leave tally in `lines`.
///
/// [`boundary_events`] is the same parse the absences use, so a count cannot include a line the
/// absence read would not; a device with no boundary is absent from the map rather than present
/// with a zero the window cannot prove.
fn activity_from(lines: &[Value]) -> HashMap<String, ChannelActivity> {
    let mut activity = HashMap::new();
    for (device, events) in boundary_events(lines) {
        let mut counts = ChannelActivity::default();
        for (_, joined, _) in events {
            if joined {
                counts.joined += 1;
            } else {
                counts.left += 1;
            }
        }
        activity.insert(device.to_owned(), counts);
    }
    activity
}

/// The boundary events per device: `(ts_ms, joined, index in lines)`, newest first.
///
/// The shared first pass of the two derivations: both ask which boundary a device's window ends
/// with, and one parse keeps their tie rule identical.
fn boundary_events(lines: &[Value]) -> HashMap<&str, Vec<(u64, bool, usize)>> {
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
    events
}

/// Oldest first. At one timestamp the later line in the file is the later event: `order` is
/// newest-first, so a smaller index is newer.
fn chronological(events: &mut [(u64, bool, usize)]) {
    events.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.2.cmp(&left.2)));
}

/// The completed absences in `lines`, newest line first.
///
/// A pair is a `device.joined` and the `device.left` immediately before it: the newest such join is
/// the return the fleet row reports. A join with another join before it is not an absence — the
/// channel was replaced while the device stayed up — and a left with no join after it is a device
/// that is still away. Ties are ordered by the line's position in the file, so a left appended after
/// a join it happened before (the two channel tasks race the writer) still pairs with that join.
fn outages_from(lines: &[Value]) -> HashMap<String, Absence> {
    let mut absences = HashMap::new();
    for (device, mut events) in boundary_events(lines) {
        chronological(&mut events);
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

/// Each device's newest `device.left` that no `device.joined` after it closed: the start of the
/// silence it is still in.
///
/// A window whose newest boundary for a device is a join has no current silence — the close was
/// answered — and is omitted rather than serving the older left as if it were current. A device
/// with no boundary at all is omitted too.
fn open_lefts_from(lines: &[Value]) -> HashMap<String, u64> {
    let mut lefts = HashMap::new();
    for (device, mut events) in boundary_events(lines) {
        chronological(&mut events);
        if let Some((ts_ms, joined, _)) = events.last() {
            if !joined {
                lefts.insert(device.to_owned(), *ts_ms);
            }
        }
    }
    lefts
}

/// Whether a line is one of the console's channel boundaries rather than a call.
fn is_channel_boundary(line: &Value) -> bool {
    matches!(
        line.get("tool").and_then(Value::as_str),
        Some(DEVICE_JOINED | DEVICE_LEFT)
    )
}

/// Whether a line is a client's call, by the same proof the hub's day line and Activity's header
/// use.
///
/// `caller: "client"` is the console's own statement that an AI client made the call; a line with
/// no caller but a token is a client call from before the field existed (the live window at the
/// time of the fix held 908 caller-less lines, 376 of them with a token); `caller: "console"` is
/// never one, even if a token were to appear on it. A channel boundary is a fact about the machine
/// and a failed poll is the console retrying itself — neither is work.
fn is_client_call(line: &Value) -> bool {
    if is_channel_boundary(line) {
        return false;
    }
    if line.get("kind").and_then(Value::as_str) == Some("poll") {
        return false;
    }
    match line.get("caller").and_then(Value::as_str) {
        Some("client") => true,
        Some("console") => false,
        _ => line.get("token_id").and_then(Value::as_str).is_some(),
    }
}

/// Whether the exchange failed, by the page's rule: a status of 400 or above, the live stream's
/// explicit `ok: false` (a device's JSON-RPC error is carried inside an HTTP 200), or a JSON-RPC
/// error's detail on a stored line. The two shapes must count the same way.
fn day_failed(line: &Value) -> bool {
    line.get("status")
        .and_then(Value::as_u64)
        .is_some_and(|status| status >= 400)
        || line.get("ok") == Some(&Value::Bool(false))
        || line
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.starts_with("jsonrpc error"))
}

/// Add one audit line to one day's buckets, by the page's rule.
///
/// The order is the page's: a channel boundary is not a call at all; a failed poll is its own bucket
/// and no failure count includes it; `caller: "client"` or a token proves a client; `caller:
/// "console"` is the console's own read; everything else is a real line whose caller was never
/// recorded. A failure is [`day_failed`]'s rule, the same one `/fleet` and the timeline use.
fn add_to_day(day: &mut DayBuckets, line: &Value) {
    if is_channel_boundary(line) {
        return;
    }
    let failed = day_failed(line);
    if line.get("kind").and_then(Value::as_str) == Some("poll") {
        day.poll += 1;
        day.poll_failed += u64::from(failed);
    } else if is_client_call(line) {
        day.client_calls += 1;
        day.failed += u64::from(failed);
    } else if line.get("caller").and_then(Value::as_str) == Some("console") {
        day.console += 1;
        day.console_failed += u64::from(failed);
    } else {
        day.unattributed += 1;
        day.unattributed_failed += u64::from(failed);
    }
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
    fn the_silence_start_is_the_newest_left_that_no_join_closed() {
        // A window with no boundary proves no silence.
        assert!(open_lefts_from(&[]).is_empty());
        // The newest close still open is the start `/fleet` uses as `last_left_ms`.
        assert_eq!(
            open_lefts_from(&[boundary(DEVICE, DEVICE_LEFT, 400)]).get(DEVICE),
            Some(&400)
        );
        // A join after the close ended that silence, so the close is not the current one.
        assert!(open_lefts_from(&[
            boundary(DEVICE, DEVICE_JOINED, 500),
            boundary(DEVICE, DEVICE_LEFT, 400),
        ])
        .is_empty());
        // Two open closes: the newest is the one a page ages.
        assert_eq!(
            open_lefts_from(&[
                boundary(DEVICE, DEVICE_LEFT, 400),
                boundary(DEVICE, DEVICE_LEFT, 100),
            ])
            .get(DEVICE),
            Some(&400)
        );
        // A close with a join, then another close: only the last one is still open.
        assert_eq!(
            open_lefts_from(&[
                boundary(DEVICE, DEVICE_LEFT, 900),
                boundary(DEVICE, DEVICE_JOINED, 800),
                boundary(DEVICE, DEVICE_LEFT, 400),
            ])
            .get(DEVICE),
            Some(&900)
        );
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

    #[test]
    fn the_window_counts_each_devices_joins_and_leaves() {
        // The counted lines are the same `device.joined` / `device.left` rows the outages are
        // derived from: a retry that was accepted is a join even when no left was written, so the
        // tally is the console's own record of its attempts.
        let other = "cd".repeat(32);
        let lines = vec![
            boundary(DEVICE, DEVICE_LEFT, 900),
            boundary(&other, DEVICE_JOINED, 800),
            boundary(DEVICE, DEVICE_JOINED, 700),
            boundary(DEVICE, DEVICE_JOINED, 600),
            boundary(DEVICE, DEVICE_LEFT, 500),
            boundary(DEVICE, DEVICE_JOINED, 400),
            serde_json::json!({
                "ts_ms": 300,
                "kind": "mcp",
                "device_id": DEVICE,
                "tool": "runs.exec",
                "status": 200,
            }),
        ];

        let activity = activity_from(&lines);
        assert_eq!(
            activity.get(DEVICE),
            Some(&ChannelActivity { joined: 3, left: 2 })
        );
        assert_eq!(
            activity.get(&other),
            Some(&ChannelActivity { joined: 1, left: 0 })
        );
        assert_eq!(
            activity.len(),
            2,
            "a call is not a channel boundary: {activity:?}"
        );
    }

    #[test]
    fn the_window_reports_the_lines_it_read_and_the_moments_they_span() {
        // The window's count is its own length, never the `FLEET_WINDOW` cap: a short file was
        // being called 2,000 lines beside a 1,000-line trace (task 76). The span is the distance
        // between the newest and oldest moments of the same lines, so the tally can be read as a
        // rate.
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 10_000),
            serde_json::json!({
                "ts_ms": 6_000,
                "kind": "mcp",
                "device_id": DEVICE,
                "status": 200,
            }),
            boundary(DEVICE, DEVICE_LEFT, 2_000),
        ];
        assert_eq!(
            WindowFacts::of(&lines),
            WindowFacts {
                lines: 3,
                span_ms: Some(8_000),
            },
            "the count is the window's length and the span its oldest-to-newest distance"
        );
    }

    #[test]
    fn a_window_whose_ends_carry_no_moment_has_no_span() {
        // A line without `ts_ms` (written before the field, or synthetic) cannot date the window.
        // The count stands; the span is absent rather than invented.
        let lines = vec![
            boundary(DEVICE, DEVICE_JOINED, 10_000),
            serde_json::json!({
                "kind": "mcp",
                "device_id": DEVICE,
                "status": 200,
            }),
        ];
        assert_eq!(
            WindowFacts::of(&lines),
            WindowFacts {
                lines: 2,
                span_ms: None,
            },
            "one undated end is not a duration"
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
        for index in 0..FLEET_WINDOW {
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
            audit
                .fleet_window()
                .await
                .boundaries
                .last_outages
                .is_empty(),
            "a pair older than the newest {FLEET_WINDOW} lines is not read"
        );
        assert!(
            audit.fleet_window().await.boundaries.last_lefts.is_empty(),
            "a close older than the newest {FLEET_WINDOW} lines is not read either"
        );
        let capped = audit.fleet_window().await;
        assert_eq!(
            capped.read.lines, FLEET_WINDOW,
            "a file longer than the cap reports the lines the read returned"
        );
        assert_eq!(
            capped.read.span_ms,
            Some(1_999),
            "the read's 2,000 newest lines run from 1,000 to 2,999 ms"
        );

        // Leave the pair exactly inside the window: 2 boundary lines and 1,998 fillers are 2,000.
        write_audit(&dir.0, &lines[..FLEET_WINDOW]);
        assert_eq!(
            audit
                .fleet_window()
                .await
                .boundaries
                .last_outages
                .get(DEVICE),
            Some(&Absence {
                away_ms: 100,
                back_at_ms: 200,
            })
        );
    }

    /// One proxied call, with the two proofs of a client and the failure left to the test.
    fn call(
        device: &str,
        ts_ms: u64,
        caller: Option<&str>,
        token: Option<&str>,
        status: u16,
    ) -> Value {
        let mut line = serde_json::json!({
            "ts_ms": ts_ms,
            "kind": "mcp",
            "device_id": device,
            "status": status,
            "tool": "files.read",
        });
        if let Some(caller) = caller {
            line["caller"] = serde_json::json!(caller);
        }
        if let Some(token) = token {
            line["token_id"] = serde_json::json!(token);
        }
        line
    }

    #[test]
    fn the_days_buckets_count_the_ais_calls_and_only_their_failures() {
        let token = "ab".repeat(32);
        let lines = vec![
            // A caller of `client` is a client's call whether or not a token authenticated it.
            call(DEVICE, 900, Some("client"), None, 200),
            // No caller is not proof of the console: a token is proof of a client. This one
            // failed at the tool level, carried inside an HTTP 200.
            {
                let mut line = call(DEVICE, 800, None, Some(&token), 200);
                line["detail"] = serde_json::json!("jsonrpc error -32000");
                line
            },
            // The console's own read is never the AI's work, even if a token appeared on it.
            call(DEVICE, 700, Some("console"), Some(&token), 500),
            // A failed poll is the console retrying itself: not a call, and no failure of the AI's.
            {
                let mut line = call(DEVICE, 600, Some("console"), None, 502);
                line["kind"] = serde_json::json!("poll");
                line
            },
            // A channel boundary names the machine but is a fact about its channel, not its work.
            boundary(DEVICE, DEVICE_LEFT, 500),
            // Neither a caller nor a token: unattributed, and it belongs to no count of the AI's.
            call(DEVICE, 400, None, None, 200),
            // A 424 is a tool-level failure the console stored as a refusal.
            call(DEVICE, 300, Some("client"), None, 424),
            // The live stream's explicit `ok: false` fails at 200; a stored line has no `ok`, but
            // the two shapes must count the same way.
            {
                let mut line = call(DEVICE, 200, Some("client"), None, 200);
                line["ok"] = serde_json::json!(false);
                line
            },
        ];

        let mut day = DayBuckets::default();
        for line in &lines {
            add_to_day(&mut day, line);
        }
        assert_eq!(
            day,
            DayBuckets {
                client_calls: 4,
                failed: 3,
                console: 1,
                console_failed: 1,
                poll: 1,
                poll_failed: 1,
                unattributed: 1,
                unattributed_failed: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_machines_day_is_a_zero_only_when_the_day_holds_a_line_for_it() {
        let dir = TempDir::new("day-presence");
        let other = "cd".repeat(32);
        let start = day_start(NOW_MS);
        write_audit(
            &dir.0,
            &[call(&other, start + 1_000, Some("console"), None, 200)],
        );
        let day = Audit::new(&dir.0, u64::MAX).day(NOW_MS).await;

        assert_eq!(
            day.devices.get(&other).map(|day| day.client_calls),
            Some(0),
            "a machine with only console reads has a day of zero calls, not an omitted one"
        );
        assert!(
            !day.devices.contains_key(DEVICE),
            "a machine the day never names has no day to serve"
        );
    }

    /// One local calendar day, whatever the console host's zone: the tests compute the boundary
    /// with the same function the read uses, so a machine in UTC and a machine in +08 both pass.
    fn day_start(now_ms: u64) -> u64 {
        local_day_start_ms(now_ms)
    }

    /// 2026-11-05 16:30 UTC, an instant far enough from both midnights that a test's offsets stay
    /// inside one local day.
    const NOW_MS: u64 = 1_793_000_000_000;

    #[tokio::test]
    async fn the_day_is_the_whole_day_not_the_newest_window() {
        let dir = TempDir::new("day-vs-window");
        let other = "cd".repeat(32);
        let start = day_start(NOW_MS);
        let at = |seconds: u64| start + seconds * 1_000;
        // Ten client calls at the day's start.
        let mut lines: Vec<Value> = (0..10)
            .map(|index| call(DEVICE, at(10 + index), Some("client"), None, 200))
            .collect();
        // Then another machine's 1,997 newer calls. The fleet's 2,000-line window drops the seven
        // oldest of this machine's day; the day read still starts at midnight.
        for index in 0..1_997 {
            lines.push(call(&other, at(100 + index), Some("client"), None, 200));
        }
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);

        let day = audit.day(NOW_MS).await;
        assert_eq!(
            day.devices.get(DEVICE),
            Some(&DayBuckets {
                client_calls: 10,
                ..DayBuckets::default()
            }),
            "the day read starts at midnight, so all ten calls are today"
        );
        assert_eq!(
            day.lines_read, 2_007,
            "the scan reads the whole day once, not the fleet's window"
        );

        // The window read still holds only the newest three of the same day; the day read above
        // is the one `/fleet` now serves, and the two numbers are allowed to differ.
        let window = audit.newest(FLEET_WINDOW).await;
        let window_calls = window
            .iter()
            .filter(|line| {
                line.get("device_id").and_then(Value::as_str) == Some(DEVICE)
                    && is_client_call(line)
            })
            .count();
        assert_eq!(window_calls, 3, "the window holds only the newest three");
    }

    #[tokio::test]
    async fn the_day_count_cannot_shrink_as_the_window_slides() {
        let dir = TempDir::new("day-monotonic");
        let other = "cd".repeat(32);
        let start = day_start(NOW_MS);
        let at = |seconds: u64| start + seconds * 1_000;
        let mut lines: Vec<Value> = (0..10)
            .map(|index| call(DEVICE, at(10 + index), Some("client"), None, 200))
            .collect();
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);
        let first = audit.day(NOW_MS).await;
        assert_eq!(
            first.devices.get(DEVICE).map(|day| day.client_calls),
            Some(10)
        );

        // The day goes on: another machine writes past the fleet's window. The day count may only
        // grow; only a rotation can take today's earlier lines out of the current file.
        for index in 0..3_000 {
            lines.push(call(&other, at(1_000 + index), Some("client"), None, 200));
        }
        write_audit(&dir.0, &lines);
        let second = audit.day(NOW_MS).await;
        assert_eq!(
            second.devices.get(DEVICE).map(|day| day.client_calls),
            Some(10),
            "the sliding window does not shorten the day"
        );
        assert!(
            second.devices[DEVICE].client_calls >= first.devices[DEVICE].client_calls,
            "a day read may not decrease while the day is still running"
        );
    }

    #[tokio::test]
    async fn a_day_that_reaches_midnight_is_whole_and_says_nothing_about_a_start() {
        let dir = TempDir::new("day-full");
        let start = day_start(NOW_MS);
        // One line before midnight; the scan stops at it and does not count it.
        let lines = vec![
            call(DEVICE, start - 1_000, Some("client"), None, 200),
            call(DEVICE, start, Some("client"), None, 200),
            call(DEVICE, start + 60_000, Some("client"), None, 424),
        ];
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);

        let day = audit.day(NOW_MS).await;
        assert_eq!(
            day.since_ms, None,
            "midnight is in the file: the day is whole"
        );
        assert!(!day.bounded);
        assert_eq!(
            day.devices.get(DEVICE),
            Some(&DayBuckets {
                client_calls: 2,
                failed: 1,
                ..DayBuckets::default()
            })
        );
        assert_eq!(
            day.lines_read, 3,
            "the line that proves midnight is read, then the scan stops"
        );
    }

    #[tokio::test]
    async fn a_file_that_starts_after_midnight_says_since_its_oldest_line() {
        let dir = TempDir::new("day-rotation");
        let start = day_start(NOW_MS);
        // 03:12, the shape a rotation leaves: the day's earlier lines are in audit.1.jsonl and the
        // current file cannot count them.
        let since = start + (3 * 60 + 12) * 60_000;
        let lines = vec![
            call(DEVICE, since, Some("client"), None, 200),
            call(DEVICE, since + 60_000, Some("client"), None, 500),
        ];
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);

        let day = audit.day(NOW_MS).await;
        assert_eq!(day.since_ms, Some(since), "the sentence says since 03:12");
        assert!(!day.bounded);
        assert_eq!(
            day.devices.get(DEVICE),
            Some(&DayBuckets {
                client_calls: 2,
                failed: 1,
                ..DayBuckets::default()
            })
        );
        assert_eq!(day.lines_read, 2);
    }

    #[tokio::test]
    async fn a_range_is_half_open_and_does_not_read_the_rotated_generation() {
        let dir = TempDir::new("range-half-open");
        // The older generation holds a line the current file cannot answer for. A range read is
        // current-file-only, so the line is not stitched in; the answer says where coverage starts.
        write_audit(&dir.0, &[call(DEVICE, 500, Some("client"), None, 200)]);
        std::fs::rename(dir.0.join(AUDIT_FILE), dir.0.join("audit.1.jsonl"))
            .expect("the seeded generation rotates");
        write_audit(
            &dir.0,
            &[
                call(DEVICE, 1_000, Some("client"), None, 200),
                call(DEVICE, 1_999, Some("client"), None, 200),
                call(DEVICE, 2_000, Some("client"), None, 200),
            ],
        );
        let audit = Audit::new(&dir.0, u64::MAX);

        let read = audit.range(1_000, 2_000).await;
        let times: Vec<u64> = read
            .lines
            .iter()
            .filter_map(|line| line["ts_ms"].as_u64())
            .collect();
        assert_eq!(
            times,
            vec![1_999, 1_000],
            "`from` belongs and `to` does not, newest first"
        );
        assert!(!read.bounded);
        assert_eq!(
            read.lines_read, 3,
            "the scan stops on the line older than `from`"
        );
        assert_eq!(
            read.since_ms, None,
            "the current file proves the range's start"
        );

        let read = audit.range(0, 3_000).await;
        let times: Vec<u64> = read
            .lines
            .iter()
            .filter_map(|line| line["ts_ms"].as_u64())
            .collect();
        assert_eq!(
            times,
            vec![2_000, 1_999, 1_000],
            "the rotated 500 is not in the answer"
        );
        assert_eq!(
            read.since_ms,
            Some(1_000),
            "coverage starts at the current file's oldest line"
        );
    }

    #[tokio::test]
    async fn a_range_the_bound_cut_says_where_its_coverage_starts() {
        let dir = TempDir::new("range-bound-unit");
        let lines: Vec<Value> = (1..=5)
            .map(|second| call(DEVICE, second * 1_000, Some("client"), None, 200))
            .collect();
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);

        let read = audit.range_with_limit(1_000, 100_000, 2).await;
        let times: Vec<u64> = read
            .lines
            .iter()
            .filter_map(|line| line["ts_ms"].as_u64())
            .collect();
        assert_eq!(times, vec![5_000, 4_000]);
        assert!(read.bounded, "the injected bound stopped the scan");
        assert_eq!(read.lines_read, 2);
        assert_eq!(
            read.since_ms,
            Some(4_000),
            "the count covers the lines the bound let the scan read"
        );
    }

    #[tokio::test]
    async fn the_scan_says_when_its_line_bound_stopped_it() {
        let dir = TempDir::new("day-bound");
        let start = day_start(NOW_MS);
        let lines: Vec<Value> = (0..5)
            .map(|index| {
                call(
                    DEVICE,
                    start + (index + 1) * 1_000,
                    Some("client"),
                    None,
                    200,
                )
            })
            .collect();
        write_audit(&dir.0, &lines);
        let audit = Audit::new(&dir.0, u64::MAX);

        let day = audit.day_with_limit(NOW_MS, 2).await;
        assert!(day.bounded, "the bound was hit before midnight");
        assert_eq!(day.lines_read, 2);
        assert_eq!(
            day.since_ms,
            Some(start + 4_000),
            "the count covers since the oldest line it read"
        );
        assert_eq!(
            day.devices.get(DEVICE).map(|day| day.client_calls),
            Some(2),
            "a bounded read reports the lines it counted, not a guess at the rest"
        );
    }
}
