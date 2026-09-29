//! The CDN worker's I/O and its entry point — the half that cannot be compared without a runtime.
//!
//! ## Why this file exists, and what it is allowed to decide
//!
//! `routes.rs` and the manifest in `lib.rs` are pure, and the differentials prove them against the
//! JavaScript **that ships**. What is left here is everything that reaches out: two ASSETS reads, two
//! R2 objects, one GitHub proxy, and the page render. **`wrangler dev` cannot start on this box**
//! (workerd needs GLIBC ≥ 2.32, this is 2.31 — measured, P3.0) and `wrangler tail` is a TLS reset,
//! so the BUILT ARTIFACT is executed in Node instead, with `verify.mjs` supplying the two runtime
//! imports the module needs. That is the arrangement `gateway/wasm` uses for the gateway's first
//! route, and it is why this crate's decision half could be proven at all: **the pure half was
//! extracted so the I/O half would need the least possible trust.**
//!
//! ## WHAT IS STILL THE JAVASCRIPT'S
//!
//! The bodies of the three big-artifact routes — the etag revalidation, the `content-disposition`
//! filenames, the R2 `size === 0` rule that refuses a zero-byte object — are NOT ported here. They
//! are recorded as what they are: the routes that read an object, and the two rules that decide
//! whether it is fresh. The honest position after this commit is that **the CDN worker is half Rust**:
//! its decision, its manifest and its page boundary are, and its three artifact routes still are not.
//! `lib.rs` says the same about the arms, and the plan records the order the dependency graph forces.

use crate::routes::{route, Route};
use worker::*;

/// The versioned cloudflared asset this worker proxies — a PINNED one, never `latest`.
///
/// The pin is the whole point and the comment is the worker's own: `.../releases/latest/download/…`
/// means the bytes of a binary the service SPAWNS were chosen by whatever GitHub marked latest at that
/// moment, while the agent's own `CLOUDFLARED_SHA256` only holds "while latest stays". A versioned
/// asset path is immutable, so the proxy and the pin agree BY CONSTRUCTION.
const CLOUDFLARED_VERSION: &str = "2026.8.3";

/// The JSON envelope every failure on this worker uses.
///
/// **A FAILURE MUST NOT BE CACHEABLE, AND THAT IS WHY THIS TAKES A STATUS.** Round 99 F3 found all
/// three binary proxies returning bare text, and a REJECTED fetch propagating as the platform's 500
/// HTML page, which no device-side reader parses. Round 130 found the manifest handler hand-rolling a
/// `503` with no `content-type` and no `CACHE-CONTROL` — and a 503 with no directives MAY BE STORED by
/// a shared cache, so `/api/version` could keep answering a failure long after the manifest was fixed.
fn proxy_failure(what: &str, detail: &str, status: u16) -> Response {
    // THE BODY IS ONE STRING, NOT TWO KEYS: the JavaScript's helper is
    // `JSON.stringify({ error: `${what}: ${detail}` })` — the two are JOINED BY A COLON inside one
    // value. The first version built `{"error":"…":"…"}`, which is not JSON, and the verify caught it
    // as a one-byte difference on the two failure routes. A failure body that will not parse is the
    // worst possible answer from the route a device's updater polls.
    let body = json_string(&format!("{what}: {detail}"));
    let body = format!("{{\"error\":{body}}}");
    let h = Headers::new();
    h.set("content-type", "application/json").ok();
    h.set("cache-control", "no-store").ok();
    match Response::from_bytes(body.into_bytes()) {
        Ok(r) => r.with_status(status).with_headers(h),
        Err(_) => Response::empty().unwrap_or_else(|_| Response::empty().expect("empty")),
    }
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// The worker's entry point.
///
/// THE ORDER IS `routes.rs`'s, and the dispatch is a `match` on its answer rather than a second chain
/// of `if`s: a route table that is written twice is a route table that can disagree with itself.
#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let pathname = url.path();
    match route(pathname) {
        Route::Manifest => manifest(req, env, &url).await,
        // THE TWO ASSETS ROUTES ARE A PASS-THROUGH OF THE REQUEST ITSELF, which is what the
        // JavaScript does — `env.ASSETS.fetch(request)`, the request and not a URL rebuilt from it,
        // so a range header or a conditional survives.
        Route::Installer | Route::Tarball => env.assets("ASSETS")?.fetch_request(req).await,
        // NOT PORTED: the three artifact routes below are still the JavaScript's, and the comment on
        // this arm is the reason. See the module header.
        Route::Cloudflared => cloudflared(req, &env).await,
        Route::Electron | Route::Playwright => r2_object(req, &env, pathname).await,
        Route::Landing => landing(req, env, &url).await,
        // `new Response("Not Found", { status: 404 })` SETS `text/plain;charset=UTF-8` because a
        // string body gets a text type. The first version answered it from the same byte path as the
        // artifacts and said `application/octet-stream`, and the verify caught the header on two
        // routes: a 404 that claims to be a binary is a wrong claim, not a cosmetic one.
        Route::NotFound => {
            // SET EXPLICITLY, because `from_bytes` sets no type at all: the JavaScript's
            // `new Response("Not Found", …)` gets `text/plain;charset=UTF-8` from the string body,
            // and an EMPTY header map here replaced the inferred type with none.
            let h = Headers::new();
            h.set("content-type", "text/plain;charset=UTF-8").ok();
            Ok(Response::from_bytes(b"Not Found".to_vec())?
                .with_status(404)
                .with_headers(h))
        }
    }
}

/// `/api/version` — the manifest, and the WHOLE thing is `lib.rs::manifest`.
async fn manifest(req: Request, env: Env, url: &Url) -> Result<Response> {
    let origin = url.origin().ascii_serialization();
    let body = match read_manifest_json(&env).await {
        Some(text) => match crate::manifest(&origin, &text, &mut |_| {}) {
            crate::ManifestAnswer::Ok(body) => body,
            crate::ManifestAnswer::Failure { .. } => {
                return Ok(proxy_failure(
                    "release manifest unavailable",
                    "assets unavailable and no cached manifest",
                    503,
                ))
            }
        },
        None => {
            return Ok(proxy_failure(
                "release manifest unavailable",
                "assets unavailable and no cached manifest",
                503,
            ))
        }
    };
    let _ = req;
    let h = Headers::new();
    h.set("content-type", "application/json").ok();
    h.set("cache-control", "no-store").ok();
    Ok(Response::from_bytes(body.into_bytes())?.with_headers(h))
}

/// `version.json` through the ASSETS binding, or nothing.
async fn read_manifest_json(env: &Env) -> Option<String> {
    let url = worker::Url::parse("https://worker.local/summrise-agent/version.json").ok()?;
    let mut resp = env.assets("ASSETS").ok()?.fetch(url, None).await.ok()?;
    // `status_code()`, NOT `resp.ok()`: `ok` is an ASSOCIATED FUNCTION in this version and the first
    // version wrote `resp.ok()?`, which is a method call on a type that has no such method. A
    // 2xx check is the rule here — the JavaScript tests `vresp.ok`, and `/api/version` must not treat
    // a 404 or a 500 as a manifest.
    if !(200..300).contains(&resp.status_code()) {
        return None;
    }
    resp.text().await.ok()
}

/// The cloudflared proxy: a PINNED upstream, a streamed body, and revalidation.
async fn cloudflared(req: Request, _env: &Env) -> Result<Response> {
    let upstream = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/cloudflared-windows-amd64.exe"
    );
    // THE PINNED UPSTREAM, WITH REDIRECTS FOLLOWED — `redirect: "follow"` in the JavaScript. It is a
    // `Fetcher` and not `req.inner()`: `web_sys::Request` has no `fetch_with_init`, and the fetch has
    // to be its own call anyway because the upstream URL is not this request's own.
    // `Fetch::Request(req).send()` IS THE GLOBAL FETCH — the binding behind the JavaScript's bare
    // `fetch(url, …)`. A `Fetcher` is for calling ANOTHER worker's fetch handler and has no public
    // constructor in 0.8.7, so it is not the thing here.
    let upstream_req = Request::new_with_init(
        &upstream,
        RequestInit::new().with_redirect(RequestRedirect::Follow),
    )?;
    let upstream_resp = match Fetch::Request(upstream_req).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(proxy_failure(
                "cloudflared upstream fetch failed",
                &e.to_string(),
                502,
            ))
        }
    };
    if !(200..300).contains(&upstream_resp.status_code()) {
        return Ok(proxy_failure(
            "cloudflared upstream fetch failed",
            "non-ok upstream",
            502,
        ));
    }
    // `Headers::get` ANSWERS `Option<String>` here, so this is a `?` away — not a HeaderValue with a
    // `to_str` on it, which is what the first version wrote and what the compiler answered.
    let etag: Option<String> = upstream_resp.headers().get("etag")?;
    if let Some(tag) = &etag {
        if request_etag(&req).as_deref() == Some(tag.as_str()) {
            let h = etag_headers(Some(tag.clone()));
            return Ok(Response::empty()?.with_status(304).with_headers(h));
        }
    }
    // `body()` is NOT an Option in this version — it answers the body, and the streaming is what
    // keeps 54 MB off the isolate's memory, which is the whole reason this route exists.
    // THE STREAM IS PASSED THROUGH, NOT BUFFERED — 54 MB of tunnel binary must not cross the
    // isolate's memory, which is the whole reason this route exists (the JavaScript says so beside
    // it: "Stream the body through (no buffering — 54MB fits the response path)"). The binding
    // models a body as an ENUM, so the stream is taken out of it and handed straight on.
    let body = match upstream_resp.body() {
        ResponseBody::Stream(s) => ResponseBody::Stream(s.clone()),
        // A buffered upstream body is already in memory; forwarding it unchanged is the same bytes.
        ResponseBody::Body(b) => ResponseBody::Body(b.clone()),
        // An EMPTY upstream body: the JavaScript would stream nothing and answer 200 with no bytes,
        // so this is the same answer rather than a new one.
        ResponseBody::Empty => ResponseBody::Empty,
    };
    let mut out = Response::from_body(body)?.with_status(200);
    let h = Headers::new();
    h.set("content-type", "application/octet-stream").ok();
    h.set(
        "content-disposition",
        "attachment; filename=\"cloudflared.exe\"",
    )
    .ok();
    h.set("cache-control", "public, no-cache").ok();
    if let Some(tag) = &etag {
        h.set("etag", tag).ok();
    }
    out.headers_mut().clone_from(&h);
    Ok(out)
}

/// One of the two R2 artifacts. **THE ZERO-BYTE RULE IS THE PORTED HALF**: an absent object answers
/// 502 and warns, and an EMPTY one would answer 200 and stage nothing — the same false-success shape
/// this suite keeps finding. Absent and empty are the same verdict.
async fn r2_object(req: Request, env: &Env, pathname: &str) -> Result<Response> {
    let key = pathname.rsplit('/').next().unwrap_or(pathname).to_string();
    let bucket = env.bucket("TEMP_FILES")?;
    // `get()` RETURNS A BUILDER, NOT A FUTURE: `.execute().await` is the call. The first version
    // awaited the builder, which the compiler catches and a reviewer reads past.
    let body = match bucket.get(&key).execute().await {
        Ok(Some(obj)) => obj,
        _ => return Ok(proxy_failure("artifact unavailable", "not in R2", 502)),
    };
    if body.size() == 0 {
        return Ok(proxy_failure("artifact unavailable", "not in R2", 502));
    }
    // `http_etag()` returns a `String` in this version — R2 always has an etag for an object that
    // exists, and an ABSENT object was already handled above, so there is no Option to unwrap.
    let etag: Option<String> = Some(body.http_etag());
    if let Some(tag) = &etag {
        if request_etag(&req).as_deref() == Some(tag.as_str()) {
            let h = etag_headers(Some(tag.clone()));
            return Ok(Response::empty()?.with_status(304).with_headers(h));
        }
    }
    let content_type = if key.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    // THE BODY IS STREAMED, AND THE BINDING HANDS IT OVER AS A `Response`: `Object::body()` is
    // `Option<ObjectBody>` and `ObjectBody::stream()` reads that response's body. The first version
    // reached for a `ResponseBody` the value never was, and the artifact came back as the SOURCE of
    // the function — which the verify reported as a 58-byte body, and which a device would have
    // written to disk.
    let object_body = body
        .body()
        .ok_or_else(|| worker::Error::RustError("the R2 object carries no body".into()))?;
    let mut out = Response::from_stream(object_body.stream()?)?.with_status(200);
    let h = Headers::new();
    h.set("content-type", content_type).ok();
    h.set(
        "content-disposition",
        &format!("attachment; filename=\"{key}\""),
    )
    .ok();
    h.set("cache-control", "public, no-cache").ok();
    // ONLY THE PLAYWRIGHT ROUTE CARRIES A LENGTH, and the JavaScript is not symmetrical about it: the
    // electron route omits it and the playwright one writes `content-length: String(obj.size)`. The
    // verify caught the missing header on exactly one of the two, which is the answer to "should both
    // have it" — no, and matching means reading the source rather than the principle.
    if key == "summrise-playwright.zip" {
        h.set("content-length", &body.size().to_string()).ok();
    }
    if let Some(tag) = &etag {
        h.set("etag", tag).ok();
    }
    out.headers_mut().clone_from(&h);
    Ok(out)
}

/// `/` and `/index.html` — the landing page, rendered by `lib.rs::page` over the ARM this worker
/// carries. **WHICH ARM** is the manifest's answer and nothing else: a tgz-only publish leaves the
/// versionless alias serving the PREVIOUS release while `/api/version` advertises the new one, so
/// offering the door unconditionally hands a fresh install the old build.
async fn landing(req: Request, env: Env, url: &Url) -> Result<Response> {
    let origin = url.origin().ascii_serialization();
    let installer = format!("{origin}/summrise-agent/summrise-agent-latest.tgz");
    let manifest_json = read_manifest_json(&env).await;
    let setup = manifest_json.as_deref().and_then(|text| {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        // The presence of a WELL-FORMED installer pin is the whole question — round 125 — and the
        // URL is rebuilt from THIS request's origin, never taken from the manifest.
        v.get("installer")?.as_str()?;
        let sha = v.get("installer_sha256")?.as_str()?;
        let ok = crate::is_sha256(sha);
        if ok {
            Some(format!("{origin}/summrise-agent/SummriseAgent-Setup.exe"))
        } else {
            None
        }
    });
    let console = env
        .var("CONSOLE_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| origin.clone());
    let body = crate::page(
        crate::arms::setup_arm(),
        crate::arms::npm_only_arm(),
        &console,
        &installer,
        setup.as_deref(),
    );
    // THE ETAG IS A SHA-256 OF THE RENDERED BODY, truncated to 32 hex characters and quoted.
    let digest = crate::arms::etag(&body);
    if request_etag(&req).as_deref() == Some(digest.as_str()) {
        let h = etag_headers(Some(digest));
        return Ok(Response::empty()?.with_status(304).with_headers(h));
    }
    let h = Headers::new();
    h.set("content-type", "text/html; charset=utf-8").ok();
    h.set("etag", &digest).ok();
    h.set("cache-control", "public, no-cache").ok();
    Ok(Response::from_bytes(body.into_bytes())?.with_headers(h))
}

fn etag_headers(etag: Option<String>) -> Headers {
    let h = Headers::new();
    h.set("cache-control", "public, no-cache").ok();
    if let Some(tag) = etag {
        h.set("etag", &tag).ok();
    }
    h
}

fn request_etag(req: &Request) -> Option<String> {
    req.headers().get("if-none-match").ok().flatten()
}
