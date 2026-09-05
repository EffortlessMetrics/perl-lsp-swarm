//! Hashing utilities shared by core crates and workspace tools.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// Compute a SHA-256 digest and return it as an algorithm-tagged hex string.
///
/// This is the workspace's collision-resistant content digest: the
/// `sha256:` prefix names the algorithm on the wire, so consumers can
/// version-stamp or reject digests whose algorithm changes. Use it wherever
/// a value substitutes for identity across processes; reserve FNV-1a
/// ([`fnv1a64_hex`]) for non-adversarial locality such as cache keys.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("sha256:{hex}")
}

/// Compute an FNV-1a 64-bit hash of `bytes`.
///
/// Deterministic and process-safe: the same bytes hash to the same value in
/// every process, which makes it suitable for bounded retention fingerprints
/// where the raw payload must not be retained (#9769).
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Compute an FNV-1a 64-bit hash and return it as a tagged hex string.
#[must_use]
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(bytes))
}

#[cfg(test)]
mod tests {
    use super::{fnv1a64_hex, sha256_hex};
    use std::error::Error;

    #[test]
    fn sha256_hex_matches_known_vectors() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            sha256_hex(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        Ok(())
    }

    #[test]
    fn sha256_hex_is_deterministic_and_separates_inputs() -> Result<(), Box<dyn Error>> {
        assert_eq!(sha256_hex(b"abc"), sha256_hex(b"abc"));
        assert_ne!(sha256_hex(b"abc"), sha256_hex(b"abd"));
        Ok(())
    }

    #[test]
    fn fnv1a64_hex_matches_known_vectors() -> Result<(), Box<dyn Error>> {
        assert_eq!(fnv1a64_hex(b""), "fnv1a64:cbf29ce484222325");
        assert_eq!(fnv1a64_hex(b"hello"), "fnv1a64:a430d84680aabd0b");
        Ok(())
    }

    #[test]
    fn fnv1a64_hex_is_deterministic() -> Result<(), Box<dyn Error>> {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"hello");
        assert_eq!(h1, h2);
        Ok(())
    }

    #[test]
    fn fnv1a64_hex_differs_on_different_input() -> Result<(), Box<dyn Error>> {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"world");
        assert_ne!(h1, h2);
        Ok(())
    }
}
