use perl_lsp_rs_core::providers::formatting::{
    FormatPosition, FormatRange, FormattingOptions, FormattingProvider,
};
use perl_lsp_rs_core::tooling::perltidy::FormatterMode;
use perl_lsp_rs_core::tooling::perltidy::native::{
    FormatContext, FormatDisposition, FormatEngine, FormatReasonCode, FormatRequestTarget,
};
use perl_lsp_rs_core::tooling::{SubprocessError, SubprocessOutput, SubprocessRuntime};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct RecordingRuntime {
    invoked: Arc<AtomicBool>,
}

impl SubprocessRuntime for RecordingRuntime {
    fn run_command(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        self.invoked.store(true, Ordering::SeqCst);
        Ok(SubprocessOutput {
            stdout: b"my $external = 1;\n".to_vec(),
            stderr: Vec::new(),
            status_code: 0,
        })
    }
}

fn options() -> FormattingOptions {
    FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

#[test]
fn native_parse_refusal_is_not_legitimate_no_change() -> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() });
    let context = FormatContext::new(Some("fixture/broken.pl".to_string()), Some(3));

    let decision = provider.format_document_decision("my $x = ;\n", &options(), &context)?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::SourceParseError);
    assert_eq!(decision.outcome.identity.source_generation, Some(3));
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Native);
    assert!(decision.document.edits.is_empty(), "a parse refusal must not emit edits");
    assert!(!invoked.load(Ordering::SeqCst), "native refusal must not spawn a subprocess");
    Ok(())
}

#[test]
fn disabled_mode_is_a_typed_refusal_not_no_change() -> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() })
        .with_formatter_mode(FormatterMode::Off);

    let decision =
        provider.format_document_decision("my$x=1;\n", &options(), &FormatContext::default())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::FormatterDisabled);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Disabled);
    assert!(decision.document.edits.is_empty(), "disabled formatting must not emit edits");
    assert!(!invoked.load(Ordering::SeqCst), "disabled formatting must not spawn a subprocess");
    Ok(())
}

#[test]
fn external_partial_range_refuses_without_native_substitution_or_spawn()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() })
        .with_formatter_mode(FormatterMode::ExternalLegacy);
    let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 8));

    let decision = provider.format_range_decision(
        "my$x=1;\nmy$y=2;\n",
        &range,
        &options(),
        &FormatContext::default(),
    )?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::UnsafeRange);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Unknown);
    assert_eq!(
        decision.outcome.target,
        FormatRequestTarget::Range {
            range: perl_lsp_rs_core::tooling::perltidy::TextRange::new(
                perl_lsp_rs_core::tooling::perltidy::TextPosition::new(0, 0),
                perl_lsp_rs_core::tooling::perltidy::TextPosition::new(0, 8),
            ),
        },
    );
    assert!(decision.document.edits.is_empty(), "unsafe ranges must not emit edits");
    assert!(!invoked.load(Ordering::SeqCst), "unsafe external ranges must not spawn");
    Ok(())
}

#[test]
fn external_whole_document_records_external_engine_and_invokes_once()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() })
        .with_formatter_mode(FormatterMode::ExternalLegacy);

    let decision =
        provider.format_document_decision("my$x=1;\n", &options(), &FormatContext::default())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::ExternalLegacy);
    assert_eq!(decision.outcome.identity.requested_mode, FormatterMode::ExternalLegacy);
    assert!(invoked.load(Ordering::SeqCst), "external formatting must invoke the runtime");
    assert_eq!(decision.document.edits.len(), 1);
    Ok(())
}

#[test]
fn external_whole_document_range_retains_range_request_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) })
            .with_formatter_mode(FormatterMode::ExternalLegacy);
    let source = "my$x=1;\n";
    let range = FormatRange::whole_document(source);

    let decision =
        provider.format_range_decision(source, &range, &options(), &FormatContext::default())?;

    assert!(
        matches!(decision.outcome.target, FormatRequestTarget::Range { .. }),
        "whole-document range requests must retain range identity"
    );
    Ok(())
}

#[test]
fn whitespace_fallback_runs_only_after_explicit_native_no_change()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() });
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    // Native leaves comment lines unchanged (NoChange) while preserving trailing
    // spaces, so LSP whitespace fallback is the only edit source.
    let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 10));

    let decision = provider.format_range_decision(
        "# trailing   \n",
        &range,
        &formatting_options,
        &FormatContext::default(),
    )?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, "# trailing");
    assert_eq!(
        decision.document.text, "# trailing\n",
        "fallback text must materialize the emitted range edit exactly"
    );
    assert!(
        decision.outcome.change.source_bytes_changed > 0,
        "fallback evidence must count changed source bytes"
    );
    assert!(
        decision.outcome.change.rendered_bytes_changed > 0,
        "fallback evidence must count changed rendered bytes"
    );
    assert!(
        decision.outcome.change.changed_lines > 0,
        "fallback evidence must count changed lines"
    );
    assert!(!invoked.load(Ordering::SeqCst), "native fallback must not spawn a subprocess");
    Ok(())
}

#[test]
fn whitespace_fallback_materializes_crlf_multiline_range_text()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    let source = "my $x = 1;\r\n# trailing   \r\n";
    let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 10));

    let decision = provider.format_range_decision(
        source,
        &range,
        &formatting_options,
        &FormatContext::default(),
    )?;

    assert_eq!(
        decision.document.text, "my $x = 1;\r\n# trailing\r\n",
        "fallback text must preserve CRLF outside the edited line"
    );
    Ok(())
}

#[test]
fn native_fingerprint_includes_post_projection_lsp_options()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let source = "print $x;   \n";
    let base = options();
    let mut trimmed = options();
    trimmed.trim_trailing_whitespace = Some(true);

    let base_decision =
        provider.format_document_decision(source, &base, &FormatContext::default())?;
    let trimmed_decision =
        provider.format_document_decision(source, &trimmed, &FormatContext::default())?;

    assert_ne!(
        base_decision.outcome.identity.config_fingerprint,
        trimmed_decision.outcome.identity.config_fingerprint,
    );
    Ok(())
}

#[test]
fn compatibility_projection_preserves_existing_successful_document_output()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });

    let document = provider.format_document("my$x=1;\n", &options())?;

    assert_eq!(document.edits.len(), 1);
    assert_eq!(document.edits[0].new_text, "my $x = 1;\n");
    Ok(())
}
