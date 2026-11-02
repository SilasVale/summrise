//! Grant signing.
//!
//! A grant is the second signature of the two the design separates: the mesh pins *who the machine
//! is*, and a console-signed grant says *what a person may do*. The device verifies it offline
//! against a pinned console public key, so the message must be reconstructible from the grant alone
//! and must not be ambiguous.
//!
//! The type and the canonical message live in `summrise-core`, which the device also depends on:
//! the two ends of this signature share one definition rather than two copies that can drift. This
//! module only owns the console's policy — which tools may be named in a grant and how long a grant
//! may live — and the signing step.

use ed25519_dalek::{Signature, Signer, SigningKey};

// Re-exported so the console's public API and the internal callers both name the one shared type.
pub use summrise_core::grant::{canonical_message, Grant, GrantEnvelope};

/// The shortest lifetime a grant may be issued for: one minute. A born-expired grant is a
/// configuration error, not a grant.
pub const MIN_GRANT_LIFETIME_S: u64 = 60;

/// The longest lifetime a grant may be issued for: seven days.
pub const MAX_GRANT_LIFETIME_S: u64 = 7 * 24 * 60 * 60;

/// Why a grant presented to the proxy was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VerifyError {
    /// The signature is absent, malformed, or not the console's.
    BadSignature,
    /// The grant is authentic but names a different device than the request.
    DeviceMismatch,
    /// The grant is authentic but has reached its expiry.
    Expired,
}

/// Verify a presented grant against the console's own public key, this device and the clock.
///
/// The signature is checked first: an expired or misaddressed grant that is not authentic is a
/// signature failure, and the proxy must not act on any of its fields before that passes.
pub(crate) fn verify(
    grant_key: &SigningKey,
    envelope: &GrantEnvelope,
    device_id: &str,
    now_ms: u64,
) -> Result<(), VerifyError> {
    summrise_core::grant::verify(
        &envelope.grant,
        &envelope.signature,
        &grant_key.verifying_key(),
    )
    .map_err(|_| VerifyError::BadSignature)?;

    if envelope.grant.device_id != device_id {
        return Err(VerifyError::DeviceMismatch);
    }
    if now_ms >= envelope.grant.expires_at_ms {
        return Err(VerifyError::Expired);
    }
    Ok(())
}

/// Why a grant was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GrantError {
    /// No tools were requested; a grant that permits nothing is a mistake, not a grant.
    EmptyTools,
    /// At least one requested tool is not in the device's capability registry.
    UnknownTool,
    /// The requested lifetime is shorter than [`MIN_GRANT_LIFETIME_S`].
    ExpiryTooShort,
    /// The requested lifetime is longer than [`MAX_GRANT_LIFETIME_S`].
    ExpiryTooLong,
}

/// A grant and its signature, ready for the wire.
pub(crate) struct SignedGrant {
    pub(crate) grant: Grant,
    pub(crate) signature: Signature,
}

/// Validate the request and sign the canonical message.
///
/// A lifetime is accepted in `MIN_GRANT_LIFETIME_S..=MAX_GRANT_LIFETIME_S`; both bounds are errors,
/// because a grant that has already expired when it is issued is a configuration mistake.
/// `allow_destructive` is the operator's explicit consent and is signed with the rest: the device
/// can only pre-approve a destructive tool when the document says so.
pub(crate) fn sign(
    grant_key: &SigningKey,
    device_id: &str,
    tools: Vec<String>,
    expires_in_s: u64,
    allow_destructive: bool,
    now_ms: u64,
) -> Result<SignedGrant, GrantError> {
    if tools.is_empty() {
        return Err(GrantError::EmptyTools);
    }
    if tools
        .iter()
        .any(|tool| summrise_core::registry::tool(tool).is_none())
    {
        return Err(GrantError::UnknownTool);
    }
    if expires_in_s < MIN_GRANT_LIFETIME_S {
        return Err(GrantError::ExpiryTooShort);
    }
    if expires_in_s > MAX_GRANT_LIFETIME_S {
        return Err(GrantError::ExpiryTooLong);
    }

    let expires_at_ms = now_ms.saturating_add(expires_in_s * 1_000);
    let grant = Grant {
        device_id: device_id.to_owned(),
        tools,
        expires_at_ms,
        allow_destructive,
        // Sixteen random bytes per signed grant: the device refuses a nonce it has already admitted,
        // which is what turns a captured frame into one that runs once.
        nonce: crate::hex::encode(&crate::auth::random_32()[..16]),
    };
    let signature = grant_key.sign(canonical_message(&grant).as_bytes());

    Ok(SignedGrant { grant, signature })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [0x22; 32];

    #[test]
    fn a_grant_is_signed_over_the_canonical_message() {
        let key = SigningKey::from_bytes(&SEED);
        let tools = vec!["files.read".to_owned(), "memory.search".to_owned()];

        let signed = sign(&key, "ab12", tools.clone(), 60, false, 1_000).expect("a valid grant");

        assert_eq!(signed.grant.expires_at_ms, 61_000);
        assert_eq!(signed.grant.tools, tools);
        assert!(
            !signed.grant.allow_destructive,
            "destructive consent is off unless the request asked for it"
        );
        // The nonce is the fifth signed field: sixteen random bytes, hex.
        assert_eq!(signed.grant.nonce.len(), 32, "{}", signed.grant.nonce);
        assert!(
            signed
                .grant
                .nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
            "the nonce is hex: {}",
            signed.grant.nonce
        );
        let message = canonical_message(&signed.grant);
        assert_eq!(
            message,
            format!(
                "grant\nab12\nfiles.read,memory.search\n61000\nfalse\n{}",
                signed.grant.nonce
            )
        );
        key.verifying_key()
            .verify_strict(message.as_bytes(), &signed.signature)
            .expect("the signature covers exactly the canonical string");
    }

    #[test]
    fn two_grants_for_the_same_request_carry_different_nonces() {
        let key = SigningKey::from_bytes(&SEED);
        let first = sign(
            &key,
            "ab12",
            vec!["files.read".to_owned()],
            60,
            false,
            1_000,
        )
        .expect("a valid grant");
        let second = sign(
            &key,
            "ab12",
            vec!["files.read".to_owned()],
            60,
            false,
            1_000,
        )
        .expect("a valid grant");
        assert_ne!(
            first.grant.nonce, second.grant.nonce,
            "a nonce that repeated would let one consent cover two calls"
        );
    }

    #[test]
    fn an_explicit_destructive_consent_is_signed_into_the_grant() {
        let key = SigningKey::from_bytes(&SEED);
        let signed = sign(
            &key,
            "ab12",
            vec!["files.write".to_owned()],
            60,
            true,
            1_000,
        )
        .expect("a valid grant");

        assert!(signed.grant.allow_destructive);
        assert_eq!(
            canonical_message(&signed.grant),
            format!(
                "grant\nab12\nfiles.write\n61000\ntrue\n{}",
                signed.grant.nonce
            )
        );
        key.verifying_key()
            .verify_strict(
                canonical_message(&signed.grant).as_bytes(),
                &signed.signature,
            )
            .expect("the flag is part of the signed message");
    }

    #[test]
    fn refusals_are_typed() {
        let key = SigningKey::from_bytes(&SEED);
        assert_eq!(
            sign(&key, "ab12", Vec::new(), 60, false, 0).err(),
            Some(GrantError::EmptyTools)
        );
        assert_eq!(
            sign(&key, "ab12", vec!["no.such.tool".to_owned()], 60, false, 0).err(),
            Some(GrantError::UnknownTool)
        );
        assert_eq!(
            sign(
                &key,
                "ab12",
                vec!["files.read".to_owned()],
                MAX_GRANT_LIFETIME_S + 1,
                false,
                0
            )
            .err(),
            Some(GrantError::ExpiryTooLong)
        );
    }
}
