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

/// A subprocess runtime whose rendered output is chosen by the test, so the
/// external adapter path can be driven to a real change and to a byte-for-byte
/// no-op.
#[derive(Clone)]
struct ScriptedRuntime {
    stdout: String,
}

impl SubprocessRuntime for ScriptedRuntime {
    fn run_command(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        Ok(SubprocessOutput {
            stdout: self.stdout.clone().into_bytes(),
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

fn context() -> FormatContext {
    FormatContext::default()
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
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() })
        .with_formatter_mode(FormatterMode::ExternalLegacy);
    let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 8));

    let decision = provider.format_range_decision(
        "my$x=1;\nmy$y=2;\n",
        &range,
        &formatting_options,
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
    assert_eq!(
        decision.document.text, "my$x=1;\nmy$y=2;\n",
        "a refusal must never be followed by whitespace-fallback edits"
    );
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
fn whitespace_options_edit_exactly_inside_the_admitted_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() });
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    // Native leaves comment lines unchanged (NoChange), so LSP whitespace
    // options are the only possible edit source.
    let source = "# trailing   \n";

    // Covering the complete line body admits exactly those bytes.
    let full_body = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 13));
    let decision =
        provider.format_range_decision(source, &full_body, &formatting_options, &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].range.start.character, 0);
    assert_eq!(decision.document.edits[0].range.end.character, 13);
    assert_eq!(decision.document.edits[0].new_text, "# trailing");
    assert_eq!(decision.document.text, "# trailing\n");
    assert!(
        decision.outcome.change.source_bytes_changed > 0,
        "fallback evidence must count changed source bytes"
    );
    assert!(!invoked.load(Ordering::SeqCst), "native fallback must not spawn a subprocess");

    // A partial interval whose end stops before the trailing spaces must stay
    // a legitimate no-change: bytes outside the admitted interval are never
    // rewritten by projection.
    let partial = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 10));
    let decision =
        provider.format_range_decision(source, &partial, &formatting_options, &context())?;
    assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(decision.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(decision.document.edits.is_empty());
    assert_eq!(decision.document.text, source);
    Ok(())
}

#[test]
fn native_range_composes_whitespace_options_after_native_formatting()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    let source = "my$x=1;";
    let range = FormatRange::whole_document(source);

    let decision =
        provider.format_range_decision(source, &range, &formatting_options, &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.document.text, "my $x = 1;\n");
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].range, range);
    assert_eq!(decision.document.edits[0].new_text, "my $x = 1;\n");
    Ok(())
}

#[test]
fn native_partial_range_that_widens_is_downgraded_without_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let source = "my$x=1;\n";
    let range = FormatRange::new(FormatPosition::new(0, 2), FormatPosition::new(0, 3));

    let decision = provider.format_range_decision(source, &range, &options(), &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::FailedOrNotProven);
    assert_eq!(decision.outcome.reason, FormatReasonCode::InstrumentFailure);
    assert!(decision.document.edits.is_empty());
    assert_eq!(decision.document.text, source);
    // The engine rendered a change that containment then rejected. Because no
    // edit is admitted, the withheld decision carries no applied-change
    // summary — the intermediate evidence must not survive (#7585).
    assert_eq!(decision.outcome.change.edit_count, 0);
    assert_eq!(decision.outcome.change.source_bytes_changed, 0);
    assert_eq!(decision.outcome.change.rendered_bytes_changed, 0);
    assert_eq!(decision.outcome.change.changed_lines, 0);
    Ok(())
}

#[test]
fn final_newline_options_recognize_crlf_and_bare_cr() -> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut trim = options();
    trim.trim_final_newlines = Some(true);

    for source in ["# comment\r\n", "# comment\r"] {
        let decision = provider.format_document_decision(source, &trim, &context())?;
        assert_eq!(
            decision.outcome.disposition,
            FormatDisposition::Applied,
            "source={source:?}, reason={:?}",
            decision.outcome.reason
        );
        assert_eq!(decision.document.text, "# comment", "source={source:?}");
    }
    Ok(())
}

#[test]
fn whitespace_options_preserve_crlf_outside_the_admitted_line()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    let source = "my $x = 1;\r\n# trailing   \r\n";
    let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 13));

    let decision =
        provider.format_range_decision(source, &range, &formatting_options, &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(
        decision.document.text, "my $x = 1;\r\n# trailing\r\n",
        "whitespace projection must preserve CRLF separators outside the edited line"
    );
    Ok(())
}

#[test]
fn surrogate_split_endpoints_refuse_before_any_engine_runs()
-> Result<(), Box<dyn std::error::Error>> {
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = FormattingProvider::new(RecordingRuntime { invoked: invoked.clone() });
    let source = "a🦀b\ncalc;\n";
    let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 2));

    let decision = provider.format_range_decision(source, &range, &options(), &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::UnsafeRange);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Unknown);
    assert!(
        decision.outcome.next_action.as_deref().is_some_and(|action| !action.is_empty()),
        "an invalid endpoint must carry its mechanically known next action"
    );
    assert!(decision.document.edits.is_empty());
    assert_eq!(decision.document.text, source);
    Ok(())
}

#[test]
fn out_of_bounds_end_lines_refuse_without_clamping_to_the_final_line()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.trim_trailing_whitespace = Some(true);
    let source = "one   \ntwo   \n";
    let range = FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(9, 5));

    let decision =
        provider.format_range_decision(source, &range, &formatting_options, &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::UnsafeRange);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Unknown);
    assert!(
        decision.document.edits.is_empty() && decision.document.text == source,
        "the old line clamp must not trim line zero for an out-of-document end"
    );
    Ok(())
}

#[test]
fn reversed_ranges_refuse_deterministically() -> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let source = "abcdef\n";
    let range = FormatRange::new(FormatPosition::new(0, 5), FormatPosition::new(0, 2));

    let decision = provider.format_range_decision(source, &range, &options(), &context())?;

    assert_eq!(decision.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(decision.outcome.reason, FormatReasonCode::UnsafeRange);
    assert!(decision.document.edits.is_empty());
    Ok(())
}

#[test]
fn terminal_empty_eof_line_positions_are_admissible_like_one_element_multi_range()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let source = "calc;\n";
    let point_at_eof_line = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 0));

    let decision =
        provider.format_range_decision(source, &point_at_eof_line, &options(), &context())?;

    // The strict multi-range geometry admits this point; single-range
    // admission must agree instead of dropping the terminal empty line.
    assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Native);
    assert!(decision.document.edits.is_empty());
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

#[test]
fn native_projection_insert_final_newline_preserves_crlf_after_generated_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "while($n){next;}\r\n";

    let decision = provider.format_document_decision(
        source,
        &formatting_options,
        &FormatContext::default(),
    )?;

    assert_eq!(decision.document.text, "while ($n) {\r\n    next;\r\n}\r\n");
    assert_eq!(decision.document.edits.len(), 1);
    assert!(decision.document.text.ends_with("}\r\n"));
    assert!(!decision.document.text.ends_with("}\r\n\r\n"));
    assert!(decision.document.edits[0].new_text.ends_with("}\r\n"));
    assert!(!decision.document.edits[0].new_text.ends_with("}\r\n\r\n"));
    assert!(!decision.document.edits[0].new_text.contains("\r\n\n"));
    Ok(())
}

#[test]
fn native_projection_insert_final_newline_uses_last_lf_then_crlf_ending()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "my $before=1;\nwhile($n){next;}\r\n";

    let decision = provider.format_document_decision(
        source,
        &formatting_options,
        &FormatContext::default(),
    )?;

    let expected = "my $before = 1;\nwhile ($n) {\r\n    next;\r\n}\r\n";
    assert_eq!(decision.document.text, expected);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, expected);
    assert!(decision.document.text.ends_with("}\r\n"));
    assert!(!decision.document.text.ends_with("}\n"));
    Ok(())
}

#[test]
fn native_projection_insert_final_newline_uses_last_crlf_then_lf_ending()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "my $before=1;\r\nwhile($n){next;}\n";

    let decision = provider.format_document_decision(
        source,
        &formatting_options,
        &FormatContext::default(),
    )?;

    let expected = "my $before = 1;\r\nwhile ($n) {\n    next;\n}\n";
    assert_eq!(decision.document.text, expected);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, expected);
    assert!(decision.document.text.ends_with("}\n"));
    assert!(!decision.document.text.ends_with("}\r\n"));
    Ok(())
}

#[test]
fn native_range_projection_mixed_prefix_lf_then_crlf_uses_edited_line_ending()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "my $before=1;\nwhile($n){next;}\r\n";
    let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 16));

    let decision = provider.format_range_decision(
        source,
        &range,
        &formatting_options,
        &FormatContext::default(),
    )?;

    let expected = "my $before=1;\nwhile ($n) {\r\n    next;\r\n}\r\n";
    assert_eq!(decision.document.text, expected);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, "while ($n) {\r\n    next;\r\n}");
    Ok(())
}

#[test]
fn native_range_projection_mixed_prefix_crlf_then_lf_uses_edited_line_ending()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "my $before=1;\r\nwhile($n){next;}\n";
    let range = FormatRange::new(FormatPosition::new(1, 0), FormatPosition::new(1, 16));

    let decision = provider.format_range_decision(
        source,
        &range,
        &formatting_options,
        &FormatContext::default(),
    )?;

    let expected = "my $before=1;\r\nwhile ($n) {\n    next;\n}\n";
    assert_eq!(decision.document.text, expected);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, "while ($n) {\n    next;\n}");
    Ok(())
}

#[test]
fn native_projection_trim_then_insert_retains_crlf_document_ending()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });
    let mut formatting_options = options();
    formatting_options.insert_final_newline = Some(true);
    formatting_options.trim_final_newlines = Some(true);
    let source = "my $x=1;\r\n";

    let decision = provider.format_document_decision(
        source,
        &formatting_options,
        &FormatContext::default(),
    )?;

    let expected = "my $x = 1;\r\n";
    assert_eq!(decision.document.text, expected);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, expected);
    assert!(!decision.document.text.ends_with("\r\r\n"));
    Ok(())
}

/// The terminal envelope must stay internally consistent across every public
/// formatting path (#7585).
///
/// `Applied` means the caller received edits that really change bytes, so its
/// change summary is non-zero; `NoChange` means no edits and a zero summary;
/// refusals and failures never carry an applied-change summary. Asserting the
/// predicate over a representative corpus keeps the invariant total instead of
/// leaving it to hold by coincidence at each individual call site.
#[test]
fn every_terminal_decision_keeps_a_consistent_change_envelope()
-> Result<(), Box<dyn std::error::Error>> {
    let provider =
        FormattingProvider::new(RecordingRuntime { invoked: Arc::new(AtomicBool::new(false)) });

    let mut trimming = options();
    trimming.trim_trailing_whitespace = Some(true);
    let mut final_newline = options();
    final_newline.insert_final_newline = Some(true);

    // (source, options) pairs spanning applied, legitimate no-change, whitespace
    // fallback, refusal, and failed/not-proven paths.
    let documents = [
        ("my$x=1;\n", options()),              // native applied
        ("my $x = 1;\n", options()),           // native legitimate no-change
        ("# trailing   \n", trimming.clone()), // whitespace fallback emits an edit
        ("# trailing\n", trimming.clone()),    // fallback finds no difference
        ("my $x = ;\n", options()),            // source parse refusal
        ("print 1;;;\n", options()),           // unsupported-syntax refusal
        ("my $x = 1;", final_newline),         // final-newline insertion
        ("", options()),                       // empty document
    ];

    for (source, formatting_options) in documents {
        let decision =
            provider.format_document_decision(source, &formatting_options, &context())?;
        assert_envelope_is_consistent(source, &decision);
    }

    let ranges = [
        FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 13)),
        FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 10)),
        FormatRange::new(FormatPosition::new(0, 0), FormatPosition::new(0, 0)),
    ];
    for range in ranges {
        let decision =
            provider.format_range_decision("# trailing   \n", &range, &trimming, &context())?;
        assert_envelope_is_consistent("# trailing   \n", &decision);
    }

    // A partial-line range whose line needs formatting: the engine emits a
    // full-line edit, containment rejects it, and the decision is downgraded to
    // not-proven with no edits. Without this case the withheld arm of the
    // predicate below is never exercised.
    let containment_rejection =
        FormatRange::new(FormatPosition::new(0, 2), FormatPosition::new(0, 3));
    let decision = provider.format_range_decision(
        "my$x=1;\n",
        &containment_rejection,
        &options(),
        &context(),
    )?;
    assert_eq!(decision.outcome.disposition, FormatDisposition::FailedOrNotProven);
    assert_envelope_is_consistent("my$x=1;\n", &decision);

    // The external adapter is the fourth terminal entry point. Drive it to a
    // real change and to a byte-for-byte no-op so both arms of its envelope
    // normalization are pinned by the same predicate.
    let source = "my$x=1;\n";
    for stdout in [source, "my $x = 1;\n"] {
        let external = FormattingProvider::new(ScriptedRuntime { stdout: stdout.to_string() })
            .with_formatter_mode(FormatterMode::ExternalLegacy);
        let decision = external.format_document_decision(source, &options(), &context())?;
        let expected =
            if stdout == source { FormatDisposition::NoChange } else { FormatDisposition::Applied };
        assert_eq!(decision.outcome.disposition, expected, "external stdout={stdout:?}");
        assert_envelope_is_consistent(source, &decision);
    }
    Ok(())
}

fn assert_envelope_is_consistent(
    source: &str,
    decision: &perl_lsp_rs_core::providers::formatting::FormattingDecision,
) {
    let change = decision.outcome.change;
    let edits = &decision.document.edits;
    assert_eq!(
        change.edit_count,
        edits.len(),
        "edit_count must equal the returned edits for {source:?}"
    );

    match decision.outcome.disposition {
        FormatDisposition::Applied => {
            assert!(!edits.is_empty(), "Applied must return edits for {source:?}");
            assert_ne!(
                decision.document.text, source,
                "Applied must change rendered bytes for {source:?}"
            );
            assert!(
                change.source_bytes_changed > 0 && change.rendered_bytes_changed > 0,
                "Applied must carry non-zero change evidence for {source:?}"
            );
        }
        FormatDisposition::NoChange => {
            assert!(edits.is_empty(), "NoChange must return no edits for {source:?}");
            assert_eq!(
                decision.document.text, source,
                "NoChange must leave the source bytes intact for {source:?}"
            );
            assert_eq!(change.source_bytes_changed, 0);
            assert_eq!(change.rendered_bytes_changed, 0);
            assert_eq!(change.changed_lines, 0);
        }
        FormatDisposition::Refused | FormatDisposition::FailedOrNotProven => {
            assert!(edits.is_empty(), "a withheld decision must not return edits for {source:?}");
            assert_eq!(
                decision.document.text, source,
                "a withheld decision must retain the source for {source:?}"
            );
            assert_eq!(
                change.source_bytes_changed, 0,
                "a withheld decision carries no applied-change summary for {source:?}"
            );
        }
    }
}
