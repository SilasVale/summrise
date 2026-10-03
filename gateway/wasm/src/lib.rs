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
async fn og_degraded(env: &Env) -> bool {
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
    let namespace = env.durable_object("BREAKER")?;
    let stub = namespace.id_from_name("og")?.get_stub()?;

    let headers = Headers::new();
    if let Ok(auth) = env.var("DO_AUTH") {
        headers.set("x-do-auth", auth.to_string().as_str())?;
    }
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let request = Request::new_with_init("https://breaker/check", &init)?;

    let mut response = stub.fetch_with_request(request).await?;
    Ok(response.text().await? == "1")
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
fn not_found() -> Result<Response> {
    json(
        r#"{"type":"error","error":{"type":"not_found_error","message":"Not Found"}}"#.to_string(),
        404,
    )
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    // `path === "/api/health"` — an EXACT match, which is what `index.ts` compares. A prefix match
    // here would answer for paths the deployed front door does not route to this handler.
    if req.method() == Method::Get && url.path() == "/api/health" {
        let health = build_health(og_degraded(&env).await);
        return json(serde_json::to_string(&health)?, 200);
    }
    not_found()
}

pub mod body_scan;
pub mod byok;
pub mod rate_limit;
pub mod redact;
pub mod request_shape;
pub mod responses;
pub mod stream;
pub mod translate;
