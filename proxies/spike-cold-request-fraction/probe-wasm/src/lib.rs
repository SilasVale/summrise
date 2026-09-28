//! The Rust half of the isolate-survival probe. THROWAWAY.
//!
//! Same shape as the TypeScript half, and the same two measured traps it
//! records: `Date::now()` is **0** during global scope evaluation in Workers, so
//! the isolate's birth is stamped on its first request; and the id is minted
//! lazily, where random values are allowed.
//!
//! Every number in the response is an INTEGER and every string is built with
//! `format!` on integers only — `{:.3}` on an f64 chains Rust's float formatter
//! into the binary (the landing-page spike measured that at 8,879 bytes gz).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use worker::*;

static ISOLATE_BORN: OnceLock<u64> = OnceLock::new();
static ISOLATE_ID: OnceLock<String> = OnceLock::new();
static REQUESTS: AtomicU64 = AtomicU64::new(0);

#[event(start)]
fn start() {
    let _ = &ISOLATE_ID;
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let t0 = Date::now().as_millis();
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
    let body = format!(
        "{{\"isolateId\":\"{}\",\"requestIndex\":{},\"isolateAgeMs\":{}}}",
        ISOLATE_ID.get().cloned().unwrap_or_default(),
        mine,
        t0.saturating_sub(born)
    );
    let mut resp = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    resp.headers_mut().set("content-type", "application/json")?;
    resp.headers_mut().set("cache-control", "no-store")?;
    Ok(resp)
}
