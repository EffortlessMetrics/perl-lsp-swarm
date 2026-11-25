//! The retired UTF-16 surface delegates to the canonical byte formatter.
#![allow(deprecated)]

use perl_lsp_perltidy::native::{
    EditSpec, FormatContext, FormatDisposition, FormatEngine, FormatExecutionStrategy,
    FormatOutcome, FormatReasonCode, FormatRequestTarget, PositionEncoding, apply_edits_exact,
};
use perl_lsp_perltidy::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatDoc, FormatterMode,
    KeywordSpacing, NativeFormatter, NativePipelineCounters, PerlFormatter, TextPosition,
    TextRange,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn original_mode_config_trait_and_typed_dto_shapes_remain_available() -> TestResult {
    let config = FormatConfig { mode: FormatterMode::Native, ..FormatConfig::default() };
    let formatter = NativeFormatter::new();
    let trait_object: &dyn PerlFormatter = &formatter;
    assert!(trait_object.format_document("my$x=1;\n", &config).changed);
    let typed = formatter.format_document_typed(
        "my$x=1;\n",
        &config,
        &FormatContext::new(Some("compat.pl".into()), Some(3)),
    );
    let FormatOutcome {
        disposition,
        reason,
        identity,
        target,
        execution,
        change,
        safety,
        next_action,
    } = typed.outcome;
    assert_eq!(disposition, FormatDisposition::Applied);
    assert_eq!(reason, FormatReasonCode::Applied);
    assert_eq!(identity.source_id.as_deref(), Some("compat.pl"));
    assert_eq!(identity.source_generation, Some(3));
    assert_eq!(target, FormatRequestTarget::Document);
    assert_eq!(execution, FormatExecutionStrategy::NativeDocument);
    assert!(change.edit_count > 0);
    assert_eq!(safety.parse_before, perl_tidy::FormatEvidenceState::Proven);
    assert!(next_action.is_none());
    let external = FormatConfig::external_legacy();
    let serialized = serde_json::to_value(&external)?;
    assert_eq!(serialized["mode"], "external-legacy");
    assert_eq!(serde_json::from_value::<FormatConfig>(serialized)?, external);
    Ok(())
}

#[test]
fn native_bytes_style_and_identity_are_owned_by_the_new_package() -> TestResult {
    let source = "sub f { my$x=1; }\r\n";
    let policy = perl_tidy::NativeFormatConfig {
        indent_width: 2,
        line_width: 40,
        brace_placement: BracePlacement::NextLine,
        else_placement: ElsePlacement::SeparateLine,
        keyword_spacing: KeywordSpacing::Compact,
        final_newline: FinalNewline::Insert,
        ..perl_tidy::NativeFormatConfig::default()
    };
    let context = FormatContext::default();
    let native = perl_tidy::NativeFormatter::new().format_document_typed(source, &policy, &context);
    let legacy =
        NativeFormatter::new().format_document_typed(source, &policy.clone().into(), &context);
    assert_eq!(native.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(legacy.result.formatted, native.result.formatted);
    assert_eq!(legacy.outcome.identity, native.outcome.identity);
    assert_eq!(legacy.outcome.disposition, native.outcome.disposition);
    assert_eq!(legacy.outcome.safety, native.outcome.safety);
    assert_eq!(legacy.result.edits[0].range, TextRange::whole_document(source));
    assert_legacy_edits_apply(source, &legacy.result)?;
    Ok(())
}

#[test]
fn legacy_ir_variants_delegate_rendering_with_the_same_style() {
    let legacy = FormatDoc::group(vec![
        FormatDoc::text("one"),
        FormatDoc::SoftLine,
        FormatDoc::indent(vec![FormatDoc::HardLine, FormatDoc::literal_preserve("two")]),
    ]);
    let native = perl_tidy::FormatDoc::group(vec![
        perl_tidy::FormatDoc::text("one"),
        perl_tidy::FormatDoc::SoftLine,
        perl_tidy::FormatDoc::indent(vec![
            perl_tidy::FormatDoc::HardLine,
            perl_tidy::FormatDoc::literal_preserve("two"),
        ]),
    ]);
    let policy = perl_tidy::NativeFormatConfig { indent_width: 2, ..Default::default() };
    assert_eq!(legacy.render(&policy.clone().into()), native.render(&policy));
}

#[test]
fn off_and_external_selection_never_invoke_the_native_pipeline() {
    let source = "my $x = ;\n";
    for mode in [FormatterMode::Off, FormatterMode::ExternalLegacy] {
        let config = FormatConfig { mode, ..FormatConfig::default() };
        let mut counters = NativePipelineCounters::default();
        let typed = NativeFormatter::new().format_document_typed_with_counters(
            source,
            &config,
            &FormatContext::default(),
            &mut counters,
        );
        assert_eq!(typed.result.formatted, source);
        assert!(typed.result.edits.is_empty());
        assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
        assert_eq!(typed.outcome.identity.requested_mode, mode);
        assert_eq!(counters.pipeline_invocations, 0);
        assert_eq!(counters.parse_gate_invocations, 0);
        if mode == FormatterMode::Off {
            assert_eq!(typed.outcome.reason, FormatReasonCode::FormatterDisabled);
            assert_eq!(typed.outcome.execution, FormatExecutionStrategy::Disabled);
            assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Disabled);
            assert!(typed.result.diagnostics.is_empty());
        } else {
            assert_eq!(typed.outcome.reason, FormatReasonCode::InvalidConfiguration);
            assert_eq!(typed.outcome.execution, FormatExecutionStrategy::NotStarted);
            assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Unknown);
            assert_eq!(typed.result.diagnostics[0].code, "native.format.external_selection");
        }
    }
}

#[test]
fn utf16_range_preserves_requested_target_and_explicit_legacy_line_selection() -> TestResult {
    let source = "# 😀\r\nmy$x=1;\r\nmy$y=2;\r\n";
    let requested = TextRange::new(TextPosition::new(1, 2), TextPosition::new(1, 5));
    let mut counters = NativePipelineCounters::default();
    let typed = NativeFormatter::new().format_range_typed_with_counters(
        source,
        requested,
        &FormatConfig::default(),
        &FormatContext::default(),
        &mut counters,
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range: requested });
    assert_eq!(typed.outcome.execution, FormatExecutionStrategy::NativeRangeOrIsland);
    assert_eq!(typed.result.formatted, "# 😀\r\nmy $x = 1;\r\nmy$y=2;\r\n");
    assert_eq!(
        typed.result.edits[0].range,
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 7))
    );
    assert_eq!(counters.pipeline_invocations, 1);
    assert_legacy_edits_apply(source, &typed.result)?;
    Ok(())
}

#[test]
fn surrogate_interior_and_out_of_bounds_ranges_are_refused_before_parsing() {
    let source = "# 😀\nmy$x=1;\n";
    for range in [
        TextRange::new(TextPosition::new(0, 3), TextPosition::new(0, 4)),
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 99)),
        TextRange::new(TextPosition::new(1, 99), TextPosition::new(2, 0)),
        TextRange::new(TextPosition::new(2, 0), TextPosition::new(1, 0)),
    ] {
        let mut counters = NativePipelineCounters::default();
        let typed = NativeFormatter::new().format_range_typed_with_counters(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
            &mut counters,
        );
        assert_eq!(typed.outcome.reason, FormatReasonCode::UnsafeRange);
        assert_eq!(typed.outcome.execution, FormatExecutionStrategy::NotStarted);
        assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range });
        assert_eq!(typed.result.formatted, source);
        assert!(typed.result.edits.is_empty());
        assert_eq!(counters.pipeline_invocations, 0);
        assert_eq!(counters.source_parse_gate_invocations, 0);
    }
}

fn assert_legacy_edits_apply(source: &str, result: &perl_lsp_perltidy::FormatResult) -> TestResult {
    // The retained proof-only oracle is independent of the production adapter.
    let edits = result
        .edits
        .iter()
        .map(|edit| {
            EditSpec::new(
                edit.range.start.line,
                edit.range.start.character,
                edit.range.end.line,
                edit.range.end.character,
                edit.new_text.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        apply_edits_exact(source, &edits, PositionEncoding::Utf16CodeUnits)?,
        result.formatted
    );
    Ok(())
}
