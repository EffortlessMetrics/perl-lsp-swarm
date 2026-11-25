//! Direct native policy and core-adapter parity, with independent UTF-16 edit replay.
//!
//! Adapter options remain part of the provider's configuration fingerprint even
//! when they request no additional whitespace projection. This proves shared
//! formatting behavior, not equality of the two configuration identities.

use perl_lsp_rs_core::providers::formatting::{
    FormatTextEdit, FormattingDecision, FormattingOptions, FormattingProvider,
};
use perl_position_tracking::{
    PositionEncoding, PositionMappingDisposition, WirePosition, WireRange, wire_range_to_bytes,
};
use perl_source_identity::{ContentDigest, SourceGeneration};
use perl_tidy::{
    ExactSource, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatReasonCode, FormatRequest, FormatRequestTarget, FormatSourceIdentity, FormatterMode,
    NativeFormatConfig, NativeFormatter, TypedFormatResult,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn options() -> FormattingOptions {
    FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

/// Interpret only returned wire edits, using the canonical position owner and
/// a separate string replay rather than the provider's byte-plan applicator.
fn replay_wire_edits(source: &str, edits: &[FormatTextEdit]) -> TestResult<String> {
    let mut mapped = Vec::with_capacity(edits.len());
    for edit in edits {
        let mapping = wire_range_to_bytes(
            source,
            WireRange {
                start: WirePosition {
                    line: edit.range.start.line,
                    character: edit.range.start.character,
                },
                end: WirePosition {
                    line: edit.range.end.line,
                    character: edit.range.end.character,
                },
            },
            PositionEncoding::Utf16,
        );
        assert_eq!(mapping.disposition(), PositionMappingDisposition::Exact);
        let bytes = mapping.byte_range().ok_or("returned UTF-16 edit has no exact byte range")?;
        mapped.push((bytes, edit.new_text.as_str()));
    }
    mapped.sort_by_key(|(range, _)| (range.start, range.end));
    for pair in mapped.windows(2) {
        let left = &pair[0].0;
        let right = &pair[1].0;
        assert!(left.end <= right.start, "returned edits overlap");
        assert!(
            left.start != right.start || (!left.is_empty() && !right.is_empty()),
            "returned edits have an ambiguous insertion boundary"
        );
    }
    let mut replayed = source.to_string();
    for (range, replacement) in mapped.into_iter().rev() {
        replayed.replace_range(range, replacement);
    }
    Ok(replayed)
}

fn assert_shared_result(
    source: &ExactSource<'_>,
    direct: &TypedFormatResult,
    provider: &FormattingDecision,
) -> TestResult {
    assert_eq!(direct.apply_exact(source)?, direct.result.formatted);
    assert_eq!(provider.document.text, direct.result.formatted);
    assert_eq!(
        replay_wire_edits(source.text(), &provider.document.edits)?,
        direct.result.formatted
    );
    assert_eq!(provider.outcome.disposition, direct.outcome.disposition);
    assert_eq!(provider.outcome.reason, direct.outcome.reason);
    assert_eq!(provider.outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(direct.outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(provider.outcome.identity.requested_mode, FormatterMode::Native);
    assert_eq!(direct.outcome.identity.requested_mode, FormatterMode::Native);
    assert_eq!(provider.outcome.execution, direct.outcome.execution);
    assert_eq!(provider.outcome.target, FormatRequestTarget::Document);
    assert_eq!(direct.outcome.target, FormatRequestTarget::Document);
    assert_eq!(provider.outcome.admitted, Some(source.whole_document()));
    assert_eq!(provider.outcome.admitted, direct.outcome.admitted);
    assert!(provider.outcome.widening.is_none());
    assert!(direct.outcome.widening.is_none());
    assert_eq!(provider.outcome.coverage, direct.outcome.coverage);
    assert_eq!(provider.outcome.source, *source.identity());
    assert_eq!(provider.outcome.source, direct.outcome.source);
    assert_eq!(provider.outcome.identity.content_digest, source.identity().content.to_string());
    assert_eq!(provider.outcome.identity.content_digest, direct.outcome.identity.content_digest);
    assert_eq!(provider.outcome.identity.source_id, direct.outcome.identity.source_id);
    assert_eq!(
        provider.outcome.identity.source_generation,
        direct.outcome.identity.source_generation
    );
    assert_eq!(provider.outcome.change, direct.outcome.change);
    assert_eq!(provider.outcome.safety, direct.outcome.safety);
    assert_ne!(
        provider.outcome.identity.config_fingerprint, direct.outcome.identity.config_fingerprint,
        "the adapter binds its complete options separately from pure native policy"
    );
    if direct.outcome.disposition == FormatDisposition::Applied {
        assert!(!provider.document.edits.is_empty());
        assert_ne!(provider.document.text, source.text());
    } else {
        assert!(provider.document.edits.is_empty());
        assert_eq!(provider.document.text, source.text());
    }
    Ok(())
}

fn exercise_cases() -> TestResult {
    struct Case {
        name: &'static str,
        source: &'static str,
        expected: &'static str,
        disposition: FormatDisposition,
        reason: FormatReasonCode,
        config: NativeFormatConfig,
    }
    let cases = [
        Case {
            name: "applied",
            source: "my$x=1;\n",
            expected: "my $x = 1;\n",
            disposition: FormatDisposition::Applied,
            reason: FormatReasonCode::Applied,
            config: NativeFormatConfig::default(),
        },
        Case {
            name: "no_change",
            source: "my $x = 1;\n",
            expected: "my $x = 1;\n",
            disposition: FormatDisposition::NoChange,
            reason: FormatReasonCode::AlreadyFormatted,
            config: NativeFormatConfig::default(),
        },
        Case {
            name: "parse_refusal",
            source: "my $x = ;\n",
            expected: "my $x = ;\n",
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::SourceParseError,
            config: NativeFormatConfig::default(),
        },
        Case {
            name: "partial_literal_preservation",
            source: "my$x=1;\nmy $text = <<'END';\nvalue  \nEND\n",
            expected: "my $x = 1;\nmy $text = <<'END';\nvalue  \nEND\n",
            disposition: FormatDisposition::Applied,
            reason: FormatReasonCode::AppliedPreservingLiterals,
            config: NativeFormatConfig::default(),
        },
        Case {
            name: "crlf_astral_true_eof",
            source: "my$x=1;\r\n# 😀",
            expected: "my $x = 1;\r\n# 😀",
            disposition: FormatDisposition::Applied,
            reason: FormatReasonCode::Applied,
            config: NativeFormatConfig::default(),
        },
        Case {
            name: "explicit_native_indent",
            source: "sub f {\nmy$x=1;\n}\n",
            expected: "sub f {\n  my $x = 1;\n}\n",
            disposition: FormatDisposition::Applied,
            reason: FormatReasonCode::Applied,
            config: NativeFormatConfig { indent_width: 2, ..NativeFormatConfig::default() },
        },
        Case {
            name: "invalid_native_policy",
            source: "my$x=1;\n",
            expected: "my$x=1;\n",
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::InvalidConfiguration,
            config: NativeFormatConfig { indent_width: 0, ..NativeFormatConfig::default() },
        },
    ];
    for case in cases {
        let context = FormatContext::default();
        let direct =
            NativeFormatter::new().format_document_typed(case.source, &case.config, &context);
        let provider = FormattingProvider::native()
            .with_native_config(case.config.clone())
            .format_document_decision(case.source, &options(), &context)?;
        assert_eq!(direct.result.formatted, case.expected, "{}", case.name);
        assert_eq!(direct.outcome.disposition, case.disposition, "{}", case.name);
        assert_eq!(direct.outcome.reason, case.reason, "{}", case.name);
        assert_shared_result(&ExactSource::new(case.source), &direct, &provider)?;
        assert_eq!(provider.outcome.source.generation, SourceGeneration::Unknown);
        assert_eq!(
            provider.outcome.source.content,
            ContentDigest::of_bytes(case.source.as_bytes())
        );
        if case.name == "partial_literal_preservation" {
            assert_eq!(
                provider.outcome.safety.literal_preservation,
                FormatEvidenceState::PreservedPartially
            );
        }
        if case.name == "crlf_astral_true_eof" {
            let edit = provider.document.edits.first().ok_or("expected astral document edit")?;
            assert_eq!((edit.range.end.line, edit.range.end.character), (1, 4));
            assert_ne!(edit.range.end.character as usize, "# 😀".len());
        }
    }
    Ok(())
}

#[test]
fn direct_native_and_core_provider_share_document_behavior() -> TestResult {
    exercise_cases()
}

#[test]
fn canonical_generation_matches_when_supplied_to_both_boundaries() -> TestResult {
    let text = "my$x=1;\n";
    let context = FormatContext::new(Some("file:///parity/owned.pl".to_string()), Some(37));
    let mut identity = FormatSourceIdentity::content_only(text);
    identity.generation = SourceGeneration::known("37");
    let source = ExactSource::with_identity(text, identity)?;
    let config = NativeFormatConfig::default();
    let direct = NativeFormatter::new().format(
        &source,
        &FormatRequest::document(&source),
        &config,
        &context,
    );
    let provider = FormattingProvider::native()
        .with_native_config(config.clone())
        .format_document_decision(text, &options(), &context)?;
    assert_shared_result(&source, &direct, &provider)?;
    assert_eq!(provider.outcome.source.generation, SourceGeneration::known("37"));
    assert!(provider.outcome.source.logical_source.is_none(), "a URI is not a logical source ID");
    Ok(())
}

#[test]
fn adapter_options_keep_separate_identity_without_overriding_explicit_native_style() -> TestResult {
    let source = "sub f {\nmy$x=1;\n}\n";
    let config = NativeFormatConfig { indent_width: 2, ..NativeFormatConfig::default() };
    let context = FormatContext::default();
    let direct = NativeFormatter::new().format_document_typed(source, &config, &context);
    let provider = FormattingProvider::native().with_native_config(config.clone());
    let baseline = provider.format_document_decision(source, &options(), &context)?;
    let editor_options = FormattingOptions { tab_size: 8, insert_spaces: false, ..options() };
    let changed = provider.format_document_decision(source, &editor_options, &context)?;
    assert_shared_result(&ExactSource::new(source), &direct, &baseline)?;
    assert_shared_result(&ExactSource::new(source), &direct, &changed)?;
    assert_eq!(changed.document.text, "sub f {\n  my $x = 1;\n}\n");
    assert_ne!(
        baseline.outcome.identity.config_fingerprint,
        changed.outcome.identity.config_fingerprint
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn native_construction_and_formatting_do_not_execute_adversarial_path_tools() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("external-probe.log");
    let completed = directory.path().join("native-parity-complete");
    for name in ["perl", "perltidy"] {
        let executable = directory.path().join(name);
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf 'external probe: %s\\n' \"$0\" >> \"$PERL_TIDY_PARITY_PROBE_LOG\"\nexit 73\n",
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        // Prove each executable would leave evidence if discovered and invoked.
        let status = Command::new(name)
            .env("PATH", directory.path())
            .env("PERL_TIDY_PARITY_PROBE_LOG", &marker)
            .status()?;
        assert_eq!(status.code(), Some(73));
    }
    let probes = std::fs::read_to_string(&marker)?;
    assert_eq!(probes.lines().count(), 2);
    std::fs::remove_file(&marker)?;

    // Only the child environment changes; the parallel Rust test process is untouched.
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", "isolated_native_parity_child", "--test-threads=1"])
        .env("PATH", directory.path())
        .env("PERL_TIDY_PARITY_PROBE_LOG", &marker)
        .env("PERL_TIDY_PARITY_CHILD_COMPLETE", &completed)
        .output()?;
    assert!(output.status.success(), "child failed: {output:?}");
    assert_eq!(std::fs::read(&completed)?, b"native parity complete");
    assert!(!marker.exists(), "native construction or formatting executed a PATH probe");
    Ok(())
}

#[cfg(unix)]
#[test]
fn isolated_native_parity_child() -> TestResult {
    let Some(completed) = std::env::var_os("PERL_TIDY_PARITY_CHILD_COMPLETE") else {
        return Ok(());
    };
    exercise_cases()?;
    std::fs::write(completed, b"native parity complete")?;
    Ok(())
}
