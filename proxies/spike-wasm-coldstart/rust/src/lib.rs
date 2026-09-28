//! The Rust (workers-rs 0.8.7) half of the cold-start instrument. THROWAWAY —
//! the deployed worker is deleted after the measurement (see ../README.md).
//!
//! `#[event(start)]` runs once per isolate, AFTER the wasm module has been
//! compiled and instantiated. That is the one asymmetry with the TypeScript
//! half, and it is stated rather than hidden: anything it measures EXCLUDES
//! wasm instantiation — the very cost this measurement exists to find. So it is
//! not used for timing at all; the isolate's birth is stamped on its first
//! request instead, exactly as the TypeScript half does, because the Workers
//! runtime returns **0** from `Date::now()` during global scope evaluation
//! (measured: a first response reported `isolateAgeMs: 1790601284826`).
//!
//! Every number in the response is an INTEGER and every string is built with
//! `format!` on integers only: `{:.3}` on an f64 would chain Rust's float
//! formatter into the binary (the landing-page spike measured that at 8,879
//! bytes gz, 26% of that payload) and inflate the bundle-size number.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use worker::*;

static ISOLATE_BORN: OnceLock<u64> = OnceLock::new();
static ISOLATE_ID: OnceLock<String> = OnceLock::new();
static REQUESTS: AtomicU64 = AtomicU64::new(0);

#[event(start)]
fn start() {
    // Deliberately no clock read here: see the header. The id is minted on the
    // first request, where random values are allowed.
    let _ = &ISOLATE_ID;
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let t0 = Date::now().as_millis();
    let url = req.url()?;

    let mine = REQUESTS.fetch_add(1, Ordering::SeqCst) + 1;
    if mine == 1 {
        let _ = ISOLATE_BORN.set(t0);
        let _ = ISOLATE_ID.set(format!(
            "{:08x}{:08x}",
            (js_sys::Math::random() * 4294967296.0) as u32,
            (js_sys::Math::random() * 4294967296.0) as u32
        ));
    }
    let born = *ISOLATE_BORN.get().unwrap_or(&t0);
    let isolate_age = t0.saturating_sub(born);
    let id = ISOLATE_ID.get().cloned().unwrap_or_default();

    let mut msg = String::from("hello");
    let mut n: u64 = 0;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "msg" => msg = v.into_owned(),
            "n" => n = v.parse().unwrap_or(0),
            _ => {}
        }
    }

    // Identical arithmetic to the TypeScript half (exact u64 there and here).
    let mut acc: u64 = 0;
    for i in 0..n {
        acc += (i.wrapping_mul(31).wrapping_add(7)) % 1000003;
    }

    let body = format!(
        "{{\"ok\":true,\"who\":\"rust\",\"isolateId\":\"{}\",\"requestIndex\":{},\
\"isolateAgeMs\":{},\"handlerMs\":{},\"n\":{},\"acc\":{},\"msg\":\"{}\"}}",
        id,
        mine,
        isolate_age,
        Date::now().as_millis().saturating_sub(t0),
        n,
        acc,
        msg
    );

    let mut resp = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    resp.headers_mut().set("content-type", "application/json")?;
    resp.headers_mut().set("cache-control", "no-store")?;
    Ok(resp)
}
