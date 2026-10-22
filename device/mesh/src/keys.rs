//! The mesh identity: an Ed25519 keypair, and the hex form of its public half.
//!
//! The public key **is** the peer's name. There is no CA, no DNS name and no
//! expiry: a certificate that does not carry these 32 bytes is rejected by
//! [`crate::endpoint`]. TLS does its own ephemeral key agreement, so the
//! identity key only ever has to sign — which is why it is Ed25519 and not
//! X25519.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

/// The length of an Ed25519 public key, in bytes.
pub const PUBLIC_KEY_BYTES: usize = 32;

/// The length of an Ed25519 signature, in bytes.
pub const SIGNATURE_BYTES: usize = 64;

/// A bad key or signature. Every variant is a refusal; nothing defaults to a
/// usable key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// A public key in hex was not 64 hex characters.
    WrongLength { got: usize },
    /// A public key in hex contained something other than hex digits.
    BadHex,
    /// The bytes were 32 long but not a valid Ed25519 public key.
    InvalidPublicKey,
    /// The signature did not verify for this key and message.
    BadSignature,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLength { got } => write!(
                f,
                "a mesh public key is {PUBLIC_KEY_BYTES} bytes ({} hex characters); this one is {got}",
                PUBLIC_KEY_BYTES * 2
            ),
            Self::BadHex => f.write_str("a mesh public key must be hexadecimal"),
            Self::InvalidPublicKey => f.write_str("not a valid Ed25519 public key"),
            Self::BadSignature => f.write_str("the Ed25519 signature does not verify"),
        }
    }
}

impl std::error::Error for KeyError {}

/// A mesh identity: the Ed25519 keypair.
///
/// `Debug` deliberately prints only the public half. A private key that
/// reaches a log line is a compromised identity, and an accidental `Debug`
/// is the easiest way for that to happen.
#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public", &self.public_hex())
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Generate a fresh identity from the OS CSPRNG.
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut rand_core::OsRng),
        }
    }

    /// The verifying (public) half.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// The 32 public bytes — the value a peer pins.
    pub fn public_bytes(&self) -> [u8; PUBLIC_KEY_BYTES] {
        self.signing.verifying_key().to_bytes()
    }

    /// The public key as 64 lowercase hex characters.
    pub fn public_hex(&self) -> String {
        hex_encode(&self.public_bytes())
    }

    /// Parse a peer's public key from its hex form.
    ///
    /// Fail-closed: a malformed key is a typed error, never a default.
    pub fn public_from_hex(hex: &str) -> Result<[u8; PUBLIC_KEY_BYTES], KeyError> {
        let hex = hex.trim();
        if hex.len() != PUBLIC_KEY_BYTES * 2 {
            return Err(KeyError::WrongLength { got: hex.len() });
        }
        let raw = hex_decode(hex)?;
        let mut key = [0u8; PUBLIC_KEY_BYTES];
        key.copy_from_slice(&raw);
        // Not every 32-byte string is a point; reject the ones that are not
        // here, where the peer id enters, rather than in the handshake.
        VerifyingKey::from_bytes(&key).map_err(|_| KeyError::InvalidPublicKey)?;
        Ok(key)
    }

    /// The private half, for persistence. Never log this.
    pub fn private_bytes(&self) -> [u8; PUBLIC_KEY_BYTES] {
        self.signing.to_bytes()
    }

    /// Rebuild an identity from its stored private bytes.
    pub fn from_private_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self {
            signing: SigningKey::from_bytes(&bytes),
        }
    }

    /// Sign `message` with this identity.
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_BYTES] {
        self.signing.sign(message).to_bytes()
    }

    /// Verify `signature` over `message` with this identity's public key.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &[u8; SIGNATURE_BYTES],
    ) -> Result<(), KeyError> {
        verify(&self.public_bytes(), message, signature)
    }
}

/// Verify a signature against a raw public key.
///
/// Strict verification is used: it rejects the malleable forms Ed25519
/// permits but no honest signer produces.
pub fn verify(
    public: &[u8; PUBLIC_KEY_BYTES],
    message: &[u8],
    signature: &[u8; SIGNATURE_BYTES],
) -> Result<(), KeyError> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| KeyError::InvalidPublicKey)?;
    let signature = Signature::from_bytes(signature);
    key.verify_strict(message, &signature)
        .map_err(|_| KeyError::BadSignature)
}

/// Lowercase hex, no prefix.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("nibble"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("nibble"));
    }
    out
}

/// Parse lowercase or uppercase hex.
pub fn hex_decode(hex: &str) -> Result<Vec<u8>, KeyError> {
    let digits = hex.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return Err(KeyError::BadHex);
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_chunks::<2>().0 {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, KeyError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(KeyError::BadHex),
    }
}

/// The 32-byte Ed25519 private key in the **PKCS#8** form rustls wants.
///
/// Ed25519's PKCS#8 v1 form is a fixed structure around the 32-byte seed:
///
/// ```text
/// SEQUENCE (0x30, 0x2e) {
///   INTEGER 0
///   SEQUENCE { OID 1.3.101.112 }
///   OCTET STRING { OCTET STRING (32) seed }
/// }
/// ```
///
/// Every length is constant, so the encoding is a constant prefix followed by
/// the seed — no key-format crate needed. rcgen's traits take the raw bytes
/// directly, so this appears only where rustls asks for a private key.
pub fn pkcs8_ed25519(seed: &[u8; PUBLIC_KEY_BYTES]) -> Vec<u8> {
    const PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    let mut encoded = Vec::with_capacity(PREFIX.len() + PUBLIC_KEY_BYTES);
    encoded.extend_from_slice(&PREFIX);
    encoded.extend_from_slice(seed);
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies() {
        let identity = Identity::generate();
        let message = b"the identity is the truth";
        let signature = identity.sign(message);

        identity
            .verify(message, &signature)
            .expect("the identity's own signature must verify");
        verify(&identity.public_bytes(), message, &signature)
            .expect("a peer with the public key must verify it too");
    }

    #[test]
    fn a_tampered_signature_does_not_verify() {
        let identity = Identity::generate();
        let message = b"the identity is the truth";
        let mut signature = identity.sign(message);
        signature[0] ^= 0x01;

        assert_eq!(
            identity.verify(message, &signature),
            Err(KeyError::BadSignature),
            "a single flipped bit must be refused"
        );
    }

    #[test]
    fn a_signature_for_another_message_does_not_verify() {
        let identity = Identity::generate();
        let signature = identity.sign(b"the original message");

        assert_eq!(
            identity.verify(b"a different message", &signature),
            Err(KeyError::BadSignature),
        );
    }

    #[test]
    fn the_public_key_hex_round_trips() {
        let identity = Identity::generate();
        let hex = identity.public_hex();

        assert_eq!(hex.len(), PUBLIC_KEY_BYTES * 2);
        assert_eq!(
            Identity::public_from_hex(&hex).expect("its own hex"),
            identity.public_bytes()
        );
        // The case of the input is not part of the value.
        assert_eq!(
            Identity::public_from_hex(&hex.to_uppercase()).expect("uppercase hex"),
            identity.public_bytes()
        );
    }

    #[test]
    fn a_malformed_public_key_is_refused() {
        assert_eq!(
            Identity::public_from_hex("00"),
            Err(KeyError::WrongLength { got: 2 })
        );
        assert_eq!(
            Identity::public_from_hex(&"zz".repeat(PUBLIC_KEY_BYTES)),
            Err(KeyError::BadHex)
        );
        assert_eq!(
            Identity::public_from_hex(&"0".repeat(PUBLIC_KEY_BYTES * 2 - 1)),
            Err(KeyError::WrongLength {
                got: PUBLIC_KEY_BYTES * 2 - 1
            })
        );
    }
}
