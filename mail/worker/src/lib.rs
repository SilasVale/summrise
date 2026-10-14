//! THE MAIL WORKER'S DECISIONS — the half that compiles on the host and is tested by `cargo test`.
//!
//! WHAT THIS CRATE IS: `dsh@saisi.online` receives mail; the worker stores each message's raw bytes in R2 and
//! serves them back over one unguessable path. The design is
//! `docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md`.
//!
//! WHAT IS NOT HERE, AND WHY: no MIME parsing. Workers Free gives **10 ms of CPU per request**, and
//! Cloudflare's own docs warn that a mail-handling worker can exceed it; parsing a multi-megabyte message
//! costs far more. So the worker copies bytes and reads the headers the runtime hands it for free, and the
//! agent's own box does the parsing. This file is small on purpose.
//!
//! THE READ PATH'S WHOLE PROTECTION IS ONE PATH SEGMENT, so the functions that decide whether a request is
//! allowed to see anything live here — on the host, where they are tested — rather than inside the wasm-only
//! handler where nothing could reach them.

use sha2::{Digest, Sha256};

// THE OTHER HALF. `#[event(fetch)]` and `#[event(email)]` expand to nothing off wasm32, so this module exists
// only for the target the worker actually runs on; the entry point is checked by
// `cargo clippy --target wasm32-unknown-unknown` and driven for real by `wrangler dev`.
#[cfg(target_arch = "wasm32")]
pub mod worker;

/// The length of an id in hex characters — 128 bits of randomness, which is what makes a message link
/// unguessable even if one leaks.
pub const ID_HEX_LEN: usize = 32;

/// How many index rows the read path answers with when the caller does not say.
pub const DEFAULT_INDEX_LIMIT: usize = 20;

/// The most index rows a caller may ask for.
///
/// **40 AND NOT 50, BECAUSE THE SUBREQUEST BUDGET IS 50.** A listing costs one subrequest and each row costs
/// one R2 `get`, so 50 rows would be 51 — one over the Workers Free ceiling, where the failure is a dead read
/// path rather than a smaller answer. 40 leaves ten for the listing and whatever comes next.
pub const MAX_INDEX_LIMIT: usize = 40;

/// What a request to the read host asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Index { limit: usize },
    Message { id: String },
}

/// One message as the index shows it.
#[derive(Debug, PartialEq, Eq)]
pub struct IndexRow {
    pub id: String,
    pub uploaded: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub bytes: u64,
    /// The object key the raw message lives under, stored IN the row because the date partition is the
    /// worker's receive time and cannot be rebuilt from anything in the message.
    pub raw: String,
}

/// The fields the email handler writes for one message.
pub struct NewRow<'a> {
    pub id: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub subject: &'a str,
    pub date: &'a str,
    pub bytes: u64,
    pub raw: &'a str,
}

/// `/s/<secret>/` → the index; `/s/<secret>/m/<id>` → one message; everything else → `None` (a 404).
///
/// REFUSES RATHER THAN FALLS OPEN, in two places that matter: an empty secret refuses every path (an unset
/// `MAIL_PATH_SECRET` is a configuration error, and without this guard `/s//` would answer to anyone), and the
/// query string is read for exactly one parameter, `n`, whose value is clamped to [`MAX_INDEX_LIMIT`].
pub fn parse_route(path: &str, secret: &str) -> Option<Route> {
    if secret.is_empty() {
        return None;
    }
    let (path_only, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    };
    let rest = path_only.strip_prefix("/s/")?;
    let (candidate, tail) = rest.split_once('/')?;
    if !secret_eq(candidate, secret) {
        return None;
    }
    if tail.is_empty() {
        return Some(Route::Index {
            limit: index_limit(query),
        });
    }
    let id = tail.strip_prefix("m/")?;
    if !is_id(id) {
        return None;
    }
    Some(Route::Message { id: id.to_string() })
}

/// The one parameter the index route reads. Anything else — an unknown name, a non-number, zero — is the
/// default rather than an error, because a read path that argues with its caller is a read path the agent
/// cannot use.
fn index_limit(query: &str) -> usize {
    for part in query.split('&') {
        if let Some(value) = part.strip_prefix("n=") {
            return match value.parse::<usize>() {
                Ok(n) if n >= 1 => n.min(MAX_INDEX_LIMIT),
                _ => DEFAULT_INDEX_LIMIT,
            };
        }
    }
    DEFAULT_INDEX_LIMIT
}

/// Compare without an early return over the bytes, so the time taken does not say where the first difference
/// is. Unequal lengths answer immediately — the length is not the secret, and pretending otherwise would cost
/// more than it hides.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// An id is exactly [`ID_HEX_LEN`] LOWERCASE hex characters — lowercase because the same 16 bytes must always
/// produce the same key.
fn is_id(id: &str) -> bool {
    id.len() == ID_HEX_LEN
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// 16 random bytes → 32 hex characters.
pub fn new_id(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(ID_HEX_LEN);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// A message's id: 128 bits derived from the path secret and what the runtime hands us for free.
///
/// **WHY DERIVED RATHER THAN DRAWN, MEASURED**: `worker` 0.8.7 exposes no cryptographically secure RNG (its
/// `crypto` module carries only the digest stream, `js-sys` has no `crypto` module, and the only drawing
/// primitive left would be `Math::random`, which is not one). Deriving is not a consolation prize here:
/// * an attacker without the path secret cannot compute an id, because the secret is an input to the hash —
///   and an attacker WITH the secret can already read everything;
/// * a sender's retry produces the same id, so a retried message overwrites itself instead of arriving twice;
/// * it is a pure function, so `cargo test` covers it instead of a comment claiming it works.
///
/// The `0x00` after every field is what keeps `("ab", "c")` and `("a", "bc")` different messages rather than
/// the same bytes; no field can carry a NUL of its own.
pub fn new_id_from(secret: &str, millis: u64, size: u64, from: &str, to: &str) -> String {
    let mut hasher = Sha256::new();
    for field in [
        secret.as_bytes(),
        &millis.to_be_bytes(),
        &size.to_be_bytes(),
        from.as_bytes(),
        to.as_bytes(),
    ] {
        hasher.update(field);
        hasher.update([0u8]);
    }
    let digest = hasher.finalize();
    let mut head = [0u8; 16];
    head.copy_from_slice(&digest[..16]);
    new_id(&head)
}

/// The receive-time partition, from the worker's own clock — never from the message.
///
/// A MESSAGE'S `Date:` HEADER IS ATTACKER-CONTROLLED TEXT and must never reach an object key. That is a
/// property of this signature rather than of a check inside it: the input is a NUMBER (milliseconds since the
/// epoch, which is what `worker::Date::now().as_millis()` returns), so there is no string a sender could write
/// that becomes a path.
///
/// **WHY MILLISECONDS AND NOT AN ISO STRING, MEASURED IN `worker` 0.8.7**: its `Date` implements `Display` as
/// JavaScript's `Date.prototype.toString()` — `"Tue Oct 14 2026 06:39:45 GMT+0000 (Coordinated Universal
/// Time)"` — not ISO 8601, so a handler that formatted the clock with `{}` would have written every message
/// into a partition called nothing this function would recognise. The conversion is therefore pure Rust, which
/// also means it runs in `cargo test` on the host.
pub fn date_partition_from_millis(ms: u64) -> String {
    let (year, month, day, _, _, _) = parts_from_millis(ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The same clock, one granularity finer: `YYYY-MM-DDTHH:MM:SSZ`, which is what the index prints and what
/// sorts chronologically as a string.
pub fn stamp_from_millis(ms: u64) -> String {
    let (year, month, day, hour, minute, second) = parts_from_millis(ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Milliseconds since the epoch → the UTC civil date and time. Howard Hinnant's `civil_from_days`, which is
/// exact for every value a `u64` of milliseconds can hold and needs no calendar library.
fn parts_from_millis(ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let seconds = (ms / 1_000) as i64;
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let mp = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { year + 1 } else { year };

    (
        year,
        month,
        day,
        (second_of_day / 3_600) as u32,
        ((second_of_day % 3_600) / 60) as u32,
        (second_of_day % 60) as u32,
    )
}

/// Where one message's bytes live. The date partition is in the key so the bucket listing groups a day's mail
/// together, and the id is in it so two messages in the same second cannot overwrite each other.
pub fn raw_key(date: &str, id: &str) -> String {
    format!("mail/raw/{date}/{id}.eml")
}

/// Where one message's index row lives.
pub fn idx_key(id: &str) -> String {
    format!("mail/idx/{id}.json")
}

/// The index row as it is stored. The id is NOT in here: it is the object's name, and one fact in two places
/// is one fact that can disagree with itself.
pub fn index_json(row: &NewRow<'_>) -> String {
    serde_json::json!({
        "from": row.from,
        "to": row.to,
        "subject": row.subject,
        "date": row.date,
        "bytes": row.bytes,
        "raw": row.raw,
    })
    .to_string()
}

/// Read a stored row back. `id` and `uploaded` come from the bucket listing rather than from the object, so a
/// row missing every field still answers with those two — and a message whose sender left `Subject:` off is a
/// message the agent can still find.
pub fn parse_index_row(json: &str, id: &str, uploaded: &str) -> Option<IndexRow> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Some(IndexRow {
        id: id.to_string(),
        uploaded: uploaded.to_string(),
        from: text("from"),
        to: text("to"),
        subject: text("subject"),
        date: text("date"),
        bytes: value.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0),
        raw: text("raw"),
    })
}

/// The whole read surface the agent gets: one line per message, newest first, with the id it needs to fetch
/// the message itself. Empty fields are named rather than left blank, so a missing subject cannot read as a
/// broken row.
pub fn render_index(rows: &[IndexRow]) -> String {
    let mut out = format!("mail index: {} message(s), newest first\n", rows.len());
    for row in rows {
        let from = if row.from.is_empty() {
            "(unknown sender)"
        } else {
            row.from.as_str()
        };
        let subject = if row.subject.is_empty() {
            "(no subject)"
        } else {
            row.subject.as_str()
        };
        out.push_str(&format!(
            "{}  {}  {}  {} B  {}\n",
            row.uploaded, from, subject, row.bytes, row.id
        ));
    }
    out
}
