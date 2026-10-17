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
//! WHAT IS *NOT* HERE, AND WHAT CHANGED ABOUT IT: `withCors`'s per-request `Access-Control-Allow-Origin`
//! stamp USED to be dispatch-layer work left to `index.ts`. It is not any more — measured 2026-10-06, this
//! door stamped nothing, and a reader comparing the two implementations is the reason `fetch` below now
//! applies the same stamp to every response (`with_cors`, from `cors.rs`'s rule). **THE ONE FAMILY THAT IS
//! DELIBERATELY NOT STAMPED IS THE PUBLIC INSTALLERS** (`tooling::is_installer_path`), because their 200 arm
//! FORCES `Access-Control-Allow-Origin: *` and a stamp would delete it; that carve-out is written at the one
//! place it takes effect. The STATIC `CORS_HEADERS` that `jsonOk` itself sets are here either way, because
//! those are the route's own bytes.

use std::sync::Mutex;

use serde::Serialize;
use worker::*;

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
    let channels: Vec<Card> = crate::registry::HEALTH_CHANNELS
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
    let recommended = crate::registry::HEALTH_PRIORITY.iter().find_map(|wanted| {
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
pub(crate) async fn read_breaker(env: &Env) -> Result<bool> {
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
        headers.set(crate::route_do::DO_AUTH_HEADER, auth.to_string().as_str())?;
    }
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let request = Request::new_with_init(&format!("https://breaker{path}"), &init)?;

    let mut response = stub.fetch_with_request(request).await?;
    Ok(Some(response.text().await?))
}

/// A JSON response with the headers `jsonOk` sets (`http.ts`).
pub(crate) fn json(body: String, status: u16) -> Result<Response> {
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

/// A `Built` sent as-is: the status, and EXACTLY the headers it lists.
///
/// **THIS IS NOT `json`.** `json()` is `jsonOk`'s shape — a JSON content type plus the static CORS pair — and
/// every JSON route wants exactly that. The public installers do not: their 200 arm is `text/plain` with a
/// forced wildcard, and `json()` would overwrite both. The distinction is the whole reason this function
/// exists rather than a fifth caller of `json`.
pub(crate) fn built_response(built: responses::Built) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(built.body.into_bytes()))?;
    for (name, value) in &built.headers {
        response.headers_mut().set(name, value)?;
    }
    if built.status != 200 {
        response = response.with_status(built.status);
    }
    Ok(response)
}

/// **WHY A ROUTE STOPPED WITHOUT A RESPONSE OF ITS OWN — THE ONE TYPE EVERY HANDLER'S ERROR IS.**
///
/// It exists because the two arms are answered by DIFFERENT LEVELS: `Threw` is the source raising
/// (`body.username` on a `null` body, an Access JWKS key the runtime rejects, a record that vanished
/// mid-request) and the shipping front door's own catch answers it `500 Internal error`; `Platform` is a
/// `workers-rs` failure while BUILDING a response, which is not a decision at all and belongs to the runtime.
/// Collapsing them into one status would turn a platform failure into a response the source never produces.
#[derive(Debug)]
pub enum RouteFailure {
    Threw,
    Platform(Error),
}

impl From<Error> for RouteFailure {
    fn from(error: Error) -> Self {
        RouteFailure::Platform(error)
    }
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
/// **AND REFUSING HAS TO TAKE AN ORIGIN AWAY — THE `else` ARM THIS FUNCTION DID NOT HAVE.** `withCors` is
/// `stampCors` applied to a response that may ALREADY carry an ACAO (an upstream's, or a route's own: the file
/// relay re-serves the index worker's headers on its host fallback, and returns the relay's verbatim on the
/// binding arm), and the source's `else` is one line — `headers.delete("Access-Control-Allow-Origin")`
/// (`gateway/src/http.ts:124`). This port had only the `if`, so a response carrying an ACAO went to a caller the
/// allowlist had just refused: the exact hole the delete exists to close, on the one stamp every response of
/// this door passes through. **Found by reading the source against the shipping implementation, not by a failing
/// case — and no case could have found it**: of the three recorded answers that carry an ACAO, all three are
/// ALLOWED origins, and nothing paired "the answer already carries one" with "the caller's origin is refused".
/// That case is in `verify.mjs` now (`… the ACAO the relay's answer CARRIED is removed, its Vary stays`).
///
/// **ONLY THE ORIGIN IS DELETED, WHICH IS THE SOURCE'S ASYMMETRY RATHER THAN AN OVERSIGHT.** `Vary` is replaced
/// in the ALLOWED branch alone (`headers.set("Vary", "Origin")`), so a device's or a relay's own `Vary` survives
/// a refusal — which is why the corpus case carries a `Vary` as well as an ACAO. (`cors.rs::stamp_cors`, which
/// removes both, has no caller outside its own unit tests now; the two live stamps are this function and
/// `device_proxy::stamp_cors_like_ts`.)
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
    } else {
        res.headers_mut().delete("Access-Control-Allow-Origin")?;
    }
    Ok(res)
}

/// **THE SCHEDULED OBSERVATION — THE ONLY WRITER OF A ROW'S STATUS.**
///
/// `wrangler.jsonc` declares one Cron (`17 */7 * * *`), and this is what it triggers: for each channel that publishes
/// a listing this worker can read (`models_probe::listable_prefixes`), ask it what it serves and hand the answer to
/// `catalogue_service::observe`. **`Caller::Timer` is what makes the write legitimate** — the console's probe asks
/// the same upstream the same question and may not record the answer, because its answer depends on an operator's own
/// key and on the moment they pressed a button.
///
/// **THE FETCH HAPPENS HERE, NOT INSIDE `observe`**, which is why the whole judgement is testable in-process: a
/// function that fetched its own listing could only be tested against a network.
///
/// A failure in one channel must not stop the others: each is independent, and a run that died on the first
/// unreachable provider would stop observing every channel behind it.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let env_json = crate::probe::env_value(&env);
    let run_id = format!("cron-{}", crate::device_store::now_ms() / 1000);
    for prefix in crate::models_probe::listable_prefixes() {
        let listing = crate::models_probe::fetch_listing_for(&env_json, prefix).await;
        let _ = crate::catalogue_service::observe(
            &env,
            crate::catalogue_service::Caller::Timer,
            prefix,
            &run_id,
            listing,
        )
        .await;
    }
}

/// The three inputs the CORS rule takes, read off a request and the environment — `withCors`'s own three.
///
/// **ONE READER, TWO CALLERS, AND THE SECOND IS WHY IT IS A FUNCTION.** The door's stamp and the installer
/// family's 404 arm both need `(origin, host, configured)`; a second inline copy is how the two would come to
/// disagree about what "the caller's origin" means.
fn cors_inputs(req: &Request, env: &Env) -> (String, Option<String>, Option<String>) {
    let origin = req
        .headers()
        .get("origin")
        .ok()
        .flatten()
        .unwrap_or_default();
    let host = req
        .url()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()));
    let configured = env.var("CONSOLE_ORIGINS").ok().map(|v| v.to_string());
    (origin, host, configured)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Read off the request BEFORE it is moved into the routing below: the stamp needs the Origin and the host,
    // and the allowlist needs the var — the same three inputs `withCors` takes in the source.
    let (origin, host, configured) = cors_inputs(&req, &env);
    // **THE PUBLIC INSTALLER FAMILY IS CARVED OUT OF THE DOOR'S STAMP, AND THE CARVE-OUT IS THE SOURCE'S
    // SHAPE RATHER THAN A CONVENIENCE.** `index.ts` returned these three routes BEFORE its dispatch and its
    // `withCors`, forcing `Access-Control-Allow-Origin: *` because the payloads are public installers a
    // browser-hosted helper may fetch from anywhere. The stamp would DELETE that wildcard for a caller the
    // allowlist does not know — so the family carries its own headers (`tooling::installer_response`) and the
    // door leaves it alone. A 404 from the same family is stamped by the handler instead, because the source's
    // `jsonError` path DOES fall through `withCors`.
    let public_installer = req.method() == Method::Get
        && req
            .url()
            .ok()
            .map(|u| tooling::is_installer_path(u.path()))
            .unwrap_or(false);
    let res = route(req, env).await?;
    if public_installer {
        return Ok(res);
    }
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
        let origin = req
            .headers()
            .get("origin")
            .ok()
            .flatten()
            .unwrap_or_default();
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

    // **THE DECISION ROUTE, AND IT IS HANDED NO `Env`.** It is publicly reachable — this worker also serves `/v1`
    // — so a decision that cannot read the environment cannot become an oracle for a key. **The signature is the
    // defence**: `decisions::handle` has no environment parameter to thread, so the mutation that would leak a key
    // is a change to that signature rather than a line someone can slip in.
    if decisions::in_family(&req.method(), url.path()) {
        return match decisions::handle(req).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => not_found(),
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
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
    // **THE DEVICE REVERSE PROXY — the family that shares `/api/devices`' prefix and is NOT the registry.**
    // Checked BEFORE the device family for the reason `plugins/devices.ts` registers its route first: it also
    // authenticates with the paired plugin token, so it lives above the session gate, and a prefix rule for
    // the family would otherwise swallow it. `device_proxy::in_family` is the route's OWN match function
    // (`/^\/api\/devices\/[^/]+\/proxy/`), so a path it claims is a path it answers — and a path under the
    // prefix whose shape the handler's anchored regex rejects gets the front door's 404, exactly as the
    // TypeScript plugin dispatch answers `null` for it.
    if device_proxy::in_family(&req.method(), url.path()) {
        return match device_proxy::handle(req, &env).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => not_found(),
            // The arms the source raises through: `decodeURIComponent` on a malformed tool name, an upstream
            // URL the parser refuses, and a 101 without a WebSocket (the runtime's own RangeError). The
            // shipping front door's catch answers every one of them the same way.
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
    }
    // **THE FILE RELAY'S UPLOAD LEG — `POST|PUT /api/upload`, the 100 MiB passthrough.** Not under
    // `/api/devices` at all, which is why it is its own family rather than an arm of the registry: one path,
    // two verbs, and a body this worker forwards as a stream (`upload.rs`).
    if upload::in_family(&req.method(), url.path()) {
        return match upload::handle(req, &env).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => not_found(),
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
    }

    // **THE PUBLIC INSTALLER FAMILY — `GET /api/summrise-cli` AND THE TWO ONE-LINERS, the last of the public
    // CLI surface `tooling.ts` held.** What is left of that module is `probe_env_key_name`, the base64 helper
    // and the two templates; this arm is the I/O it never had: one `ASSETS` read, then `installer_response`
    // decides the bytes for all three paths. **THE SOURCE'S TWO ARMS ARE ONE HERE**: `serveAssetText` returns
    // `null` for an absent binding and for a non-ok asset, and both answer the same 404, so the `Option` is the
    // whole story. A THROWING `ASSETS.fetch` is the third case, and it is the front door's `500 Internal
    // error` — the source has no try/catch there either, so it landed in the door's own catch.
    if req.method() == Method::Get && tooling::is_installer_path(url.path()) {
        let cli = match env.get_binding::<Fetcher>("ASSETS") {
            Ok(assets) => {
                let request = Request::new("https://assets.local/summrise", Method::Get)?;
                match device::service_fetch(&assets, &request).await {
                    Ok(mut response) if response.status_code() == 200 => {
                        Some(response.text().await?)
                    }
                    // `serveAssetText`'s own "not ok → null": any non-200 is the 404 arm.
                    Ok(_) => None,
                    Err(_) => return json_error(500, "Internal error", "api_error"),
                }
            }
            Err(_) => None,
        };
        let (origin, host, configured) = cors_inputs(&req, &env);
        let cors = cors::cors_headers_for(&origin, host.as_deref(), configured.as_deref());
        let built = tooling::installer_response(cli.as_deref(), url.path(), cors);
        return built_response(built);
    }

    // **THE PUBLIC PROBE — AND THE ONE ROUTE WHOSE GUARD MUST TRAVEL WITH IT.** `ip_rate_limit` was
    // ported with the device family, and the probe kept its own copy in TypeScript until the route
    // moved: a guard and what it guards belong in the same place.
    if probe::in_family(&req.method(), url.path()) {
        return match probe::handle(req, &env).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => not_found(),
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
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
    // **THE IDENTITY SURFACE — the fourth route group: the session, the caller, and the caller's own
    // credentials.** It is checked AFTER the device family because the two are disjoint lists of paths (the
    // device family is `/api/devices*` + three public device routes; this one is `/api/auth/*` + `/api/me*`), and
    // the order is written down so a future path that belongs to both is a visible decision rather than an
    // accident of layout.
    //
    // `auth_routes::handle` answers its own 404 for a family path it does not own — that is not a fallback, it
    // is the front door's answer for a path under a prefix whose plugin has no route for it, and the corpus
    // compares it (`GET /api/auth/nonsense`).
    // **THE ADMIN FAMILY — `/api/admin/*`, the console's own surface.** Ordered here because it is the same
    // kind of thing as the auth family above it: a route table over session-gated stores. A path under the
    // prefix that no slice serves yet answers this module's 404, which is the front door's answer for a family
    // prefix with no route under it.
    if admin_routes::in_family(&req.method(), url.path()) {
        return match admin_routes::handle(req, &env).await {
            Ok(response) => Ok(response),
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
    }
    if auth_routes::in_family(&req.method(), url.path()) {
        // **THE FAMILY THAT RESOLVES AN IDENTITY IS THE FAMILY THAT MUST HAVE ONE.** `user_store::seed_admin` is
        // the console's `seedAdmin` — the v1 migration, the empty-token backfill and the fresh record — and it is
        // called HERE rather than on every request because this is the only surface that cannot answer without it:
        // login mints a session for an id that must resolve, and `require_cookie_session` reads `user:<id>`. **A
        // FRESH DEPLOYMENT SERVED BY THIS WORKER HAD NO ADMIN RECORD AT ALL** — `getAdminPassword` migrates the
        // Worker secret, so the password verified, and then the session it issued resolved to nothing.
        //
        // The cost is once per isolate: `seed_admin`'s first act is an atomic flag, and the next 1000 requests
        // through this branch pay one `swap`.
        user_store::seed_admin(&env).await;
        return match auth_routes::handle(req, &env).await {
            Ok(response) => Ok(response),
            // The one arm the routes do not answer themselves: the source THREW (`Failure::Threw`) — a `null`
            // body on `login`, an Access JWKS key the runtime rejects, a user record that vanished mid-request.
            // The shipping front door's catch answers exactly this, and the corpus has cases for two of them.
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
    }
    // **THE MCP SURFACE — `/mcp`, the JSON-RPC endpoint a model drives a device through.** One path, and the
    // predicate is mirrored EXACTLY in `mcp.rs::in_family`, so a path handed over is a path served. The
    // plugin's OTHER route (`GET /api/plugins/status`) is deliberately NOT in the family: its response carries
    // the TypeScript plugin registry's own dispatch counters, which this worker does not have and must not
    // invent. The harness's cutover section proves that it stays where it is.
    if mcp::in_family(&req.method(), url.path()) {
        return match mcp::handle(req, &env).await {
            Ok(response) => Ok(response),
            // The one arm the endpoint does not answer itself: the source THREW (a JSON body that is a literal
            // `null` destructures to a TypeError). The shipping front door's catch answers exactly this.
            Err(RouteFailure::Threw) => json_error(500, "Internal error", "api_error"),
            Err(RouteFailure::Platform(error)) => Err(error),
        };
    }
    not_found()
}

pub mod access;
pub mod admin_routes;
pub mod auth;
pub mod auth_routes;
pub mod auto_route;
pub mod body_scan;
pub mod byok;
pub mod catalogue;
pub mod catalogue_file;
pub mod catalogue_service;
pub mod cors;
pub mod decision_client;
pub mod decisions;
pub mod device;
pub mod device_proxy;
pub mod device_registry;
pub mod device_store;
pub mod devices;
pub mod ip_rate_limit;
pub mod key_lock;
pub mod key_probe;
pub mod lifecycle;
pub mod mcp;
pub mod mcp_browser;
pub mod mcp_errors;
pub mod mcp_tools;
pub mod model_store;
pub mod models_probe;
pub mod probe;
pub mod rate_limit;
pub mod redact;
pub mod registry;
pub mod reliability;
pub mod request_shape;
pub mod responses;
pub mod route_do;
pub mod routing;
pub mod session;
pub mod session_gate;
pub mod settings_logic;
pub mod store;
pub mod stream;
pub mod tokens;
pub mod tool_policy;
pub mod tooling;
pub mod translate;
pub mod upload;
pub mod user_store;
pub mod v1;
pub mod vision;
pub mod webcrypto;
