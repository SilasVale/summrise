//! Tool definitions for the monitor plugin.
//!
//! Four tools over ONE instrument (`crate::monitor`): read the watches, take a probe now, and
//! add or remove a watch. Nothing here re-derives a rule the panel also has — `summary`,
//! `series` and `TargetSpec` are the module's own, so an AI reading `monitor_list` and an
//! operator reading the Reachability card see the same numbers (round 261's whole point). What the
//! add door keeps is its own ENVELOPE (`DeviceError::InvalidParams`) and not a copy of the rules:
//! they live in `crate::monitor::TargetSpec::parse`, which the HTTP form calls as well.
//!
//! WHAT THE AI IS TOLD, in the descriptions, is the part that makes these usable rather than
//! merely present: the probe interval, that a series gap is a FAILED probe, that a refused
//! connection counts as down (the probe answers one question), and that the watch list is
//! PERSISTED — an AI that adds a watch is leaving an instrument for the operator, not just
//! arming something for its own next call.

use crate::plugins::require_str;
use serde_json::{json, Value};
use summrise_agent_core::{DeviceError, ToolDef};

/// How many recent probes `monitor_list` returns per target. The SUMMARY covers the whole
/// series (240 samples = one hour); this is the tail an AI reads to see the shape — a hundred
/// rows of JSON per target would crowd out everything else in a context window.
const SERIES_TAIL: usize = 60;

pub fn build() -> Vec<ToolDef> {
    vec![tool_list(), tool_add(), tool_remove(), tool_probe()]
}

fn tool_list() -> ToolDef {
    ToolDef::new(
        "monitor_list",
        "List the host:port targets this device watches over TCP, with each one's summary and its most recent probes. \
         The summary carries what an operator asks first: `up_now`, `since_ms` (when the CURRENT state began — the number that turns a state into a story), `up_pct`, the latency range, `drops` (how many times it fell from up to down inside the window — a target that is down now contributes the drop that started it), `last_status` (the HTTP status code, for a target watched with a path) and `last_expect_ok` (whether the body contained the expected text, when one was given). \
         The series is oldest-first; a probe with `ok: false` carries NO latency (nothing was measured) and a GAP in time is a probe that failed. `transitions` is the LOG of state changes — when it went down or came back, and how long the state it ended had lasted (for a recovery, the OUTAGE), which is the form a person writes into a report. \
         The device probes every 15 s on its own timer, so this is what happened while you were doing something else — including whether something you did took a host down.",
        json!({"type": "object", "properties": {}}),
        move |_params: Value| {
            async move {
                let targets: Vec<Value> = crate::monitor::targets()
                    .into_iter()
                    .map(|t| {
                        json!({
                            "id": t.id,
                            "host": t.host,
                            "port": t.port,
                            "path": t.path,
                            "summary": crate::monitor::summary(&t.id),
                            "series": crate::monitor::series(&t.id, SERIES_TAIL),
                        })
                    })
                    .collect();
                Ok(json!({
                    "ok": true,
                    "interval_secs": crate::monitor::PROBE_INTERVAL_SECS,
                    "targets": targets,
                }))
            }
        },
    )
}

fn tool_add() -> ToolDef {
    ToolDef::new(
        "monitor_add",
        "Start watching a host:port on this device and leave the watch in place. \
         The list is PERSISTED, so a watch you add survives an agent restart and is still there for the operator afterwards — add one when something you are about to touch must be seen coming back. \
         The probe is a TCP connect: a REFUSED connection counts as down (the service is not there), which is the question this instrument answers. \
         Adding the same host:port (and path) twice is idempotent — it is the same watch, not a second one. A port is required: name the SERVICE (22 for SSH, 80 for a web UI), because guessing it would probe the wrong thing and report it as fact.          `path` turns the check into a real HTTP GET of that path (\"/\" for a UI's front page, \"/api/health\" for a health endpoint): the probe then records the STATUS CODE, and `ok` means a response arrived with a status below 500 — so a UI answering 500 is DOWN while one answering 401 is UP (it wants credentials, and it is serving). Without a path the probe is a bare TCP connect, which cannot tell those apart. HTTP only: a TLS check needs a certificate story this instrument does not have.",
        json!({
            "type": "object",
            "properties": {
                "host": {"type": "string", "description": "IP address or name, e.g. \"192.168.1.1\"."},
                "port": {"type": "integer", "description": "TCP port to connect to, 1-65535."},
                "path": {"type": "string", "description": "Optional HTTP path to GET, e.g. \"/\" or \"/api/health\". Omit for a plain TCP connect check."},
                "expect": {"type": "string", "description": "Optional text the response body MUST contain (needs a path). A page that answers 200 without it counts as down — the difference between a working UI and a login page or an error stub."}
            },
            "required": ["host", "port"]
        }),
        move |params: Value| {
            async move {
                let host = require_str(&params, "host")?;
                // THIS DOOR'S WIRE FACTS: is the port field there at all, and is it a number. WHICH
                // NUMBER NAMES A SERVICE is `TargetSpec::parse`'s rule — this closure used to carry
                // its own copy of the range check and its own wording for it, which is how the same
                // mistake came to answer differently here and through the HTTP form. Even the
                // absent-field sentence is monitor.rs's, for the same reason.
                let port = params.get("port").and_then(|p| p.as_u64()).ok_or_else(|| {
                    DeviceError::InvalidParams {
                        message: crate::monitor::PORT_REQUIRED_REASON.into(),
                    }
                })?;
                let path = params.get("path").and_then(|p| p.as_str()).unwrap_or("");
                let expect = params.get("expect").and_then(|p| p.as_str()).unwrap_or("");
                // ONE VALIDATOR, THEN THE STORE. `parse` is the door's door (the rules, with a
                // reason a person reads); `add_target_full` is the store's. The MCP ENVELOPE stays
                // this door's own — the HTTP form answers `{"ok":false,…,"code":"invalid_params"}`
                // and that difference is the transports', not the rules'.
                let spec = crate::monitor::TargetSpec::parse(&host, port, path, expect)
                    .map_err(|reason| DeviceError::InvalidParams { message: reason })?;
                match crate::monitor::add_target_full(&crate::paths::data_dir(), &spec) {
                    Ok(t) => Ok(json!({
                        "ok": true,
                        "target": t,
                        "note": "watching; the first reading appears within one probe interval (or call monitor_probe now)",
                    })),
                    // The reason is written for a person ("a host is required", "at most N
                    // targets") and is passed through rather than wrapped.
                    Err(reason) => Err(DeviceError::InvalidParams { message: reason }),
                }
            }
        },
    )
}

fn tool_remove() -> ToolDef {
    ToolDef::new(
        "monitor_remove",
        "Stop watching a target. `removed` says whether anything was being watched under that id — removing one that is not there is reported as a no-op, never as a success. \
         Removing a watch DISCARDS its series: the record is gone, not hidden.",
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The target id from monitor_list (\"host:port\")."}
            },
            "required": ["id"]
        }),
        move |params: Value| {
            async move {
                let id = require_str(&params, "id")?;
                let removed = crate::monitor::remove_target(&crate::paths::data_dir(), &id);
                Ok(json!({"ok": true, "removed": removed}))
            }
        },
    )
}

fn tool_probe() -> ToolDef {
    ToolDef::new(
        "monitor_probe",
        "Probe one watched target RIGHT NOW and return the result plus the refreshed summary — the synchronous half of the instrument, against the 15 s timer that runs on its own. \
         Use it as a BEFORE and AFTER around anything that could take a host down or bring it back: probe, act, probe. \
         It is recorded in the series like any other probe, so the pair also becomes part of what the operator sees.",
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The target id from monitor_list (\"host:port\")."}
            },
            "required": ["id"]
        }),
        move |params: Value| {
            async move {
                let id = require_str(&params, "id")?;
                match crate::monitor::probe_once(&id).await {
                    // The answer carries the CRITERION it was judged against: `expect_ok: false`
                    // is unreadable without the text the probe wanted, so the target's own
                    // expectation travels with the result.
                    Some(probe) => {
                        // ONE ENVELOPE, ONE PLACE (round 218): this block used to be copied verbatim
                        // into the HTTP route, which is how the two came to disagree about `expect`.
                        Ok(crate::monitor::probe_envelope(&id, probe))
                    }
                    None => Err(DeviceError::InvalidParams {
                        message: format!("not watching {id} — monitor_list shows what is"),
                    }),
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool<'a>(tools: &'a [ToolDef], name: &str) -> &'a ToolDef {
        tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("missing tool: {name}"))
    }

    /// THE MCP DOOR'S OWN ENVELOPE — and the FIRST test this file has had. Its siblings carry 7 to
    /// 28; this one carried none, which is how a second copy of the port rules (and of their
    /// wording) survived here while `monitor.rs` had the original.
    ///
    /// WHAT IS PINNED IS THE TRANSLATION, NOT THE RULE. The rules are `monitor::TargetSpec::parse`'s
    /// and are pinned there without a filesystem; what this file owes is that a refusal arrives as
    /// `DeviceError::InvalidParams` carrying EXACTLY the validator's sentence — not a paraphrase
    /// written here. So the expected text is asked of the validator, and the case that is spelled
    /// out in full is the port one, which is the sentence the two doors used to disagree about.
    ///
    /// No data dir and no state: every case is refused before `add_target_full` is reached.
    #[tokio::test]
    async fn a_refused_add_answers_in_the_mcp_envelope_with_the_validators_own_reason() {
        let tools = build();
        let add = tool(&tools, "monitor_add");
        let cases = [
            // The number the two doors disagreed about: the HTTP form said "a port is required"
            // with no examples, this door appended them. One sentence now, and this is it.
            (
                json!({"host": "192.0.2.1", "port": 0}),
                crate::monitor::PORT_REQUIRED_REASON.to_string(),
            ),
            // …including when the field is not there at all, or is not a number: both are wire
            // facts THIS door establishes, and both answer with the same sentence.
            (
                json!({"host": "192.0.2.1"}),
                crate::monitor::PORT_REQUIRED_REASON.to_string(),
            ),
            (
                json!({"host": "192.0.2.1", "port": "22"}),
                crate::monitor::PORT_REQUIRED_REASON.to_string(),
            ),
            // The range check that now lives in `TargetSpec::parse` (once, for both doors).
            (
                json!({"host": "192.0.2.1", "port": 65536}),
                "65536 is not a port".to_string(),
            ),
            // A rule from `validate_target`, shown verbatim rather than restated here.
            (
                json!({"host": "a/b", "port": 22}),
                crate::monitor::validate_target("a/b", 22).unwrap_err(),
            ),
            // The refusal `parse` owns: an expectation with no path has no body to read.
            (
                json!({"host": "192.0.2.1", "port": 22, "expect": "OpenWrt"}),
                crate::monitor::TargetSpec::parse("192.0.2.1", 22, "", "OpenWrt").unwrap_err(),
            ),
        ];
        for (params, reason) in cases {
            let err = add
                .handler
                .call(params.clone())
                .await
                .expect_err("a bad add must be refused, not answered with ok:true");
            assert_eq!(err.code(), "invalid_params", "{params}: {err}");
            // `DeviceError`'s Display is "Invalid parameters: {message}" — the MESSAGE is what the
            // person (and the MCP client) reads, so that is what must be the validator's.
            assert!(
                err.to_string().ends_with(&reason),
                "{params}: the door paraphrased the validator: {}",
                err
            );
        }
    }
}
