//! The integrity primitive the component loader and the boxed manifest both need.

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Lowercase hex sha256 of a byte string.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Lowercase hex sha256 of a file's bytes. Read whole, exactly as `sha256File` does.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let data = std::fs::read(path)?;
    Ok(sha256_bytes(&data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_known_vector_for_empty_input() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
