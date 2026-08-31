//! Typed outcomes for semantic queries.
//!
//! This module is deliberately below LSP and provider transports. It records
//! what a query proved, what it consumed, and why it could not produce an
//! exact value; it does not execute queries or decide how a transport renders
//! them.

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

    /// Validate identity, family, and exact-empty denominator structure.
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
    #[must_use]
    pub fn supports_exact(&self) -> bool {
        self.source_generation.is_known()
            && self.workspace_generation.is_known()
            && self.covers_required_families()
            && self.limitations.is_empty()
            && self.provenance
                == crate::SemanticProvenance::Known(crate::Provenance::SemanticAnalyzer)
            && self.confidence == crate::SemanticConfidence::Known(crate::Confidence::High)
    }
}

/// Versioned requirements for one semantic query family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticQueryRequirement {
    /// Stable query-family name.
    pub query_family: String,
    /// Schema identity governing the requirement.
    pub schema: String,
    /// Fact families that must be complete for an exact result or exact empty.
    pub required_fact_families: Vec<SemanticFactFamily>,
    /// Whether a complete denominator is required for legitimate empty.
    pub exact_empty_requires_complete: bool,
}

impl SemanticQueryRequirement {
    /// Construct a versioned query requirement.
    pub fn new(
        query_family: impl Into<String>,
        schema: impl Into<String>,
        required_fact_families: Vec<SemanticFactFamily>,
    ) -> Result<Self, SemanticQueryContractError> {
        let requirement = Self {
            query_family: query_family.into(),
            schema: schema.into(),
            required_fact_families,
            exact_empty_requires_complete: true,
        };
        if requirement.query_family.trim().is_empty() || requirement.schema.trim().is_empty() {
            return Err(SemanticQueryContractError::EmptyIdentity("query requirement"));
        }
        if duplicate_families(&requirement.required_fact_families) {
            return Err(SemanticQueryContractError::DuplicateFactFamily);
        }
        Ok(requirement)
    }

    /// Check that evidence is for this requirement and covers its denominator.
    pub fn validate_evidence(
        &self,
        evidence: &SemanticQueryEvidence,
    ) -> Result<(), SemanticQueryContractError> {
        evidence.validate()?;
        if evidence.query_schema != self.schema {
            return Err(SemanticQueryContractError::SchemaMismatch);
        }
        if evidence.query_family != self.query_family
            || !same_families(&evidence.required_fact_families, &self.required_fact_families)
            || (self.exact_empty_requires_complete && !evidence.covers_required_families())
        {
            return Err(SemanticQueryContractError::IncompleteDenominator);
        }
        Ok(())
    }
}

/// Versioned registry of query-family completeness requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticQueryRequirementRegistry {
    /// Requirements keyed by their stable query-family names.
    pub requirements: Vec<SemanticQueryRequirement>,
}

impl SemanticQueryRequirementRegistry {
    /// Construct an empty requirement registry.
    #[must_use]
    pub const fn new() -> Self {
        Self { requirements: Vec::new() }
    }

    /// Add a requirement, rejecting duplicate query-family/schema pairs.
    pub fn insert(
        &mut self,
        requirement: SemanticQueryRequirement,
    ) -> Result<(), SemanticQueryContractError> {
        if self.requirements.iter().any(|existing| {
            existing.query_family == requirement.query_family
                && existing.schema == requirement.schema
        }) {
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
}

impl Default for SemanticQueryRequirementRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Transport-neutral semantic query result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticQueryOutcome<T> {
    /// An exact value supported by complete evidence.
    Complete { value: T, evidence: SemanticQueryEvidence },
    /// A useful value exists, but limitations remain.
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
    /// Validate the outcome's evidence and exactness claims.
    pub fn validate(&self) -> Result<(), SemanticQueryContractError> {
        match self {
            Self::Complete { evidence, .. } | Self::LegitimateEmpty { evidence } => {
                evidence.validate()?;
                if !evidence.supports_exact() {
                    return Err(SemanticQueryContractError::ExactOutcomeLacksEvidence);
                }
            }
            Self::Partial { evidence, .. }
            | Self::NotReady { evidence, .. }
            | Self::Stale { evidence, .. }
            | Self::Ambiguous { evidence, .. }
            | Self::Dynamic { evidence, .. }
            | Self::Unsupported { evidence, .. } => evidence.validate()?,
            Self::InstrumentFailure { .. } => {}
        }
        Ok(())
    }

    /// Validate an outcome against a registered query-family requirement.
    pub fn validate_against(
        &self,
        requirement: &SemanticQueryRequirement,
    ) -> Result<(), SemanticQueryContractError> {
        self.validate()?;
        match self {
            Self::Complete { evidence, .. } | Self::LegitimateEmpty { evidence } => {
                requirement.validate_evidence(evidence)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether the outcome is safe to consume as an exact result.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Complete { .. } | Self::LegitimateEmpty { .. })
            && self.validate().is_ok()
    }
}

/// Contract violation in a semantic query result or requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticQueryContractError {
    /// A required identity was empty.
    EmptyIdentity(&'static str),
    /// Producer identity was not established.
    UnknownProducer,
    /// A fact family was listed more than once.
    DuplicateFactFamily,
    /// A complete family was not part of the required denominator.
    CompleteFamilyNotRequired,
    /// A consumed fact identity was repeated.
    DuplicateConsumedFact,
    /// Evidence used a different query schema.
    SchemaMismatch,
    /// Evidence did not cover the required denominator.
    IncompleteDenominator,
    /// Exact result was claimed without complete evidence.
    ExactOutcomeLacksEvidence,
    /// A query-family requirement was registered more than once.
    DuplicateRequirement,
}

impl std::fmt::Display for SemanticQueryContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyIdentity(field) => format!("empty semantic query identity: {field}"),
            Self::UnknownProducer => "semantic query producer is unknown".to_owned(),
            Self::DuplicateFactFamily => "semantic query fact family is duplicated".to_owned(),
            Self::CompleteFamilyNotRequired => {
                "complete family is outside the denominator".to_owned()
            }
            Self::DuplicateConsumedFact => "consumed fact identity is duplicated".to_owned(),
            Self::SchemaMismatch => "semantic query schema does not match requirement".to_owned(),
            Self::IncompleteDenominator => "semantic query denominator is incomplete".to_owned(),
            Self::ExactOutcomeLacksEvidence => {
                "exact semantic query outcome lacks complete evidence".to_owned()
            }
            Self::DuplicateRequirement => "semantic query requirement is duplicated".to_owned(),
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for SemanticQueryContractError {}

fn duplicate_families(families: &[SemanticFactFamily]) -> bool {
    let mut seen = HashSet::new();
    families.iter().any(|family| !seen.insert(*family))
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

    fn evidence(complete: bool) -> SemanticQueryEvidence {
        SemanticQueryEvidence::new(
            "project",
            "root",
            crate::SourceGeneration::known("doc-1"),
            crate::SourceGeneration::known("ws-1"),
            "semantic-query-v1",
            "definitions",
            crate::SemanticProducer::SemanticAnalyzer,
            crate::SemanticProvenance::Known(crate::Provenance::SemanticAnalyzer),
            crate::SemanticConfidence::Known(crate::Confidence::High),
            vec![SemanticFactFamily::ScopeLocalDeclaration],
            complete.then_some(SemanticFactFamily::ScopeLocalDeclaration).into_iter().collect(),
            vec![FactId(1)],
            vec![],
        )
        .expect("fixture evidence is valid")
    }

    #[test]
    fn exact_and_legitimate_empty_require_complete_evidence() {
        assert!(
            SemanticQueryOutcome::<u32>::Complete { value: 7, evidence: evidence(true) }
                .validate()
                .is_ok()
        );
        assert!(
            SemanticQueryOutcome::<u32>::LegitimateEmpty { evidence: evidence(true) }
                .validate()
                .is_ok()
        );
        assert!(
            SemanticQueryOutcome::<u32>::Complete { value: 7, evidence: evidence(false) }
                .validate()
                .is_err()
        );
        assert!(
            SemanticQueryOutcome::<u32>::LegitimateEmpty { evidence: evidence(false) }
                .validate()
                .is_err()
        );
    }

    #[test]
    fn every_non_exact_state_remains_distinct() {
        let e = evidence(true);
        let states = [
            SemanticQueryOutcome::Partial {
                value: 1_u8,
                limitations: vec![SemanticQueryLimitation::BudgetExceeded],
                evidence: e.clone(),
            },
            SemanticQueryOutcome::NotReady { reason: "building".into(), evidence: e.clone() },
            SemanticQueryOutcome::Stale {
                expected: crate::SourceGeneration::known("doc-2"),
                observed: crate::SourceGeneration::known("doc-1"),
                evidence: e.clone(),
            },
            SemanticQueryOutcome::Ambiguous {
                candidates: vec![1_u8, 2],
                limitations: vec![],
                evidence: e.clone(),
            },
            SemanticQueryOutcome::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: e.clone(),
            },
            SemanticQueryOutcome::Unsupported { reason: "profile".into(), evidence: e },
        ];
        assert!(states.iter().all(|state| state.validate().is_ok() && !state.is_exact()));
    }

    #[test]
    fn requirement_rejects_schema_or_denominator_mismatch() {
        let requirement = SemanticQueryRequirement::new(
            "definitions",
            "semantic-query-v1",
            vec![SemanticFactFamily::ScopeLocalDeclaration],
        )
        .expect("fixture requirement is valid");
        assert!(requirement.validate_evidence(&evidence(true)).is_ok());
        assert!(requirement.validate_evidence(&evidence(false)).is_err());
    }

    #[test]
    fn requirement_matches_fact_families_without_relying_on_order() {
        let requirement = SemanticQueryRequirement::new(
            "definitions",
            "semantic-query-v1",
            vec![
                SemanticFactFamily::ScopeLocalDeclaration,
                SemanticFactFamily::PackageFact,
            ],
        )
        .expect("fixture requirement is valid");
        let valid = SemanticQueryEvidence::new(
            "project",
            "root",
            crate::SourceGeneration::known("doc-1"),
            crate::SourceGeneration::known("ws-1"),
            "semantic-query-v1",
            "definitions",
            crate::SemanticProducer::SemanticAnalyzer,
            crate::SemanticProvenance::Known(crate::Provenance::SemanticAnalyzer),
            crate::SemanticConfidence::Known(crate::Confidence::High),
            vec![
                SemanticFactFamily::PackageFact,
                SemanticFactFamily::ScopeLocalDeclaration,
            ],
            vec![
                SemanticFactFamily::ScopeLocalDeclaration,
                SemanticFactFamily::PackageFact,
            ],
            vec![FactId(1)],
            vec![],
        )
        .expect("fixture evidence is valid");

        assert!(requirement.validate_evidence(&valid).is_ok());
    }

    #[test]
    fn direct_requirement_validation_rejects_malformed_evidence() {
        let requirement = SemanticQueryRequirement::new(
            "definitions",
            "semantic-query-v1",
            vec![SemanticFactFamily::ScopeLocalDeclaration],
        )
        .expect("fixture requirement is valid");
        let mut malformed = evidence(true);
        malformed.consumed_fact_ids = vec![FactId(1), FactId(1)];

        assert_eq!(
            requirement.validate_evidence(&malformed),
            Err(SemanticQueryContractError::DuplicateConsumedFact)
        );
    }

    #[test]
    fn malformed_evidence_and_empty_normalization_are_rejected() {
        let duplicate = SemanticQueryEvidence::new(
            "project",
            "root",
            crate::SourceGeneration::known("doc-1"),
            crate::SourceGeneration::known("ws-1"),
            "semantic-query-v1",
            "definitions",
            crate::SemanticProducer::SemanticAnalyzer,
            crate::SemanticProvenance::Known(crate::Provenance::SemanticAnalyzer),
            crate::SemanticConfidence::Known(crate::Confidence::High),
            vec![
                SemanticFactFamily::ScopeLocalDeclaration,
                SemanticFactFamily::ScopeLocalDeclaration,
            ],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(duplicate, Err(SemanticQueryContractError::DuplicateFactFamily));
        assert!(
            !SemanticQueryOutcome::<u8>::Dynamic {
                boundary: BoundaryKind::DynamicValue,
                evidence: evidence(true)
            }
            .is_exact()
        );
        assert!(
            !SemanticQueryOutcome::<u8>::InstrumentFailure { reason: "probe unavailable".into() }
                .is_exact()
        );
    }

    #[test]
    fn registry_is_versioned_and_rejects_duplicate_entries() {
        let requirement = SemanticQueryRequirement::new(
            "definitions",
            "semantic-query-v1",
            vec![SemanticFactFamily::ScopeLocalDeclaration],
        )
        .expect("fixture requirement is valid");
        let mut registry = SemanticQueryRequirementRegistry::new();
        assert!(registry.insert(requirement.clone()).is_ok());
        assert!(registry.get("definitions", "semantic-query-v1").is_some());
        assert_eq!(
            registry.insert(requirement),
            Err(SemanticQueryContractError::DuplicateRequirement)
        );
    }
}
