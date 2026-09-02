//! Fixture digest helpers for the critic rule-proof manifest.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use super::error::ProofError;

/// SHA-256 digest of one fixture file, encoded as `sha256:` plus lowercase hex.
pub fn file_digest(path: &Path) -> Result<String, ProofError> {
    let bytes = fs::read(path).map_err(|error| {
        ProofError::new(format!("{}: cannot read fixture: {error}", path.display()))
    })?;
    Ok(digest_bytes(&bytes))
}

pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity("sha256:".len() + 64);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}
