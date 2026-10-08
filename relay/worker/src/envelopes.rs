//! THE ENVELOPES BOTH wasm32 MODULES SEND — one definition, because the file relay's worker and its claim
//! Durable Object answer the same three JSON shapes and a second copy is how they would drift.
//!
//! **EVERY ONE IS A HAND-BUILT `format!` RATHER THAN a `serde_json::Map`.** The differential compares bytes,
//! `JSON.stringify` emits keys in insertion order, and a `Map`'s default order is alphabetical — so the
//! envelope is assembled exactly as the shipping worker assembles it. `UNAVAILABLE`'s body is shared with
//! the host-side constant in `lib.rs`, which is where the decision half keeps it.
#![cfg(target_arch = "wasm32")]

use worker::*;

use crate::UNAVAILABLE;

/// `{"error":"…"}` with `content-type: application/json` — the shape every refusal in this worker uses.
pub fn error_json(status: u16, message: &str) -> Result<Response> {
    json(status, format!(r#"{{"error":"{message}"}}"#))
}

pub fn json(status: u16, body: String) -> Result<Response> {
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(status)
        .with_headers(headers))
}

/// The 503 an R2/DO outage surfaces as — never an uncaught throw, which the platform answers with its own
/// HTML 500 that no device-side reader parses.
pub fn unavailable() -> Result<Response> {
    json(UNAVAILABLE.status, UNAVAILABLE.body.to_string())
}

/// **`new Response("Not Found", { status: 404 })` — THE ONE NON-JSON REFUSAL IN THIS WORKER**, and it is
/// the claim route's answer for a path or method it does not name. A JSON envelope here would be a
/// different contract for the same status, which is exactly what the differential compares.
pub fn plain_not_found() -> Result<Response> {
    // **THE RUNTIME'S OWN DEFAULT FOR A STRING BODY, WRITTEN DOWN**: JavaScript's `new Response("Not
    // Found")` carries `content-type: text/plain;charset=UTF-8`, and workers-rs's `from_body` sets no
    // content type at all. The differential is what found it — four cases, byte-identical bodies and
    // statuses, differing on this one header.
    let headers = Headers::new();
    headers.set("content-type", "text/plain;charset=UTF-8")?;
    Ok(
        Response::from_body(ResponseBody::Body(b"Not Found".to_vec()))?
            .with_status(404)
            .with_headers(headers),
    )
}

/// `tooLargeResponse(MAX_BYTES)` — the 413 both upload arms answer with.
pub fn too_large(max_bytes: u64) -> Result<Response> {
    json(
        413,
        format!(r#"{{"error":"file too large (max {max_bytes} bytes)"}}"#),
    )
}
