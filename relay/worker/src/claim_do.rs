//! THE CLAIM DURABLE OBJECT, IN RUST — and the answer to the race the JavaScript's own comment describes.
//!
//! ── WHY THE DO EXISTS AT ALL, IN THE SHIPPING WORKER'S WORDS ─────────────────────────────────────
//!
//! `GET /files/<token>` used to R2-get, stream, then delete, and **two concurrent GETs could both pass the
//! get before either delete landed** — the "one-time" file downloaded twice. R2 has a compare-and-swap on
//! create but no conditional DELETE, and KV is last-write-wins, so neither can close it. A Durable Object
//! instance named by the token is the primitive that can: **the runtime delivers one instance's requests
//! strictly one at a time**, so the first claim wins and the losers observe the winner's delete as "gone".
//!
//! The port keeps that shape exactly — the worker forwards, this class decides and deletes — because the
//! serialization is the *runtime's* property, not something either implementation could reproduce locally.
//!
//! ── WHAT THE DIFFERENTIAL CAN AND CANNOT SEE, STATED RATHER THAN IMPLIED ─────────────────────────
//!
//! `tests/claim_differential.rs` drives this class and the shipping `TempClaimDO` through the same cases
//! with the same R2 stub, and compares status, headers, body bytes and **the bucket operations in order** —
//! so "delete before streaming" and "delete only on expiry" are both visible. **THE SERIALIZATION ITSELF IS
//! NOT**: no stub can show that two concurrent claims do not both win, because that is a guarantee of the
//! runtime both implementations are deployed into. It is named here so a reader does not mistake a green
//! differential for a proof of it.
//!
//! MUTATION: return the object's bytes WITHOUT deleting the key first.
//! RESULT:   **MEASURED — three serve cases fail on the `r2 operations` row alone:**
//!             FAIL GET /files/AAAABBBBCCCCDDDDEEEEFF — 14 bytes, status 200
//!             r2 operations DIFFERS
//!               recorded: "get files/… ; delete files/…"
//!               rust:     "get files/…"
//!           The status and the bytes are identical, and the file would download twice. **That row is the
//!           entire reason this object exists**, and this is the mutation it was built around.
//!
//! MUTATION: treat an unparseable `expiresAt` as "serve" (the pre-2026-09-23 fail-open rule).
//! RESULT:   the corpus's `"abc"` and `{}` rows fail: the shipping rule is 410 and a delete, and the
//!           difference is an unbounded download from a corrupt deadline.

use worker::*;

use crate::envelopes::{error_json, plain_not_found, unavailable};
use crate::{claim_token, decide_claim, Claim};

#[durable_object]
pub struct TempClaimDO {
    env: Env,
}

/// `authorized(request, env)` — **IT ACTIVATES ONLY WHEN `DO_AUTH` IS CONFIGURED**, so existing deploys
/// without the secret keep working, and it is defence in depth for the residual risk that a DO instance has
/// its own external address even with `workers_dev: false`.
fn authorized(req: &Request, env: &Env) -> Result<bool> {
    let expected = env
        .var("DO_AUTH")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if expected.is_empty() {
        return Ok(true);
    }
    let got = req.headers().get("x-do-auth")?.unwrap_or_default();
    if got.len() != expected.len() {
        return Ok(false);
    }
    let diff = got
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    Ok(diff == 0)
}

impl DurableObject for TempClaimDO {
    fn new(_state: State, env: Env) -> Self {
        Self { env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        // The instance is named by the token, so a path it does not name is a 404 — and the route is a GET.
        let Some(token) = claim_token(url.path()) else {
            return plain_not_found();
        };
        if req.method() != Method::Get {
            return plain_not_found();
        }
        if !authorized(&req, &self.env)? {
            return error_json(401, "unauthorized");
        }
        let key = format!("files/{token}");
        let bucket = self.env.bucket("TEMP_FILES")?;
        // P1-1: R2 get/delete are network I/O — an outage must surface as the 503 envelope, never as an
        // uncaught throw (the platform's 500 HTML, which no device-side reader parses).
        let object = match bucket.get(key.clone()).execute().await {
            Ok(o) => o,
            Err(_) => return unavailable(),
        };
        // `obj && obj.customMetadata && obj.customMetadata.expiresAt` — absent metadata is `undefined`,
        // which the rule fails OPEN on (legacy uploads predate the field).
        let expires = object
            .as_ref()
            .and_then(|o| o.custom_metadata().ok())
            .and_then(|m| m.get("expiresAt").cloned());
        let raw = expires.map(serde_json::Value::String);
        let decision = decide_claim(
            object.is_some(),
            raw.as_ref(),
            Date::now().as_millis() as f64,
        );
        match decision {
            Claim::Gone => error_json(404, "file not found or already downloaded"),
            Claim::Expired => {
                if bucket.delete(key).await.is_err() {
                    return unavailable();
                }
                error_json(410, "file expired")
            }
            Claim::Serve => {
                // **ONE-TIME: THE KEY IS DELETED BEFORE THE BODY STREAMS**, so a retry after a completed
                // download 404s. Concurrent claims cannot both arrive here — the runtime serializes the
                // instance's input queue, and a loser observes this delete as "gone". (`obj.body` stays
                // readable: `get()` already fetched the object, and deleting the key does not invalidate it.)
                if bucket.delete(key).await.is_err() {
                    return unavailable();
                }
                let Some(object) = object else {
                    return error_json(404, "file not found or already downloaded");
                };
                let metadata = object.http_metadata();
                let headers = Headers::new();
                headers.set(
                    "content-type",
                    metadata
                        .content_type
                        .as_deref()
                        .unwrap_or("application/octet-stream"),
                )?;
                headers.set(
                    "content-disposition",
                    metadata
                        .content_disposition
                        .as_deref()
                        .unwrap_or("attachment"),
                )?;
                headers.set("cache-control", "no-store")?;
                let Some(body) = object.body() else {
                    return unavailable();
                };
                Ok(Response::from_body(body.response_body()?)?.with_headers(headers))
            }
        }
    }
}
