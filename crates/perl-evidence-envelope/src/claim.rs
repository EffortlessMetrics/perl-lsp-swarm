//! What an evidence envelope claims, and what it explicitly does not.
//!
//! [`ClaimBoundary`] and [`Limitation`] carry the "claim, and what it does not
//! establish" discipline into the data itself, so a consumer reading one
//! envelope in isolation (without its producer's documentation) still learns
//! the boundary of what was actually checked.

use serde::{Deserialize, Serialize};

/// The explicit boundary of what one envelope's evidence establishes.
///
/// Both lists are plain, producer-authored statements; this crate does not
/// interpret their content. Order within each list carries no meaning — two
/// `ClaimBoundary` values with the same statements in a different order
/// describe the same boundary (see [`crate::EnvelopeFingerprint`], which
/// canonicalizes both lists before hashing).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaimBoundary {
    /// Statements of what this evidence establishes.
    pub established: Vec<String>,
    /// Statements of what this evidence explicitly does not establish, to
    /// forestall a consumer over-reading the envelope's scope.
    pub not_established: Vec<String>,
}

impl ClaimBoundary {
    /// Construct a claim boundary from its established and not-established
    /// statement lists.
    #[must_use]
    pub fn new(established: Vec<String>, not_established: Vec<String>) -> Self {
        Self { established, not_established }
    }

    /// Construct an empty claim boundary (nothing established, nothing
    /// explicitly excluded). Producers should prefer stating both lists
    /// explicitly; this constructor exists for the genuinely boundary-free
    /// case rather than as a convenience default to reach for.
    #[must_use]
    pub fn empty() -> Self {
        Self { established: Vec::new(), not_established: Vec::new() }
    }
}

/// One explicit limitation on an envelope's evidence.
///
/// A limitation records a known gap or caveat that a consumer must weigh
/// before treating the envelope's evidence as sufficient for a decision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Limitation {
    /// Human-readable description of the limitation.
    pub detail: String,
    /// The specific claim this limitation affects, if it is scoped to one
    /// entry in [`ClaimBoundary::established`] rather than the envelope as a
    /// whole.
    pub affected_claim: Option<String>,
}

impl Limitation {
    /// Construct a limitation with no specific affected claim (applies to the
    /// envelope as a whole).
    #[must_use]
    pub fn general(detail: impl Into<String>) -> Self {
        Self { detail: detail.into(), affected_claim: None }
    }

    /// Construct a limitation scoped to one specific claim.
    #[must_use]
    pub fn scoped(detail: impl Into<String>, affected_claim: impl Into<String>) -> Self {
        Self { detail: detail.into(), affected_claim: Some(affected_claim.into()) }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn claim_boundary_serde_round_trip() {
        let boundary = ClaimBoundary::new(
            vec!["parses without panicking".to_string()],
            vec!["does not prove semantic correctness".to_string()],
        );
        let json = serde_json::to_string(&boundary).expect("serialize");
        let back: ClaimBoundary = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(boundary, back);
    }

    #[test]
    fn claim_boundary_empty_has_no_statements() {
        let boundary = ClaimBoundary::empty();
        assert!(boundary.established.is_empty());
        assert!(boundary.not_established.is_empty());
    }

    #[test]
    fn limitation_general_has_no_affected_claim() {
        let l = Limitation::general("sample size is small");
        assert_eq!(l.affected_claim, None);
    }

    #[test]
    fn limitation_scoped_records_affected_claim() {
        let l = Limitation::scoped("flaky on this platform", "cross-platform correctness");
        assert_eq!(l.affected_claim.as_deref(), Some("cross-platform correctness"));
    }

    #[test]
    fn limitation_serde_round_trip_both_forms() {
        for l in [
            Limitation::general("general limitation"),
            Limitation::scoped("scoped limitation", "some claim"),
        ] {
            let json = serde_json::to_string(&l).expect("serialize");
            let back: Limitation = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(l, back);
        }
    }
}
