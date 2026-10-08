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
//!
//! **THAT PARAGRAPH WAS TRUE WHEN IT WAS WRITTEN AND IS NOT TRUE NOW (corrected 2026-10-06).** The live path
//! is `stream_response` below — `Response::stream()`, `StreamTransform::on_chunk`, `unfold`,
//! `Response::from_stream`, and `sseResponse`'s headers — and the `ArmOutcome::Stream` arm above it is the
//! UNREACHABLE one ("the upstream body was consumed before it could be streamed"), not a 501 this file
//! answers. A comment that names a gap which has since closed is the same defect as one that claims coverage
//! that is not there, so it is corrected here rather than left to mislead the next reader — **and the
//! measurement that shows which half is really left is the one this round added** (`verify.mjs`'s
//! "the deployment's own key reaches the upstream" section, plus the streaming cases beside it).

use std::sync::Mutex;

use worker::*;

use crate::request_shape::{
    v1_finish, v1_plan, ArmOutcome, UpstreamAnswer, V1Plan, V1PlanInputs, V1Request,
};
use crate::responses::Built;

/// **THE PER-TOKEN RATE LIMIT'S COUNTERS — THE SOURCE'S MODULE-LEVEL `Map`s, AS STATE.** Per isolate, exactly
/// like the JavaScript's: `checkRateLimit` keeps `__rlMin`/`__rlDay` in module scope and never touches KV, so a
/// second isolate starts its own count. `rate_limit.rs` holds the decision (already corpus-tested); this is the
/// instance the request path uses.
static RATE_LIMIT: Mutex<Option<crate::rate_limit::RateLimitState>> = Mutex::new(None);

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

/// **THE PROVIDER LIST, THROUGH THE SAME CACHE** — the source's `readList` is cached, and `providers:` is on the
/// short-TTL list for a reason its own comment gives: "a deleted or re-pointed provider must stop being dialled
/// within a minute on every isolate, not within a day."
async fn providers_cached(env: &Env) -> Vec<serde_json::Value> {
    let now = Date::now().as_millis() as i64;
    if let Some(hit) = crate::store::cached_get("providers:custom", now) {
        return serde_json::from_value(hit).unwrap_or_default();
    }
    let value: Vec<serde_json::Value> = kv_json(env, "providers:custom").await;
    crate::store::cache_put(
        "providers:custom",
        serde_json::to_value(&value).unwrap_or(serde_json::Value::Null),
        now,
    );
    value
}

/// **THE REQUEST PATH'S KV READ — THROUGH THE PER-ISOLATE CACHE, WHICH IS WHAT DECOUPLES KV VOLUME FROM REQUEST
/// VOLUME.** The source reads users, keys, settings and the provider list through `store/cache.ts`; this worker
/// read KV directly and paid for it. Measured 2026-10-06 with a counting KV stub, ten requests on one isolate:
/// **shipping 8 reads (0.8/request), wasm 60 (6.0/request)** — the same responses, seven and a half times the KV
/// operations, which is latency and cost rather than bytes, and therefore invisible to every case in the
/// divergence sweep.
///
/// A MISS IS CACHED TOO (`cset` stores `null`), so a token that does not exist is not re-read on every request
/// that presents it — the source's "no zombie lookups".
async fn kv_text_cached(env: &Env, key: &str) -> Option<String> {
    let now = Date::now().as_millis() as i64;
    if let Some(hit) = crate::store::cached_get(key, now) {
        return hit.as_str().map(|s| s.to_string());
    }
    let value = kv_text(env, key).await;
    crate::store::cache_put(
        key,
        match &value {
            Some(text) => serde_json::Value::String(text.clone()),
            None => serde_json::Value::Null,
        },
        now,
    );
    value
}

/// **EVERY ENV INPUT THE PORTED DECISIONS READ, READ OFF THE WORKER.**
///
/// The pure layer reads these out of the object `handle` hands it: `byok.rs`'s `BYOK_CHANNELS` env column
/// (seven names — `nvidia` and `gmi` deliberately have no deployment fallback), `routing.rs`'s
/// `US_PROXY_BASE` and `MUSE_RESPONSES_EXIT`, and `reliability.rs`'s two timeout budgets.
///
/// **UNTIL 2026-10-06 THAT OBJECT WAS `{"US_PROXY": …}` ALONE, AND THE MEASUREMENT IS WHAT SAYS SO.** With a
/// valid token, no user key in KV, and the deployment's own `DEEPSEEK_API_KEY` in the worker's env, the BUILT
/// worker answered `502 config_error — "DEEPSEEK_API_KEY not configured — add your own key in the console"`
/// and made **no upstream call at all**, while the shipping `handleGateway` on the same three inputs called
/// `api.deepseek.com`, `token-plan.ap-southeast-1.maas.aliyuncs.com` and `opencode.ai` with
/// `Bearer sk-env-…`. Every user without a key of their own — which is what the deployment's keys exist for —
/// would have met that at the cutover, on every channel at once.
///
/// **`var` READS SECRETS TOO**, which is why one call covers both: workers-rs's own doc for it is "Get an
/// environment variable defined in the [vars] section of your wrangler.toml **or a secret defined using
/// `wrangler secret` as a plaintext value**". Absent names are omitted rather than inserted as `null`, which
/// is what the source's `env.NAME === undefined` means to `isKeyMissing` and to `bearerKeyFor`.
///
/// `US_PROXY` IS NOT IN THIS LIST because it is not read straight off the worker: the KV setting
/// `settings:US_PROXY` takes precedence over the var, and that precedence is its own proved decision.
const ENV_KEYS: [&str; 13] = [
    // the channel keys, in `BYOK_CHANNELS`' own order
    "OPENCODE_GO_API_KEY",
    "DEEPSEEK_API_KEY",
    "QWEN_API_KEY",
    "OPENROUTER_API_KEY",
    "CMD_API_KEY",
    "AMD_API_KEY",
    "R4_API_KEY",
    // the routing layer's two, and the two timeout budgets
    "US_PROXY_BASE",
    "MUSE_RESPONSES_EXIT",
    "OG_TIMEOUT_MS",
    "UPSTREAM_TIMEOUT_MS",
    // **AND THE VISION SUBSYSTEM'S TWO, WHICH WERE MISSING UNTIL THE SWEEP FOUND THEM.** They are the
    // operator's own names (`VISION_MODEL`, `VISION_CAPABLE_MODELS`), and a fixed list has to be told about
    // them: measured 2026-10-06, with `VISION_CAPABLE_MODELS=og/mimo-v2.5` the shipping route made ONE upstream
    // call (the model sees images itself, so the pass short-circuits) and this worker made TWO — it described
    // the picture because the allowlist it read was EMPTY. Same failure shape as the provider `apiKeyEnv` a
    // round earlier: a name the list cannot know.
    "VISION_MODEL",
    "VISION_CAPABLE_MODELS",
];

/// The env object `handle` hands the ported decisions: every name above that the worker actually carries.
fn env_json(env: &Env, us_proxy_setting: bool) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for name in ENV_KEYS {
        if let Ok(value) = env.var(name) {
            let text = value.to_string();
            if !text.is_empty() {
                map.insert(name.to_string(), serde_json::json!(text));
            }
        }
    }
    map.insert(
        "US_PROXY".to_string(),
        serde_json::json!(if us_proxy_setting { "1" } else { "0" }),
    );
    serde_json::Value::Object(map)
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
        let providers: Vec<serde_json::Value> = providers_cached(&env).await;
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

    // `mut`: the search swap REBUILDS it (`rawText = JSON.stringify(body)`), because the passthrough arm
    // forwards this text with the model swapped in place.
    let mut raw = req.text().await?;
    // `mut` because the VISION PASS rewrites `messages` in place before the plan sees it.
    let mut parsed: Option<serde_json::Value> = serde_json::from_str(&raw).ok();
    let model = parsed
        .as_ref()
        .and_then(|b| b.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();

    // **A BODY THAT IS NOT JSON IS A 500 WHERE THE SOURCE PARSES IT — MEASURED, NOT REASONED.**
    //
    // `translate.ts` parses `rawText` in the POST arms and lets a `JSON.parse` throw reach the front door's
    // catch (`jsonError(500, "Internal error", "api_error")`). `serde_json::from_str(...).ok()` SWALLOWED that:
    // the model resolved to `""`, the prefix to `""`, and the request fell through to the DEFAULT channel.
    // Measured 2026-10-06 on the built worker — a body of `{not json`, and an EMPTY body, both answered
    // **`502 config_error — "CMD_API_KEY not configured — add your Command Code key in the console"`**, a
    // diagnosis pointing an operator at a channel they never asked for, where the shipping route answers
    // `500 {"type":"error","error":{"type":"api_error","message":"Internal error"}}`.
    //
    // **THE TWO EXEMPTIONS ARE MEASURED TOO**: `count_tokens` answers identically on both sides with a
    // malformed body (it never parses), and a GET carries no body to parse — so this rule is scoped to the
    // three POST arms and leaves both alone. `[]` also parses, and stays on its normal path.
    let shape_for_body = crate::request_shape::detect_route(method.as_ref(), &path);
    if parsed.is_none()
        && (shape_for_body.is_messages
            || shape_for_body.is_chat_completions
            || shape_for_body.is_responses)
    {
        console_error!("[gateway] unhandled: the request body is not JSON");
        return to_response(crate::responses::json_error(
            500,
            "Internal error",
            "api_error",
        ));
    }

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
    let user = match kv_text_cached(&env, &format!("token:{token}")).await {
        Some(uid) => kv_text_cached(&env, &format!("user:{uid}"))
            .await
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()),
        None => None,
    };
    // **THE RATE LIMIT — THE SOURCE'S POSITION (after auth, before the route match) AND ITS OWN STATE.**
    // Measured 2026-10-06, both sides driven with the same KV: the shipping route answers `429` on the 49th
    // POST of a minute for one token, and this worker answered 200 fifty times out of fifty, because nothing
    // called the ported guard. It is the only thing between a token and an unbounded bill.
    let now_ms = Date::now().as_millis() as i64;
    let rate_refusal = {
        // A poisoned lock must not take the worker down: the counters are a budget, not a ledger.
        let mut guard = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
        guard.get_or_insert_with(Default::default).check(
            env.kv("KEYS").is_ok(),
            method.as_ref(),
            &path,
            &token,
            now_ms,
        )
    };
    if let Some(built) = rate_refusal {
        return to_response(built);
    }

    // **THE USER'S OWN KEYS, IN THE SHAPE THE STORE WRITES — WHICH IS NOT THE SHAPE THE GATES READ.**
    // `store.ts` documents the record: "`ukeys:<id>` → that user's own backend keys
    // `{DEEPSEEK_API_KEY, OPENCODE_GO_API_KEY, OPENROUTER_API_KEY}`", and `translate.ts`'s `extractByokKeys`
    // turns those ENV-STYLE names into the short fields the gates look up (`deepseek`, `opencodeGo`, …).
    // **THIS FILE HANDED THE RAW RECORD STRAIGHT TO THE GATE UNTIL 2026-10-06**, so a user's own key was
    // invisible: measured on the built worker, `{"DEEPSEEK_API_KEY": "sk-user-ds"}` answered
    // `502 config_error — not configured — add your own key in the console` with NO upstream call, while the
    // shape only the TESTS build (`{"deepseek": "sk-user-ds"}`) reached the upstream with `Bearer sk-user-ds`.
    // The replay in `route-corpus.json`'s test always applied this step; production did not.
    // The user id, kept because the vision pass needs it TWICE: the cache key's user scope and the zen session
    // header (`opencodeSessionHeader(undefined, uid)`).
    let uid = user
        .as_ref()
        .and_then(|u| u.get("id"))
        .and_then(|i| i.as_str())
        .unwrap_or("")
        .to_string();
    let byok = match user
        .as_ref()
        .and_then(|u| u.get("id"))
        .and_then(|i| i.as_str())
    {
        Some(uid) => {
            let raw = kv_text_cached(&env, &format!("ukeys:{uid}"))
                .await
                .and_then(|t| {
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok()
                })
                .unwrap_or_default();
            crate::byok::extract_byok_keys(&serde_json::Value::Object(raw))
        }
        None => serde_json::Map::new(),
    };

    // THE SETTINGS the chain reads: `settings:US_PROXY` with the Worker var as its fallback.
    let us_proxy_setting = crate::store::global_setting_enabled(
        crate::store::get_global_setting(
            kv_text_cached(&env, "settings:US_PROXY").await.as_deref(),
            env.var("US_PROXY").ok().map(|v| v.to_string()).as_deref(),
        )
        .as_deref(),
    );
    // **THE REST OF THE ENV, WHICH USED TO BE MISSING ENTIRELY** — see `ENV_KEYS`. The channel keys, the two
    // routing names and the two timeout budgets are read off the worker here; without them `key_gate` refuses
    // a request the deployment's own key would have served, and `bearer_key_for` sends an empty bearer.
    let env_json = env_json(&env, us_proxy_setting);

    // **THE CUSTOM PROVIDERS, WHICH ARE DATA AND THEREFORE THE CALLER'S READ** — `resolve_model` is pure, so the
    // records it consults come from here. An absent or malformed key is an empty list, which is what an
    // untouched deployment has (and what `readList` answers in the source).
    let providers: Vec<serde_json::Value> = providers_cached(&env).await;
    // **AND THEIR `apiKeyEnv` BINDINGS RIDE THE SAME OBJECT**, because that name is the provider's own choice
    // rather than one of `ENV_KEYS`: the source reads `env[p.apiKeyEnv]` per request, and a fixed list cannot
    // know it. Measured — this port's first version answered `502 — "acme/: no provider key"` for a record
    // whose named binding the worker carried, and `verify.mjs`'s named-binding case is what caught it.
    let mut env_json = env_json;
    if let Some(map) = env_json.as_object_mut() {
        for name in crate::store::provider_key_env_names(&providers) {
            if let Ok(value) = env.var(&name) {
                let text = value.to_string();
                if !text.is_empty() {
                    map.insert(name, serde_json::json!(text));
                }
            }
        }
    }

    // THE CHAIN: the model -> the route, then the two-phase dispatch.
    // `mut`: the og/ SEARCH SWAP below rewrites the route and the wire model, which is what the source does
    // to its own `route` object before the arm reads it.
    let mut resolved = crate::routing::resolve_model(
        &model,
        us_proxy_setting,
        &path,
        Some(&env_json),
        &providers,
    );
    // **`route.type === "error"` — ANSWERED, NEVER DIALED.** The source puts this check in the same place: after
    // auth, the route match and the retired check, and before every key decision, because an unroutable record
    // must not reach a channel's key gate and be reported as that channel's problem.
    if let Some(reason) = &resolved.unroutable {
        return to_response(crate::responses::json_error(502, reason, "config_error"));
    }
    // **THE og/ SEARCH PATH — THE SWAP TO ZEN'S NATIVE `/v1/messages`, AND IT WAS MISSING ENTIRELY.**
    //
    // Measured 2026-10-06 on the built worker: a request forcing `web_search` made the shipping route dial
    // `https://opencode.ai/zen/go/v1/**messages**` (a PASSTHROUGH — zen executes the server-side tool) and hand
    // the upstream's bytes back verbatim, while this worker dialled `/v1/**chat/completions**` (the translate
    // path) and answered a reshaped Anthropic message. The tool is NOT executed on the translation, so web
    // search was broken here in a way no status code shows.
    //
    // The source's three steps, in its own order: a SEARCH-ONLY body (exactly one tool, no `tool_choice`) gets
    // the force INJECTED first; a forced `tool_choice` then selects the search target; and the swap happens
    // unless the route is ALREADY that passthrough. `searchTargetFor` keeps a caller's search-capable model and
    // forces every other og/ model to the version-less `deepseek-flash` lane, because the translate-only models
    // fabricate a query and return no `web_search_tool_result` (verified 2026-08-13).
    //
    // The US exit is PRESERVED rather than bypassed (round-116): a swap that set the raw constant "silently
    // bypassed the US exit that pickRoute's via() had chosen — direct zen is exactly what US_PROXY exists to
    // avoid".
    let us_proxy_for_search =
        crate::routing::og_force_us_proxy(&model) || us_proxy_setting;
    let mut model = model;
    if resolved.route.kind == "opencode" {
        if let Some(body) = parsed.as_mut() {
            if crate::request_shape::is_search_only_request(body) {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert(
                        "tool_choice".to_string(),
                        serde_json::json!({ "type": "tool", "name": "web_search" }),
                    );
                }
            }
        }
    }
    let forced_search = parsed
        .as_ref()
        .map(|b| {
            crate::request_shape::is_forced_web_search(
                b.get("tool_choice").unwrap_or(&serde_json::Value::Null),
            )
        })
        .unwrap_or(false);
    if forced_search && resolved.route.kind == "opencode" {
        let target = crate::registry::search_target_for(&model, &resolved.upstream_model);
        let already_native = !resolved.route.is_translate
            && resolved.route.upstream == crate::routing::OG_ZEN_ANTHROPIC;
        if !already_native {
            model = target.model;
            resolved.upstream_model = target.wire_model.clone();
            if let Some(body) = parsed.as_mut() {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert(
                        "model".to_string(),
                        serde_json::json!(target.wire_model.clone()),
                    );
                }
                // The passthrough forwards the RAW text with the model swapped in place, so the rebuilt body is
                // what the arm reads — and both now name the same wire slug.
                raw = serde_json::to_string(body).unwrap_or_else(|_| raw.clone());
            }
            resolved.route.is_translate = false;
            resolved.route.upstream = if us_proxy_for_search {
                format!(
                    "{}/api/zen?target=og&path={}",
                    crate::routing::us_proxy_base(Some(&env_json)),
                    crate::routing::encode_uri_component("/v1/messages")
                )
            } else {
                crate::routing::OG_ZEN_ANTHROPIC.to_string()
            };
            resolved.route.kind = "opencode";
        }
    }

    // **THE VISION PASS — `preprocessImages`, IN THE SOURCE'S PLACE** (after the route is resolved, before the
    // arm's key gates) **AND WITH ITS OWN PROXY READ**: the describe's US exit comes from the KV setting ALONE
    // (`getGlobalSetting(env, "US_PROXY")`), never the request path's `US_PROXY` env fallback — with the proxy
    // on, a describe that went direct turned every image into a failure marker for US users (round-119).
    //
    // **A FAILED DESCRIBE IS A 500, NOT A FABRICATED DESCRIPTION**: the source throws, the front door's catch
    // answers `jsonError(500, "Internal error", "api_error")`, and that is what this returns.
    if let Some(body) = parsed.as_mut() {
        let shape = crate::request_shape::detect_route(method.as_ref(), &path);
        if shape.is_messages {
            let declared_vision = resolved.route.kind == "custom"
                && crate::store::provider_model_vision(
                    resolved.provider.as_ref(),
                    &resolved.upstream_model,
                );
            let vision_proxy = crate::store::global_setting_enabled(
                kv_text_cached(&env, "settings:US_PROXY").await.as_deref(),
            );
            let inputs = crate::vision::VisionInputs {
                model: &model,
                upstream_model: &resolved.upstream_model,
                uid: &uid,
                declared_vision,
                byok: &byok,
                us_proxy_setting: vision_proxy,
                providers: &providers,
                env: &env_json,
                timeout_ms: crate::reliability::upstream_timeout_ms(Some(&env_json)),
            };
            let messages = body
                .get("messages")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match crate::vision::preprocess_images(&env, &messages, &inputs).await {
                Ok((rewritten, true)) => {
                    if let Some(obj) = body.as_object_mut() {
                        obj.insert("messages".to_string(), rewritten);
                    }
                }
                Ok((_, false)) => {}
                Err(e) => {
                    console_error!("[gateway] unhandled: {e}");
                    return to_response(crate::responses::json_error(
                        500,
                        "Internal error",
                        "api_error",
                    ));
                }
            }
        }
    }

    // **THE INCOMING HEADERS, BECAUSE THE PLAN COMPOSES THE og SESSION HEADER FROM THEM.** `ogSession`
    // relays the client's own conversation id when it sent one (four spellings) and synthesizes a stable
    // per-user value otherwise, so the decision needs the raw list. This is the ONLY thing the front door
    // supplies for it — the composition itself is `session::og_session`, inside the plan, where it is
    // proved. The keys are folded to lower case for the same reason `Headers::get` folds them.
    let incoming: serde_json::Map<String, serde_json::Value> = headers
        .entries()
        .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
        .collect();
    let request = V1Request {
        method: method.to_string(),
        path: path.clone(),
        user,
        kind: resolved.route.kind.to_string(),
        byok,
        env: env_json,
        raw_text: serde_json::json!(raw),
        headers: incoming,
    };
    let plan = v1_plan(&V1PlanInputs {
        req: &request,
        route: &resolved.route,
        model: &model,
        prefix: &resolved.prefix,
        upstream_model: &resolved.upstream_model,
        parsed_body: parsed.as_ref(),
        scanned: None,
        provider: resolved.provider.as_ref(),
    });
    let V1Plan::Dial(call) = plan else {
        let V1Plan::Respond(built) = plan else {
            unreachable!()
        };
        return to_response(built);
    };

    // **THE og CIRCUIT, BEFORE THE DIAL** — `channelDegradedError`'s position: after the arm's key gate (which
    // is inside the plan) and before the upstream call, so a degraded channel fails fast with the sentence
    // clients already classify as transient instead of hanging on a channel that is not answering. Measured
    // 2026-10-06: with the BreakerDO reporting degraded, the shipping route answered this 502 and this worker
    // dialled anyway (200 from the stub upstream).
    if call.kind == "opencode" && crate::og_degraded(&env).await {
        return to_response(crate::responses::json_error(
            502,
            "og: circuit open (recent upstream failures, try again in ~1 min)",
            "api_error",
        ));
    }

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
    // **THE RETRY LOOP — `fetchWithRetry`, WHICH THIS WORKER NEVER RAN, AND THE POLICY WAS ALREADY HERE.**
    // `v1_plan` computes the arm's `RetryPolicy` and puts it on the request; nothing read it, so a 429 was
    // answered on the first try where the shipping route retries it. Measured 2026-10-06 on a 429 stub:
    // **3 upstream calls against 1**. The loop below is the source's, including the two details it can carry
    // (they ride the error body when there is no response to normalize) and the "billing guard" that refuses
    // to re-send a POST which may already have been processed.
    let policy = call.request.policy.clone();
    let attempts = policy
        .attempts
        .unwrap_or(crate::reliability::DEFAULT_ATTEMPTS);
    let backoff_ms = policy
        .backoff_ms
        .map(|b| b as f64)
        .unwrap_or(crate::reliability::DEFAULT_BACKOFF_MS);
    let retry502 = policy.retry502.unwrap_or(false);
    let ignore_retry_after = policy.ignore_retry_after.unwrap_or(false);
    let mut attempt: i64 = 1;
    let mut detail = String::new();
    let mut upstream: Option<Response> = None;
    while attempt <= attempts {
        match fetch_with_timeout(&call.request.url, &init, policy.timeout_ms).await {
            Err(FetchFailure::Timeout) => {
                detail = format!("timeout after {}ms", policy.timeout_ms);
                break;
            }
            Err(FetchFailure::Network(e)) => {
                detail = format!("network error: {}", js_error_message(&e));
                break;
            }
            Ok(res) => {
                let status = res.status_code();
                match crate::reliability::retry_decision(status, attempt, attempts, false, retry502) {
                    crate::reliability::RetryDecision::Stop { detail: d } => {
                        detail = d;
                        upstream = Some(res);
                        break;
                    }
                    crate::reliability::RetryDecision::Retry { detail: d } => {
                        detail = d;
                        let retry_after = res.headers().get("retry-after")?;
                        let wait = crate::reliability::retry_wait_ms(
                            ignore_retry_after,
                            retry_after.as_deref(),
                            backoff_ms,
                            attempt,
                            js_sys::Math::random() * 200.0,
                            crate::reliability::DEFAULT_MAX_WAIT_MS,
                        );
                        Delay::from(std::time::Duration::from_millis(wait.max(0.0) as u64)).await;
                        attempt += 1;
                    }
                }
            }
        }
    }
    // **NO RESPONSE AFTER THE LOOP IS THE ARM'S FAILURE, NOT A `None` ANSWER** — and the trip condition is the
    // source's: only a hard network error or a timeout counts ("a fast 5xx/429 is the upstream being flaky, not
    // dead — it must NOT trip").
    let Some(mut res) = upstream else {
        if call.kind == "opencode"
            && (detail.starts_with("network error") || detail.starts_with("timeout"))
        {
            crate::record_channel_failure(&env).await;
        }
        let label = crate::responses::translate_label(&call.kind, None);
        return to_response(crate::request_shape::upstream_fetch_failure(
            &call.kind,
            call.is_translate,
            &label,
            &detail,
        ));
    };
    let mut upstream_stream = None;
    // Declared, not initialised: the failure path returns above, so the only path that reads it assigns it.
    let live;
    let answer = {
        {
            let status = res.status_code();
            // **A REAL 2xx RESETS THE CONSECUTIVE-FAILURE COUNT** — "otherwise yesterday's blips would combine
            // with today's to trip" — and the reset is a DO call this worker never made (measured: `[]` where
            // the shipping route called `["/check", "/reset"]`).
            if call.kind == "opencode" && (200..=299).contains(&status) {
                crate::record_channel_success(&env).await;
            }
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

/// Why a fetch produced no response — the two things `fetchWithTimeout` distinguishes.
enum FetchFailure {
    /// The source's `TimeoutError`: `fetchWithTimeout` aborts at `ms` and renames the `AbortError`.
    Timeout,
    /// Everything else: the runtime's own error, whose `e.message` is what the detail carries.
    Network(worker::Error),
}

/// **`fetchWithTimeout(url, init, ms)` — AND ITS ONE DIFFERENCE FROM THE SOURCE IS RECORDED, NOT HIDDEN.**
///
/// The source aborts the request through an `AbortController`'s signal; `workers-rs` 0.8.7's `RequestInit`
/// exposes no signal at all (`request_init.rs` has headers, method, redirect, cache, body and cf only), so this
/// RACES the fetch against a `Delay` instead. **The client sees the same thing at the same moment** — the
/// answer is `timeout after <ms>ms` and the circuit's trip condition matches — while the upstream request
/// itself is not cancelled; the platform's own limit ends it. Measured before this existed: with
/// `OG_TIMEOUT_MS=1500` and a stub upstream that never answers, the shipping route answered
/// `502 og: timeout after 1500ms` and this worker **never answered at all** (the harness's 8 s deadline).
async fn fetch_with_timeout(
    url: &str,
    init: &RequestInit,
    timeout_ms: f64,
) -> std::result::Result<Response, FetchFailure> {
    let request = Request::new_with_init(url, init).map_err(FetchFailure::Network)?;
    // A `let` BINDING, not a temporary: `pin_mut!` borrows it, and `Fetch::Request(request).send()` in the
    // `let fetch = …` position frees the `Fetch` at the end of that statement.
    let fetcher = Fetch::Request(request);
    let fetch = fetcher.send();
    let delay = Delay::from(std::time::Duration::from_millis(timeout_ms.max(0.0) as u64));
    futures_util::pin_mut!(fetch);
    futures_util::pin_mut!(delay);
    match futures_util::future::select(fetch, delay).await {
        futures_util::future::Either::Left((result, _)) => result.map_err(FetchFailure::Network),
        futures_util::future::Either::Right((_, _)) => Err(FetchFailure::Timeout),
    }
}

/// **`e.message`, NOT `String(e)` — AND THE DIFFERENCE IS IN A BODY CLIENTS PARSE.**
///
/// The source's detail is `` `network error: ${e.message}` `` — the message WITHOUT the class prefix — while
/// `worker::Error`'s `Display` prints `Error: <message>` for a JS-thrown error. Measured on the built worker
/// 2026-10-06 with the same thrown `new Error("network down")`: the shipping route answered
/// `og: network error: network down` (89 B) and this worker `og: network error: Error: network down` (96 B).
/// Stripping the prefix reproduces `e.message` — including for a message that itself starts with `Error: `,
/// where the runtime prints the prefix twice.
fn js_error_message(e: &worker::Error) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
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
