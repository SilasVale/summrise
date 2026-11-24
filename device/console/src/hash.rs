//! The one digest the console stores: SHA-256, lowercase hex.
//!
//! The device registry, the enrollment codes and the permanent API tokens all keep a hash instead
//! of a credential; one function keeps the three the same shape so no caller has to remember which
//! hash the file holds.

use sha2::{Digest, Sha256};

/// SHA-256 of `bytes`, lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    crate::hex::encode(&Sha256::digest(bytes))
}
