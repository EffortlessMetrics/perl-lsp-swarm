//! Redaction and retention classification for evidence envelopes.
//!
//! Both enums are `#[non_exhaustive]` so a future variant can be added
//! without breaking downstream `match` expressions, and neither implements
//! `Default`: a producer must state a value explicitly. There is no implicit
//! fallback variant that a missing or default-constructed field would quietly
//! resolve to.

use serde::{Deserialize, Serialize};

/// Whether, and how, an envelope's payload has been redacted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RedactionClass {
    /// No redaction was necessary or performed; the payload is safe to share
    /// as-is.
    Public,
    /// The payload contains only internal-audience material with no
    /// redaction applied.
    Internal,
    /// Sensitive material was identified and redacted before this envelope
    /// was produced.
    Redacted,
    /// Sensitive material was identified but **not** redacted; consumers
    /// must treat the payload as sensitive.
    SensitiveUnredacted,
    /// The producer could not determine a redaction class.
    Unknown,
}

impl RedactionClass {
    /// Returns `true` if the payload is safe to display without further
    /// review (i.e. [`RedactionClass::Public`]).
    #[must_use]
    pub fn is_publicly_shareable(&self) -> bool {
        matches!(self, Self::Public)
    }
}

impl std::fmt::Display for RedactionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Redacted => "redacted",
            Self::SensitiveUnredacted => "sensitive-unredacted",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

/// How long an envelope's evidence should be retained.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RetentionClass {
    /// May be discarded as soon as it has been consumed once.
    Ephemeral,
    /// Retained for a short, bounded window (e.g. one CI run's lifetime).
    ShortTerm,
    /// Retained under the project's standard evidence-retention policy.
    Standard,
    /// Retained beyond the standard policy window (e.g. for audit purposes).
    Extended,
    /// Must never be deleted by routine retention processes.
    Permanent,
    /// The producer could not determine a retention class.
    Unknown,
}

impl std::fmt::Display for RetentionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Ephemeral => "ephemeral",
            Self::ShortTerm => "short-term",
            Self::Standard => "standard",
            Self::Extended => "extended",
            Self::Permanent => "permanent",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const ALL_REDACTION: [RedactionClass; 5] = [
        RedactionClass::Public,
        RedactionClass::Internal,
        RedactionClass::Redacted,
        RedactionClass::SensitiveUnredacted,
        RedactionClass::Unknown,
    ];

    const ALL_RETENTION: [RetentionClass; 6] = [
        RetentionClass::Ephemeral,
        RetentionClass::ShortTerm,
        RetentionClass::Standard,
        RetentionClass::Extended,
        RetentionClass::Permanent,
        RetentionClass::Unknown,
    ];

    #[test]
    fn redaction_class_serde_round_trip_every_variant() {
        for v in ALL_REDACTION {
            let json = serde_json::to_string(&v).expect("serialize");
            let back: RedactionClass = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(v, back, "round-trip failed for {v}");
        }
    }

    #[test]
    fn retention_class_serde_round_trip_every_variant() {
        for v in ALL_RETENTION {
            let json = serde_json::to_string(&v).expect("serialize");
            let back: RetentionClass = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(v, back, "round-trip failed for {v}");
        }
    }

    #[test]
    fn redaction_class_rejects_unknown_variant_name() {
        assert!(serde_json::from_str::<RedactionClass>("\"made-up\"").is_err());
    }

    #[test]
    fn retention_class_rejects_unknown_variant_name() {
        assert!(serde_json::from_str::<RetentionClass>("\"made-up\"").is_err());
    }

    #[test]
    fn only_public_is_publicly_shareable() {
        for v in ALL_REDACTION {
            assert_eq!(
                v.is_publicly_shareable(),
                v == RedactionClass::Public,
                "unexpected for {v}"
            );
        }
    }

    #[test]
    fn redaction_class_display_is_stable() {
        assert_eq!(format!("{}", RedactionClass::Public), "public");
        assert_eq!(format!("{}", RedactionClass::Internal), "internal");
        assert_eq!(format!("{}", RedactionClass::Redacted), "redacted");
        assert_eq!(format!("{}", RedactionClass::SensitiveUnredacted), "sensitive-unredacted");
        assert_eq!(format!("{}", RedactionClass::Unknown), "unknown");
    }

    #[test]
    fn retention_class_display_is_stable() {
        assert_eq!(format!("{}", RetentionClass::Ephemeral), "ephemeral");
        assert_eq!(format!("{}", RetentionClass::ShortTerm), "short-term");
        assert_eq!(format!("{}", RetentionClass::Standard), "standard");
        assert_eq!(format!("{}", RetentionClass::Extended), "extended");
        assert_eq!(format!("{}", RetentionClass::Permanent), "permanent");
        assert_eq!(format!("{}", RetentionClass::Unknown), "unknown");
    }
}
