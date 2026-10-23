//! Operator sessions.
//!
//! The operator presents one secret, once: the 32-byte token from `--operator-token-file`. The
//! console answers with a session token it generates, keeps in memory, and expires after 24 hours.
//! Both comparisons — the secret and the session — walk the full 32 bytes, so neither tells a caller
//! how many leading bytes it guessed.

use axum::http::{header, HeaderMap};
use rand_core::{OsRng, RngCore};

use crate::hex;

/// How long a session token is valid.
pub(crate) const SESSION_LIFETIME_MS: u64 = 24 * 60 * 60 * 1000;

/// One in-memory session. The token is never persisted: a console restart ends every session, which
/// is the right default for a credential that can mint grants.
#[derive(Clone, Debug)]
pub(crate) struct Session {
    /// The 32 random bytes the caller presents as a bearer token, in hex.
    pub(crate) token: [u8; 32],
    /// When the session stops being valid, in Unix milliseconds.
    pub(crate) expires_at_ms: u64,
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
