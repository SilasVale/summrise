//! `GET /api/plugins/status` — the fleet's health, and the route table's own counter.
//!
//! **IT WAS THE LAST PLUGIN ROUTE, AND THE PATH KEEPS ITS OLD NAME ON PURPOSE.** `plugins/status.ts`
//! registered it into the console worker's plugin registry, which is why it is called *plugins* status — but
//! nothing about the answer is a plugin's: it is the DEVICE family's (a probe of every device through its own
//! tunnel) plus the ROUTE TABLE's instrumentation. Both halves are this worker's now, and the path is left
//! alone because the console calls it (`CONSOLE_POLL_MS` in `gateway/ui/src/lib/deviceState.ts`) and a rename
//! would be a second, cosmetic migration.
//!
//! **THE PROBE'S SHAPE IS THE SOURCE'S, INCLUDING ITS TWO CLASSIFICATIONS**, which were each a bug once:
//!
//!   * the 30 s cache keys on `name` and stores the STATE, not a boolean; `?fresh=1` (the console's "check
//!     now") bypasses the READ and still writes, so a burst of clicks costs one probe per click while every
//!     other poller keeps seeing the refreshed state. The round-98 defect is why the comment is here: the TTL
//!     check read `hit.at` while the cache stored `ts`, so the cache NEVER hit and every poll live-probed
//!     every device — the hammering the cache exists to prevent.
//!   * `deviceFetch` NEVER THROWS (`device.rs`, and the same sentence is in its header): it answers
//!     `{status: 502, ok: false, error: "Device unreachable: …"}` on any network failure. So the tunnel/agent
//!     split is read from the RETURNED SHAPE — the 502-with-unreachable string is the tunnel-level case — and
//!     not from a `catch`, which is what round-101 found: the old `try/catch` was unreachable, so a down
//!     device reported `tunnel_up: true`.
//!
//! **AND `routes` IS THE ROUTE TABLE'S OWN INSTRUMENT** (`route_table.rs`): `{index, hits}` per route, the
//! dead-route report round 179 wired up. In the console worker it counted the plugin registry's dispatches;
//! here it counts THIS worker's dispatches, which is the table that actually serves the API now.

use std::cell::RefCell;

use serde_json::{json, Map, Value};
use worker::Env;

use crate::device::DialInit;

/// The probe cache's TTL. The console polls every 60 s, so a request it makes is NEVER served by this cache —
/// the margin exists for a second view and for the manual refresh, and any value comfortably under the poll
/// would do.
const PROBE_TTL_MS: i64 = 30_000;

/// The cache's bound, and its behaviour at the bound is the source's: clear, then insert.
const PROBE_CACHE_CAP: usize = 64;

/// `SEEN_WRITE_INTERVAL_MS` — `touchDeviceSeen` writes at most once an hour per device.
const SEEN_WRITE_INTERVAL_MS: i64 = 60 * 60 * 1000;

thread_local! {
    /// `DEVICE_PROBE_CACHE` — `name → (state, ts)`, per isolate. A `Vec` rather than a `HashMap` because the
    /// bound is 64 and the source's `Map` had no order guarantee to preserve.
    static PROBE_CACHE: RefCell<Vec<(String, Value, i64)>> = const { RefCell::new(Vec::new()) };
}

/// `wireVersion(j)` — **WHICH VERSION FIELD WINS, AND IT IS A RULE BECAUSE IT HAS BEEN BROKEN TWICE.**
///
/// `release` is the npm release (1.2.x, every release); `version` is the Cargo protocol version (1.0.x,
/// FROZEN). The console read `version` once (round-304) and showed v1.0.145 forever, so the outdated badge
/// never cleared. **ABSENT, NOT FABRICATED**: this returns `None` where the panel's `releaseVersion` returns
/// `""` and renders "v?" — a state object saying `version: ""` claims an answer it does not have.
///
/// **THE PANEL'S HALF IS `agent/resources/panel-logic/src/version.rs`** (`releaseVersion`, reached through the
/// panel's `lib/agentVersion.ts` wrapper), and **`agent/tests/device_version_rule.rs` holds the two to one
/// table**: the same gate that held this rule to the TypeScript's has followed it into the worker, which is why
/// the two branches below are spelled out rather than looped over an array.
pub fn wire_version(j: &Value) -> Option<String> {
    // **THE TWO BRANCHES ARE SPELLED OUT, NOT LOOPED, AND THAT IS THE GATE'S BUSINESS**:
    // `agent/tests/device_version_rule.rs` reads the FIRST branch that names either field and asserts it tests
    // `release` — a loop over `["release", "version"]` would hide the order behind an array literal, which is
    // exactly the kind of indirection that let the round-304 defect live (the old reader measured DECLARATION
    // order and stayed green when the two returns were swapped).
    if let Some(release) = j.get("release").and_then(Value::as_str) {
        if !release.is_empty() {
            return Some(release.to_string());
        }
    }
    if let Some(version) = j.get("version").and_then(Value::as_str) {
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }
    None
}

/// `cachedDeviceProbe(env, device, fresh)` — the state the fleet card is built from.
pub async fn cached_device_probe(env: &Env, device: &Value, fresh: bool) -> (Value, bool) {
    let name = device
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let now = crate::device_store::now_ms();
    if !fresh {
        let hit = PROBE_CACHE.with(|c| {
            c.borrow()
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, state, ts)| (state.clone(), *ts))
        });
        if let Some((state, ts)) = hit {
            if now - ts < PROBE_TTL_MS {
                // **THE AGENT VERDICT COMES BACK WITH THE STATE SO THE CALLER NEVER RE-READS ITS OWN
                // PAYLOAD.** `agent_up` is this worker's KEY, not a name read off a device answer, and
                // `agent/tests/wire_fields.rs` scans this file for device-answer names — reading the value
                // back out of the payload would make it indistinguishable from a device field.
                let agent = state
                    .as_object()
                    .and_then(|o| o.values().next())
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                return (state, agent);
            }
        }
    }

    // The two dials read the device row's own host and token, through the same guard stack the device proxy
    // uses (`device_fetch`): the authority check, the hostname equality, the private-IP blocklist, the suffix
    // allowlist, the header hygiene and `redirect: manual`.
    let mut state = Map::new();
    let mut tunnel = false;
    let mut agent = false;
    // **THE PROBE HONOURS THE DEVICE'S TRANSPORT** (`device_fetch_via`): a relay device is reached by name on the
    // deployment's relay. Without this the fleet card showed a relay device as DOWN while it was answering — the
    // console asking the wrong address and reporting the silence as the device's fault.
    let dial = crate::device::device_fetch_via(env, device, "/api/status", DialInit::get()).await;
    // **THE REFUSALS (`400`) ARE NOT "UNREACHABLE"**: the source's `res.status !== undefined` test admits both
    // a refusal and an answer, and only the throw-shaped failure carries the unreachable sentence.
    if !dial.threw.is_some() {
        let tunnel_level = !dial.ok
            && dial
                .error
                .as_deref()
                .is_some_and(|e| e.contains("Device unreachable"));
        tunnel = !tunnel_level;
        agent = dial.ok;
        if dial.ok {
            if let Some(mut resp) = dial.resp {
                let body: Value = resp
                    .text()
                    .await
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null);
                if let Some(version) = wire_version(&body) {
                    state.insert("version".to_string(), Value::String(version));
                }
                // THE ONE VERDICT THE FLEET SHOWS: a device whose last run crashed. Both halves or neither —
                // the console keys a mark on the kind and shows the sentence as its hover.
                if body.get("last_boot_kind").and_then(Value::as_str) == Some("crashed") {
                    if let Some(sentence) = body
                        .get("last_boot")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        state.insert("last_boot_kind".to_string(), json!("crashed"));
                        state.insert("last_boot".to_string(), json!(sentence));
                    }
                }
            }
        }
    }

    // A SECOND CALL, AND ONLY WHEN THE FIRST ANSWERED: a device that is down, or older than `/api/update`,
    // costs one request rather than two.
    if agent {
        let up = crate::device::device_fetch_via(env, device, "/api/update", DialInit::get()).await;
        if up.ok {
            if let Some(mut resp) = up.resp {
                let body: Value = resp
                    .text()
                    .await
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null);
                if let Some(update) = device_update(&body) {
                    state.insert("update".to_string(), update);
                }
            }
        }
    }

    // **THE KEY ORDER THE CONSOLE READS**: `agent_up, tunnel_up, …optional…, checked_at`. The optionals were
    // inserted into `state` first, so the two fixed keys are written here BEFORE it and `checked_at` after.
    let mut out = Map::new();
    out.insert("agent_up".to_string(), json!(agent));
    out.insert("tunnel_up".to_string(), json!(tunnel));
    for (k, v) in state.iter() {
        if k != "agent_up" && k != "tunnel_up" {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert("checked_at".to_string(), json!(now));
    let out = Value::Object(out);

    PROBE_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.len() >= PROBE_CACHE_CAP {
            cache.clear();
        }
        cache.retain(|(n, _, _)| *n != name);
        cache.push((name, out.clone(), now));
    });
    (out, agent)
}

/// The `/api/update` answer, narrowed to the fields the console acts on.
///
/// **EVERY FIELD IS SPREAD-GUARDED, AND AN ABSENT FIELD IS A SENTENCE**: "this device did not report it" is
/// different from a field forwarded as `null` or as `0`, which is why the guards are per-field rather than a
/// shape check on the whole body. `update_available` is the ONE field forwarded as a plain boolean (absent
/// reads as `false`, which is the source's `=== true`).
pub fn device_update(u: &Value) -> Option<Value> {
    if !u.is_object() {
        return None;
    }
    let current = u.get("current").and_then(Value::as_str);
    let latest = u.get("latest").and_then(Value::as_str);
    if current.is_none() && latest.is_none() {
        return None;
    }
    let mut out = Map::new();
    if let Some(current) = current {
        out.insert("current".to_string(), json!(current));
    }
    if let Some(latest) = latest {
        out.insert("latest".to_string(), json!(latest));
    }
    out.insert(
        "update_available".to_string(),
        json!(u.get("update_available") == Some(&Value::Bool(true))),
    );
    out.insert(
        "pinned_to".to_string(),
        match u.get("pinned_to").and_then(Value::as_str) {
            Some(pinned) => json!(pinned),
            None => Value::Null,
        },
    );
    if let Some(busy) = u.get("busy").and_then(Value::as_bool) {
        out.insert("busy".to_string(), json!(busy));
    }
    if let Some(error) = u
        .get("error")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
    {
        out.insert("error".to_string(), json!(error));
    }
    if let Some(checked) = u.get("checked_at").and_then(Value::as_i64) {
        if checked > 0 {
            out.insert("checked_at".to_string(), json!(checked));
        }
    }
    if let Some(attempt) = u.get("last_attempt") {
        if attempt
            .get("at_ms")
            .and_then(Value::as_i64)
            .is_some_and(|at| at > 0)
        {
            out.insert("last_attempt".to_string(), attempt.clone());
        }
    }
    Some(Value::Object(out))
}

/// `touchDeviceSeen(env, name, version)` — **THE WRITE IS BOUNDED ON PURPOSE**: the probe runs every 30 s per
/// device and writing KV per poll would burn the daily write quota, so the record is written ONLY when the
/// agent version changed or the last write is over an hour old. The cheap cached-list check short-circuits
/// before any lock or raw KV read.
pub async fn touch_device_seen(env: &Env, name: &str, version: Option<&str>) {
    let devices = crate::device_store::list_devices(env).await;
    let Some(device) = crate::device_registry::find_device(&devices, name) else {
        return;
    };
    let now = crate::device_store::now_ms();
    let version_changed = version.is_some_and(|v| !v.is_empty())
        && device.get("lastVersion").and_then(Value::as_str) != version;
    let stale_seen = device
        .get("lastSeenAt")
        .and_then(Value::as_i64)
        .is_none_or(|seen| now - seen > SEEN_WRITE_INTERVAL_MS);
    if !version_changed && !stale_seen {
        return;
    }
    crate::device_store::touch_seen(env, name, now, version).await;
}

/// `pluginStatus(request, env)` — the whole answer, in the source's key order.
pub async fn plugin_status(env: &Env, fresh: bool) -> Value {
    let mut devices = Map::new();
    for device in crate::device_store::list_devices(env).await {
        let name = device
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (probe, agent) = cached_device_probe(env, &device, fresh).await;
        if agent {
            touch_device_seen(env, &name, probe.get("version").and_then(Value::as_str)).await;
        }
        devices.insert(name, probe);
    }
    json!({ "devices": Value::Object(devices), "routes": crate::route_table::stats() })
}

/// `GET /api/plugins/status` — one path, one verb.
pub fn in_family(method: &worker::Method, path: &str) -> bool {
    *method == worker::Method::Get && path == "/api/plugins/status"
}

/// The handler: the session, the ADMIN gate, then the payload.
///
/// **THE TWO REFUSALS ARE THIS ROUTE'S OWN STRINGS, NOT THE FAMILY'S**, and that is a fact about the source
/// rather than an oversight here: `plugins/status.ts` answers `401 "Not logged in"` and `403 "Admin only"`,
/// where `require_admin` (which the admin family uses) answers `"Not logged in or session expired"` and
/// `"Admin permission required"`. The route's own comment explains why the 401 exists at all — the console's
/// client treats ANY 401 as a dead session and logs the user out, so a logged-in non-admin who opened the
/// Overview was ejected; 401 means "you are not authenticated", and this user is.
pub async fn handle(
    request: &worker::Request,
    env: &Env,
) -> std::result::Result<Option<worker::Response>, crate::RouteFailure> {
    // **THROWS TRAVEL**: `require_session`'s outer `Err` is the front door's 500, not a refusal.
    let Some(user) = crate::session_gate::require_session(request, env).await? else {
        return Ok(Some(crate::json_error(
            401,
            "Not logged in",
            "authentication_error",
        )?));
    };
    if user.get("role").and_then(Value::as_str) != Some("admin") {
        return Ok(Some(crate::json_error(403, "Admin only", "forbidden")?));
    }
    let url = request.url()?;
    let fresh = url.query_pairs().any(|(k, v)| k == "fresh" && v == "1");
    let payload = plugin_status(env, fresh).await;
    Ok(Some(crate::built_response(crate::responses::json_ok(
        &payload,
        &[],
    ))?))
}

#[cfg(test)]
mod tests {
    //! The pure half — the two rules a port gets wrong silently: which version field wins, and which fields of
    //! an `/api/update` answer are forwarded.

    use super::*;

    #[test]
    fn the_npm_release_wins_over_the_frozen_cargo_version() {
        assert_eq!(
            wire_version(&json!({"release": "1.2.354", "version": "1.0.145"})),
            Some("1.2.354".to_string())
        );
        // A pre-release agent has only the frozen one.
        assert_eq!(
            wire_version(&json!({"version": "1.0.145"})),
            Some("1.0.145".to_string())
        );
        // **ABSENT, NOT FABRICATED** — and not `""`, which would claim an answer.
        assert_eq!(wire_version(&json!({})), None);
        assert_eq!(wire_version(&json!({"release": "", "version": ""})), None);
        assert_eq!(wire_version(&Value::Null), None);
    }

    #[test]
    fn an_update_answer_forwards_only_what_it_actually_carries() {
        // Neither `current` nor `latest` → no update object at all (the source's own guard).
        assert!(device_update(&json!({"busy": true})).is_none());
        assert!(device_update(&Value::Null).is_none());
        // The two version fields, then the rest in the source's order.
        let got = device_update(&json!({
            "current": "1.2.0",
            "latest": "1.2.1",
            "update_available": true,
            "pinned_to": "1.2.0",
            "busy": false,
            "error": "",
            "checked_at": 0,
            "last_attempt": {"at_ms": 0}
        }))
        .expect("an update object");
        assert_eq!(
            serde_json::to_string(&got).unwrap(),
            r#"{"current":"1.2.0","latest":"1.2.1","update_available":true,"pinned_to":"1.2.0","busy":false}"#
        );
        // `pinned_to` is the ONE field that is null rather than absent when the device omits it.
        let bare = device_update(&json!({"latest": "1.2.1"})).expect("an update object");
        assert_eq!(
            serde_json::to_string(&bare).unwrap(),
            r#"{"latest":"1.2.1","update_available":false,"pinned_to":null}"#
        );
        // And the four the verdict cannot be acted on without, when they ARE there.
        let full = device_update(&json!({
            "current": "1.2.0",
            "busy": true,
            "error": "download failed",
            "checked_at": 1700000000000i64,
            "last_attempt": {"at_ms": 1700000000000i64, "ok": false}
        }))
        .expect("an update object");
        assert_eq!(
            serde_json::to_string(&full).unwrap(),
            r#"{"current":"1.2.0","update_available":false,"pinned_to":null,"busy":true,"error":"download failed","checked_at":1700000000000,"last_attempt":{"at_ms":1700000000000,"ok":false}}"#
        );
    }
}
