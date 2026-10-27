//! Operator sessions.
//!
//! The operator presents one secret, once: the 32-byte token from `--operator-token-file`. The
//! console answers with a session token it generates and expires after thirty days.
//!
//! The session outlives a console restart: `<data>/sessions.json` holds each session's token HASH,
//! CSRF value and expiry, so a deploy does not sign the operator out (measured: every restart used
//! to, and a day of deploys meant a day of sign-ins). The token itself is never written, which still
//! means a copy of the file is not a credential. Both comparisons — the secret and the session —
//! walk the full 32 bytes, so neither tells a caller how many leading bytes it guessed.

use std::path::Path;

use axum::http::{header, HeaderMap};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::hex;

/// How long a session token is valid.
///
/// Thirty days, not one: the console is a single operator's page behind Cloudflare Access, and a
/// day-long session meant signing in again every morning — the one thing an operator asked about
/// twice. The session is a hash on disk, `HttpOnly`, `SameSite=Lax`, and `Secure` whenever the
/// request arrived over TLS, so the longer life widens nothing an attacker could not already reach
/// with the operator token itself.
pub(crate) const SESSION_LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// One operator session. The token is not kept — only its hash — so a restart can validate a cookie
/// without the file being a credential.
#[derive(Clone, Debug)]
pub(crate) struct Session {
    /// SHA-256 of the token the caller presents, in bytes.
    pub(crate) token_hash: [u8; 32],
    /// When the session stops being valid, in Unix milliseconds.
    pub(crate) expires_at_ms: u64,
    /// The value a server-rendered form echoes back. It lives in the session (server memory),
    /// never in the cookie, so a cross-site request cannot read it.
    pub(crate) csrf: [u8; 32],
}

/// A fresh session token from the OS CSPRNG.
pub(crate) fn new_session_token() -> [u8; 32] {
    random_32()
}

/// 32 bytes from the OS CSPRNG, for every secret this service mints: a session token, an
/// enrollment code, a device token.
pub(crate) fn random_32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// The bearer token from the request headers, decoded to its 32 bytes.
pub(crate) fn bearer_secret(headers: &HeaderMap) -> Option<[u8; 32]> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    hex::decode_32(value.strip_prefix("Bearer ")?)
}

/// Whether two byte strings are equal without returning early on the first difference.
pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= left ^ right;
    }

    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_secrets_match_and_one_changed_byte_does_not() {
        let secret = [0x5a; 32];
        assert!(constant_time_eq(&secret, &secret));

        let mut other = secret;
        other[31] ^= 0x01;
        assert!(!constant_time_eq(&secret, &other));

        assert!(!constant_time_eq(&secret, &secret[..31]));
        assert!(constant_time_eq(&[], &[]));
    }

    #[test]
    fn two_sessions_are_different_tokens() {
        assert_ne!(new_session_token(), new_session_token());
    }
}

/// The file a console restart reads sessions back from.
pub(crate) fn sessions_path(data: &Path) -> std::path::PathBuf {
    data.join("sessions.json")
}

/// One session as the file keeps it.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    token_hash: String,
    csrf: String,
    expires_at_ms: u64,
}

/// Read the sessions a previous run left, dropping anything malformed or expired.
pub(crate) fn load_sessions(data: &Path, now_ms: u64) -> Vec<Session> {
    let Ok(text) = std::fs::read_to_string(sessions_path(data)) else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<StoredSession>>(&text) else {
        eprintln!("summrise-console: the session file is not readable JSON; starting signed out");
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            Some(Session {
                token_hash: hex::decode_32(&row.token_hash)?,
                csrf: hex::decode_32(&row.csrf)?,
                expires_at_ms: row.expires_at_ms,
            })
        })
        .filter(|session| session.expires_at_ms > now_ms)
        .collect()
}

/// Write the sessions, atomically, so a crash mid-write cannot leave a half file behind.
pub(crate) fn save_sessions(data: &Path, sessions: &[Session]) -> std::io::Result<()> {
    let rows: Vec<StoredSession> = sessions
        .iter()
        .map(|session| StoredSession {
            token_hash: hex::encode(&session.token_hash),
            csrf: hex::encode(&session.csrf),
            expires_at_ms: session.expires_at_ms,
        })
        .collect();
    let text = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_owned());
    let path = sessions_path(data);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}
