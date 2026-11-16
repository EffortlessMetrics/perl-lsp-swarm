//! Shared UX readiness enum taxonomy.
//!
//! Defined once, consumed by both the in-process `UxRunRecorder`
//! and the xtask `UxRegressionReceiptEmitter`.

use serde::{Deserialize, Serialize};

/// Classification of why a UX scenario failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxFailureClass {
    ProviderRegression,
    ServerCrash,
    Timeout,
    TestRace,
    Infra,
    MatrixDrift,
    BaselineDrift,
    NewTestBug,
    Unknown,
}

/// Semantic routing hint for failure investigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxRoute {
    CiInvestigation,
    FixtureUpdate,
    TestFix,
    ProviderFix,
    Triage,
    BaselineUpdate,
    CrashFix,
    TimeoutTriage,
}

/// CI execution tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxCiTier {
    Pr,
    Nightly,
    Release,
}

/// Outcome of a UX scenario execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxScenarioResult {
    Pass,
    Fail,
    Quarantined,
    Skipped,
}

/// State of a metric value in the scorecard.
///
/// `InsufficientData` represents missing or below-threshold receipt data —
/// it is a metric state, not a scenario execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MetricState<T> {
    Measured { value: T, sample_count: usize },
    InsufficientData { reason: String },
}

/// Component subsystem that a scenario exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxComponent {
    Completion,
    Diagnostics,
    ModuleResolution,
    WorkspaceSymbols,
    Rename,
    SafeDelete,
    Hover,
    GotoDefinition,
    SignatureHelp,
    CodeLens,
    FoldingRange,
    SemanticTokens,
    CodeActions,
    Infra,
    AiCompletion,
}

impl UxComponent {
    /// Every UX component in canonical taxonomy order.
    ///
    /// Kept adjacent to the enum so a new variant is registered here in the
    /// same commit: [`UxComponent::exhaustiveness_witness`] stops compiling
    /// until the variant is named, and the receipt schema lock test asserts
    /// the checked-in `.ci/schemas/ux-scenario-run.schema.json` component
    /// enum equals this list's serialized form.
    pub const ALL: &'static [UxComponent] = &[
        UxComponent::Completion,
        UxComponent::Diagnostics,
        UxComponent::ModuleResolution,
        UxComponent::WorkspaceSymbols,
        UxComponent::Rename,
        UxComponent::SafeDelete,
        UxComponent::Hover,
        UxComponent::GotoDefinition,
        UxComponent::SignatureHelp,
        UxComponent::CodeLens,
        UxComponent::FoldingRange,
        UxComponent::SemanticTokens,
        UxComponent::CodeActions,
        UxComponent::Infra,
        UxComponent::AiCompletion,
    ];

    /// Wildcard-free witness over the variant set, indexed in declaration
    /// order. Adding a `UxComponent` variant breaks this match at compile
    /// time until the variant is registered in [`UxComponent::ALL`].
    #[cfg(test)]
    const fn exhaustiveness_witness(component: Self) -> usize {
        match component {
            UxComponent::Completion => 0,
            UxComponent::Diagnostics => 1,
            UxComponent::ModuleResolution => 2,
            UxComponent::WorkspaceSymbols => 3,
            UxComponent::Rename => 4,
            UxComponent::SafeDelete => 5,
            UxComponent::Hover => 6,
            UxComponent::GotoDefinition => 7,
            UxComponent::SignatureHelp => 8,
            UxComponent::CodeLens => 9,
            UxComponent::FoldingRange => 10,
            UxComponent::SemanticTokens => 11,
            UxComponent::CodeActions => 12,
            UxComponent::Infra => 13,
            UxComponent::AiCompletion => 14,
        }
    }

    /// Inverse of [`Self::exhaustiveness_witness`]: canonical index to variant.
    ///
    /// Returns `None` once `index` passes the last witness slot. Contiguity
    /// tests derive expected cardinality from this range instead of
    /// [`Self::ALL`], so a variant registered only in the witness still fails
    /// until it appears in `ALL` (and the receipt schema lock).
    #[cfg(test)]
    const fn witness_component_at(index: usize) -> Option<Self> {
        match index {
            0 => Some(UxComponent::Completion),
            1 => Some(UxComponent::Diagnostics),
            2 => Some(UxComponent::ModuleResolution),
            3 => Some(UxComponent::WorkspaceSymbols),
            4 => Some(UxComponent::Rename),
            5 => Some(UxComponent::SafeDelete),
            6 => Some(UxComponent::Hover),
            7 => Some(UxComponent::GotoDefinition),
            8 => Some(UxComponent::SignatureHelp),
            9 => Some(UxComponent::CodeLens),
            10 => Some(UxComponent::FoldingRange),
            11 => Some(UxComponent::SemanticTokens),
            12 => Some(UxComponent::CodeActions),
            13 => Some(UxComponent::Infra),
            14 => Some(UxComponent::AiCompletion),
            _ => None,
        }
    }

    /// Number of witness slots (variant cardinality), independent of `ALL`.
    #[cfg(test)]
    fn witness_variant_count() -> usize {
        let mut count = 0usize;
        while Self::witness_component_at(count).is_some() {
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod ux_component_all_tests {
    use super::UxComponent;

    /// `ALL` must list every variant exactly once, in canonical order. The
    /// witness match is the compile-time enumeration of the variant set; the
    /// contiguity assertion proves `ALL` carries each witness index exactly
    /// once with no gaps.
    #[test]
    fn all_lists_every_variant_exactly_once_in_canonical_order() {
        let witness_count = UxComponent::witness_variant_count();
        let witness_ordered: Vec<UxComponent> = (0..witness_count)
            .map(|index| {
                UxComponent::witness_component_at(index)
                    .unwrap_or_else(|| panic!("witness_variant_count() promised index {index}"))
            })
            .collect();
        assert_eq!(
            UxComponent::ALL,
            witness_ordered.as_slice(),
            "UxComponent::ALL must match the exhaustiveness witness canonical order"
        );

        let indexes: Vec<usize> = UxComponent::ALL
            .iter()
            .map(|component| UxComponent::exhaustiveness_witness(*component))
            .collect();
        let expected: Vec<usize> = (0..witness_count).collect();
        assert_eq!(indexes, expected, "UxComponent::ALL drifted from the UxComponent variant set");
    }

    #[test]
    fn exhaustiveness_witness_and_index_lookup_are_inverse() {
        let witness_count = UxComponent::witness_variant_count();
        for index in 0..witness_count {
            let component = UxComponent::witness_component_at(index)
                .unwrap_or_else(|| panic!("missing witness slot at index {index}"));
            assert_eq!(
                UxComponent::exhaustiveness_witness(component),
                index,
                "witness index lookup drifted for {component:?}"
            );
        }
        for &component in UxComponent::ALL {
            let index = UxComponent::exhaustiveness_witness(component);
            assert_eq!(
                UxComponent::witness_component_at(index),
                Some(component),
                "witness component lookup drifted for {component:?}"
            );
        }
    }
}

/// Evidence class of a UX scenario row.
///
/// Defined once so UX/status projections can mechanically separate what a
/// passing row proves. A [`UxEvidenceClass::TransportCharacterization`] row
/// proves only that a transport path stayed responsive (no protocol error, no
/// crash); it must never be counted as definition correctness, recovery
/// exactness, first-correct-answer evidence, or any other semantic/provider
/// proof class. Exactness replacements for characterization rows stay owned by
/// their semantic-proof issues (e.g. #10675 for Scenario 24 post-edit
/// navigation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxEvidenceClass {
    /// The row carries semantic/provider proof and may feed correctness
    /// projections such as exact-hit percentages.
    #[default]
    SemanticProof,
    /// The row is transport-responsiveness characterization only. Empty
    /// successful results are not evidence of correctness for this class.
    TransportCharacterization,
}

impl UxEvidenceClass {
    /// Every evidence class in canonical taxonomy order.
    pub const ALL: &'static [UxEvidenceClass] =
        &[UxEvidenceClass::SemanticProof, UxEvidenceClass::TransportCharacterization];

    /// Wildcard-free witness over the variant set, indexed in declaration order.
    #[cfg(test)]
    const fn exhaustiveness_witness(class: Self) -> usize {
        match class {
            UxEvidenceClass::SemanticProof => 0,
            UxEvidenceClass::TransportCharacterization => 1,
        }
    }

    /// Inverse of [`Self::exhaustiveness_witness`]; see [`UxComponent::witness_component_at`].
    #[cfg(test)]
    const fn witness_class_at(index: usize) -> Option<Self> {
        match index {
            0 => Some(UxEvidenceClass::SemanticProof),
            1 => Some(UxEvidenceClass::TransportCharacterization),
            _ => None,
        }
    }

    #[cfg(test)]
    fn witness_variant_count() -> usize {
        let mut count = 0usize;
        while Self::witness_class_at(count).is_some() {
            count += 1;
        }
        count
    }

    /// Whether this class may satisfy semantic/provider proof projections.
    ///
    /// Exhaustive over the variant set: adding a variant stops compilation
    /// here until the new class states its semantic eligibility explicitly.
    pub const fn supports_semantic_proof(self) -> bool {
        matches!(self, UxEvidenceClass::SemanticProof)
    }
}

/// Reject a semantic-proof claim made by a transport-characterization row.
///
/// Returns `Err` with an actionable message when `class` cannot satisfy
/// `projection`. Callers building UX/status projections funnel every semantic
/// attribution through this check so a characterization-only row cannot be
/// represented as definition correctness, recovery exactness, or legitimate
/// empty-result evidence without the owning semantic-proof issue's evidence.
pub fn ensure_evidence_supports_projection(
    class: UxEvidenceClass,
    projection: &str,
) -> Result<(), String> {
    if class.supports_semantic_proof() {
        return Ok(());
    }
    Err(format!(
        "evidence class `{class:?}` cannot satisfy semantic projection `{projection}`: \
         transport-characterization rows prove responsiveness only and need the \
         owning semantic-proof issue's evidence"
    ))
}

/// Map a failure class to its semantic route.
pub fn route_for_failure_class(class: UxFailureClass) -> UxRoute {
    match class {
        UxFailureClass::ProviderRegression => UxRoute::ProviderFix,
        UxFailureClass::ServerCrash => UxRoute::CrashFix,
        UxFailureClass::Timeout => UxRoute::TimeoutTriage,
        UxFailureClass::Infra => UxRoute::CiInvestigation,
        UxFailureClass::MatrixDrift => UxRoute::FixtureUpdate,
        UxFailureClass::BaselineDrift => UxRoute::BaselineUpdate,
        UxFailureClass::TestRace | UxFailureClass::NewTestBug => UxRoute::TestFix,
        UxFailureClass::Unknown => UxRoute::Triage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Exhaustive route mapping table test ──────────────────────────────

    #[test]
    fn route_for_failure_class_exhaustive_mapping() -> Result<(), Box<dyn std::error::Error>> {
        let table: &[(UxFailureClass, UxRoute)] = &[
            (UxFailureClass::ProviderRegression, UxRoute::ProviderFix),
            (UxFailureClass::ServerCrash, UxRoute::CrashFix),
            (UxFailureClass::Timeout, UxRoute::TimeoutTriage),
            (UxFailureClass::Infra, UxRoute::CiInvestigation),
            (UxFailureClass::MatrixDrift, UxRoute::FixtureUpdate),
            (UxFailureClass::BaselineDrift, UxRoute::BaselineUpdate),
            (UxFailureClass::TestRace, UxRoute::TestFix),
            (UxFailureClass::NewTestBug, UxRoute::TestFix),
            (UxFailureClass::Unknown, UxRoute::Triage),
        ];

        for &(class, expected_route) in table {
            let actual = route_for_failure_class(class);
            assert_eq!(
                actual, expected_route,
                "route_for_failure_class({class:?}) = {actual:?}, expected {expected_route:?}"
            );
        }

        Ok(())
    }

    // ── Serde round-trip helpers ─────────────────────────────────────────

    /// Serialize to JSON then deserialize back, asserting equality.
    fn serde_round_trip<T>(value: &T) -> Result<(), Box<dyn std::error::Error>>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
    {
        let json = serde_json::to_string(value)?;
        let back: T = serde_json::from_str(&json)?;
        assert_eq!(&back, value, "round-trip failed for {value:?}: serialized as {json}");
        Ok(())
    }

    // ── UxFailureClass serde round-trip ──────────────────────────────────

    #[test]
    fn serde_round_trip_ux_failure_class() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [
            UxFailureClass::ProviderRegression,
            UxFailureClass::ServerCrash,
            UxFailureClass::Timeout,
            UxFailureClass::TestRace,
            UxFailureClass::Infra,
            UxFailureClass::MatrixDrift,
            UxFailureClass::BaselineDrift,
            UxFailureClass::NewTestBug,
            UxFailureClass::Unknown,
        ];
        for variant in &variants {
            serde_round_trip(variant)?;
        }
        Ok(())
    }

    // ── UxRoute serde round-trip ─────────────────────────────────────────

    #[test]
    fn serde_round_trip_ux_route() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [
            UxRoute::CiInvestigation,
            UxRoute::FixtureUpdate,
            UxRoute::TestFix,
            UxRoute::ProviderFix,
            UxRoute::Triage,
            UxRoute::BaselineUpdate,
            UxRoute::CrashFix,
            UxRoute::TimeoutTriage,
        ];
        for variant in &variants {
            serde_round_trip(variant)?;
        }
        Ok(())
    }

    // ── UxEvidenceClass classification contract ──────────────────────────

    #[test]
    fn serde_round_trip_ux_evidence_class() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [UxEvidenceClass::SemanticProof, UxEvidenceClass::TransportCharacterization];
        for variant in &variants {
            serde_round_trip(variant)?;
        }

        let serialized = serde_json::to_string(&UxEvidenceClass::TransportCharacterization)?;
        assert_eq!(
            serialized, "\"transport_characterization\"",
            "evidence class serialization drifted from the receipt schema enum"
        );
        Ok(())
    }

    #[test]
    fn transport_characterization_rejects_semantic_projections() {
        assert!(UxEvidenceClass::SemanticProof.supports_semantic_proof());
        assert!(!UxEvidenceClass::TransportCharacterization.supports_semantic_proof());

        for projection in
            ["definition_exact_hit", "hover_correct", "recovery_exactness", "first_correct"]
        {
            assert!(
                ensure_evidence_supports_projection(UxEvidenceClass::SemanticProof, projection)
                    .is_ok(),
                "semantic proof must satisfy {projection}"
            );
            let rejection = ensure_evidence_supports_projection(
                UxEvidenceClass::TransportCharacterization,
                projection,
            );
            assert!(
                matches!(&rejection, Err(message) if message.contains(projection)),
                "transport characterization must not satisfy {projection}; got {rejection:?}"
            );
        }
    }

    #[test]
    fn evidence_class_all_lists_every_variant_exactly_once() {
        let witness_count = UxEvidenceClass::witness_variant_count();
        let witness_ordered: Vec<UxEvidenceClass> = (0..witness_count)
            .map(|index| {
                UxEvidenceClass::witness_class_at(index)
                    .unwrap_or_else(|| panic!("witness_variant_count() promised index {index}"))
            })
            .collect();
        assert_eq!(
            UxEvidenceClass::ALL,
            witness_ordered.as_slice(),
            "UxEvidenceClass::ALL must match the exhaustiveness witness canonical order"
        );

        let indexes: Vec<usize> = UxEvidenceClass::ALL
            .iter()
            .map(|class| UxEvidenceClass::exhaustiveness_witness(*class))
            .collect();
        let expected: Vec<usize> = (0..witness_count).collect();
        assert_eq!(indexes, expected, "UxEvidenceClass::ALL drifted from the variant set");
    }

    // ── UxCiTier serde round-trip ────────────────────────────────────────

    #[test]
    fn serde_round_trip_ux_ci_tier() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [UxCiTier::Pr, UxCiTier::Nightly, UxCiTier::Release];
        for variant in &variants {
            serde_round_trip(variant)?;
        }
        Ok(())
    }

    // ── UxScenarioResult serde round-trip ────────────────────────────────

    #[test]
    fn serde_round_trip_ux_scenario_result() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [
            UxScenarioResult::Pass,
            UxScenarioResult::Fail,
            UxScenarioResult::Quarantined,
            UxScenarioResult::Skipped,
        ];
        for variant in &variants {
            serde_round_trip(variant)?;
        }
        Ok(())
    }

    // ── UxComponent serde round-trip ─────────────────────────────────────

    #[test]
    fn serde_round_trip_ux_component() -> Result<(), Box<dyn std::error::Error>> {
        let variants = [
            UxComponent::Completion,
            UxComponent::Diagnostics,
            UxComponent::ModuleResolution,
            UxComponent::WorkspaceSymbols,
            UxComponent::Rename,
            UxComponent::SafeDelete,
            UxComponent::Hover,
            UxComponent::GotoDefinition,
            UxComponent::SignatureHelp,
            UxComponent::CodeLens,
            UxComponent::FoldingRange,
            UxComponent::SemanticTokens,
            UxComponent::Infra,
            UxComponent::AiCompletion,
        ];
        for variant in &variants {
            serde_round_trip(variant)?;
        }
        Ok(())
    }
}
