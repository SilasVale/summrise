//! THE FILE RELAY'S PURE DECISIONS, IN RUST — the half of `relay/src/index.js` and `relay/src/claim.js`
//! that is a FUNCTION OF ITS INPUTS rather than of R2, the Durable Object or the network.
//!
//! ── WHY THIS SPLIT, AND WHY IT COMES FIRST ──────────────────────────────────────────────────────
//!
//! The worker is 558 lines of JavaScript across two files. Two thirds of it is I/O: an R2 put with
//! metadata, an R2 get-then-delete inside a Durable Object, a streamed body, a `crypto.subtle` digest.
//! **THE REST IS DECISIONS** — a `Content-Disposition` header built from a client-supplied filename, a
//! four-branch one-time-claim rule, a token generator, a digest regex — and decisions are what a corpus
//! can pin. So the port lands in the order this repository uses for every worker: the decisions first,
//! with a corpus recorded from the shipping implementation, and the entry point second
//! (`worker.rs` + `#[event(fetch)]`, next round).
//!
//! ── THE CORPUS, AND WHO RECORDED IT ─────────────────────────────────────────────────────────────
//!
//! `pure-corpus.json` was recorded by `record-pure.mjs` — a script that IMPORTS THE SHIPPING
//! JAVASCRIPT and asks it these same questions (31 filenames, 220 claim states, 10 digests, the token
//! shape). `tests/pure.rs` compares this file's answers against the functions below. **THE ORACLE IS
//! THE IMPLEMENTATION THAT IS STILL SERVING `agent.saisi.online/files/*`**, which is the strongest
//! form this comparison can take, and it retires with the JavaScript when the cutover deletes it.
//!
//! ── THE THREE TRAPS THE CORPUS EXISTS FOR ───────────────────────────────────────────────────────
//!
//! **① `trim()` IS NOT THE SAME FUNCTION IN THE TWO LANGUAGES.** JavaScript trims U+FEFF and U+00A0;
//! Rust does not. Rust trims U+0085 (NEL); JavaScript does not. All three code points are in the
//! corpus, and a port that reached for `str::trim()` fails on two of them — measured, not feared: the
//! recorded answers carry `nel.txt` through to `filename*` while `bom.txt` and `nbsp.txt` come out
//! trimmed.
//!
//! **② `Number(x)` IS A COERCION, NOT A CAST.** The claim rule asks `Number.isFinite(Number(raw))`, so
//! `"0x10"`, `" 42 "`, `"1e3"`, `[]`, `[1]`, `true` and `"Infinity"` all have defined answers, and
//! `{}`/`"abc"` are `NaN` — which the rule turns into "expired" (fail closed) rather than "serve".
//!
//! **③ `encodeURIComponent` IS NOT PERCENT-ENCODING.** `!~*'()` and `-_.` stay literal; everything
//! else becomes uppercase `%XX` per UTF-8 byte. A port that escaped the unreserved set differently
//! would produce a header browsers decode to a different filename.
//!
//! MUTATION: make `js_trim` call `str::trim()` instead.
//! RESULT:   `tests/pure.rs` fails on `"\u{85}nel.txt"` — the shipping JavaScript keeps the NEL (it
//!           reaches `filename*=UTF-8''%C2%85nel.txt`) and the port would have trimmed it away. (And on
//!           `"\u{feff}bom.txt"` in the other direction: `str::trim()` does not remove the BOM, so the
//!           ascii fallback would differ too.)
//!
//! ── AND TWO DEFECTS THIS PORT FOUND IN ITSELF, WHICH IS WHAT THE CORPUS IS FOR ─────────────────
//!
//! **① `char::is_control()` IS NOT `[\u0000-\u001f\u007f]`.** The first version filtered the filename with
//! Rust's `is_control`, which also covers C1 (U+0080–U+009F) — so U+0085 was removed where the shipping
//! worker keeps it, and the header came out `filename="nel.txt"` instead of carrying
//! `filename*=UTF-8''%C2%85nel.txt`. `tests/pure.rs` failed on exactly that case.
//!
//! **② `str::parse::<f64>()` IS NOT `Number()`.** Rust's parser accepts `inf`, `infinity` and `NaN` in
//! any case; JavaScript's `Number` accepts only the exact word `Infinity`. `Number("infinity")` is NaN
//! in the shipping worker and was `inf` in the first version of `js_number` — **a corrupt deadline
//! becoming an unbounded download**, which is the failure the claim rule's fail-closed branch exists to
//! prevent. The grammar (`is_str_decimal_literal`) is what fixes it.
//!
//! MUTATION (the claim rule): return `Serve` where `!ts.is_finite()`.
//! RESULT:   the corpus's `"abc"`, `{}`, `"NaN"` rows fail — they are `expired` in the shipping worker,
//!           and that branch is the difference between a corrupt deadline and an unbounded download.

use serde_json::Value;

/// The claim Durable Object — the class the worker's download route forwards into.
#[cfg(target_arch = "wasm32")]
pub mod claim_do;
/// **THE ENTRY POINT, AND IT IS wasm32-ONLY ON PURPOSE.** `#[event(fetch)]` expands to nothing on the
/// host, so the host build compiles none of `worker.rs` — which is exactly why the CI job carries a
/// `cargo clippy --target wasm32-unknown-unknown` step for this crate, and why the differential drives
/// `worker-build`'s output rather than a host binary.
#[cfg(target_arch = "wasm32")]
pub mod envelopes;
#[cfg(target_arch = "wasm32")]
pub mod worker;

/// `^/files/([A-Za-z0-9_-]{16,64})$` — **ONE DEFINITION, TWO CALLERS**: the worker decides whether to
/// forward a download, and the Durable Object decides whether the request it received names a claim. They
/// must agree, and the first version of this port had the pattern written out twice.
pub fn claim_token(path: &str) -> Option<&str> {
    let token = path.strip_prefix("/files/")?;
    let ok = (16..=64).contains(&token.len())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(token)
}

/// The alphabet `genToken` draws from, in the shipping implementation's order.
pub const TOKEN_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// What the one-time-claim rule decides. The strings are the shipping implementation's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// The object is not in the bucket: 404, already downloaded.
    Gone,
    /// The deadline is missing or empty — legacy uploads predate it, and it fails OPEN for compat.
    Serve,
    /// The deadline is past, or present and unparseable — 410, and the object is deleted.
    Expired,
}

/// The 503 envelope an R2/DO outage must surface as (never an uncaught throw, which the platform
/// answers with its own HTML 500 that no device-side reader parses).
pub struct Envelope {
    pub status: u16,
    pub content_type: &'static str,
    pub body: &'static str,
}

pub const UNAVAILABLE: Envelope = Envelope {
    status: 503,
    content_type: "application/json",
    body: r#"{"error":"temporarily unavailable"}"#,
};

/// **JAVASCRIPT'S `String.prototype.trim`, WHICH IS NOT `str::trim`.**
///
/// The set is WhiteSpace ∪ LineTerminator from the ECMAScript spec: TAB, LF, VT, FF, CR, SP, NBSP,
/// U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF. **U+0085 (NEL) IS NOT IN
/// IT** — it is the one code point where Rust's `char::is_whitespace` and JavaScript's `trim` disagree
/// in the direction that changes a header, and the corpus carries it.
pub fn js_trim(s: &str) -> &str {
    let is_js_space = |c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    };
    s.trim_matches(is_js_space)
}

/// **`encodeURIComponent`, EXACTLY** — the unreserved set stays literal and everything else is
/// uppercase `%XX` per UTF-8 byte. (Not `urlencoding`, not `percent-encoding`'s default set: both
/// escape characters this one leaves alone, and the header is read by browsers that decode strictly.)
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The hardened `Content-Disposition` for a client-supplied filename, or `None` when nothing survives
/// (the caller answers 400 — **an illegal name must never reach the R2 put as a forged header**).
pub fn build_content_disposition(raw: &str) -> Option<String> {
    // The header-split defence: quotes, backslashes and every C0 control plus DEL are removed BEFORE
    // the trim, which is the order the shipping implementation uses. **C0 AND DEL ONLY — NOT
    // `char::is_control()`**, which also covers C1 (U+0080–U+009F) and would eat U+0085: the shipping
    // worker keeps the NEL, carries it into `filename*`, and the corpus recorded that. Measured on the
    // first run of `tests/pure.rs`, which is what that case is for.
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '"' | '\\') && !matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'))
        .collect();
    let cleaned = js_trim(&cleaned);
    if cleaned.is_empty() {
        return None;
    }
    let ascii: String = cleaned.chars().filter(|c| matches!(c, ' '..='~')).collect();
    let ascii = js_trim(&ascii);
    let ascii = if ascii.is_empty() {
        "download.bin"
    } else {
        ascii
    };
    if ascii == cleaned {
        return Some(format!("attachment; filename=\"{ascii}\""));
    }
    Some(format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        encode_uri_component(cleaned)
    ))
}

/// **`Number(x)`, the parts a claim deadline can meet.** `None` is JavaScript's `undefined`.
///
/// A string is trimmed by the JS rule, then: empty → 0, `0x`/`0b`/`0o` prefixes → integer, `Infinity`
/// with an optional sign → infinite, anything else → the decimal parse, and a failure → `NaN`. An array
/// coerces by its contents (`[]` → 0, `[1]` → 1, longer → `NaN`), a boolean to 0/1, an object to `NaN`.
pub fn js_number(v: Option<&Value>) -> f64 {
    match v {
        None | Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Array(a)) => match a.len() {
            0 => 0.0,
            1 => js_number(Some(&a[0])),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
        Some(Value::String(s)) => js_number_from_str(s),
    }
}

fn js_number_from_str(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    // **THE RADIX PREFIXES TAKE NO SIGN** — `Number("-0x10")` is NaN in JavaScript, and a port that
    // accepted it would turn a corrupt deadline into a valid one.
    if t == body {
        if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            return i64::from_str_radix(hex, 16)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN);
        }
        if let Some(bin) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
            return i64::from_str_radix(bin, 2)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN);
        }
        if let Some(oct) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
            return i64::from_str_radix(oct, 8)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN);
        }
    }
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // **`body.parse::<f64>()` IS NOT `Number(body)`, AND THIS IS THE SECOND TRAP IN THE COERCION.**
    // Rust's parser accepts `inf`, `infinity` and `NaN` in ANY case; JavaScript's `Number` accepts only
    // the exact word `Infinity`, and everything else must match StrDecimalLiteral. Measured: the first
    // run of `tests/pure.rs` failed on `Number("infinity")`, which JavaScript answers NaN and a bare
    // `parse()` answered `inf` — a corrupt deadline would have become an unbounded one.
    if !is_str_decimal_literal(body) {
        return f64::NAN;
    }
    sign * body.parse::<f64>().unwrap_or(f64::NAN)
}

/// ECMAScript's `StrDecimalLiteral`: `digits[.digits][e[±]digits]` or `.digits[e…]` — no sign (the
/// caller strips it), no whitespace, no words. The parse after it is safe because the shape is known.
fn is_str_decimal_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let mut i = 0;
    let mut seen = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        seen += digits(&mut i);
    }
    if seen == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// The one-time-claim rule, four branches, exactly as `claim.js` argues them:
/// missing → gone; no deadline (or empty) → serve; past or unparseable → expired; otherwise serve.
/// **The `ts < now` comparison is strict**: a deadline exactly equal to now serves.
pub fn decide_claim(exists: bool, expires_at_raw: Option<&Value>, now_ms: f64) -> Claim {
    if !exists {
        return Claim::Gone;
    }
    match expires_at_raw {
        None | Some(Value::Null) => return Claim::Serve,
        Some(Value::String(s)) if s.is_empty() => return Claim::Serve,
        _ => {}
    }
    let ts = js_number(expires_at_raw);
    if !ts.is_finite() {
        return Claim::Expired;
    }
    if ts < now_ms {
        return Claim::Expired;
    }
    Claim::Serve
}

/// `SHA256_RE = /^[0-9a-f]{64}$/i` — the anchored, case-insensitive digest shape.
pub fn sha256_matches(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// **THE TOKEN GENERATOR'S DECISION, SEPARATED FROM ITS RANDOMNESS.**
///
/// The shipping `genToken` draws 32 bytes at a time from `crypto.getRandomValues` and keeps a byte only
/// when it is below 248 — the largest multiple of 62 under 256 — because `byte % 62` would overweight
/// the first eight symbols (A–H). **THE REJECTION SAMPLING IS THE DECISION**; the bytes are I/O, so they
/// arrive as an iterator here and the shell passes `crypto.getRandomValues`. A port that used `%62`
/// directly would still produce 22 characters from the right alphabet, which is why the distribution is
/// a property this crate tests rather than something the corpus could record.
pub fn gen_token_from<I: Iterator<Item = u8>>(bytes: I, len: usize) -> String {
    const RANGE: u8 = 248; // 256 - (256 % 62)
    let mut out = String::with_capacity(len);
    for b in bytes {
        if out.len() >= len {
            break;
        }
        if b < RANGE {
            let idx = (b as usize) % TOKEN_CHARS.len();
            out.push(TOKEN_CHARS.as_bytes()[idx] as char);
        }
    }
    out
}
