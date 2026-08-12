use perl_semantic_facts::framework::{
    AdapterBudget, AdapterCancellation, AdapterCancellationControl, AdapterDescriptor,
    AdapterDisposition, AdapterId, AdapterInput, AdapterOutcome, AdapterResult, AdapterSourceScope,
    FactClass, FactSink, FactSinkId, NoopAdapterCancellationControl,
};
use perl_semantic_facts::{FileId, SourceGeneration};

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
    assert!(!result.is_authoritative(), "unbound results must fail closed");
}
