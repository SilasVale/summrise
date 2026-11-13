//! `journal`: the declaration's implementation. The bus is `summrise_bus`; this module is the tool
//! surface and the document shapes its answers have.

use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::mcp::ToolError;
use summrise_core::capabilities::journal::CAPABILITY;

/// The longest a trace id may be.
const MAX_TRACE_CHARS: usize = 128;

const DAY_LOOKBACK_ROWS: usize = 2_000;

const DAY_LOOKBACK_PAGE: usize = 200;

/// The call a journal row belongs to, when its trace names one.
fn call_of(envelope: &summrise_bus::Envelope) -> Option<&str> {
    envelope
        .trace
        .as_ref()
        .and_then(|trace| trace.get("call"))
        .and_then(Value::as_str)
}

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

/// Run one of this capability's tools against the bus, when this process has one.
pub fn call(
    tool: &str,
    arguments: Map<String, Value>,
    bus: Option<&summrise_bus::Bus>,
) -> Result<String, ToolError> {
    match tool {
        "journal.read" => {
            let (before_seq, limit) = parse_read_window(arguments, tool)?;
            let Some(bus) = bus else {
                return Ok(no_journal_window_document().to_string());
            };
            let (window, bus_more) = bus.read_window(before_seq, limit + 1).map_err(|error| {
                ToolError::Refused(format!("{tool}: the journal could not be read: {error}"))
            })?;
            let shown = window.len().min(limit);
            let more = bus_more || window.len() > limit;
            Ok(journal_rows_document(bus, &window, shown, more).to_string())
        }
        "journal.get" => {
            let traces = parse_traces(arguments, tool)?;
            let Some(bus) = bus else {
                return Ok(no_journal_document().to_string());
            };
            let rows = bus.scan_newest(SCAN_ROWS).map_err(|error| {
                ToolError::Refused(format!("{tool}: the journal could not be read: {error}"))
            })?;
            Ok(journal_traces_document(bus, &traces, &rows).to_string())
        }
        "journal.day" => {
            let (day_start, day_end) = parse_day_range(arguments, tool)?;
            let Some(bus) = bus else {
                return Ok(no_journal_day_document(day_start, day_end).to_string());
            };
            let (rows, more) =
                bus.read_between(day_start, day_end, MAX_DAY_ROWS)
                    .map_err(|error| {
                        ToolError::Refused(format!(
                            "{tool}: the journal could not be read: {error}"
                        ))
                    })?;
            Ok(day_calls_document(bus, day_start, day_end, &rows, more).to_string())
        }
        other => Err(ToolError::Internal(format!(
            "{other}: the journal capability does not declare this tool"
        ))),
    }
}

const SCAN_ROWS: usize = 10_000;

const MAX_READ_ROWS: usize = 200;

const MAX_DAY_ROWS: usize = 20_000;

const MAX_DAY_SPAN_MS: u64 = 26 * 60 * 60 * 1000;

const MAX_TRACES: usize = 512;

fn parse_traces(arguments: Map<String, Value>, tool: &str) -> Result<Vec<String>, ToolError> {
    let invalid = |rule: String| ToolError::Arguments(format!("{tool}: {rule}"));
    if let Some(unknown) = arguments.keys().find(|key| key.as_str() != "traces") {
        return Err(invalid(format!(
            "the argument `{unknown}` is not what this tool takes; it takes `traces`"
        )));
    }
    let Some(value) = arguments.get("traces") else {
        return Err(invalid(
            "the argument `traces` is required: it names the call traces to look for".to_owned(),
        ));
    };
    let Some(items) = value.as_array() else {
        return Err(invalid(
            "`traces` is an array of trace ids, the value of a call's `trace.call`".to_owned(),
        ));
    };
    if items.is_empty() {
        return Err(invalid(
            "`traces` names at least one trace, and this call named none".to_owned(),
        ));
    }
    if items.len() > MAX_TRACES {
        return Err(invalid(format!(
            "`traces` names at most {MAX_TRACES} traces, and this call named {}",
            items.len()
        )));
    }
    let mut traces = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let Some(trace) = item.as_str() else {
            return Err(invalid(format!(
                "`traces[{position}]` is a string, the value of a call's `trace.call`"
            )));
        };
        if trace.is_empty() {
            return Err(invalid(format!(
                "`traces[{position}]` is not empty: a trace names a call"
            )));
        }
        if trace.chars().count() > MAX_TRACE_CHARS {
            return Err(invalid(format!(
                "`traces[{position}]` is at most {MAX_TRACE_CHARS} characters"
            )));
        }
        traces.push(trace.to_owned());
    }
    Ok(traces)
}

fn no_journal_document() -> Value {
    json!({
        "journal": false,
        "oldest_seq": 0,
        "last_seq": 0,
        "scanned": 0,
        "bounded": false,
        "traces": [],
        "missing": [],
    })
}

fn parse_day_range(arguments: Map<String, Value>, tool: &str) -> Result<(u64, u64), ToolError> {
    let invalid = |rule: String| ToolError::Arguments(format!("{tool}: {rule}"));
    if arguments
        .keys()
        .any(|key| !matches!(key.as_str(), "day_start_ms" | "day_end_ms"))
    {
        return Err(invalid(
            "the arguments are not what this tool takes; it takes `day_start_ms` and `day_end_ms`"
                .to_owned(),
        ));
    }
    let start = match arguments.get("day_start_ms").and_then(Value::as_u64) {
        Some(start) => start,
        None => {
            return Err(invalid(
                "`day_start_ms` is the range's first moment, in Unix milliseconds".to_owned(),
            ))
        }
    };
    let end = match arguments.get("day_end_ms").and_then(Value::as_u64) {
        Some(end) => end,
        None => {
            return Err(invalid(
                "`day_end_ms` is the moment after the range, in Unix milliseconds".to_owned(),
            ))
        }
    };
    if end <= start {
        return Err(invalid(
            "`day_end_ms` must be after `day_start_ms`: the range is half-open, [start, end)"
                .to_owned(),
        ));
    }
    if end - start > MAX_DAY_SPAN_MS {
        return Err(invalid(format!(
            "the range must be no more than {} hours apart",
            MAX_DAY_SPAN_MS / 3_600_000
        )));
    }
    Ok((start, end))
}

fn no_journal_day_document(day_start: u64, day_end: u64) -> Value {
    json!({
        "journal": false,
        "day_start_ms": day_start,
        "day_end_ms": day_end,
        "oldest_seq": 0,
        "last_seq": 0,
        "scanned": 0,
        "more": false,
        "calls": {
            "console": { "calls": 0, "failed": 0 },
            "panel": { "calls": 0, "failed": 0 },
            "mesh": { "calls": 0, "failed": 0 },
            "loopback": { "calls": 0, "failed": 0 },
            "unknown": { "calls": 0, "failed": 0 },
        },
    })
}

fn day_calls_document(
    bus: &summrise_bus::Bus,
    day_start: u64,
    day_end: u64,
    rows: &[summrise_bus::Envelope],
    more: bool,
) -> Value {
    let mut doors: HashMap<String, String> = HashMap::new();
    let mut unresolved: HashSet<String> = HashSet::new();
    for row in rows {
        let Some(call) = call_of(row) else { continue };
        match row.kind.as_str() {
            "call.started" => {
                if let Some(door) = row.payload.get("door").and_then(Value::as_str) {
                    doors.insert(call.to_owned(), door.to_owned());
                }
            }
            "call.finished" | "call.denied" if !doors.contains_key(call) => {
                unresolved.insert(call.to_owned());
            }
            _ => {}
        }
    }

    // **THE HALF BELOW THE DAY, BOUNDED.** A call that started before midnight and finished after it
    // is an outcome of the day whose door is on the older row. Reading just below the oldest row of
    // the range resolves those halves; a call starting further back is unknown by the rule, not by
    // silence.
    let mut cursor = rows.last().map(|row| row.seq);
    let mut looked = 0;
    while !unresolved.is_empty() && looked < DAY_LOOKBACK_ROWS {
        let Some(before) = cursor else { break };
        let Ok((window, _)) = bus.read_window(Some(before), DAY_LOOKBACK_PAGE) else {
            break;
        };
        if window.is_empty() {
            break;
        }
        for row in &window {
            if row.kind != "call.started" {
                continue;
            }
            let Some(call) = call_of(row) else { continue };
            if !unresolved.contains(call) {
                continue;
            }
            if let Some(door) = row.payload.get("door").and_then(Value::as_str) {
                doors.insert(call.to_owned(), door.to_owned());
                unresolved.remove(call);
            }
        }
        looked += window.len();
        cursor = window.last().map(|row| row.seq);
    }

    let mut counts: BTreeMap<&'static str, (u64, u64)> = BTreeMap::from([
        ("console", (0, 0)),
        ("panel", (0, 0)),
        ("mesh", (0, 0)),
        ("loopback", (0, 0)),
        ("unknown", (0, 0)),
    ]);
    for row in rows {
        if !matches!(row.kind.as_str(), "call.finished" | "call.denied") {
            continue;
        }
        let door = call_of(row)
            .and_then(|call| doors.get(call))
            .map(String::as_str)
            .filter(|door| matches!(*door, "console" | "panel" | "mesh" | "loopback"))
            .unwrap_or("unknown");
        let failed = row.payload.get("ok").and_then(Value::as_bool) != Some(true);
        let entry = counts
            .get_mut(door)
            .expect("the five doors are initialized");
        entry.0 += 1;
        if failed {
            entry.1 += 1;
        }
    }

    let oldest_seq = bus.oldest_seq();
    let oldest_ts = if oldest_seq == 0 {
        None
    } else {
        bus.read_window(Some(oldest_seq.saturating_add(1)), 1)
            .ok()
            .and_then(|(window, _)| window.into_iter().next())
            .map(|row| row.ts)
    };

    let mut document = json!({
        "journal": true,
        "day_start_ms": day_start,
        "day_end_ms": day_end,
        "oldest_seq": oldest_seq,
        "last_seq": bus.last_seq(),
        "scanned": rows.len(),
        "more": more,
        "calls": counts
            .into_iter()
            .map(|(door, (calls, failed))| {
                (door.to_owned(), json!({ "calls": calls, "failed": failed }))
            })
            .collect::<serde_json::Map<_, _>>(),
    });
    if let Some(ts) = oldest_ts {
        document["oldest_ts"] = json!(ts);
    }
    document
}

fn parse_read_window(
    arguments: Map<String, Value>,
    tool: &str,
) -> Result<(Option<u64>, usize), ToolError> {
    let invalid = |rule: String| ToolError::Arguments(format!("{tool}: {rule}"));
    if arguments
        .keys()
        .any(|key| !matches!(key.as_str(), "before_seq" | "limit"))
    {
        // **THE RULE, NOT THE VALUE.** An unknown key is a caller's own bytes; naming it back would
        // echo whatever was sent, which is the rule this tool's refusals keep (a client that put a
        // secret in the wrong field must not read it out of the refusal). The two keys it does take
        // are the rule, so they are the sentence.
        return Err(invalid(
            "the arguments are not what this tool takes; it takes `limit` and `before_seq`"
                .to_owned(),
        ));
    }
    let Some(value) = arguments.get("limit") else {
        return Err(invalid(
            "the argument `limit` is required: it says how many rows to answer".to_owned(),
        ));
    };
    let Some(count) = value.as_u64() else {
        return Err(invalid(
            "`limit` is a whole number of rows, at least one".to_owned(),
        ));
    };
    if count == 0 || count > MAX_READ_ROWS as u64 {
        // The number asked for is the caller's, never the machine's: the refusal states the bound,
        // not the value that crossed it.
        return Err(invalid(format!(
            "`limit` is between 1 and {MAX_READ_ROWS} rows"
        )));
    }
    let before_seq =
        match arguments.get("before_seq") {
            None => None,
            Some(value) => match value.as_u64() {
                Some(seq) if seq >= 1 => Some(seq),
                _ => return Err(invalid(
                    "`before_seq` is the last sequence the caller drew, a whole number at least 1"
                        .to_owned(),
                )),
            },
        };
    Ok((before_seq, count as usize))
}

fn no_journal_window_document() -> Value {
    json!({
        "journal": false,
        "oldest_seq": 0,
        "last_seq": 0,
        "rows": [],
        "more": false,
    })
}

fn machine_row(envelope: &summrise_bus::Envelope) -> Value {
    let payload = &envelope.payload;
    let mut row = json!({
        "seq": envelope.seq,
        "ts": envelope.ts,
        "type": envelope.kind,
        "payload_bytes": serde_json::to_string(payload)
            .map(|text| text.len() as u64)
            .unwrap_or(0),
    });
    if let Some(trace) = envelope
        .trace
        .as_ref()
        .and_then(|trace| trace.get("call"))
        .and_then(Value::as_str)
    {
        row["trace"] = json!(trace);
    }
    // A tool name and an outcome belong to a call. The envelope's `type` is what says so, so the
    // rule is the row's own rather than a payload field a caller could invent.
    if envelope.kind.starts_with("call.") {
        if let Some(tool) = payload.get("tool").and_then(Value::as_str) {
            row["tool"] = json!(tool);
        }
        if let Some(ok) = payload.get("ok").and_then(Value::as_bool) {
            row["ok"] = json!(ok);
        }
        if let Some(reason) = payload.get("reason").and_then(Value::as_str) {
            row["reason"] = json!(reason);
        }
        if let Some(door) = payload.get("door").and_then(Value::as_str) {
            row["door"] = json!(door);
        }
    }
    row
}

fn journal_rows_document(
    bus: &summrise_bus::Bus,
    window: &[summrise_bus::Envelope],
    shown: usize,
    more: bool,
) -> Value {
    // **THE DOOR IS WRITTEN ON THE HALF THAT HAS IT.** A call's arrival is on `call.started`; its
    // outcome half carries only the tool and whether it worked. A page drawing outcomes would have to
    // read both halves to say who called, so the window carries the door across, by the `trace.call`
    // the two halves already share. Reading one row past the page is what keeps a door from
    // disappearing just because its outcome sat on the boundary; a started half truly outside the
    // window still leaves the door unknown, rather than a guess.
    let doors: std::collections::HashMap<&str, &str> = window
        .iter()
        .filter(|row| row.kind == "call.started")
        .filter_map(|row| {
            let call = row.trace.as_ref()?.get("call")?.as_str()?;
            Some((call, row.payload.get("door")?.as_str()?))
        })
        .collect();
    let summarised: Vec<Value> = window
        .iter()
        .take(shown)
        .map(|row| {
            let mut summarised = machine_row(row);
            if summarised.get("door").is_none() {
                if let Some(door) = summarised
                    .get("trace")
                    .and_then(Value::as_str)
                    .and_then(|call| doors.get(call))
                {
                    summarised["door"] = json!(door);
                }
            }
            summarised
        })
        .collect();
    json!({
        "journal": true,
        "oldest_seq": bus.oldest_seq(),
        "last_seq": bus.last_seq(),
        "rows": summarised,
        "more": more,
    })
}

fn trace_matches(rows: &[summrise_bus::Envelope], traces: &[String]) -> (Vec<String>, Vec<String>) {
    let held: std::collections::HashSet<&str> = rows
        .iter()
        .filter_map(|row| row.trace.as_ref())
        .filter_map(|trace| trace.get("call"))
        .filter_map(Value::as_str)
        .collect();
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for trace in traces {
        if held.contains(trace.as_str()) {
            found.push(trace.clone());
        } else {
            missing.push(trace.clone());
        }
    }
    (found, missing)
}

fn journal_traces_document(
    bus: &summrise_bus::Bus,
    traces: &[String],
    rows: &[summrise_bus::Envelope],
) -> Value {
    let (found, missing) = trace_matches(rows, traces);
    let bounded = rows.len() >= SCAN_ROWS;
    json!({
        "journal": true,
        "oldest_seq": bus.oldest_seq(),
        "last_seq": bus.last_seq(),
        "scanned": rows.len(),
        "bounded": bounded,
        "traces": found,
        "missing": if bounded { Vec::new() } else { missing },
    })
}
