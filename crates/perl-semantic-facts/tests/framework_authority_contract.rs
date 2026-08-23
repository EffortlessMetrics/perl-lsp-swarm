use perl_semantic_facts::framework::{
    AdapterAuthorityError, AdapterBudget, AdapterCancellation, AdapterCancellationControl,
    AdapterDescriptor, AdapterDisposition, AdapterId, AdapterInput, AdapterOutcome, AdapterResult,
    AdapterSourceScope, EmittedFact, FactClass, FactLimitation, FactSink, FactSinkId,
};
use perl_semantic_facts::{
    AnchorId, Confidence, EntityId, FactId, FileId, LifecyclePhase, Provenance, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactKind, SemanticFreshness, SemanticProducer,
    SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct AtomicCancellation(Arc<AtomicBool>);

impl AdapterCancellationControl for AtomicCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn descriptor(disposition: AdapterDisposition) -> AdapterDescriptor {
    AdapterDescriptor::new(AdapterId(7), "moo", "Moo", None, 1, disposition)
}

fn scope() -> AdapterSourceScope {
    AdapterSourceScope::new(
        FileId(3),
        SourceGeneration::known("source-1"),
        None,
        Some(AnchorId(4)),
        Some("Example".to_string()),
    )
}

fn envelope(provenance: Provenance) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        FactId(5),
        Some(EntityId(6)),
        SemanticFactKind::Declaration,
        SourceAnchor::new(Some(AnchorId(8)), FileId(3), 10, 20),
        SourceGeneration::known("source-1"),
        None,
        Some("Example".to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(provenance),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        None,
        Vec::new(),
        if provenance == Provenance::ExactAst {
            SemanticReasonCode::ExactSource
        } else {
            SemanticReasonCode::GeneratedFromSource
        },
    )
}

fn result(disposition: AdapterDisposition, fact: EmittedFact) -> AdapterResult {
    let mut sink = FactSink::new(FactSinkId(9), AdapterId(7));
    sink.facts.push(fact);
    if let Some(bytes) = sink.serialized_payload_bytes() {
        sink.total_payload_bytes = bytes;
    }
    AdapterResult::new(
        descriptor(disposition),
        scope(),
        SourceGeneration::known("source-1"),
        AdapterOutcome::Applied { sink, limitations: Vec::new() },
    )
}

fn input() -> AdapterInput {
    AdapterInput::new(
        descriptor(AdapterDisposition::Production),
        scope(),
        vec![FactClass::GeneratedMembers],
        Vec::new(),
        Some(AdapterBudget::new(2, 4096)),
        AdapterCancellation::active(),
    )
}

#[test]
fn serialized_snapshot_and_live_cancellation_are_distinct() {
    let admission = AdapterCancellation::active();
    assert!(!admission.is_cancelled);

    let flag = Arc::new(AtomicBool::new(false));
    let control = AtomicCancellation(Arc::clone(&flag));
    assert!(!control.is_cancelled());
    flag.store(true, Ordering::SeqCst);
    assert!(control.is_cancelled());
    assert!(!admission.is_cancelled, "durable snapshots must not pretend to be live handles");
}

#[test]
fn nonproduction_output_cannot_become_authority() {
    for disposition in [AdapterDisposition::Shadow, AdapterDisposition::Experimental] {
        let fact = EmittedFact::new(
            FactSinkId(9),
            AdapterId(7),
            "Moo",
            Provenance::FrameworkSynthesis,
            Confidence::High,
            envelope(Provenance::FrameworkSynthesis),
            FactClass::GeneratedMembers,
            None,
            false,
        );
        assert_eq!(
            result(disposition, fact).validate_authority_against(&input()),
            Err(AdapterAuthorityError::NonProduction)
        );
    }
}

#[test]
fn generated_precedence_claim_is_not_publication_authority() {
    let fact = EmittedFact::new(
        FactSinkId(9),
        AdapterId(7),
        "Moo",
        Provenance::FrameworkSynthesis,
        Confidence::High,
        envelope(Provenance::FrameworkSynthesis),
        FactClass::GeneratedMembers,
        None,
        true,
    );
    assert!(fact.is_stronger_than_generated, "compatibility input is retained");
    assert!(!fact.can_override_generated(), "generated provenance cannot validate the hint");
    assert_eq!(
        result(AdapterDisposition::Production, fact).validate_authority_against(&input()),
        Err(AdapterAuthorityError::InvalidFact)
    );
}

#[test]
fn exact_source_precedence_is_generation_bound() {
    let fact = EmittedFact::new(
        FactSinkId(9),
        AdapterId(7),
        "Moo",
        Provenance::ExactAst,
        Confidence::High,
        envelope(Provenance::ExactAst),
        FactClass::GeneratedMembers,
        None,
        true,
    );
    let mut candidate = result(AdapterDisposition::Production, fact);
    assert!(candidate.is_authoritative_against(&input()));
    candidate.invocation_generation = SourceGeneration::known("source-2");
    assert_eq!(
        candidate.validate_authority_against(&input()),
        Err(AdapterAuthorityError::GenerationMismatch)
    );
}

/// An otherwise-authoritative candidate, used as the baseline for falsifiers.
fn authoritative_candidate() -> AdapterResult {
    let fact = EmittedFact::new(
        FactSinkId(9),
        AdapterId(7),
        "Moo",
        Provenance::ExactAst,
        Confidence::High,
        envelope(Provenance::ExactAst),
        FactClass::GeneratedMembers,
        None,
        true,
    );
    result(AdapterDisposition::Production, fact)
}

/// Each structural check must have a falsifier, or deleting it stays green.
///
/// `UnsupportedSchema`, `BlockingLimitation`, `SinkIdentityMismatch`, and
/// `InputMismatch` are distinct branches of `validate_structure` and
/// `validate_authority_against` that no other test reaches.
#[test]
fn each_structural_authority_check_has_a_falsifier() {
    let input = input();
    assert!(
        authoritative_candidate().is_authoritative_against(&input),
        "the baseline must be authoritative, or the mutations below prove nothing"
    );

    let mut wrong_schema = authoritative_candidate();
    wrong_schema.schema_version += 1;
    assert_eq!(
        wrong_schema.validate_authority_against(&input),
        Err(AdapterAuthorityError::UnsupportedSchema)
    );

    let mut blocked = authoritative_candidate();
    if let AdapterOutcome::Applied { limitations, .. } = &mut blocked.outcome {
        limitations.push(FactLimitation::new("dynamic symbol table", true, Confidence::Low));
    }
    assert_eq!(
        blocked.validate_authority_against(&input),
        Err(AdapterAuthorityError::BlockingLimitation)
    );

    let mut foreign_sink = authoritative_candidate();
    if let AdapterOutcome::Applied { sink, .. } = &mut foreign_sink.outcome {
        sink.adapter_id = AdapterId(8);
    }
    assert_eq!(
        foreign_sink.validate_authority_against(&input),
        Err(AdapterAuthorityError::SinkIdentityMismatch)
    );

    // A descriptor field structural validation does not read, so this reaches
    // the input-binding check rather than failing earlier.
    let mut other_descriptor = authoritative_candidate();
    other_descriptor.descriptor.framework_version_constraint = Some(">=2".to_string());
    assert_eq!(
        other_descriptor.validate_authority_against(&input),
        Err(AdapterAuthorityError::InputMismatch)
    );

    let mut other_source_scope = authoritative_candidate();
    other_source_scope.source_scope.primary_content_digest = Some("sha256:other".to_string());
    assert_eq!(
        other_source_scope.validate_authority_against(&input),
        Err(AdapterAuthorityError::InputMismatch)
    );
}
