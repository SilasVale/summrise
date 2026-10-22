//! The mesh's self-signed certificate.
//!
//! TLS speaks certificates, not raw keys. The certificate is only an envelope:
//! its SubjectPublicKeyInfo carries the identity's Ed25519 public key, and the
//! peer's verifier compares those 32 bytes with the pin. There is no CA, no
//! DNS name, no expiry, and nothing else in the certificate is trusted.
//!
//! The SPKI is found by walking the DER to the *actual* `subjectPublicKeyInfo`
//! field — certificate → tbsCertificate → the field after subject — rather
//! than by searching the bytes for a shape. A search would accept a key buried
//! in an extension; the walk cannot.

use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use rcgen::{CertificateParams, DistinguishedName, DnType, PublicKeyData};
use rustls::pki_types::CertificateDer;

use crate::keys::{Identity, PUBLIC_KEY_BYTES};

/// Why a certificate could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertError {
    /// rcgen refused the parameters or the signature.
    Generate(String),
    /// The DER ended in the middle of a value.
    Truncated,
    /// The DER was structurally valid but not the shape an X.509 certificate
    /// or an Ed25519 SubjectPublicKeyInfo must have.
    Malformed(&'static str),
}

impl std::fmt::Display for CertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generate(message) => write!(f, "could not build the mesh certificate: {message}"),
            Self::Truncated => f.write_str("the certificate's DER is truncated"),
            Self::Malformed(what) => write!(f, "not a valid mesh certificate: {what}"),
        }
    }
}

impl std::error::Error for CertError {}

/// The certificate's Ed25519 public key, in the 32-byte form a peer pins.
pub fn public_key(cert: &CertificateDer<'_>) -> Result<[u8; PUBLIC_KEY_BYTES], CertError> {
    let der = cert.as_ref();
    let (certificate, consumed) = read_tlv(der, 0)?;
    if certificate.tag != 0x30 {
        return Err(CertError::Malformed("a certificate is a SEQUENCE"));
    }
    if consumed != der.len() {
        return Err(CertError::Malformed("trailing bytes after the certificate"));
    }

    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
    let (tbs, next) = read_tlv(certificate.content, 0)?;
    if tbs.tag != 0x30 {
        return Err(CertError::Malformed("tbsCertificate is a SEQUENCE"));
    }
    let (algorithm, next) = read_tlv(certificate.content, next)?;
    if algorithm.tag != 0x30 {
        return Err(CertError::Malformed("signatureAlgorithm is a SEQUENCE"));
    }
    let (signature, end) = read_tlv(certificate.content, next)?;
    if signature.tag != 0x03 {
        return Err(CertError::Malformed("signatureValue is a BIT STRING"));
    }
    if end != certificate.content.len() {
        return Err(CertError::Malformed("trailing bytes after signatureValue"));
    }

    // TBSCertificate ::= SEQUENCE {
    //   [0] version OPTIONAL, serialNumber, signature, issuer, validity,
    //   subject, subjectPublicKeyInfo, ... }
    let mut offset = 0;
    let (first, after_first) = read_tlv(tbs.content, offset)?;
    if first.tag == 0xa0 {
        offset = after_first;
    }
    let (serial, after_serial) = read_tlv(tbs.content, offset)?;
    if serial.tag != 0x02 {
        return Err(CertError::Malformed("serialNumber is an INTEGER"));
    }
    offset = after_serial;
    // signature, issuer, validity, subject — each exactly one SEQUENCE.
    for _ in 0..4 {
        let (field, next) = read_tlv(tbs.content, offset)?;
        if field.tag != 0x30 {
            return Err(CertError::Malformed("a TBSCertificate name is a SEQUENCE"));
        }
        offset = next;
    }
    let (spki, end) = read_tlv(tbs.content, offset)?;
    if spki.tag != 0x30 {
        return Err(CertError::Malformed("subjectPublicKeyInfo is a SEQUENCE"));
    }
    if end != tbs.content.len() {
        return Err(CertError::Malformed(
            "trailing bytes after subjectPublicKeyInfo",
        ));
    }

    // SubjectPublicKeyInfo ::= SEQUENCE { algorithm, subjectPublicKey }
    let (algorithm, next) = read_tlv(spki.content, 0)?;
    if algorithm.tag != 0x30 {
        return Err(CertError::Malformed("AlgorithmIdentifier is a SEQUENCE"));
    }
    let (oid, after_oid) = read_tlv(algorithm.content, 0)?;
    if oid.tag != 0x06 || oid.content != ED25519_OID {
        return Err(CertError::Malformed(
            "the subject public key is not Ed25519",
        ));
    }
    if after_oid != algorithm.content.len() {
        return Err(CertError::Malformed(
            "Ed25519 takes no algorithm parameters",
        ));
    }
    let (key, end) = read_tlv(spki.content, next)?;
    if key.tag != 0x03 {
        return Err(CertError::Malformed("subjectPublicKey is a BIT STRING"));
    }
    if end != spki.content.len() {
        return Err(CertError::Malformed(
            "trailing bytes after subjectPublicKey",
        ));
    }
    if key.content.len() != 1 + PUBLIC_KEY_BYTES || key.content[0] != 0 {
        return Err(CertError::Malformed(
            "an Ed25519 public key is 32 bytes with no unused bits",
        ));
    }
    let mut public = [0u8; PUBLIC_KEY_BYTES];
    public.copy_from_slice(&key.content[1..]);
    Ok(public)
}

/// The DER OID of Ed25519, 1.3.101.112.
const ED25519_OID: &[u8] = &[0x2b, 0x65, 0x70];

/// One DER tag-length-value, with `content` borrowing the input.
struct Tlv<'a> {
    tag: u8,
    content: &'a [u8],
}

/// Read one TLV at `offset` and return it alongside the offset just past it.
///
/// Only the forms an X.509 certificate can carry are accepted: definite
/// lengths, at most four length bytes, no indefinite-length BER. A truncated
/// or impossible length is a typed error, never a guess.
fn read_tlv(buf: &[u8], offset: usize) -> Result<(Tlv<'_>, usize), CertError> {
    if offset + 2 > buf.len() {
        return Err(CertError::Truncated);
    }
    let tag = buf[offset];
    let first = buf[offset + 1];
    let (length, header) = if first < 0x80 {
        (first as usize, 2)
    } else if first == 0x80 {
        return Err(CertError::Malformed("indefinite lengths are not DER"));
    } else {
        let count = (first & 0x7f) as usize;
        if count > 4 {
            return Err(CertError::Malformed("a DER length of more than four bytes"));
        }
        if offset + 2 + count > buf.len() {
            return Err(CertError::Truncated);
        }
        let mut length = 0usize;
        for index in 0..count {
            length = (length << 8) | buf[offset + 2 + index] as usize;
        }
        (length, 2 + count)
    };
    let start = offset + header;
    let end = start
        .checked_add(length)
        .ok_or(CertError::Malformed("a DER length that overflows"))?;
    if end > buf.len() {
        return Err(CertError::Truncated);
    }
    Ok((
        Tlv {
            tag,
            content: &buf[start..end],
        },
        end,
    ))
}

/// Build a fresh self-signed certificate carrying `identity`'s public key.
///
/// It is rebuilt per call: the identity is the durable fact, and a long-lived
/// certificate would add an expiry and a rotation story the design does not
/// want.
pub fn self_signed(identity: &Identity) -> Result<CertificateDer<'static>, CertError> {
    let key = MeshSigningKey {
        dalek: DalekSigningKey::from_bytes(&identity.private_bytes()),
        public: identity.public_bytes(),
    };
    let mut params = CertificateParams::new(Vec::new())
        .map_err(|error| CertError::Generate(error.to_string()))?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "summrise-mesh");
    params.distinguished_name = name;

    let certificate = params
        .self_signed(&key)
        .map_err(|error| CertError::Generate(error.to_string()))?;
    Ok(certificate.der().clone())
}

/// rcgen's view of the identity key.
///
/// rcgen 0.14 has no "here are 32 raw bytes" constructor; its public traits
/// take exactly those bytes, so the mesh implements the traits and signs with
/// ed25519-dalek directly. One signing implementation, no PKCS#8 round trip.
struct MeshSigningKey {
    dalek: DalekSigningKey,
    /// Kept because `PublicKeyData::der_bytes` returns a borrow of it.
    public: [u8; PUBLIC_KEY_BYTES],
}

impl PublicKeyData for MeshSigningKey {
    fn der_bytes(&self) -> &[u8] {
        &self.public
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ED25519
    }
}

impl rcgen::SigningKey for MeshSigningKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        Ok(self.dalek.sign(message).to_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_certificate_carries_the_identitys_public_key() {
        // This is the whole point of the certificate: the bytes the verifier
        // compares against the pin are the identity's own public key.
        let identity = Identity::generate();
        let cert = self_signed(&identity).expect("a certificate");

        assert_eq!(
            public_key(&cert).expect("an Ed25519 spki"),
            identity.public_bytes()
        );
    }

    #[test]
    fn two_certificates_from_one_identity_carry_the_same_key() {
        let identity = Identity::generate();
        let first = self_signed(&identity).expect("a certificate");
        let second = self_signed(&identity).expect("a certificate");

        assert_eq!(
            public_key(&first).expect("an Ed25519 spki"),
            public_key(&second).expect("an Ed25519 spki"),
        );
    }

    #[test]
    fn two_identities_carry_different_keys() {
        let first = Identity::generate();
        let second = Identity::generate();
        let first_cert = self_signed(&first).expect("a certificate");
        let second_cert = self_signed(&second).expect("a certificate");

        assert_ne!(
            public_key(&first_cert).expect("an Ed25519 spki"),
            public_key(&second_cert).expect("an Ed25519 spki"),
        );
    }

    #[test]
    fn a_truncated_certificate_is_refused() {
        let identity = Identity::generate();
        let cert = self_signed(&identity).expect("a certificate");
        let cut = &cert.as_ref()[..cert.as_ref().len() / 2];

        assert_eq!(
            public_key(&CertificateDer::from(cut.to_vec())),
            Err(CertError::Truncated)
        );
    }

    #[test]
    fn a_certificate_without_an_ed25519_spki_is_refused() {
        // A syntactically valid DER SEQUENCE that is not a certificate: the
        // walk must refuse it rather than fall back to searching the bytes.
        let not_a_certificate = CertificateDer::from(vec![0x30, 0x01, 0x00]);
        assert!(public_key(&not_a_certificate).is_err());
    }
}
