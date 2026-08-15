use perl_lsp_perltidy::native::{
    FormatConfig, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatReasonCode, FormatRequestTarget, FormatterMode, NativeFormatter, TextPosition, TextRange,
};

#[test]
fn typed_document_result_reports_applied_with_identity_and_evidence() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\n";
    let context = FormatContext::new(Some("fixture/basic.pl".to_string()), Some(7));

    let typed = formatter.format_document_typed(source, &FormatConfig::default(), &context);

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(typed.outcome.identity.source_id.as_deref(), Some("fixture/basic.pl"));
    assert_eq!(typed.outcome.identity.source_generation, Some(7));
    assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(typed.outcome.identity.requested_mode, FormatterMode::Native);
    assert!(typed.outcome.identity.content_digest.starts_with("source-v1:"));
    assert!(typed.outcome.identity.config_fingerprint.starts_with("format-config-v1:"));
    assert_eq!(typed.outcome.target, FormatRequestTarget::Document);
    assert_eq!(typed.outcome.change.edit_count, typed.result.edits.len());
    assert!(typed.outcome.change.source_bytes_changed > 0);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::Proven);
    assert_eq!(typed.outcome.safety.parse_after, FormatEvidenceState::Proven);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Proven);
}

#[test]
fn typed_document_result_distinguishes_legitimate_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
    assert_eq!(typed.outcome.change.source_bytes_changed, 0);
    assert_eq!(typed.outcome.change.rendered_bytes_changed, 0);
}

#[test]
fn typed_document_result_does_not_flatten_disabled_mode_to_no_change() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { mode: FormatterMode::Off, ..FormatConfig::default() };

    let typed = formatter.format_document_typed("my $x = ;\n", &config, &FormatContext::default());

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::FormatterDisabled);
    assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Disabled);
    assert_eq!(typed.outcome.identity.requested_mode, FormatterMode::Off);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::NotRun);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
}

#[test]
fn typed_document_result_does_not_flatten_literal_refusal_to_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $matched = $text =~ /needle/i;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported,);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Refused,);
    assert_eq!(typed.result.formatted, source);
    assert!(typed.result.edits.is_empty());
    assert_eq!(typed.result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn typed_document_result_does_not_flatten_parse_refusal_to_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $x = ;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::SourceParseError);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::Failed);
    assert_eq!(typed.outcome.safety.parse_after, FormatEvidenceState::NotRun);
    assert_eq!(typed.result.formatted, source);
    assert!(typed.result.edits.is_empty());
    assert_eq!(typed.result.diagnostics[0].code, "native.format.parse_error");
}

#[test]
fn typed_range_result_records_the_exact_requested_range() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\nmy$y=2;\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::new(Some("fixture/range.pl".to_string()), Some(11)),
    );

    assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range });
    assert_eq!(typed.outcome.identity.source_generation, Some(11));
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert!(typed.result.edits.iter().all(|edit| edit.range.start.line >= 1));
}

#[test]
fn typed_config_fingerprint_changes_with_load_bearing_configuration() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\n";
    let default_config = FormatConfig::default();
    let narrow_config = FormatConfig { line_width: 40, ..FormatConfig::default() };

    let default_result =
        formatter.format_document_typed(source, &default_config, &FormatContext::default());
    let narrow_result =
        formatter.format_document_typed(source, &narrow_config, &FormatContext::default());

    assert_ne!(
        default_result.outcome.identity.config_fingerprint,
        narrow_result.outcome.identity.config_fingerprint,
    );
    assert_eq!(
        default_result.outcome.identity.content_digest,
        narrow_result.outcome.identity.content_digest,
    );
}

#[test]
fn typed_outcome_serializes_without_parsing_diagnostic_prose() -> Result<(), serde_json::Error> {
    let formatter = NativeFormatter::new();
    let typed = formatter.format_document_typed(
        "my $x = ;\n",
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    let value = serde_json::to_value(&typed)?;

    assert_eq!(value["outcome"]["disposition"], "refused");
    assert_eq!(value["outcome"]["reason"], "source_parse_error");
    assert_eq!(value["outcome"]["identity"]["actual_engine"], "native");
    assert!(value["result"]["edits"].as_array().is_some_and(Vec::is_empty));

    Ok(())
}
