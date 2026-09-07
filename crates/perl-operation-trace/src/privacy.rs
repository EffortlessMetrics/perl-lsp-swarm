//! Field privacy classification and the two non-public value wrappers.
//!
//! Three tiers, modeled on `perl-subprocess-runtime`'s process-identity
//! module (`crates/perl-subprocess-runtime/src/process/identity.rs`, whose
//! `PrivatePath`/`PrivateBytes` vs. `SecretValue` split is the closest
//! existing idiom in the repository):
//!
//! - [`FieldPrivacy::Public`] — appears verbatim in a serialized trace.
//! - [`FieldPrivacy::Private`] ([`PrivateValue`]) — the raw value is never
//!   serialized, but its byte length is (`<redacted:N bytes>`).
//!   Appropriate for high-entropy content such as a host path or a line of
//!   source text, where distinct values are worth telling apart by size.
//! - [`FieldPrivacy::Secret`] ([`SecretField`]) — the raw value is never
//!   serialized and *nothing about it is*, not even a length. Appropriate
//!   for a low-entropy secret such as a token or password, where even a byte
//!   count would help narrow a guess.
//!
//! # This crate never fingerprints
//!
//! `perl-subprocess-runtime`'s fingerprinted tier (`PrivatePath`,
//! `PrivateBytes`) additionally exposes a digest of the redacted value, so
//! two different high-entropy inputs stay distinguishable without exposing
//! either. That requires a hash function. This crate deliberately has none
//! — see `tests/dependency_contract.rs`, which forbids `sha2` outright — so
//! fingerprinting a private or secret value here is not just undone, it is
//! unreachable. A producer that needs that property owns computing and
//! carrying its own digest; this crate's `Private` tier means "the raw value
//! never leaves this crate serialized, and its length may", full stop.

use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

/// Which privacy tier a field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldPrivacy {
    /// Appears verbatim in a serialized trace.
    Public,
    /// Redacted; byte length disclosed.
    Private,
    /// Redacted; nothing disclosed, not even a length.
    Secret,
}

impl fmt::Display for FieldPrivacy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Secret => "secret",
        };
        f.write_str(s)
    }
}

/// A high-entropy value that must never be serialized in the clear, but
/// whose byte length is not itself sensitive.
///
/// `Debug` and `Serialize` both emit `<redacted:N bytes>` — never the
/// content. The redaction holds through `serde_json::to_string`, not only
/// through `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct PrivateValue(String);

impl PrivateValue {
    /// Wrap a value as private.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the underlying value.
    ///
    /// Callers that expose the result publicly are responsible for the leak;
    /// the name is deliberately awkward.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The number of bytes in the wrapped value, which is not itself
    /// private.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the wrapped value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for PrivateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateValue(<redacted:{} bytes>)", self.0.len())
    }
}

impl Serialize for PrivateValue {
    /// Emits the redaction placeholder — never the wrapped content.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("<redacted:{} bytes>", self.0.len()))
    }
}

/// A low-entropy secret that must never be serialized, fingerprinted, or
/// have any property of its content disclosed — not even a length.
///
/// Named `SecretField` rather than reusing `perl-subprocess-runtime`'s
/// `SecretValue` name, even though the shape and reasoning are the same, so
/// a reader does not mistake the two for one shared type: `SecretValue`
/// belongs to the process domain, `SecretField` to `operation_trace.v1`.
///
/// A digest of a low-entropy secret is a guessable secret, and even a byte
/// count narrows a guess (`<redacted:4 bytes>` all but announces a PIN) — so,
/// unlike [`PrivateValue`], nothing about the content survives redaction.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretField(String);

impl SecretField {
    /// Wrap a value as secret.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the underlying value.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The number of bytes in the wrapped value.
    ///
    /// This accessor exists for the owner that already holds the plaintext
    /// (for example to enforce a maximum secret length before wrapping), not
    /// for a consumer of a redacted trace — `Debug` and `Serialize`
    /// deliberately do not expose it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the wrapped value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretField(<redacted>)")
    }
}

impl Serialize for SecretField {
    /// Emits a fixed redaction placeholder with no length or other content
    /// property — never the wrapped content.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // ── FieldPrivacy ──────────────────────────────────────────────────────

    #[test]
    fn field_privacy_serde_round_trip() {
        for privacy in [FieldPrivacy::Public, FieldPrivacy::Private, FieldPrivacy::Secret] {
            let json = serde_json::to_string(&privacy).unwrap();
            let back: FieldPrivacy = serde_json::from_str(&json).unwrap();
            assert_eq!(privacy, back);
        }
    }

    #[test]
    fn field_privacy_display() {
        assert_eq!(FieldPrivacy::Public.to_string(), "public");
        assert_eq!(FieldPrivacy::Private.to_string(), "private");
        assert_eq!(FieldPrivacy::Secret.to_string(), "secret");
    }

    // ── PrivateValue ──────────────────────────────────────────────────────

    #[test]
    fn private_value_debug_is_redacted_with_length() {
        let v = PrivateValue::new("/home/alice/.ssh/id_rsa");
        let debug = format!("{v:?}");
        assert!(!debug.contains("alice"), "raw content must not appear in Debug");
        assert!(debug.contains("<redacted:23 bytes>"), "got: {debug}");
    }

    #[test]
    fn private_value_serialize_is_redacted_with_length() {
        let v = PrivateValue::new("/home/alice/.ssh/id_rsa");
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("alice"), "raw content must not appear in JSON");
        assert!(json.contains("23 bytes"), "got: {json}");
    }

    #[test]
    fn private_value_length_varies_with_content() {
        let short = PrivateValue::new("ab");
        let long = PrivateValue::new("abcdefgh");
        assert_ne!(
            serde_json::to_string(&short).unwrap(),
            serde_json::to_string(&long).unwrap(),
            "distinct lengths must serialize distinctly for the Private tier"
        );
    }

    // ── SecretField ───────────────────────────────────────────────────────

    #[test]
    fn secret_field_debug_has_no_length() {
        let s = SecretField::new("sk-abcdef0123456789");
        let debug = format!("{s:?}");
        assert!(!debug.contains("sk-abcdef"), "raw content must not appear in Debug");
        assert!(!debug.contains("19"), "byte length must not appear in Debug: {debug}");
        assert_eq!(debug, "SecretField(<redacted>)");
    }

    #[test]
    fn secret_field_serialize_has_no_length() {
        let s = SecretField::new("sk-abcdef0123456789");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"<redacted>\"", "no length must leak into the serialized form");
    }

    #[test]
    fn secret_field_serialization_is_identical_regardless_of_length() {
        let short = SecretField::new("a");
        let long = SecretField::new("a very much longer low-entropy secret value indeed");
        assert_eq!(
            serde_json::to_string(&short).unwrap(),
            serde_json::to_string(&long).unwrap(),
            "unlike PrivateValue, a Secret's serialized form must not vary with its length"
        );
    }

    #[test]
    fn secret_field_len_accessor_is_available_to_the_owner_but_not_serialized() {
        let s = SecretField::new("abc");
        assert_eq!(s.len(), 3);
        assert!(!s.is_empty());
        assert!(!serde_json::to_string(&s).unwrap().contains('3'));
    }
}
