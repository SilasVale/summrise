//! The server challenge and the signed hello that answers it.
//!
//! TLS proves **the peer the dialer reached** — the dialer pins the server's
//! certificate. It does not prove **the peer that dialed the server**: the
//! server listens for many devices and must not decide their identity from a
//! certificate it was never told to trust. So the **server speaks first** on
//! every connection: it sends a fresh [`Challenge`] drawn from the OS CSPRNG.
//! The dialer answers with a [`Hello`] that names its `peer_id_hex`, echoes the
//! challenge byte for byte, carries the TLS session's [`BINDING_BYTES`]-byte
//! exporter value, and signs
//! `hello\n{peer_id_hex}\n{challenge_hex}\n{binding_hex}` (lowercase hex) with
//! its identity key. The server checks the echo and the binding, then resolves
//! the peer id to a pin and verifies the signature, **before any other frame**.
//!
//! **The challenge stops a recording; the binding stops a REFLECTION.** A
//! recorded hello echoes a challenge no other connection issued, so it cannot
//! be replayed. But a malicious peer that is itself the dialer's server can
//! relay a third peer's live challenge to the dialer and forward the resulting
//! hello to that peer — the echo is perfect and the signature is real, and the
//! third peer would believe it is talking to the dialer. The signature alone
//! never named the channel it was made on. The binding does: it is derived from
//! the TLS session's secrets with an exporter, identical on the two ends of one
//! connection and unrelated on any other, so a hello made on the attacker's
//! connection does not verify on the third peer's.
//!
//! **The binding is checked before the signature**, in constant time, so a
//! lifted hello is refused even though its signature is valid.
//!
//! The peer id is an arbitrary name the control plane uses; the pin it
//! resolves to is the identity. Binding the two, and the connection's
//! challenge, in the signature means a peer cannot claim another peer's id
//! without the other peer's key and cannot answer a challenge it did not
//! receive.

use std::collections::HashMap;

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::frame::{Frame, Kind};
use crate::keys::{
    hex_decode, hex_encode, verify as verify_signature, Identity, KeyError, PUBLIC_KEY_BYTES,
    SIGNATURE_BYTES,
};

/// A peer's Ed25519 public key, in the 32-byte form the pin uses.
pub type PublicKey = [u8; PUBLIC_KEY_BYTES];

/// The length of a server challenge, in bytes.
pub const CHALLENGE_BYTES: usize = 32;

/// The length of the TLS-channel binding a hello is signed over, in bytes.
pub const BINDING_BYTES: usize = 32;

/// The TLS exporter label both ends derive the hello binding with.
///
/// An exporter is a function of the TLS session's secrets: the two ends of ONE connection compute the
/// same bytes, and any other connection — including one a malicious peer controls — computes different
/// ones. That is what makes the binding a proof of the channel rather than another nonce.
pub const BINDING_LABEL: &[u8] = b"summrise mesh hello";

/// The exporter context: the wire version.
///
/// The context is part of the derivation, so a hello from one wire version cannot be replayed onto
/// another even by a peer that can compute the exporter of its own connection.
pub const BINDING_CONTEXT: &[u8] = b"summrise/mesh/3";

/// The first frame a server sends on a connection: `{bytes}`.
///
/// The bytes are fresh for the connection and unpredictable. The dialer must
/// echo them in its [`Hello`] and sign them; the server compares the echo with
/// the value it issued on this connection before it looks at the signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    /// The challenge bytes, from the OS CSPRNG.
    pub bytes: [u8; CHALLENGE_BYTES],
}

/// The first frame a dialer sends after the challenge: `{peer_id_hex,
/// challenge, sig}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// The name the control plane knows this peer by, hex for a mesh peer.
    pub peer_id_hex: String,
    /// The connection's challenge, echoed byte for byte.
    pub challenge: [u8; CHALLENGE_BYTES],
    /// The TLS session's exporter value. **THIS IS WHAT BINDS THE SIGNATURE TO THE CONNECTION.** Without
    /// it a malicious peer that is the dialer's TLS server can relay a third peer's live challenge and
    /// forward the resulting hello to that peer: the challenge is fresh per connection, but the signature
    /// never named the connection it was made on.
    pub binding: [u8; BINDING_BYTES],
    /// Hex Ed25519 signature over [`Hello::signed_message`].
    pub sig: String,
}

/// Why a challenge or a hello was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloError {
    /// The resolver has no pin for this peer id.
    UnknownPeer(String),
    /// The signature does not verify for the resolved pin.
    BadSignature,
    /// The hello does not echo the challenge issued on this connection.
    ChallengeMismatch,
    /// The hello was signed over a different TLS connection's exporter.
    BindingMismatch,
    /// The payload is not a hello (bad hex, wrong length, wrong JSON).
    Malformed(String),
    /// The first frame was not a hello.
    NotFirstFrame,
    /// The server's first frame was not a challenge.
    NotChallenge,
}

impl std::fmt::Display for HelloError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPeer(peer_id) => write!(f, "no mesh pin is known for peer {peer_id:?}"),
            Self::BadSignature => f.write_str("the mesh hello signature does not verify"),
            Self::ChallengeMismatch => {
                f.write_str("the mesh hello does not echo this connection's challenge")
            }
            Self::BindingMismatch => {
                f.write_str("the mesh hello is bound to a different TLS connection")
            }
            Self::Malformed(message) => write!(f, "not a mesh hello: {message}"),
            Self::NotFirstFrame => {
                f.write_str("the first mesh frame from a client must be a hello")
            }
            Self::NotChallenge => {
                f.write_str("the first mesh frame from a server must be a challenge")
            }
        }
    }
}

impl std::error::Error for HelloError {}

/// Resolve a peer id to the pin the server knows for it.
///
/// Returning `None` is a refusal. There is deliberately no default pin.
pub trait PinResolver: Send + Sync {
    /// The pinned key for `peer_id`, if this server knows the peer.
    fn resolve(&self, peer_id: &str) -> Option<PublicKey>;
}

impl PinResolver for HashMap<String, PublicKey> {
    fn resolve(&self, peer_id: &str) -> Option<PublicKey> {
        self.get(peer_id).copied()
    }
}

impl Challenge {
    /// Draw a challenge from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; CHALLENGE_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self { bytes }
    }

    /// A challenge with caller-supplied bytes, for a test that needs them
    /// fixed. Production callers use [`Challenge::generate`].
    pub fn from_bytes(bytes: [u8; CHALLENGE_BYTES]) -> Self {
        Self { bytes }
    }

    /// Does this challenge echo `other`?
    ///
    /// The comparison does not stop at the first differing byte: a challenge
    /// is public, but a timing side channel here would still tell a dialer how
    /// much of a guessed value was right.
    pub fn matches(&self, other: &Self) -> bool {
        let mut difference = 0u8;
        for (received, expected) in self.bytes.iter().zip(other.bytes.iter()) {
            difference |= received ^ expected;
        }
        difference == 0
    }

    /// Wrap this challenge in the frame that carries it.
    pub fn to_frame(&self) -> Result<Frame, HelloError> {
        let payload =
            serde_json::to_value(self).map_err(|error| HelloError::Malformed(error.to_string()))?;
        Ok(Frame {
            kind: Kind::Challenge,
            // A challenge is not a request and has no request id.
            id: 0,
            payload,
        })
    }

    /// Read a challenge from a frame, refusing any other kind.
    pub fn from_frame(frame: &Frame) -> Result<Self, HelloError> {
        if frame.kind != Kind::Challenge {
            return Err(HelloError::NotChallenge);
        }
        serde_json::from_value(frame.payload.clone())
            .map_err(|error| HelloError::Malformed(error.to_string()))
    }
}

impl Hello {
    /// Sign a hello for `peer_id` over the connection's `challenge`.
    pub fn sign(
        identity: &Identity,
        peer_id: impl Into<String>,
        challenge: &Challenge,
        binding: &[u8; BINDING_BYTES],
    ) -> Self {
        let peer_id = peer_id.into();
        let signature = identity.sign(&Self::signed_message(&peer_id, challenge, binding));
        Self {
            peer_id_hex: peer_id,
            challenge: challenge.bytes,
            binding: *binding,
            sig: hex_encode(&signature),
        }
    }

    /// The exact bytes the signature covers:
    /// `hello\n{peer_id_hex}\n{challenge_hex}\n{binding_hex}`, lowercase hex.
    ///
    /// **THE BINDING LINE IS THE REFLECTION DEFENCE.** It names a value only the two ends of one TLS
    /// session can derive, so a hello made on one connection does not verify on another.
    pub fn signed_message(
        peer_id: &str,
        challenge: &Challenge,
        binding: &[u8; BINDING_BYTES],
    ) -> Vec<u8> {
        format!(
            "hello\n{peer_id}\n{}\n{}",
            hex_encode(&challenge.bytes),
            hex_encode(binding)
        )
        .into_bytes()
    }

    /// Verify this hello for the pin the resolver produced on the challenge
    /// this connection issued.
    ///
    /// The echo is checked first, in constant time, so a replayed hello is
    /// refused before the signature is even considered.
    pub fn verify(
        &self,
        public: &PublicKey,
        challenge: &Challenge,
        binding: &[u8; BINDING_BYTES],
    ) -> Result<(), HelloError> {
        if !challenge.matches(&Challenge::from_bytes(self.challenge)) {
            return Err(HelloError::ChallengeMismatch);
        }
        // THE BINDING, BEFORE THE SIGNATURE, IN CONSTANT TIME. A hello lifted from another connection is
        // refused here even when its signature is perfectly valid — which is the whole point: the
        // signature alone never named the channel it was made on.
        let mut difference = 0u8;
        for (received, expected) in self.binding.iter().zip(binding.iter()) {
            difference |= received ^ expected;
        }
        if difference != 0 {
            return Err(HelloError::BindingMismatch);
        }

        let raw = hex_decode(&self.sig).map_err(|error| {
            HelloError::Malformed(format!("the hello signature is not hex: {error}"))
        })?;
        let signature: [u8; SIGNATURE_BYTES] = raw.try_into().map_err(|raw: Vec<u8>| {
            HelloError::Malformed(format!(
                "an Ed25519 signature is {SIGNATURE_BYTES} bytes; this one is {}",
                raw.len()
            ))
        })?;
        // The signature is over the challenge this connection issued, not the
        // one the hello happens to carry: the echo above already proved them
        // equal, and this keeps the verified message tied to the server's
        // fresh value.
        verify_signature(
            public,
            &Self::signed_message(&self.peer_id_hex, challenge, binding),
            &signature,
        )
        .map_err(|error| match error {
            KeyError::InvalidPublicKey => {
                HelloError::Malformed("the resolved pin is not an Ed25519 public key".into())
            }
            _ => HelloError::BadSignature,
        })
    }

    /// Wrap this hello in the frame that carries it.
    pub fn to_frame(&self) -> Result<Frame, HelloError> {
        let payload =
            serde_json::to_value(self).map_err(|error| HelloError::Malformed(error.to_string()))?;
        Ok(Frame {
            kind: Kind::Hello,
            // A hello is not a request and has no request id.
            id: 0,
            payload,
        })
    }

    /// Read a hello from a frame, refusing any other kind.
    pub fn from_frame(frame: &Frame) -> Result<Self, HelloError> {
        if frame.kind != Kind::Hello {
            return Err(HelloError::NotFirstFrame);
        }
        serde_json::from_value(frame.payload.clone())
            .map_err(|error| HelloError::Malformed(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed binding for tests that do not care which connection the hello was made on.
    const BINDING: [u8; BINDING_BYTES] = [7u8; BINDING_BYTES];

    fn challenge(byte: u8) -> Challenge {
        Challenge::from_bytes([byte; CHALLENGE_BYTES])
    }

    #[test]
    fn the_signed_message_is_the_documented_bytes() {
        let challenge = challenge(0x0f);

        assert_eq!(
            Hello::signed_message("peer-a", &challenge, &BINDING),
            format!(
                "hello\npeer-a\n{}\n{}",
                "0f".repeat(CHALLENGE_BYTES),
                "07".repeat(BINDING_BYTES)
            )
            .into_bytes(),
            "the binding is the FOURTH line, and it is what pins the signature to one connection"
        );
    }

    #[test]
    fn a_challenge_matches_only_the_same_bytes() {
        assert!(challenge(1).matches(&challenge(1)));
        assert!(!challenge(1).matches(&challenge(2)));
    }

    #[test]
    fn a_fresh_challenge_is_not_the_same_twice() {
        assert_ne!(Challenge::generate().bytes, Challenge::generate().bytes);
    }

    #[test]
    fn a_hello_verifies_for_the_peer_it_names() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);

        hello
            .verify(&identity.public_bytes(), &challenge, &BINDING)
            .expect("the signer's own pin verifies the hello");
    }

    #[test]
    fn a_hello_does_not_verify_for_another_connections_challenge() {
        let identity = Identity::generate();
        let hello = Hello::sign(&identity, "peer-a", &challenge(1), &BINDING);

        assert_eq!(
            hello.verify(&identity.public_bytes(), &challenge(2), &BINDING),
            Err(HelloError::ChallengeMismatch),
            "the challenge belongs to the connection, not to the hello"
        );
    }

    #[test]
    fn a_hello_for_another_peer_id_does_not_verify() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let mut hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);
        hello.peer_id_hex = "peer-b".to_string();

        assert_eq!(
            hello.verify(&identity.public_bytes(), &challenge, &BINDING),
            Err(HelloError::BadSignature),
            "the peer id is covered by the signature"
        );
    }

    #[test]
    fn a_hello_signed_by_another_key_does_not_verify() {
        let allowed = Identity::generate();
        let attacker = Identity::generate();
        let challenge = Challenge::generate();
        let hello = Hello::sign(&attacker, "peer-a", &challenge, &BINDING);

        assert_eq!(
            hello.verify(&allowed.public_bytes(), &challenge, &BINDING),
            Err(HelloError::BadSignature)
        );
    }

    #[test]
    fn a_hello_with_a_malformed_signature_is_refused() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let mut hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);
        hello.sig = "zz".to_string();

        assert!(matches!(
            hello.verify(&identity.public_bytes(), &challenge, &BINDING),
            Err(HelloError::Malformed(_))
        ));
    }

    #[test]
    fn a_hello_without_a_challenge_field_is_refused() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);
        let mut value = serde_json::to_value(&hello).expect("a hello value");
        value
            .as_object_mut()
            .expect("a hello is an object")
            .remove("challenge");

        assert!(
            serde_json::from_value::<Hello>(value).is_err(),
            "a hello that names no challenge cannot be checked against this connection"
        );
    }

    #[test]
    fn unknown_fields_are_refused_by_hello_and_challenge() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);

        let mut hello_value = serde_json::to_value(&hello).expect("a hello value");
        hello_value["extra"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<Hello>(hello_value).is_err(),
            "the hello wire has no room for fields the verifier ignores"
        );

        let mut challenge_value = serde_json::to_value(challenge).expect("a challenge value");
        challenge_value["extra"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<Challenge>(challenge_value).is_err(),
            "the challenge wire has no room for fields the verifier ignores"
        );
    }

    #[test]
    fn a_frame_that_is_not_a_hello_is_refused() {
        let frame = Frame {
            kind: Kind::Ping,
            id: 1,
            payload: serde_json::json!({}),
        };
        assert_eq!(Hello::from_frame(&frame), Err(HelloError::NotFirstFrame));
    }

    #[test]
    fn a_hello_round_trips_through_its_frame() {
        let identity = Identity::generate();
        let challenge = Challenge::generate();
        let hello = Hello::sign(&identity, "peer-a", &challenge, &BINDING);
        let frame = hello.to_frame().expect("a hello frame");

        assert_eq!(frame.kind, Kind::Hello);
        assert_eq!(frame.id, 0);
        assert_eq!(Hello::from_frame(&frame), Ok(hello));
    }

    #[test]
    fn a_challenge_round_trips_through_its_frame() {
        let challenge = Challenge::generate();
        let frame = challenge.to_frame().expect("a challenge frame");

        assert_eq!(frame.kind, Kind::Challenge);
        assert_eq!(frame.id, 0);
        assert_eq!(Challenge::from_frame(&frame), Ok(challenge));

        let not_a_challenge = Frame {
            kind: Kind::Hello,
            id: 0,
            payload: frame.payload,
        };
        assert_eq!(
            Challenge::from_frame(&not_a_challenge),
            Err(HelloError::NotChallenge)
        );
    }

    #[test]
    fn the_resolver_maps_a_peer_id_to_its_pin() {
        let identity = Identity::generate();
        let key = identity.public_bytes();
        let resolver = HashMap::from([("peer-a".to_string(), key)]);

        assert_eq!(resolver.resolve("peer-a"), Some(key));
        assert_eq!(resolver.resolve("peer-b"), None);
    }
}
