use sha2::{Digest, Sha256};

const DIGEST_PREFIX: &str = "sha256:";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// SHA-256 of exact fixture bytes, rendered as `sha256:` plus lowercase hex.
#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut rendered = String::with_capacity(DIGEST_PREFIX.len() + digest.len() * 2);
    rendered.push_str(DIGEST_PREFIX);
    for byte in digest {
        rendered.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        rendered.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    rendered
}
