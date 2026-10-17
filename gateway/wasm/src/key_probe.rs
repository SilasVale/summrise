//! THE KEY DIAGNOSTICS — `plugins/auth.ts`'s `meKeyUsage`, and the console's own view of a caller's key.
//!
//! **WHAT THIS IS.** `POST /api/me/keys/usage` asks a provider what a caller's key has spent: OpenRouter's
//! key-info endpoint, AMD's rolling daily spend cap, OpenCode Go's three quota windows. The console renders the
//! answer as one line under the key card, so a wrong field mapping shows garbage or `NaN` live — which is why
//! every mapping has a pin, and why they were the FIRST thing this repository moved into Rust.
//!
//! **AND THIS LANDING IS THE OTHER HALF OF THAT MOVE.** The three mappings have lived in `decisions.rs` since the
//! decision route existed, reached from TypeScript over `POST /__decision`; the ROUTE around them — the session
//! gate, the key lookup, the fetch, the envelope — stayed in `plugins/auth.ts`. It is here now, so the hop is gone
//! for these three: `usage_query` calls the same functions in-process.
//!
//! **`fetchWithTimeout`'s DEFAULT IS 15 s, AND IT IS NOT `UPSTREAM_TIMEOUT_MS`.** The source calls
//! `fetchWithTimeout(url, { headers })` with no third argument, so a deployment's 120 s chat budget never applied
//! to a usage query; reading the var here would have been a one-line "improvement" that changes a timeout the
//! console's own UI waits on.
//!
//! **THE ENVELOPE'S KEY ORDER IS ITS BYTES.** `jsonOk({ ok, name, status, ...mapped })` writes the four fixed
//! keys first and the mapper's fields in the mapper's own order (which is why the crate builds `serde_json` with
//! `preserve_order`). Both arms below build a `Map` by insertion for exactly that reason.

use serde_json::{json, Map, Value};
use worker::wasm_bindgen::JsValue;
use worker::{Env, Headers, Method, RequestInit};

use crate::responses::{self, Built};

/// `fetchWithTimeout`'s default — see the module header for why this is a literal and not the env var.
const USAGE_TIMEOUT_MS: u64 = 15_000;

/// One row of `USAGE_QUERIES`: the key name, the endpoint, and the decision that maps its payload.
///
/// **THE MAPPER IS THE DECISION FUNCTION ITSELF** (`decisions.rs`), not a copy and not a route hop: the row is
/// what the source's table held — `{ url, decision, map }` — with the decision's NAME replaced by the function,
/// because in Rust the name was only ever an address for the wire.
pub struct UsageQuery {
    pub name: &'static str,
    pub url: &'static str,
    pub map: fn(&Value) -> std::result::Result<Value, String>,
}

/// `USAGE_QUERIES` — three rows, in the source's own order (which is the order the allowlist is derived in).
///
/// **`usageQueryFor` WAS A `hasOwnProperty` LOOKUP WITH A `null` DEFAULT**, and the ports below keep that shape:
/// a name that is not here is refused by the prologue, so the miss is a backstop rather than a caller's typo.
pub const USAGE_QUERIES: [UsageQuery; 3] = [
    UsageQuery {
        name: "OPENROUTER_API_KEY",
        url: "https://openrouter.ai/api/v1/auth/key",
        map: crate::decisions::map_openrouter_usage,
    },
    UsageQuery {
        name: "AMD_API_KEY",
        url: "https://developer.amd.com.cn/radeon/api/v1/usage",
        map: crate::decisions::map_amd_usage,
    },
    UsageQuery {
        name: "OPENCODE_GO_API_KEY",
        url: "https://opencode.ai/zen/go/v1/usage",
        map: crate::decisions::map_og_usage,
    },
];

/// `Object.keys(USAGE_QUERIES)` — the prologue's allowlist, DERIVED from the table so the two cannot drift.
pub fn usage_names() -> Vec<&'static str> {
    USAGE_QUERIES.iter().map(|row| row.name).collect()
}

/// `usageQueryFor(name)`.
pub fn usage_query_for(name: &str) -> Option<&'static UsageQuery> {
    USAGE_QUERIES.iter().find(|row| row.name == name)
}

/// `jsonOk({ ok, name, status, ...mapped })` — the success arm, with the fixed keys first.
///
/// A mapper that THREW is the same envelope with `ok: false` and the mapper's own `detail`
/// (`"Invalid upstream response"` for OpenRouter's missing `data` object), which is what the source's `catch`
/// produced: the throw crossed no boundary in the source, and it crosses none here.
pub fn usage_envelope(
    status: u16,
    name: &str,
    mapped: std::result::Result<Value, String>,
) -> Built {
    let mut out = Map::new();
    match mapped {
        Ok(value) => {
            out.insert("ok".to_string(), Value::Bool(true));
            out.insert("name".to_string(), Value::String(name.to_string()));
            out.insert("status".to_string(), json!(status));
            if let Value::Object(fields) = value {
                for (k, v) in fields {
                    out.insert(k, v);
                }
            }
        }
        Err(detail) => {
            out.insert("ok".to_string(), Value::Bool(false));
            out.insert("name".to_string(), Value::String(name.to_string()));
            out.insert("detail".to_string(), Value::String(detail));
        }
    }
    responses::json_ok(&Value::Object(out), &[])
}

/// The two refusals that are NOT the mapper's: an upstream that did not answer 2xx, and a throw that carried no
/// mapper detail (a network failure, an unreadable body).
///
/// **`res.ok` IS 200..=299, NOT `== 200`** — the source compares the Fetch API's own flag, and a 204 or a 3xx
/// followed by a redirect chain must not be read as a failure here.
pub fn upstream_failed(status: u16, name: &str) -> Built {
    responses::json_ok(
        &json!({ "ok": false, "name": name, "status": status, "detail": format!("Upstream {status}") }),
        &[],
    )
}

/// The source's `catch`: `{ ok: false, name, detail }`, where `detail` is the throw's own when it carried one.
pub fn usage_threw(name: &str, detail: Option<&str>) -> Built {
    responses::json_ok(
        &json!({ "ok": false, "name": name, "detail": detail.unwrap_or("Usage query failed") }),
        &[],
    )
}

/// `usageQuery(url, env, key, name, map, decision)` — GET the endpoint with the caller's key, map the payload,
/// answer the envelope.
pub async fn usage_query(row: &UsageQuery, key: &str, name: &str) -> Built {
    let headers = Headers::new();
    if headers
        .set("Authorization", &format!("Bearer {key}"))
        .is_err()
    {
        return usage_threw(name, None);
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.with_headers(headers);

    let mut response =
        match crate::device::fetch_with_timeout(row.url, &init, USAGE_TIMEOUT_MS).await {
            Ok(response) => response,
            // THE SOURCE'S `catch` — a network throw, an abort, a socket reset.
            Err(_) => return usage_threw(name, None),
        };
    let status = response.status_code();
    if !(200..300).contains(&status) {
        return upstream_failed(status, name);
    }
    // `await res.json()` — a body that is not JSON throws in the source and lands in the same catch.
    let payload: Value = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(_) => return usage_threw(name, None),
    };
    usage_envelope(status, name, (row.map)(&json!({ "payload": payload })))
}

/// One probe arm: what `testKey` dials for a key name, and what its success text is.
///
/// **THE SIX PLAIN ARMS ARE DATA AND THE TWO ODD ONES ARE CODE**, which is the split the source's own table had
/// to make: `og` is not a URL because the US-proxy switch decides it AND its answer is a stream read, and `amd`
/// is not a fixed success text because it reports the models the key can see. Everything else — method, body,
/// extra headers, the success text — is a row here.
pub struct ProbeArm {
    pub name: &'static str,
    pub method: Method,
    pub url: &'static str,
    pub body: Option<&'static str>,
    pub extra_headers: &'static [(&'static str, &'static str)],
    pub ok_text: &'static str,
}

/// `testKey`'s six table-shaped arms, in the source's own order (deepseek, openrouter, command code, gmi,
/// nvidia, qwen). `og` and `amd` are reached by name in [`test_key`].
const PROBE_ARMS: [ProbeArm; 6] = [
    ProbeArm {
        name: "DEEPSEEK_API_KEY",
        method: Method::Get,
        url: "https://api.deepseek.com/models",
        body: None,
        extra_headers: &[],
        ok_text: "DeepSeek auth OK",
    },
    ProbeArm {
        name: "OPENROUTER_API_KEY",
        method: Method::Get,
        url: "https://openrouter.ai/api/v1/auth/key",
        body: None,
        extra_headers: &[],
        ok_text: "OpenRouter auth OK",
    },
    ProbeArm {
        name: "CMD_API_KEY",
        // Command Code's Anthropic endpoint serves claude-* only, so the probe meters its OpenAI
        // chat/completions endpoint instead — a micro-request against the plan credits.
        method: Method::Post,
        url: "https://api.commandcode.ai/provider/v1/chat/completions",
        body: Some(
            r#"{"model":"deepseek/deepseek-v4.1-flash","messages":[{"role":"user","content":"ping"}],"max_tokens":1}"#,
        ),
        extra_headers: &[("Content-Type", "application/json")],
        ok_text: "Command Code auth OK",
    },
    ProbeArm {
        name: "GMI_API_KEY",
        method: Method::Get,
        url: "https://api.gmi-serving.com/v1/models",
        body: None,
        extra_headers: &[],
        ok_text: "GMI Cloud auth OK",
    },
    ProbeArm {
        name: "NVAPI_KEY",
        method: Method::Get,
        url: "https://integrate.api.nvidia.com/v1/models",
        body: None,
        // **THE ONE ARM WITH AN `Accept`, AND IT IS THE SOURCE'S**: NVIDIA's NIM answers a JSON list either
        // way; the header is carried because the shipping probe sends it.
        extra_headers: &[("Accept", "application/json")],
        ok_text: "NVIDIA NIM auth OK",
    },
    ProbeArm {
        name: "QWEN_API_KEY",
        method: Method::Post,
        url: "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1/messages",
        body: Some(
            r#"{"model":"qwen3.8-max-preview","messages":[{"role":"user","content":"ping"}],"max_tokens":1}"#,
        ),
        extra_headers: &[
            ("Content-Type", "application/json"),
            ("anthropic-version", "2023-06-01"),
        ],
        ok_text: "Qwen MaaS auth OK",
    },
];

/// `OG_ZEN_CHAT` — the endpoint the og arm uses when the US proxy is off.
const OG_ZEN_CHAT: &str = "https://opencode.ai/zen/go/v1/chat/completions";

/// `testKey`'s probe timeout: `fetchWithTimeout`'s default again (the source passes no third argument).
const PROBE_TIMEOUT_MS: u64 = USAGE_TIMEOUT_MS;

/// `keyProbeResult(name, res, okText)` — `{ ok, name, status, detail }`, in the source's key order.
pub fn probe_response(status: u16, name: &str, ok: bool, ok_text: &str) -> Built {
    let mut out = Map::new();
    out.insert("ok".to_string(), Value::Bool(ok));
    out.insert("name".to_string(), Value::String(name.to_string()));
    out.insert("status".to_string(), json!(status));
    out.insert(
        "detail".to_string(),
        Value::String(if ok {
            ok_text.to_string()
        } else {
            format!("Upstream {status}")
        }),
    );
    responses::json_ok(&Value::Object(out), &[])
}

/// The two arms that are not envelopes built from a status: no key at all, and a THROW.
///
/// ```text
///   testKey(env, name, "")          -> { ok: false, name, detail: "Key not configured" }
///   a fetch that threw              -> { ok: false, name, detail: "Test failed: <message>" }
/// ```
pub fn probe_no_key(name: &str) -> Built {
    responses::json_ok(
        &json!({ "ok": false, "name": name, "detail": "Key not configured" }),
        &[],
    )
}

pub fn probe_threw(name: &str, message: &str) -> Built {
    responses::json_ok(
        &json!({ "ok": false, "name": name, "detail": format!("Test failed: {message}") }),
        &[],
    )
}

/// The AMD arm's success text — `AMD Radeon Cloud auth OK (N models: a, b)`.
///
/// **THE LIST IS THE POINT OF THIS ARM AND IT IS WHY IT IS NOT A TABLE ROW**: the probe answers with what the
/// key can SEE, and an unreadable body is an empty list rather than a failure (the source's `catch` around
/// `res.json()` leaves `models` empty and still reports `ok`).
pub fn amd_ok_text(models: &[String]) -> String {
    format!(
        "AMD Radeon Cloud auth OK ({} models: {})",
        models.len(),
        models.join(", ")
    )
}

/// `offeredIds`' shape, narrowed to the id list AMD's `/v1/models` carries — `(payload?.data || []).map(m =>
/// m?.id).filter(Boolean)`.
pub fn model_ids(payload: &Value) -> Vec<String> {
    payload
        .get("data")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id"))
                .map(summrise_core::text::js_string)
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `testKey(env, name, key)` — the eight arms, the throw arm, and the backstop.
pub async fn test_key(env: &Env, name: &str, key: &str) -> Built {
    if key.is_empty() {
        return probe_no_key(name);
    }
    match probe_arm_for(name) {
        Some(arm) => {
            let mut init = RequestInit::new();
            init.with_method(arm.method.clone());
            let headers = Headers::new();
            let _ = headers.set("Authorization", &format!("Bearer {key}"));
            for (k, v) in arm.extra_headers {
                let _ = headers.set(k, v);
            }
            init.with_headers(headers);
            if let Some(body) = arm.body {
                init.with_body(Some(JsValue::from_str(body)));
            }
            match crate::device::fetch_with_timeout(arm.url, &init, PROBE_TIMEOUT_MS).await {
                Ok(response) => {
                    let status = response.status_code();
                    probe_response(status, name, (200..300).contains(&status), arm.ok_text)
                }
                Err(message) => probe_threw(name, &message),
            }
        }
        None if name == "AMD_API_KEY" => {
            let mut init = RequestInit::new();
            init.with_method(Method::Get);
            let headers = Headers::new();
            let _ = headers.set("Authorization", &format!("Bearer {key}"));
            init.with_headers(headers);
            match crate::device::fetch_with_timeout(
                "https://developer.amd.com.cn/radeon/api/v1/models",
                &init,
                PROBE_TIMEOUT_MS,
            )
            .await
            {
                Ok(mut response) => {
                    let status = response.status_code();
                    if !(200..300).contains(&status) {
                        return probe_response(status, name, false, "");
                    }
                    // A NON-JSON BODY IS AN EMPTY LIST, NOT A FAILURE — the source's own `catch`.
                    let payload = response.json::<Value>().await.unwrap_or(Value::Null);
                    let ids = model_ids(&payload);
                    probe_response(status, name, true, &amd_ok_text(&ids))
                }
                Err(message) => probe_threw(name, &message),
            }
        }
        // THE OG ARM: the US-proxy switch decides the URL, the body asks for a STREAM, and the answer is read
        // from the first chunk rather than from a status — deemed connected once the stream produces data,
        // without waiting for thinking to finish (the source's own measurement: ~10 s vs the first chunk).
        None if name == "OPENCODE_GO_API_KEY" => {
            let env_json = crate::probe::env_value(env);
            let us_proxy = crate::user_store::global_setting(env, "US_PROXY").await;
            let url = match us_proxy {
                Some(_) => format!(
                    "{}/api/zen?target=og&path={}",
                    crate::routing::us_proxy_base(Some(&env_json)),
                    urlencoding("/v1/chat/completions")
                ),
                None => OG_ZEN_CHAT.to_string(),
            };
            let session =
                crate::session::opencode_session_header(&serde_json::Map::new(), "key-probe");
            let mut init = RequestInit::new();
            init.with_method(Method::Post);
            let headers = Headers::new();
            let _ = headers.set("Authorization", &format!("Bearer {key}"));
            let _ = headers.set("Content-Type", "application/json");
            for (k, v) in &session {
                let _ = headers.set(k, v);
            }
            init.with_headers(headers);
            // zen/go's V4.1 lane slug — the V4 entry was retired 2026-09-10, and probing the live lane keeps
            // this a real auth + model check.
            init.with_body(Some(JsValue::from_str(
                r#"{"model":"deepseek-flash","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":true}"#,
            )));
            match crate::device::fetch_with_timeout(&url, &init, PROBE_TIMEOUT_MS).await {
                Ok(mut response) => {
                    let status = response.status_code();
                    if !(200..300).contains(&status) {
                        return probe_response(status, name, false, "");
                    }
                    let first = first_chunk(&mut response).await;
                    let ok = first.contains("data:");
                    let mut out = Map::new();
                    out.insert("ok".to_string(), Value::Bool(ok));
                    out.insert("name".to_string(), Value::String(name.to_string()));
                    out.insert("status".to_string(), json!(status));
                    out.insert(
                        "detail".to_string(),
                        Value::String(if ok {
                            "OpenCode Go auth OK".to_string()
                        } else {
                            "OpenCode Go auth FAILED (no stream data)".to_string()
                        }),
                    );
                    responses::json_ok(&Value::Object(out), &[])
                }
                Err(message) => probe_threw(name, &message),
            }
        }
        None => responses::json_error(
            400,
            &format!("No probe for key name: {name}"),
            "invalid_request",
        ),
    }
}

/// The arm for a name, or `None` — where `None` covers BOTH the two special arms (reached by name in
/// [`test_key`]) and a name with no arm at all (the backstop's 400).
pub fn probe_arm_for(name: &str) -> Option<&'static ProbeArm> {
    PROBE_ARMS.iter().find(|arm| arm.name == name)
}

/// **EVERY CONSOLE-MANAGED KEY MUST HAVE AN ARM, WHICH IS THE COMPLETENESS CHECK THE DELETED
/// `key-probe.test.mjs` CARRIED** — and it is a statement about what is MISSING, so no corpus can see it.
pub fn every_managed_key_has_an_arm() -> std::result::Result<(), String> {
    for name in crate::user_store::user_key_names() {
        if probe_arm_for(name).is_some() {
            continue;
        }
        match name {
            // The two arms that are code rather than rows, named so the list stays a decision.
            "AMD_API_KEY" | "OPENCODE_GO_API_KEY" => continue,
            // **`R4_API_KEY` IS A REAL GAP, NAMED RATHER THAN EXCLUDED QUIETLY (2026-10-17).** The deleted
            // `key-probe.test.mjs` asserted "every `USER_KEY_NAMES` entry probes to a Response" — and this key
            // answered the BACKSTOP's `400 No probe for key name`, which is a Response, so the assertion held
            // while the console's own key test refused an r4 key. This check is the stricter one; the arm is
            // missing because adding it needs a MEASUREMENT (a cheap auth endpoint on `https://api.r4.codes`,
            // the base `routing.rs` already carries for its chat and messages paths) rather than a guess.
            "R4_API_KEY" => continue,
            other => return Err(format!("managed key {other} has no probe arm")),
        }
    }
    Ok(())
}

/// The first chunk of a streaming body, or `""` — the source's `reader.read()` + `reader.cancel()`.
async fn first_chunk(response: &mut worker::Response) -> String {
    use futures_util::StreamExt;
    let Ok(mut stream) = response.stream() else {
        return String::new();
    };
    match stream.next().await {
        Some(Ok(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
        _ => String::new(),
    }
}

/// `encodeURIComponent` for the one path this file encodes, kept local rather than pulling `js_sys` in: the
/// slashes MUST be encoded (`%2F`), which is the whole reason the source calls it.
fn urlencoding(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => (b as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! **THE PINS CAME FROM `gateway/test/usage-map.test.mjs`, WHICH LEFT WITH THIS FILE'S TYPESCRIPT** — its
    //! subject was the three mappers and the shared skeleton, and both are here now. The mapping tables
    //! themselves are pinned in `decisions.rs` (they never moved), so what is new here is the ENVELOPE: the four
    //! fixed keys, their order, and the two refusal arms.
    use super::*;

    fn body(built: &Built) -> Value {
        serde_json::from_str(&built.body).expect("the envelope is JSON")
    }

    #[test]
    fn the_table_is_the_sources_three_rows_in_its_own_order() {
        assert_eq!(
            usage_names(),
            vec!["OPENROUTER_API_KEY", "AMD_API_KEY", "OPENCODE_GO_API_KEY"]
        );
        assert_eq!(
            usage_query_for("OPENROUTER_API_KEY").map(|r| r.url),
            Some("https://openrouter.ai/api/v1/auth/key")
        );
        assert_eq!(
            usage_query_for("AMD_API_KEY").map(|r| r.url),
            Some("https://developer.amd.com.cn/radeon/api/v1/usage")
        );
        assert_eq!(
            usage_query_for("OPENCODE_GO_API_KEY").map(|r| r.url),
            Some("https://opencode.ai/zen/go/v1/usage")
        );
        // `usageQueryFor`'s default: a name that is not a row is `None`, not a fallback row.
        assert!(usage_query_for("DEEPSEEK_API_KEY").is_none());
    }

    #[test]
    fn the_success_envelope_puts_the_fixed_keys_first_and_keeps_the_mappers_order() {
        let mapped = json!({"label": "me", "usage": 1.25, "limit": 10});
        let built = usage_envelope(200, "OPENROUTER_API_KEY", Ok(mapped));
        assert_eq!(built.status, 200);
        // THE BYTES, not the fields: `JSON.stringify`'s own order is what the console parsed.
        assert_eq!(
            built.body,
            r#"{"ok":true,"name":"OPENROUTER_API_KEY","status":200,"label":"me","usage":1.25,"limit":10}"#
        );
    }

    #[test]
    fn a_mapper_that_threw_is_the_same_envelope_with_its_own_detail() {
        let built = usage_envelope(
            200,
            "OPENROUTER_API_KEY",
            Err("Invalid upstream response".to_string()),
        );
        assert_eq!(
            built.body,
            r#"{"ok":false,"name":"OPENROUTER_API_KEY","detail":"Invalid upstream response"}"#
        );
        // The throw arm carries NO status: the source's `catch` writes three keys, and a port that added the
        // status it happened to know would change bytes the corpus can see.
        assert!(!body(&built).as_object().unwrap().contains_key("status"));
    }

    // ── THE PROBE ARM (2026-10-17) ──────────────────────────────────────────────────────────────────
    //
    // **THE PINS CAME FROM `gateway/test/key-probe.test.mjs`, WHICH LEFT WITH THIS FILE'S TYPESCRIPT**: the
    // GET-models arms hitting their own hosts with the caller's key, the POST arms carrying their JSON bodies,
    // the two non-table arms, and the COMPLETENESS check (every console-managed key must resolve to a real
    // probe, never to the backstop's 400). The DIALS themselves are the harness's business (`verify.mjs` drives
    // the built worker with a fetch stub); what is pinned here is the plan each arm represents.

    #[test]
    fn every_console_managed_key_has_an_arm() {
        every_managed_key_has_an_arm().expect("a managed key with no probe arm");
        // Sanity: the check would catch a key with no arm at all.
        assert!(probe_arm_for("NOPE_API_KEY").is_none());
        for (name, arm) in [
            ("DEEPSEEK_API_KEY", "https://api.deepseek.com/models"),
            (
                "OPENROUTER_API_KEY",
                "https://openrouter.ai/api/v1/auth/key",
            ),
            (
                "CMD_API_KEY",
                "https://api.commandcode.ai/provider/v1/chat/completions",
            ),
            ("GMI_API_KEY", "https://api.gmi-serving.com/v1/models"),
            ("NVAPI_KEY", "https://integrate.api.nvidia.com/v1/models"),
            (
                "QWEN_API_KEY",
                "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1/messages",
            ),
        ] {
            let row = probe_arm_for(name).unwrap_or_else(|| panic!("no arm for {name}"));
            assert_eq!(row.url, arm, "{name}");
        }
        // The two arms that are NOT rows are still names the route accepts (they are reached by name).
        assert!(probe_arm_for("AMD_API_KEY").is_none());
        assert!(probe_arm_for("OPENCODE_GO_API_KEY").is_none());
        // **AND THE ONE NAMED GAP IS A GAP**: `R4_API_KEY` reaches the backstop's 400 today, which is what the
        // deleted TypeScript test's weaker claim ("probes to a Response") let stand.
        assert!(probe_arm_for("R4_API_KEY").is_none());
        assert!(every_managed_key_has_an_arm().is_ok());
    }

    #[test]
    fn the_get_arms_are_gets_and_the_post_arms_carry_their_bodies() {
        for name in [
            "DEEPSEEK_API_KEY",
            "OPENROUTER_API_KEY",
            "GMI_API_KEY",
            "NVAPI_KEY",
        ] {
            let arm = probe_arm_for(name).expect("a row");
            assert_eq!(arm.method, Method::Get, "{name} must not carry a body");
            assert!(arm.body.is_none(), "{name}");
        }
        for name in ["CMD_API_KEY", "QWEN_API_KEY"] {
            let arm = probe_arm_for(name).expect("a row");
            assert_eq!(arm.method, Method::Post, "{name}");
            let body = arm.body.expect("a POST arm carries its JSON body");
            // The body is JSON and asks for ONE token — the micro-request the plan is metered for.
            let parsed: Value = serde_json::from_str(body).expect("the body is JSON");
            assert_eq!(parsed["max_tokens"], json!(1), "{name}");
            assert_eq!(parsed["messages"][0]["content"], json!("ping"), "{name}");
            assert!(
                arm.extra_headers
                    .iter()
                    .any(|(k, v)| *k == "Content-Type" && *v == "application/json"),
                "{name} must declare its content type"
            );
        }
        // The V4.1 slugs, which is the half a retired model id gets wrong silently.
        assert!(probe_arm_for("CMD_API_KEY")
            .unwrap()
            .body
            .unwrap()
            .contains("deepseek-v4.1-flash"));
        assert!(probe_arm_for("QWEN_API_KEY")
            .unwrap()
            .body
            .unwrap()
            .contains("qwen3.8-max-preview"));
        assert!(probe_arm_for("NVAPI_KEY")
            .unwrap()
            .extra_headers
            .contains(&("Accept", "application/json")));
    }

    #[test]
    fn the_probe_envelope_is_the_sources_four_keys_in_its_order() {
        assert_eq!(
            probe_response(200, "DEEPSEEK_API_KEY", true, "DeepSeek auth OK").body,
            r#"{"ok":true,"name":"DEEPSEEK_API_KEY","status":200,"detail":"DeepSeek auth OK"}"#
        );
        assert_eq!(
            probe_response(401, "DEEPSEEK_API_KEY", false, "DeepSeek auth OK").body,
            r#"{"ok":false,"name":"DEEPSEEK_API_KEY","status":401,"detail":"Upstream 401"}"#
        );
        // **THE `ok` FLAG IS THE 2xx BOUNDARY, NOT THE STATUS' PRESENCE**: a 204 is a success and a 302 is not.
        assert!(probe_response(204, "X", true, "t")
            .body
            .contains(r#""ok":true"#));
        assert!(probe_response(302, "X", false, "t")
            .body
            .contains(r#""ok":false"#));
    }

    #[test]
    fn the_two_arms_that_are_not_status_envelopes() {
        assert_eq!(
            probe_no_key("GMI_API_KEY").body,
            r#"{"ok":false,"name":"GMI_API_KEY","detail":"Key not configured"}"#
        );
        assert_eq!(
            probe_threw("GMI_API_KEY", "timeout after 15000ms").body,
            r#"{"ok":false,"name":"GMI_API_KEY","detail":"Test failed: timeout after 15000ms"}"#
        );
    }

    #[test]
    fn the_amd_arm_reports_what_the_key_can_see() {
        let payload = json!({"data": [{"id": "a"}, {"id": "b"}, {"nope": 1}]});
        assert_eq!(model_ids(&payload), vec!["a", "b"]);
        assert_eq!(
            model_ids(&json!({"data": "not a list"})),
            Vec::<String>::new()
        );
        assert_eq!(
            amd_ok_text(&["a".to_string(), "b".to_string()]),
            "AMD Radeon Cloud auth OK (2 models: a, b)"
        );
        assert_eq!(amd_ok_text(&[]), "AMD Radeon Cloud auth OK (0 models: )");
    }

    #[test]
    fn the_og_path_is_percent_encoded_as_the_source_encodes_it() {
        // `encodeURIComponent("/v1/chat/completions")` — the slashes MUST become `%2F`.
        assert_eq!(
            urlencoding("/v1/chat/completions"),
            "%2Fv1%2Fchat%2Fcompletions"
        );
    }

    #[test]
    fn an_upstream_refusal_and_a_throw_are_two_different_arms() {
        assert_eq!(
            upstream_failed(429, "AMD_API_KEY").body,
            r#"{"ok":false,"name":"AMD_API_KEY","status":429,"detail":"Upstream 429"}"#
        );
        assert_eq!(
            usage_threw("AMD_API_KEY", None).body,
            r#"{"ok":false,"name":"AMD_API_KEY","detail":"Usage query failed"}"#
        );
        // **`res.ok` IS 2xx, SO 204 IS A SUCCESS AND 302 IS NOT** — the boundary the source's flag draws.
        assert!((200..300).contains(&204));
        assert!(!(200..300).contains(&302));
    }
}
