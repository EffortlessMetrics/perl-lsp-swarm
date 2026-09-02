//! One ordered structural access hop and the laws it must satisfy.

use serde::{Deserialize, Serialize};

use super::{
    STRUCTURAL_ACCESS_SCHEMA_TAG, StructuralAccessAggregate, StructuralAccessBudget,
    StructuralAccessContractError, StructuralAccessLimitation, StructuralAccessOperator,
    StructuralAccessSelector, StructuralAccessSpelling, StructuralAggregateCompleteness,
    StructuralAggregateDisposition, StructuralHopCertainty, StructuralHopOutcome,
};
use crate::semantic_identity::SemanticIdentityFingerprint;
use crate::{SemanticConfidence, SemanticProducer, SemanticProvenance, SemanticReasonCode};

/// One local transition in a structural access chain.
///
/// A hop is ordered: its `ordinal` is its position in the owning chain, and
/// its `aggregate` must agree with that position. Everything the consumer
/// needs to explain the hop without re-deriving it is bound here — the
/// operator as written, the member selected, the value produced, the honesty
/// state of that answer, the generation it holds under, and the work spent.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessHop {
    /// Zero-based position of this hop in its chain.
    pub ordinal: u32,
    /// What this hop selected out of.
    pub aggregate: StructuralAccessAggregate,
    /// The local operator exactly as written at this position.
    pub operator: StructuralAccessOperator,
    /// Canonical identity of the member selected.
    pub selector: StructuralAccessSelector,
    /// Source spelling and range. Evidence only; never folded into identity.
    pub spelling: StructuralAccessSpelling,
    /// What the hop produced, or why it produced nothing.
    pub outcome: StructuralHopOutcome,
    /// Whether the outcome holds on every path or only on some path.
    pub certainty: StructuralHopCertainty,
    /// Whether the aggregate's member set is closed.
    pub completeness: StructuralAggregateCompleteness,
    /// Whether the aggregate escaped or was mutated.
    pub disposition: StructuralAggregateDisposition,
    /// Producer subsystem that derived the hop.
    pub producer: SemanticProducer,
    /// How the hop was derived.
    pub provenance: SemanticProvenance,
    /// Confidence in the hop.
    pub confidence: SemanticConfidence,
    /// Why the hop is exact, degraded, or refused.
    pub reason_code: SemanticReasonCode,
    /// Work accounting across this hop.
    pub budget: StructuralAccessBudget,
    /// Canonical limitation view; sorted and de-duplicated by the constructor.
    limitations: Vec<StructuralAccessLimitation>,
}

impl StructuralAccessHop {
    /// Construct and validate one hop.
    ///
    /// Limitations are canonicalized (sorted, de-duplicated) so two producers
    /// that record the same limitations in different orders build equal hops.
    ///
    /// # Errors
    /// Returns a [`StructuralAccessContractError`] when the hop is
    /// structurally impossible. See [`Self::validate`] for the full law set.
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    pub fn new(
        ordinal: u32,
        aggregate: StructuralAccessAggregate,
        operator: StructuralAccessOperator,
        selector: StructuralAccessSelector,
        spelling: StructuralAccessSpelling,
        outcome: StructuralHopOutcome,
        certainty: StructuralHopCertainty,
        completeness: StructuralAggregateCompleteness,
        disposition: StructuralAggregateDisposition,
        producer: SemanticProducer,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        reason_code: SemanticReasonCode,
        budget: StructuralAccessBudget,
        mut limitations: Vec<StructuralAccessLimitation>,
    ) -> Result<Self, StructuralAccessContractError> {
        limitations.sort();
        limitations.dedup();
        let hop = Self {
            ordinal,
            aggregate,
            operator,
            selector,
            spelling,
            outcome,
            certainty,
            completeness,
            disposition,
            producer,
            provenance,
            confidence,
            reason_code,
            budget,
            limitations,
        };
        hop.validate()?;
        Ok(hop)
    }

    /// Canonical limitation view.
    #[must_use]
    pub fn limitations(&self) -> &[StructuralAccessLimitation] {
        &self.limitations
    }

    /// Validate every law this hop must satisfy on its own.
    ///
    /// Chain-position agreement beyond the hop's own ordinal, and cross-hop
    /// ordering, are validated by
    /// [`StructuralAccessChain`](super::StructuralAccessChain).
    ///
    /// The laws are:
    ///
    /// 1. A keyed operator takes a keyed selector and an indexed operator
    ///    takes an indexed selector. `->{}` can never carry an index and
    ///    `->[]` can never carry a key.
    /// 2. The first hop cannot select out of a preceding hop, and a later hop
    ///    must select out of its immediate predecessor.
    /// 3. Definite absence requires a closed aggregate. A member missing from
    ///    an open aggregate is [`StructuralHopOutcome::UnknownMember`].
    /// 4. A definite selection requires a stable aggregate: an escaped or
    ///    mutated aggregate cannot support a claim that holds on every path.
    /// 5. [`StructuralHopOutcome::BudgetExhausted`] requires the budget to be
    ///    actually exhausted, and an exhausted budget cannot accompany a
    ///    definite outcome.
    /// 6. Identity fields are non-empty and ranges are not inverted.
    ///
    /// # Errors
    /// Returns the first violated law as a [`StructuralAccessContractError`].
    pub fn validate(&self) -> Result<(), StructuralAccessContractError> {
        self.aggregate.validate()?;

        if self.spelling.text.trim().is_empty() {
            return Err(StructuralAccessContractError::EmptyIdentityField(
                "StructuralAccessSpelling.text",
            ));
        }
        if self.spelling.anchor.start_byte > self.spelling.anchor.end_byte {
            return Err(StructuralAccessContractError::MalformedRange {
                start_byte: self.spelling.anchor.start_byte,
                end_byte: self.spelling.anchor.end_byte,
            });
        }
        if let StructuralAccessSelector::StaticKey(key) = &self.selector
            && key.trim().is_empty()
        {
            return Err(StructuralAccessContractError::EmptyIdentityField(
                "StructuralAccessSelector::StaticKey",
            ));
        }

        // Law 1: operator class and selector class must agree.
        if self.operator.is_keyed() != self.selector.is_keyed() {
            return Err(StructuralAccessContractError::SelectorOperatorMismatch {
                operator: self.operator.tag(),
                selector: self.selector.tag(),
            });
        }

        // Law 2: the aggregate must match this hop's position.
        match (&self.aggregate, self.ordinal) {
            (StructuralAccessAggregate::PrecedingHop { .. }, 0) => {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: 0,
                    reason: "the first hop has no preceding hop to select out of",
                });
            }
            (StructuralAccessAggregate::PrecedingHop { ordinal }, position) => {
                if position.checked_sub(1) != Some(*ordinal) {
                    return Err(StructuralAccessContractError::AggregateChainPosition {
                        ordinal: position,
                        reason: "a hop must select out of its immediate predecessor",
                    });
                }
            }
            (_, 0) => {}
            (_, position) => {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: position,
                    reason: "only the first hop may name an input aggregate directly",
                });
            }
        }

        // Law 3: definite absence is only sayable about a closed aggregate.
        if matches!(self.outcome, StructuralHopOutcome::AbsentMember)
            && matches!(self.completeness, StructuralAggregateCompleteness::Open)
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "a member missing from an open aggregate is unknown, not absent",
            ));
        }

        // Law 4: a definite claim requires an aggregate that did not move.
        if matches!(self.certainty, StructuralHopCertainty::Definite)
            && !self.disposition.is_stable()
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "an escaped or mutated aggregate cannot support a definite outcome",
            ));
        }

        // Law 5: an exhausted-budget outcome must be backed by the accounting,
        // and an exhausted budget cannot yield a definite answer.
        if matches!(self.outcome, StructuralHopOutcome::BudgetExhausted)
            && !self.budget.is_exhausted()
        {
            return Err(StructuralAccessContractError::MalformedBudget(
                "a budget-exhausted outcome requires zero remaining units",
            ));
        }
        if self.budget.is_exhausted() && matches!(self.certainty, StructuralHopCertainty::Definite)
        {
            return Err(StructuralAccessContractError::MalformedBudget(
                "an exhausted budget cannot support a definite outcome",
            ));
        }

        Ok(())
    }

    /// Deterministic fingerprint of this hop's identity.
    ///
    /// The fingerprint folds the schema tag, ordinal, aggregate, operator,
    /// selector, outcome, and honesty state. It deliberately excludes the
    /// source spelling and anchor: reformatting a file must not change what a
    /// hop *is*. It also excludes producer/confidence/budget, which record how
    /// the hop was obtained rather than which access it describes.
    ///
    /// Fingerprint equality is a candidate match to be confirmed by structural
    /// equality, never proof of identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        SemanticIdentityFingerprint::new(STRUCTURAL_ACCESS_SCHEMA_TAG)
            .field("ordinal", &self.ordinal.to_string())
            .discriminant("aggregate-kind", self.aggregate.tag())
            .field("aggregate", &self.aggregate.identity_text())
            .discriminant("operator", self.operator.tag())
            .discriminant("selector-kind", self.selector.tag())
            .field("selector", &self.selector.identity_text())
            .discriminant("outcome-kind", self.outcome.tag())
            .field("outcome", &self.outcome.identity_text())
            .discriminant("certainty", self.certainty.tag())
            .discriminant("completeness", self.completeness.tag())
            .discriminant("disposition", self.disposition.tag())
            .finish()
    }
}
