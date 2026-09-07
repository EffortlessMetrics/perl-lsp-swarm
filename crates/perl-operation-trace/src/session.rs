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

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// An explicit, caller-supplied label identifying one tracing session.
///
/// A session groups the [`crate::OperationId`] values one
/// [`crate::OperationIdAllocator`] mints. The label is opaque to this crate —
/// no particular structure (UUID, timestamp, counter) is assumed or enforced
/// beyond "not blank".
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SessionId(String);

/// Why a candidate [`SessionId`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdError {
    /// The label was empty or contained only whitespace.
    Blank,
}

impl fmt::Display for SessionIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => f.write_str("session id must not be blank"),
        }
    }
}

impl std::error::Error for SessionIdError {}

impl SessionId {
    /// Construct a session id from a caller-supplied label.
    ///
    /// # Errors
    ///
    /// Returns [`SessionIdError::Blank`] if `label` is empty or
    /// whitespace-only. A blank session label carries no information and
    /// would make every `OperationId` minted under it indistinguishable from
    /// one minted under any other blank session.
    pub fn new(label: impl Into<String>) -> Result<Self, SessionIdError> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(SessionIdError::Blank);
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
    /// Validating deserialization: a blank session id is rejected at the
    /// serde boundary rather than carried into the type system unchecked.
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
        assert!(SessionId::new("").is_err());
        assert!(SessionId::new("   ").is_err());
        assert!(SessionId::new("\t\n").is_err());
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
}
