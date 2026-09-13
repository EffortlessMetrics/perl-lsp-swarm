//! Field privacy classification and the two non-public value wrappers.
//!
//! Three tiers, modeled on `perl-subprocess-runtime`'s process-identity
//! module (`crates/perl-subprocess-runtime/src/process/identity.rs`, whose
//! `PrivatePath`/`PrivateBytes` vs. `SecretValue` split is the closest
//! existing idiom in the repository):
//!
//! - [`FieldPrivacy::Public`] — appears verbatim in a serialized trace.
//! - [`FieldPrivacy::Private`] ([`PrivateValue`]) — the raw value is never
//!   serialized, but its byte length is (`<redacted:N bytes>`). Appropriate
//!   for content that is both (a) not meant to appear verbatim and (b)
//!   genuinely high-entropy enough that distinct values are worth telling
//!   apart by size alone — a host path is the crate's working example.
//!   **A line of Perl source text is deliberately *not* offered as an
//!   example here.** Real source lines are frequently low-entropy — `}`,
//!   `1;`, `);`, or blank are common — and a low-entropy value is exactly
//!   what byte length alone can identify, which is this crate's own stated
//!   reason to prefer `Secret` (below). This crate's own registry classifies
//!   its `source_line` field as `Secret`, not `Private`, precisely because
//!   of this: see `crate::registry`.
//! - [`FieldPrivacy::Secret`] ([`SecretField`]) — the raw value is never
//!   serialized and *nothing about it is*, not even a length. Appropriate
//!   for low-entropy content such as a token, a password, or (per the
//!   correction above) a line of source text, where even a byte count would
//!   help narrow a guess.
//!
//! # This crate does not fold identity into a digest — and cannot enforce
//! # that a caller won't
//!
//! `perl-subprocess-runtime`'s fingerprinted tier (`PrivatePath`,
//! `PrivateBytes`) additionally exposes a digest of the redacted value, so
//! two different high-entropy inputs stay distinguishable without exposing
//! either. That requires a hash function. What is actually proven here, and
//! no more:
//!
//! - this crate's own dependency closure contains no hash function
//!   (`tests/dependency_contract.rs` forbids `sha2`, fail-closed);
//! - no API in this crate folds a [`PrivateValue`], [`SecretField`], or
//!   `OperationId` into a digest or fingerprint;
//! - `tests/negative_control.rs` proves recorded event payloads are
//!   independent of which operation recorded them.
//!
//! Absence of a `sha2` dependency does **not** make fingerprinting
//! unreachable in any absolute sense: a hash or digest can be implemented in
//! ordinary safe Rust with no dependency at all, and nothing stops a
//! downstream caller from computing one over a value it already holds in
//! the clear (its own copy of a secret before wrapping it, or
//! `OperationId::as_wire`'s exposed string). The dependency allowlist proves
//! this crate's own dependency *closure*, not the absence of every possible
//! digest computation reachable from a caller. A producer that needs a
//! fingerprinted tier owns computing and carrying that digest itself; this
//! crate's `Private` tier means "the raw value never leaves this crate
//! serialized, and its length may", full stop, and keeping ephemeral or
//! redacted identity out of a durable digest is a contract obligation this
//! crate places on its consumers, not a property this crate can enforce for
//! them.

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

    /// The exact rendered redaction placeholder for a value of the given
    /// byte length. The sole source of truth for this text: both
    /// [`Self::approx_serialized_len`] and the `Serialize` impl below call
    /// this, so the two cannot drift apart.
    fn redacted_placeholder(byte_len: usize) -> String {
        format!("<redacted:{byte_len} bytes>")
    }

    /// The exact byte length of this value's serialized redacted form
    /// (`<redacted:N bytes>`), used by [`crate::OperationRecorder`]'s
    /// `max_payload_bytes` accounting so that budget tracks what this value
    /// actually serializes to, not its private plaintext's own length (which
    /// would be a wrong-arithmetic bug, though not a redaction leak, since
    /// `Private` already discloses its length by design).
    pub(crate) fn approx_serialized_len(&self) -> usize {
        Self::redacted_placeholder(self.0.len()).len()
    }
}

impl fmt::Debug for PrivateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateValue({})", Self::redacted_placeholder(self.0.len()))
    }
}

impl Serialize for PrivateValue {
    /// Emits the redaction placeholder — never the wrapped content.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&Self::redacted_placeholder(self.0.len()))
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
/// unlike [`PrivateValue`], nothing about the content survives redaction —
/// **including in [`crate::OperationRecorder`]'s own budget accounting**: see
/// [`Self::approx_serialized_len`].
#[derive(Clone, PartialEq, Eq)]
pub struct SecretField(String);

/// The fixed redaction placeholder [`SecretField`] serializes to. The sole
/// source of truth for this text: both the `Serialize` impl below and
/// [`SecretField::approx_serialized_len`] use this constant, so the two
/// cannot drift apart.
const SECRET_REDACTED_PLACEHOLDER: &str = "<redacted>";

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

    /// The byte length of this value's serialized redacted form — always
    /// [`SECRET_REDACTED_PLACEHOLDER`]'s fixed length, **never** a function
    /// of this value's own plaintext length.
    ///
    /// This is the fix for a real length side channel: [`crate::OperationRecorder`]'s
    /// `max_payload_bytes` budget previously charged a `Secret` field's
    /// *plaintext* length, so whether an event was admitted or truncated
    /// depended on the secret's length — exactly the property `Secret`
    /// exists to keep an observer from learning. Charging this fixed
    /// constant instead means two secrets of different lengths cost the
    /// budget identically, matching what they actually serialize to.
    pub(crate) fn approx_serialized_len(&self) -> usize {
        SECRET_REDACTED_PLACEHOLDER.len()
    }
}

impl fmt::Debug for SecretField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretField({SECRET_REDACTED_PLACEHOLDER})")
    }
}

impl Serialize for SecretField {
    /// Emits a fixed redaction placeholder with no length or other content
    /// property — never the wrapped content.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(SECRET_REDACTED_PLACEHOLDER)
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
