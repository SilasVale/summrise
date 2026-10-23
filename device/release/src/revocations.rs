//! The signed revocation list: a release withdrawn after it shipped (design §4.4, §6).
//!
//! A release document is signed when it is published, and its signature stays valid for 180 days.
//! Withdrawing a release therefore needs a second signed fact, not an edited document: [`Revocations`]
//! is that fact. The device fetches it beside the artifact before it stages anything and refuses a
//! `(version, sha256)` pair it names (exit 4, the policy lane). A version alone is not enough: the
//! digest is the artifact's identity, so a rebuilt release with the same version is a different
//! artifact and can be shipped after the failed one was withdrawn.
//!
//! ## The signed bytes, exactly
//!
//! The signature is ed25519 by the release key over the ASCII message
//!
//! ```text
//! revocations\n
//! {issued_at_ms}\n
//! {version1}:{sha256_1}:{reason_1}\n
//! {version2}:{sha256_2}:{reason_2}\n
//! ...
//! ```
//!
//! Entries are in the order given, each `version:sha256:reason` line ends with a newline, the
//! digests are **lowercase** hex, and an absent (or empty) reason is the empty field: the line
//! becomes `version:sha256:` followed immediately by the newline. The message therefore **always
//! ends with a newline** — including the empty list, whose message is exactly
//! `"revocations\n{issued_at_ms}\n"`. The trailing newline is part of the signed bytes, not a
//! serialization accident: it makes every entry-terminated line unambiguous, and
//! [`canonical_message`] and its tests spell it out byte-for-byte.
//!
//! The reason is signed **whole**, in the entry line, so a host that can rewrite the list cannot
//! change the text an operator reads: "broken on Windows 11" and "install the old driver" are
//! different signed bytes. A reason containing a line break is refused before signing and during
//! verification, because a newline would split one entry into two.
//!
//! The JSON is camelCase and `deny_unknown_fields`, like the release document: a field this build
//! has not heard of is a list this build does not understand.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::document::{version_is_plain, ReleaseDocument, MAX_ISSUED_FUTURE_MS};
use crate::sign::{hex_decode, hex_encode, now_ms, read_seed, write_atomic, SignError};

/// The longest a revocation list stays usable: 365 days.
///
/// A revoked artifact stays revoked; the age limit is about the signature, not the decision. A list
/// older than a year was signed against a world that may no longer exist (a rotated key, a rewritten
/// policy), and pretending it is current would be worse than refusing the update. Re-signing a stale
/// list with [`Revocations::sign`] refreshes the date while keeping every entry.
pub const MAX_REVOCATIONS_AGE_MS: u64 = 365 * 24 * 60 * 60 * 1_000;

/// One withdrawn artifact.
///
/// The identity is the pair, not the version: a rebuilt release with the same version but a different
/// digest is a different artifact and stays installable. [`Revocations::revoked_release`] compares the
/// digest case-insensitively, as the rest of the update path does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokedRelease {
    /// The release's version, as `major.minor.patch`.
    pub version: String,
    /// Lower-hex SHA-256 of the withdrawn artifact.
    pub sha256: String,
    /// Why it was withdrawn, for the operator. Signed with the entry; absent means an empty field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The signed list of withdrawn releases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revocations {
    /// Millisecond Unix time the list was signed.
    pub issued_at_ms: u64,
    /// The withdrawn artifacts, in the order they were added.
    pub revoked: Vec<RevokedRelease>,
    /// Lower-hex ed25519 signature over [`canonical_message`].
    pub signature: String,
}

/// The exact bytes the signature covers.
///
/// `"revocations\n{issued_at_ms}\n"` followed by one `"{version}:{sha256}:{reason}\n"` line per
/// entry, in the order given. An absent reason is the empty field, so the line ends `:{sha256}:\n`.
/// The trailing newline is present even when the list is empty; see the module docs.
pub fn canonical_message(revocations: &Revocations) -> String {
    use std::fmt::Write as _;

    let mut message = format!("revocations\n{}\n", revocations.issued_at_ms);
    for entry in &revocations.revoked {
        let reason = entry.reason.as_deref().unwrap_or("");
        // Writing to a String cannot fail.
        let _ = writeln!(message, "{}:{}:{}", entry.version, entry.sha256, reason);
    }
    message
}

impl Revocations {
    /// Sign a list for `issued_at_ms` with the release key.
    ///
    /// Every entry is validated first — the version must be the stager's `major.minor.patch`, the
    /// digest must be 64 lowercase hex characters, and the reason must be one line — so the signer
    /// cannot produce a message whose line boundaries are ambiguous. A duplicate
    /// `(version, sha256)` is refused: the list has no meaning for it, and the CLI must not silently
    /// add one twice.
    pub fn sign(
        key: &SigningKey,
        issued_at_ms: u64,
        revoked: Vec<RevokedRelease>,
    ) -> Result<Self, RevocationError> {
        validate_entries(&revoked)?;
        let mut revocations = Revocations {
            issued_at_ms,
            revoked,
            signature: String::new(),
        };
        let signature = key.sign(canonical_message(&revocations).as_bytes());
        revocations.signature = hex_encode(&signature.to_bytes());
        Ok(revocations)
    }

    /// Verify the signature and the 365-day freshness window against the system clock.
    pub fn verify(&self, key: &VerifyingKey) -> Result<(), RevocationError> {
        self.verify_at(key, now_ms())
    }

    /// Verify the signature and the freshness window against an explicit clock.
    ///
    /// The shape and the signature are checked before `issuedAtMs` is used, exactly as the release
    /// document's policy signature is: an edited timestamp fails as a signature, never as a stale
    /// list that invites the reader to look at the wrong fact first.
    pub fn verify_at(&self, key: &VerifyingKey, now_ms: u64) -> Result<(), RevocationError> {
        self.verify_signature(key)?;
        check_freshness(self.issued_at_ms, now_ms)
    }

    /// Verify the shape and the signature, without applying the freshness window.
    ///
    /// This is what the `revoke` command uses on `--from`: a stale list is still a genuinely signed
    /// list, and re-signing it is how an operator refreshes it. The device always uses [`verify`],
    /// which does apply the window.
    pub fn verify_signature(&self, key: &VerifyingKey) -> Result<(), RevocationError> {
        validate_entries(&self.revoked)?;
        let signature = decode_signature(&self.signature)?;
        key.verify_strict(canonical_message(self).as_bytes(), &signature)
            .map_err(|_| RevocationError::Signature)
    }

    /// The entry naming exactly this version and digest, if the list withdraws it.
    ///
    /// The digest comparison is case-insensitive because the document's `sha256` may be written in
    /// either case; the version is exact.
    pub fn revoked_release(&self, version: &str, sha256: &str) -> Option<&RevokedRelease> {
        self.revoked
            .iter()
            .find(|entry| entry.version == version && entry.sha256.eq_ignore_ascii_case(sha256))
    }

    /// Whether the list withdraws exactly this version and digest.
    pub fn is_revoked(&self, version: &str, sha256: &str) -> bool {
        self.revoked_release(version, sha256).is_some()
    }
}

/// The facts one `revoke` run has to establish before it signs.
///
/// `document` and `version`/`sha256` are two ways to name the same pair; exactly one is used. `from`
/// is the existing signed list to append to, `None` starts a new one. The key file is read through
/// [`read_seed`], so its mode is checked before any of its contents are used.
#[derive(Clone, Copy, Debug)]
pub struct RevokeRequest<'a> {
    /// An existing `release.json` to take the version and digest from.
    pub document: Option<&'a Path>,
    /// The release version, when no document is given.
    pub version: Option<&'a str>,
    /// The artifact digest, when no document is given.
    pub sha256: Option<&'a str>,
    /// The operator's reason, stored in the new entry and signed with it.
    pub reason: Option<&'a str>,
    /// An existing signed list to append to.
    pub from: Option<&'a Path>,
    /// The 0600 file holding the release key seed.
    pub key_file: &'a Path,
    /// Where the new list is written, atomically.
    pub out: &'a Path,
}

/// Why `revoke` refused to produce or write a list.
#[derive(Debug)]
pub enum RevokeError {
    /// The command's facts are incomplete or contradictory.
    Usage(&'static str),
    /// The key file could not be read, or is not 0600.
    Key(SignError),
    /// The release document could not be read.
    Document {
        /// The document that failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The release document is not JSON this build can read.
    DocumentParse {
        /// The document that failed.
        path: PathBuf,
        /// The parser's error.
        source: serde_json::Error,
    },
    /// The existing revocations list could not be read.
    Existing {
        /// The list that failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The existing revocations list is not JSON this build can read.
    ExistingParse {
        /// The list that failed.
        path: PathBuf,
        /// The parser's error.
        source: serde_json::Error,
    },
    /// The existing list's signature or entries are not usable.
    ExistingSignature(RevocationError),
    /// The version is not the plain `major.minor.patch` the stager accepts.
    Version(String),
    /// The reason contains a line break, which would split the signed entry into two.
    MalformedReason(String),
    /// The digest is not 64 hex characters.
    Sha(String),
    /// A new list would overwrite an existing file without being told to append.
    WouldOverwrite(PathBuf),
    /// The new list could not be signed (typically a duplicate pair).
    List(RevocationError),
    /// The new list could not be serialized.
    Serialize(serde_json::Error),
    /// The new list could not be written.
    Write {
        /// The destination that failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
}

impl RevokeError {
    /// Whether this is a command-line shape error (exit 2) rather than a refusal (exit 1).
    pub fn is_usage(&self) -> bool {
        matches!(self, RevokeError::Usage(_))
    }
}

impl fmt::Display for RevokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RevokeError::Usage(detail) => f.write_str(detail),
            RevokeError::Key(error) => write!(f, "cannot read the release key: {error}"),
            RevokeError::Document { path, source } => {
                write!(
                    f,
                    "cannot read the release document {}: {source}",
                    path.display()
                )
            }
            RevokeError::DocumentParse { path, source } => {
                write!(
                    f,
                    "cannot parse the release document {}: {source}",
                    path.display()
                )
            }
            RevokeError::Existing { path, source } => {
                write!(
                    f,
                    "cannot read the revocations list {}: {source}",
                    path.display()
                )
            }
            RevokeError::ExistingParse { path, source } => {
                write!(
                    f,
                    "cannot parse the revocations list {}: {source}",
                    path.display()
                )
            }
            RevokeError::ExistingSignature(error) => {
                write!(f, "the existing revocations list is not usable: {error}")
            }
            RevokeError::Version(value) => {
                write!(f, "the release version {value:?} is not major.minor.patch")
            }
            RevokeError::MalformedReason(value) => write!(
                f,
                "the reason {value:?} contains a line break; the signed entry is one line"
            ),
            RevokeError::Sha(value) => {
                write!(f, "the artifact sha256 {value:?} is not 64 hex characters")
            }
            RevokeError::WouldOverwrite(path) => write!(
                f,
                "{} already exists; pass --from {} to append to it",
                path.display(),
                path.display()
            ),
            RevokeError::List(error) => write!(f, "cannot sign the revocations list: {error}"),
            RevokeError::Serialize(error) => {
                write!(f, "cannot serialize the revocations list: {error}")
            }
            RevokeError::Write { path, source } => write!(
                f,
                "cannot write the revocations list {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RevokeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RevokeError::Key(error) => Some(error),
            RevokeError::Document { source, .. }
            | RevokeError::Existing { source, .. }
            | RevokeError::Write { source, .. } => Some(source),
            RevokeError::DocumentParse { source, .. }
            | RevokeError::ExistingParse { source, .. }
            | RevokeError::Serialize(source) => Some(source),
            RevokeError::ExistingSignature(error) | RevokeError::List(error) => Some(error),
            _ => None,
        }
    }
}

/// Append one `(version, sha256)` to a signed revocation list and write it atomically.
///
/// The new list's `issued_at_ms` is the current time: appending re-signs the whole list, which is
/// also how an expired list is refreshed. An existing list named by `from` is verified with the key
/// before its entries are used, so a tampered file cannot be laundered into a genuine signature. A
/// pair already present is refused by [`Revocations::sign`] and nothing is written.
pub fn revoke_file(request: &RevokeRequest<'_>) -> Result<Revocations, RevokeError> {
    let (version, sha256) = match (request.document, request.version, request.sha256) {
        (Some(document), None, None) => document_pair(document)?,
        (None, Some(version), Some(sha256)) => {
            if !version_is_plain(version) {
                return Err(RevokeError::Version(version.to_owned()));
            }
            (version.to_owned(), normalize_sha(sha256)?)
        }
        (Some(_), _, _) => {
            return Err(RevokeError::Usage(
                "give either --document or --version/--sha256, not both",
            ));
        }
        (None, None, _) => {
            return Err(RevokeError::Usage(
                "give --document, or both --version and --sha256",
            ));
        }
        (None, Some(_), None) => {
            return Err(RevokeError::Usage("--version also needs --sha256"));
        }
    };

    let key = read_seed(request.key_file).map_err(RevokeError::Key)?;

    let mut revoked = match request.from {
        Some(from) => {
            let bytes = fs::read(from).map_err(|source| RevokeError::Existing {
                path: from.to_owned(),
                source,
            })?;
            let existing: Revocations =
                serde_json::from_slice(&bytes).map_err(|source| RevokeError::ExistingParse {
                    path: from.to_owned(),
                    source,
                })?;
            existing
                .verify_signature(&key.verifying_key())
                .map_err(RevokeError::ExistingSignature)?;
            existing.revoked
        }
        None => {
            match fs::metadata(request.out) {
                Ok(_) => return Err(RevokeError::WouldOverwrite(request.out.to_owned())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(RevokeError::Write {
                        path: request.out.to_owned(),
                        source,
                    })
                }
            }
            Vec::new()
        }
    };
    revoked.push(RevokedRelease {
        version: version.clone(),
        sha256: sha256.clone(),
        reason: request.reason.map(str::to_owned),
    });

    let list = Revocations::sign(&key, now_ms(), revoked).map_err(RevokeError::List)?;
    let mut bytes = serde_json::to_vec_pretty(&list).map_err(RevokeError::Serialize)?;
    bytes.push(b'\n');
    write_atomic(request.out, &bytes).map_err(|source| RevokeError::Write {
        path: request.out.to_owned(),
        source,
    })?;
    Ok(list)
}

/// Read a release document and return its version and normalized digest.
fn document_pair(path: &Path) -> Result<(String, String), RevokeError> {
    let bytes = fs::read(path).map_err(|source| RevokeError::Document {
        path: path.to_owned(),
        source,
    })?;
    let document: ReleaseDocument =
        serde_json::from_slice(&bytes).map_err(|source| RevokeError::DocumentParse {
            path: path.to_owned(),
            source,
        })?;
    if !version_is_plain(&document.version) {
        return Err(RevokeError::Version(document.version));
    }
    Ok((document.version, normalize_sha(&document.sha256)?))
}

/// Validate a 64-hex digest and normalize it to the lowercase spelling the signed bytes require.
fn normalize_sha(text: &str) -> Result<String, RevokeError> {
    if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(text.to_ascii_lowercase())
    } else {
        Err(RevokeError::Sha(text.to_owned()))
    }
}

/// Why a revocation list was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevocationError {
    /// The ed25519 signature over [`canonical_message`] does not verify.
    Signature,
    /// A revoked version is not a plain `major.minor.patch`.
    MalformedVersion(String),
    /// A revoked digest is not 64 lowercase hex characters.
    MalformedSha(String),
    /// A reason contains a line break, which would split the signed entry into two.
    MalformedReason(String),
    /// The signature is not 64 bytes of hex.
    MalformedSignature(String),
    /// The same `(version, sha256)` appears more than once.
    Duplicate {
        /// The duplicated version.
        version: String,
        /// The duplicated digest.
        sha256: String,
    },
    /// The list is older than [`MAX_REVOCATIONS_AGE_MS`].
    Stale {
        /// When the list says it was signed.
        issued_at_ms: u64,
        /// The clock the check used.
        now_ms: u64,
    },
    /// The list claims to have been issued further ahead than [`MAX_ISSUED_FUTURE_MS`].
    IssuedInFuture {
        /// When the list says it was signed.
        issued_at_ms: u64,
        /// The clock the check used.
        now_ms: u64,
    },
}

impl fmt::Display for RevocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RevocationError::Signature => {
                f.write_str("the revocations signature does not verify")
            }
            RevocationError::MalformedVersion(value) => {
                write!(f, "the revoked version {value:?} is not major.minor.patch")
            }
            RevocationError::MalformedSha(value) => write!(
                f,
                "the revoked sha256 {value:?} is not 64 lowercase hex characters"
            ),
            RevocationError::MalformedReason(value) => write!(
                f,
                "the revoked reason {value:?} contains a line break; the signed entry is one line"
            ),
            RevocationError::MalformedSignature(value) => {
                write!(f, "the revocations signature {value:?} is not 64 bytes of hex")
            }
            RevocationError::Duplicate { version, sha256 } => write!(
                f,
                "the release {version} (sha256 {sha256}) appears more than once in the revocation list"
            ),
            RevocationError::Stale {
                issued_at_ms,
                now_ms,
            } => write!(
                f,
                "the revocations list is stale: issued at {issued_at_ms} ms, now {now_ms} ms, \
                 older than the {} day limit",
                MAX_REVOCATIONS_AGE_MS / (24 * 60 * 60 * 1_000)
            ),
            RevocationError::IssuedInFuture {
                issued_at_ms,
                now_ms,
            } => write!(
                f,
                "the revocations list was issued at {issued_at_ms} ms, more than {} minutes ahead \
                 of {now_ms} ms",
                MAX_ISSUED_FUTURE_MS / (60 * 1_000)
            ),
        }
    }
}

impl std::error::Error for RevocationError {}

/// Decode one 64-byte ed25519 signature from lower- or upper-case hex.
fn decode_signature(text: &str) -> Result<Signature, RevocationError> {
    let bytes = hex_decode(text)
        .filter(|bytes| bytes.len() == 64)
        .ok_or_else(|| RevocationError::MalformedSignature(text.to_owned()))?;
    Signature::from_slice(&bytes).map_err(|_| RevocationError::MalformedSignature(text.to_owned()))
}

/// Validate every entry's shape and uniqueness, without touching the key.
fn validate_entries(revoked: &[RevokedRelease]) -> Result<(), RevocationError> {
    for entry in revoked {
        if !version_is_plain(&entry.version) {
            return Err(RevocationError::MalformedVersion(entry.version.clone()));
        }
        if !is_lower_sha256_hex(&entry.sha256) {
            return Err(RevocationError::MalformedSha(entry.sha256.clone()));
        }
        if let Some(reason) = &entry.reason {
            if reason.contains('\n') || reason.contains('\r') {
                return Err(RevocationError::MalformedReason(reason.clone()));
            }
        }
    }

    for (index, entry) in revoked.iter().enumerate() {
        for other in &revoked[index + 1..] {
            if entry.version == other.version && entry.sha256 == other.sha256 {
                return Err(RevocationError::Duplicate {
                    version: entry.version.clone(),
                    sha256: entry.sha256.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Whether `value` is exactly 64 lowercase hex characters.
fn is_lower_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Refuse a list older than [`MAX_REVOCATIONS_AGE_MS`] or too far in the future.
fn check_freshness(issued_at_ms: u64, now_ms: u64) -> Result<(), RevocationError> {
    if now_ms.saturating_sub(issued_at_ms) > MAX_REVOCATIONS_AGE_MS {
        return Err(RevocationError::Stale {
            issued_at_ms,
            now_ms,
        });
    }
    if issued_at_ms.saturating_sub(now_ms) > MAX_ISSUED_FUTURE_MS {
        return Err(RevocationError::IssuedInFuture {
            issued_at_ms,
            now_ms,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signature, SigningKey, Verifier};

    use super::*;

    const SEED: [u8; 32] = [7u8; 32];
    const NOW: u64 = 1_700_000_000_000;
    const SHA_A: &str = "b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f";
    const SHA_B: &str = "86c0ea10853cdebc340b41ccd1a151ff282ec570bad37f52fa2321dd34b7479f";

    fn key() -> SigningKey {
        SigningKey::from_bytes(&SEED)
    }

    fn entry(version: &str, sha256: &str, reason: Option<&str>) -> RevokedRelease {
        RevokedRelease {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
            reason: reason.map(str::to_owned),
        }
    }

    fn list(entries: Vec<RevokedRelease>) -> Revocations {
        Revocations::sign(&key(), NOW, entries).expect("the fixture list signs")
    }

    #[test]
    fn the_canonical_bytes_end_with_a_newline_even_for_an_empty_list() {
        let empty = list(Vec::new());

        let message = canonical_message(&empty);

        assert_eq!(
            message.as_bytes(),
            b"revocations\n1700000000000\n",
            "the empty list's signed message is two lines and a trailing newline"
        );
        assert!(
            message.ends_with('\n'),
            "the trailing newline is part of the signed bytes"
        );
    }

    #[test]
    fn the_canonical_bytes_carry_every_entry_in_order() {
        let entries = list(vec![
            entry("2.0.3", SHA_A, Some("boot loop on Windows 11")),
            entry("2.0.4", SHA_B, None),
        ]);

        let message = canonical_message(&entries);

        assert_eq!(
            message.as_bytes(),
            b"revocations\n1700000000000\n\
              2.0.3:b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f:boot loop on Windows 11\n\
              2.0.4:86c0ea10853cdebc340b41ccd1a151ff282ec570bad37f52fa2321dd34b7479f:\n",
            "the test's own literal is the contract: header, timestamp, then one \
             version:sha256:reason line per entry, each newline-terminated"
        );
        assert_eq!(message.lines().count(), 4);
        assert!(message.ends_with('\n'));
    }

    #[test]
    fn the_reason_is_signed_whole_and_an_empty_reason_is_a_trailing_colon() {
        let with_reason = list(vec![entry("2.0.3", SHA_A, Some("withdrawn"))]);

        assert_eq!(
            canonical_message(&with_reason).as_bytes(),
            b"revocations\n1700000000000\n\
              2.0.3:b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f:withdrawn\n",
            "the reason is part of the signed bytes, after the digest and before the newline"
        );

        let empty_reason = list(vec![entry("2.0.3", SHA_A, None)]);
        assert_eq!(
            canonical_message(&empty_reason).as_bytes(),
            b"revocations\n1700000000000\n\
              2.0.3:b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f:\n",
            "an absent reason is the empty field: a colon immediately before the newline"
        );
    }

    #[test]
    fn a_tampered_reason_fails_verification() {
        let key = key();
        let mut entries = list(vec![entry("2.0.3", SHA_A, Some("broken on Windows 11"))]);
        entries.revoked[0].reason = Some("install the old driver".to_owned());

        assert_eq!(
            entries.verify_at(&key.verifying_key(), NOW),
            Err(RevocationError::Signature),
            "a host cannot rewrite what the operator reads without breaking the signature"
        );
    }

    #[test]
    fn a_signed_list_verifies_with_its_key_and_not_with_another() {
        let key = key();
        let entries = list(vec![entry("2.0.3", SHA_A, Some("broken install"))]);

        assert_eq!(entries.verify_at(&key.verifying_key(), NOW), Ok(()));
        assert_eq!(
            entries.verify_at(&SigningKey::from_bytes(&[8u8; 32]).verifying_key(), NOW),
            Err(RevocationError::Signature)
        );
    }

    #[test]
    fn the_signature_is_ed25519_over_the_canonical_bytes() {
        let key = key();
        let entries = list(vec![entry("2.0.3", SHA_A, None)]);
        let signature = crate::sign::hex_decode(&entries.signature).expect("the signature is hex");

        key.verifying_key()
            .verify(
                canonical_message(&entries).as_bytes(),
                &Signature::from_slice(&signature).expect("64 bytes"),
            )
            .expect("the signature verifies over exactly the canonical message");
    }

    #[test]
    fn a_tampered_entry_or_signature_fails_verification() {
        let key = key();

        let mut version = list(vec![entry("2.0.3", SHA_A, Some("withdrawn"))]);
        version.revoked[0].version = "2.0.4".to_owned();
        assert_eq!(
            version.verify_at(&key.verifying_key(), NOW),
            Err(RevocationError::Signature)
        );

        let mut digest = list(vec![entry("2.0.3", SHA_A, Some("withdrawn"))]);
        digest.revoked[0].sha256 = SHA_B.to_owned();
        assert_eq!(
            digest.verify_at(&key.verifying_key(), NOW),
            Err(RevocationError::Signature)
        );

        let mut reason = list(vec![entry("2.0.3", SHA_A, Some("withdrawn"))]);
        reason.revoked[0].reason = Some("a different story".to_owned());
        assert_eq!(
            reason.verify_at(&key.verifying_key(), NOW),
            Err(RevocationError::Signature)
        );

        let mut signature = list(vec![entry("2.0.3", SHA_A, None)]);
        signature.signature = "00".repeat(64);
        assert_eq!(
            signature.verify_at(&key.verifying_key(), NOW),
            Err(RevocationError::Signature)
        );
    }

    #[test]
    fn a_list_is_stale_after_365_days() {
        let key = key();
        let issued = NOW;
        let entries = list(vec![entry("2.0.3", SHA_A, None)]);

        assert_eq!(
            entries.verify_at(&key.verifying_key(), issued + MAX_REVOCATIONS_AGE_MS),
            Ok(()),
            "exactly 365 days old is still inside the window"
        );
        assert_eq!(
            entries.verify_at(&key.verifying_key(), issued + MAX_REVOCATIONS_AGE_MS + 1),
            Err(RevocationError::Stale {
                issued_at_ms: issued,
                now_ms: issued + MAX_REVOCATIONS_AGE_MS + 1,
            })
        );
    }

    #[test]
    fn a_list_far_in_the_future_is_refused() {
        let key = key();
        let entries = list(vec![entry("2.0.3", SHA_A, None)]);

        assert_eq!(
            entries.verify_at(&key.verifying_key(), NOW - MAX_ISSUED_FUTURE_MS),
            Ok(())
        );
        assert_eq!(
            entries.verify_at(&key.verifying_key(), NOW - MAX_ISSUED_FUTURE_MS - 1),
            Err(RevocationError::IssuedInFuture {
                issued_at_ms: NOW,
                now_ms: NOW - MAX_ISSUED_FUTURE_MS - 1,
            })
        );
    }

    #[test]
    fn the_lookup_is_the_version_and_the_digest_together() {
        let entries = list(vec![entry("2.0.3", SHA_A, Some("withdrawn"))]);

        assert!(entries.is_revoked("2.0.3", SHA_A));
        assert!(
            entries.is_revoked("2.0.3", &SHA_A.to_uppercase()),
            "a document's digest is compared case-insensitively"
        );
        assert!(
            !entries.is_revoked("2.0.3", SHA_B),
            "the same version with a different digest is a different artifact"
        );
        assert!(!entries.is_revoked("2.0.4", SHA_A));
        assert_eq!(
            entries
                .revoked_release("2.0.3", SHA_A)
                .and_then(|entry| entry.reason.as_deref()),
            Some("withdrawn")
        );
        assert!(entries.revoked_release("2.0.4", SHA_A).is_none());
    }

    #[test]
    fn a_signed_list_round_trips_with_camel_case_fields() {
        let entries = list(vec![
            entry("2.0.3", SHA_A, Some("withdrawn")),
            entry("2.0.4", SHA_B, None),
        ]);

        let mut value = serde_json::to_value(&entries).expect("the list serializes");

        assert_eq!(value["issuedAtMs"], serde_json::json!(NOW));
        assert_eq!(value["revoked"][0]["version"], serde_json::json!("2.0.3"));
        assert_eq!(value["revoked"][0]["sha256"], serde_json::json!(SHA_A));
        assert_eq!(
            value["revoked"][0]["reason"],
            serde_json::json!("withdrawn")
        );
        assert!(
            value["revoked"][1].get("reason").is_none(),
            "an absent reason is omitted, not null: {value}"
        );
        assert_eq!(
            serde_json::from_value::<Revocations>(value.clone()).expect("the list parses"),
            entries
        );

        value["revoked"][0]["notes"] = serde_json::json!("an unsigned fact");
        assert!(
            serde_json::from_value::<Revocations>(value).is_err(),
            "an unknown field must not parse"
        );
    }

    #[test]
    fn signing_refuses_malformed_entries_and_duplicates() {
        let key = key();

        let uppercase = SHA_A.to_uppercase();
        for (version, sha256) in [
            ("2.0", SHA_A),
            ("+2.0.3", SHA_A),
            ("2.0.3", "not-a-digest"),
            ("2.0.3", uppercase.as_str()),
        ] {
            let result = Revocations::sign(&key, NOW, vec![entry(version, sha256, None)]);
            assert!(result.is_err(), "{version:?}/{sha256:?} must not be signed");
        }

        let newline_reason = Revocations::sign(
            &key,
            NOW,
            vec![entry("2.0.3", SHA_A, Some("one line\nthen another"))],
        );
        assert!(
            matches!(
                newline_reason,
                Err(RevocationError::MalformedReason(ref reason))
                    if reason == "one line\nthen another"
            ),
            "a line break would split the signed entry: {newline_reason:?}"
        );

        let duplicate = Revocations::sign(
            &key,
            NOW,
            vec![entry("2.0.3", SHA_A, None), entry("2.0.3", SHA_A, None)],
        );
        assert!(
            matches!(duplicate, Err(RevocationError::Duplicate { .. })),
            "{duplicate:?}"
        );
    }
}
