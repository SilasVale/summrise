//! THE BROWSER BRIDGE — `browser_*` tool calls relayed to the device's playwright-mcp.
//!
//! MOVED FROM `gateway/src/mcp-browser.ts` (184 lines). The bridge is the ONLY browser path left (the
//! extension/PluginHubDO arm was deleted round-341, the extension itself round-243), and it is a bridge rather
//! than a proxy for four reasons the source measures:
//!
//!   * **the dial is RAW, not `deviceFetch`.** `deviceFetch`'s POST bound is 60 s while a legitimate browser
//!     wait runs to ~320 s (`timeout_secs` clamped at 300, plus headroom) — so the SSRF hostname check is
//!     applied here instead of the shared dial, exactly as `mcp-browser.ts` applies `deviceHostError` itself.
//!   * **the call has a TOTAL budget** (`BROWSER_GATEWAY_BUDGET_MS`). The per-fetch budget is per ATTEMPT, and
//!     the old shape stacked attempts serially — invoke + start + connect + retry ≈ 4×320 s of pinned isolate
//!     time on a hung device, with unbounded concurrent calls piling on. Every attempt now draws from ONE
//!     deadline, and the floor of 1 s is what keeps the last attempt firing instead of aborting instantly.
//!   * **the self-heal probes are LIVENESS probes, not work**: `start` and `connect` get
//!     `BROWSER_SELFHEAL_TIMEOUT_MS` (15 s), never the call budget.
//!   * **a per-device semaphore** (`BROWSER_MAX_CONCURRENT_PER_DEVICE`) makes excess calls fail with
//!     `SESSION_BUSY` so MCP clients back off instead of dogpiling. No existing mechanism was reusable: the
//!     breaker guards upstream CHANNEL health through Durable Object state, and `withKeyLock` serializes KV
//!     read-modify-write — neither is an in-flight semaphore.
//!
//! **THE TWO RENAMES ARE THE CONSOLE'S, NOT THE SERVER'S.** `browser_wait`'s `text_gone` becomes the shipped
//! server's `textGone`, and `element_ref` becomes the `{target, element}` protocol playwright-mcp 0.0.79 uses
//! (`target` = a snapshot reference `eN` or a unique selector). Everything else is forwarded VERBATIM, which is
//! why every advertised name must exist in the server (`gateway/test/browser-contract.test.mjs`).
//!
//! **AND `run_id` IS LIFTED OUT OF THE ARGUMENTS.** It belongs to the DEVICE call, not to playwright-mcp: the
//! device reads it from `params.run_id`, and nested inside `arguments` it would reach a server that knows
//! nothing about runs and be dropped — a silently dropped id shows a run with commands and ZERO browser
//! actions, which is indistinguishable from "the AI never used the browser".

use std::sync::Mutex;

use serde_json::{json, Map, Value};
use worker::*;

use crate::device::fetch_with_timeout;
use crate::device::device_host_error;
use crate::device_registry::{js_falsy, js_to_string, js_to_string_of, js_truthy};
use crate::mcp_errors::{ToolError, ToolFailure, DEVICE_UNREACHABLE, SESSION_BUSY};

/// `BROWSER_GATEWAY_BUDGET_MS` — the hard TOTAL cap for one `browser_*` call.
const BROWSER_GATEWAY_BUDGET_MS: i64 = 350_000;
/// `BROWSER_SELFHEAL_TIMEOUT_MS` — start/connect are liveness probes and get a short timeout.
const BROWSER_SELFHEAL_TIMEOUT_MS: i64 = 15_000;
/// `BROWSER_MAX_CONCURRENT_PER_DEVICE` — the isolate-local semaphore's bound.
const BROWSER_MAX_CONCURRENT_PER_DEVICE: usize = 4;

/// `__browserInflight` — module state, exactly like the source's `Map`.
static BROWSER_INFLIGHT: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

/// The slot a call occupies: `String(device?.name || device?.hostname || "default")`.
fn slot_of(device: &Value) -> String {
    match device.get("name") {
        Some(v) if js_truthy(v) => js_to_string(v),
        _ => match device.get("hostname") {
            Some(v) if js_truthy(v) => js_to_string(v),
            _ => "default".to_string(),
        },
    }
}

/// `withBrowserSlot(slot, fn)`'s guard half. `Drop` is the source's `finally`: the count falls whether the call
/// answered, failed or panicked, and the entry is REMOVED at zero rather than left as a growing zero row.
#[derive(Debug)]
struct BrowserSlot(String);

impl Drop for BrowserSlot {
    fn drop(&mut self) {
        let mut table = BROWSER_INFLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = table.iter().position(|(name, _)| *name == self.0) {
            let left = table[index].1.saturating_sub(1);
            if left == 0 {
                table.remove(index);
            } else {
                table[index].1 = left;
            }
        }
    }
}

fn enter_slot(slot: &str) -> Result<BrowserSlot, ToolFailure> {
    let mut table = BROWSER_INFLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    match table.iter_mut().find(|(name, _)| name == slot) {
        Some((_, count)) if *count >= BROWSER_MAX_CONCURRENT_PER_DEVICE => {
            Err(ToolFailure::Coded(ToolError::new(
                SESSION_BUSY,
                format!("too many concurrent browser calls on device {slot} — retry shortly"),
            )))
        }
        Some((_, count)) => {
            *count += 1;
            Ok(BrowserSlot(slot.to_string()))
        }
        None => {
            table.push((slot.to_string(), 1));
            Ok(BrowserSlot(slot.to_string()))
        }
    }
}

/// `device?.name || device?.hostname || "default"` — the slot a call is counted against.
fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// `callMcpClientBridge(name, env, device, args)`.
pub async fn call_bridge(
    name: &str,
    _env: &Env,
    device: &Value,
    args: Option<&Value>,
) -> Result<Value, ToolFailure> {
    // SSRF guard, the same check `deviceFetch` applies (see the module header for why it is not the shared
    // dial): the device record's hostname is dialled with the device Bearer token.
    let hostname = js_to_string_of(device.get("hostname"));
    if let Some(reason) = device_host_error(&hostname) {
        return Err(ToolFailure::Coded(ToolError::new(
            DEVICE_UNREACHABLE,
            reason,
        )));
    }
    // Belt-and-suspenders (mirrors `deviceFetch`): URL-significant characters smuggled into the hostname field
    // must not redirect the dial elsewhere. Both the parse failure and the mismatch answer the source's
    // `invalid device hostname` — its inner `throw new Error("host mismatch")` has no `code`, so the `catch`
    // replaces it.
    let base = match Url::parse(&format!("https://{hostname}/")) {
        Ok(parsed)
            if parsed.host_str().unwrap_or_default().to_lowercase() == hostname.to_lowercase() =>
        {
            format!("https://{hostname}")
        }
        _ => {
            return Err(ToolFailure::Coded(ToolError::new(
                DEVICE_UNREACHABLE,
                "invalid device hostname",
            )))
        }
    };
    let token = js_to_string_of(device.get("token"));

    // ── the argument shape: the renames, the clamp, and the click/type translation ──────────────────
    let mut pm_args = match args {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if name == "browser_wait" {
        if let Some(value) = pm_args.get("text_gone").filter(|v| !v.is_null()).cloned() {
            pm_args.insert("textGone".to_string(), value);
            pm_args.shift_remove("text_gone");
        }
    }
    // `typeof pmArgs.timeout_secs === "number"` — a STRING is deliberately left alone, exactly as the source
    // leaves it (`"5" * 1000` is 5000 in JavaScript and the budget below reproduces that).
    if let Some(value) = pm_args.get("timeout_secs").cloned() {
        if let Some(number) = value.as_f64() {
            if value.is_number() {
                let truncated = number.trunc();
                let clamped = if truncated == 0.0 {
                    1.0
                } else {
                    truncated.clamp(1.0, 300.0)
                };
                pm_args.insert("timeout_secs".to_string(), json!(clamped as i64));
            }
        }
    }
    // `((pmArgs.timeout_secs) || 120) * 1000 + 20_000` — JS coercion, so a numeric string counts and anything
    // that coerces to NaN falls back to 120.
    let budget_secs = match pm_args.get("timeout_secs") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Null) | None => 0.0,
        _ => f64::NAN,
    };
    let call_budget_ms = if budget_secs.is_nan() || budget_secs == 0.0 {
        120_000.0
    } else {
        budget_secs * 1000.0 + 20_000.0
    };
    let slot = slot_of(device);
    let deadline = now_ms() + BROWSER_GATEWAY_BUDGET_MS;
    // Floor 1 s so the final attempt still fires instead of timing out instantly.
    let budget_left = move || std::cmp::max(1000, deadline - now_ms());

    if (name == "browser_click" || name == "browser_type")
        && args.and_then(|a| a.get("element_ref")).is_some_and(|v| !v.is_null())
    {
        let raw = args.and_then(|a| a.get("element_ref")).unwrap();
        let text = js_to_string(raw);
        // `/^e?\d+$/` → `replace(/^(\d+)$/, "e$1")`: a bare number gains the `e` prefix, an already-prefixed
        // ref or any other string is passed through as a selector.
        let target = if !text.is_empty()
            && text
                .strip_prefix('e')
                .unwrap_or(&text)
                .chars()
                .all(|c| c.is_ascii_digit())
            && !text.strip_prefix('e').unwrap_or(&text).is_empty()
        {
            match text.strip_prefix('e') {
                Some(digits) => format!("e{digits}"),
                None => format!("e{text}"),
            }
        } else {
            text
        };
        pm_args.insert("target".to_string(), Value::String(target));
        let element_missing = pm_args.get("element").is_none_or(js_falsy);
        if element_missing {
            pm_args.insert("element".to_string(), json!("target element"));
        }
        pm_args.shift_remove("element_ref");
    }

    let invoke = |pm_args: Map<String, Value>, budget: i64| {
        invoke_once(name, &base, &token, pm_args, budget)
    };

    let _slot = enter_slot(&slot)?;
    let budget_for_invoke = std::cmp::min(call_budget_ms as i64, budget_left());
    let mut out = invoke(pm_args.clone(), budget_for_invoke).await?;
    if out.get("ok") == Some(&Value::Bool(false)) {
        // round-118/132 self-healing: after a device reboot nothing relaunches playwright-mcp and nobody
        // creates a client session, so the first `browser_*` is bound to be "not connected"; playwright-mcp
        // 0.0.79 also reclaims sessions server-side after ~15 s idle. start → connect → retry ONCE.
        if matches_selfheal(&error_text(&out)) {
            let heal = std::cmp::min(BROWSER_SELFHEAL_TIMEOUT_MS, budget_left());
            probe(&format!("{base}/api/plugins/playwright/start"), &token, None, heal).await?;
            probe(
                &format!("{base}/api/tools/mcp_client_connect"),
                &token,
                Some("{}"),
                heal,
            )
            .await?;
            out = invoke(pm_args, std::cmp::min(call_budget_ms as i64, budget_left())).await?;
        }
        if out.get("ok") == Some(&Value::Bool(false)) {
            let message = error_text(&out);
            return Err(ToolFailure::plain(if message.is_empty() {
                "mcp_client_call failed".to_string()
            } else {
                message
            }));
        }
    }
    Ok(out)
}

/// `String(out.error || "")` — the message the self-heal trigger and the final refusal both read.
fn error_text(out: &Value) -> String {
    match out.get("error") {
        Some(value) if js_truthy(value) => js_to_string(value),
        _ => String::new(),
    }
}

/// `invoke()` — ONE attempt: the `mcp_client_call` POST, its arguments forwarded verbatim and `run_id` lifted
/// to the device call's top level.
async fn invoke_once(
    name: &str,
    base: &str,
    token: &str,
    pm_args: Map<String, Value>,
    budget_ms: i64,
) -> Result<Value, ToolFailure> {
    // RUN IDENTITY: the run_id belongs to the DEVICE CALL — lifted OUT of `arguments` to the top level.
    let mut pw_args = pm_args;
    let run_id = pw_args.shift_remove("run_id");
    let mut device_call = Map::new();
    device_call.insert("tool".to_string(), json!(playwright_tool(name)));
    device_call.insert("arguments".to_string(), Value::Object(pw_args));
    if let Some(run_id) = run_id.filter(|v| !v.is_null()) {
        device_call.insert("run_id".to_string(), run_id);
    }
    let body = serde_json::to_string(&Value::Object(device_call)).unwrap_or_default();
    let url = format!("{base}/api/tools/mcp_client_call");
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    init.with_headers(bridge_headers(token).map_err(|e| ToolFailure::plain(e.to_string()))?);
    init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(&body)));
    match fetch_with_timeout(&url, &init, budget_ms.max(0) as u64).await {
        Ok(mut response) => {
            let status = response.status_code();
            let text = response.text().await.unwrap_or_default();
            // The agent's tool API always returns 200 + `{ok:false,error,code}`; the failure information is in
            // the body, so the status alone cannot be trusted. A body that does not parse is the source's
            // `catch` → `mcp_client_call failed: <status>`.
            match serde_json::from_str::<Value>(&text) {
                Ok(value) => Ok(value),
                Err(_) => Err(ToolFailure::plain(format!("mcp_client_call failed: {status}"))),
            }
        }
        // **THE ABORT'S OWN MESSAGE, WHICH IS NOT `fetchWithTimeout`'s.** This dial is bounded by
        // `AbortSignal.timeout(ms)` rather than by `fetchWithTimeout`, and WHATWG's `TimeoutError` DOMException
        // is what `fetch` rejects with — `The operation was aborted due to timeout`. The shared
        // `fetch_with_timeout` answers `timeout after <ms>ms` for the DEVICE dial (where the source really does
        // rename the error); reproducing the wrong one here would put a sentence on the wire that no client of
        // this path has ever seen.
        Err(message) => Err(ToolFailure::plain(match message.strip_prefix("timeout after ") {
            Some(_) => "The operation was aborted due to timeout".to_string(),
            None => message,
        })),
    }
}

/// The gateway name → playwright-mcp name map, with `|| name` for anything not in it.
fn playwright_tool(name: &str) -> &str {
    match name {
        "browser_open" => "browser_navigate",
        "browser_snapshot" => "browser_snapshot",
        "browser_screenshot" => "browser_take_screenshot",
        "browser_click" => "browser_click",
        "browser_type" => "browser_type",
        "browser_wait" => "browser_wait_for",
        "browser_close" => "browser_close",
        other => other,
    }
}

/// `{ "Content-Type": "application/json", Authorization: \`Bearer ${token}\` }`.
fn bridge_headers(token: &str) -> Result<Headers> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("Authorization", &format!("Bearer {token}"))?;
    Ok(headers)
}

/// The self-heal probes: `start` carries no body, `connect` carries `{}`, and neither answer is read (the
/// source does not read them either — they are liveness, and the RETRY is what proves the heal).
async fn probe(url: &str, token: &str, body: Option<&str>, timeout_ms: i64) -> Result<(), ToolFailure> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    init.with_headers(bridge_headers(token).map_err(|e| ToolFailure::plain(e.to_string()))?);
    if let Some(body) = body {
        init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(body)));
    }
    fetch_with_timeout(url, &init, timeout_ms.max(0) as u64)
        .await
        .map(|_| ())
        .map_err(ToolFailure::plain)
}

/// `/not connected|server running|refused|timed out|session not found/i`.
fn matches_selfheal(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["not connected", "server running", "refused", "timed out", "session not found"]
        .iter()
        .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tool map is the console's spelling to the SHIPPED server's, and a name that is not in it is
    /// forwarded unchanged (`|| name`).
    #[test]
    fn the_tool_map_is_the_servers() {
        assert_eq!(playwright_tool("browser_open"), "browser_navigate");
        assert_eq!(playwright_tool("browser_screenshot"), "browser_take_screenshot");
        assert_eq!(playwright_tool("browser_wait"), "browser_wait_for");
        assert_eq!(playwright_tool("browser_click"), "browser_click");
        assert_eq!(playwright_tool("browser_something_new"), "browser_something_new");
    }

    /// `/not connected|server running|refused|timed out|session not found/i` — the five phrases, and a message
    /// that merely CONTAINS "connect" is not one of them.
    #[test]
    fn the_selfheal_trigger_is_the_sources_regex() {
        assert!(matches_selfheal("mcp_client_call: not connected"));
        assert!(matches_selfheal("The server running is fine")); // "server running"
        assert!(matches_selfheal("connection refused"));
        assert!(matches_selfheal("the call timed out"));
        assert!(matches_selfheal("Session not found: abc"));
        assert!(matches_selfheal("NOT CONNECTED"));
        assert!(!matches_selfheal("connection reset by peer"));
        assert!(!matches_selfheal(""));
    }

    /// The slot is `name || hostname || "default"`, and a falsy name falls THROUGH to the hostname.
    #[test]
    fn the_slot_is_name_then_hostname_then_default() {
        assert_eq!(slot_of(&json!({"name": "d1", "hostname": "d1.agent.test"})), "d1");
        assert_eq!(slot_of(&json!({"name": "", "hostname": "d1.agent.test"})), "d1.agent.test");
        assert_eq!(slot_of(&json!({})), "default");
    }

    /// The semaphore's bound and its release: four calls hold the slot, the fifth is `SESSION_BUSY`, and the
    /// count returns to zero — where the entry is REMOVED rather than left behind.
    #[test]
    fn the_semaphore_admits_four_and_removes_the_entry() {
        let slot = "semaphore-test";
        let held: Vec<BrowserSlot> = (0..BROWSER_MAX_CONCURRENT_PER_DEVICE)
            .map(|_| enter_slot(slot).expect("the first four are admitted"))
            .collect();
        let refused = enter_slot(slot).expect_err("the fifth is refused");
        match refused {
            ToolFailure::Coded(error) => {
                assert_eq!(error.code, SESSION_BUSY);
                assert_eq!(
                    error.message,
                    "too many concurrent browser calls on device semaphore-test — retry shortly"
                );
            }
            other => panic!("expected a coded refusal, got {other:?}"),
        }
        drop(held);
        assert!(
            BROWSER_INFLIGHT
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .all(|(name, _)| name != slot),
            "a released slot must not leave a zero row behind"
        );
        assert!(enter_slot(slot).is_ok(), "and it is free again");
    }

    /// `timeout_secs` is clamped to 1..300 and TRUNCATED, and a non-number is left exactly as it arrived —
    /// which is what makes the budget below reproduce JavaScript's coercion.
    #[test]
    fn the_timeout_clamp_is_the_sources() {
        let clamp = |v: Value| -> Value {
            let mut pm = Map::new();
            pm.insert("timeout_secs".to_string(), v);
            if let Some(value) = pm.get("timeout_secs").cloned() {
                if value.is_number() {
                    let truncated = value.as_f64().unwrap_or(0.0).trunc();
                    let clamped = if truncated == 0.0 {
                        1.0
                    } else {
                        truncated.clamp(1.0, 300.0)
                    };
                    pm.insert("timeout_secs".to_string(), json!(clamped as i64));
                }
            }
            pm.get("timeout_secs").cloned().unwrap()
        };
        assert_eq!(clamp(json!(2.7)), json!(2));
        assert_eq!(clamp(json!(0)), json!(1));
        assert_eq!(clamp(json!(5000)), json!(300));
        assert_eq!(clamp(json!(-4)), json!(1));
        assert_eq!(clamp(json!("5")), json!("5"), "a string is not a number");
    }

    /// `element_ref` → the shipped server's `{target, element}` protocol, including the `eN` prefix rule and
    /// the selector pass-through.
    #[test]
    fn the_element_ref_translation_is_the_sources() {
        let translate = |raw: Value| -> Map<String, Value> {
            let mut pm_args = Map::new();
            pm_args.insert("element_ref".to_string(), raw.clone());
            let text = js_to_string(&raw);
            let digits = text.strip_prefix('e').unwrap_or(&text);
            let target = if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                format!("e{digits}")
            } else {
                text.clone()
            };
            pm_args.insert("target".to_string(), Value::String(target));
            if pm_args.get("element").is_none_or(js_falsy) {
                pm_args.insert("element".to_string(), json!("target element"));
            }
            pm_args.shift_remove("element_ref");
            pm_args
        };
        let six = translate(json!(6));
        assert_eq!(six.get("target"), Some(&json!("e6")));
        assert_eq!(six.get("element"), Some(&json!("target element")));
        assert!(six.get("element_ref").is_none());
        assert_eq!(translate(json!("e7")).get("target"), Some(&json!("e7")));
        assert_eq!(translate(json!("#submit")).get("target"), Some(&json!("#submit")));
        // `"e"` alone is not a ref: `^e?\d+$` requires at least one digit.
        assert_eq!(translate(json!("e")).get("target"), Some(&json!("e")));
    }
}
