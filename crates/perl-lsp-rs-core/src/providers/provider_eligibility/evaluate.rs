use super::super::semantic_port::{
    ProviderCancellationState, ProviderQueryCapability, ProviderQueryDeadline,
    ProviderReadinessRequirement, ProviderReadinessState,
};
use super::super::semantic_port_adapters::ProviderSnapshotCompleteness;
use super::model::{
    ProviderCompletenessRequirementV1, ProviderEligibilityBlockerClass,
    ProviderEligibilityBlockerEvidence, ProviderEligibilityDecisionV1,
    ProviderEligibilityInstrumentState, ProviderEligibilityOutcome,
    ProviderEligibilityOwnerDimension, ProviderRequestSubjectV1, ProviderResultEvidenceV1,
    ProviderSubjectLifecycleState, evidence_has_required_fields, generation_matches,
    generation_observed, identity_matches, identity_observed, optional_generation_matches,
    subject_has_required_fields,
};
use perl_semantic_facts::{SemanticProducer, SourceGeneration};

/// Pure currentness/completeness eligibility evaluation (#12154 / #6819 E01).
#[must_use]
pub fn evaluate_provider_eligibility(
    subject: &ProviderRequestSubjectV1,
    evidence: &ProviderResultEvidenceV1,
    requirement: &ProviderCompletenessRequirementV1,
) -> ProviderEligibilityDecisionV1 {
    let limitations = evidence.limitations.clone();
    let instrument_state = evidence.instrument_state.clone();

    if subject.cancellation == ProviderCancellationState::Cancelled {
        return finish(ProviderEligibilityOutcome::Cancelled, limitations, instrument_state);
    }
    if subject.deadline == ProviderQueryDeadline::Expired {
        return finish(ProviderEligibilityOutcome::DeadlineExceeded, limitations, instrument_state);
    }
    if matches!(evidence.instrument_state, ProviderEligibilityInstrumentState::Failed { .. }) {
        return finish(
            ProviderEligibilityOutcome::InstrumentFailure,
            limitations,
            instrument_state,
        );
    }

    if !subject_has_required_fields(subject) || !evidence_has_required_fields(evidence) {
        return finish(ProviderEligibilityOutcome::NotProven, limitations, instrument_state);
    }

    if requirement.capability != subject.profile.capability {
        return blocked(
            ProviderEligibilityOutcome::Refuse {
                reason: ProviderEligibilityBlockerClass::UnsupportedRequestProfile,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Completeness),
            Some(requirement.capability),
        );
    }

    if requirement.capability != evidence.capability {
        return blocked(
            ProviderEligibilityOutcome::Refuse {
                reason: ProviderEligibilityBlockerClass::FactFamilyMismatch,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Completeness),
            Some(requirement.capability),
        );
    }

    if subject.profile.edit_producing
        || subject.profile.readiness_requirement == ProviderReadinessRequirement::EditAuthorizing
    {
        if !evidence.edit_denominator_complete {
            return blocked(
                ProviderEligibilityOutcome::Refuse {
                    reason: ProviderEligibilityBlockerClass::IncompleteEditDenominator,
                },
                limitations,
                instrument_state,
                None,
                None,
                Some(ProviderEligibilityOwnerDimension::Completeness),
                None,
            );
        }
    }

    match &subject.lifecycle {
        ProviderSubjectLifecycleState::SourceDeleted => {
            return blocked(
                ProviderEligibilityOutcome::Stale {
                    reason: ProviderEligibilityBlockerClass::SourceDeleted,
                },
                limitations,
                instrument_state,
                None,
                None,
                Some(ProviderEligibilityOwnerDimension::Document),
                None,
            );
        }
        ProviderSubjectLifecycleState::RootRemoved => {
            return blocked(
                ProviderEligibilityOutcome::Unavailable {
                    reason: ProviderEligibilityBlockerClass::RootRemoved,
                },
                limitations,
                instrument_state,
                None,
                None,
                Some(ProviderEligibilityOwnerDimension::Root),
                None,
            );
        }
        ProviderSubjectLifecycleState::InstanceReplaced { prior_instance_identity }
            if identity_matches(prior_instance_identity, &evidence.document_instance_identity) =>
        {
            return blocked(
                ProviderEligibilityOutcome::Stale {
                    reason: ProviderEligibilityBlockerClass::DocumentInstanceMismatch,
                },
                limitations,
                instrument_state,
                identity_observed(&subject.document_instance_identity),
                identity_observed(&evidence.document_instance_identity),
                Some(ProviderEligibilityOwnerDimension::Document),
                None,
            );
        }
        ProviderSubjectLifecycleState::InstanceReplaced { .. }
        | ProviderSubjectLifecycleState::Current => {}
    }

    if !identity_matches(&subject.configuration_identity, &evidence.configuration_identity) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::ConfigurationOrEnvironmentMismatch,
            },
            limitations,
            instrument_state,
            identity_observed(&subject.configuration_identity),
            identity_observed(&evidence.configuration_identity),
            Some(ProviderEligibilityOwnerDimension::Configuration),
            None,
        );
    }

    if !identity_matches(&subject.environment_identity, &evidence.environment_identity) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::ConfigurationOrEnvironmentMismatch,
            },
            limitations,
            instrument_state,
            identity_observed(&subject.environment_identity),
            identity_observed(&evidence.environment_identity),
            Some(ProviderEligibilityOwnerDimension::Environment),
            None,
        );
    }

    if !identity_matches(&subject.project_identity, &evidence.project_identity) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::WrongRootOrProject,
            },
            limitations,
            instrument_state,
            identity_observed(&subject.project_identity),
            identity_observed(&evidence.project_identity),
            Some(ProviderEligibilityOwnerDimension::Project),
            None,
        );
    }

    if !identity_matches(&subject.root_identity, &evidence.root_identity) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::WrongRootOrProject,
            },
            limitations,
            instrument_state,
            identity_observed(&subject.root_identity),
            identity_observed(&evidence.root_identity),
            Some(ProviderEligibilityOwnerDimension::Root),
            None,
        );
    }

    if !generation_matches(&subject.workspace_generation, &evidence.workspace_generation) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::StaleWorkspaceGeneration,
            },
            limitations,
            instrument_state,
            generation_observed(&subject.workspace_generation),
            generation_observed(&evidence.workspace_generation),
            Some(ProviderEligibilityOwnerDimension::Workspace),
            None,
        );
    }

    if !generation_matches(&subject.document_generation, &evidence.document_generation) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::StaleDocumentGeneration,
            },
            limitations,
            instrument_state,
            generation_observed(&subject.document_generation),
            generation_observed(&evidence.document_generation),
            Some(ProviderEligibilityOwnerDimension::Document),
            None,
        );
    }

    if !identity_matches(&subject.document_instance_identity, &evidence.document_instance_identity)
    {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::DocumentInstanceMismatch,
            },
            limitations,
            instrument_state,
            identity_observed(&subject.document_instance_identity),
            identity_observed(&evidence.document_instance_identity),
            Some(ProviderEligibilityOwnerDimension::Document),
            None,
        );
    }

    if !generation_matches(&subject.parser_generation, &evidence.parser_generation) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::StaleDocumentGeneration,
            },
            limitations,
            instrument_state,
            generation_observed(&subject.parser_generation),
            generation_observed(&evidence.parser_generation),
            Some(ProviderEligibilityOwnerDimension::Parser),
            None,
        );
    }

    if !optional_generation_matches(
        subject.semantic_contribution_generation.as_ref(),
        evidence.semantic_contribution_generation.as_ref(),
    ) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::StaleDocumentGeneration,
            },
            limitations,
            instrument_state,
            generation_observed(
                subject
                    .semantic_contribution_generation
                    .as_ref()
                    .unwrap_or(&SourceGeneration::Unknown),
            ),
            generation_observed(
                evidence
                    .semantic_contribution_generation
                    .as_ref()
                    .unwrap_or(&SourceGeneration::Unknown),
            ),
            Some(ProviderEligibilityOwnerDimension::Semantic),
            None,
        );
    }

    if !optional_generation_matches(
        subject.compiler_contribution_generation.as_ref(),
        evidence.compiler_contribution_generation.as_ref(),
    ) {
        return blocked(
            ProviderEligibilityOutcome::Stale {
                reason: ProviderEligibilityBlockerClass::StaleDocumentGeneration,
            },
            limitations,
            instrument_state,
            generation_observed(
                subject
                    .compiler_contribution_generation
                    .as_ref()
                    .unwrap_or(&SourceGeneration::Unknown),
            ),
            generation_observed(
                evidence
                    .compiler_contribution_generation
                    .as_ref()
                    .unwrap_or(&SourceGeneration::Unknown),
            ),
            Some(ProviderEligibilityOwnerDimension::Compiler),
            None,
        );
    }

    if evidence.dynamic_boundary_present {
        return blocked(
            ProviderEligibilityOutcome::Dynamic {
                reason: ProviderEligibilityBlockerClass::DynamicBoundary,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Semantic),
            None,
        );
    }

    if evidence.mixed_producer_lineage {
        return blocked(
            ProviderEligibilityOutcome::Ambiguous {
                reason: ProviderEligibilityBlockerClass::AmbiguousFacts,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Semantic),
            None,
        );
    }

    if evidence.recovered_facts_present {
        return blocked(
            ProviderEligibilityOutcome::FallbackRequired {
                reason: ProviderEligibilityBlockerClass::PartialOrRecoveredFacts,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Semantic),
            None,
        );
    }

    if has_adapter_exactness_limitation(&limitations) {
        return blocked(
            ProviderEligibilityOutcome::FallbackRequired {
                reason: ProviderEligibilityBlockerClass::AdapterLimitation,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Semantic),
            None,
        );
    }

    if requirement.require_current_binding && !evidence.has_current_binding {
        return blocked(
            ProviderEligibilityOutcome::Unavailable {
                reason: ProviderEligibilityBlockerClass::NoMatchingEntity,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Completeness),
            None,
        );
    }

    let denominator_ready = evidence.denominator_complete
        && evidence.completeness == ProviderSnapshotCompleteness::Complete;

    if subject.readiness_state != ProviderReadinessState::Ready {
        if requirement.require_complete_denominator && !denominator_ready {
            return blocked(
                ProviderEligibilityOutcome::Unavailable {
                    reason: ProviderEligibilityBlockerClass::IncompleteDenominator,
                },
                limitations,
                instrument_state,
                None,
                None,
                Some(ProviderEligibilityOwnerDimension::Completeness),
                None,
            );
        }
        if matches!(
            subject.readiness_state,
            ProviderReadinessState::ReadyLimited | ProviderReadinessState::Building
        ) && evidence.completeness != ProviderSnapshotCompleteness::Unknown
        {
            return finish(
                ProviderEligibilityOutcome::QualifiedEligible,
                limitations,
                instrument_state,
            );
        }
        return blocked(
            ProviderEligibilityOutcome::Unavailable {
                reason: ProviderEligibilityBlockerClass::ReadinessBoundary,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Readiness),
            None,
        );
    }

    if requirement.require_complete_denominator && !denominator_ready {
        return blocked(
            ProviderEligibilityOutcome::Unavailable {
                reason: ProviderEligibilityBlockerClass::IncompleteDenominator,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Completeness),
            None,
        );
    }

    if subject.profile.edit_producing
        || subject.profile.readiness_requirement == ProviderReadinessRequirement::EditAuthorizing
    {
        return blocked(
            ProviderEligibilityOutcome::Refuse {
                reason: ProviderEligibilityBlockerClass::IncompleteEditDenominator,
            },
            limitations,
            instrument_state,
            None,
            None,
            Some(ProviderEligibilityOwnerDimension::Completeness),
            None,
        );
    }

    if !producer_lineage_supports_exact(&evidence.producer_lineage) {
        return finish(ProviderEligibilityOutcome::NotProven, limitations, instrument_state);
    }

    if denominator_ready && evidence.filtered_fact_count == 0 {
        return finish(
            ProviderEligibilityOutcome::ExactEligible { exact_empty: true },
            limitations,
            instrument_state,
        );
    }

    if denominator_ready && evidence.filtered_fact_count > 0 {
        return finish(
            ProviderEligibilityOutcome::ExactEligible { exact_empty: false },
            limitations,
            instrument_state,
        );
    }

    blocked(
        ProviderEligibilityOutcome::Unavailable {
            reason: ProviderEligibilityBlockerClass::IncompleteDenominator,
        },
        limitations,
        instrument_state,
        None,
        None,
        Some(ProviderEligibilityOwnerDimension::Completeness),
        None,
    )
}

fn has_adapter_exactness_limitation(limitations: &[String]) -> bool {
    limitations.iter().any(|limitation| {
        limitation.contains("multi_shard_single_snapshot_exactness")
            || limitation.contains("malformed_adapted_fact")
            || limitation.contains("tombstone")
            || limitation.contains("missing_source_anchor")
            || limitation.contains("conflicting_duplicate")
            || limitation.contains("unresolved_source_anchor")
    })
}

fn producer_lineage_supports_exact(lineage: &[SemanticProducer]) -> bool {
    !lineage.is_empty() && lineage.iter().all(|producer| *producer != SemanticProducer::Unknown)
}

fn finish(
    outcome: ProviderEligibilityOutcome,
    limitations: Vec<String>,
    instrument_state: ProviderEligibilityInstrumentState,
) -> ProviderEligibilityDecisionV1 {
    let evidence =
        ProviderEligibilityBlockerEvidence::for_outcome(&outcome, limitations, instrument_state);
    ProviderEligibilityDecisionV1 { outcome, evidence }
}

fn blocked(
    outcome: ProviderEligibilityOutcome,
    limitations: Vec<String>,
    instrument_state: ProviderEligibilityInstrumentState,
    expected_identity: Option<String>,
    observed_identity: Option<String>,
    owner_dimension: Option<ProviderEligibilityOwnerDimension>,
    required_capability: Option<ProviderQueryCapability>,
) -> ProviderEligibilityDecisionV1 {
    let mut evidence =
        ProviderEligibilityBlockerEvidence::for_outcome(&outcome, limitations, instrument_state);
    if let Some(owner_dimension) = owner_dimension {
        evidence.owner_dimension = Some(owner_dimension);
    }
    evidence.expected_identity = expected_identity;
    evidence.observed_identity = observed_identity;
    evidence.required_capability = required_capability;
    ProviderEligibilityDecisionV1 { outcome, evidence }
}
