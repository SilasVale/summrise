//! THE FILE RELAY'S ENTRY POINT, IN RUST — the routing, the upload screening, and the claim forward.
//!
//! This is the second half of the port whose decisions landed in `lib.rs` (the header builder, the claim
//! rule, the token generator, `Number()`). What is here is what the shipping `relay/src/index.js` does at
//! the edge of the world: three routes, the credential, the multipart and raw upload arms, and the
//! forward into the claim Durable Object.
//!
//! ── THE FOUR THINGS THAT ARE EASY TO GET WRONG, EACH ONE MEASURED OR ARGUED ─────────────────────
//!
//! **① JSON KEY ORDER IS BYTES.** `JSON.stringify` emits keys in insertion order and the differential
//! compares bytes, so every envelope here is a hand-built `format!` rather than a `serde_json::Map`
//! (whose default is alphabetical) — the same reason `json_error_body` in the zen satellites is a
//! `format!`.
//!
//! **② THE RAW ARM STREAMS, AND THAT IS THE POINT OF IT.** Round-554 split `PUT /api/upload` out of the
//! multipart path because `formData()` materializes the whole body inside the isolate's 128 MB ceiling.
//! So this arm hands R2 `Data::ReadableStream(req.inner().body())` — **a port that read the body into a
//! `Vec<u8>` would be a different worker under a large upload**, and the differential could not see it.
//!
//! **③ THE 411 IS A SCREEN, NOT A NICETY.** A chunked client sends no `content-length`; the shipping
//! worker refuses (411) rather than buffering an unbounded body and answering 413 after the fact. Both
//! upload arms carry it, and the corpus has a case for each.
//!
//! **④ THE CLIENT'S FILENAME NEVER SHAPES A HEADER UNLESS IT SURVIVES SANITIZING** — `?name=../../evil.txt`
//! is reduced to its basename first, and a name nothing survives is a 400 rather than an R2 put with a
//! forged `Content-Disposition`.
//!
//! ── THE MUTATION THAT MUST FAIL THIS FILE'S COMPANION ────────────────────────────────────────────
//!
//! MUTATION: forward the claim request WITHOUT setting `x-do-auth` when `DO_AUTH` is configured.
//! RESULT:   `tests/worker_differential.rs` fails the DO_AUTH case on the `forwarded` row — the response
//!           bytes are identical (the stub answers the same either way), which is exactly why that row
//!           exists. The mutation is written into the corpus's own case list, and the row is what catches
//!           it.

use sha2::{Digest, Sha256};
use worker::*;

use crate::{build_content_disposition, decide_claim, gen_token_from, js_trim, Claim, UNAVAILABLE};

const MAX_BYTES: u64 = 100 * 1024 * 1024;
/// The multipart framing (boundary + part headers) rides on top of the file bytes, so the pre-screen
/// allows a margin and the authoritative check stays `file.size()`.
const CL_MARGIN: u64 = 64 * 1024;
const NOTE: &str = "one-time download: file is deleted after first access or 24h";

fn json(status: u16, body: String) -> Result<Response> {
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    Ok(Response::from_bytes(body.into_bytes())?
        .with_status(status)
        .with_headers(headers))
}

fn error_json(status: u16, message: &str) -> Result<Response> {
    // `{"error":"…"}` — the shipping envelope, with the message unescaped because every call site passes
    // a fixed string or a value that has already been reduced (a filename's basename, an error's text).
    json(status, format!(r#"{{"error":"{message}"}}"#))
}

fn too_large() -> Result<Response> {
    json(
        413,
        format!(r#"{{"error":"file too large (max {MAX_BYTES} bytes)"}}"#),
    )
}

/// `safeEq(a, b)` — SHA-256 both sides to fixed 32-byte digests, then fold XOR across every byte without
/// short-circuiting. **The digest is what removes the length early-exit**, which is why the shipping
/// worker hashes instead of comparing.
fn safe_eq(a: &str, b: &str) -> bool {
    let da = Sha256::digest(a.as_bytes());
    let db = Sha256::digest(b.as_bytes());
    da.iter()
        .zip(db.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// `Number(header)` for the length screens: the header is a string, and the shipping worker's
/// `Number()` answers `NaN` for anything that is not a number — which the raw arm turns into 400 and the
/// multipart arm into a comparison that is false (so it proceeds to `formData()`, which then decides).
fn js_number_header(raw: &str) -> f64 {
    let t = js_trim(raw);
    if t.is_empty() {
        return 0.0;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// **THE RANDOMNESS IS THE RUNTIME'S OWN `crypto.getRandomValues`**, reached through the global object
/// rather than through `Math.random` — a token that is a capability (the shipping comment: ~131 bits,
/// unguessable) must not come from a PRNG. `worker::crypto` exposes digests only, so the call is made
/// through `js_sys` exactly as the shipping worker makes it.
fn random_bytes(len: u32) -> Vec<u8> {
    let buf = js_sys::Uint8Array::new_with_length(len);
    let global = js_sys::global();
    let crypto = js_sys::Reflect::get(&global, &wasm_bindgen::JsValue::from_str("crypto"))
        .expect("the runtime has a crypto global");
    let get = js_sys::Reflect::get(&crypto, &wasm_bindgen::JsValue::from_str("getRandomValues"))
        .expect("crypto has getRandomValues");
    let f: js_sys::Function = get.into();
    f.call1(&crypto, &buf).expect("getRandomValues");
    buf.to_vec()
}

/// The token and the deadline both arms mint: `genToken(22)` from the runtime's randomness, and 24 hours
/// from the clock.
fn mint() -> (String, i64) {
    let token = gen_token_from(random_bytes(32).into_iter(), 22);
    let now = Date::now().as_millis() as i64;
    (token, now + 24 * 3600 * 1000)
}

/// `new Date(ms).toISOString()` — the one shape the shipping worker puts on the wire. `worker::Date`'s
/// `Display` is the JS `toString()` (a local-time human format), which is NOT this, so the conversion goes
/// through `js_sys::Date` rather than through a chrono dependency for one format.
fn iso_from_ms(ms: i64) -> String {
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64))
        .to_iso_string()
        .into()
}

/// `publicBase(env, url)`: `PUBLIC_BASE` when the deployment sets it, the request's own origin otherwise.
/// **The upload leg arrives through the gateway's service binding**, whose URL is not reachable by a
/// client, so minting the download URL from `request.url` alone produced a host nothing could resolve.
fn public_base(env: &Env, url: &Url) -> String {
    let configured = env
        .var("PUBLIC_BASE")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let configured = configured.trim_end_matches('/').to_string();
    if configured.is_empty() {
        format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default())
    } else {
        configured
    }
}

fn upload_key(env: &Env) -> String {
    env.var("UPLOAD_KEY")
        .map(|v| v.to_string())
        .unwrap_or_default()
}

async fn raw_upload(req: &mut Request, env: &Env, url: &Url) -> Result<Response> {
    let Some(declared_raw) = req.headers().get("content-length")? else {
        return error_json(411, "content-length required");
    };
    if declared_raw.is_empty() {
        return error_json(411, "content-length required");
    }
    let declared = js_number_header(&declared_raw);
    if !declared.is_finite() || declared < 0.0 {
        return error_json(400, "invalid content-length");
    }
    if declared > MAX_BYTES as f64 {
        return too_large();
    }
    let raw_name = url
        .query_pairs()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.to_string())
        .or_else(|| req.headers().get("x-filename").ok().flatten())
        .unwrap_or_else(|| "file".to_string());
    // **THE BASENAME RULE**: a client-supplied path must never shape the stored Content-Disposition.
    let base = raw_name
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("file")
        .to_string();
    let Some(disposition) = build_content_disposition(&base) else {
        return error_json(400, "invalid filename");
    };
    let (token, expires_at) = mint();
    let key = format!("files/{token}");
    let content_type = req
        .headers()
        .get("x-content-type")?
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bucket = env.bucket("TEMP_FILES")?;
    // **STREAMED, NOT BUFFERED** — see the header. `req.inner().body()` is the request's own stream.
    let data = match req.inner().body() {
        Some(stream) => Data::ReadableStream(stream),
        None => Data::Empty,
    };
    let stored = bucket
        .put(key.clone(), data)
        .http_metadata(HttpMetadata {
            content_type: Some(content_type),
            content_disposition: Some(disposition),
            ..Default::default()
        })
        .custom_metadata(std::collections::HashMap::from([(
            "expiresAt".to_string(),
            expires_at.to_string(),
        )]))
        .execute()
        .await;
    let stored = match stored {
        Ok(o) => o,
        Err(e) => {
            return json(502, format!(r#"{{"error":"r2 put failed: {e}"}}"#));
        }
    };
    let size = stored.as_ref().map(|o| o.size() as f64).unwrap_or(declared);
    let body = format!(
        r#"{{"token":"{token}","url":"{base}/files/{token}","size":{},"filename":"{base_name}","expiresAt":"{iso}","note":"{NOTE}"}}"#,
        size as u64,
        base = public_base(env, url),
        base_name = base,
        iso = iso_from_ms(expires_at),
    );
    json(200, body)
}

async fn multipart_upload(req: &mut Request, env: &Env, url: &Url) -> Result<Response> {
    let ct = req.headers().get("content-type")?.unwrap_or_default();
    if !ct.contains("multipart/form-data") {
        return error_json(
            400,
            "expected multipart/form-data (POST) or a raw body (PUT)",
        );
    }
    let Some(declared_raw) = req.headers().get("content-length")? else {
        return error_json(411, "content-length required");
    };
    if declared_raw.is_empty() {
        return error_json(411, "content-length required");
    }
    if js_number_header(&declared_raw) > (MAX_BYTES + CL_MARGIN) as f64 {
        return too_large();
    }
    let form = match req.form_data().await {
        Ok(f) => f,
        Err(_) => return error_json(400, "invalid multipart body"),
    };
    let Some(FormEntry::File(file)) = form.get("file") else {
        // A string field of the same name is not a file, and the shipping worker's `typeof file ===
        // "string"` check is this arm.
        return error_json(400, "no file field");
    };
    let filename = file.name();
    let content_type = file.type_();
    let bytes = match file.bytes().await {
        Ok(b) => b,
        Err(_) => return error_json(400, "invalid multipart body"),
    };
    if bytes.len() as u64 > MAX_BYTES {
        return too_large();
    }
    let Some(disposition) = build_content_disposition(&filename) else {
        return error_json(400, "invalid filename");
    };
    let (token, expires_at) = mint();
    let key = format!("files/{token}");
    let bucket = env.bucket("TEMP_FILES")?;
    let stored = bucket
        .put(key.clone(), bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(if content_type.is_empty() {
                "application/octet-stream".to_string()
            } else {
                content_type
            }),
            content_disposition: Some(disposition),
            ..Default::default()
        })
        .custom_metadata(std::collections::HashMap::from([(
            "expiresAt".to_string(),
            expires_at.to_string(),
        )]))
        .execute()
        .await;
    if let Err(e) = stored {
        // The multipart arm's failures fall to the route's catch-all in the shipping worker, which
        // answers 500 with `String(err)`.
        return error_json(500, &e.to_string());
    }
    let body = format!(
        r#"{{"token":"{token}","url":"{base}/files/{token}","size":{},"filename":"{filename}","expiresAt":"{iso}","note":"{NOTE}"}}"#,
        bytes.len(),
        base = public_base(env, url),
        iso = iso_from_ms(expires_at),
    );
    json(200, body)
}

/// `^/files/([A-Za-z0-9_-]{16,64})$`
fn claim_token(path: &str) -> Option<&str> {
    let token = path.strip_prefix("/files/")?;
    let ok = (16..=64).contains(&token.len())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(token)
}

#[event(fetch)]
async fn fetch(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    // ── Upload: POST /api/upload (multipart) | PUT /api/upload?name=<f> (raw stream) ──────────────
    if path == "/api/upload" && (method == Method::Post || method == Method::Put) {
        let auth = req.headers().get("authorization")?.unwrap_or_default();
        let key = upload_key(&env);
        let expected = format!("Bearer {key}");
        if key.is_empty() || auth.is_empty() || !safe_eq(&auth, &expected) {
            return error_json(401, "unauthorized");
        }
        if method == Method::Put {
            return raw_upload(&mut req, &env, &url).await;
        }
        return multipart_upload(&mut req, &env, &url).await;
    }

    // ── Download: GET /files/<token> — forwarded to the claim Durable Object ─────────────────────
    if method == Method::Get {
        if let Some(token) = claim_token(&path) {
            let namespace = env.durable_object("TEMP_CLAIM")?;
            let stub = match namespace.get_by_name(&format!("files/{token}")) {
                Ok(s) => s,
                Err(_) => return unavailable(),
            };
            // The internal DO credential, attached when the deployment configures one. **`set()`
            // OVERWRITES ANY CLIENT-SUPPLIED VALUE** — a caller cannot forge it — and the DO verifies it
            // only when `DO_AUTH` is set, so deploys without the secret keep working.
            let do_auth = env
                .var("DO_AUTH")
                .map(|v| v.to_string())
                .unwrap_or_default();
            let forwarded = if do_auth.is_empty() {
                req
            } else {
                let headers = Headers::new();
                for (k, v) in req.headers().entries() {
                    headers.set(&k, &v)?;
                }
                headers.set("x-do-auth", &do_auth)?;
                let mut init = RequestInit::new();
                init.with_method(method.clone()).with_headers(headers);
                match Request::new_with_init(url.as_str(), &init) {
                    Ok(r) => r,
                    Err(_) => return unavailable(),
                }
            };
            return match stub.fetch_with_request(forwarded).await {
                Ok(r) => Ok(r),
                Err(_) => unavailable(),
            };
        }
    }

    // This worker is the FILE RELAY; the download site stays on the CDN worker (ADR 0010, D6).
    error_json(404, "not found")
}

/// The 503 envelope an R2/DO outage must surface as — never an uncaught throw, which the platform
/// answers with its own HTML 500 that no device-side reader parses.
fn unavailable() -> Result<Response> {
    json(UNAVAILABLE.status, UNAVAILABLE.body.to_string())
}

/// The claim rule and the response shaping the DO will use — declared here so the compiler proves the
/// import list stays honest until the DO class lands (`src/claim_do.rs`, next step).
#[allow(dead_code)]
fn claim_decision_for(
    exists: bool,
    expires_at_raw: Option<&serde_json::Value>,
    now_ms: f64,
) -> Claim {
    decide_claim(exists, expires_at_raw, now_ms)
}
