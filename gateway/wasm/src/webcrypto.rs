//! THE PLATFORM HALF OF THE SESSION GATE: WebCrypto, reached through the same one-shot reflection the crate
//! already uses for `crypto.subtle.digest` (`vision.rs::sha256_hex16`).
//!
//! WHY THIS IS A PLATFORM CALL RATHER THAN A DECISION, said in one line: HMAC-SHA256 and PBKDF2 are the RUNTIME's
//! primitives — the shipping TypeScript calls `crypto.subtle.importKey`/`sign`/`deriveBits`, and re-implementing
//! them in Rust would be a second implementation of the same standard, compared against the first only by luck.
//! What is DECIDED (which key, which payload, whether the result matches a token, what an expired token means)
//! lives in `session_gate.rs`, which calls these two functions and nothing else.
//!
//! **THE ARGUMENT SHAPES ARE WEB STANDARD, NOT A CONVENIENCE API**: `importKey("raw", keyData, algorithm,
//! extractable, keyUsages)`, `sign(algorithm, key, data)`, `deriveBits(algorithm, baseKey, length)`. The
//! algorithm objects are built here because `{name: "HMAC", hash: "SHA-256"}` is not expressible as a JSON
//! string that `Reflect::get` would accept as an object.
//!
//! EVERY FUNCTION ANSWERS `Option`: a missing `crypto`, a rejected key or a failed promise is `None`, and every
//! caller treats `None` as "not verified" — the same fail-closed arm the source's try/catch has.

use worker::js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;

fn global(name: &str) -> Option<JsValue> {
    Reflect::get(&worker::js_sys::global(), &JsValue::from_str(name)).ok()
}

fn get(target: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(target, &JsValue::from_str(name)).ok()
}

/// `fn.apply(this, args)` with a built argument array — `js_sys::Function` exposes `call0`..`call3`, and WebCrypto's
/// entry points take four and five arguments.
fn apply(function: &JsValue, this: &JsValue, args: &[JsValue]) -> Option<JsValue> {
    let function: Function = function.clone().dyn_into().ok()?;
    let list = Array::new();
    for arg in args {
        list.push(arg);
    }
    Reflect::apply(&function, this, &list).ok()
}

/// AWebCrypto promise → the bytes it resolved with.
async fn resolve_bytes(value: JsValue) -> Option<Vec<u8>> {
    let promise: Promise = value.dyn_into().ok()?;
    let buffer = JsFuture::from(promise).await.ok()?;
    Some(Uint8Array::new(&buffer).to_vec())
}

/// `crypto.subtle`, or `None` where there is none.
fn subtle() -> Option<JsValue> {
    get(&global("crypto")?, "subtle")
}

fn object(pairs: &[(&str, JsValue)]) -> Option<JsValue> {
    let obj = Object::new();
    for (k, v) in pairs {
        Reflect::set(&obj, &JsValue::from_str(k), v).ok()?;
    }
    Some(obj.into())
}

/// `crypto.subtle.importKey("raw", bytes, algorithm, false, usages)` → the CryptoKey.
async fn import_raw_key(algorithm: JsValue, bytes: &[u8], usages: &[&str]) -> Option<JsValue> {
    let subtle = subtle()?;
    let usage_list = Array::new();
    for u in usages {
        usage_list.push(&JsValue::from_str(u));
    }
    let promise = apply(
        &get(&subtle, "importKey")?,
        &subtle,
        &[
            JsValue::from_str("raw"),
            Uint8Array::from(bytes).into(),
            algorithm,
            JsValue::from_bool(false),
            usage_list.into(),
        ],
    )?;
    JsFuture::from(promise.dyn_into::<Promise>().ok()?)
        .await
        .ok()
}

/// `crypto.subtle.sign("HMAC", key, data)` for the HMAC-SHA256 key `secret` — the primitive
/// `verifySessionToken` compares against.
pub async fn hmac_sha256(secret: &str, message: &str) -> Option<Vec<u8>> {
    let algorithm = object(&[
        ("name", JsValue::from_str("HMAC")),
        ("hash", JsValue::from_str("SHA-256")),
    ])?;
    let key = import_raw_key(algorithm, secret.as_bytes(), &["sign"]).await?;
    let subtle = subtle()?;
    let signature = apply(
        &get(&subtle, "sign")?,
        &subtle,
        &[
            JsValue::from_str("HMAC"),
            key,
            Uint8Array::from(message.as_bytes()).into(),
        ],
    )?;
    resolve_bytes(signature).await
}

/// `crypto.subtle.deriveBits({name:"PBKDF2", salt, iterations, hash:"SHA-256"}, key, 256)` — the derivation
/// `auth.ts`'s `hashPassword` performs (100,000 iterations, a per-user salt), reached here only for
/// `getAdminPassword`'s one-time `legacy:` migration from the `ADMIN_PASSWORD` Worker secret.
pub async fn pbkdf2_sha256(password: &str, salt: &str, iterations: u32) -> Option<Vec<u8>> {
    let key = import_raw_key(
        JsValue::from_str("PBKDF2"),
        password.as_bytes(),
        &["deriveBits"],
    )
    .await?;
    let subtle = subtle()?;
    let algorithm = object(&[
        ("name", JsValue::from_str("PBKDF2")),
        ("salt", Uint8Array::from(salt.as_bytes()).into()),
        ("iterations", JsValue::from_f64(iterations as f64)),
        ("hash", JsValue::from_str("SHA-256")),
    ])?;
    let bits = apply(
        &get(&subtle, "deriveBits")?,
        &subtle,
        &[algorithm, key, JsValue::from_f64(256.0)],
    )?;
    resolve_bytes(bits).await
}

/// `randomHex(bytes)` — the ONE randomness the device routes need, and it goes through the platform for the
/// reason the source does: `crypto.getRandomValues` is the CSPRNG. (A panel grant that a caller could predict is
/// a panel token; a registration key that a caller could predict is an install.)
pub fn random_hex(bytes: usize) -> String {
    let filled = (|| -> Option<String> {
        let crypto = global("crypto")?;
        let array = Uint8Array::new_with_length(bytes as u32);
        apply(
            &get(&crypto, "getRandomValues")?,
            &crypto,
            &[array.clone().into()],
        )?;
        let mut out = String::with_capacity(bytes * 2);
        for i in 0..array.length() {
            out.push_str(&format!("{:02x}", array.get_index(i)));
        }
        Some(out)
    })();
    // A runtime without `crypto.getRandomValues` cannot mint a credential; answering an empty string would be a
    // PREDICTABLE one. The empty answer fails every shape test downstream (`/^[0-9a-f]{32}$/`, or the key is
    // simply unusable), which is the fail-closed arm.
    filled.unwrap_or_default()
}

/// The hex `hashPassword` returns — `[...bytes].map(x => x.toString(16).padStart(2, "0")).join("")`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `hex` is the source's `toString(16).padStart(2, "0")` — and the padding is the whole test: `0x0a` is
    /// `"0a"`, not `"a"`, and a leading zero that is dropped produces a HASH THAT NEVER MATCHES again.
    #[test]
    fn hex_pads_every_byte() {
        assert_eq!(hex(&[0x0a, 0x00, 0xff]), "0a00ff");
        assert_eq!(hex(&[]), "");
    }
}
