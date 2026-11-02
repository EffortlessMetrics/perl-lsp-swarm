use super::super::semantic_port::{
    ProviderCancellationState, ProviderIdentity, ProviderQueryCapability, ProviderQueryDeadline,
    ProviderReadinessRequirement, ProviderReadinessState,
};
use super::super::semantic_port_adapters::ProviderSnapshotCompleteness;
use super::evaluate_provider_eligibility;
use super::model::{
    ProviderCompletenessRequirementV1, ProviderEligibilityBlockerClass,
    ProviderEligibilityInstrumentState, ProviderEligibilityOutcome,
    ProviderEligibilityRequestProfile, ProviderRequestSubjectV1, ProviderResultEvidenceV1,
    ProviderSubjectLifecycleState,
};
use perl_semantic_facts::{SemanticProducer, SourceGeneration};

fn references_profile() -> ProviderEligibilityRequestProfile {
    ProviderEligibilityRequestProfile::new(
        "textDocument/references",
        ProviderQueryCapability::References,
        ProviderReadinessRequirement::ActiveDocument,
        false,
    )
}

fn aligned_subject() -> ProviderRequestSubjectV1 {
    ProviderRequestSubjectV1 {
        profile: references_profile(),
        project_identity: ProviderIdentity::known("project-a"),
        root_identity: ProviderIdentity::known("root-a"),
        workspace_generation: SourceGeneration::known("ws-1"),
        document_instance_identity: ProviderIdentity::known("doc-instance-1"),
        document_generation: SourceGeneration::known("doc-gen-1"),
        parser_generation: SourceGeneration::known("parser-1"),
        semantic_contribution_generation: Some(SourceGeneration::known("semantic-1")),
        compiler_contribution_generation: None,
        configuration_identity: ProviderIdentity::known("cfg-1"),
        environment_identity: ProviderIdentity::known("env-1"),
        lifecycle: ProviderSubjectLifecycleState::Current,
        readiness_state: ProviderReadinessState::Ready,
        deadline: ProviderQueryDeadline::RemainingMillis(100),
        cancellation: ProviderCancellationState::Active,
    }
}

fn aligned_evidence() -> ProviderResultEvidenceV1 {
    ProviderResultEvidenceV1 {
        project_identity: ProviderIdentity::known("project-a"),
        root_identity: ProviderIdentity::known("root-a"),
        workspace_generation: SourceGeneration::known("ws-1"),
        document_instance_identity: ProviderIdentity::known("doc-instance-1"),
        document_generation: SourceGeneration::known("doc-gen-1"),
        parser_generation: SourceGeneration::known("parser-1"),
        semantic_contribution_generation: Some(SourceGeneration::known("semantic-1")),
        compiler_contribution_generation: None,
        configuration_identity: ProviderIdentity::known("cfg-1"),
        environment_identity: ProviderIdentity::known("env-1"),
        capability: ProviderQueryCapability::References,
        completeness: ProviderSnapshotCompleteness::Complete,
        producer_lineage: vec![SemanticProducer::WorkspaceIndex],
        limitations: Vec::new(),
        instrument_state: ProviderEligibilityInstrumentState::ok(),
        has_current_binding: true,
        filtered_fact_count: 2,
        denominator_complete: true,
        dynamic_boundary_present: false,
        recovered_facts_present: false,
        mixed_producer_lineage: false,
    }
}

fn references_requirement(include_declaration: bool) -> ProviderCompletenessRequirementV1 {
    ProviderCompletenessRequirementV1::lexical_references_same_file(include_declaration)
}

#[test]
fn exact_semantic_result_when_identities_align() {
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::ExactEligible { exact_empty: false }
    ));
}

#[test]
fn exact_empty_requires_complete_denominator_not_count_alone() {
    let mut evidence = aligned_evidence();
    evidence.filtered_fact_count = 0;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::ExactEligible { exact_empty: true }
    ));

    evidence.denominator_complete = false;
    evidence.completeness = ProviderSnapshotCompleteness::Partial;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Unavailable {
            reason: ProviderEligibilityBlockerClass::IncompleteDenominator
        }
    ));
}

#[test]
fn missing_binding_is_not_exact_empty() {
    let mut evidence = aligned_evidence();
    evidence.has_current_binding = false;
    evidence.filtered_fact_count = 0;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Unavailable {
            reason: ProviderEligibilityBlockerClass::NoMatchingEntity
        }
    ));
}

#[test]
fn stale_document_generation_fails_closed() {
    let mut evidence = aligned_evidence();
    evidence.document_generation = SourceGeneration::known("doc-gen-old");
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Stale {
            reason: ProviderEligibilityBlockerClass::StaleDocumentGeneration
        }
    ));
}

#[test]
fn close_reopen_instance_mismatch_is_stale() {
    let mut subject = aligned_subject();
    subject.document_instance_identity = ProviderIdentity::known("doc-instance-2");
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Stale {
            reason: ProviderEligibilityBlockerClass::DocumentInstanceMismatch
        }
    ));
}

#[test]
fn wrong_workspace_generation_is_stale() {
    let mut evidence = aligned_evidence();
    evidence.workspace_generation = SourceGeneration::known("ws-old");
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Stale {
            reason: ProviderEligibilityBlockerClass::StaleWorkspaceGeneration
        }
    ));
}

#[test]
fn configuration_movement_is_stale() {
    let mut evidence = aligned_evidence();
    evidence.configuration_identity = ProviderIdentity::known("cfg-2");
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Stale {
            reason: ProviderEligibilityBlockerClass::ConfigurationOrEnvironmentMismatch
        }
    ));
}

#[test]
fn workspace_not_ready_then_qualified_when_limited() {
    let mut subject = aligned_subject();
    subject.readiness_state = ProviderReadinessState::ReadyLimited;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(decision.outcome, ProviderEligibilityOutcome::QualifiedEligible));
}

#[test]
fn adapter_limitations_block_exact_readiness() {
    let mut evidence = aligned_evidence();
    evidence.limitations.push("entity:9:tombstone_duplicate_anchor".to_string());
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::FallbackRequired {
            reason: ProviderEligibilityBlockerClass::AdapterLimitation
        }
    ));
}

#[test]
fn dynamic_and_ambiguous_facts_fail_closed() {
    let mut evidence = aligned_evidence();
    evidence.dynamic_boundary_present = true;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Dynamic {
            reason: ProviderEligibilityBlockerClass::DynamicBoundary
        }
    ));

    evidence.dynamic_boundary_present = false;
    evidence.mixed_producer_lineage = true;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Ambiguous {
            reason: ProviderEligibilityBlockerClass::AmbiguousFacts
        }
    ));
}

#[test]
fn edit_producing_requests_require_complete_edit_denominator() {
    let mut subject = aligned_subject();
    subject.profile = ProviderEligibilityRequestProfile::new(
        "textDocument/rename",
        ProviderQueryCapability::References,
        ProviderReadinessRequirement::EditAuthorizing,
        true,
    );
    let requirement =
        ProviderCompletenessRequirementV1::edit_producing(ProviderQueryCapability::References);
    let decision = evaluate_provider_eligibility(&subject, &aligned_evidence(), &requirement);
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Refuse {
            reason: ProviderEligibilityBlockerClass::IncompleteEditDenominator
        }
    ));
}

#[test]
fn cancellation_deadline_and_instrument_terminal_states() {
    let mut subject = aligned_subject();
    subject.cancellation = ProviderCancellationState::Cancelled;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(decision.outcome, ProviderEligibilityOutcome::Cancelled));

    subject = aligned_subject();
    subject.deadline = ProviderQueryDeadline::Expired;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(decision.outcome, ProviderEligibilityOutcome::DeadlineExceeded));

    let mut evidence = aligned_evidence();
    evidence.instrument_state =
        ProviderEligibilityInstrumentState::Failed { reason_code: "instrument_failed".to_string() };
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(decision.outcome, ProviderEligibilityOutcome::InstrumentFailure));
}

#[test]
fn fact_family_mismatch_is_refused() {
    let mut evidence = aligned_evidence();
    evidence.capability = ProviderQueryCapability::Declarations;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Refuse {
            reason: ProviderEligibilityBlockerClass::FactFamilyMismatch
        }
    ));
}

#[test]
fn source_deleted_and_root_removed_lifecycle_blockers() {
    let mut subject = aligned_subject();
    subject.lifecycle = ProviderSubjectLifecycleState::SourceDeleted;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Stale {
            reason: ProviderEligibilityBlockerClass::SourceDeleted
        }
    ));

    subject.lifecycle = ProviderSubjectLifecycleState::RootRemoved;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::Unavailable {
            reason: ProviderEligibilityBlockerClass::RootRemoved
        }
    ));
}

#[test]
fn unknown_required_field_fails_closed_as_not_proven() {
    let mut subject = aligned_subject();
    subject.document_generation = SourceGeneration::Unknown;
    let decision = evaluate_provider_eligibility(
        &subject,
        &aligned_evidence(),
        &references_requirement(false),
    );
    assert!(matches!(decision.outcome, ProviderEligibilityOutcome::NotProven));
}

#[test]
fn recovered_facts_require_fallback_not_exact() {
    let mut evidence = aligned_evidence();
    evidence.recovered_facts_present = true;
    let decision = evaluate_provider_eligibility(
        &aligned_subject(),
        &evidence,
        &references_requirement(false),
    );
    assert!(matches!(
        decision.outcome,
        ProviderEligibilityOutcome::FallbackRequired {
            reason: ProviderEligibilityBlockerClass::PartialOrRecoveredFacts
        }
    ));
}
