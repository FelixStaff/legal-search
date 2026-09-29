//! Stable document identity: SHA-256 of the exact PDF bytes, lowercase hex.
//! The extension computes it with WebCrypto; this implementation is the reference
//! used by tests and tools to prove both sides agree.

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::is_valid_sha256;

    #[test]
    fn known_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn output_is_valid_id_and_sensitive_to_any_byte() {
        let a = sha256_hex(b"%PDF-1.4 hello");
        let b = sha256_hex(b"%PDF-1.4 hellp");
        assert!(is_valid_sha256(&a));
        assert_ne!(a, b);
        assert_eq!(a, sha256_hex(b"%PDF-1.4 hello"));
    }
}
