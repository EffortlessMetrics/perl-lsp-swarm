use perl_semantic_facts::framework::{
    AdapterAuthorityError, AdapterBudget, AdapterCancellation, AdapterCancellationControl,
    AdapterDescriptor, AdapterDisposition, AdapterId, AdapterInput, AdapterOutcome, AdapterResult,
    AdapterSourceScope, FactClass, FactSink, FactSinkId, NoopAdapterCancellationControl,
};
use perl_semantic_facts::{FileId, SourceGeneration};
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

trait TestAdapter {
    fn run(input: &AdapterInput, control: &dyn AdapterCancellationControl) -> AdapterResult;
}

struct MinimalAdapter;

impl TestAdapter for MinimalAdapter {
    fn run(input: &AdapterInput, control: &dyn AdapterCancellationControl) -> AdapterResult {
        let outcome = if control.is_cancelled() || input.cancellation.is_cancelled {
            AdapterOutcome::Cancelled
        } else {
            AdapterOutcome::Applied {
                sink: FactSink::new(FactSinkId(9), input.descriptor.adapter_id),
                limitations: Vec::new(),
            }
        };
        AdapterResult::new(
            input.descriptor.clone(),
            input.source_scope.clone(),
            input.source_scope.primary_generation.clone(),
            outcome,
        )
    }
}

#[test]
#[allow(deprecated)]
fn external_adapter_can_construct_and_validate_public_sdk_values() {
    let input = AdapterInput::new(
        AdapterDescriptor::new(
            AdapterId(9),
            "minimal",
            "Example",
            None,
            1,
            AdapterDisposition::Production,
        ),
        AdapterSourceScope::new(FileId(1), SourceGeneration::known("source-1"), None, None, None),
        vec![FactClass::Diagnostics],
        Vec::new(),
        Some(AdapterBudget::new(1, 1024)),
        AdapterCancellation::active(),
    );
    let result = MinimalAdapter::run(&input, &NoopAdapterCancellationControl);

    assert!(result.is_authoritative_against(&input));
    assert_eq!(
        result.validate_authority(),
        Err(AdapterAuthorityError::InputRequired),
        "an unbound result fails closed and names the cause"
    );
}

/// The live control must be what carries cancellation, not the admission snapshot.
///
/// Both calls use the same `input`, whose `AdapterCancellation` snapshot stays
/// active for the whole test. Only the shared flag changes between them, so a
/// build that read the snapshot instead of the control — or ignored the control
/// entirely — would return `Applied` twice and fail here.
#[test]
fn live_control_cancels_an_input_whose_admission_snapshot_stayed_active() {
    let input = AdapterInput::new(
        AdapterDescriptor::new(
            AdapterId(9),
            "minimal",
            "Example",
            None,
            1,
            AdapterDisposition::Production,
        ),
        AdapterSourceScope::new(FileId(1), SourceGeneration::known("source-1"), None, None, None),
        vec![FactClass::Diagnostics],
        Vec::new(),
        Some(AdapterBudget::new(1, 1024)),
        AdapterCancellation::active(),
    );

    let flag = Arc::new(AtomicBool::new(false));
    let control = AtomicCancellation(Arc::clone(&flag));

    let before = MinimalAdapter::run(&input, &control);
    assert!(
        matches!(before.outcome, AdapterOutcome::Applied { .. }),
        "an uncancelled invocation applies"
    );

    // Cancellation requested after the input was admitted.
    flag.store(true, Ordering::SeqCst);

    let after = MinimalAdapter::run(&input, &control);
    assert!(
        !input.cancellation.is_cancelled,
        "the admission snapshot is immutable and never observed the request"
    );
    assert!(
        matches!(after.outcome, AdapterOutcome::Cancelled),
        "the live control must reach the adapter after dispatch"
    );
    assert_eq!(
        after.validate_authority_against(&input),
        Err(AdapterAuthorityError::IncompleteOutcome),
        "a cancelled invocation is never publication authority"
    );
}
