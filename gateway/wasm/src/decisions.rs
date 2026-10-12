//! THE DECISION ROUTE — how a pure decision moves into Rust WITHOUT moving its route.
//!
//! ── WHY THIS EXISTS ───────────────────
//!
//! `WASM_GATE` is a SERVICE BINDING to this worker (`gateway/wrangler.jsonc`), so the console reaches Rust only
//! through `fetch`. That made a decision unmovable on its own: the only way to move one was to move the whole route
//! that carried it, which is why porting the probe route did not move `MAX_LOGIC` at all.
//!
//! This is the second way. `POST /__decision` takes a NAME and its JSON ARGS and answers with the decision's value,
//! so a pure decision can be ported one row at a time and the TypeScript caller keeps its route.
//!
//! **THE HANDLER TAKES NO `Env`, AND THAT IS THE WHOLE SECURITY ARGUMENT.** This worker also serves `/v1` on a public
//! host, so this route is publicly reachable. A decision that CANNOT READ `env` cannot become an oracle for a key, and
//! the signature is what enforces it rather than a comment: `handle(request)` has no environment to leak. **A decision
//! that needs `env` is not a decision — it is a route, and it moves by moving its route.**
//!
//! MUTATION: make `handle` take `env` and add a decision that returns `env.var("DEEPSEEK_API_KEY")`.
//! RESULT:   it compiles, and the route answers a WORKER KEY to any public caller. The defence is the signature, so
//!           this mutation is the reason the signature has no `Env` parameter to thread.
//!
//! MUTATION: change the family's `==` to `starts_with`.
//! RESULT:   `__decision_anything` reaches the table — this test fails, and nothing else notices.
//!
//! MUTATION: drop the `args` arm from `upstream_models_url`.
//! RESULT:   every call answers `upstream must be a string` instead of a URL — the decision's own test fails.

use serde_json::{json, Value};
// **NO `Env` IN THIS IMPORT LIST, AND THE COMPILER IS WHAT SAYS SO.** The first version imported it out of habit and
// rustc answered `unused import: Env` — which is this module's whole security argument stated by the toolchain rather
// than by me: there is no environment here to read.
use worker::{Method, Request, Response, Result};

use crate::responses::{self, Built};
use crate::RouteFailure;

/// `POST /__decision` — POST only, and the EXACT path: a prefix would swallow a neighbour.
pub fn in_family(method: &Method, path: &str) -> bool {
    *method == Method::Post && path == "/__decision"
}

/// A pure decision: JSON in, JSON out, and **no environment**.
type Decision = fn(&Value) -> std::result::Result<Value, String>;

/// **THE TABLE.** A decision is added by adding a row, and the TypeScript side needs no change to reach it.
const DECISIONS: [(&str, Decision); 5] = [
    ("upstream_models_url", upstream_models_url),
    ("offered_ids", offered_ids),
    ("map_openrouter_usage", map_openrouter_usage),
    ("map_amd_usage", map_amd_usage),
    ("map_og_usage", map_og_usage),
];

/// The dispatcher. `Ok(None)` is "not this family". **NO `Env` PARAMETER — see the module header.**
pub async fn handle(mut request: Request) -> Result<Option<Response>, RouteFailure> {
    let url = request.url().map_err(|_| RouteFailure::Threw)?;
    if !in_family(&request.method(), url.path()) {
        return Ok(None);
    }
    Ok(Some(reply(run(&mut request).await)))
}

/// The protocol, in one place.
async fn run(request: &mut Request) -> Built {
    let text = request.text().await.unwrap_or_default();
    let body: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return refusal("body must be JSON"),
    };
    let Some(name) = body.get("name").and_then(Value::as_str) else {
        return refusal("body must carry a `name`");
    };
    let args = body.get("args").cloned().unwrap_or_else(|| json!({}));
    let Some((_, decision)) = DECISIONS.iter().find(|(n, _)| *n == name) else {
        return refusal(&format!("unknown decision: {name}"));
    };
    match decision(&args) {
        Ok(value) => responses::json_ok(&json!({ "ok": true, "value": value }), &[]),
        Err(error) => refusal(&error),
    }
}

/// Every refusal is the same shape, so a caller has one arm to write.
fn refusal(message: &str) -> Built {
    responses::json_error(400, message, "invalid_request")
}

fn reply(built: Built) -> Response {
    crate::json(built.body, built.status).unwrap_or_else(|_| Response::error("", 500).unwrap())
}

/* ─────────────────────────── the decisions ─────────────────────────── */

/// `upstreamModelsUrl(upstream)` — `gateway/src/plugins/models-probe.ts`.
///
/// `upstream.endsWith("/chat/completions") ? upstream.slice(0, -len) + "/models" : null`
fn upstream_models_url(args: &Value) -> std::result::Result<Value, String> {
    const SUFFIX: &str = "/chat/completions";
    let upstream = args
        .get("upstream")
        .and_then(Value::as_str)
        .ok_or_else(|| "upstream must be a string".to_string())?;
    Ok(match upstream.strip_suffix(SUFFIX) {
        Some(head) => Value::String(format!("{head}/models")),
        None => Value::Null,
    })
}

/// `offeredIds(payload)` — `gateway/src/plugins/models-probe.ts`.
///
/// Three dialects: a bare array, an OpenAI/Anthropic-style `{data:[…]}`, or neither (which is `[]`). Each row is
/// either a string or an object whose `id` is taken, then trimmed, then dropped when empty.
///
/// **`String(r?.id ?? "")` IS JAVASCRIPT'S COERCION AND IS REPRODUCED, NOT APPROXIMATED** — a number becomes its
/// decimal form, a bool becomes `true`/`false`, and an object becomes `[object Object]`. An id is a string in every
/// dialect we speak, so the last two are unreachable in practice; they are here because **a port that silently
/// answers `""` where the source answered `"[object Object]"` is a divergence with no test on it.**
fn offered_ids(args: &Value) -> std::result::Result<Value, String> {
    let payload = args.get("payload").cloned().unwrap_or(Value::Null);
    let rows = if payload.is_array() {
        payload
    } else if let Some(data) = payload.get("data").filter(|d| d.is_array()) {
        data.clone()
    } else {
        return Ok(json!([]));
    };
    let out: Vec<Value> = rows
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|row| {
            let text = match row {
                Value::String(s) => s.clone(),
                other => js_string(other.get("id").unwrap_or(&Value::Null)),
            };
            Value::String(text.trim().to_string())
        })
        .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        .collect();
    Ok(Value::Array(out))
}

/// JavaScript's `String(x)`, for the arms a JSON `id` can actually be.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // `String({})` and `String([])` — the two exotic arms, carried so the coercion is the source's.
        Value::Object(_) => "[object Object]".to_string(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
    }
}

/// `mapOpenRouterUsage(payload)` — `gateway/src/plugins/auth.ts`.
///
/// **THE KEY ORDER IS THE SOURCE'S AND IS PART OF THE BYTES.** `preserve_order` is on for this crate (its Cargo.toml
/// says it is not optional for the translation), so the map is built in the same order the object literal is: the
/// three passthrough fields in the loop's order, then `isFreeTier`, then `rateLimit`.
///
/// A field passes through only when it is `null`, a string or a number — **a bool or an object is DROPPED, not
/// coerced**, and the numbers are cloned rather than converted to `f64` (which would answer `5.0` where the source
/// answers `5`).
fn map_openrouter_usage(args: &Value) -> std::result::Result<Value, String> {
    let data = args
        .get("payload")
        .and_then(|p| p.get("data"))
        .filter(|d| d.is_object())
        .ok_or_else(|| "Invalid upstream response".to_string())?;
    let mut out = serde_json::Map::new();
    for field in ["label", "usage", "limit"] {
        if let Some(v) = data.get(field) {
            if v.is_null() || v.is_string() || v.is_number() {
                out.insert(field.to_string(), v.clone());
            }
        }
    }
    if let Some(b) = data.get("is_free_tier").and_then(Value::as_bool) {
        out.insert("isFreeTier".to_string(), json!(b));
    }
    if let Some(rl) = data.get("rate_limit").filter(|r| r.is_object()) {
        let mut rate = serde_json::Map::new();
        if let Some(v) = rl.get("limit").filter(|v| v.is_number()) {
            rate.insert("limit".to_string(), v.clone());
        }
        if let Some(s) = rl.get("interval").and_then(Value::as_str) {
            rate.insert("interval".to_string(), json!(s));
        }
        if let Some(s) = rl.get("reset").and_then(Value::as_str) {
            rate.insert("reset".to_string(), json!(s));
        }
        if !rate.is_empty() {
            out.insert("rateLimit".to_string(), Value::Object(rate));
        }
    }
    Ok(Value::Object(out))
}

/// `mapAmdUsage(payload)` — `gateway/src/plugins/auth.ts`.
///
/// **`limit` IS ALWAYS PRESENT AND IS `null` WHEN THE SOURCE HAS NO NUMBER** — it is assigned outside the guard, so
/// an absent cap is a null cap rather than an absent key. And `label`'s org falls back on JAVASCRIPT FALSINESS, not
/// on absence: `payload.organization_id || "radeon"` answers `"radeon"` for `0` and `""` too, which is why the check
/// is on the value rather than on the string it becomes.
fn map_amd_usage(args: &Value) -> std::result::Result<Value, String> {
    let payload = args.get("payload").cloned().unwrap_or(Value::Null);
    let mut out = serde_json::Map::new();
    if let Some(v) = payload.get("daily_cost_used_usd").filter(|v| v.is_number()) {
        out.insert("usage".to_string(), v.clone());
    }
    out.insert(
        "limit".to_string(),
        payload
            .get("daily_cost_limit_usd")
            .filter(|v| v.is_number())
            .cloned()
            .unwrap_or(Value::Null),
    );
    if let Some(v) = payload.get("rpm_limit").filter(|v| v.is_number()) {
        let mut rate = serde_json::Map::new();
        rate.insert("limit".to_string(), v.clone());
        rate.insert("interval".to_string(), json!("minute"));
        if let Some(s) = payload.get("daily_reset_at").and_then(Value::as_str) {
            rate.insert("reset".to_string(), json!(s));
        }
        out.insert("rateLimit".to_string(), Value::Object(rate));
    }
    if let Some(all) = payload.get("all_time").filter(|v| v.is_object()) {
        let requests = all
            .get("requests")
            .filter(|v| v.is_number())
            .cloned()
            .unwrap_or_else(|| json!(0));
        let tokens = all
            .get("total_tokens")
            .filter(|v| v.is_number())
            .cloned()
            .unwrap_or_else(|| json!(0));
        let org = match payload.get("organization_id") {
            Some(v) if !is_falsy(v) => js_string(v),
            _ => "radeon".to_string(),
        };
        out.insert(
            "label".to_string(),
            json!(format!("{org} · {requests} req · {tokens} tok")),
        );
    }
    Ok(Value::Object(out))
}

/// `mapOgUsage(payload)` — `gateway/src/plugins/auth.ts`.
///
/// **`"limit" in payload` IS A KEY CHECK, NOT A VALUE CHECK**, and it is the one place the source asks whether a key
/// EXISTS: a `limit` of `null` is carried through as `null`, while an absent `limit` adds nothing. The multi-window
/// shape keeps its own insertion order for the same reason the outer map does.
fn map_og_usage(args: &Value) -> std::result::Result<Value, String> {
    let payload = args.get("payload").cloned().unwrap_or(Value::Null);
    let mut out = serde_json::Map::new();
    if let Some(p) = Some(&payload).filter(|v| v.is_object()) {
        if let Some(v) = p.get("used").filter(|v| v.is_number()) {
            out.insert("usage".to_string(), v.clone());
        }
        if let Some(v) = p.get("limit") {
            if v.is_number() || v.is_null() {
                out.insert("limit".to_string(), v.clone());
            }
        }
        if let Some(v) = p.get("balance").filter(|v| v.is_number()) {
            out.insert("balance".to_string(), v.clone());
        }
        if let Some(s) = p.get("plan").and_then(Value::as_str) {
            out.insert("label".to_string(), json!(s));
        }
        if let Some(w) = p.get("windows").filter(|v| v.is_object()) {
            let mut windows = serde_json::Map::new();
            // **AN EMPTY MAP AND AN ABSENT ONE ARE THE SAME ANSWER HERE, WHICH IS WHY THIS IS A `match` RATHER
            // THAN AN ITERATOR DEFAULT**: `serde_json::Map`'s iterator implements no `Default`.
            let Some(entries) = w.as_object() else {
                unreachable!("filtered to an object above")
            };
            for (wk, wv) in entries {
                if !wv.is_object() {
                    continue;
                }
                let mut one = serde_json::Map::new();
                if let Some(v) = wv.get("used").filter(|v| v.is_number()) {
                    one.insert("used".to_string(), v.clone());
                }
                if let Some(v) = wv.get("limit") {
                    if v.is_number() || v.is_null() {
                        one.insert("limit".to_string(), v.clone());
                    }
                }
                if let Some(v) = wv.get("remaining").filter(|v| v.is_number()) {
                    one.insert("remaining".to_string(), v.clone());
                }
                if let Some(v) = wv.get("reset_at") {
                    if v.is_string() || v.is_null() {
                        one.insert("resetAt".to_string(), v.clone());
                    }
                }
                windows.insert(wk.clone(), Value::Object(one));
            }
            out.insert("windows".to_string(), Value::Object(windows));
        }
    }
    Ok(Value::Object(out))
}

/// JavaScript falsiness, for the arms a JSON value can be: `false`, `0`, `""` and `null` are falsy; an object and an
/// array are TRUTHY (which is why `[] || "radeon"` answers the array).
fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f == 0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(_) | Value::Object(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> Value {
        let (_, decision) = DECISIONS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("no decision named {name}"));
        decision(&args).unwrap_or_else(|e| panic!("{name} refused: {e}"))
    }

    /// **THE FAMILY IS POST AND THE EXACT PATH.** A prefix would let `__decision_x` reach the table.
    ///
    /// MUTATION: `==` -> `starts_with`.
    /// RESULT:   the third assertion fails, and nothing else notices.
    #[test]
    fn the_family_is_post_and_the_exact_path() {
        assert!(in_family(&Method::Post, "/__decision"));
        assert!(!in_family(&Method::Get, "/__decision"));
        assert!(!in_family(&Method::Post, "/__decision_x"));
    }

    /// `upstreamModelsUrl` — the suffix is the decision, and a URL without it is `null` rather than a guess.
    ///
    /// MUTATION: return `upstream + "/models"` instead of slicing the suffix off.
    /// RESULT:   the first assertion fails: the URL would read `…/chat/completions/models`.
    #[test]
    fn upstream_models_url_swaps_the_suffix() {
        assert_eq!(
            call(
                "upstream_models_url",
                json!({"upstream": "https://x/v1/chat/completions"})
            ),
            json!("https://x/v1/models")
        );
        assert_eq!(
            call(
                "upstream_models_url",
                json!({"upstream": "https://x/v1/messages"})
            ),
            Value::Null
        );
        // The suffix is the WHOLE tail, so a URL that merely contains it is not one.
        assert_eq!(
            call(
                "upstream_models_url",
                json!({"upstream": "https://x/chat/completions/9"})
            ),
            Value::Null
        );
    }

    /// `offeredIds` — three dialects, and the empty ones are dropped AFTER the trim.
    ///
    /// MUTATION: drop the `.filter(..)`.
    /// RESULT:   a row with a whitespace-only id becomes `""` in the answer instead of being dropped — the
    ///           assertion on the whitespace row fails.
    #[test]
    fn offered_ids_reads_all_three_dialects() {
        assert_eq!(
            call("offered_ids", json!({"payload": ["a", "b"]})),
            json!(["a", "b"])
        );
        assert_eq!(
            call(
                "offered_ids",
                json!({"payload": {"data": [{"id": "a"}, {"id": " b "}]}})
            ),
            json!(["a", "b"])
        );
        assert_eq!(
            call("offered_ids", json!({"payload": {"object": "list"}})),
            json!([])
        );
        // A whitespace-only id trims to empty and is DROPPED, not returned.
        assert_eq!(
            call("offered_ids", json!({"payload": ["a", "   ", ""]})),
            json!(["a"])
        );
    }

    /// **JAVASCRIPT'S COERCION IS REPRODUCED, NOT APPROXIMATED** — `String(r?.id ?? "")`.
    ///
    /// MUTATION: answer `""` for a non-string id instead of coercing it.
    /// RESULT:   the number and bool assertions fail — `5` must become `"5"`, not `""`.
    #[test]
    fn a_non_string_id_is_coerced_the_way_javascript_does() {
        assert_eq!(
            call("offered_ids", json!({"payload": [{"id": 5}]})),
            json!(["5"])
        );
        assert_eq!(
            call("offered_ids", json!({"payload": [{"id": true}]})),
            json!(["true"])
        );
        assert_eq!(
            call("offered_ids", json!({"payload": [{"id": null}]})),
            json!([])
        );
        assert_eq!(call("offered_ids", json!({"payload": [{}]})), json!([]));
        assert_eq!(
            call("offered_ids", json!({"payload": [{"id": {"a": 1}}]})),
            json!(["[object Object]"])
        );
    }

    /// **THE KEY ORDER IS THE BYTES, SO THE ASSERTION IS THE SERIALIZED STRING.** `jsonOk(out)` writes
    /// `JSON.stringify`'s own order, and a port that built the same map in a different order would answer the same
    /// VALUES in different bytes — which every field-by-field assertion would pass.
    ///
    /// MUTATION: insert `rateLimit` before the passthrough loop.
    /// RESULT:   the string assertion fails while every value assertion still passes.
    #[test]
    fn the_openrouter_map_keeps_the_sources_key_order() {
        let got = call(
            "map_openrouter_usage",
            json!({"payload": {"data": {"label": "free", "usage": 1, "limit": null, "is_free_tier": true,
                                        "rate_limit": {"limit": 5, "interval": "day"}}}}),
        );
        assert_eq!(
            got.to_string(),
            r#"{"label":"free","usage":1,"limit":null,"isFreeTier":true,"rateLimit":{"limit":5,"interval":"day"}}"#
        );
    }

    /// **A FIELD PASSES THROUGH ONLY AS `null`, A STRING OR A NUMBER — a bool or an object is DROPPED, not coerced.**
    ///
    /// MUTATION: accept every field unconditionally.
    /// RESULT:   `extra` (a bool) and `obj` (an object) appear in the answer, and the source's own shape is lost.
    #[test]
    fn the_openrouter_map_drops_the_field_types_the_source_drops() {
        let got = call(
            "map_openrouter_usage",
            json!({"payload": {"data": {"label": true, "usage": {"a": 1}, "limit": "x"}}}),
        );
        assert_eq!(got.to_string(), r#"{"limit":"x"}"#);
        // And a payload with no object `data` is the source's throw, which the route answers as a refusal.
        let (_, decision) = DECISIONS
            .iter()
            .find(|(n, _)| *n == "map_openrouter_usage")
            .unwrap();
        assert_eq!(
            decision(&json!({"payload": {"data": 7}})).unwrap_err(),
            "Invalid upstream response"
        );
    }

    /// **`limit` IS ALWAYS PRESENT IN THE AMD MAP AND IS `null` WHEN THERE IS NO NUMBER** — it is assigned outside
    /// the guard. And `interval` is the literal `minute`.
    ///
    /// MUTATION: move the `limit` insert inside the number guard.
    /// RESULT:   the first assertion fails: an absent cap would be an ABSENT KEY, and the console would render a
    ///           missing row where it used to render a null one.
    #[test]
    fn the_amd_map_always_carries_a_limit() {
        assert_eq!(
            call("map_amd_usage", json!({"payload": {}})).to_string(),
            r#"{"limit":null}"#
        );
        assert_eq!(
            call(
                "map_amd_usage",
                json!({"payload": {"daily_cost_used_usd": 1.5, "rpm_limit": 60}})
            )
            .to_string(),
            r#"{"usage":1.5,"limit":null,"rateLimit":{"limit":60,"interval":"minute"}}"#
        );
    }

    /// **THE LABEL'S ORG FALLS BACK ON JAVASCRIPT FALSINESS, NOT ON ABSENCE.** `payload.organization_id || "radeon"`
    /// answers `"radeon"` for `0` and `""` as well as for a missing key — and `5` is TRUTHY, so it becomes `"5"`.
    ///
    /// MUTATION: fall back only when the key is absent.
    /// RESULT:   the `0` and `""` assertions fail — the console would print an empty org where it printed `radeon`.
    #[test]
    fn the_amd_label_falls_back_on_falsiness() {
        let label = |org: Value| {
            call(
                "map_amd_usage",
                json!({"payload": {"all_time": {"requests": 2, "total_tokens": 9},
                                                     "organization_id": org}}),
            )["label"]
                .clone()
        };
        assert_eq!(label(json!(0)), json!("radeon · 2 req · 9 tok"));
        assert_eq!(label(json!("")), json!("radeon · 2 req · 9 tok"));
        assert_eq!(label(json!(null)), json!("radeon · 2 req · 9 tok"));
        assert_eq!(label(json!("acme")), json!("acme · 2 req · 9 tok"));
        assert_eq!(label(json!(5)), json!("5 · 2 req · 9 tok"));
    }

    /// **`"limit" in payload` IS A KEY CHECK, NOT A VALUE CHECK** — the one place the source asks whether a key
    /// exists. A `null` limit is CARRIED; an absent one adds nothing.
    ///
    /// MUTATION: use `payload.limit != null` instead of the key check.
    /// RESULT:   the `limit: null` assertion fails: the key would be dropped where the source carries it.
    #[test]
    fn the_og_map_carries_a_null_limit_but_not_an_absent_one() {
        assert_eq!(
            call("map_og_usage", json!({"payload": {"limit": null}})).to_string(),
            r#"{"limit":null}"#
        );
        assert_eq!(
            call("map_og_usage", json!({"payload": {}})).to_string(),
            "{}"
        );
        assert_eq!(
            call(
                "map_og_usage",
                json!({"payload": {"used": 3, "plan": "go", "balance": 1}})
            )
            .to_string(),
            r#"{"usage":3,"balance":1,"label":"go"}"#
        );
    }

    /// The multi-window shape, where each window keeps only the four fields the source names — and `reset_at`
    /// becomes `resetAt`.
    #[test]
    fn the_og_windows_keep_the_four_named_fields() {
        let got = call(
            "map_og_usage",
            json!({"payload": {"windows": {"5h": {"used": 1, "limit": null, "remaining": 2,
                                                  "reset_at": "soon", "ignored": true}}}}),
        );
        assert_eq!(
            got.to_string(),
            r#"{"windows":{"5h":{"used":1,"limit":null,"remaining":2,"resetAt":"soon"}}}"#
        );
        // A non-object window is DROPPED, and an object with none of the four becomes `{}` — carried, not skipped.
        assert_eq!(
            call(
                "map_og_usage",
                json!({"payload": {"windows": {"a": 7, "b": {}}}})
            )
            .to_string(),
            r#"{"windows":{"b":{}}}"#
        );
    }

    /// **THE TABLE IS THE ONLY WAY IN, AND AN UNKNOWN NAME IS A REFUSAL.** The mutation this guards is the one that
    /// would matter: a fallback that answered `null` for an unknown name would make a typo look like a decision.
    #[test]
    fn every_row_is_reachable_and_nothing_else_is() {
        assert_eq!(
            DECISIONS.len(),
            5,
            "the table changed — update this count deliberately"
        );
        for (name, _) in DECISIONS {
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
