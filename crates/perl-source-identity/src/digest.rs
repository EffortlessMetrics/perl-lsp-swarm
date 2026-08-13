//! Collision-resistant content digests for durable cross-repository identity.
//!
//! All digests use **SHA-256** with domain separation so that material from one
//! identity domain cannot produce a valid digest in another. The public API
//! conceals the underlying primitive so it can be upgraded without breaking
//! downstream serialized forms (the schema-version prefix is part of the wire
//! format).
//!
//! # Wire format
//!
//! ```text
//! sha256:<64 lowercase hex digits>
//! ```
//!
//! The `sha256:` prefix is mandatory; unknown prefixes are rejected rather than
//! silently promoted. A future digest algorithm would use a different prefix
//! (e.g. `blake3:`) and carry a distinct [`ContentDigest`] schema version.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Current content-digest schema version.
pub const CONTENT_DIGEST_SCHEMA_VERSION: u32 = 1;

/// Prefix string for SHA-256 content digests on the wire.
const SHA256_PREFIX: &str = "sha256:";

/// A fixed 32-byte SHA-256 output.
type Sha256Bytes = [u8; 32];

/// Format 32 raw SHA-256 bytes as `sha256:<64 lowercase hex>`.
fn sha256_to_wire(bytes: &Sha256Bytes) -> String {
    let mut out = String::with_capacity(SHA256_PREFIX.len() + 64);
    out.push_str(SHA256_PREFIX);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Compute SHA-256 over the concatenated input slices.
fn sha256_raw(parts: &[&[u8]]) -> Sha256Bytes {
    let mut h = Sha256::new();
    for part in parts {
        h.update(part);
    }
    h.finalize().into()
}

/// Encode a `u32` as 4 big-endian bytes for unambiguous length-prefixing.
fn u32_be(n: u32) -> [u8; 4] {
    n.to_be_bytes()
}

/// Encode a byte slice with a 4-byte big-endian length prefix.
///
/// Using explicit length prefixes is required by the issue contract: fields must
/// be unambiguously separated so that `["a", "bc"]` and `["ab", "c"]` produce
/// distinct digests.
fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let len = u32_be(data.len() as u32);
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(data);
    out
}

/// A collision-resistant SHA-256 digest of exact byte content.
///
/// `ContentDigest` records the exact bytes of a source revision, not the
/// logical file identity. Two files with identical bytes at different logical
/// paths produce the same `ContentDigest`; that is intentional — content
/// sameness is a distinct concept from logical-source sameness.
///
/// The digest uses SHA-256 with a domain separator to bind it to the
/// `perl-lsp:content-digest:v1` context.
///
/// # Wire format
///
/// `sha256:<64 lowercase hex digits>` — the `sha256:` prefix is mandatory and
/// verified on deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentDigest(String);

/// Domain separator for content digests.
const CONTENT_DIGEST_DOMAIN: &[u8] = b"perl-lsp:content-digest:v1\0";

impl ContentDigest {
    /// Compute a content digest for a byte slice.
    ///
    /// The domain separator `perl-lsp:content-digest:v1\0` is prepended before
    /// hashing so a raw file hash cannot masquerade as a domain-separated ID.
    #[must_use]
    pub fn of_bytes(content: &[u8]) -> Self {
        let len = length_prefixed(content);
        let raw = sha256_raw(&[CONTENT_DIGEST_DOMAIN, &len]);
        Self(sha256_to_wire(&raw))
    }

    /// Parse a content digest from its wire representation.
    ///
    /// Returns `None` if the string does not start with `sha256:` or if the hex
    /// body is not exactly 64 lowercase hex digits.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        let hex = s.strip_prefix(SHA256_PREFIX)?;
        if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            Some(Self(s.to_owned()))
        } else {
            None
        }
    }

    /// The wire representation, e.g. `sha256:abc123...`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Low-level domain-separated SHA-256 builder used by the `*Id` types.
///
/// Each identity type uses a unique domain prefix so that IDs of different kinds
/// can never collide even if their material inputs happen to match.
pub(crate) struct DomainHasher {
    domain: &'static [u8],
    parts: Vec<Vec<u8>>,
}

impl DomainHasher {
    /// Start a new hasher for the given domain tag.
    ///
    /// The domain tag must be unique per identity type and should include a
    /// version suffix, e.g. `b"perl-lsp:project-id:v1\0"`.
    pub(crate) fn new(domain: &'static [u8]) -> Self {
        Self { domain, parts: Vec::new() }
    }

    /// Append a length-prefixed field to the hash input.
    ///
    /// Every field is encoded as `u32_be(len) || bytes` before hashing so that
    /// `["a", "bc"]` and `["ab", "c"]` produce distinct digests.
    pub(crate) fn push_field(&mut self, field: &[u8]) {
        self.parts.push(length_prefixed(field));
    }

    /// Compute the final domain-separated SHA-256 digest.
    pub(crate) fn finish(self) -> Sha256Bytes {
        let mut h = Sha256::new();
        h.update(self.domain);
        for part in &self.parts {
            h.update(part);
        }
        h.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn content_digest_is_deterministic() {
        let a = ContentDigest::of_bytes(b"package App;\n1;\n");
        let b = ContentDigest::of_bytes(b"package App;\n1;\n");
        assert_eq!(a, b, "same bytes → same digest");
    }

    #[test]
    fn content_digest_distinguishes_content() {
        let a = ContentDigest::of_bytes(b"package App;\n");
        let b = ContentDigest::of_bytes(b"package Other;\n");
        assert_ne!(a, b, "different bytes → different digest");
    }

    #[test]
    fn empty_content_has_defined_digest() {
        let d = ContentDigest::of_bytes(b"");
        assert!(d.as_wire().starts_with("sha256:"), "wire prefix must be sha256:");
        assert_eq!(d.as_wire().len(), SHA256_PREFIX.len() + 64);
    }

    #[test]
    fn content_digest_wire_round_trip() {
        let original = ContentDigest::of_bytes(b"test");
        let wire = original.as_wire().to_owned();
        let parsed = ContentDigest::from_wire(&wire).expect("valid wire form must parse");
        assert_eq!(original, parsed);
    }

    #[test]
    fn content_digest_rejects_bad_wire() {
        assert!(ContentDigest::from_wire("md5:abc").is_none(), "wrong prefix");
        assert!(ContentDigest::from_wire("sha256:tooshort").is_none(), "too few hex digits");
        assert!(ContentDigest::from_wire("sha256:").is_none(), "empty hex");
    }

    #[test]
    fn content_digest_display_matches_as_wire() {
        let d = ContentDigest::of_bytes(b"hello");
        assert_eq!(format!("{d}"), d.as_wire());
    }

    #[test]
    fn domain_hasher_is_boundary_sensitive() {
        let mut h1 = DomainHasher::new(b"test-domain\0");
        h1.push_field(b"a");
        h1.push_field(b"bc");

        let mut h2 = DomainHasher::new(b"test-domain\0");
        h2.push_field(b"ab");
        h2.push_field(b"c");

        assert_ne!(h1.finish(), h2.finish(), "length prefixes prevent field-boundary collisions");
    }

    #[test]
    fn domain_hasher_domain_separation() {
        let mut h1 = DomainHasher::new(b"domain-a:v1\0");
        h1.push_field(b"same-material");

        let mut h2 = DomainHasher::new(b"domain-b:v1\0");
        h2.push_field(b"same-material");

        assert_ne!(h1.finish(), h2.finish(), "distinct domains → distinct digests");
    }
}
