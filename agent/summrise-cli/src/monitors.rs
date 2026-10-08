//! MACHINE-READABLE OUTPUT AND THE LINES AN OPERATOR PASTES.
//!
//! Ported from `fmtDuration`, `targetLine`, `probeLine`, `monitorsJson` and `asciiJson` in
//! `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! The payloads are [`serde_json::Value`] rather than typed structs, and that is deliberate: these
//! functions project a document the DEVICE wrote, and the TypeScript's whole job here is to keep
//! every field present as `null` when the device did not send it. A struct with `Option` fields
//! would serialise the same way for the fields that exist, and would silently DROP a key the device
//! grows — which is the failure the TypeScript's own comment names: "a field that vanishes from the
//! JSON is a field a script cannot tell from `false`".

use serde_json::{json, Map, Value};

/// A duration in the shapes the rest of the panel uses: `45s`, `4m`, `1h 04m`, `2d 4h`.
pub fn fmt_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    if s < 3600 {
        return format!("{}m", s / 60);
    }
    let h = s / 3600;
    if h >= 24 {
        format!("{}d {}h", h / 24, h % 24)
    } else {
        format!("{h}h {:02}m", (s % 3600) / 60)
    }
}

fn s_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// One target, as the line an operator reads.
///
/// `state` is three-valued: `up`, `down`, or `no readings` — never a claim of absence from a value
/// that was not sent.
pub fn target_line(t: &Value, now_ms: i64, width: usize) -> String {
    let summary = t.get("summary").cloned().unwrap_or(Value::Null);
    let up = summary.get("up_now") == Some(&Value::Bool(true));
    let is_down = summary.get("up_now") == Some(&Value::Bool(false));
    let state = if up {
        "up"
    } else if is_down {
        "down"
    } else {
        "no readings"
    };
    let id = t.get("id").map(s_of).unwrap_or_default();
    let since = match summary.get("since_ms").and_then(|v| v.as_i64()) {
        Some(ms) if ms != 0 => format!(" {}", fmt_duration(now_ms - ms)),
        _ => String::new(),
    };
    let status = match summary.get("last_status") {
        Some(Value::Null) | None => String::new(),
        Some(v) => format!("  HTTP {}", s_of(v)),
    };
    // A content check that did not find its text is the case a status code cannot express, so it is
    // printed as its own word next to the code.
    let mat = match summary.get("last_expect_ok") {
        Some(Value::Bool(false)) => "  no match",
        Some(Value::Bool(true)) => "  matches",
        _ => "",
    };
    let lat = match summary.get("latency").and_then(|l| l.get("avg")) {
        Some(Value::Null) | None => String::new(),
        Some(v) => format!("  {}ms avg", s_of(v)),
    };
    let pct = match summary.get("up_pct") {
        Some(Value::Null) | None => String::new(),
        Some(v) => format!("  {}% up", s_of(v)),
    };
    let drops = match summary.get("drops").and_then(|v| v.as_i64()) {
        Some(n) if n != 0 => format!("  {n} {}", if n == 1 { "drop" } else { "drops" }),
        _ => String::new(),
    };
    // The operator's own words belong on the line they explain, not in a separate view.
    let note = match t.get("note").and_then(|n| n.get("text")) {
        Some(Value::String(s)) if !s.is_empty() => format!("  \u{2014} {s}"),
        _ => String::new(),
    };
    let prefix = if up {
        "UP  "
    } else if is_down {
        "DOWN"
    } else {
        "?   "
    };
    let mut padded = id.clone();
    while padded.chars().count() < width {
        padded.push(' ');
    }
    format!("{prefix} {padded} {state}{since}{status}{mat}{lat}{pct}{drops}{note}")
}

/// ONE PROBE'S ANSWER, as a line — the terminal's version of what the device's `monitor_probe`
/// returns. Pure, because the wording is the feature: a DOWN probe says WHICH way it failed (no
/// connection / status 500 / 200 without the expected text), not merely that it failed.
pub fn probe_line(target: &Value, probe: &Value, _now_ms: i64) -> String {
    let id = target.get("id").map(s_of).unwrap_or_default();
    let ok = probe.get("ok") == Some(&Value::Bool(true));
    let state = if ok { "UP  " } else { "DOWN" };
    let mut bits: Vec<String> = Vec::new();
    match probe.get("status") {
        Some(Value::Null) | None => {}
        Some(v) => bits.push(format!("HTTP {}", s_of(v))),
    }
    // A content check is the one failure a status code cannot express, so it is named.
    if let Some(expect) = target.get("expect").and_then(|v| v.as_str()) {
        if !expect.is_empty() {
            match probe.get("expect_ok") {
                Some(Value::Bool(false)) => bits.push(format!("no match for \"{expect}\"")),
                Some(Value::Bool(true)) => bits.push(format!("matches \"{expect}\"")),
                _ => bits.push(format!("could not read the body to look for \"{expect}\"")),
            }
        }
    }
    if let Some(ms) = probe.get("ms").and_then(|v| v.as_i64()) {
        bits.push(format!("{ms}ms"));
    }
    if !ok && bits.is_empty() {
        bits.push("no answer".to_string());
    }
    if bits.is_empty() {
        format!("{state} {id}")
    } else {
        format!("{state} {id}  {}", bits.join("  "))
    }
}

/// The device's own answer, PROJECTED rather than re-derived.
///
/// `{device, asked_at_ms, interval_secs, targets:[…]}` — a STABLE shape, not a passthrough:
/// anything a script needs to branch on is in it, and anything that is presentation (the coloured
/// line, the outage wording) is not. The CLI adds only what the device cannot know — which device
/// answered, and when this script asked — under names that say so.
pub fn monitors_json(
    device: &str,
    asked_at_ms: i64,
    payload: Option<&Value>,
    only: Option<&str>,
) -> Value {
    let empty = Value::Null;
    let payload = payload.unwrap_or(&empty);
    let targets_in = payload
        .get("targets")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let mut targets = Vec::new();
    for t in &targets_in {
        let id = t.get("id").cloned().unwrap_or(Value::Null);
        if let Some(only) = only {
            if id.as_str() != Some(only) {
                continue;
            }
        }
        let summary = t.get("summary").cloned().unwrap_or(Value::Null);
        let pick = |key: &str| -> Value { summary.get(key).cloned().unwrap_or(Value::Null) };
        let mut trs = Vec::new();
        if let Some(list) = t.get("transitions").and_then(|v| v.as_array()) {
            for x in list {
                // EVERY field is present in every row of a TARGET, as `null` when the device did
                // not send it — but a TRANSITION is copied as-is, so a key the device omitted is
                // omitted here too. That asymmetry is the TypeScript's (`x.at_ms` with no `?? null`),
                // and it is what `JSON.stringify` does to an `undefined` property.
                let mut o = Map::new();
                for k in ["at_ms", "up", "lasted_ms"] {
                    if let Some(v) = x.get(k) {
                        if !v.is_null() {
                            o.insert(k.to_string(), v.clone());
                        }
                    }
                }
                trs.push(Value::Object(o));
            }
        }
        let mut row = Map::new();
        for (k, v) in [
            ("id", id),
            ("host", t.get("host").cloned().unwrap_or(Value::Null)),
            ("port", t.get("port").cloned().unwrap_or(Value::Null)),
            ("path", t.get("path").cloned().unwrap_or(Value::Null)),
            ("expect", t.get("expect").cloned().unwrap_or(Value::Null)),
            ("up", pick("up_now")),
            ("up_pct", pick("up_pct")),
            ("since_ms", pick("since_ms")),
            ("drops", pick("drops")),
            (
                "latency_ms",
                summary
                    .get("latency")
                    .and_then(|l| l.get("avg"))
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            ("last_status", pick("last_status")),
            ("last_expect_ok", pick("last_expect_ok")),
            ("probes", pick("probes")),
            ("transitions", Value::Array(trs)),
        ] {
            row.insert(k.to_string(), v);
        }
        targets.push(Value::Object(row));
    }
    // `(payload && payload.interval_secs) || null` — a falsy value (0, absent) is null.
    let interval = payload
        .get("interval_secs")
        .filter(|v| !v.is_null() && v.as_i64() != Some(0) && v != &&Value::Bool(false))
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "device": device,
        "asked_at_ms": asked_at_ms,
        "interval_secs": interval,
        "targets": Value::Array(targets),
    })
}

/// JSON that survives the command line.
///
/// A `curl -d <string>` argument is encoded in the process's ANSI code page on Windows — NOT
/// UTF-8 — so a note typed with an em dash reached the device as mojibake while the same text
/// printed directly read fine. Escaping every non-ASCII character as `\uXXXX` makes the body pure
/// ASCII, which no code page can mangle, and the device's JSON parser restores the original text.
///
/// `indent` is `Some(2)` for the human-readable printers; `None` is the compact form. Anything else
/// is treated as compact, because the TypeScript only ever passes those two.
pub fn ascii_json(value: &Value, indent: Option<usize>) -> String {
    let text = if indent == Some(2) {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else {
        serde_json::to_string(value).unwrap_or_default()
    };
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let cp = c as u32;
        if cp <= 0x7e {
            out.push(c);
        } else if cp <= 0xffff {
            out.push_str(&format!("\\u{cp:04x}"));
        } else {
            // An astral character is TWO \uXXXX escapes in JavaScript, because the string is
            // UTF-16 there: `🚀` is `\ud83d\ude80`. The device parses both forms to the same text,
            // but the parity check compares the TEXT, so the port has to match.
            let v = cp - 0x10000;
            let hi = 0xd800 + (v >> 10);
            let lo = 0xdc00 + (v & 0x3ff);
            out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: cli.test.mjs:2752 — "targetLine: one drop is 'drop', several are 'drops'".
    #[test]
    fn target_line_singular_and_plural_drops() {
        let now = 1_789_000_000_000i64;
        let at = |drops: i64| {
            target_line(
                &json!({"id": "a:22", "summary": {"up_now": true, "since_ms": now - 1000, "drops": drops}}),
                now,
                30,
            )
        };
        // `/1 drop(?!s)/` — a negative lookahead Rust's regex does not have. The claim is the same
        // one: the word attached to the number is `drop`, and it is not `drops`.
        assert!(at(1).contains(" 1 drop"), "{}", at(1));
        assert!(!at(1).contains("drops"), "{}", at(1));
        assert!(at(2).contains(" 2 drops"), "{}", at(2));
        assert!(!at(0).contains("drop"), "{}", at(0));
    }

    /// Oracle: cli.test.mjs:2766 — "targetLine: a content check that failed says so, next to the
    /// code that looked fine".
    #[test]
    fn target_line_names_a_failed_content_check() {
        let now = 1_789_000_000_000i64;
        let line = target_line(
            &json!({"id": "h:80/", "summary": {"up_now": false, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": false}}),
            now,
            30,
        );
        assert!(line.contains("HTTP 200"), "{line}");
        assert!(line.contains("no match"), "{line}"); // 200 AND wrong
        let ok = target_line(
            &json!({"id": "h:80/", "summary": {"up_now": true, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": true}}),
            now,
            30,
        );
        assert!(ok.contains("matches"), "{ok}");
        let plain = target_line(
            &json!({"id": "h:80/", "summary": {"up_now": true, "since_ms": now - 1000, "last_status": 200, "last_expect_ok": null}}),
            now,
            30,
        );
        assert!(!plain.contains("match"), "{plain}");
    }

    /// Oracle: cli.test.mjs:2814 — "monitorsJson: the device's numbers verbatim, with only what the
    /// CLI knows added".
    #[test]
    fn monitors_json_projects_the_devices_numbers() {
        let payload = json!({
            "ok": true,
            "interval_secs": 15,
            "targets": [
                {
                    "id": "192.168.1.1:22", "host": "192.168.1.1", "port": 22, "path": null, "expect": null,
                    "summary": {
                        "probes": 12, "up": 12, "down": 0, "up_pct": 100, "up_now": true,
                        "since_ms": 111, "drops": 0, "latency": {"min": 7, "avg": 9, "max": 16},
                        "last_status": null, "last_expect_ok": null
                    },
                    "transitions": [{"at_ms": 100, "up": true, "lasted_ms": 39_000}],
                    "series": [{"ts_ms": 1, "ok": true, "ms": 9}]
                },
                {
                    "id": "h:80/", "host": "h", "port": 80, "path": "/", "expect": "OpenWrt",
                    "summary": {
                        "probes": 4, "up": 2, "down": 2, "up_pct": 50, "up_now": false,
                        "since_ms": 222, "drops": 1, "latency": null, "last_status": 200,
                        "last_expect_ok": false
                    },
                    "transitions": []
                }
            ]
        });
        let all = monitors_json("d1", 1_789_000_000_000, Some(&payload), None);
        assert_eq!(all["device"], json!("d1"));
        assert_eq!(all["asked_at_ms"], json!(1_789_000_000_000i64));
        assert_eq!(all["interval_secs"], json!(15));
        assert_eq!(all["targets"].as_array().unwrap().len(), 2);
        assert_eq!(
            all["targets"][0],
            json!({
                "id": "192.168.1.1:22", "host": "192.168.1.1", "port": 22, "path": null,
                "expect": null, "up": true, "up_pct": 100, "since_ms": 111, "drops": 0,
                "latency_ms": 9, "last_status": null, "last_expect_ok": null, "probes": 12,
                "transitions": [{"at_ms": 100, "up": true, "lasted_ms": 39_000}]
            })
        );
        assert_eq!(all["targets"][1]["last_expect_ok"], json!(false));
        assert_eq!(all["targets"][1]["last_status"], json!(200));
        let one = monitors_json("d1", 1, Some(&payload), Some("h:80/"));
        assert_eq!(one["targets"].as_array().unwrap().len(), 1);
        assert_eq!(one["targets"][0]["id"], json!("h:80/"));
        // Nothing read yet is null, NOT false: a script must be able to tell "not known" from "down".
        let unknown = monitors_json(
            "d1",
            1,
            Some(
                &json!({"targets": [{"id": "x:1", "host": "x", "port": 1, "summary": {"up_now": null, "probes": 0}}]}),
            ),
            None,
        );
        assert_eq!(unknown["targets"][0]["up"], Value::Null);
        assert_eq!(unknown["targets"][0]["up_pct"], Value::Null);
        assert_eq!(
            unknown["targets"][0]["transitions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        // A payload with no targets at all is an empty list, not a crash.
        assert_eq!(monitors_json("d1", 1, None, None)["targets"], json!([]));
    }

    /// Oracle: cli.test.mjs:2921 — "asciiJson: text that goes through a command line must survive
    /// it".
    #[test]
    fn ascii_json_survives_a_command_line() {
        let body = ascii_json(
            &json!({"id": "h:22", "text": "I rebooted it \u{2014} not a fault"}),
            None,
        );
        assert!(
            !body.chars().any(|c| (c as u32) > 0x7e),
            "pure ASCII: no code page can mangle what has no high bytes"
        );
        assert!(body.contains("\\u2014"), "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"id": "h:22", "text": "I rebooted it \u{2014} not a fault"})
        );
        assert_eq!(
            ascii_json(&json!({"a": 1, "b": true, "c": null}), None),
            "{\"a\":1,\"b\":true,\"c\":null}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&ascii_json(
                &json!({"t": "\u{4e2d}\u{6587} / \u{65e5}\u{672c}\u{8a9e} / \u{e9}moji \u{1f680}"}),
                None
            ))
            .unwrap(),
            json!({"t": "\u{4e2d}\u{6587} / \u{65e5}\u{672c}\u{8a9e} / \u{e9}moji \u{1f680}"})
        );
        assert_eq!(ascii_json(&json!({}), None), "{}");
        // The same rule serves the OUTPUT side: a pipe is an encoding boundary too.
        let out = ascii_json(
            &json!({"device": "d1", "targets": [{"note": {"text": "\u{6211}\u{91cd}\u{542f}\u{7684} \u{2014} ok \u{1f680}"}}]}),
            Some(2),
        );
        assert!(!out.chars().any(|c| (c as u32) > 0x7e), "{out}");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap()["targets"][0]["note"]["text"],
            json!("\u{6211}\u{91cd}\u{542f}\u{7684} \u{2014} ok \u{1f680}")
        );
        assert!(out.contains("\n  \"device\""), "the indent is kept");
    }

    /// Oracle: cli.test.mjs:2961 — "the --json path actually USES the escaping helper (a helper test
    /// is not a wiring test)".
    ///
    /// Both encoding bugs of that round had the same shape: the helper was right and the CALL SITE
    /// was not. The TypeScript pinned the wiring by reading the shipped file; here the wiring is a
    /// function, so the case EXECUTES it — which is the stronger form of the same assertion.
    #[test]
    fn the_json_printer_goes_through_the_escaper() {
        // A field the PROJECTION carries (`expect`), because the whole point of the printer is the
        // shape `monitorsJson` produces — a field it does not project is not a test of the escaper.
        let payload = json!({"targets": [{"id": "h:80/", "expect": "\u{2014}"}]});
        let printed = crate::dispatch::print_monitors_json("d1", 1, &payload, None);
        assert!(
            !printed.chars().any(|c| (c as u32) > 0x7e),
            "the --json printer must go through ascii_json: {printed}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&printed).unwrap()["targets"][0]["expect"],
            json!("\u{2014}")
        );
    }
}
