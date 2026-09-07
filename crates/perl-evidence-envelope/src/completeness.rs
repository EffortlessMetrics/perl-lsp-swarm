//! Explicit completeness classification for evidence envelopes.
//!
//! An envelope's [`crate::EvidenceEnvelope::inputs`] array can legitimately be
//! empty for several distinct reasons — the run had zero inputs to record, no
//! inputs, and every one that mattered is missing. [`Completeness`] makes the
//! reason explicit so that "zero entries" is never itself the signal.
//! [`crate::EvidenceEnvelope`] requires this field on decode: an envelope
//! that omits it fails to deserialize rather than being read as
//! [`Completeness::Complete`] by default.

use serde::{Deserialize, Serialize};

/// How complete the evidence carried by one envelope is.
///
/// # Design rule
///
/// A zero-length `inputs` array must never be read as evidence of
/// completeness on its own. `Completeness` is a first-class, always-present
/// field precisely so that "empty array" and "complete" are never conflated:
/// an envelope can be `Complete` with zero inputs (nothing was ever
/// applicable) or `Partial` with a hundred inputs (more were expected).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Completeness {
    /// Every input the producer expected to record is present.
    Complete,
    /// Some, but not all, expected inputs are present.
    Partial,
    /// Completeness does not apply to this envelope (e.g. the payload kind
    /// has no notion of "inputs").
    NotApplicable,
    /// The producer could not determine completeness because the underlying
    /// data was structurally unavailable (e.g. a required upstream system
    /// was unreachable, not merely slow).
    StructurallyUnavailable,
    /// The evidence was complete as of an earlier point but is no longer
    /// current relative to what it describes.
    Stale,
    /// The producer detected an internal inconsistency and cannot vouch for
    /// completeness at all.
    Invalid,
}

impl Completeness {
    /// Returns `true` only for [`Completeness::Complete`].
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    /// The stable textual tag used when hashing this value into an
    /// [`crate::EnvelopeFingerprint`].
    ///
    /// # Why not the discriminant
    ///
    /// Hashing `self as u8` would bind the fingerprint to *declaration order*.
    /// This enum is `#[non_exhaustive]` precisely so variants can be added, and
    /// a variant inserted anywhere but the end shifts every later
    /// discriminant — silently changing the fingerprint of already-produced
    /// evidence whose JSON form did not change at all. These tags are part of
    /// the durable fingerprint contract: renaming or reordering them is a
    /// breaking change, and the golden-vector test in `fingerprint.rs` fails
    /// loudly if one moves.
    #[must_use]
    pub const fn fingerprint_tag(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::NotApplicable => "not-applicable",
            Self::StructurallyUnavailable => "structurally-unavailable",
            Self::Stale => "stale",
            Self::Invalid => "invalid",
        }
    }
}

impl std::fmt::Display for Completeness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.fingerprint_tag())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const ALL_VARIANTS: [Completeness; 6] = [
        Completeness::Complete,
        Completeness::Partial,
        Completeness::NotApplicable,
        Completeness::StructurallyUnavailable,
        Completeness::Stale,
        Completeness::Invalid,
    ];

    #[test]
    fn only_complete_reports_is_complete() {
        for v in ALL_VARIANTS {
            assert_eq!(v.is_complete(), v == Completeness::Complete, "unexpected for {v}");
        }
    }

    #[test]
    fn completeness_serde_round_trip_every_variant() {
        for v in ALL_VARIANTS {
            let json = serde_json::to_string(&v).expect("serialize");
            let back: Completeness = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(v, back, "round-trip failed for {v}");
        }
    }

    #[test]
    fn completeness_rejects_unknown_variant_name() {
        assert!(serde_json::from_str::<Completeness>("\"totally-made-up\"").is_err());
    }

    #[test]
    fn completeness_display_is_stable() {
        assert_eq!(format!("{}", Completeness::Complete), "complete");
        assert_eq!(format!("{}", Completeness::Partial), "partial");
        assert_eq!(format!("{}", Completeness::NotApplicable), "not-applicable");
        assert_eq!(
            format!("{}", Completeness::StructurallyUnavailable),
            "structurally-unavailable"
        );
        assert_eq!(format!("{}", Completeness::Stale), "stale");
        assert_eq!(format!("{}", Completeness::Invalid), "invalid");
    }
}
