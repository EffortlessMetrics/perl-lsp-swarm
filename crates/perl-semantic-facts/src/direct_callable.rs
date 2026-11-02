//! Transport-neutral direct-call resolution subject and assessment contract (#11138).
//!
//! Types and validation only: no query execution, diagnostics, or import policy.

use serde::{Deserialize, Serialize};

use crate::semantic_identity::SemanticIdentityFingerprint;
use crate::semantic_query::{
    SemanticQueryContractError, SemanticQueryRequirement, SemanticQueryRequirementRegistry,
};
use crate::{AnchorId, EntityId, FileId, ScopeId, SourceGeneration};

/// `direct_callable_resolution.v1` schema tag.
pub const DIRECT_CALLABLE_RESOLUTION_SCHEMA: &str = "direct_callable_resolution.v1";
/// Canonical query family identity for direct-call resolution.
pub const DIRECT_CALLABLE_QUERY_FAMILY: &str = "direct_callable_resolution";

/// Parser-admitted direct-call syntax classes (#11138 subject rows).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableCallSyntaxClass {
    /// `foo(...)` with exact function-call parser identity.
    BareParenthesizedFunctionCall,
    /// Unparenthesized bareword call only when parser identity is exact.
    BareUnparenthesizedCall,
    /// `Pkg::name(...)` qualified function call.
    PackageQualifiedFunctionCall,
    /// `&name(...)` explicit ampersand named call.
    ExplicitAmpersandNamedCall,
    /// `$coderef->(...)` or equivalent coderef variable call.
    CoderefVariableCall,
    /// `$obj->method(...)` — structurally separate from direct import subjects.
    MethodCall,
    /// Builtin or feature-gated language construct call site.
    BuiltinOrFeatureKeyword,
    /// Indirect object, filehandle, or other ambiguous bareword role.
    IndirectObjectOrFilehandleAmbiguous,
    /// Unsupported or recovered synthetic call shape.
    OtherUnsupportedOrDynamicShape,
}

/// Final disposition projected for consumers (#11138 result contract).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableDisposition {
    ResolvedBuiltinOrLanguageConstruct,
    ResolvedLocalOrPackageSub,
    ResolvedImportedOrVisibleBinding,
    ResolvedGeneratedOrFrameworkBinding,
    ResolvedQualifiedTarget,
    AmbiguousMultipleTargets,
    DynamicCallablePossible,
    ExternalOrDependencyUnindexed,
    NotReadyOrIncomplete,
    ExactUnresolved,
    UnsupportedCallShape,
    Stale,
    InstrumentFailure,
}

/// Call-shape axis retained inside the assessment.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableCallShapeDisposition {
    ExactBareParenthesized,
    ExactBareUnparenthesized,
    ExactQualified,
    ExactNamedAmpersand,
    CoderefVariable,
    Method,
    AmbiguousIndirectOrFilehandle,
    Unsupported,
    RecoveredOrSynthetic,
}

/// Target-resolution axis.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableTargetResolution {
    ExactOne,
    ExactSeveral,
    NoneObserved,
    Contradicted,
    NotComparable,
}

/// Dynamic-callable boundary axis.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableDynamicBoundary {
    ProvenAbsentForSubject,
    Possible,
    PresentAndRelevant,
    UnknownOrIncomplete,
    NotApplicable,
}

/// Source-tier / readiness coverage axis.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableCoverageDisposition {
    CompleteForCallClass,
    Partial,
    ExternalOrDependencyUnindexed,
    NotReady,
    Stale,
    InstrumentFailure,
}

/// Whether legacy untyped query output was used to infer absence.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableLegacyInputKind {
    /// No legacy empty/optional inference was used.
    None,
    /// Empty vector from an unavailable or incomplete query.
    EmptyVectorAbsence,
    /// `None` from dynamic helper with unknown availability.
    DynamicNoneAbsence,
    /// Default/no-op query implementation.
    NullQueryImplementation,
}

/// Stable direct-call subject (#11138).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectCallableSubject {
    /// Stable subject identifier (schema-owned digest input).
    pub subject_id: String,
    /// Logical document instance.
    pub document: FileId,
    /// Exact call occurrence anchor.
    pub call_anchor: AnchorId,
    /// Parser-admitted syntax class.
    pub syntax_class: DirectCallableCallSyntaxClass,
    /// Callee token spelling as written (presentation evidence).
    pub callee_spelling: String,
    /// Owning package name at the call site when known.
    pub owning_package: Option<String>,
    /// Enclosing lexical scope when established.
    pub lexical_scope: Option<ScopeId>,
    /// Accepted source generation for the subject.
    pub source_generation: SourceGeneration,
    /// Workspace root identity string.
    pub root_identity: String,
    /// Semantic snapshot / query identity string.
    pub query_snapshot_id: String,
}

/// Evidence axes assembled before final disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectCallableEvidenceAxes {
    /// Call-shape disposition axis.
    pub call_shape: DirectCallableCallShapeDisposition,
    /// Target-resolution axis.
    pub target: DirectCallableTargetResolution,
    /// Dynamic-callable boundary axis.
    pub dynamic: DirectCallableDynamicBoundary,
    /// Source-tier/readiness coverage axis.
    pub coverage: DirectCallableCoverageDisposition,
    /// Whether legacy untyped empty/optional inference was used.
    pub legacy_input: DirectCallableLegacyInputKind,
    /// Resolved entity when `target` is `ExactOne`.
    pub resolved_target: Option<EntityId>,
    /// Stable reason identifiers for downstream projection.
    pub reason_ids: Vec<String>,
}

/// Typed assessment below diagnostics/LSP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectCallableResolutionAssessment {
    /// Subject the assessment describes.
    pub subject: DirectCallableSubject,
    /// Evidence axes used for projection.
    pub axes: DirectCallableEvidenceAxes,
    /// Final disposition for consumers.
    pub disposition: DirectCallableDisposition,
    /// Deterministic digest for identity comparisons.
    pub assessment_digest: String,
    /// Claim ceiling for downstream consumers.
    pub claim_ceiling: DirectCallableClaimCeiling,
}

/// What downstream operations may infer from this assessment.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DirectCallableClaimCeiling {
    /// Positive exact binding only.
    ExactBinding,
    /// Non-exact evidence; no absence claim.
    NonExactEvidence,
    /// Instrument or readiness failure.
    InstrumentOrReadiness,
}

/// Contract violation while validating or deriving assessments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectCallableContractError {
    /// Required identity field was blank.
    EmptyIdentity(&'static str),
    /// Evidence axes contradict each other.
    InvalidAxisCombination(&'static str),
    /// Exact unresolved requires a complete denominator.
    ExactUnresolvedWithoutCompleteDenominator,
    /// Method calls cannot become exact unresolved.
    MethodCallExactUnresolved,
    /// Legacy empty inputs cannot prove exact unresolved.
    LegacyEmptyExactUnresolved,
}

impl core::fmt::Display for DirectCallableContractError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyIdentity(field) => write!(f, "empty identity field: {field}"),
            Self::InvalidAxisCombination(msg) => write!(f, "invalid axis combination: {msg}"),
            Self::ExactUnresolvedWithoutCompleteDenominator => {
                write!(f, "exact_unresolved requires complete coverage denominator")
            }
            Self::MethodCallExactUnresolved => {
                write!(f, "method calls cannot become exact_unresolved")
            }
            Self::LegacyEmptyExactUnresolved => {
                write!(f, "legacy empty/optional inputs cannot prove exact_unresolved")
            }
        }
    }
}

impl std::error::Error for DirectCallableContractError {}

impl DirectCallableSubject {
    /// Validate subject identity fields.
    pub fn validate(&self) -> Result<(), DirectCallableContractError> {
        if self.subject_id.trim().is_empty() {
            return Err(DirectCallableContractError::EmptyIdentity("subject_id"));
        }
        if self.callee_spelling.trim().is_empty() {
            return Err(DirectCallableContractError::EmptyIdentity("callee_spelling"));
        }
        if self.root_identity.trim().is_empty() {
            return Err(DirectCallableContractError::EmptyIdentity("root_identity"));
        }
        if self.query_snapshot_id.trim().is_empty() {
            return Err(DirectCallableContractError::EmptyIdentity("query_snapshot_id"));
        }
        Ok(())
    }
}

impl DirectCallableEvidenceAxes {
    /// Validate axis combinations that are forbidden before disposition projection.
    pub fn validate(&self) -> Result<(), DirectCallableContractError> {
        if self.target == DirectCallableTargetResolution::ExactOne && self.resolved_target.is_none()
        {
            return Err(DirectCallableContractError::InvalidAxisCombination(
                "ExactOne requires resolved_target",
            ));
        }
        if self.target != DirectCallableTargetResolution::ExactOne && self.resolved_target.is_some()
        {
            return Err(DirectCallableContractError::InvalidAxisCombination(
                "resolved_target only allowed for ExactOne",
            ));
        }
        Ok(())
    }
}

impl DirectCallableResolutionAssessment {
    /// Validate assessment invariants, including exact-unresolved proof laws.
    pub fn validate(&self) -> Result<(), DirectCallableContractError> {
        self.subject.validate()?;
        self.axes.validate()?;
        if self.disposition == DirectCallableDisposition::ExactUnresolved {
            if self.subject.syntax_class == DirectCallableCallSyntaxClass::MethodCall
                || self.axes.call_shape == DirectCallableCallShapeDisposition::Method
            {
                return Err(DirectCallableContractError::MethodCallExactUnresolved);
            }
            if self.axes.legacy_input != DirectCallableLegacyInputKind::None {
                return Err(DirectCallableContractError::LegacyEmptyExactUnresolved);
            }
            if self.axes.coverage != DirectCallableCoverageDisposition::CompleteForCallClass {
                return Err(DirectCallableContractError::ExactUnresolvedWithoutCompleteDenominator);
            }
            if self.axes.dynamic != DirectCallableDynamicBoundary::ProvenAbsentForSubject {
                return Err(DirectCallableContractError::ExactUnresolvedWithoutCompleteDenominator);
            }
            if self.axes.target != DirectCallableTargetResolution::NoneObserved {
                return Err(DirectCallableContractError::ExactUnresolvedWithoutCompleteDenominator);
            }
        }
        if self.assessment_digest.trim().is_empty() {
            return Err(DirectCallableContractError::EmptyIdentity("assessment_digest"));
        }
        Ok(())
    }

    /// Compute the canonical digest for `subject` and `axes`.
    #[must_use]
    pub fn fingerprint(
        subject: &DirectCallableSubject,
        axes: &DirectCallableEvidenceAxes,
    ) -> String {
        let source_generation = match &subject.source_generation {
            SourceGeneration::Known(value) => value.as_str(),
            SourceGeneration::Unknown => "unknown",
        };
        let mut fp = SemanticIdentityFingerprint::new(DIRECT_CALLABLE_RESOLUTION_SCHEMA)
            .field("subject_id", &subject.subject_id)
            .field("document", &subject.document.0.to_string())
            .field("call_anchor", &subject.call_anchor.0.to_string())
            .discriminant("syntax_class", &format!("{:?}", subject.syntax_class))
            .field("callee_spelling", &subject.callee_spelling)
            .field("source_generation", source_generation)
            .field("root_identity", &subject.root_identity)
            .field("query_snapshot_id", &subject.query_snapshot_id)
            .discriminant("call_shape", &format!("{:?}", axes.call_shape))
            .discriminant("target", &format!("{:?}", axes.target))
            .discriminant("dynamic", &format!("{:?}", axes.dynamic))
            .discriminant("coverage", &format!("{:?}", axes.coverage))
            .discriminant("legacy_input", &format!("{:?}", axes.legacy_input));
        if let Some(package) = &subject.owning_package {
            fp = fp.field("owning_package", package);
        }
        if let Some(scope) = subject.lexical_scope {
            fp = fp.field("lexical_scope", &scope.0.to_string());
        }
        if let Some(entity) = axes.resolved_target {
            fp = fp.field("resolved_target", &entity.0.to_string());
        }
        for (index, reason) in axes.reason_ids.iter().enumerate() {
            fp = fp.field(&format!("reason_{index}"), reason);
        }
        fp.finish()
    }
}

/// Register the direct-call query requirement on a registry.
pub fn register_direct_callable_requirement(
    registry: &mut SemanticQueryRequirementRegistry,
) -> Result<(), SemanticQueryContractError> {
    let requirement = direct_callable_query_requirement()?;
    registry.insert(requirement)
}

/// Versioned completeness requirement for the direct-call query family.
pub fn direct_callable_query_requirement()
-> Result<SemanticQueryRequirement, SemanticQueryContractError> {
    SemanticQueryRequirement::new(
        DIRECT_CALLABLE_QUERY_FAMILY,
        DIRECT_CALLABLE_RESOLUTION_SCHEMA,
        vec![
            crate::semantic_identity::SemanticFactFamily::ScopeLocalDeclaration,
            crate::semantic_identity::SemanticFactFamily::ImportFact,
            crate::semantic_identity::SemanticFactFamily::GeneratedMemberFact,
            crate::semantic_identity::SemanticFactFamily::DynamicLimitation,
        ],
        vec![crate::BoundaryKind::CompileTimeExecution, crate::BoundaryKind::SymbolicReference],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_subject(class: DirectCallableCallSyntaxClass) -> DirectCallableSubject {
        DirectCallableSubject {
            subject_id: "subj-1".to_string(),
            document: FileId(1),
            call_anchor: AnchorId(10),
            syntax_class: class,
            callee_spelling: "foo".to_string(),
            owning_package: Some("Main".to_string()),
            lexical_scope: Some(ScopeId(2)),
            source_generation: SourceGeneration::known("gen-a"),
            root_identity: "root-a".to_string(),
            query_snapshot_id: "snap-a".to_string(),
        }
    }

    #[test]
    fn direct_callable_fingerprint_is_deterministic() {
        let subject = sample_subject(DirectCallableCallSyntaxClass::BareParenthesizedFunctionCall);
        let axes = DirectCallableEvidenceAxes {
            call_shape: DirectCallableCallShapeDisposition::ExactBareParenthesized,
            target: DirectCallableTargetResolution::NoneObserved,
            dynamic: DirectCallableDynamicBoundary::ProvenAbsentForSubject,
            coverage: DirectCallableCoverageDisposition::CompleteForCallClass,
            legacy_input: DirectCallableLegacyInputKind::None,
            resolved_target: None,
            reason_ids: vec!["denominator.complete".to_string()],
        };
        let first = DirectCallableResolutionAssessment::fingerprint(&subject, &axes);
        let second = DirectCallableResolutionAssessment::fingerprint(&subject, &axes);
        assert_eq!(first, second);
    }

    #[test]
    fn direct_callable_requirement_registers_once() -> Result<(), SemanticQueryContractError> {
        let requirement = direct_callable_query_requirement()?;
        let mut registry = SemanticQueryRequirementRegistry::new();
        registry.insert(requirement.clone())?;
        assert!(
            registry.get(DIRECT_CALLABLE_QUERY_FAMILY, DIRECT_CALLABLE_RESOLUTION_SCHEMA).is_some()
        );
        Ok(())
    }
}
