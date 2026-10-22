//! The release document, the channel rule and the signer (design §4.4, §6).
//!
//! `summrise-release` is the distribution half of the device: [`document`] is what a device fetches
//! and verifies, and [`sign`] is the offline tool that produces it. The document carries two
//! signatures: the policy signature covers every policy fact the device acts on (version, channel,
//! digest, artifact URL, minimum version and issue time), and the artifact signature is the older
//! `"{version}\n{channel}\n{sha256}"` message the stager and the launcher already verify. A
//! successful [`verify`] therefore settles `minVersion`, `artifactUrl` and `issuedAtMs` before any
//! of them is used.

#![forbid(unsafe_code)]

pub mod document;
pub mod revocations;
pub mod sign;
pub mod url;

pub use ::url::Url;
pub use document::{
    artifact_message, canonical_message, select, verify, verify_at, version_is_plain, Channel,
    ReleaseDocument, ReleaseError, Selection, VersionField, MAX_ISSUED_AGE_MS,
    MAX_ISSUED_FUTURE_MS,
};
pub use revocations::{
    canonical_message as revocations_message, revoke_file, RevocationError, Revocations,
    RevokeError, RevokeRequest, RevokedRelease, MAX_REVOCATIONS_AGE_MS,
};
pub use sign::{
    hex_encode, now_ms, read_seed, seed_from_hex, sha256_hex, sign_document, sign_file,
    sign_sidecar, ArtifactSidecar, SignError, SignRequest,
};
pub use url::{parse_release_url, UrlError};
