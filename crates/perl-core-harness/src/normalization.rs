//! Canonical value encodings shared by the harness domain modules.
//!
//! These helpers own the byte representation of every digest the harness
//! publishes, so they must stay free of domain-specific framing.

use sha2::{Digest, Sha256};

/// Render a `sha256:`-prefixed digest of the supplied bytes.
pub(crate) fn sha256_digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_lower(&Sha256::digest(bytes)))
}

/// Render bytes as lower-case hexadecimal.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}
