//! Tool definitions for the monitor plugin.
//!
//! Four tools over ONE instrument (`crate::monitor`): read the watches, take a probe now, and
//! add or remove a watch. Nothing here re-derives a rule the panel also has — `summary`,
//! `series` and `add_target` are the module's own, so an AI reading `monitor_list` and an
//! operator reading the Reachability card see the same numbers (round 261's whole point).
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
                let port = params
                    .get("port")
                    .and_then(|p| p.as_u64())
                    .ok_or_else(|| DeviceError::InvalidParams {
                        message: "a port is required (22 for SSH, 80 for a web UI, …)".into(),
                    })?;
                if port == 0 || port > 65535 {
                    return Err(DeviceError::InvalidParams {
                        message: format!("{port} is not a port"),
                    });
                }
                let path = params.get("path").and_then(|p| p.as_str()).unwrap_or("");
                let expect = params.get("expect").and_then(|p| p.as_str()).unwrap_or("");
                match crate::monitor::add_target_full(&crate::paths::data_dir(), &host, port as u16, path, expect) {
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
                        let expect = crate::monitor::targets()
                            .into_iter()
                            .find(|t| t.id == id)
                            .and_then(|t| t.expect);
                        Ok(json!({
                            "ok": true,
                            "probe": probe,
                            "expect": expect,
                            "summary": crate::monitor::summary(&id),
                        }))
                    }
                    None => Err(DeviceError::InvalidParams {
                        message: format!("not watching {id} — monitor_list shows what is"),
                    }),
                }
            }
        },
    )
}
