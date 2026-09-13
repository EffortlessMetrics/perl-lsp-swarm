//! Explicit, caller-supplied session identity.
//!
//! [`SessionId`] is deliberately **not** minted by this crate. Ambient
//! entropy (a random seed, a PID, a wall-clock timestamp) is the caller's
//! concern: a production caller supplies a real session label (for example a
//! process-lifetime UUID it generated itself), and a test supplies a fixed
//! literal so that `operation_trace.v1` fixtures stay deterministic (#4853).
//! There is no `SessionId::new_random()` here and there will not be one —
//! keeping that ambient decision above this crate is what keeps this crate
//! itself fully deterministic and testable.
//!
//! # `SessionId` is outside the three-tier privacy model
//!
//! [`crate::EventFieldValue`]'s three privacy tiers
//! ([`crate::FieldPrivacy::Public`]/[`crate::FieldPrivacy::Private`]/
//! [`crate::FieldPrivacy::Secret`], see `crate::privacy`) apply only to event
//! field values. `SessionId` is not an event field: it is embedded verbatim
//! into [`crate::OperationId::as_wire`] — which is that type's `Serialize`
//! impl — and appears in the `Display` text of several
//! [`crate::RecordError`] variants, both of which are places a caller's
//! serialized trace or a log line built from an error message reads
//! unconditionally, with no tier to opt into. This module therefore
//! validates a `SessionId` for hygiene (see below), but that validation
//! carries **no redaction guarantee at all**: a caller that passes an
//! API-key-shaped, secret-shaped, or otherwise sensitive string as a session
//! label gets that string back verbatim in every wire form and error message
//! this crate produces. Never construct a `SessionId` from a value that
//! would need `Secret`- or `Private`-tier treatment if it were an event
//! field.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// The maximum byte length of a [`SessionId`] label.
///
/// A session label is embedded verbatim into every [`crate::OperationId`]
/// this crate mints under it and into this crate's error text; an unbounded
/// label would give a caller (or a bug) an unbounded-size string with no
/// observable signal, mirroring why [`crate::RecorderBounds`] and
/// [`crate::MAX_ATTACHED_IDENTITY_REFS`] exist.
pub const MAX_SESSION_ID_BYTES: usize = 128;

/// An explicit, caller-supplied label identifying one tracing session.
///
/// A session groups the [`crate::OperationId`] values one
/// [`crate::OperationIdAllocator`] mints. The label is opaque to this crate —
/// no particular structure (UUID, timestamp, counter) is assumed or enforced
/// beyond "not blank", containing no ASCII control character, and no longer
/// than [`MAX_SESSION_ID_BYTES`].
///
/// **This is hygiene validation, not a privacy tier.** See this module's
/// top-level docs: a `SessionId` is embedded verbatim into
/// [`crate::OperationId::as_wire`]'s serialized form and into
/// [`crate::RecordError`]'s `Display` text, so it carries no redaction
/// guarantee whatsoever, unlike [`crate::EventFieldValue`]'s three privacy
/// tiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SessionId(String);

/// Why a candidate [`SessionId`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdError {
    /// The label was empty or contained only whitespace.
    Blank,
    /// The label contained an ASCII control character.
    ///
    /// A `SessionId` is embedded verbatim into
    /// [`crate::OperationId::as_wire`] (that type's `Serialize` impl) and
    /// into [`crate::RecordError`]'s `Display` text — both places a caller's
    /// serialized trace or a log line built from an error message reads
    /// unconditionally. A control character, especially a newline or
    /// carriage return, embedded in either is a log/output-injection hazard
    /// (forging extra log lines, splitting a structured record, confusing a
    /// terminal) independent of and in addition to this crate's privacy
    /// tiers — those tiers only ever apply to event field values, never to
    /// `SessionId`.
    ContainsControlCharacter,
    /// The label exceeded [`MAX_SESSION_ID_BYTES`].
    TooLong {
        /// The maximum permitted byte length.
        max: usize,
        /// The candidate label's actual byte length.
        actual: usize,
    },
}

impl fmt::Display for SessionIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => f.write_str("session id must not be blank"),
            Self::ContainsControlCharacter => {
                f.write_str("session id must not contain an ASCII control character")
            }
            Self::TooLong { max, actual } => {
                write!(f, "session id must be at most {max} bytes, got {actual}")
            }
        }
    }
}

impl std::error::Error for SessionIdError {}

impl SessionId {
    /// Construct a session id from a caller-supplied label.
    ///
    /// # Errors
    ///
    /// - [`SessionIdError::Blank`] if `label` is empty or whitespace-only. A
    ///   blank session label carries no information and would make every
    ///   `OperationId` minted under it indistinguishable from one minted
    ///   under any other blank session.
    /// - [`SessionIdError::ContainsControlCharacter`] if `label` contains an
    ///   ASCII control character. See
    ///   [`SessionIdError::ContainsControlCharacter`]'s docs and this
    ///   module's top-level docs for why: a session label is embedded
    ///   verbatim into serialized output and error text with no privacy tier
    ///   to opt into, so a control character there is a log/output-injection
    ///   hazard this crate rejects at construction rather than merely
    ///   discouraging.
    /// - [`SessionIdError::TooLong`] if `label` exceeds
    ///   [`MAX_SESSION_ID_BYTES`].
    ///
    /// `:` is explicitly allowed: [`crate::OperationId::from_wire`]'s parser
    /// splits on the *last* colon precisely so a session label may itself
    /// contain one.
    pub fn new(label: impl Into<String>) -> Result<Self, SessionIdError> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(SessionIdError::Blank);
        }
        if label.chars().any(|c| c.is_control()) {
            return Err(SessionIdError::ContainsControlCharacter);
        }
        if label.len() > MAX_SESSION_ID_BYTES {
            return Err(SessionIdError::TooLong { max: MAX_SESSION_ID_BYTES, actual: label.len() });
        }
        Ok(Self(label))
    }

    /// Borrow the session label text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    /// Validating deserialization: a blank, control-character-containing, or
    /// over-long session id is rejected at the serde boundary rather than
    /// carried into the type system unchecked.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn rejects_blank_labels() {
        assert_eq!(SessionId::new(""), Err(SessionIdError::Blank));
        assert_eq!(SessionId::new("   "), Err(SessionIdError::Blank));
        assert_eq!(SessionId::new("\t\n"), Err(SessionIdError::Blank));
    }

    /// `MAX_SESSION_ID_BYTES` is a public contract constant; pin its exact
    /// numeric value directly. The boundary tests below only ever exercise
    /// it *by name*, so a mutation to the constant's own declared value
    /// would still pass every boundary test without this line.
    #[test]
    fn max_session_id_bytes_is_exactly_128() {
        assert_eq!(MAX_SESSION_ID_BYTES, 128);
    }

    /// Exhaustive over every [`SessionIdError`] variant: pins `Display`'s
    /// exact text for each. Nothing previously asserted this text at all.
    #[test]
    fn session_id_error_display_matches_the_exact_documented_string_for_every_variant() {
        assert_eq!(SessionIdError::Blank.to_string(), "session id must not be blank");
        assert_eq!(
            SessionIdError::ContainsControlCharacter.to_string(),
            "session id must not contain an ASCII control character"
        );
        assert_eq!(
            SessionIdError::TooLong { max: 128, actual: 200 }.to_string(),
            "session id must be at most 128 bytes, got 200"
        );
    }

    #[test]
    fn accepts_non_blank_labels() {
        assert!(SessionId::new("s1").is_ok());
        assert!(
            SessionId::new("  padded  ").is_ok(),
            "surrounding whitespace on an otherwise real label is fine"
        );
    }

    #[test]
    fn display_matches_label() {
        let id = SessionId::new("dev-session").unwrap();
        assert_eq!(format!("{id}"), "dev-session");
    }

    #[test]
    fn as_str_matches_label() {
        let id = SessionId::new("dev-session").unwrap();
        assert_eq!(id.as_str(), "dev-session");
    }

    #[test]
    fn deserialization_rejects_blank() {
        assert!(serde_json::from_str::<SessionId>("\"\"").is_err());
        assert!(serde_json::from_str::<SessionId>("\"   \"").is_err());
    }

    #[test]
    fn deserialization_round_trips_valid_labels() {
        let json = serde_json::to_string(&SessionId::new("s1").unwrap()).unwrap();
        let back: SessionId = serde_json::from_str(&json).expect("valid label must parse");
        assert_eq!(back.as_str(), "s1");
    }

    // ── Control-character rejection ──────────────────────────────────────

    #[test]
    fn rejects_control_characters() {
        for bad in ["with\nnewline", "with\rcarriage-return", "with\ttab", "with\u{0}nul"] {
            assert_eq!(
                SessionId::new(bad),
                Err(SessionIdError::ContainsControlCharacter),
                "must reject {bad:?}"
            );
        }
    }

    #[test]
    fn colon_remains_allowed() {
        // The wire parser's rsplit_once(':') handles a colon inside the
        // session component; control-character rejection must not sweep in
        // an ordinary, wire-safe punctuation character.
        assert!(SessionId::new("sess:with:colons").is_ok());
    }

    #[test]
    fn deserialization_fails_closed_on_control_character() {
        let bad = serde_json::to_string("with\nnewline").unwrap();
        assert!(serde_json::from_str::<SessionId>(&bad).is_err());
    }

    // ── Length bound ──────────────────────────────────────────────────────

    #[test]
    fn accepts_label_at_the_max_length() {
        let label = "a".repeat(MAX_SESSION_ID_BYTES);
        assert!(SessionId::new(label).is_ok());
    }

    #[test]
    fn rejects_label_over_the_max_length() {
        let label = "a".repeat(MAX_SESSION_ID_BYTES + 1);
        assert_eq!(
            SessionId::new(label),
            Err(SessionIdError::TooLong {
                max: MAX_SESSION_ID_BYTES,
                actual: MAX_SESSION_ID_BYTES + 1
            })
        );
    }

    #[test]
    fn deserialization_fails_closed_on_over_long_label() {
        let label = "a".repeat(MAX_SESSION_ID_BYTES + 1);
        let bad = serde_json::to_string(&label).unwrap();
        assert!(serde_json::from_str::<SessionId>(&bad).is_err());
    }
}
