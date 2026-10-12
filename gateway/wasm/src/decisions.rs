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
const DECISIONS: [(&str, Decision); 2] = [
    ("upstream_models_url", upstream_models_url),
    ("offered_ids", offered_ids),
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

    /// **THE TABLE IS THE ONLY WAY IN, AND AN UNKNOWN NAME IS A REFUSAL.** The mutation this guards is the one that
    /// would matter: a fallback that answered `null` for an unknown name would make a typo look like a decision.
    #[test]
    fn every_row_is_reachable_and_nothing_else_is() {
        assert_eq!(
            DECISIONS.len(),
            2,
            "the table changed — update this count deliberately"
        );
        for (name, _) in DECISIONS {
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
