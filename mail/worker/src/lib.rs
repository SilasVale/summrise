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

/// The length of an id in hex characters — 128 bits of randomness, which is what makes a message link
/// unguessable even if one leaks.
pub const ID_HEX_LEN: usize = 32;

/// How many index rows the read path answers with when the caller does not say.
pub const DEFAULT_INDEX_LIMIT: usize = 20;

/// The most index rows a caller may ask for. Each row costs one R2 `get`, and Workers Free allows 50
/// subrequests per invocation, so this has to stay below that with room for the listing itself.
pub const MAX_INDEX_LIMIT: usize = 50;

/// The partition used when the clock's value is not the ISO 8601 shape this worker expects. It is a word
/// rather than an empty string because an empty partition would put objects at `mail/raw//<id>.eml`.
const UNDATED: &str = "undated";

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

/// The receive-time partition, from the worker's own clock — never from the message.
///
/// A MESSAGE'S `Date:` HEADER IS ATTACKER-CONTROLLED TEXT and must never reach an object key, which is why the
/// only shape accepted here is the ISO 8601 one [`worker::Date`] produces. An RFC 2822 header, a bare date, a
/// relative path: all of them answer [`UNDATED`], which is a key nobody can steer.
pub fn date_partition(iso: &str) -> String {
    let b = iso.as_bytes();
    let iso_shaped = b.len() >= 11
        && b[10] == b'T'
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..10]
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    // The slice is taken only after the first ten bytes are known to be ASCII, so this cannot split a
    // multi-byte character.
    if iso_shaped {
        iso[..10].to_string()
    } else {
        UNDATED.to_string()
    }
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
