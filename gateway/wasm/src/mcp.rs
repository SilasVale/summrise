//! THE CONSOLE'S MCP ENDPOINT — `/mcp`, the JSON-RPC surface a model drives a device through.
//!
//! MOVED FROM `gateway/src/mcp.ts` (417 lines) plus the dispatch half of `plugins/mcp.ts`. What is here is the
//! ORDER of the decisions: authenticate, transport, parse, method, tool, device, route — and the two routing
//! arms (`mcp_browser.rs` for the playwright bridge, this file for everything the DEVICE serves itself).
//!
//! **THE TABLE IS NOT HERE.** `mcp_tools.rs` owns the catalogue and the routing PARTITION is one predicate
//! ([`is_device_direct_tool`]) — which is the defect this landing removes: the TypeScript held the catalogue in
//! one file and the partition in another, and a name in only the first was registered-but-uncallable, failing
//! at call time with `No route for registered tool …`. `mcp_tools::tests::every_tool_has_a_route` is the gate.
//!
//! **THE TWO TRANSPORTS, AND WHY GET IS NOT A 405.** Claude Code v2.1.84+ probes `GET /mcp` first and treats a
//! 405 as server failure, so GET answers a keep-alive SSE stream — 15 s of `: keepalive\n\n`, for as long as the
//! client holds it — and every other verb but POST is the 405 the JSON-RPC spec allows. The stream is
//! `Response::from_stream` over an `unfold` that never ends; the source's `setInterval` and its `cancel()`
//! teardown are the same shape, and the `ReadableStream`'s cancel is what the runtime's body-drop does here.
//!
//! **THE SELF-HEAL IS THE REASON THIS FILE IS MORE THAN A PROXY.** An agent restart wipes the device's PTY
//! registry while MCP clients keep holding old `session_id`s; the source's comment measures the cost (153 wasted
//! calls a week bounced "Session not found" back to the model with no recovery path). So a `SESSION_NOT_FOUND`
//! on a session-taking tool lists the device's live sessions and retargets ONCE when exactly one exists; with
//! several it returns the list, with none it points at `terminal_open`, and `terminal_close` on a dead session
//! is a SUCCESS because the intent was already satisfied.
//!
//! **AND WHAT THE SOURCE DID NOT DO, WHICH THIS PORT DOES NOT DO EITHER.** `findUserByToken` resolves the
//! presented token; `/mcp` requires `role === "admin"` and an ENABLED user, so a relay-role token (ADR-0007's
//! scoped downgrade) is refused here — that is the point of the downgrade, and the corpus has a case for it.

use std::time::Duration;

use serde_json::{json, Map, Value};
use worker::*;

use crate::device::{device_fetch, DialInit};
use crate::device_registry::{js_falsy, js_to_string, js_to_string_of, js_truthy};
use crate::device_store::{get_device, list_devices};
use crate::mcp_errors::{
    ToolError, ToolFailure, DEVICE_UNREACHABLE, SESSION_BUSY, SESSION_NOT_FOUND, TIMEOUT, TOOL_ERROR,
};
use crate::mcp_tools;
use crate::user_store;
use crate::RouteFailure;

/// `PARAMETER RENAMES APPLIED BEFORE FORWARDING` (round 227).
///
/// The device names `terminal_execute`'s first parameter `command`; the console's MCP surface calls it `input`
/// and rewrites it here. It is a TABLE rather than an `if` because the source's contract test restated it byte
/// identically in three places — and a fourth once one caller needed it — with nothing comparing any copy to
/// the code. There is one copy now, and `gateway/test/mcp-handler.test.mjs` still imports it from the
/// TypeScript (which is the rollback and is still the shipping implementation until the deploy).
pub fn param_renames(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "terminal_execute" => &[("input", "command")],
        _ => &[],
    }
}

/// Does this request belong to the family at all? Used by the front door's cutover AND by `handle`, so a path
/// that is handed over is a path that is served.
///
/// ONE PATH, AND IT IS NOT A PREFIX: `/mcp` is the whole of this family. `plugins/mcp.ts`'s OTHER route
/// (`GET /api/plugins/status`) is deliberately NOT here — its response carries `routes`, the console plugin
/// registry's own dispatch instrumentation (`routeStats(ctx)`: a registration index and a per-isolate hit
/// counter for every route of every plugin). Those counters are a property of the TypeScript plugin table and
/// of the traffic ONE isolate has seen; a Rust worker cannot reproduce them without porting the plugin
/// framework, and it must not guess them. The route stays on the TypeScript path, and the harness's cutover
/// section proves that it does.
pub fn in_family(_method: &Method, path: &str) -> bool {
    path == "/mcp"
}

/// **THE ROUTING PARTITION** — a tool name is DEVICE-DIRECT when the gateway relays it to the agent's own
/// `/api/tools/<name>`, and everything else registered goes through the playwright bridge.
///
/// The prefixes are the source's, and the two `browser_*` names are individual because they are the bundled
/// playwright RUNNER on the device rather than the bridge (round-161: the bridge rejects them). The prefix form
/// is safe because the DEVICE's registry is the contract: a `run_*` or `monitor_*` name the device does not
/// serve fails AT THE DEVICE, which is the honest place for it to fail.
pub fn is_device_direct_tool(name: &str) -> bool {
    name.starts_with("terminal_")
        || name.starts_with("secret_")
        || name.starts_with("system_")
        || name.starts_with("run_")
        || name.starts_with("monitor_")
        || name == "browser_pw_info"
        || name == "browser_run_script"
}

/* ─────────────────────────── the JSON-RPC envelopes ─────────────────────────── */

/// A JSON body with the ONE header the source's envelopes set (`content-type: application/json`).
///
/// **NOT `crate::json`**, and the difference is observable: `http.ts`'s `jsonOk` also stamps the static
/// `CORS_HEADERS`, while `mcpJson`/`mcpError` build a bare `Response` — the per-request ACAO is added by the
/// dispatch layer's `withCors` (and by this worker's own front door), never by the route.
fn json_response(body: String, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response.headers_mut().set("content-type", "application/json")?;
    if status != 200 {
        response = response.with_status(status);
    }
    Ok(response)
}

/// `mcpJson(result, id)` → `{jsonrpc, result, id}`, with the `id` key OMITTED when the request carried none
/// (`JSON.stringify` drops an `undefined` property, and a request without an `id` is exactly that).
fn mcp_json(result: Value, id: Option<&Value>) -> Result<Response> {
    let mut envelope = Map::new();
    envelope.insert("jsonrpc".to_string(), json!("2.0"));
    envelope.insert("result".to_string(), result);
    if let Some(id) = id {
        envelope.insert("id".to_string(), id.clone());
    }
    json_response(serde_json::to_string(&Value::Object(envelope))?, 200)
}

/// `mcpError(code, message, id, data)` → `{jsonrpc, error: {code, message[, data]}, id}`, HTTP 200.
///
/// `data` is the STABLE CODE (`e.code`), wrapped as `{code: …}` — which is the whole point of the family: a
/// flat `-32603 Tool failed: …` hides whether the device is offline, the session is gone or a call timed out.
fn mcp_error(
    code: i64,
    message: &str,
    id: Option<&Value>,
    data: Option<&str>,
) -> Result<Response> {
    let mut error = Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), json!(message));
    if let Some(code) = data {
        error.insert("data".to_string(), json!({ "code": code }));
    }
    let mut envelope = Map::new();
    envelope.insert("jsonrpc".to_string(), json!("2.0"));
    envelope.insert("error".to_string(), Value::Object(error));
    if let Some(id) = id {
        envelope.insert("id".to_string(), id.clone());
    }
    json_response(serde_json::to_string(&Value::Object(envelope))?, 200)
}

/// `mcpStatusError(code, message, status)` — the transport-level refusals (401/405/400), whose `id` is an
/// explicit `null` because there is no parsed request to take one from.
fn mcp_status_error(code: i64, message: &str, status: u16) -> Result<Response> {
    let error = json!({ "code": code, "message": message });
    let envelope = json!({ "jsonrpc": "2.0", "error": error, "id": Value::Null });
    json_response(serde_json::to_string(&envelope)?, status)
}

/* ─────────────────────────── the endpoint ─────────────────────────── */

pub async fn handle(mut request: Request, env: &Env) -> Result<Response, RouteFailure> {
    let auth = request
        .headers()
        .get("authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let token = auth
        .strip_prefix("Bearer ")
        .map(|rest| rest.trim().to_string())
        .unwrap_or_default();
    let user = if token.is_empty() {
        None
    } else {
        user_store::find_user_by_token(env, &token).await
    };
    // `!user || !user.enabled || user.role !== "admin"` — the enabled test is JS truthiness, so a record whose
    // `enabled` is missing, `0` or `""` is refused, and the role test is STRICT (a relay token resolves to
    // `"relay"` and is refused here by design).
    let authorized = user.as_ref().is_some_and(|u| {
        !js_falsy(u.get("enabled").unwrap_or(&Value::Null))
            && u.get("role").and_then(Value::as_str) == Some("admin")
    });
    if !authorized {
        return Ok(mcp_status_error(
            -32001,
            "Unauthorized: admin token required",
            401,
        )?);
    }

    if request.method() == Method::Get {
        return Ok(mcp_sse_stream()?);
    }
    if request.method() != Method::Post {
        return Ok(mcp_status_error(-32600, "Method not allowed", 405)?);
    }

    let text = request.text().await.unwrap_or_default();
    let Ok(body) = serde_json::from_str::<Value>(&text) else {
        return Ok(mcp_status_error(-32700, "Parse error", 400)?);
    };
    // `const { method, params, id } = body` — destructuring a LITERAL `null` throws, and the shipping front
    // door's own catch answers that 500. Every other JSON value destructures (a string or a number has no
    // `method`, which is the `Method not found: undefined` arm below).
    if body.is_null() {
        return Err(RouteFailure::Threw);
    }

    let method = body.get("method");
    let params = body.get("params");
    let id = body.get("id");
    let name_of_method = method.and_then(Value::as_str);

    if name_of_method == Some("initialize") {
        // `params?.protocolVersion || "2025-03-26"` — a truthy value is forwarded AS IT IS (a number stays a
        // number), and everything falsy takes the default.
        let protocol_version = params
            .and_then(|p| p.get("protocolVersion"))
            .filter(|v| js_truthy(v))
            .cloned()
            .unwrap_or_else(|| json!("2025-03-26"));
        let result = json!({
            "protocolVersion": protocol_version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "summrise-gate", "version": "0.1.0" },
        });
        return Ok(mcp_json(result, id)?);
    }
    if name_of_method == Some("notifications/initialized")
        || name_of_method == Some("notifications/cancelled")
    {
        // JSON-RPC 2.0 notifications are not answered; streamable HTTP: 202, empty body, no headers at all.
        return Ok(Response::empty()?.with_status(202));
    }
    if name_of_method == Some("ping") {
        return Ok(mcp_json(json!({}), id)?);
    }
    if name_of_method == Some("tools/list") {
        return Ok(mcp_json(mcp_tools::tools_result(), id)?);
    }
    if name_of_method == Some("tools/call") {
        let tool_name = params.and_then(|p| p.get("name"));
        // `arguments` is spread as it is; `args?.device` is the raw value, so a non-string device name can
        // never match a record (`getDevice` compares with `===`).
        let args = params.and_then(|p| p.get("arguments"));
        let Some(tool) = tool_name.and_then(Value::as_str).and_then(mcp_tools::find) else {
            return Ok(mcp_error(
                -32602,
                &format!("Unknown tool: {}", js_to_string_of(tool_name)),
                id,
                None,
            )?);
        };

        let device_name = args.and_then(|a| a.get("device"));
        let mut device = match device_name.and_then(Value::as_str) {
            Some(name) => get_device(env, name).await,
            None => None,
        };
        if device.is_none() {
            let all = list_devices(env).await;
            let named = device_name.is_some_and(js_truthy);
            if !named && all.len() == 1 {
                // round-160/audit-I6a: the fallback applies ONLY when the caller named NO device, so a typo'd
                // name can never execute on the one registered device.
                device = Some(all[0].clone());
            } else if !named && all.is_empty() {
                return Ok(mcp_error(
                    -32602,
                    "No devices registered — register one on the console Devices page first",
                    id,
                    None,
                )?);
            } else if !named {
                return Ok(mcp_error(
                    -32602,
                    &format!(
                        "Multiple devices registered — specify device: {}",
                        join_names(&all)
                    ),
                    id,
                    None,
                )?);
            } else {
                return Ok(mcp_error(
                    -32602,
                    &format!(
                        "Unknown device: {}. Registered devices: {}",
                        js_to_string(device_name.unwrap_or(&Value::Null)),
                        join_names(&all)
                    ),
                    id,
                    None,
                )?);
            }
        }
        let device = device.unwrap_or(Value::Null);

        return match call_tool(tool.name, env, &device, args).await {
            Ok(result) => Ok(mcp_json(json!({ "content": format_result(&result) }), id)?),
            Err(ToolFailure::Coded(error)) => Ok(mcp_error(
                -32603,
                &format!("Tool {} failed: {}", tool.name, error.message),
                id,
                Some(error.code),
            )?),
            // A path that THREW a plain error (the source's `TypeError`, the bridge's `mcp_client_call
            // failed`) carries no code, and the client sees no `data` — which is the distinction the source's
            // `if (data)` makes.
            Err(ToolFailure::Plain(message)) => Ok(mcp_error(
                -32603,
                &format!("Tool {} failed: {message}", tool.name),
                id,
                None,
            )?),
        };
    }
    Ok(mcp_error(
        -32601,
        &format!("Method not found: {}", js_to_string_of(method)),
        id,
        None,
    )?)
}

/// `all.map(d => d.name).join(", ")` — `Array.prototype.join` renders a missing or null name as an EMPTY
/// string rather than as "undefined", which is what the two "specify device" refusals carry.
fn join_names(devices: &[Value]) -> String {
    devices
        .iter()
        .map(|d| match d.get("name") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => js_to_string(other),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `callTool(tool, env, device, args)` — ONE routing rule, and a registered-but-unrouted name is a programming
/// error rather than an empty result.
async fn call_tool(
    name: &str,
    env: &Env,
    device: &Value,
    args: Option<&Value>,
) -> Result<Value, ToolFailure> {
    if is_device_direct_tool(name) {
        return call_terminal_tool(name, env, device, args).await;
    }
    if name.starts_with("browser_") {
        return crate::mcp_browser::call_bridge(name, env, device, args).await;
    }
    Err(ToolFailure::Coded(ToolError::new(
        TOOL_ERROR,
        format!("No route for registered tool {name}"),
    )))
}

/// `{...v}` for the values a request body can carry: an object's own entries, an array's INDEX keys, a string's
/// per-code-unit keys, and nothing at all for a primitive.
///
/// A STRING SPREAD IS UTF-16 IN JAVASCRIPT and code points here, so an argument body that is a string
/// containing an astral character spreads to two lone surrogates there and to one character here. That is a
/// divergence in a shape no client sends, and it is stated rather than discovered.
fn spread_of(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, item)| (i.to_string(), item.clone()))
            .collect(),
        Some(Value::String(text)) => text
            .chars()
            .enumerate()
            .map(|(i, c)| (i.to_string(), Value::String(c.to_string())))
            .collect(),
        _ => Map::new(),
    }
}

/// `{...base, k: v}` — an existing key KEEPS ITS POSITION, a new one is appended.
fn spread_with(base: Map<String, Value>, overrides: Vec<(String, Value)>) -> Map<String, Value> {
    let mut out = base;
    for (k, v) in overrides {
        out.insert(k, v);
    }
    out
}

/// `callTerminalTool` — the device-direct dispatch with stale-session self-healing.
async fn call_terminal_tool(
    name: &str,
    env: &Env,
    device: &Value,
    args: Option<&Value>,
) -> Result<Value, ToolFailure> {
    let failure = match call_terminal_tool_once(name, env, device, args).await {
        Ok(value) => return Ok(value),
        Err(failure) => failure,
    };
    // `if (e.code !== SESSION_NOT_FOUND || !args?.session_id) throw e;`
    let ToolFailure::Coded(error) = &failure else {
        return Err(failure);
    };
    if error.code != SESSION_NOT_FOUND {
        return Err(failure);
    }
    let Some(session) = args.and_then(|a| a.get("session_id")).filter(|v| js_truthy(v)) else {
        return Err(failure);
    };
    if name == "terminal_close" {
        // The intent was already satisfied: the session is gone, so there is nothing to close.
        return Ok(json!({
            "ok": true,
            "note": format!(
                "session {} was already gone (agent restart?) — nothing to close",
                js_to_string(session)
            ),
        }));
    }
    let list = call_terminal_tool_once("terminal_list", env, device, None).await?;
    // `(list?.result || list?.sessions || [])` — a truthy non-array here THROWS in the source (`.map` is not a
    // function on a number or an object). No device answers that shape, and the honest reading of "no list" is
    // an empty one, which is the arm that points at `terminal_open`.
    let source = list
        .get("result")
        .filter(|v| js_truthy(v))
        .or_else(|| list.get("sessions").filter(|v| js_truthy(v)));
    let live: Vec<String> = match source {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|s| s.get("id"))
            .filter(|v| js_truthy(v))
            .map(js_to_string)
            .collect(),
        _ => Vec::new(),
    };
    // `live[0] !== args.session_id` — STRICT, so a numeric `session_id` of 5 is not the live session "5" and
    // the call IS retargeted (the source's own comparison, kept rather than tidied).
    let same_as_presented = matches!(session, Value::String(s) if live.first() == Some(s));
    if live.len() == 1 && !same_as_presented {
        let retargeted = spread_with(
            spread_of(args),
            vec![("session_id".to_string(), Value::String(live[0].clone()))],
        );
        let data = call_terminal_tool_once(name, env, device, Some(&Value::Object(retargeted))).await?;
        let mut out = spread_of(Some(&data));
        out.insert(
            "note".to_string(),
            Value::String(format!(
                "session_id {} was stale (agent restart?) — call retargeted to the live session {}",
                js_to_string(session),
                live[0]
            )),
        );
        return Ok(Value::Object(out));
    }
    if live.is_empty() {
        return Err(ToolFailure::Coded(ToolError::new(
            SESSION_NOT_FOUND,
            format!(
                "session {} not found and the device has no live sessions — open one with terminal_open first",
                js_to_string(session)
            ),
        )));
    }
    Err(ToolFailure::Coded(ToolError::new(
        SESSION_NOT_FOUND,
        format!(
            "session {} not found. Live sessions on {}: {} — pass one of these as session_id",
            js_to_string(session),
            js_to_string_of(device.get("name")),
            live.join(", ")
        ),
    )))
}

/// `callTerminalToolOnce` — the relay: `/api/tools/<name>` with the renames applied, the device's own `ok`
/// flag read, and the failure mapped to one of the five stable codes.
async fn call_terminal_tool_once(
    name: &str,
    env: &Env,
    device: &Value,
    args: Option<&Value>,
) -> Result<Value, ToolFailure> {
    if !is_device_direct_tool(name) {
        return Err(ToolFailure::Plain(format!("Unknown device tool: {name}")));
    }
    let tool_path = format!("/api/tools/{name}");
    let mut body = spread_of(args);
    // **`shift_remove`, NOT `remove`.** A `serde_json::Map` under `preserve_order` is an `IndexMap`, and its
    // `remove` is a `swap_remove` — which moves the LAST entry into the hole and silently reorders the body.
    // The devices slice lost a byte comparison to exactly that, and this body's key order is on the wire.
    body.shift_remove("device");
    for (from, to) in param_renames(name) {
        if let Some(value) = body.get(*from).cloned() {
            body.insert((*to).to_string(), value);
            body.shift_remove(*from);
        }
    }
    if name == "terminal_execute" {
        // The default must match the agent's own 200 ms — a gateway-side 400 invented a different quiet window
        // than the device actually uses (round-54). `??` is NULLISH: an explicit 0 is kept.
        if body.get("quiet_ms").is_none_or(Value::is_null) {
            body.insert("quiet_ms".to_string(), json!(200));
        }
    }
    // `JSON.stringify(body)` — and `stringify_like_json` is that function for the values a body holds (a whole
    // float prints as a whole number), which is the crate's existing answer to the same question.
    let payload = crate::responses::stringify_like_json(&Value::Object(body));

    let hostname = js_to_string_of(device.get("hostname"));
    let token = js_to_string_of(device.get("token"));
    let dial = device_fetch(env, &hostname, &token, &tool_path, DialInit::post_json(&payload)).await;
    // The source's `new URL` throw, which is not a refusal and carries no code.
    if let Some(message) = dial.threw {
        return Err(ToolFailure::Plain(message));
    }
    let Some(mut resp) = dial.resp else {
        let error = dial
            .error
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "Device unreachable".to_string());
        // round-55: a timeout and a hard unreachable are DIFFERENT codes, and the source tells them apart by
        // the message `fetchWithTimeout` wrote.
        let code = if error.to_lowercase().contains("timeout") {
            TIMEOUT
        } else {
            DEVICE_UNREACHABLE
        };
        return Err(ToolFailure::Coded(ToolError::new(code, error)));
    };
    let status = resp.status_code();
    let text = resp.text().await.unwrap_or_default();
    // `await resp.json().catch(() => null)` — and a body that IS the four bytes `null` parses to `null`, which
    // the source treats as "no parseable body" rather than as an empty object the device sent.
    let data = serde_json::from_str::<Value>(&text)
        .ok()
        .filter(|v| !v.is_null());
    let Some(data) = data else {
        return Err(ToolFailure::Coded(ToolError::new(
            TOOL_ERROR,
            format!(
                "Device returned {status} with a body that is not JSON (an empty or truncated reply, or a \
                 proxy's error page). Nothing was executed as far as this call can tell."
            ),
        )));
    };
    // round-58: `deviceFetch`'s `ok` is the HTTP status, and the agent returns tool errors as HTTP 200 +
    // `{"ok":false,…}` — so the status alone would sail a failure back as a successful result.
    if !dial.ok || data.get("ok") == Some(&Value::Bool(false)) {
        let msg = match data.get("error") {
            Some(value) if js_truthy(value) => js_to_string(value),
            _ => format!("Device returned {status}"),
        };
        let code_value = data.get("code");
        let code = match code_value.and_then(Value::as_str) {
            Some("session_not_found") => SESSION_NOT_FOUND,
            Some("session_busy") => SESSION_BUSY,
            Some("ssh_timeout") => TIMEOUT,
            // round-64: the agent ships NINE typed codes but only three were mapped, so a device UP and
            // reporting `serial_port_not_found` read as "device offline" and sent clients on a
            // device-recovery detour. Any OTHER typed code is a device-up tool failure.
            Some(_) => TOOL_ERROR,
            None if code_value.is_some_and(js_truthy) => TOOL_ERROR,
            None => {
                let lower = msg.to_lowercase();
                if lower.contains("session not found") {
                    SESSION_NOT_FOUND
                } else if lower.contains("session busy") {
                    SESSION_BUSY
                } else if lower.contains("timed out") {
                    TIMEOUT
                } else {
                    DEVICE_UNREACHABLE
                }
            }
        };
        return Err(ToolFailure::Coded(ToolError::new(code, msg)));
    }
    Ok(data)
}

/// `formatResult(result)` — the MCP content blocks.
///
/// Three arms, in the source's order, and the KEY ORDER of each is the wire: the `{image:{…}}` arm is
/// `type, data, mimeType`, the data-URL arm is `type, mimeType, data`, and the text arm is `type, text`.
fn format_result(result: &Value) -> Value {
    if let Value::Object(map) = result {
        if let Some(image) = map.get("image").filter(|v| js_truthy(v)) {
            let mut block = Map::new();
            block.insert("type".to_string(), json!("image"));
            // `data: result.image.data` — an ABSENT value is dropped by `JSON.stringify`, an explicit null is
            // not, and the two are different bytes.
            if let Some(data) = image.get("data") {
                block.insert("data".to_string(), data.clone());
            }
            block.insert(
                "mimeType".to_string(),
                match image.get("mimeType") {
                    Some(v) if js_truthy(v) => v.clone(),
                    _ => json!("image/png"),
                },
            );
            return Value::Array(vec![Value::Object(block)]);
        }
        // round-118: the mcp_client bridge returns screenshots as data-URL TEXT (the agent renders image
        // content as "data:image/png;base64,…"); unwrap them back into an MCP image block so the model does not
        // chew on a whole screen of base64 text.
        let data_url = map
            .get("ok")
            .filter(|v| **v == Value::Bool(true))
            .and_then(|_| map.get("result"))
            .and_then(Value::as_str);
        if let Some(text) = data_url.filter(|t| t.starts_with("data:image/")) {
            if let Some((mime, data)) = data_url_parts(text) {
                return Value::Array(vec![json!({ "type": "image", "mimeType": mime, "data": data })]);
            }
        }
    }
    let text = match result {
        Value::String(text) => text.clone(),
        other => crate::responses::stringify_like_json(other),
    };
    Value::Array(vec![json!({ "type": "text", "text": text })])
}

/// `/^data:(image\/[a-z+]+);base64,(.+)$/` — the subtype is lowercase letters and `+` only, and the payload
/// must run to the END of the string with no newline in it (JavaScript's `.` does not match `\n` and its `$`
/// does not match before one).
fn data_url_parts(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("data:image/")?;
    let semi = rest.find(';')?;
    let subtype = &rest[..semi];
    if subtype.is_empty() || !subtype.chars().all(|c| c.is_ascii_lowercase() || c == '+') {
        return None;
    }
    let payload = rest[semi..].strip_prefix(";base64,")?;
    if payload.is_empty() || payload.contains('\n') || payload.contains('\r') {
        return None;
    }
    Some((format!("image/{subtype}"), payload.to_string()))
}

/// `mcpSseStream()` — the keep-alive stream Claude Code's GET probe needs, one `: keepalive\n\n` every 15 s for
/// as long as the client holds it.
///
/// The source keeps its timer in the stream source's closure so `cancel()` can clear it; here the `unfold`'s own
/// state IS that closure, and dropping the body (which is what a cancelled response does) drops the pending
/// `Delay` with it.
fn mcp_sse_stream() -> Result<Response> {
    let stream = futures_util::stream::unfold((), |_| async {
        Delay::from(Duration::from_millis(15_000)).await;
        Some((Ok::<Vec<u8>, Error>(b": keepalive\n\n".to_vec()), ()))
    });
    let mut response = Response::from_stream(stream)?;
    response
        .headers_mut()
        .set("content-type", "text/event-stream")?;
    response.headers_mut().set("cache-control", "no-cache")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `isDeviceDirectTool` — the partition, as the source wrote it. The `browser_*` arm is the bridge, and the
    /// two runner tools are the exceptions that make it a partition rather than a prefix rule.
    #[test]
    fn the_routing_partition_is_the_sources() {
        for name in [
            "terminal_open",
            "secret_set",
            "system_file_upload",
            "run_begin",
            "monitor_probe",
            "browser_pw_info",
            "browser_run_script",
        ] {
            assert!(is_device_direct_tool(name), "{name} is device-direct");
        }
        for name in ["browser_open", "browser_snapshot", "browser_close", "nonsense"] {
            assert!(!is_device_direct_tool(name), "{name} is not device-direct");
        }
    }

    /// `PARAM_RENAMES` — one entry, and it is the one the device's own `required` names.
    #[test]
    fn the_rename_table_is_the_sources() {
        assert_eq!(param_renames("terminal_execute"), &[("input", "command")]);
        assert!(param_renames("terminal_write").is_empty());
    }

    /// **THE BODY'S KEY ORDER IS ON THE WIRE**, and `shift_remove` is what keeps it: `remove` on a
    /// `preserve_order` map is a `swap_remove`, which would move the LAST key into the deleted one's place.
    /// This is the devices slice's own recorded bug, pinned here in the shape this module meets it.
    #[test]
    fn removing_the_device_field_does_not_reorder_the_body() {
        let args = json!({"device": "d1", "session_id": "t1", "lines": 5});
        let mut body = spread_of(Some(&args));
        body.shift_remove("device");
        let keys: Vec<&str> = body.keys().map(String::as_str).collect();
        assert_eq!(keys, ["session_id", "lines"]);
    }

    /// The rename's own shape: the new key takes the old one's POSITION, and a name that is not there is not
    /// invented (`body[to] = undefined` is dropped by `JSON.stringify`, so it is nothing at all).
    #[test]
    fn the_rename_keeps_the_position_and_invents_nothing() {
        let args = json!({"device": "d1", "session_id": "t1", "input": "ls", "timeout_secs": 5});
        let mut body = spread_of(Some(&args));
        body.shift_remove("device");
        for (from, to) in param_renames("terminal_execute") {
            if let Some(value) = body.get(*from).cloned() {
                body.insert((*to).to_string(), value);
                body.shift_remove(*from);
            }
        }
        // **THE RENAMED KEY IS APPENDED, NOT PUT IN THE OLD ONE'S PLACE.** `body[to] = body[from]` creates a
        // NEW property — only an assignment to a key that ALREADY exists keeps its position — so `command`
        // lands after `timeout_secs` and the forwarded body is this, in this order.
        assert_eq!(
            crate::responses::stringify_like_json(&Value::Object(body)),
            r#"{"session_id":"t1","timeout_secs":5,"command":"ls"}"#
        );

        let bare = json!({"device": "d1", "session_id": "t1"});
        let mut body = spread_of(Some(&bare));
        body.shift_remove("device");
        for (from, to) in param_renames("terminal_execute") {
            if let Some(value) = body.get(*from).cloned() {
                body.insert((*to).to_string(), value);
                body.shift_remove(*from);
            }
        }
        assert_eq!(
            crate::responses::stringify_like_json(&Value::Object(body)),
            r#"{"session_id":"t1"}"#
        );
    }

    /// `quiet_ms` is added only when it is NULLISH, and it is appended when it is not already there — the
    /// source's `body.quiet_ms = body.quiet_ms ?? 200`.
    #[test]
    fn the_quiet_default_is_nullish_not_falsy() {
        let mut body = spread_of(Some(&json!({"input": "ls"})));
        if body.get("quiet_ms").is_none_or(Value::is_null) {
            body.insert("quiet_ms".to_string(), json!(200));
        }
        assert_eq!(body.get("quiet_ms"), Some(&json!(200)));

        let mut explicit = spread_of(Some(&json!({"input": "ls", "quiet_ms": 0})));
        if explicit.get("quiet_ms").is_none_or(Value::is_null) {
            explicit.insert("quiet_ms".to_string(), json!(200));
        }
        assert_eq!(explicit.get("quiet_ms"), Some(&json!(0)));

        let mut null = spread_of(Some(&json!({"input": "ls", "quiet_ms": Value::Null})));
        if null.get("quiet_ms").is_none_or(Value::is_null) {
            null.insert("quiet_ms".to_string(), json!(200));
        }
        assert_eq!(null.get("quiet_ms"), Some(&json!(200)));
    }

    /// `formatResult`'s three arms, key order included — the two image arms put their keys in DIFFERENT orders
    /// and both are on the wire.
    #[test]
    fn the_content_blocks_are_the_sources() {
        assert_eq!(
            crate::responses::stringify_like_json(&format_result(&json!({"image": {"data": "AAA", "mimeType": "image/jpeg"}}))),
            r#"[{"type":"image","data":"AAA","mimeType":"image/jpeg"}]"#
        );
        // …the default mime, and the ABSENT data key that `JSON.stringify` drops.
        assert_eq!(
            crate::responses::stringify_like_json(&format_result(&json!({"image": {"mimeType": ""}}))),
            r#"[{"type":"image","mimeType":"image/png"}]"#
        );
        assert_eq!(
            crate::responses::stringify_like_json(&format_result(
                &json!({"ok": true, "result": "data:image/png;base64,QUJD"})
            )),
            r#"[{"type":"image","mimeType":"image/png","data":"QUJD"}]"#
        );
        assert_eq!(
            crate::responses::stringify_like_json(&format_result(&json!({"ok": true, "result": "plain"}))),
            r#"[{"type":"text","text":"{\"ok\":true,\"result\":\"plain\"}"}]"#
        );
        assert_eq!(
            crate::responses::stringify_like_json(&format_result(&json!("a string"))),
            r#"[{"type":"text","text":"a string"}]"#
        );
    }

    /// The data-URL matcher's edges, which are the regex's: an uppercase subtype, an empty payload, a newline
    /// inside it, and a `;base64,` that is not immediately after the subtype.
    #[test]
    fn the_data_url_matcher_is_the_regex() {
        assert_eq!(
            data_url_parts("data:image/png;base64,QUJD"),
            Some(("image/png".to_string(), "QUJD".to_string()))
        );
        assert_eq!(
            data_url_parts("data:image/svg+xml;base64,QQ=="),
            Some(("image/svg+xml".to_string(), "QQ==".to_string()))
        );
        assert_eq!(data_url_parts("data:image/PNG;base64,QQ=="), None);
        assert_eq!(data_url_parts("data:image/png;base64,"), None);
        assert_eq!(data_url_parts("data:image/png;base64,QQ\nQQ"), None);
        assert_eq!(data_url_parts("data:image/png,QQ=="), None);
        assert_eq!(data_url_parts("data:text/plain;base64,QQ=="), None);
    }

    /// `{...v}` for the four shapes a body can be — including the string spread, whose index keys are what
    /// `{..."ab"}` produces.
    #[test]
    fn the_spread_shapes_are_javascripts() {
        assert_eq!(spread_of(Some(&json!({"a": 1}))).len(), 1);
        assert_eq!(spread_of(Some(&json!(["x", "y"]))).get("1"), Some(&json!("y")));
        assert_eq!(spread_of(Some(&json!("ab"))).get("0"), Some(&json!("a")));
        assert!(spread_of(Some(&json!(5))).is_empty());
        assert!(spread_of(Some(&Value::Null)).is_empty());
        assert!(spread_of(None).is_empty());
    }

    /// `join(", ")` renders a missing name as an EMPTY string, not as "undefined".
    #[test]
    fn the_device_list_joins_like_array_join() {
        assert_eq!(
            join_names(&[json!({"name": "d1"}), json!({}), json!({"name": "d3"})]),
            "d1, , d3"
        );
    }

    /// The family predicate: `/mcp` and nothing else — the plugin's other route is deliberately excluded (see
    /// its doc comment), and a path that merely STARTS with the base is not the endpoint.
    #[test]
    fn the_family_is_one_path() {
        assert!(in_family(&Method::Get, "/mcp"));
        assert!(in_family(&Method::Post, "/mcp"));
        assert!(!in_family(&Method::Get, "/mcp/extra"));
        assert!(!in_family(&Method::Get, "/api/plugins/status"));
        assert!(!in_family(&Method::Get, "/api/devices/d1/mcp"));
    }
}
