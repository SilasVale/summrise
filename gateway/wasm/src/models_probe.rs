//! `plugins/models-probe.ts` — ASK A PROVIDER WHAT IT ACTUALLY SERVES.
//!
//! **WHY IT EXISTS.** The catalogue is hand-maintained: a model is added by NAME, and nothing tells an operator
//! that the upstream has since added or retired one. `agent/model-drift/` has been able to answer that since round
//! 57 — as an ops tool, run from a laptop, with its own copy of the upstream URLs. This brings the same question
//! into the console, where the person editing the catalogue is.
//!
//! **THE GUARD RUNS AT DIAL TIME**, on the URL about to be fetched, and it is the SAME function the registration
//! path uses (`guard::device_host_error`). Registration already validates a custom baseURL, but a record that
//! predates a guard — or one written by a path that forgot to call it — would otherwise be dialled anyway. The
//! rule this repository learned the hard way (round 95) is that **a guard is only real at the point of use**.
//!
//! **NO REDIRECTS ARE FOLLOWED**, for round 90's reason: a 3xx is a different host, and the request carries a
//! credential.
//!
//! **DERIVE, DO NOT GUESS.** The model-list endpoint is derived from the upstream the router already resolved
//! (`…/chat/completions` → `…/models`). For a dialect where that derivation does not hold — the Anthropic-shaped
//! channel — the probe says it CANNOT CHECK rather than inventing a URL. A wrong endpoint would produce a
//! confident, wrong catalogue diff.

use serde_json::Value;
use worker::{Env, Method, RequestInit, Response};

/// `upstreamModelsUrl(upstream)` — the upstream's model-list endpoint, or `None` when this dialect has no
/// derivable one.
pub fn upstream_models_url(upstream: &str) -> Option<String> {
    const SUFFIX: &str = "/chat/completions";
    upstream
        .strip_suffix(SUFFIX)
        .map(|head| format!("{head}/models"))
}

/// `offeredIds(payload)` — upstream ids from either dialect we speak: an OpenAI-style `{data:[{id}]}`, an
/// Anthropic-style one (the same shape), or a bare array. Anything else is not a list.
///
/// **A NON-STRING ROW IS COERCED, NOT DROPPED**: `String(r?.id ?? "")` turns a numeric id into its text, and
/// `?? ""` (not `||`) means a `0` id becomes `"0"` — the empty string is what a MISSING id gives, and the
/// `.filter(Boolean)` afterwards removes it.
pub fn offered_ids(payload: &Value) -> Vec<String> {
    let rows = if let Some(array) = payload.as_array() {
        Some(array)
    } else {
        payload.get("data").and_then(Value::as_array)
    };
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.iter()
        .map(|r| {
            if r.is_string() {
                summrise_core::text::js_string(r)
            } else {
                summrise_core::text::js_or_empty(r.get("id"))
            }
        })
        .map(|s| summrise_core::text::js_trim(&s).to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The kind→user-key table, **DERIVED from [`crate::byok::BYOK_CHANNELS`]** — the source's own lesson (round
/// 191): the vocabulary had four independently-typed copies with nothing comparing them, and the shared source is
/// the answer. A kind the table does not know answers no key, which is the channel's own business.
fn byok_key_for_kind(kind: &str) -> Option<&'static str> {
    crate::byok::BYOK_CHANNELS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map(|(_, user_key, ..)| *user_key)
}

/// Everything the probe can answer. Each arm is a distinct RESPONSE SHAPE, which is why they are a type rather
/// than a string: the console branches on `checked`.
pub enum ProbeOutcome {
    /// `jsonOk({checked: false, reason})` — nothing was compared, and the reason says why.
    NotChecked {
        reason: String,
        endpoint: Option<String>,
    },
    /// `jsonError(502, …, "config_error")` — the dial was refused by the SSRF guard.
    Refused { reason: String },
    /// The diff.
    Checked {
        endpoint: String,
        offered: Vec<String>,
        advertised: Vec<String>,
        not_advertised: Vec<String>,
        not_offered: Vec<String>,
    },
}

impl ProbeOutcome {
    pub fn into_response(self) -> worker::Result<Response> {
        let body = match self {
            ProbeOutcome::NotChecked { reason, endpoint } => match endpoint {
                Some(endpoint) => {
                    serde_json::json!({"checked": false, "endpoint": endpoint, "reason": reason})
                }
                None => serde_json::json!({"checked": false, "reason": reason}),
            },
            ProbeOutcome::Refused { reason } => {
                return crate::json_error(502, &reason, "config_error");
            }
            ProbeOutcome::Checked {
                endpoint,
                offered,
                advertised,
                not_advertised,
                not_offered,
            } => serde_json::json!({
                "checked": true,
                "endpoint": endpoint,
                "offered": offered,
                "advertised": advertised,
                "notAdvertised": not_advertised,
                "notOffered": not_offered,
            }),
        };
        crate::json(serde_json::to_string(&body).unwrap_or_default(), 200)
    }
}

/// The probe, from the prefix down. The caller has already gated and read the body.
///
/// **THE KEY IS THE ONE THE REQUEST WOULD USE**: a custom provider's own record, otherwise the CALLER's BYOK key
/// for that kind. A channel that needs one and has none is reported NOT CHECKED — never as "offers nothing",
/// which is the same distinction the ops tool makes.
pub async fn probe(
    env: &Env,
    env_json: &Value,
    bare: &str,
    provider: Option<&Value>,
    caller_id: &str,
) -> ProbeOutcome {
    // A PREFIX THAT NOBODY CLAIMS IS NOT A ROUTE TO TEST: `resolveRoute` falls through to the default channel
    // for an unknown prefix, which is the correct ROUTING rule and the WRONG answer here — the probe would dial,
    // and report on, a channel the caller never named.
    let built_in = summrise_core::catalogue::RESERVED_PREFIXES.contains(&bare) || bare == "none";
    if provider.is_none() && !built_in {
        return ProbeOutcome::NotChecked {
            reason: format!(
                "{bare}/ is not a provider — requests for it would fall through to the default channel"
            ),
            endpoint: None,
        };
    }
    // **THE ROUTER'S OWN ANSWER, SO THE PROBE CANNOT TEST A DIFFERENT ROUTE THAN REQUESTS TAKE.** A custom
    // provider's record answers through `provider_route`; a built-in through `pick_route`, whose path argument
    // is the CHAT path (the probe asks about the dial a chat request would make).
    let route = match provider {
        Some(p) => match crate::routing::provider_route(p) {
            (info, None) => info,
            (_, Some(reason)) => {
                return ProbeOutcome::NotChecked {
                    reason: if reason.is_empty() {
                        format!("{bare} is not a routable prefix")
                    } else {
                        reason
                    },
                    endpoint: None,
                };
            }
        },
        None => crate::routing::pick_route(bare, Some(env_json), None, ""),
    };
    let upstream = route.upstream.clone();
    let kind = route.kind.to_string();
    // The decision is the DECISION ROUTE's function, called rather than restated — `/__decision` and this route
    // must not be able to disagree about where a provider's model list lives.
    let decided =
        crate::decisions::upstream_models_url(&serde_json::json!({ "upstream": upstream }))
            .unwrap_or(Value::Null);
    let endpoint = match decided.as_str() {
        Some(s) => Some(s.to_string()),
        None => upstream_models_url(&upstream),
    };
    let Some(endpoint) = endpoint else {
        return ProbeOutcome::NotChecked {
            reason: format!(
                "{bare} speaks a dialect whose model list this build cannot derive — the checked list lives in \
                 agent/model-drift/"
            ),
            endpoint: None,
        };
    };

    // THE GUARD, ON THE URL ABOUT TO BE FETCHED.
    let Ok(parsed) = url::Url::parse(&endpoint) else {
        return ProbeOutcome::NotChecked {
            reason: format!("{bare}: the resolved upstream is not a URL"),
            endpoint: None,
        };
    };
    let host = parsed.host_str().unwrap_or_default().to_string();
    if let Some(refused) = summrise_core::guard::device_host_error(&host) {
        return ProbeOutcome::Refused {
            reason: format!("{bare}: {refused}"),
        };
    }

    let mut key = String::new();
    if kind == "custom" {
        key = crate::store::provider_key(env_json, provider);
    } else if let Some(name) = byok_key_for_kind(&kind) {
        let ukeys = crate::user_store::get_user_keys(env, caller_id).await;
        key = summrise_core::text::js_or_empty(ukeys.get(name));
    }

    let headers = worker::Headers::new();
    let _ = headers.set("accept", "application/json");
    if !key.is_empty() {
        let _ = headers.set("authorization", &format!("Bearer {key}"));
    }
    let init = RequestInit {
        method: Method::Get,
        headers,
        redirect: worker::RequestRedirect::Manual,
        ..Default::default()
    };
    let timeout = crate::reliability::upstream_timeout_ms(Some(env_json)) as u64;
    let mut response = match crate::device::fetch_with_timeout(&endpoint, &init, timeout).await {
        Ok(response) => response,
        Err(message) => {
            return ProbeOutcome::NotChecked {
                reason: format!("could not reach the upstream: {message}"),
                endpoint: Some(endpoint),
            };
        }
    };
    if response.status_code() == 401 || response.status_code() == 403 {
        return ProbeOutcome::NotChecked {
            reason: "the upstream rejected this credential (401/403) — nothing was compared"
                .to_string(),
            endpoint: Some(endpoint),
        };
    }
    if !(200..300).contains(&response.status_code()) {
        return ProbeOutcome::NotChecked {
            reason: format!("upstream answered {}", response.status_code()),
            endpoint: Some(endpoint),
        };
    }
    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(error) => {
            return ProbeOutcome::NotChecked {
                reason: format!("could not reach the upstream: {error}"),
                endpoint: Some(endpoint),
            };
        }
    };
    let offered = crate::decisions::offered_ids(&serde_json::json!({ "payload": payload }))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|rows| {
            rows.iter()
                .map(summrise_core::text::js_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| offered_ids(&payload));

    // What WE advertise for this prefix, from the authoritative advertised set (the same list `/v1/models`
    // serves), minus the prefix so the two are comparable.
    let advertised: Vec<String> = crate::admin_routes::advertised_ids_for(env)
        .await
        .into_iter()
        .filter(|id| id.starts_with(&format!("{bare}/")))
        .collect();
    let offered_set: Vec<&String> = offered.iter().collect();
    // OFFERED BUT NOT ADVERTISED — the adopt candidates, returned PREFIXED so that adopting one is a direct call
    // to the existing add-model route. A bare id here would leave the panel to re-derive the prefix, which is the
    // exact join that once filed a model under the wrong channel.
    let not_advertised: Vec<String> = offered
        .iter()
        .filter(|id| !advertised.iter().any(|a| a == &format!("{bare}/{id}")))
        .map(|id| format!("{bare}/{id}"))
        .collect();
    // ADVERTISED BUT NOT LISTED — printed as a CHECK, never a verdict: the router normalises names (wire
    // remaps, `[1m]` markers), so a raw diff reports false drift.
    let not_offered: Vec<String> = advertised
        .iter()
        .filter(|id| {
            let bare_id = &id[bare.len() + 1..];
            !offered_set.iter().any(|o| o.as_str() == bare_id)
        })
        .cloned()
        .collect();
    ProbeOutcome::Checked {
        endpoint,
        offered,
        advertised,
        not_advertised,
        not_offered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE ENDPOINT DERIVATION, AND THE DIALECT THAT HAS NONE.**
    ///
    /// MUTATION: return `Some(format!("{upstream}/models"))` for every upstream.
    /// RESULT:   the anthropic-shaped case fails, and the probe would invent a URL — producing a confident,
    ///           wrong catalogue diff, which is the failure the source's own comment refuses.
    #[test]
    fn the_models_url_is_derived_or_refused() {
        assert_eq!(
            upstream_models_url("https://acme.test/v1/chat/completions").as_deref(),
            Some("https://acme.test/v1/models")
        );
        assert_eq!(upstream_models_url("https://acme.test/v1/messages"), None);
        assert_eq!(upstream_models_url(""), None);
    }

    /// **`offeredIds` READS BOTH DIALECTS, AND COERCES THE WAY THE SOURCE DOES** (`String(r?.id ?? "")`, then a
    /// trim and a truthiness filter).
    #[test]
    fn the_offered_ids_read_both_shapes() {
        assert_eq!(
            offered_ids(&json!({"data": [{"id": "a"}, {"id": " b "}, {"id": 5}, {}]})),
            vec!["a".to_string(), "b".to_string(), "5".to_string()],
            "a numeric id is coerced, a missing one is dropped, and the whitespace is trimmed"
        );
        assert_eq!(
            offered_ids(&json!(["a", "b"])),
            vec!["a".to_string(), "b".to_string()],
            "a bare array is a list too"
        );
        assert_eq!(
            offered_ids(&json!({"object": "list"})),
            Vec::<String>::new()
        );
        assert_eq!(offered_ids(&json!("nope")), Vec::<String>::new());
    }
}
