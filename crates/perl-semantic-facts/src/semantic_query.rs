//! Typed outcomes for semantic queries.
//!
//! This module is deliberately below LSP and provider transports. It records
//! what a query proved, what it consumed, and why it could not produce an
//! exact value; it does not execute queries or decide how a transport renders
//! them.
//!
//! Exactness is never a local property of an outcome. [`SemanticQueryOutcome`]
//! can check that its own fields are self-consistent, but `Complete` and
//! `LegitimateEmpty` only count as exact once the evidence has been validated
//! against the [`SemanticQueryRequirement`] registered for the query family.

use crate::semantic_identity::SemanticFactFamily;
use crate::{BoundaryKind, FactId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// A bounded limitation attached to a non-exact query result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticQueryLimitation {
    /// A required fact family was not complete.
    IncompleteFactFamily(SemanticFactFamily),
    /// A known dynamic or compatibility boundary limited the result.
    Boundary(BoundaryKind),
    /// The producer could not establish the requested source identity.
    MissingIdentity(String),
    /// The result was bounded by an explicit budget.
    BudgetExceeded,
    /// The producer did not expose a required fact family.
    UnavailableFactFamily(SemanticFactFamily),
    /// A producer-specific limitation that remains inspectable at the boundary.
    Other(String),
}

/// Evidence attached to every exact-capable semantic query outcome.
///
/// The evidence is intentionally a record rather than a boolean grant. A
/// consumer can inspect the denominator, generations, producer, and consumed
/// facts before deciding whether an answer is safe for its operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticQueryEvidence {
    /// Stable project identity for the queried view.
    pub project_identity: String,
    /// Stable workspace-root identity for the queried view.
    pub root_identity: String,
    /// Source/document generation consumed by the query.
    pub source_generation: crate::SourceGeneration,
    /// Workspace/model generation consumed by the query.
    pub workspace_generation: crate::SourceGeneration,
    /// Versioned semantic-query contract identity.
    pub query_schema: String,
    /// Stable query-family identity for the operation.
    pub query_family: String,
    /// Producer that supplied the evidence.
    pub producer: crate::SemanticProducer,
    /// Producer provenance for the returned facts.
    pub provenance: crate::SemanticProvenance,
    /// Producer confidence ceiling.
    pub confidence: crate::SemanticConfidence,
    /// Fact families required by the query.
    pub required_fact_families: Vec<SemanticFactFamily>,
    /// Required families actually covered by the evidence.
    pub complete_fact_families: Vec<SemanticFactFamily>,
    /// Stable identities of facts consumed by the query.
    pub consumed_fact_ids: Vec<FactId>,
    /// Limitations that remain true even when a value is returned.
    pub limitations: Vec<SemanticQueryLimitation>,
}

impl SemanticQueryEvidence {
    /// Construct evidence and reject malformed identity or duplicate-family records.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_identity: impl Into<String>,
        root_identity: impl Into<String>,
        source_generation: crate::SourceGeneration,
        workspace_generation: crate::SourceGeneration,
        query_schema: impl Into<String>,
        query_family: impl Into<String>,
        producer: crate::SemanticProducer,
        provenance: crate::SemanticProvenance,
        confidence: crate::SemanticConfidence,
        required_fact_families: Vec<SemanticFactFamily>,
        complete_fact_families: Vec<SemanticFactFamily>,
        consumed_fact_ids: Vec<FactId>,
        limitations: Vec<SemanticQueryLimitation>,
    ) -> Result<Self, SemanticQueryContractError> {
        let evidence = Self {
            project_identity: project_identity.into(),
            root_identity: root_identity.into(),
            source_generation,
            workspace_generation,
            query_schema: query_schema.into(),
            query_family: query_family.into(),
            producer,
            provenance,
            confidence,
            required_fact_families,
            complete_fact_families,
            consumed_fact_ids,
            limitations,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    /// Validate identity, family, and consumed-fact structure.
    ///
    /// This checks that the record is well formed. It does not decide whether
    /// the evidence satisfies any query family; see
    /// [`SemanticQueryRequirement::validate_evidence`].
    pub fn validate(&self) -> Result<(), SemanticQueryContractError> {
        for (name, value) in [
            ("project_identity", self.project_identity.as_str()),
            ("root_identity", self.root_identity.as_str()),
            ("query_schema", self.query_schema.as_str()),
            ("query_family", self.query_family.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(SemanticQueryContractError::EmptyIdentity(name));
            }
        }
        if self.producer == crate::SemanticProducer::Unknown {
            return Err(SemanticQueryContractError::UnknownProducer);
        }
        if duplicate_families(&self.required_fact_families)
            || duplicate_families(&self.complete_fact_families)
        {
            return Err(SemanticQueryContractError::DuplicateFactFamily);
        }
        let required: HashSet<_> = self.required_fact_families.iter().copied().collect();
        if self.complete_fact_families.iter().any(|family| !required.contains(family)) {
            return Err(SemanticQueryContractError::CompleteFamilyNotRequired);
        }
        if has_duplicates(&self.consumed_fact_ids) {
            return Err(SemanticQueryContractError::DuplicateConsumedFact);
        }
        Ok(())
    }

    /// Whether every required family has been covered by this evidence.
    #[must_use]
    pub fn covers_required_families(&self) -> bool {
        let complete: HashSet<_> = self.complete_fact_families.iter().copied().collect();
        self.required_fact_families.iter().all(|family| complete.contains(family))
    }

    /// Whether this evidence is eligible to support an exact answer.
    ///
    /// Eligibility is local to the evidence's self-declared denominator. An
    /// exact claim additionally needs that denominator to match the registered
    /// query-family requirement.
    #[must_use]
    pub fn supports_exact(&self) -> bool {
        identifies_snapshot(&self.source_generation)
            && identifies_snapshot(&self.workspace_generation)
            && self.covers_required_families()
            && self.limitations.is_empty()
            && self.provenance
                == crate::SemanticProvenance::Known(crate::Provenance::SemanticAnalyzer)
            && self.confidence == crate::SemanticConfidence::Known(crate::Confidence::High)
    }

    /// Whether every limitation entry that names a boundary belongs to `registered`.
    fn boundaries_within(&self, registered: &[BoundaryKind]) -> Result<(), BoundaryKind> {
        self.limitations
            .iter()
            .filter_map(|limitation| match limitation {
                SemanticQueryLimitation::Boundary(kind) => Some(*kind),
                _ => None,
            })
            .find(|kind| !registered.contains(kind))
            .map_or(Ok(()), Err)
    }
}

/// Versioned requirements for one semantic query family.
///
/// The requirement is the denominator authority for a family: it states which
/// fact families must be complete and which boundary classes the family
/// recognises. Evidence that names an unregistered boundary cannot satisfy
/// the family, and exact outcomes must have resolved every boundary (no
/// limitation may remain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticQueryRequirement {
    /// Stable query-family name.
    pub query_family: String,
    /// Schema identity governing the requirement.
    pub schema: String,
    /// Fact families that must be complete for an exact result or exact empty.
    pub required_fact_families: Vec<SemanticFactFamily>,
    /// Boundary classes this family recognises and must resolve before an
    /// exact result or exact empty is legal.
    pub boundary_classes: Vec<BoundaryKind>,
}

impl SemanticQueryRequirement {
    /// Construct a versioned query requirement.
    pub fn new(
        query_family: impl Into<String>,
        schema: impl Into<String>,
        required_fact_families: Vec<SemanticFactFamily>,
        boundary_classes: Vec<BoundaryKind>,
    ) -> Result<Self, SemanticQueryContractError> {
        let requirement = Self {
            query_family: query_family.into(),
            schema: schema.into(),
            required_fact_families,
            boundary_classes,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    /// Validate the requirement's own structure.
    ///
    /// Fields are public so the requirement can be declared statically, so
    /// every consumer path re-validates before trusting the record.
    pub fn validate(&self) -> Result<(), SemanticQueryContractError> {
        if self.query_family.trim().is_empty() {
            return Err(SemanticQueryContractError::EmptyIdentity("query_family"));
        }
        if self.schema.trim().is_empty() {
            return Err(SemanticQueryContractError::EmptyIdentity("schema"));
        }
        if duplicate_families(&self.required_fact_families) {
            return Err(SemanticQueryContractError::DuplicateFactFamily);
        }
        if has_duplicates(&self.boundary_classes) {
            return Err(SemanticQueryContractError::DuplicateBoundaryClass);
        }
        Ok(())
    }

    /// Check that evidence belongs to this family and declares its denominator.
    ///
    /// This binds identity: schema, query family, the required fact-family set,
    /// and the boundary vocabulary. It does not require the denominator to be
    /// complete, because non-exact outcomes legitimately carry incomplete
    /// evidence. Completeness is enforced by [`SemanticQueryOutcome::validate`]
    /// for the exact variants only.
    pub fn validate_evidence(
        &self,
        evidence: &SemanticQueryEvidence,
    ) -> Result<(), SemanticQueryContractError> {
        self.validate()?;
        evidence.validate()?;
        if evidence.query_schema != self.schema {
            return Err(SemanticQueryContractError::SchemaMismatch);
        }
        if evidence.query_family != self.query_family {
            return Err(SemanticQueryContractError::QueryFamilyMismatch);
        }
        if !same_families(&evidence.required_fact_families, &self.required_fact_families) {
            return Err(SemanticQueryContractError::IncompleteDenominator);
        }
        evidence
            .boundaries_within(&self.boundary_classes)
            .map_err(SemanticQueryContractError::UnregisteredBoundary)
    }
}

/// Versioned registry of query-family completeness requirements.
///
/// The requirement list is private and every entry passes through
/// [`Self::insert`], including entries arriving through deserialization, so a
/// registry cannot hold a malformed or duplicate requirement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<SemanticQueryRequirement>", into = "Vec<SemanticQueryRequirement>")]
pub struct SemanticQueryRequirementRegistry {
    requirements: Vec<SemanticQueryRequirement>,
}

impl SemanticQueryRequirementRegistry {
    /// Construct an empty requirement registry.
    #[must_use]
    pub const fn new() -> Self {
        Self { requirements: Vec::new() }
    }

    /// Add a requirement, rejecting malformed entries and duplicate
    /// query-family/schema pairs.
    pub fn insert(
        &mut self,
        requirement: SemanticQueryRequirement,
    ) -> Result<(), SemanticQueryContractError> {
        requirement.validate()?;
        if self.get(&requirement.query_family, &requirement.schema).is_some() {
            return Err(SemanticQueryContractError::DuplicateRequirement);
        }
        self.requirements.push(requirement);
        Ok(())
    }

    /// Find a requirement by query family and schema identity.
    #[must_use]
    pub fn get(&self, query_family: &str, schema: &str) -> Option<&SemanticQueryRequirement> {
        self.requirements.iter().find(|requirement| {
            requirement.query_family == query_family && requirement.schema == schema
        })
    }

    /// Registered requirements in insertion order.
    #[must_use]
    pub fn requirements(&self) -> &[SemanticQueryRequirement] {
        &self.requirements
    }
}

impl TryFrom<Vec<SemanticQueryRequirement>> for SemanticQueryRequirementRegistry {
    type Error = SemanticQueryContractError;

    fn try_from(requirements: Vec<SemanticQueryRequirement>) -> Result<Self, Self::Error> {
        let mut registry = Self::new();
        for requirement in requirements {
            registry.insert(requirement)?;
        }
        Ok(registry)
    }
}

impl From<SemanticQueryRequirementRegistry> for Vec<SemanticQueryRequirement> {
    fn from(registry: SemanticQueryRequirementRegistry) -> Self {
        registry.requirements
    }
}

/// Transport-neutral semantic query result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticQueryOutcome<T> {
    /// An exact value supported by complete evidence.
    Complete { value: T, evidence: SemanticQueryEvidence },
    /// A useful value exists, but limitations remain.
    ///
    /// `limitations` is the subset of `evidence.limitations` that bounded this
    /// value; it may not name a limitation the evidence does not record.
    Partial { value: T, limitations: Vec<SemanticQueryLimitation>, evidence: SemanticQueryEvidence },
    /// No value was found and the complete denominator proves that absence.
    LegitimateEmpty { evidence: SemanticQueryEvidence },
    /// Required facts are still being built or admitted.
    NotReady { reason: String, evidence: SemanticQueryEvidence },
    /// The result belongs to a different source/model generation.
    Stale {
        expected: crate::SourceGeneration,
        observed: crate::SourceGeneration,
        evidence: SemanticQueryEvidence,
    },
    /// Multiple candidates remain and no exact choice was proven.
    ///
    /// `limitations` follows the same subset rule as `Partial`.
    Ambiguous {
        candidates: Vec<T>,
        limitations: Vec<SemanticQueryLimitation>,
        evidence: SemanticQueryEvidence,
    },
    /// A dynamic boundary prevents exact resolution.
    Dynamic { boundary: BoundaryKind, evidence: SemanticQueryEvidence },
    /// This query family is not supported by the current producer/profile.
    Unsupported { reason: String, evidence: SemanticQueryEvidence },
    /// Instrumentation failed, so the result cannot claim evidence-backed status.
    InstrumentFailure { reason: String },
}

impl<T> SemanticQueryOutcome<T> {
    /// Validate the outcome's own fields against its evidence.
    ///
    /// This proves self-consistency only. Exact variants are additionally
    /// checked for complete, high-confidence evidence, but nothing here binds
    /// the outcome to a registered query family; use [`Self::validate_against`]
    /// or [`Self::is_exact`] for that.
    pub fn validate(&self) -> Result<(), SemanticQueryContractError> {
        match self {
            Self::Complete { evidence, .. } | Self::LegitimateEmpty { evidence } => {
                evidence.validate()?;
                if !evidence.supports_exact() {
                    return Err(SemanticQueryContractError::ExactOutcomeLacksEvidence);
                }
            }
            Self::Partial { limitations, evidence, .. } => {
                evidence.validate()?;
                if limitations.is_empty() {
                    return Err(SemanticQueryContractError::MissingLimitation);
                }
                limitations_recorded(limitations, evidence)?;
            }
            Self::NotReady { reason, evidence } | Self::Unsupported { reason, evidence } => {
                evidence.validate()?;
                non_blank_reason(reason)?;
            }
            Self::Stale { expected, observed, evidence } => {
                evidence.validate()?;
                if expected == observed {
                    return Err(SemanticQueryContractError::MatchingGenerations);
                }
            }
            Self::Ambiguous { candidates, limitations, evidence } => {
                evidence.validate()?;
                if candidates.len() < 2 {
                    return Err(SemanticQueryContractError::InsufficientCandidates);
                }
                limitations_recorded(limitations, evidence)?;
            }
            Self::Dynamic { boundary, evidence } => {
                evidence.validate()?;
                if !evidence.limitations.iter().any(|limitation| {
                    matches!(limitation, SemanticQueryLimitation::Boundary(value) if value == boundary)
                }) {
                    return Err(SemanticQueryContractError::MissingBoundaryLimitation);
                }
            }
            Self::InstrumentFailure { reason } => non_blank_reason(reason)?,
        }
        Ok(())
    }

    /// Validate an outcome against a registered query-family requirement.
    ///
    /// Every evidence-bearing variant must belong to the requirement's schema,
    /// family, denominator, and boundary vocabulary. Only the exact variants
    /// must additionally have covered that denominator.
    pub fn validate_against(
        &self,
        requirement: &SemanticQueryRequirement,
    ) -> Result<(), SemanticQueryContractError> {
        self.validate()?;
        match self.evidence() {
            Some(evidence) => requirement.validate_evidence(evidence),
            None => Ok(()),
        }
    }

    /// Whether the outcome is safe to consume as an exact result under
    /// `requirement`.
    #[must_use]
    pub fn is_exact(&self, requirement: &SemanticQueryRequirement) -> bool {
        matches!(self, Self::Complete { .. } | Self::LegitimateEmpty { .. })
            && self.validate_against(requirement).is_ok()
    }

    /// Evidence carried by the outcome, if the variant carries any.
    #[must_use]
    pub fn evidence(&self) -> Option<&SemanticQueryEvidence> {
        match self {
            Self::Complete { evidence, .. }
            | Self::Partial { evidence, .. }
            | Self::LegitimateEmpty { evidence }
            | Self::NotReady { evidence, .. }
            | Self::Stale { evidence, .. }
            | Self::Ambiguous { evidence, .. }
            | Self::Dynamic { evidence, .. }
            | Self::Unsupported { evidence, .. } => Some(evidence),
            Self::InstrumentFailure { .. } => None,
        }
    }
}

/// Contract violation in a semantic query result or requirement.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticQueryContractError {
    /// A required identity was empty.
    EmptyIdentity(&'static str),
    /// Producer identity was not established.
    UnknownProducer,
    /// A fact family was listed more than once.
    DuplicateFactFamily,
    /// A boundary class was listed more than once.
    DuplicateBoundaryClass,
    /// A complete family was not part of the required denominator.
    CompleteFamilyNotRequired,
    /// A consumed fact identity was repeated.
    DuplicateConsumedFact,
    /// Evidence used a different query schema.
    SchemaMismatch,
    /// Evidence was produced for a different query family.
    QueryFamilyMismatch,
    /// Evidence declared a different denominator than the requirement.
    IncompleteDenominator,
    /// Evidence named a boundary class the query family does not register.
    UnregisteredBoundary(BoundaryKind),
    /// Exact result was claimed without complete evidence.
    ExactOutcomeLacksEvidence,
    /// A query-family requirement was registered more than once.
    DuplicateRequirement,
    /// A non-exact outcome omitted its stated limitation.
    MissingLimitation,
    /// An outcome named a limitation its evidence does not record.
    LimitationNotInEvidence,
    /// A not-ready, unsupported, or instrument-failure outcome omitted its reason.
    EmptyReason,
    /// A stale outcome used the same generation on both sides.
    MatchingGenerations,
    /// An ambiguous outcome did not retain multiple candidates.
    InsufficientCandidates,
    /// A dynamic outcome did not record its boundary in evidence.
    MissingBoundaryLimitation,
}

impl std::fmt::Display for SemanticQueryContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentity(field) => {
                write!(formatter, "empty semantic query identity: {field}")
            }
            Self::UnknownProducer => formatter.write_str("semantic query producer is unknown"),
            Self::DuplicateFactFamily => {
                formatter.write_str("semantic query fact family is duplicated")
            }
            Self::DuplicateBoundaryClass => {
                formatter.write_str("semantic query boundary class is duplicated")
            }
            Self::CompleteFamilyNotRequired => {
                formatter.write_str("complete family is outside the denominator")
            }
            Self::DuplicateConsumedFact => {
                formatter.write_str("consumed fact identity is duplicated")
            }
            Self::SchemaMismatch => {
                formatter.write_str("semantic query schema does not match requirement")
            }
            Self::QueryFamilyMismatch => {
                formatter.write_str("semantic query family does not match requirement")
            }
            Self::IncompleteDenominator => {
                formatter.write_str("semantic query denominator does not match requirement")
            }
            Self::UnregisteredBoundary(kind) => {
                write!(
                    formatter,
                    "semantic query boundary is not registered for the family: {kind:?}"
                )
            }
            Self::ExactOutcomeLacksEvidence => {
                formatter.write_str("exact semantic query outcome lacks complete evidence")
            }
            Self::DuplicateRequirement => {
                formatter.write_str("semantic query requirement is duplicated")
            }
            Self::MissingLimitation => {
                formatter.write_str("non-exact semantic query outcome lacks a limitation")
            }
            Self::LimitationNotInEvidence => formatter
                .write_str("semantic query outcome names a limitation absent from its evidence"),
            Self::EmptyReason => formatter.write_str("semantic query outcome reason is empty"),
            Self::MatchingGenerations => {
                formatter.write_str("stale semantic query generations unexpectedly match")
            }
            Self::InsufficientCandidates => {
                formatter.write_str("ambiguous semantic query outcome lacks multiple candidates")
            }
            Self::MissingBoundaryLimitation => {
                formatter.write_str("dynamic semantic query outcome lacks its boundary limitation")
            }
        }
    }
}

impl std::error::Error for SemanticQueryContractError {}

/// A generation identifies a snapshot only when it is known and carries a
/// non-blank identity; `Known("  ")` is not an identifiable snapshot.
fn identifies_snapshot(generation: &crate::SourceGeneration) -> bool {
    matches!(generation, crate::SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn non_blank_reason(reason: &str) -> Result<(), SemanticQueryContractError> {
    if reason.trim().is_empty() {
        return Err(SemanticQueryContractError::EmptyReason);
    }
    Ok(())
}

/// Outcome-local limitations must be a subset of what the evidence records so
/// a consumer never receives two contradictory explanations.
fn limitations_recorded(
    limitations: &[SemanticQueryLimitation],
    evidence: &SemanticQueryEvidence,
) -> Result<(), SemanticQueryContractError> {
    if limitations.iter().any(|limitation| !evidence.limitations.contains(limitation)) {
        return Err(SemanticQueryContractError::LimitationNotInEvidence);
    }
    Ok(())
}

fn duplicate_families(families: &[SemanticFactFamily]) -> bool {
    let mut seen = HashSet::new();
    families.iter().any(|&family| !seen.insert(family))
}

fn same_families(left: &[SemanticFactFamily], right: &[SemanticFactFamily]) -> bool {
    let left: HashSet<_> = left.iter().copied().collect();
    let right: HashSet<_> = right.iter().copied().collect();
    left == right
}

fn has_duplicates<T: Ord>(values: &[T]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        Confidence, Provenance, SemanticConfidence, SemanticProducer, SemanticProvenance,
        SourceGeneration,
    };

    const SCHEMA: &str = "semantic-query-v1";
    const FAMILY: &str = "definitions";

    fn evidence(complete: bool) -> SemanticQueryEvidence {
        SemanticQueryEvidence::new(
            "project",
            "root",
            SourceGeneration::known("doc-1"),
            SourceGeneration::known("ws-1"),
            SCHEMA,
            FAMILY,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(Provenance::SemanticAnalyzer),
            SemanticConfidence::Known(Confidence::High),
            vec![SemanticFactFamily::ScopeLocalDeclaration],
            complete.then_some(SemanticFactFamily::ScopeLocalDeclaration).into_iter().collect(),
            vec![FactId(1)],
            vec![],
        )
        .expect("fixture evidence is valid")
    }

    fn evidence_with_limitation(limitation: SemanticQueryLimitation) -> SemanticQueryEvidence {
        let mut evidence = evidence(true);
        evidence.limitations.push(limitation);
        evidence
    }

    fn requirement() -> SemanticQueryRequirement {
        SemanticQueryRequirement::new(
            FAMILY,
            SCHEMA,
            vec![SemanticFactFamily::ScopeLocalDeclaration],
            vec![BoundaryKind::DynamicValue],
        )
        .expect("fixture requirement is valid")
    }

    fn budget_partial(complete: bool) -> SemanticQueryOutcome<u8> {
        let mut evidence = evidence(complete);
        evidence.limitations.push(SemanticQueryLimitation::BudgetExceeded);
        SemanticQueryOutcome::Partial {
            value: 1,
            limitations: vec![SemanticQueryLimitation::BudgetExceeded],
            evidence,
        }
    }

    #[test]
    fn exact_and_legitimate_empty_require_complete_evidence() {
        let requirement = requirement();
        let complete = SemanticQueryOutcome::<u32>::Complete { value: 7, evidence: evidence(true) };
        let empty = SemanticQueryOutcome::<u32>::LegitimateEmpty { evidence: evidence(true) };
        assert!(complete.is_exact(&requirement));
        assert!(empty.is_exact(&requirement));

        // Zero rows over an incomplete denominator is not a legitimate empty.
        let incomplete = SemanticQueryOutcome::<u32>::LegitimateEmpty { evidence: evidence(false) };
        assert_eq!(
            incomplete.validate(),
            Err(SemanticQueryContractError::ExactOutcomeLacksEvidence)
        );
        assert!(!incomplete.is_exact(&requirement));
        assert!(
            !SemanticQueryOutcome::<u32>::Complete { value: 7, evidence: evidence(false) }
                .is_exact(&requirement)
        );
    }

    #[test]
    fn producer_identity_cannot_upgrade_exactness() {
        let mut low = evidence(true);
        low.confidence = SemanticConfidence::Known(Confidence::Medium);
        assert!(!low.supports_exact());

        let mut heuristic = evidence(true);
        heuristic.provenance = SemanticProvenance::Known(Provenance::NameHeuristic);
        assert!(!heuristic.supports_exact());
    }

    #[test]
    fn blank_generations_do_not_identify_a_snapshot() {
        let mut blank = evidence(true);
        blank.source_generation = SourceGeneration::known("   ");
        assert!(!blank.supports_exact());
        assert!(
            !SemanticQueryOutcome::<u8>::LegitimateEmpty { evidence: blank }
                .is_exact(&requirement())
        );

        let mut unknown = evidence(true);
        unknown.workspace_generation = SourceGeneration::Unknown;
        assert!(!unknown.supports_exact());
    }

    #[test]
    fn every_non_exact_state_remains_distinct_and_validates_against_the_family() {
        let requirement = requirement();
        let e = evidence(true);
        let states = [
            budget_partial(true),
            SemanticQueryOutcome::NotReady { reason: "building".into(), evidence: e.clone() },
            SemanticQueryOutcome::Stale {
                expected: SourceGeneration::known("doc-2"),
                observed: SourceGeneration::known("doc-1"),
                evidence: e.clone(),
            },
            SemanticQueryOutcome::Ambiguous {
                candidates: vec![1_u8, 2],
                limitations: vec![],
                evidence: e.clone(),
            },
            SemanticQueryOutcome::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: evidence_with_limitation(SemanticQueryLimitation::Boundary(
                    BoundaryKind::DynamicValue,
                )),
            },
            SemanticQueryOutcome::Unsupported { reason: "profile".into(), evidence: e },
            SemanticQueryOutcome::InstrumentFailure { reason: "probe unavailable".into() },
        ];
        for state in &states {
            assert_eq!(state.validate_against(&requirement), Ok(()), "{state:?}");
            assert!(!state.is_exact(&requirement), "{state:?}");
        }
    }

    #[test]
    fn non_exact_outcomes_accept_incomplete_evidence_under_the_family() {
        let requirement = requirement();
        let incomplete = evidence(false);
        let states = [
            budget_partial(false),
            SemanticQueryOutcome::NotReady {
                reason: "building".into(),
                evidence: incomplete.clone(),
            },
            SemanticQueryOutcome::Stale {
                expected: SourceGeneration::known("doc-2"),
                observed: SourceGeneration::known("doc-1"),
                evidence: incomplete.clone(),
            },
            SemanticQueryOutcome::Unsupported { reason: "profile".into(), evidence: incomplete },
        ];
        for state in &states {
            assert_eq!(state.validate_against(&requirement), Ok(()), "{state:?}");
        }
    }

    #[test]
    fn non_exact_states_cannot_be_normalized_to_legitimate_empty() {
        let requirement = requirement();
        // Evidence lifted from Partial, Dynamic, and an incomplete NotReady
        // cannot be re-tagged as exact empty.
        let sources = [
            budget_partial(true),
            SemanticQueryOutcome::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: evidence_with_limitation(SemanticQueryLimitation::Boundary(
                    BoundaryKind::DynamicValue,
                )),
            },
            SemanticQueryOutcome::NotReady { reason: "building".into(), evidence: evidence(false) },
        ];
        for source in &sources {
            let evidence = source.evidence().expect("evidence-bearing fixture").clone();
            let flattened = SemanticQueryOutcome::<u8>::LegitimateEmpty { evidence };
            assert!(!flattened.is_exact(&requirement), "{source:?}");
        }
        // InstrumentFailure carries no evidence at all, so it cannot become empty.
        assert!(
            SemanticQueryOutcome::<u8>::InstrumentFailure { reason: "probe".into() }
                .evidence()
                .is_none()
        );
    }

    #[test]
    fn exactness_is_bound_to_the_registered_denominator() {
        let requirement = SemanticQueryRequirement::new(
            FAMILY,
            SCHEMA,
            vec![SemanticFactFamily::ScopeLocalDeclaration, SemanticFactFamily::PackageFact],
            vec![],
        )
        .expect("fixture requirement is valid");
        // Evidence fully covers a strict subset of the registered denominator.
        let outcome = SemanticQueryOutcome::<u8>::Complete { value: 1, evidence: evidence(true) };
        assert_eq!(outcome.validate(), Ok(()));
        assert_eq!(
            outcome.validate_against(&requirement),
            Err(SemanticQueryContractError::IncompleteDenominator)
        );
        assert!(!outcome.is_exact(&requirement));
    }

    #[test]
    fn requirement_rejects_schema_and_family_mismatch() {
        let requirement = requirement();
        let mut other_schema = evidence(true);
        other_schema.query_schema = "semantic-query-v2".into();
        assert_eq!(
            requirement.validate_evidence(&other_schema),
            Err(SemanticQueryContractError::SchemaMismatch)
        );
        let mut other_family = evidence(true);
        other_family.query_family = "references".into();
        assert_eq!(
            requirement.validate_evidence(&other_family),
            Err(SemanticQueryContractError::QueryFamilyMismatch)
        );
        assert_eq!(
            budget_partial(true).validate_against(
                &SemanticQueryRequirement::new(
                    "references",
                    SCHEMA,
                    vec![SemanticFactFamily::ScopeLocalDeclaration],
                    vec![],
                )
                .expect("fixture requirement is valid")
            ),
            Err(SemanticQueryContractError::QueryFamilyMismatch)
        );
    }

    #[test]
    fn requirement_matches_fact_families_without_relying_on_order() {
        let requirement = SemanticQueryRequirement::new(
            FAMILY,
            SCHEMA,
            vec![SemanticFactFamily::ScopeLocalDeclaration, SemanticFactFamily::PackageFact],
            vec![],
        )
        .expect("fixture requirement is valid");
        let mut permuted = evidence(true);
        permuted.required_fact_families =
            vec![SemanticFactFamily::PackageFact, SemanticFactFamily::ScopeLocalDeclaration];
        permuted.complete_fact_families =
            vec![SemanticFactFamily::ScopeLocalDeclaration, SemanticFactFamily::PackageFact];
        assert_eq!(requirement.validate_evidence(&permuted), Ok(()));
    }

    #[test]
    fn unregistered_boundaries_cannot_satisfy_the_family() {
        let requirement = requirement();
        let foreign = evidence_with_limitation(SemanticQueryLimitation::Boundary(
            BoundaryKind::DynamicRequire,
        ));
        assert_eq!(
            requirement.validate_evidence(&foreign),
            Err(SemanticQueryContractError::UnregisteredBoundary(BoundaryKind::DynamicRequire))
        );
        let dynamic = SemanticQueryOutcome::<u8>::Dynamic {
            boundary: BoundaryKind::DynamicRequire,
            evidence: foreign,
        };
        assert_eq!(dynamic.validate(), Ok(()));
        assert_eq!(
            dynamic.validate_against(&requirement),
            Err(SemanticQueryContractError::UnregisteredBoundary(BoundaryKind::DynamicRequire))
        );
        // Exact outcomes must have resolved every boundary: a remaining
        // registered boundary still blocks exactness.
        let unresolved = SemanticQueryOutcome::<u8>::LegitimateEmpty {
            evidence: evidence_with_limitation(SemanticQueryLimitation::Boundary(
                BoundaryKind::DynamicValue,
            )),
        };
        assert!(!unresolved.is_exact(&requirement));
    }

    #[test]
    fn requirement_rejects_malformed_structure() {
        assert_eq!(
            SemanticQueryRequirement::new("", SCHEMA, vec![], vec![]).err(),
            Some(SemanticQueryContractError::EmptyIdentity("query_family"))
        );
        assert_eq!(
            SemanticQueryRequirement::new(FAMILY, " ", vec![], vec![]).err(),
            Some(SemanticQueryContractError::EmptyIdentity("schema"))
        );
        assert_eq!(
            SemanticQueryRequirement::new(
                FAMILY,
                SCHEMA,
                vec![],
                vec![BoundaryKind::DynamicValue, BoundaryKind::DynamicValue],
            )
            .err(),
            Some(SemanticQueryContractError::DuplicateBoundaryClass)
        );
        // A struct literal bypasses `new`, so validation re-runs on use.
        let literal = SemanticQueryRequirement {
            query_family: String::new(),
            schema: SCHEMA.into(),
            required_fact_families: vec![],
            boundary_classes: vec![],
        };
        assert_eq!(
            literal.validate_evidence(&evidence(true)),
            Err(SemanticQueryContractError::EmptyIdentity("query_family"))
        );
        assert_eq!(
            SemanticQueryRequirementRegistry::new().insert(literal),
            Err(SemanticQueryContractError::EmptyIdentity("query_family"))
        );
    }

    #[test]
    fn direct_requirement_validation_rejects_malformed_evidence() {
        let mut malformed = evidence(true);
        malformed.consumed_fact_ids = vec![FactId(1), FactId(1)];
        assert_eq!(
            requirement().validate_evidence(&malformed),
            Err(SemanticQueryContractError::DuplicateConsumedFact)
        );
        let duplicate = SemanticQueryEvidence::new(
            "project",
            "root",
            SourceGeneration::known("doc-1"),
            SourceGeneration::known("ws-1"),
            SCHEMA,
            FAMILY,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(Provenance::SemanticAnalyzer),
            SemanticConfidence::Known(Confidence::High),
            vec![
                SemanticFactFamily::ScopeLocalDeclaration,
                SemanticFactFamily::ScopeLocalDeclaration,
            ],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(duplicate, Err(SemanticQueryContractError::DuplicateFactFamily));
    }

    #[test]
    fn typed_variants_reject_contradictory_payloads() {
        let e = evidence(true);
        assert_eq!(
            SemanticQueryOutcome::<u8>::NotReady { reason: "  ".into(), evidence: e.clone() }
                .validate(),
            Err(SemanticQueryContractError::EmptyReason)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::Unsupported { reason: String::new(), evidence: e.clone() }
                .validate(),
            Err(SemanticQueryContractError::EmptyReason)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::InstrumentFailure { reason: " ".into() }.validate(),
            Err(SemanticQueryContractError::EmptyReason)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::Stale {
                expected: SourceGeneration::known("doc-1"),
                observed: SourceGeneration::known("doc-1"),
                evidence: e.clone(),
            }
            .validate(),
            Err(SemanticQueryContractError::MatchingGenerations)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::Ambiguous {
                candidates: vec![1_u8],
                limitations: vec![],
                evidence: e.clone(),
            }
            .validate(),
            Err(SemanticQueryContractError::InsufficientCandidates)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::Ambiguous {
                candidates: vec![1_u8, 2],
                limitations: vec![SemanticQueryLimitation::BudgetExceeded],
                evidence: e.clone(),
            }
            .validate(),
            Err(SemanticQueryContractError::LimitationNotInEvidence)
        );
        assert_eq!(
            SemanticQueryOutcome::<u8>::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: e
            }
            .validate(),
            Err(SemanticQueryContractError::MissingBoundaryLimitation)
        );
    }

    #[test]
    fn partial_limitations_must_be_recorded_in_evidence() {
        assert_eq!(
            SemanticQueryOutcome::<u8>::Partial {
                value: 1,
                limitations: vec![],
                evidence: evidence_with_limitation(SemanticQueryLimitation::BudgetExceeded),
            }
            .validate(),
            Err(SemanticQueryContractError::MissingLimitation)
        );
        // Contradictory explanations: the outcome names a limitation the
        // evidence never recorded.
        assert_eq!(
            SemanticQueryOutcome::<u8>::Partial {
                value: 1,
                limitations: vec![SemanticQueryLimitation::BudgetExceeded],
                evidence: evidence_with_limitation(SemanticQueryLimitation::Other("io".into())),
            }
            .validate(),
            Err(SemanticQueryContractError::LimitationNotInEvidence)
        );
        assert_eq!(budget_partial(true).validate(), Ok(()));
    }

    #[test]
    fn registry_validates_every_entry_path() {
        let mut registry = SemanticQueryRequirementRegistry::new();
        assert_eq!(registry.insert(requirement()), Ok(()));
        assert!(registry.get(FAMILY, SCHEMA).is_some());
        assert_eq!(
            registry.insert(requirement()),
            Err(SemanticQueryContractError::DuplicateRequirement)
        );
        assert_eq!(registry.requirements().len(), 1);

        // Deserialization goes through the same insert path.
        let duplicated =
            serde_json::to_string(&vec![requirement(), requirement()]).expect("fixture serializes");
        assert!(serde_json::from_str::<SemanticQueryRequirementRegistry>(&duplicated).is_err());
        let malformed = serde_json::json!([{
            "query_family": "",
            "schema": SCHEMA,
            "required_fact_families": [],
            "boundary_classes": []
        }]);
        assert!(serde_json::from_value::<SemanticQueryRequirementRegistry>(malformed).is_err());
    }

    #[test]
    fn contract_types_round_trip_through_json() {
        fn round_trip<V>(value: &V)
        where
            V: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
        {
            let json = serde_json::to_string(value).expect("serializes");
            let back: V = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(&back, value);
        }
        round_trip(&evidence_with_limitation(SemanticQueryLimitation::Boundary(
            BoundaryKind::DynamicValue,
        )));
        round_trip(&requirement());
        let mut registry = SemanticQueryRequirementRegistry::new();
        registry.insert(requirement()).expect("fixture inserts");
        round_trip(&registry);
        let outcomes: Vec<SemanticQueryOutcome<u8>> = vec![
            SemanticQueryOutcome::Complete { value: 1, evidence: evidence(true) },
            budget_partial(true),
            SemanticQueryOutcome::LegitimateEmpty { evidence: evidence(true) },
            SemanticQueryOutcome::NotReady { reason: "building".into(), evidence: evidence(false) },
            SemanticQueryOutcome::Stale {
                expected: SourceGeneration::known("doc-2"),
                observed: SourceGeneration::Unknown,
                evidence: evidence(true),
            },
            SemanticQueryOutcome::Ambiguous {
                candidates: vec![1, 2],
                limitations: vec![],
                evidence: evidence(true),
            },
            SemanticQueryOutcome::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: evidence_with_limitation(SemanticQueryLimitation::Boundary(
                    BoundaryKind::DynamicValue,
                )),
            },
            SemanticQueryOutcome::Unsupported {
                reason: "profile".into(),
                evidence: evidence(true),
            },
            SemanticQueryOutcome::InstrumentFailure { reason: "probe".into() },
        ];
        round_trip(&outcomes);
        // A deserialized exact tag is still not exact until it validates.
        let forged: SemanticQueryOutcome<u8> = serde_json::from_value(serde_json::json!({
            "LegitimateEmpty": { "evidence": evidence(false) }
        }))
        .expect("deserializes");
        assert!(!forged.is_exact(&requirement()));
    }
}
