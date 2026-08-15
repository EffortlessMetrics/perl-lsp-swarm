//! Transport-neutral semantic query contract for provider implementations.
//!
//! The contract keeps five identities separate:
//!
//! - the request subject that selects a target;
//! - selector facts that establish what is at that subject;
//! - value facts returned by the provider;
//! - supporting facts that justify a qualified or no-value outcome;
//! - caller-owned live controls that validate terminal claims after the provider returns.
//!
//! One canonical fact set supplies both values and evidence. Exact empty requires
//! a capability-private grant issued from a concrete denominator snapshot; generic
//! evidence metadata cannot manufacture producers, generations, provenance,
//! confidence, or completeness. Provider implementations return unchecked drafts,
//! while [`execute_provider_query`] alone creates a checked result against the
//! original request and control. Retained results serialize deterministically but
//! intentionally cannot be deserialized without a versioned receipt validator.

mod model;
mod result;

pub use model::{
    NoopProviderQueryControl, ProviderCancellationState, ProviderCompletenessAuthorityReceipt,
    ProviderCompletenessGrant, ProviderFactGenerationScope, ProviderIdentity,
    ProviderQueryCapability, ProviderQueryContext, ProviderQueryControl, ProviderQueryDeadline,
    ProviderQueryFact, ProviderQueryFactRole, ProviderQueryKind, ProviderQueryRequest,
    ProviderQuerySubject, ProviderReadinessRequirement, ProviderReadinessState,
};
pub use result::*;

use model::{facts_are_related, semantic_provenance_is_exact};
use perl_semantic_facts::{FactId, SemanticFactEnvelope};
use std::error::Error;
use std::fmt;

/// Failure to construct or validate the provider query contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderQueryContractError {
    /// Request contains a malformed explicit identity.
    MalformedRequest,
    /// A source-level symbol alias is empty.
    MalformedSymbolKey,
    /// A canonical semantic envelope is structurally malformed.
    MalformedFact(FactId),
    /// More than one fact uses the same canonical fact identity.
    DuplicateFactId(FactId),
    /// Fact does not match or relate to the query subject.
    FactDoesNotMatchSubject(FactId),
    /// A position query has facts but no cursor-bound selector.
    MissingPositionSelector,
    /// A position-selected value is unrelated to the cursor selector.
    UnrelatedPositionValue(FactId),
    /// Value fact kind does not match the requested family.
    FactKindDoesNotMatchRequest(FactId),
    /// A completeness grant is malformed or bound to another request.
    InvalidCompletenessGrant,
    /// Exact empty lacks a separate request-bound completeness grant.
    MissingCompletenessGrant,
    /// A non-empty result supplied unrelated completeness authority.
    UnexpectedCompletenessGrant,
    /// A retained trace names a different provider surface.
    TraceSurfaceMismatch,
    /// Result is being consumed against a different request.
    RequestBindingMismatch,
    /// Outcome, facts, controls, or evidence are contradictory.
    InvalidOutcomeEvidence(ProviderQueryOutcome),
}

impl fmt::Display for ProviderQueryContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedRequest => formatter.write_str("provider query request is malformed"),
            Self::MalformedSymbolKey => {
                formatter.write_str("provider query symbol key is malformed")
            }
            Self::MalformedFact(fact_id) => {
                write!(formatter, "provider fact {} is structurally malformed", fact_id.0)
            }
            Self::DuplicateFactId(fact_id) => {
                write!(formatter, "duplicate provider fact identity {}", fact_id.0)
            }
            Self::FactDoesNotMatchSubject(fact_id) => {
                write!(formatter, "provider fact {} does not match query subject", fact_id.0)
            }
            Self::MissingPositionSelector => {
                formatter.write_str("position query has no cursor-bound selector fact")
            }
            Self::UnrelatedPositionValue(fact_id) => {
                write!(formatter, "provider fact {} is unrelated to the cursor selector", fact_id.0)
            }
            Self::FactKindDoesNotMatchRequest(fact_id) => {
                write!(formatter, "provider fact {} does not match query family", fact_id.0)
            }
            Self::InvalidCompletenessGrant => {
                formatter.write_str("provider completeness grant is invalid for this request")
            }
            Self::MissingCompletenessGrant => {
                formatter.write_str("exact empty requires a request-bound completeness grant")
            }
            Self::UnexpectedCompletenessGrant => formatter.write_str(
                "non-empty provider results cannot add unrelated completeness authority",
            ),
            Self::TraceSurfaceMismatch => {
                formatter.write_str("provider trace surface differs from the request surface")
            }
            Self::RequestBindingMismatch => {
                formatter.write_str("provider result is bound to a different request")
            }
            Self::InvalidOutcomeEvidence(outcome) => {
                write!(formatter, "provider outcome {outcome:?} has contradictory evidence")
            }
        }
    }
}

impl Error for ProviderQueryContractError {}

/// Convenience adapter for an envelope that is already canonical and does not
/// need a source-level symbol alias.
pub fn query_fact_from_envelope(
    role: ProviderQueryFactRole,
    generation_scope: ProviderFactGenerationScope,
    envelope: SemanticFactEnvelope,
) -> Result<ProviderQueryFact, ProviderQueryContractError> {
    ProviderQueryFact::from_envelope(role, generation_scope, envelope)
}

#[cfg(test)]
mod tests;
