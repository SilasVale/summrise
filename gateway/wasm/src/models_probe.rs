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

/// **FETCH A PROVIDER'S LISTING — the one network call a scheduled run makes, and the reason `observe` takes the
/// listing as a parameter.**
///
/// Three outcomes, and the middle one is what the whole lifecycle rests on:
///
///   * **`Some(ids)`** — the upstream answered with a listing (which may be empty; `conclusive` decides whether an
///     empty one is evidence, and it is not);
///   * **`None`** — the upstream could not be reached, answered with a non-2xx, or sent a body nothing can read.
///     **This is not an error the run handles: it is an INCONCLUSIVE run**, which breaks any streak and writes no
///     row state, so an outage can never demote a model;
///   * and there is no third case — **a fetch that cannot be made is indistinguishable from one that failed, because
///     the run does the same thing either way.**
///
/// The response body is read through the same `offered_ids` the console's probe uses, so a provider that renames a
/// dialect changes both readers at once rather than one of them silently.
pub async fn fetch_listing(env_json: &Value, upstream: &str) -> Option<Vec<String>> {
    let endpoint = upstream_models_url(upstream)?;
    let headers = worker::Headers::new();
    let _ = headers.set("accept", "application/json");
    let init = RequestInit {
        method: Method::Get,
        headers,
        redirect: worker::RequestRedirect::Manual,
        ..Default::default()
    };
    let timeout = crate::reliability::upstream_timeout_ms(Some(env_json)) as u64;
    let mut response = crate::device::fetch_with_timeout(&endpoint, &init, timeout)
        .await
        .ok()?;
    if !(200..300).contains(&response.status_code()) {
        return None;
    }
    let payload = response.json::<Value>().await.ok()?;
    let ids = offered_ids(&payload);
    // **AN UNREADABLE BODY IS A FAILED FETCH, NOT AN EMPTY LISTING.** `offered_ids` answers `[]` for a shape it does
    // not recognise, and passing that on would make a renamed dialect look like "the provider retired everything" —
    // the exact failure `conclusive` was written to prevent, arriving one layer too early to be caught by it.
    if ids.is_empty() && !payload_looks_like_a_listing(&payload) {
        return None;
    }
    Some(ids)
}

/// **THE LISTING FOR ONE OF OUR CHANNELS, ROUTED THE WAY A REQUEST WOULD BE ROUTED.**
///
/// The upstream comes from `routing::pick_route` — **the router's own answer, called rather than restated** — so a
/// scheduled run cannot ask a different URL than the requests it is drawing conclusions about. That is the same rule
/// the console's probe follows, and for the same reason: two answers to "where does this prefix go" is how a
/// catalogue ends up describing a channel nobody uses.
pub async fn fetch_listing_for(env_json: &Value, bare: &str) -> Option<Vec<String>> {
    let route = crate::routing::pick_route(bare, Some(env_json), None, "");
    let upstream = route.upstream.clone();
    if upstream.is_empty() {
        return None;
    }
    fetch_listing(env_json, &upstream).await
}

/// Does this payload LOOK like a listing, even if it carries no ids? An empty `data: []` is a provider that serves
/// nothing (evidence); `{"error": "..."}` is not.
fn payload_looks_like_a_listing(payload: &Value) -> bool {
    payload.get("data").and_then(Value::as_array).is_some() || payload.as_array().is_some()
}

/// **THE PREFIXES THIS CONSOLE CAN ACTUALLY CHECK TODAY, AND THE PIN IS THE POINT.** A scheduled run asks an
/// upstream for its listing; four of our channels publish one at a URL this worker knows (`og`, `nv`, `gmi`, `cm`),
/// and the others (`or`, `qw`, `r4`, `amd`) do not — OpenRouter's catalogue is a different API, and the Anthropic
/// passthroughs have no `/models` at all. **Widening the scope later is a deliberate edit with a reason**, not a
/// side effect of a loop that happens to iterate every prefix.
pub fn listable_prefixes() -> &'static [&'static str] {
    &["og", "nv", "gmi", "cm"]
}

/// **THE UPSTREAM SPELLINGS OF ONE PREFIX'S CURATED ROWS** — the bare names the provider's own listing would carry,
/// which is NOT the registry id: `og/deepseek-v4.1-flash` is `deepseek-flash` upstream (`OG_WIRE_REMAP`, and the
/// `wire` facet when a row has one). **A comparison that used the registry id would conclude that every model had
/// vanished**, which is the loudest possible way to be wrong.
pub fn upstream_spellings(prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for model in crate::registry::MODEL_REGISTRY {
        if !model.id.starts_with(prefix) {
            continue;
        }
        let bare = model.wire.unwrap_or_else(|| {
            model
                .id
                .split_once('/')
                .map(|(_, rest)| rest)
                .unwrap_or(model.id)
        });
        out.push(bare.to_string());
    }
    out
}

/// **ONE REGISTRY ID, AS THE UPSTREAM SPELLS IT** — the per-id half of `upstream_spellings`, and the thing a caller
/// gets wrong: the ring compares UPSTREAM names, so asking it about `og/deepseek-v4.1-flash` when the listing carries
/// `deepseek-flash` finds nothing and reports `Unchanged` forever. **A silent `Unchanged` is the failure mode of this
/// whole design** — it looks exactly like "no news", which is why the mapping is a function rather than a paragraph.
pub fn upstream_spelling(id: &str) -> String {
    for model in crate::registry::MODEL_REGISTRY {
        if model.id == id {
            return model
                .wire
                .map(str::to_string)
                .unwrap_or_else(|| bare(id).to_string());
        }
    }
    bare(id).to_string()
}

fn bare(id: &str) -> &str {
    id.split_once('/').map(|(_, rest)| rest).unwrap_or(id)
}

/// **IS A LISTING EVIDENCE?** — the floor the whole lifecycle rests on, because a demotion is decided by ABSENCE.
///
/// Three answers, and the first two are the ones that matter:
///
///   * **an empty (or unrecognised) listing is NOT conclusive** — a 200 with an empty body, or a body whose shape
///     changed under us, must hide nothing. This is the plan's first review focus, and it is the failure mode that
///     would demote every model on a provider whose API answered a stub.
///   * **a listing carrying NONE of the curated rows is not conclusive either** — the listing we received is then
///     not the list we think we are reading (a renamed dialect, a proxy's error page that parsed as JSON, the wrong
///     provider). Absence of everything is not evidence that one thing is gone.
///   * **one curated row present makes it conclusive**, and only then: from that point, a curated row's ABSENCE
///     means the upstream really does not serve it, which is what a demotion is allowed to act on.
///
/// The comparison is NORMALISED (`upstream_spellings` provides the right side) and case-insensitive on the wire,
/// because a provider that renames `DeepSeek-V4` to `deepseek-v4` has not retired anything.
pub fn conclusive(offered: &[String], prefix: &str, curated: &[&str]) -> bool {
    if offered.is_empty() {
        return false;
    }
    let normal = |s: &str| s.trim().to_ascii_lowercase();
    let wanted: Vec<String> = curated.iter().map(|c| normal(c)).collect();
    let _ = prefix;
    offered
        .iter()
        .any(|id| wanted.iter().any(|c| *c == normal(id)))
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

    /// **AN EMPTY LISTING IS EVIDENCE ONLY WHEN THE BODY IS A LISTING.** This is the guard's whole job: `offered_ids`
    /// answers `[]` for a shape it does not recognise, and passing that on would make **a renamed dialect look like
    /// "the provider retired everything"** — the failure `conclusive` exists to prevent, arriving one layer earlier
    /// than `conclusive` can see it.
    #[test]
    fn only_a_listing_shaped_body_can_be_empty_and_still_be_evidence() {
        // A provider that genuinely serves nothing: evidence, and `conclusive` will read it as such.
        assert!(payload_looks_like_a_listing(
            &serde_json::json!({"data": []})
        ));
        assert!(payload_looks_like_a_listing(&serde_json::json!([])));
        // A body that is NOT a listing: a failed fetch, whatever it says.
        assert!(!payload_looks_like_a_listing(
            &serde_json::json!({"error": "rate limited"})
        ));
        assert!(
            !payload_looks_like_a_listing(&serde_json::json!({"object": "list"})),
            "a shape we do not know is not an empty catalogue"
        );
        assert!(!payload_looks_like_a_listing(&serde_json::json!("nope")));
        assert!(!payload_looks_like_a_listing(&serde_json::Value::Null));
    }

    /// **THE THREE ANSWERS, AND THE TWO THAT MATTER ARE THE NEGATIVE ONES.** A demotion is decided by absence, so a
    /// listing that is empty — or that carries none of our rows — must be `false`, or one bad upstream response
    /// retires the catalogue.
    #[test]
    fn an_empty_or_unrelated_listing_is_not_conclusive() {
        let curated = ["deepseek-flash", "mimo-v2.5"];
        assert!(
            !conclusive(&[], "og", &curated),
            "an empty 200 is evidence of nothing"
        );
        let offered = vec!["something-new".to_string(), "another".to_string()];
        assert!(
            !conclusive(&offered, "og", &curated),
            "a listing with none of our rows is not the list we think we are reading"
        );
    }

    /// One curated row present, and the listing becomes evidence — which is what lets an absence mean something.
    #[test]
    fn one_curated_row_makes_a_listing_conclusive() {
        let curated = ["deepseek-flash", "mimo-v2.5"];
        let offered = vec!["something-new".to_string(), "deepseek-flash".to_string()];
        assert!(conclusive(&offered, "og", &curated));
        // Case and padding are not a retirement.
        let offered = vec!["  DeepSeek-Flash ".to_string()];
        assert!(conclusive(&offered, "og", &curated));
    }

    /// **THE UPSTREAM SPELLING IS NOT THE REGISTRY ID**, and a comparison that forgot it would report every model
    /// as vanished: `og/deepseek-v4.1-flash` answers to `deepseek-flash` upstream.
    #[test]
    fn the_upstream_spellings_are_the_wire_names() {
        let og = upstream_spellings("og");
        assert!(
            og.iter().any(|s| s == "deepseek-flash"),
            "the wire remap was lost: {og:?}"
        );
        assert!(
            !og.iter().any(|s| s.starts_with("og/")),
            "a registry id leaked into the listing names"
        );
        assert!(upstream_spellings("nothing-claims-this").is_empty());
    }

    /// The scope pin: four prefixes are checkable today and the rest are not.
    #[test]
    fn only_four_prefixes_are_listable_today() {
        assert_eq!(listable_prefixes(), &["og", "nv", "gmi", "cm"]);
        for unlistable in ["or", "qw", "r4", "amd"] {
            assert!(
                !listable_prefixes().contains(&unlistable),
                "{unlistable} is not checkable and must not be probed by a timer"
            );
        }
    }
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
