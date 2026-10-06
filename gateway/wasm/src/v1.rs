//! THE `/v1` FRONT DOOR — the glue between `worker::Fetch`/KV and the ported decisions.
//!
//! WHY THIS FILE EXISTS AND WHY IT IS THIS SHORT. Every decision on this path was ported and proved
//! separately: **auth, the route match, the retired check**, the per-arm key gates, the model chain, the
//! two-phase dispatch and the response shaping. What is left is the I/O — four KV reads, one upstream call —
//! and this file is that, so that the parts that CAN be proved in-process stay free of `worker::`.
//!
//! **THE ORDER OF THOSE DECISIONS IS PART OF THE PORT, AND IT WAS WRONG HERE UNTIL 2026-10-06** — this
//! header listed "the route match" first and the file ran the retired check before the token was read. Both
//! are the reverse of `handleGatewayImpl`, which authenticates (401), matches the route (404), and only then
//! reads `retiredModelHint` (400). The fix is in `request_shape.rs` (`v1_plan`/`v1_dispatch`) and the
//! measurement is `fixtures/front-door-corpus.json`.
//!
//! THE KEYS IT READS, each matching `store/`'s own:
//!
//! ```text
//!     token:<token>          -> the uid
//!     user:<uid>             -> the user record (whose `enabled` the auth gate checks)
//!     ukeys:<uid>            -> the user's BYOK keys
//!     settings:<name>        -> a global setting, with the Worker var of the same name as the fallback
//! ```
//!
//! AND ONE THING IT DOES NOT DO YET, NAMED RATHER THAN HIDDEN: the true-streaming path. `v1_finish` answers
//! `ArmOutcome::Stream` for a real SSE upstream, and this file returns a **501** for it — the ported encoder
//! exists (`stream.rs`), but wiring a live `ReadableStream` through it is its own step with its own proof.

use worker::*;

use crate::request_shape::{
    v1_finish, v1_plan, ArmOutcome, UpstreamAnswer, V1Plan, V1PlanInputs, V1Request,
};
use crate::responses::Built;

/// A `Built` becomes the `Response` the runtime sends.
pub fn to_response(built: Built) -> Result<Response> {
    let headers = Headers::new();
    for (k, v) in &built.headers {
        headers.set(k, v)?;
    }
    Ok(Response::ok(built.body)?
        .with_status(built.status)
        .with_headers(headers))
}

/// One KV string, or `None`.
async fn kv_text(env: &Env, key: &str) -> Option<String> {
    let kv = env.kv("KEYS").ok()?;
    kv.get(key).text().await.ok().flatten()
}

/// The same read, for the keys that hold a JSON ARRAY — `models:disabled`, `models:custom`,
/// `providers:custom`, `models:overrides`. **AN ABSENT KEY IS AN EMPTY LIST AND NOT AN ERROR**, which is
/// what `readList` does in the shipping store: a deployment nobody has configured lists the registry.
async fn kv_json<T: serde::de::DeserializeOwned + Default>(env: &Env, key: &str) -> T {
    match kv_text(env, key).await {
        Some(text) => serde_json::from_str(&text).unwrap_or_default(),
        None => T::default(),
    }
}

/// **`GET …/models` IS WIRED NOW.** This comment used to say it was not — "`GET /v1/models`'s advertised
/// set is not wired here" — and a live comparison is what turned that from a known gap into a measured one:
/// the wasm answered **404** where the shipping gateway answered **200**, on both `/v1/models` and
/// `/models`. The listing itself was already ported (`responses::models_listing`, replayed by
/// `fixtures/models-corpus.json`); what was missing was this predicate and the arm below.
///
/// **`ends_with`, NOT EQUALITY, AND NO PATH REWRITE IS NEEDED**: the shipping route tests
/// `path.endsWith("/models")` and rewrites `/models` to `/v1/models` before calling its handler, but the
/// listing never reads the path — so matching either spelling here is the same decision, made once.
pub fn is_v1_route(method: &Method, path: &str) -> bool {
    // **THE SHIPPING FRONT DOOR ROUTES BY PREFIX AND THEN AUTHENTICATES.** `index.ts` is
    // `if (!path.startsWith("/v1/")) return 404` and hands everything else to `handleGateway`, which checks
    // the token BEFORE it looks at the method or the path. So `GET /v1/messages` — a method that route does
    // not take — answers **401** there and answered **404** here, and so did `POST /v1/models`. A live
    // comparison measured both; this predicate was the cause.
    //
    // **AND `starts_with`, NOT `ends_with`, BECAUSE THE SOURCE IS A PREFIX TEST.** The four `ends_with`
    // forms this replaces also matched `/foo/v1/messages`, which the shipping front door answers 404 — a
    // second divergence the same comparison would have found on a path nobody sends. The arms below still
    // match by suffix, which is correct for them: they are choosing among paths that already passed here.
    if method == &Method::Get && path.ends_with("/models") {
        return true;
    }
    path.starts_with("/v1/")
}

/// The front door for `/v1/*`.
pub async fn handle(mut req: Request, env: Env) -> Result<Response> {
    let url = req.url()?;
    let method = req.method();
    let path = url.path().to_string();
    if !is_v1_route(&method, &path) {
        return Response::error("Not Found", 404);
    }
    // **`GET …/models` — PUBLIC, AND BEFORE THE AUTH GATE.** The shipping route says so in as many words:
    // "public, no auth required (DSH/OpenAI clients list models first)". Putting it after the gate would
    // make every client's first request a 401, which is a behaviour the live comparison would have caught
    // and a code review would not.
    if method == Method::Get && path.ends_with("/models") {
        let disabled: Vec<String> = kv_json(&env, "models:disabled").await;
        let custom: Vec<serde_json::Value> = kv_json(&env, "models:custom").await;
        let providers: Vec<serde_json::Value> = kv_json(&env, "providers:custom").await;
        let override_records: Vec<serde_json::Value> = kv_json(&env, "models:overrides").await;

        let registry_ids: Vec<&str> = crate::registry::MODEL_REGISTRY.iter().map(|m| m.id).collect();
        let custom_ids: Vec<String> = custom
            .iter()
            .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
            .collect();
        let provided = crate::store::advertised_provider_models(&providers);
        let provider_ids: Vec<String> = provided
            .iter()
            .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
            .collect();

        let live = crate::store::advertised_ids(&registry_ids, &disabled, &custom_ids, &provider_ids);
        let extra = crate::store::extra_model_entries(&custom, &provided);
        let overrides: serde_json::Map<String, serde_json::Value> = override_records
            .iter()
            .filter_map(|o| {
                o.get("id")
                    .and_then(|i| i.as_str())
                    .map(|i| (i.to_string(), o.clone()))
            })
            .collect();
        return to_response(crate::responses::models_listing(
            &crate::registry::MODEL_REGISTRY,
            &live,
            &extra,
            &overrides,
        ));
    }

    let raw = req.text().await?;
    let parsed: Option<serde_json::Value> = serde_json::from_str(&raw).ok();
    let model = parsed
        .as_ref()
        .and_then(|b| b.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();

    // **THE RETIRED GATE IS NOT HERE ANY MORE, AND THAT IS A FIX RATHER THAN A TIDY-UP.** It ran at this
    // point — before the token was read — so a retired id sent with NO token answered **400** where the
    // shipping gateway answers **401**, because `handleGatewayImpl` authenticates, matches the route and only
    // THEN reads `retiredModelHint` (400). It lives in `v1_plan` now, in that position, with
    // `fixtures/front-door-corpus.json` as the measurement. The model is still resolved here because the
    // plan takes it as an argument.

    // AUTH: the token names a uid, the uid names a user, and the user's `enabled` is the gate's business.
    // **THE TOKEN HAS TWO DOORS AND THIS READ ONE UNTIL 2026-10-06** — `x-api-key`, then
    // `Authorization: Bearer`, which is the spelling OpenAI-compatible clients send. The rule is a pure
    // decision (`request_shape::effective_token`) and `fixtures/auth-header-corpus.json` is its measurement.
    let headers = req.headers();
    let token = crate::request_shape::effective_token(
        headers.get("x-api-key").ok().flatten().as_deref(),
        headers.get("authorization").ok().flatten().as_deref(),
    );
    let user = match kv_text(&env, &format!("token:{token}")).await {
        Some(uid) => kv_text(&env, &format!("user:{uid}"))
            .await
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()),
        None => None,
    };
    let byok = match user
        .as_ref()
        .and_then(|u| u.get("id"))
        .and_then(|i| i.as_str())
    {
        Some(uid) => kv_text(&env, &format!("ukeys:{uid}"))
            .await
            .and_then(|t| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok()
            })
            .unwrap_or_default(),
        None => serde_json::Map::new(),
    };

    // THE SETTINGS the chain reads: `settings:US_PROXY` with the Worker var as its fallback.
    let us_proxy_setting = crate::store::global_setting_enabled(
        crate::store::get_global_setting(
            kv_text(&env, "settings:US_PROXY").await.as_deref(),
            env.var("US_PROXY").ok().map(|v| v.to_string()).as_deref(),
        )
        .as_deref(),
    );
    let env_json = serde_json::json!({
        "US_PROXY": if us_proxy_setting { "1" } else { "0" },
    });

    // THE CHAIN: the model -> the route, then the two-phase dispatch.
    let resolved = crate::routing::resolve_model(&model, us_proxy_setting, &path, Some(&env_json));
    let request = V1Request {
        method: method.to_string(),
        path: path.clone(),
        user,
        kind: resolved.route.kind.to_string(),
        byok,
        env: env_json,
        raw_text: serde_json::json!(raw),
    };
    let og_session: Vec<(String, String)> = Vec::new();
    let plan = v1_plan(&V1PlanInputs {
        req: &request,
        route: &resolved.route,
        model: &model,
        prefix: &resolved.prefix,
        upstream_model: &resolved.upstream_model,
        parsed_body: parsed.as_ref(),
        og_session: &og_session,
        scanned: None,
    });
    let V1Plan::Dial(call) = plan else {
        let V1Plan::Respond(built) = plan else {
            unreachable!()
        };
        return to_response(built);
    };

    // PHASE TWO'S I/O: one upstream call.
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    // `Headers::set` takes `&self` in workers-rs, so this is not `mut`.
    let headers = Headers::new();
    for (k, v) in &call.request.headers {
        headers.set(k, v)?;
    }
    init.with_headers(headers);
    init.with_body(Some(call.request.body.clone().into()));
    let upstream = Fetch::Request(Request::new_with_init(&call.request.url, &init)?)
        .send()
        .await;
    let mut upstream_stream = None;
    let mut live = false;
    let answer = match upstream {
        Ok(mut res) => {
            let status = res.status_code();
            let content_type = res.headers().get("content-type")?.unwrap_or_default();
            let retry_after = res.headers().get("retry-after")?;
            // **THE BODY IS TAKEN ONE WAY OR THE OTHER, NEVER BOTH.** `text()` consumes it and `stream()`
            // hands it over live, so the question "is this a live stream?" is asked FIRST — through the same
            // function `messages_response` uses — and the losing branch is never called.
            live = crate::request_shape::is_live_stream(
                call.is_translate,
                call.wants_stream,
                status,
                &content_type,
            );
            if live {
                upstream_stream = res.stream().ok();
                Some(UpstreamAnswer {
                    status,
                    content_type,
                    json: None,
                    retry_after,
                    text: String::new(),
                })
            } else {
                let text = res.text().await.unwrap_or_default();
                let json = serde_json::from_str(&text).ok();
                Some(UpstreamAnswer {
                    status,
                    content_type,
                    json,
                    retry_after,
                    text,
                })
            }
        }
        Err(_) => None,
    };
    if live {
        // **THE LIVE SSE PATH — the last decision to be wired, and the transform does the work.** The upstream
        // body is a `ByteStream`; `StreamTransform` is the `pull` loop's body as a state machine; `unfold`
        // drives it; `Response::from_stream` sends it. Nothing here decides anything the corpus does not
        // already pin — which is what the previous rounds were for.
        return stream_response(upstream_stream, &call);
    }
    match v1_finish(&call, answer.as_ref()) {
        ArmOutcome::Response(built) => to_response(built),
        // Unreachable: `live` was asked with the same function that answers `Stream` here.
        ArmOutcome::Stream => to_response(crate::responses::json_error(
            502,
            "the upstream body was consumed before it could be streamed",
            "api_error",
        )),
    }
}

/// Build the SSE response from a live upstream stream.
///
/// `upstream_stream` is `Response::stream()`'s `ByteStream` — the bytes as they arrive. Each chunk goes
/// through `StreamTransform::on_chunk`, and the FIRST error ends the transform with `read_failed = true`,
/// which is the branch whose sentence is "upstream stream died mid-response".
fn stream_response(
    upstream_stream: Option<worker::ByteStream>,
    call: &crate::request_shape::ArmCall,
) -> Result<Response> {
    let Some(body) = upstream_stream else {
        return to_response(crate::responses::json_error(
            502,
            "upstream returned no body",
            "api_error",
        ));
    };
    let transform = crate::stream::StreamTransform::new(&call.kind, &call.upstream_model);
    let failed = false;
    let stream = futures_util::stream::unfold(
        (body, transform, failed),
        move |(mut body, mut transform, mut failed)| async move {
            use futures_util::StreamExt;
            match body.next().await {
                Some(Ok(chunk)) => {
                    let out = transform.on_chunk(&String::from_utf8_lossy(&chunk));
                    Some((Ok::<Vec<u8>, worker::Error>(out), (body, transform, failed)))
                }
                Some(Err(_)) => {
                    failed = true;
                    let out = transform.on_end(true);
                    // The error branch is the LAST item: the state machine is done.
                    Some((Ok::<Vec<u8>, worker::Error>(out), (body, transform, failed)))
                }
                None => {
                    if transform.ended() {
                        return None;
                    }
                    let out = transform.on_end(failed);
                    Some((Ok(out), (body, transform, true)))
                }
            }
        },
    );
    let mut response = Response::from_stream(stream)?;
    // `sseResponse`'s own headers, which the string form already carries.
    for (k, v) in crate::responses::sse_response("").headers {
        response.headers_mut().set(&k, &v)?;
    }
    Ok(response)
}
