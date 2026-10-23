//! The console's grant, as one type shared by the two sides that sign and verify it.
//!
//! A grant is the second of the design's two signatures: the mesh pins *who the machine is*, and a
//! console-signed grant says *what a person may do*. The console signs it and the device verifies
//! it offline against a pinned console key, so the message has to be reconstructible from the
//! grant alone and must not be ambiguous.
//!
//! The canonical message is exactly:
//!
//! ```text
//! grant
//! {device_id}
//! {tool},{tool}
//! {expires_at_ms}
//! {allow_destructive}
//! ```
//!
//! Every field is constrained — the device id is hex, the tool names are registry names (none
//! contains a comma), the expiry is decimal, and `allow_destructive` is the lowercase `true` or
//! `false` — so no value can be moved across a separator to produce a different grant with the same
//! bytes. The type lives in `core`, which both the console and the device depend on, so the two
//! sides cannot drift. Destructive consent is the last line because it is part of the document, not
//! a local policy: a grant that did not consent must not be turned into one that did.

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// How far the device's clock may be from the console's when it reads an expiry.
///
/// A grant is signed by a service on another machine; the device's clock is not the console's.
/// Five minutes is the same allowance registration uses, so a grant does not die early merely
/// because one of the two clocks is a little fast.
pub const CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;

/// The signed facts a device can verify offline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The device the grant is for: the registered id, the hex public key.
    pub device_id: String,
    /// The registry tools the operator may call, in the order the operator asked for them.
    pub tools: Vec<String>,
    /// When the grant stops being valid, in Unix milliseconds.
    pub expires_at_ms: u64,
    /// Whether the operator consented to destructive tools under this grant.
    ///
    /// `true` means the console recorded the operator explicitly asking for destructive calls
    /// without a per-call prompt. It is inside the signature: flipping it after signing changes the
    /// canonical message, and a grant that did not consent can never be replayed as one that did.
    pub allow_destructive: bool,
}

/// A grant and the console's signature, as `POST /grants` returns it and the device channel carries
/// it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantEnvelope {
    /// The facts the signature covers.
    pub grant: Grant,
    /// The console's Ed25519 signature over [`canonical_message`], in hex.
    pub signature: String,
}

/// Why a signature did not verify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureError {
    /// The signature is absent, malformed, or not the console's.
    BadSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the console's signature does not verify")
    }
}

impl std::error::Error for SignatureError {}

/// The exact bytes the console's grant key signs.
///
/// It takes the grant rather than its fields so there is one spelling of the message: a caller
/// cannot join the tools differently, or forget a separator, without changing this function.
pub fn canonical_message(grant: &Grant) -> String {
    format!(
        "grant\n{}\n{}\n{}\n{}",
        grant.device_id,
        grant.tools.join(","),
        grant.expires_at_ms,
        grant.allow_destructive
    )
}

/// Verify a signature over `grant` against the console key the device pins.
///
/// The signature is checked before any field of the grant is believed: a grant naming the wrong
/// device or carrying a live expiry is only meaningful once the bytes are the console's.
pub fn verify(grant: &Grant, signature: &str, key: &VerifyingKey) -> Result<(), SignatureError> {
    let bytes = decode_signature(signature).ok_or(SignatureError::BadSignature)?;
    let signature = Signature::from_bytes(&bytes);
    key.verify_strict(canonical_message(grant).as_bytes(), &signature)
        .map_err(|_| SignatureError::BadSignature)
}

/// Decode exactly 64 bytes of hex, either case.
fn decode_signature(text: &str) -> Option<[u8; 64]> {
    let digits = text.as_bytes();
    if digits.len() != 128 {
        return None;
    }

    let mut out = [0u8; 64];
    for (byte, pair) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const SEED: [u8; 32] = [0x22; 32];

    fn grant() -> Grant {
        Grant {
            device_id: "ab".repeat(32),
            tools: vec!["files.read".to_owned(), "terminal.create".to_owned()],
            expires_at_ms: 1_700_000_000_000,
            allow_destructive: false,
        }
    }

    fn destructive_grant() -> Grant {
        Grant {
            allow_destructive: true,
            ..grant()
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn signed(grant: &Grant, key: &SigningKey) -> GrantEnvelope {
        let signature = key.sign(canonical_message(grant).as_bytes());
        GrantEnvelope {
            grant: grant.clone(),
            signature: hex(&signature.to_bytes()),
        }
    }

    #[test]
    fn the_canonical_message_is_exactly_the_agreed_string() {
        assert_eq!(
            canonical_message(&grant()),
            "grant\nabababababababababababababababababababababababababababababababab\nfiles.read,terminal.create\n1700000000000\nfalse"
        );
    }

    #[test]
    fn destructive_consent_is_the_last_signed_line() {
        let key = SigningKey::from_bytes(&SEED);
        let grant = destructive_grant();
        assert!(
            canonical_message(&grant).ends_with("\ntrue"),
            "the flag is a lowercase `true` on its own line"
        );
        assert_eq!(
            verify(
                &grant,
                &signed(&grant, &key).signature,
                &key.verifying_key()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_destructive_flag_altered_after_signing_is_refused() {
        let key = SigningKey::from_bytes(&SEED);
        let mut envelope = signed(&destructive_grant(), &key);
        envelope.grant.allow_destructive = false;

        assert_eq!(
            verify(&envelope.grant, &envelope.signature, &key.verifying_key()),
            Err(SignatureError::BadSignature)
        );
    }

    #[test]
    fn a_signature_over_the_old_four_line_message_is_refused() {
        let key = SigningKey::from_bytes(&SEED);
        let grant = grant();
        // The message this type signed before destructive consent existed: four lines, no flag.
        let old_message = format!(
            "grant\n{}\n{}\n{}",
            grant.device_id,
            grant.tools.join(","),
            grant.expires_at_ms
        );
        let signature = key.sign(old_message.as_bytes());

        assert_eq!(
            verify(&grant, &hex(&signature.to_bytes()), &key.verifying_key()),
            Err(SignatureError::BadSignature),
            "a grant is not valid under the document a previous version signed"
        );
    }

    #[test]
    fn a_signature_over_the_canonical_message_verifies() {
        let key = SigningKey::from_bytes(&SEED);
        assert_eq!(
            verify(
                &grant(),
                &signed(&grant(), &key).signature,
                &key.verifying_key()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_signature_by_another_key_is_refused() {
        let signer = SigningKey::from_bytes(&SEED);
        let envelope = signed(&grant(), &signer);
        let other = SigningKey::from_bytes(&[0x33; 32]);

        assert_eq!(
            verify(&grant(), &envelope.signature, &other.verifying_key()),
            Err(SignatureError::BadSignature)
        );
    }

    #[test]
    fn a_grant_altered_after_it_was_signed_is_refused() {
        let key = SigningKey::from_bytes(&SEED);
        let mut envelope = signed(&grant(), &key);
        envelope.grant.tools.push("files.write".to_owned());

        assert_eq!(
            verify(&envelope.grant, &envelope.signature, &key.verifying_key()),
            Err(SignatureError::BadSignature)
        );
    }

    #[test]
    fn a_malformed_or_absent_signature_is_refused() {
        let key = SigningKey::from_bytes(&SEED);
        for signature in ["", "00", &"zz".repeat(64), &"00".repeat(63)] {
            assert_eq!(
                verify(&grant(), signature, &key.verifying_key()),
                Err(SignatureError::BadSignature),
                "{signature:?}"
            );
        }
    }
}
