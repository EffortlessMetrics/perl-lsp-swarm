//! The ordered chain of structural access hops.

use serde::{Deserialize, Serialize};

use super::{
    STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION, STRUCTURAL_ACCESS_SCHEMA_TAG,
    StructuralAccessAggregate, StructuralAccessContractError, StructuralAccessHop,
    StructuralAccessSubject, StructuralHopOutcome,
};
use crate::semantic_identity::SemanticIdentityFingerprint;

/// One complete ordered structural access, e.g. `$config->{groups}{staff}[0]`.
///
/// Order is the contract. Hops are held privately and exposed as a slice so a
/// consumer cannot reorder or drop one after validation, and every hop's
/// aggregate names its immediate predecessor so a dropped or reordered hop is
/// mechanically detectable rather than merely improbable.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessChain {
    /// Contract schema version.
    pub schema_version: u32,
    /// Source, document, project, and root generation the chain holds under.
    pub subject: StructuralAccessSubject,
    /// Ordered hops, first written first.
    hops: Vec<StructuralAccessHop>,
}

impl StructuralAccessChain {
    /// Construct and validate a chain.
    ///
    /// # Errors
    /// Returns a [`StructuralAccessContractError`] when any hop is invalid or
    /// the sequence is not a possible access. See [`Self::validate`].
    pub fn new(
        subject: StructuralAccessSubject,
        hops: Vec<StructuralAccessHop>,
    ) -> Result<Self, StructuralAccessContractError> {
        let chain = Self { schema_version: STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION, subject, hops };
        chain.validate()?;
        Ok(chain)
    }

    /// Ordered hop view.
    #[must_use]
    pub fn hops(&self) -> &[StructuralAccessHop] {
        &self.hops
    }

    /// The value the whole chain selected, when it selected one.
    ///
    /// A chain whose last hop did not select has no result; the caller must
    /// read that hop's outcome rather than substituting an absence.
    #[must_use]
    pub fn selected(&self) -> Option<&StructuralHopOutcome> {
        self.hops.last().map(|hop| &hop.outcome).filter(|outcome| outcome.is_selecting())
    }

    /// Validate the chain and every hop in it.
    ///
    /// Beyond each hop's own laws, a chain must satisfy:
    ///
    /// 1. It has at least one hop — an empty chain describes no access.
    /// 2. Hop ordinals are exactly `0..n`, in order. A reordered or dropped
    ///    hop breaks this, and so does a duplicated one.
    /// 3. Only the last hop may fail to select. A hop that produced no value
    ///    cannot be selected out of, so any non-selecting hop before the end
    ///    describes an impossible access.
    /// 4. Work accounting is monotone across hops: a hop cannot begin with
    ///    more remaining units than its predecessor ended with.
    ///
    /// # Errors
    /// Returns the first violated law as a [`StructuralAccessContractError`].
    pub fn validate(&self) -> Result<(), StructuralAccessContractError> {
        if self.schema_version != STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION {
            return Err(StructuralAccessContractError::MalformedChain(
                "chain schema version is not the version this contract validates",
            ));
        }
        if self.hops.is_empty() {
            return Err(StructuralAccessContractError::MalformedChain(
                "a structural access chain must contain at least one hop",
            ));
        }

        for (position, hop) in self.hops.iter().enumerate() {
            hop.validate()?;

            let Ok(expected) = u32::try_from(position) else {
                return Err(StructuralAccessContractError::MalformedChain(
                    "chain length exceeds the ordinal space",
                ));
            };
            if hop.ordinal != expected {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: hop.ordinal,
                    reason: "hop ordinals must be a dense ascending sequence from zero",
                });
            }

            // A hop's own `validate` already proves it names ordinal - 1; this
            // re-derives the link against the actual predecessor so a chain
            // assembled from separately valid hops cannot smuggle in a gap.
            if let StructuralAccessAggregate::PrecedingHop { ordinal } = &hop.aggregate {
                let predecessor = position.checked_sub(1).and_then(|index| self.hops.get(index));
                match predecessor {
                    Some(previous) if previous.ordinal == *ordinal => {}
                    _ => {
                        return Err(StructuralAccessContractError::AggregateChainPosition {
                            ordinal: hop.ordinal,
                            reason: "the named preceding hop is not this hop's predecessor",
                        });
                    }
                }
            }
        }

        for window in self.hops.windows(2) {
            let (previous, next) = (&window[0], &window[1]);
            if !previous.outcome.is_selecting() {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "only the final hop may fail to select; nothing can be selected out of nothing",
                ));
            }
            if next.budget.units_before > previous.budget.units_after {
                return Err(StructuralAccessContractError::MalformedBudget(
                    "a hop cannot begin with more remaining units than its predecessor left",
                ));
            }
        }

        Ok(())
    }

    /// Deterministic fingerprint of the whole chain.
    ///
    /// The subject and every hop fingerprint are folded in written order, so
    /// two chains that differ only in hop order, or in one hop's operator,
    /// produce different fingerprints. Spelling and anchors are excluded for
    /// the same reason they are excluded per hop.
    ///
    /// Fingerprint equality is a candidate match to be confirmed by structural
    /// equality, never proof of identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut accumulator = SemanticIdentityFingerprint::new(STRUCTURAL_ACCESS_SCHEMA_TAG)
            .field("schema-version", &self.schema_version.to_string())
            .field("subject", &self.subject.identity_text())
            .field("hop-count", &self.hops.len().to_string());
        for hop in &self.hops {
            accumulator = accumulator.field("hop", &hop.fingerprint());
        }
        accumulator.finish()
    }
}
