use perl_lsp_rs_core::providers::formatting::{FormattingOptions, FormattingProvider};
use perl_lsp_rs_core::tooling::perltidy::native::{
    FormatContext, FormatDisposition, FormatReasonCode,
};
use perl_lsp_rs_core::tooling::{SubprocessError, SubprocessOutput, SubprocessRuntime};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct RecordingRuntime(Arc<AtomicBool>);

impl SubprocessRuntime for RecordingRuntime {
    fn run_command(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        self.0.store(true, Ordering::SeqCst);
        Ok(SubprocessOutput { stdout: Vec::new(), stderr: Vec::new(), status_code: 0 })
    }
}

#[test]
fn decision_receipt_serializes_outcome_without_duplicating_source_text()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime(invoked.clone()));
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let context = FormatContext::new(Some("fixture/receipt.pl".to_string()), Some(9));

    let decision = provider.format_document_decision("my$x=1;\n", &options, &context)?;
    let value = serde_json::to_value(&decision)?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.outcome.reason, FormatReasonCode::Applied);
    assert!(
        value.get("document").is_none(),
        "the serialized receipt must not duplicate document source text"
    );
    assert_eq!(value["outcome"]["disposition"], "applied");
    assert_eq!(value["outcome"]["identity"]["source_generation"], 9);
    assert_eq!(value["outcome"]["identity"]["source_id"], "fixture/receipt.pl");
    assert!(!invoked.load(Ordering::SeqCst), "native formatting must not invoke the runtime");

    Ok(())
}
