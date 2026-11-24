//! Lowercase hex, the one encoding every secret and key in this service uses on the wire.
//!
//! The decoder is deliberately strict about length and accepts either case: a key file written by
//! `openssl rand -hex 32` is lowercase, but an operator pasting a key from another tool may not be,
//! and the bytes are the value.

pub(crate) const SECRET_BYTES: usize = 32;

/// Lowercase hex, no prefix.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("a nibble"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("a nibble"));
    }
    out
}

/// Decode hex of exactly [`SECRET_BYTES`] bytes.
pub(crate) fn decode_32(hex: &str) -> Option<[u8; SECRET_BYTES]> {
    let raw = decode(hex)?;
    raw.try_into().ok()
}

/// Decode hex of exactly 64 bytes: the length of an Ed25519 signature.
pub(crate) fn decode_64(hex: &str) -> Option<[u8; 64]> {
    let raw = decode(hex)?;
    raw.try_into().ok()
}

/// Decode lowercase or uppercase hex of any even length: the grant header is one such blob.
pub(crate) fn decode_bytes(text: &str) -> Option<Vec<u8>> {
    decode(text)
}

fn decode(hex: &str) -> Option<Vec<u8>> {
    let digits = hex.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }

    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_chunks::<2>().0 {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
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
    use super::*;

    #[test]
    fn a_secret_round_trips_and_accepts_either_case() {
        let bytes = [0xab; SECRET_BYTES];
        assert_eq!(encode(&bytes), "ab".repeat(SECRET_BYTES));
        assert_eq!(decode_32(&encode(&bytes)), Some(bytes));
        assert_eq!(decode_32(&encode(&bytes).to_uppercase()), Some(bytes));
    }

    #[test]
    fn a_secret_of_the_wrong_length_or_alphabet_is_refused() {
        assert_eq!(decode_32(""), None);
        assert_eq!(decode_32("abc"), None);
        assert_eq!(decode_32(&"zz".repeat(32)), None);
        assert_eq!(decode_32(&"00".repeat(31)), None);
        assert_eq!(decode_32(&"00".repeat(33)), None);
    }

    #[test]
    fn a_signature_is_exactly_sixty_four_bytes() {
        assert_eq!(decode_64(&"ab".repeat(64)), Some([0xab; 64]));
        assert_eq!(decode_64(&"ab".repeat(63)), None);
        assert_eq!(decode_64(&"ab".repeat(65)), None);
    }
}
