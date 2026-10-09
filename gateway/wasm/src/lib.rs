//! The Rust half of the Summrise Gate worker — workers-rs 0.8.7.
//!
//! ONE ROUTE, AND IT IS THE FIRST ONE: `GET /api/health`.
//!
//! WHY THIS ROUTE FIRST (measured, and the plan's own criterion). P3's order is per WORKER, and
//! P3.1 says `gateway` is the only worker cheap enough to move (0.6–2.7 % cold requests, +0.4–1.5 %
//! of its own CPU; `index` +43–68 %, `proxies` +58–1122 %). Inside the worker the unit is a ROUTE,
//! and the brief for this step is the smallest PURE-LOGIC one — a route that only reads and reshapes
//! JSON. `/api/health` is that route:
//!
//!   * its input is a TABLE (`HEALTH_CHANNELS`, 20 cards) plus a 4-id priority list and ONE boolean;
//!   * its output is a JSON document with a derived `recommended`;
//!   * it is public, unauthenticated, and reads no KV, no R2 and no upstream;
//!   * and because the boolean is its only input, THE OUTPUT SPACE IS TWO DOCUMENTS — so byte
//!     equality can be shown over the WHOLE space rather than sampled (`verify.mjs`).
//!
//! THE ONE NON-PURE INPUT, STATED RATHER THAN HIDDEN: the `og` cards report the og circuit breaker,
//! which lives in the `BreakerDO` of the deployed `vale-gate` script. This worker reads the SAME
//! object through the same binding name, exactly as `reliability.ts`'s `isChannelDegraded` does —
//! `idFromName("og")` then `get()`, with the `x-do-auth` header when `DO_AUTH` is set. It is NOT
//! dropped: a health card that answers `ok` without asking is the failure this repository has a
//! standing rule against (AGENTS.md, "A status says what was CHECKED"). The binding is declared in
//! `wrangler.jsonc` with `script_name`, which is what makes it the SAME Durable Object.
//!
//! WHAT IS DELIBERATELY NOT HERE: `withCors`'s per-request `Access-Control-Allow-Origin` stamp. That
//! is dispatch-layer work in `index.ts` (it is applied to every route's response, not to this one),
//! and moving it means moving the front door's CORS decision — a second route, with its own proof.
//! The STATIC `CORS_HEADERS` that `jsonOk` itself sets ARE here, because those are the route's own
//! bytes.

use std::sync::Mutex;

use serde::Serialize;
use worker::*;

/// The health cards, MIRRORING `gateway/src/channels.ts`'s `HEALTH_CHANNELS` verbatim — order
/// included, because the order is what the response bytes are. The duplicate ids are deliberate
/// there and here: `build_health` asks the og circuit once per og card, and `recommended` takes the
/// FIRST card of a priority id.
const HEALTH_CHANNELS: [(&str, &str); 20] = [
    ("qw", "qw/qwen3.8-max-preview"),
    ("qw", "qw/qwen3.8-flash"),
    ("qw", "qw/deepseek-v4.1-flash"),
    ("og", "og/deepseek-v4.1-flash"),
    ("og", "og/gpt-5.6-luna"),
    ("og", "og/mimo-v2.5"),
    ("og", "og/mimo-v2.6-flash"),
    ("og", "og/ox-alpha-free"),
    ("og", "og/muse-spark-1.3-contributor"),
    ("or", "or/openai/gpt-5.6-luna:floor[1m]"),
    ("or", "or/z-ai/glm-5.2:free"),
    ("or", "or/nvidia/nemotron-3-ultra-550b-a55b:free"),
    ("or", "or/stealth/ox-alpha"),
    ("nv", "nv/nvidia/nemotron-3-ultra-550b-a55b"),
    ("gmi", "gmi/MiniMaxAI/MiniMax-M3"),
    ("gmi", "gmi/MiniMaxAI/MiniMax-M2.7"),
    ("cm", "cm/deepseek/deepseek-v4.1-flash"),
    ("cm", "cm/meituan/LongCat-2.0:free"),
    ("cm", "cm/poolside/laguna-s-2.1-free"),
    ("r4", "r4/deepseek-v4.1-flash"),
];

/// The default channel leads (`HEALTH_PRIORITY`), so the "recommended" badge points where `auto`
/// resolves.
const HEALTH_PRIORITY: [&str; 4] = ["cm", "qw", "og", "or"];

/// The static headers `jsonOk` puts on every JSON response (`http.ts`'s `CORS_HEADERS`). The
/// per-request ACAO is NOT here — see the module header.
const CORS_HEADERS: [(&str, &str); 2] = [
    (
        "Access-Control-Allow-Methods",
        "GET,POST,OPTIONS,DELETE,PUT",
    ),
    ("Access-Control-Allow-Headers", "*"),
];

/// One health card. FIELD ORDER IS THE WIRE FORMAT: `id`, `ok`, `model`, then `reason` only when
/// there is one — the same spread the TypeScript writes (`...(reason ? { reason } : {})`), and the
/// same reason this is a struct rather than a `json!` literal.
#[derive(Serialize)]
struct Card {
    id: &'static str,
    ok: bool,
    model: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Serialize)]
struct Recommended {
    channel: &'static str,
    model: &'static str,
}

#[derive(Serialize)]
struct Health {
    channels: Vec<Card>,
    recommended: Option<Recommended>,
}

/// THE ROUTE'S WHOLE LOGIC: one boolean in, one document out.
///
/// `degraded` is the og circuit — the only thing about this route that is not a constant. Every
/// other card is `ok` by construction, which is what `tooling.ts` says out loud: "a card costs
/// NOTHING upstream: buildHealth reads the channel's circuit breaker, it does not call zen".
fn build_health(degraded: bool) -> Health {
    let channels: Vec<Card> = HEALTH_CHANNELS
        .iter()
        .map(|(id, model)| {
            let (ok, reason) = if *id == "og" && degraded {
                (false, Some("circuit open"))
            } else {
                (true, None)
            };
            Card {
                id,
                ok,
                model,
                reason,
            }
        })
        .collect();

    // The TypeScript maps each priority id to the FIRST card carrying it and then takes the first
    // of those that is ok — the same two steps, in the same order.
    let recommended = HEALTH_PRIORITY.iter().find_map(|wanted| {
        channels
            .iter()
            .find(|c| c.id == *wanted)
            .filter(|c| c.ok)
            .map(|c| Recommended {
                channel: c.id,
                model: c.model,
            })
    });

    Health {
        channels,
        recommended,
    }
}

/// In-isolate cache for the breaker check, mirroring `reliability.ts`'s `degradedCache` (5 s): the
/// DO's `/check` is a storage read on EVERY `/api/health`, while `degradedUntil` only changes every
/// 60 s.
static DEGRADED_CACHE: Mutex<Option<(u64, bool)>> = Mutex::new(None);
const DEGRADED_CACHE_TTL_MS: u64 = 5000;

/// The og circuit, exactly as `isChannelDegraded` reads it: the cached value if it is younger than
/// the TTL, otherwise the DO — and `false` when the read FAILS, which is the TypeScript's `catch`
/// (`console.error("[breaker] check failed: …")` then `return false`). A failed read must not be
/// reported as a healthy channel NOR as a tripped one.
pub(crate) async fn og_degraded(env: &Env) -> bool {
    let now = Date::now().as_millis();
    if let Ok(cache) = DEGRADED_CACHE.lock() {
        if let Some((at, value)) = *cache {
            if now - at < DEGRADED_CACHE_TTL_MS {
                return value;
            }
        }
    }
    let value = read_breaker(env).await.unwrap_or(false);
    if let Ok(mut cache) = DEGRADED_CACHE.lock() {
        *cache = Some((now, value));
    }
    value
}

/// `breakerStub(env).fetch("https://breaker/check", { headers: breakerHeaders(env) })` → `"1"`.
async fn read_breaker(env: &Env) -> Result<bool> {
    Ok(breaker_call(env, "/check").await?.unwrap_or_default() == "1")
}

/// **THE WRITE SIDE OF THE BREAKER — AND IT WAS MISSING ENTIRELY.** `recordChannelFailure` /
/// `recordChannelSuccess` are calls to the DO (`/trip`, `/reset`), and this worker made neither: measured
/// 2026-10-06 with a DO stub that records every path it is asked for, a SUCCESSFUL og request made the
/// shipping route call `["/check", "/reset"]` while this worker called `[]`, and a hard network error made it
/// call `["/check", "/trip"]` against `[]`. The consequence at cutover is not cosmetic: the circuit would
/// never open on new failures nor close on new successes, so the health card and the fail-fast guard would
/// both be reading state that only the old worker still maintained.
pub(crate) async fn record_channel_failure(env: &Env) -> bool {
    // `recordChannelFailure` invalidates the 5 s cache immediately — "otherwise this isolate's health checks
    // keep reporting ok for up to 5s after a trip".
    if let Ok(mut cache) = DEGRADED_CACHE.lock() {
        *cache = Some((0, false));
    }
    breaker_call(env, "/trip").await.is_ok()
}

/// `recordChannelSuccess` → `/reset`. It does NOT touch the cache in the source (only the failure path does).
pub(crate) async fn record_channel_success(env: &Env) -> bool {
    breaker_call(env, "/reset").await.is_ok()
}

/// `breakerStub(env).fetch("https://breaker<path>", { headers: breakerHeaders(env) })` — the check, the trip
/// and the reset differ only in the path. `Ok(None)` is the TypeScript's `catch`: a DO that cannot be reached
/// is logged there and swallowed, never surfaced as a request failure.
async fn breaker_call(env: &Env, path: &str) -> Result<Option<String>> {
    let namespace = env.durable_object("BREAKER")?;
    let stub = namespace.id_from_name("og")?.get_stub()?;

    let headers = Headers::new();
    if let Ok(auth) = env.var("DO_AUTH") {
        headers.set("x-do-auth", auth.to_string().as_str())?;
    }
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let request = Request::new_with_init(&format!("https://breaker{path}"), &init)?;

    let mut response = stub.fetch_with_request(request).await?;
    Ok(Some(response.text().await?))
}

/// A JSON response with the headers `jsonOk` sets (`http.ts`).
fn json(body: String, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "application/json")?;
    for (name, value) in CORS_HEADERS {
        response.headers_mut().set(name, value)?;
    }
    if status != 200 {
        // `Response::from_body` is 200; the error arms below are the only other status this worker
        // produces, and `with_status` is how workers-rs sets it.
        response = response.with_status(status);
    }
    Ok(response)
}

/// The front door's own answer for a path this worker does not serve — the SAME envelope
/// `jsonError(404, "Not Found", "not_found_error")` produces. It is here so the worker's behaviour
/// is defined for every request rather than only for the route it moved; it is NOT part of the
/// route, and the byte comparison in `verify.mjs` does not claim it.
/// `new Response(null, { headers })` — an EMPTY 200 carrying only headers.
fn empty_with_headers(headers: Vec<(String, String)>) -> Result<Response> {
    let h = Headers::new();
    for (k, v) in &headers {
        h.set(k, v)?;
    }
    Ok(Response::empty()?.with_status(200).with_headers(h))
}

/// The `jsonError(status, message, type)` envelope from `http.ts` — the ONE shape every refusal in this worker
/// answers with (the device family's 401/403/404/409 arms and the front door's own 404).
pub(crate) fn json_error(status: u16, message: &str, kind: &str) -> Result<Response> {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": message}});
    json(serde_json::to_string(&body)?, status)
}

fn not_found() -> Result<Response> {
    json_error(404, "Not Found", "not_found_error")
}

/// **`withCors` — THE PER-REQUEST STAMP, APPLIED TO EVERY RESPONSE THE DOOR SENDS.**
///
/// The shipping front door wraps every one of its returns in it (`index.ts`: `withCors(request, …)` on the
/// health route, the tooling routes, the dispatch and both 404s), and its own comment says why the stamp is
/// per-request rather than a constant: *"Deliberately NO Access-Control-Allow-Origin here — the origin is
/// reflected per request, otherwise a static `*` would ride along on every merge of this constant."*
///
/// **MEASURED 2026-10-06, AND THIS DOOR DID NOT DO IT AT ALL**: driving both doors with the same env and an
/// `Origin` of one of the deployment's own console origins gave the shipping door
/// `access-control-allow-origin: <that origin>` plus `vary: Origin` on all nine cases, and the wasm worker only
/// the static `access-control-allow-headers/methods` pair — so **a browser client (the console's own UI among
/// them) would have had every cross-origin response refused at the cutover**, on a route whose body and status
/// matched exactly.
///
/// **AND IT READS `CONSOLE_ORIGINS`, NOT `CONSOLE_HOST`.** The preflight path passed `env.var("CONSOLE_HOST")`
/// as the allowlist — the deployed value of that var is a list of HOSTNAMES, where the rule wants ORIGINS
/// (`https://…`), so nothing ever matched and even the preflight reflected nothing. `CONSOLE_HOST` keeps its own
/// job: it is the console-host isolation in `index.ts`, not the allowlist.
///
/// **AND THESE COMMENTS DO NOT SPELL THE HOST, WHICH IS A GATE'S RULE RATHER THAN STYLE**: the first version of
/// this paragraph wrote the deployment's origins out, and `agent/tests/production_host.rs` failed the tree by
/// naming this file — five occurrences, none of them code.
fn with_cors(
    mut res: Response,
    origin: &str,
    host: Option<&str>,
    configured: Option<&str>,
) -> Result<Response> {
    if crate::cors::is_allowed_origin(origin, host, configured) {
        let headers = res.headers_mut();
        headers.set("Access-Control-Allow-Origin", origin)?;
        headers.set("Vary", "Origin")?;
    }
    Ok(res)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Read off the request BEFORE it is moved into the routing below: the stamp needs the Origin and the host,
    // and the allowlist needs the var — the same three inputs `withCors` takes in the source.
    let origin = req.headers().get("origin").ok().flatten().unwrap_or_default();
    let host = req
        .url()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()));
    let configured = env.var("CONSOLE_ORIGINS").ok().map(|v| v.to_string());
    let res = route(req, env).await?;
    with_cors(res, &origin, host.as_deref(), configured.as_deref())
}

async fn route(req: Request, env: Env) -> Result<Response> {
    let url = req.url()?;

    // **THE GLOBAL PREFLIGHT, AND IT IS FIRST FOR TWO REASONS.** The shipping front door does the same in
    // the same place — `if (request.method === "OPTIONS") return new Response(null, { headers:
    // corsHeadersFor(request, env) })` — and a live comparison is what showed it was missing here:
    //
    //     OPTIONS /v1/messages   wasm 404 (and 30 SECONDS with a body)   ts 200 in 0.86s
    //
    // **THE HANG IS THE REASON IT IS NOT MERELY A MISSING HEADER.** An `OPTIONS` that fell through to
    // `not_found()` never read the request body, and the runtime waited on it — so a browser's preflight,
    // which is the one request every cross-origin call makes first, was the one request this worker could
    // not answer. A 404 that hangs is worse than a 404.
    //
    // **THE BODY IS NOT READ HERE, WHICH IS THE POINT**: the answer is empty and the headers are the whole
    // of it, exactly as the source's `new Response(null, …)`.
    if req.method() == Method::Options {
        let origin = req.headers().get("origin").ok().flatten().unwrap_or_default();
        let host = url.host_str().unwrap_or_default().to_string();
        // **`CONSOLE_ORIGINS`, WHICH IS THE NAME THE SOURCE READS** (`allowedOrigins(env)` is
        // `env.CONSOLE_ORIGINS`). This passed `CONSOLE_HOST` until 2026-10-06 — a list of hostnames where the
        // rule wants origins — so `is_allowed_origin` never matched and even the preflight reflected nothing.
        let configured = env.var("CONSOLE_ORIGINS").ok().map(|v| v.to_string());
        // **NO `retain` HERE, AND THE SOURCE SAYS WHY**: `corsHeadersFor` "starts from a fresh set that
        // never had an ACAO, so 'not allowed' means 'do not add one'" — the delete-the-origin branch
        // belongs to `stampCors`, which is handed the UPSTREAM's headers. A first version of this copied
        // that branch here and clippy was right to refuse it: it was a case-insensitive comparison the
        // helper had already made unnecessary.
        let headers = crate::cors::cors_headers_for(&origin, Some(&host), configured.as_deref());
        return empty_with_headers(headers);
    }

    // `path === "/api/health"` — an EXACT match, which is what `index.ts` compares. A prefix match
    // here would answer for paths the deployed front door does not route to this handler.
    if req.method() == Method::Get && url.path() == "/api/health" {
        let health = build_health(og_degraded(&env).await);
        return json(serde_json::to_string(&health)?, 200);
    }
    // **THE `/v1` FRONT DOOR — the second route, and the one the migration was for.** Its decisions are all
    // ported and proved; `v1.rs` is the I/O around them, and its header names what is deliberately not wired
    // yet (the live SSE path, and `/v1/models`).
    if v1::is_v1_route(&req.method(), url.path()) {
        return v1::handle(req, env).await;
    }
    // **THE DEVICE FAMILY — the console's registry, the third route group.** `devices::in_family` is the SAME
    // predicate the TypeScript front door uses to decide what to hand over, so a path this worker is given is a
    // path it answers: the routes it serves, and the front door's own 404 for the shapes it does not.
    if devices::in_family(&req.method(), url.path()) {
        return match devices::handle(req, &env).await? {
            Some(response) => Ok(response),
            None => not_found(),
        };
    }
    not_found()
}

pub mod auth;
pub mod body_scan;
pub mod byok;
pub mod cors;
pub mod device;
pub mod device_registry;
pub mod device_store;
pub mod devices;
pub mod ip_rate_limit;
pub mod key_lock;
pub mod rate_limit;
pub mod redact;
pub mod registry;
pub mod reliability;
pub mod request_shape;
pub mod responses;
pub mod routing;
pub mod session;
pub mod session_gate;
pub mod store;
pub mod stream;
pub mod tokens;
pub mod tooling;
pub mod translate;
pub mod v1;
pub mod vision;
pub mod webcrypto;
