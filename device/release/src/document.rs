//! The signed release document (design §4.4, §6).
//!
//! One document names one artifact, one version, one channel and one floor. It carries two
//! signatures by the same release key:
//!
//! * `signature` covers the **whole policy** — exactly
//!   `"{version}\n{channel}\n{sha256}\n{artifactUrl}\n{minVersion}\n{issuedAtMs}"`, one line each,
//!   no trailing newline. Every fact the device acts on is therefore signed: a CDN that can rewrite
//!   `minVersion` or point `artifactUrl` elsewhere without the key produces a document `verify`
//!   refuses.
//! * `artifact_signature` covers the older `"{version}\n{channel}\n{sha256}"` message. That is the
//!   exact bytes `summrise-device`'s stager and `summrise-launch` already verify, so the stager's
//!   contract does not change: the updater hands it this signature after the policy check.
//!
//! `issuedAtMs` is enforced, not recorded: [`verify`] refuses a document older than
//! [`MAX_ISSUED_AGE_MS`] (180 days) or issued more than [`MAX_ISSUED_FUTURE_MS`] (10 minutes) in the
//! future. [`verify_at`] takes the clock as an argument so a test can pin it.
//!
//! The document is parsed with `deny_unknown_fields`: a key this build has not heard of is a
//! document this build does not understand, and silently ignoring it is how a release acquires a
//! meaning nobody signed.

use std::fmt;
use std::str::FromStr;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use summrise_core::manifest::Version;

/// The longest a signed document stays usable: 180 days.
///
/// A release document is a policy, and an old policy is how a rollback is replayed. The window is
/// generous enough for a release that sits on a CDN while a fleet updates and short enough that a
/// document signed a year ago is not treated as current.
pub const MAX_ISSUED_AGE_MS: u64 = 180 * 24 * 60 * 60 * 1_000;

/// How far ahead of the local clock a document may be issued: 10 minutes.
///
/// The allowance absorbs ordinary clock skew between the signing host and the device without letting
/// a document scheduled for the future become valid early.
pub const MAX_ISSUED_FUTURE_MS: u64 = 10 * 60 * 1_000;

/// The release channel a document belongs to.
///
/// Channels are values, not deployments (design §6): the same artifact and the same document shape
/// serve both, and the channel decides who may install it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// The channel every device follows.
    Stable,
    /// The channel a device opts into; it accepts stable documents too.
    Canary,
}

impl Channel {
    /// The lowercase spelling used on the wire and in the signed message.
    pub const fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Canary => "canary",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Channel {
    type Err = ReleaseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "stable" => Ok(Channel::Stable),
            "canary" => Ok(Channel::Canary),
            other => Err(ReleaseError::UnknownChannel(other.to_owned())),
        }
    }
}

/// The release document a device fetches, verifies and then stages.
///
/// The field order is the JSON order. `signature` covers every policy field through
/// [`canonical_message`]; `artifact_signature` covers the stager's three-part
/// [`artifact_message`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseDocument {
    /// The release's version, as `major.minor.patch`.
    pub version: String,
    /// The channel the signature was made for.
    pub channel: Channel,
    /// Lower-hex SHA-256 of the artifact.
    pub sha256: String,
    /// Where the artifact is fetched from.
    pub artifact_url: String,
    /// The oldest version this release will let a device stay on.
    pub min_version: String,
    /// Millisecond Unix time the document was signed.
    pub issued_at_ms: u64,
    /// Lower-hex ed25519 signature over [`artifact_message`], the stager's three-part message.
    pub artifact_signature: String,
    /// Lower-hex ed25519 signature over [`canonical_message`], the whole policy.
    pub signature: String,
}

/// The exact bytes the policy signature covers: six lines and no trailing newline.
///
/// The newlines are part of the signed message so a value cannot be moved into the next field and
/// produce a second valid reading of the same signature. `artifactUrl` and `minVersion` are included
/// because they are policy: without them a CDN could lower the floor or redirect the artifact while
/// keeping the three older signed facts intact.
pub fn canonical_message(document: &ReleaseDocument) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        document.version,
        document.channel,
        document.sha256,
        document.artifact_url,
        document.min_version,
        document.issued_at_ms
    )
}

/// The stager's and the launcher's message: three lines and no trailing newline.
///
/// This is deliberately the older contract, unchanged: the updater passes
/// [`ReleaseDocument::artifact_signature`] to `stage`, which verifies exactly these bytes.
pub fn artifact_message(document: &ReleaseDocument) -> String {
    format!(
        "{}\n{}\n{}",
        document.version, document.channel, document.sha256
    )
}

/// Whether `version` is exactly three dot-separated ASCII-digit components and nothing else.
///
/// [`Version`] itself accepts a leading `+` (integer parsing does), but the update's version is a
/// file name too. The stager refuses anything this predicate refuses; the signer uses the same
/// function so it cannot produce a document the stager will refuse at `stage`.
pub fn version_is_plain(version: &str) -> bool {
    if version.contains('/') || version.contains('\\') || version.contains("..") {
        return false;
    }

    let mut components = 0;
    for component in version.split('.') {
        if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        components += 1;
    }

    components == 3 && Version::parse(version).is_ok()
}

/// Verify the document against `key`, using the system clock for its freshness window.
///
/// This is [`verify_at`] with `now = summrise_release::now_ms()`.
pub fn verify(document: &ReleaseDocument, key: &VerifyingKey) -> Result<(), ReleaseError> {
    verify_at(document, key, crate::sign::now_ms())
}

/// Verify both signatures and the freshness window against an explicit clock.
///
/// The order is the security property: the shape checks reject an ambiguous message, then the policy
/// signature is checked **before** any policy field is used (including `issuedAtMs`), then the
/// freshness window is applied, and only then is the artifact signature checked. A document whose
/// floor, URL or issue time was edited fails [`ReleaseError::Signature`] rather than being evaluated
/// on the edited value.
pub fn verify_at(
    document: &ReleaseDocument,
    key: &VerifyingKey,
    now_ms: u64,
) -> Result<(), ReleaseError> {
    if document.version.is_empty()
        || document.version.contains('\n')
        || document.version.contains('\r')
    {
        return Err(ReleaseError::MalformedVersion(document.version.clone()));
    }
    if !is_sha256_hex(&document.sha256) {
        return Err(ReleaseError::MalformedSha(document.sha256.clone()));
    }
    if !policy_field_is_clean(&document.artifact_url) {
        return Err(ReleaseError::MalformedPolicy("artifact URL"));
    }
    if !policy_field_is_clean(&document.min_version) {
        return Err(ReleaseError::MalformedPolicy("minimum version"));
    }

    let signature = decode_signature(&document.signature)?;
    key.verify_strict(canonical_message(document).as_bytes(), &signature)
        .map_err(|_| ReleaseError::Signature)?;
    check_freshness(document.issued_at_ms, now_ms)?;

    let artifact_signature = decode_signature(&document.artifact_signature)?;
    key.verify_strict(artifact_message(document).as_bytes(), &artifact_signature)
        .map_err(|_| ReleaseError::Signature)
}

/// Decode one 64-byte ed25519 signature from lower- or upper-case hex.
fn decode_signature(text: &str) -> Result<Signature, ReleaseError> {
    let bytes = hex_decode(text)
        .filter(|bytes| bytes.len() == 64)
        .ok_or_else(|| ReleaseError::MalformedSignature(text.to_owned()))?;
    Signature::from_slice(&bytes).map_err(|_| ReleaseError::MalformedSignature(text.to_owned()))
}

/// Refuse a document older than [`MAX_ISSUED_AGE_MS`] or too far in the future.
fn check_freshness(issued_at_ms: u64, now_ms: u64) -> Result<(), ReleaseError> {
    if now_ms.saturating_sub(issued_at_ms) > MAX_ISSUED_AGE_MS {
        return Err(ReleaseError::Expired {
            issued_at_ms,
            now_ms,
        });
    }
    if issued_at_ms.saturating_sub(now_ms) > MAX_ISSUED_FUTURE_MS {
        return Err(ReleaseError::IssuedInFuture {
            issued_at_ms,
            now_ms,
        });
    }
    Ok(())
}

/// Whether a policy string can be one line of the signed message.
fn policy_field_is_clean(value: &str) -> bool {
    !value.is_empty() && !value.contains('\n') && !value.contains('\r')
}

/// What [`select`] says to do with a verified document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The document names a release newer than the installed version: stage it.
    Update,
    /// The document names the installed version exactly: there is nothing to do.
    ///
    /// Re-running a check against the document that is already installed is a success, not a
    /// refusal. The device makes no request to the supervisor and writes nothing.
    UpToDate,
}

/// The device's channel and version policy. This is the **only** update decision; `core` exposes no
/// second one.
///
/// A `stable` device refuses a `canary` document even when its signature is genuine: the signature
/// proves who signed the release, not that this device is its audience. A `canary` device accepts
/// both channels, because every canary release also belongs to the stable line.
///
/// A release below the document's own `min_version` is malformed policy and is refused. So is a
/// release older than the installed version. A release **equal** to the installed version is
/// [`Selection::UpToDate`]: it is not a downgrade, and a check that finds nothing to do exits 0.
pub fn select(
    document: &ReleaseDocument,
    device_channel: Channel,
    installed: &str,
) -> Result<Selection, ReleaseError> {
    if device_channel == Channel::Stable && document.channel == Channel::Canary {
        return Err(ReleaseError::Channel);
    }

    let release = parse_version(VersionField::Release, &document.version)?;
    let minimum = parse_version(VersionField::Minimum, &document.min_version)?;
    let installed_version = parse_version(VersionField::Installed, installed)?;

    if release < minimum {
        return Err(ReleaseError::BelowMinimum);
    }
    if release < installed_version {
        return Err(ReleaseError::OlderThanInstalled);
    }
    if release == installed_version {
        return Ok(Selection::UpToDate);
    }

    Ok(Selection::Update)
}

/// Parse one version, naming the field that failed.
fn parse_version(field: VersionField, value: &str) -> Result<Version, ReleaseError> {
    Version::parse(value).map_err(|_| ReleaseError::Version {
        field,
        value: value.to_owned(),
    })
}

/// Whether `value` is exactly 64 hex characters (either case).
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Decode a hex string; `None` for an odd length, non-ASCII or a non-hex character.
fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        bytes.push((high * 16 + low) as u8);
    }
    Some(bytes)
}

/// Which version a [`ReleaseError`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionField {
    /// The document's version.
    Release,
    /// The document's minimum supported version.
    Minimum,
    /// The device's installed version.
    Installed,
}

impl fmt::Display for VersionField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionField::Release => f.write_str("release version"),
            VersionField::Minimum => f.write_str("minimum version"),
            VersionField::Installed => f.write_str("installed version"),
        }
    }
}

/// Why a document was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseError {
    /// The document's ed25519 signature does not verify.
    Signature,
    /// The version is empty or contains a line break, which would make the signed message ambiguous.
    MalformedVersion(String),
    /// The digest is not 64 hex characters.
    MalformedSha(String),
    /// A signed policy field is empty or contains a line break, which would make the message ambiguous.
    MalformedPolicy(&'static str),
    /// The signature is not 64 bytes of hex.
    MalformedSignature(String),
    /// The document was issued longer ago than [`MAX_ISSUED_AGE_MS`].
    Expired {
        /// When the document says it was signed.
        issued_at_ms: u64,
        /// The clock the check used.
        now_ms: u64,
    },
    /// The document claims to have been issued further ahead than [`MAX_ISSUED_FUTURE_MS`].
    IssuedInFuture {
        /// When the document says it was signed.
        issued_at_ms: u64,
        /// The clock the check used.
        now_ms: u64,
    },
    /// The document names a channel this build does not know.
    UnknownChannel(String),
    /// The document is for a channel this device does not accept.
    Channel,
    /// The release is below the minimum version the document itself names.
    BelowMinimum,
    /// The release is older than the installed version: a downgrade.
    OlderThanInstalled,
    /// A version is not `major.minor.patch`.
    Version {
        /// Which version was malformed.
        field: VersionField,
        /// The string as it was written.
        value: String,
    },
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseError::Signature => f.write_str("the release signature does not verify"),
            ReleaseError::MalformedVersion(value) => write!(
                f,
                "the release version {value:?} is empty or contains a line break"
            ),
            ReleaseError::MalformedSha(value) => {
                write!(f, "the release sha256 {value:?} is not 64 hex characters")
            }
            ReleaseError::MalformedPolicy(field) => {
                write!(f, "the release's {field} is empty or contains a line break")
            }
            ReleaseError::MalformedSignature(value) => {
                write!(f, "the release signature {value:?} is not 64 bytes of hex")
            }
            ReleaseError::Expired {
                issued_at_ms,
                now_ms,
            } => write!(
                f,
                "the release document is stale: issued at {issued_at_ms} ms, now {now_ms} ms, \
                 older than the {} day limit",
                MAX_ISSUED_AGE_MS / (24 * 60 * 60 * 1_000)
            ),
            ReleaseError::IssuedInFuture {
                issued_at_ms,
                now_ms,
            } => write!(
                f,
                "the release document was issued at {issued_at_ms} ms, more than {} minutes ahead \
                 of {now_ms} ms",
                MAX_ISSUED_FUTURE_MS / (60 * 1_000)
            ),
            ReleaseError::UnknownChannel(value) => {
                write!(
                    f,
                    "unknown release channel {value:?}; expected stable or canary"
                )
            }
            ReleaseError::Channel => {
                f.write_str("this device does not accept the release's channel")
            }
            ReleaseError::BelowMinimum => {
                f.write_str("the release version is below the minimum version it names")
            }
            ReleaseError::OlderThanInstalled => f.write_str(
                "the release is older than the installed version; a release that is not newer \
                 than the installed version is a downgrade",
            ),
            ReleaseError::Version { field, value } => {
                write!(f, "the {field} {value:?} is not major.minor.patch")
            }
        }
    }
}

impl std::error::Error for ReleaseError {}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const SHA: &str = "b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f";
    const TEST_SEED: [u8; 32] = [7u8; 32];

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&TEST_SEED)
    }

    fn hex_encode(bytes: &[u8]) -> String {
        use std::fmt::Write as _;

        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(text, "{byte:02x}");
        }
        text
    }

    fn document(version: &str, channel: Channel, min_version: &str) -> ReleaseDocument {
        ReleaseDocument {
            version: version.to_owned(),
            channel,
            sha256: SHA.to_owned(),
            artifact_url: "https://example.test/summrise-device.exe".to_owned(),
            min_version: min_version.to_owned(),
            issued_at_ms: 1_700_000_000_000,
            artifact_signature: "00".repeat(64),
            signature: "00".repeat(64),
        }
    }

    fn signed_document(version: &str, channel: Channel, min_version: &str) -> ReleaseDocument {
        signed_document_at(version, channel, min_version, 1_700_000_000_000)
    }

    fn signed_document_at(
        version: &str,
        channel: Channel,
        min_version: &str,
        issued_at_ms: u64,
    ) -> ReleaseDocument {
        let key = signing_key();
        let mut document = document(version, channel, min_version);
        document.issued_at_ms = issued_at_ms;
        let artifact_signature = key.sign(artifact_message(&document).as_bytes());
        let signature = key.sign(canonical_message(&document).as_bytes());
        document.artifact_signature = hex_encode(&artifact_signature.to_bytes());
        document.signature = hex_encode(&signature.to_bytes());
        document
    }

    #[test]
    fn the_artifact_message_is_still_exactly_three_lines() {
        let stable = document("2.0.3", Channel::Stable, "2.0.0");
        let canary = document("2.0.3", Channel::Canary, "2.0.0");

        let message = artifact_message(&stable);

        assert_eq!(message, format!("2.0.3\nstable\n{SHA}"));
        assert_eq!(artifact_message(&canary), format!("2.0.3\ncanary\n{SHA}"));
        assert_eq!(message.lines().count(), 3);
        assert!(!message.ends_with('\n'), "there is no trailing separator");
    }

    #[test]
    fn the_canonical_message_covers_the_whole_policy() {
        let document = document("2.0.3", Channel::Stable, "2.0.0");

        let message = canonical_message(&document);

        assert_eq!(
            message,
            format!(
                "2.0.3\nstable\n{SHA}\nhttps://example.test/summrise-device.exe\n2.0.0\n1700000000000"
            ),
            "minVersion, artifactUrl and issuedAtMs are policy facts and must be signed"
        );
        assert_eq!(message.lines().count(), 6);
        assert!(!message.ends_with('\n'), "there is no trailing separator");
    }

    #[test]
    fn channel_names_are_the_closed_lowercase_pair() {
        assert_eq!(Channel::Stable.as_str(), "stable");
        assert_eq!(Channel::Canary.as_str(), "canary");
        assert_eq!("stable".parse::<Channel>(), Ok(Channel::Stable));
        assert_eq!("canary".parse::<Channel>(), Ok(Channel::Canary));
        assert_eq!(
            "beta".parse::<Channel>(),
            Err(ReleaseError::UnknownChannel("beta".to_owned()))
        );
    }

    #[test]
    fn a_document_round_trips_with_camel_case_fields_and_lowercase_channels() {
        let document = signed_document("2.0.3", Channel::Stable, "2.0.0");

        let value = serde_json::to_value(&document).expect("the document serializes");

        assert_eq!(value["channel"], serde_json::json!("stable"));
        assert_eq!(
            value["artifactUrl"],
            serde_json::json!("https://example.test/summrise-device.exe")
        );
        assert_eq!(value["minVersion"], serde_json::json!("2.0.0"));
        assert_eq!(value["issuedAtMs"], serde_json::json!(1_700_000_000_000u64));
        assert_eq!(
            value["artifactSignature"],
            serde_json::json!(document.artifact_signature)
        );
        assert_eq!(value["signature"], serde_json::json!(document.signature));
        assert!(
            value.get("artifact_url").is_none(),
            "the wire spelling is artifactUrl: {value}"
        );
        assert!(
            value.get("min_version").is_none(),
            "the wire spelling is minVersion: {value}"
        );
        assert_eq!(
            serde_json::from_value::<ReleaseDocument>(value).expect("the document parses"),
            document
        );
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let document = signed_document("2.0.3", Channel::Stable, "2.0.0");
        let mut value = serde_json::to_value(&document).expect("the document serializes");
        value["notes"] = serde_json::json!("an unsigned fact this build does not know");

        let parsed = serde_json::from_value::<ReleaseDocument>(value);

        assert!(
            parsed.is_err(),
            "an unknown field must not parse: {parsed:?}"
        );
    }

    #[test]
    fn a_signed_document_verifies() {
        let key = signing_key();
        let document = signed_document("2.0.3", Channel::Stable, "2.0.0");

        assert_eq!(
            verify_at(&document, &key.verifying_key(), document.issued_at_ms),
            Ok(())
        );

        let other = SigningKey::from_bytes(&[8u8; 32]);
        assert_eq!(
            verify_at(&document, &other.verifying_key(), document.issued_at_ms),
            Err(ReleaseError::Signature),
            "a signature is not transferable between keys"
        );
    }

    #[test]
    fn a_tampered_sha256_fails_verification() {
        let key = signing_key();
        let mut document = signed_document("2.0.3", Channel::Stable, "2.0.0");
        document.sha256 = "0".repeat(64);

        assert_eq!(
            verify_at(&document, &key.verifying_key(), document.issued_at_ms),
            Err(ReleaseError::Signature)
        );
    }

    #[test]
    fn a_tampered_version_or_channel_fails_verification() {
        let key = signing_key();

        let mut version = signed_document("2.0.3", Channel::Stable, "2.0.0");
        version.version = "2.0.4".to_owned();
        assert_eq!(
            verify_at(&version, &key.verifying_key(), version.issued_at_ms),
            Err(ReleaseError::Signature)
        );

        let mut channel = signed_document("2.0.3", Channel::Stable, "2.0.0");
        channel.channel = Channel::Canary;
        assert_eq!(
            verify_at(&channel, &key.verifying_key(), channel.issued_at_ms),
            Err(ReleaseError::Signature)
        );
    }

    #[test]
    fn a_tampered_floor_or_artifact_url_fails_verification() {
        let key = signing_key();
        let now = 1_700_000_000_000;

        // The reviewer's attack: the document was signed with a floor of 9.0.0, then edited down so
        // a 0.0.1 device would be eligible. The policy signature must cover the floor.
        let mut floor = signed_document_at("2.0.3", Channel::Stable, "9.0.0", now);
        floor.min_version = "0.0.1".to_owned();
        assert_eq!(
            verify_at(&floor, &key.verifying_key(), now),
            Err(ReleaseError::Signature)
        );

        // Redirecting the artifact is the same tamper, one field over.
        let mut url = signed_document("2.0.3", Channel::Stable, "2.0.0");
        url.artifact_url = "https://attacker.test/summrise-device.exe".to_owned();
        assert_eq!(
            verify_at(&url, &key.verifying_key(), now),
            Err(ReleaseError::Signature)
        );

        // The issue time is a policy fact too: editing it must not be able to revive a stale replay.
        let mut issued = signed_document("2.0.3", Channel::Stable, "2.0.0");
        issued.issued_at_ms += 1;
        assert_eq!(
            verify_at(&issued, &key.verifying_key(), now),
            Err(ReleaseError::Signature)
        );
    }

    #[test]
    fn a_document_is_fresh_for_180_days_and_10_minutes_ahead() {
        let key = signing_key();
        let issued = 1_700_000_000_000;
        let document = signed_document_at("2.0.3", Channel::Stable, "2.0.0", issued);

        assert_eq!(
            verify_at(&document, &key.verifying_key(), issued + MAX_ISSUED_AGE_MS),
            Ok(()),
            "exactly 180 days old is still inside the window"
        );
        assert_eq!(
            verify_at(
                &document,
                &key.verifying_key(),
                issued + MAX_ISSUED_AGE_MS + 1
            ),
            Err(ReleaseError::Expired {
                issued_at_ms: issued,
                now_ms: issued + MAX_ISSUED_AGE_MS + 1,
            })
        );
        assert_eq!(
            verify_at(
                &document,
                &key.verifying_key(),
                issued - MAX_ISSUED_FUTURE_MS
            ),
            Ok(()),
            "exactly 10 minutes ahead is inside the skew allowance"
        );
        assert_eq!(
            verify_at(
                &document,
                &key.verifying_key(),
                issued - MAX_ISSUED_FUTURE_MS - 1
            ),
            Err(ReleaseError::IssuedInFuture {
                issued_at_ms: issued,
                now_ms: issued - MAX_ISSUED_FUTURE_MS - 1,
            })
        );
    }

    #[test]
    fn a_malformed_signature_is_named_before_it_is_checked() {
        let key = signing_key();
        let mut document = signed_document("2.0.3", Channel::Stable, "2.0.0");
        document.signature = "not hex".to_owned();

        assert_eq!(
            verify_at(&document, &key.verifying_key(), document.issued_at_ms),
            Err(ReleaseError::MalformedSignature("not hex".to_owned()))
        );
    }

    #[test]
    fn stable_refuses_a_canary_document() {
        let document = document("2.0.3", Channel::Canary, "2.0.0");

        assert_eq!(
            select(&document, Channel::Stable, "2.0.0"),
            Err(ReleaseError::Channel)
        );
    }

    #[test]
    fn canary_accepts_both_channels() {
        for channel in [Channel::Stable, Channel::Canary] {
            let document = document("2.0.3", channel, "2.0.0");

            assert_eq!(
                select(&document, Channel::Canary, "2.0.0"),
                Ok(Selection::Update),
                "a canary device accepts a {channel} document"
            );
        }
    }

    #[test]
    fn the_same_version_is_up_to_date_not_a_downgrade() {
        let document = document("2.0.3", Channel::Stable, "2.0.0");

        assert_eq!(
            select(&document, Channel::Stable, "2.0.3"),
            Ok(Selection::UpToDate),
            "re-running the check that is already installed succeeds without staging anything"
        );
        assert_eq!(
            select(&document, Channel::Canary, "2.0.3"),
            Ok(Selection::UpToDate),
            "the equality rule is the same on both channels"
        );
    }

    #[test]
    fn a_release_below_its_minimum_is_refused() {
        let document = document("2.0.0", Channel::Stable, "2.0.3");

        assert_eq!(
            select(&document, Channel::Stable, "1.0.0"),
            Err(ReleaseError::BelowMinimum)
        );
    }

    #[test]
    fn an_older_release_is_refused_as_older_than_installed() {
        let document = document("2.0.3", Channel::Stable, "2.0.0");

        for installed in ["2.0.4", "2.1.0"] {
            assert_eq!(
                select(&document, Channel::Stable, installed),
                Err(ReleaseError::OlderThanInstalled),
                "{installed} must not take the older 2.0.3"
            );
        }
    }

    #[test]
    fn a_newer_release_above_the_floor_is_accepted() {
        let document = document("2.0.3", Channel::Stable, "2.0.0");

        assert_eq!(
            select(&document, Channel::Stable, "2.0.2"),
            Ok(Selection::Update)
        );
    }

    #[test]
    fn a_malformed_version_is_refused_with_its_field() {
        let document = document("2.0", Channel::Stable, "2.0.0");

        assert_eq!(
            select(&document, Channel::Stable, "1.0.0"),
            Err(ReleaseError::Version {
                field: VersionField::Release,
                value: "2.0".to_owned(),
            })
        );
    }
}
