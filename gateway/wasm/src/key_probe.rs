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
use worker::{Headers, Method, RequestInit};

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
