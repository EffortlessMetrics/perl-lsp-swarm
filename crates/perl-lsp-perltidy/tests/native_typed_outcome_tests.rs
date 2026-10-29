#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_lsp_perltidy::PerlFormatter;
use perl_lsp_perltidy::native::{
    BracePlacement, ElsePlacement, FormatConfig, FormatContext, FormatDisposition, FormatEngine,
    FormatEvidenceState, FormatReasonCode, FormatRequestTarget, FormatterMode, KeywordSpacing,
    NativeFormatter, TextPosition, TextRange,
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
fn typed_document_preserves_literal_only_document_without_refusing() {
    // #17708: a document whose only construct is a preserved literal is no
    // longer refused wholesale. The literal line stays byte-exact and the
    // outcome reports partial preservation instead of a refusal.
    let formatter = NativeFormatter::new();
    let source = "my $matched = $text =~ /needle/i;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormattedPreservingLiterals);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::PreservedPartially);
    assert_eq!(typed.result.formatted, source);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
}

#[test]
fn typed_document_formats_around_preserved_heredoc_and_pod() {
    let formatter = NativeFormatter::new();
    let source =
        "my$x=1;\nmy $text = <<'EOF';\n  keep   me\nEOF\n=head1 NAME\n\nDemo\n=cut\nmy$y=2;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AppliedPreservingLiterals);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::PreservedPartially);
    assert!(typed.result.formatted.contains("my $x = 1;"), "got: {}", typed.result.formatted);
    assert!(typed.result.formatted.contains("my $y = 2;"), "got: {}", typed.result.formatted);
    assert!(
        typed.result.formatted.contains("  keep   me\n"),
        "heredoc body must stay byte-exact, got: {}",
        typed.result.formatted
    );
    assert!(
        typed.result.formatted.contains("=head1 NAME\n\nDemo\n=cut\n"),
        "POD must stay byte-exact, got: {}",
        typed.result.formatted
    );
    // Formatting the formatted output is a no-op (idempotent).
    let again = formatter.format_document_typed(
        &typed.result.formatted,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(again.result.formatted, typed.result.formatted);
}

#[test]
fn typed_document_preserves_queued_heredoc_bodies() {
    let formatter = NativeFormatter::new();
    let source = "my ($a, $b) = (<<'A', <<'B');\nfirst  body\nA\nsecond   body\nB\nmy$x=1;\n";
    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert!(typed.result.formatted.contains("first  body\n"), "got: {}", typed.result.formatted);
    assert!(typed.result.formatted.contains("second   body\n"), "got: {}", typed.result.formatted);
    assert!(typed.result.formatted.contains("my $x = 1;"), "got: {}", typed.result.formatted);
}

#[test]
fn typed_document_preserves_heredoc_body_after_padded_terminator_line() {
    // A padded delimiter is body text for an ordinary heredoc, not a close.
    let formatter = NativeFormatter::new();
    let source = "my $s = <<'EOF';\n EOF\nmy$x=1;\nEOF\nmy$y=2;\n";
    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert!(typed.result.formatted.contains(" EOF\nmy$x=1;\n"), "got: {}", typed.result.formatted);
    assert!(typed.result.formatted.contains("my $y = 2;"), "got: {}", typed.result.formatted);
}

#[test]
fn typed_document_comment_heredoc_marker_does_not_suppress_formatting() {
    let formatter = NativeFormatter::new();
    let source = "# example: <<'EOF'\nmy$x=1;\n";
    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert!(typed.result.formatted.contains("my $x = 1;"), "got: {}", typed.result.formatted);
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
fn typed_document_result_admits_the_blocks_it_rendered_on_a_second_pass() {
    // #13205 / FPH-008. The block renderers emit headers and tails on their
    // own lines, and neither is re-derivable from one line, so before the
    // already-formatted admission recognized them a block the formatter had
    // just rendered classified as `Refused/UnsupportedSyntax` on the next
    // pass. That is what kept the FPH-008 dormancy fail-closed.
    let formatter = NativeFormatter::new();
    let config = FormatConfig::default();

    let sources = [
        "while($ok){my$x=1;return$x;}\n",
        "while($ok){next;}continue{last;}\n",
        "if($ok){my$x=1;}else{my$y=2;}\n",
        "if($ok){my$x=1;}elsif($z){my$y=2;}else{my$w=3;}\n",
        "unless($ok){my$x=1;}\n",
        "until($ok){my$x=1;}\n",
        "sub foo{my$x=1;return$x;}\n",
    ];

    for source in sources {
        let rendered = formatter.format_document(source, &config);
        assert!(rendered.changed, "source {source:?} was expected to render a block");

        let second = formatter.format_document_typed(
            &rendered.formatted,
            &config,
            &FormatContext::default(),
        );

        assert_eq!(
            second.outcome.disposition,
            FormatDisposition::NoChange,
            "second pass over rendered {:?} must be NoChange",
            rendered.formatted
        );
        assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert!(!second.result.changed);
        assert!(second.result.edits.is_empty(), "second pass must carry no edits");
        assert!(second.result.diagnostics.is_empty());
        assert_eq!(
            second.result.formatted, rendered.formatted,
            "second pass must leave the rendered bytes stable"
        );
        assert_eq!(second.outcome.change.source_bytes_changed, 0);
        assert_eq!(second.outcome.change.rendered_bytes_changed, 0);
    }
}

#[test]
fn typed_document_result_admits_next_line_braces_on_a_second_pass() {
    // The same conversion under `BracePlacement::NextLine`, which is a distinct
    // rendered shape: the header loses its trailing brace and the opening brace
    // becomes its own line. `foreach$e(@list){next;}\r\nforeach my$item(@items){return$item;}\nforeach$e(@list){next;} # tail`
    // is the FPH-008 harness counterexample that caught a brace-terminated-only
    // admission, and it also mixes in CRLF separators, compact keyword spacing,
    // and a trailing tail comment.
    let formatter = NativeFormatter::new();
    let config = FormatConfig {
        brace_placement: BracePlacement::NextLine,
        keyword_spacing: KeywordSpacing::Compact,
        ..FormatConfig::default()
    };
    let source = "foreach$e(@list){next;}\r\nforeach my$item(@items){return$item;}\nforeach$e(@list){next;} # tail";

    let rendered = formatter.format_document(source, &config);
    assert!(rendered.changed, "source was expected to render foreach blocks");
    assert!(
        rendered.formatted.contains("\r\n"),
        "the rendered shape must keep the mixed CRLF separator under test"
    );

    let second =
        formatter.format_document_typed(&rendered.formatted, &config, &FormatContext::default());

    assert_eq!(
        second.outcome.disposition,
        FormatDisposition::NoChange,
        "second pass over NextLine-brace render {:?} must be NoChange",
        rendered.formatted
    );
    assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!second.result.changed);
    assert!(second.result.edits.is_empty());
    assert!(second.result.diagnostics.is_empty());
    assert_eq!(second.result.formatted, rendered.formatted);
}

#[test]
fn typed_document_result_admits_separate_line_clauses_on_a_second_pass() {
    // #16708: under `ElsePlacement::SeparateLine` the else/elsif renderers put
    // the clause header on its own line, and under `BracePlacement::NextLine`
    // that header loses its brace. Neither shape was admitted by the
    // already-formatted check, so the formatter refused its own output on a
    // second pass. Both placements must classify NoChange.
    let formatter = NativeFormatter::new();
    for brace_placement in [BracePlacement::SameLine, BracePlacement::NextLine] {
        let config = FormatConfig {
            else_placement: ElsePlacement::SeparateLine,
            brace_placement,
            ..FormatConfig::default()
        };
        for source in [
            "if($ok){return 1;}else{return 2;}
",
            "if($ok){return 1;}elsif($z){return 2;}else{return 3;}
",
        ] {
            let rendered = formatter.format_document(source, &config);
            assert!(
                rendered.changed,
                "source {source:?} under {brace_placement:?} was expected to render"
            );
            let second = formatter.format_document_typed(
                &rendered.formatted,
                &config,
                &FormatContext::default(),
            );
            assert_eq!(
                second.outcome.disposition,
                FormatDisposition::NoChange,
                "second pass over the SeparateLine render under {brace_placement:?} ({:?}) must be NoChange",
                rendered.formatted
            );
            assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
            assert_eq!(second.result.formatted, rendered.formatted);
        }
    }
}

#[test]
fn typed_document_result_refuses_a_tail_with_an_inline_body() {
    // #16708: a `}`-prefixed tail used to be admitted unconditionally, so an
    // inline unsupported body riding the tail line reached `NoChange` even
    // though no renderer emits that shape. The tail must end at the clause's
    // opening brace.
    let formatter = NativeFormatter::new();
    let source = "if ($ok) {
    my $x = 1;
} else { my %h = (a => 1, b => 2); }
";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_document_result_refuses_a_header_condition_the_renderer_cannot_format() {
    // #16708: header admission checked only the line's endpoints, so a
    // multi-operator condition (which `format_simple_condition_tokens`
    // refuses) endpoint-admitted as `NoChange` despite never passing a
    // renderer. The interior must be validated with the renderer's own
    // condition formatter.
    let formatter = NativeFormatter::new();
    let source = "if ($a && $b && $c) {
    my $x = 1;
}
";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_document_result_still_refuses_a_block_whose_body_is_unsupported() {
    // Negative control for the admission above: recognizing block boundaries
    // must not admit the block *body*. This subject is already in rendered
    // form and its body line parses cleanly with no diagnostic, so only the
    // already-formatted admission can refuse it.
    let formatter = NativeFormatter::new();
    let source = "while ($ok) {\n    my %h = (a => 1, b => 2);\n}\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
    assert!(typed.result.diagnostics.is_empty(), "refusal is by admission, not by diagnostic");
}

#[test]
fn typed_document_result_formats_a_hand_written_block() {
    // #17660 contract change (supersedes the #16708 refusal pin): a
    // hand-written block under admitted boundaries is now reindented to
    // the renderer's layout and classifies Applied, instead of refusing
    // as UnsupportedSyntax. Boundary shape still cannot claim *rendered*
    // form by itself â€” the formatted bytes are produced by the pipeline
    // and parse-gated, and a second pass must reach NoChange.
    let formatter = NativeFormatter::new();
    for source in [
        "sub f {\nreturn 1;\n}\n",
        "while ($ok) {\nreturn 1;\n}\n",
        "if ($ok)\n{\nreturn 1;\n}\n",
        "sub f {\n    return 1;\n  }\n",
    ] {
        let typed = formatter.format_document_typed(
            source,
            &FormatConfig::default(),
            &FormatContext::default(),
        );

        assert_eq!(
            typed.outcome.disposition,
            FormatDisposition::Applied,
            "hand-written {source:?} must now be formatted"
        );
        assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
        assert!(typed.result.changed);

        let second = formatter.format_document_typed(
            &typed.result.formatted,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(second.outcome.disposition, FormatDisposition::NoChange);
        assert!(!second.result.changed);
    }
}

#[test]
fn typed_document_result_admits_renderer_layout_bytes_without_a_rendering_pass() {
    // The document-level evidence admits exactly the layout the renderer
    // emits Ã¢â‚¬â€ checked directly against those bytes, not only through a first
    // rendering pass: same block as the refused hand-written subject above,
    // with the body at the renderer's own indent.
    let formatter = NativeFormatter::new();
    let source = "sub f {\n    return 1;\n}\n";

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

/// A bare-CR document whose requested range overlaps a heredoc marker must be
/// refused with `LiteralPreservationUnsupported`, not `UnsafeRange`.
///
/// Previously `valid_range` split only on `\n`, so for a bare-CR source the
/// whole document appeared as a single line. Any range referencing line 1+
/// failed the range-validity check and returned `UnsafeRange` before the
/// CR-aware `literal_preserve_region_for_range` guard in `format_range` could
/// fire. Now `valid_range` uses CR-aware line geometry (`\r\n`, `\r`, or `\n`),
/// so the typed path reaches the same literal-preservation refusal as the
/// compatibility `format_range` path.
///
/// Pins issue #13338.
#[test]
fn format_range_typed_refuses_bare_cr_heredoc_as_literal_preservation_not_unsafe_range() {
    let formatter = NativeFormatter::new();
    // Bare-CR document (line endings are bare \r):
    //   line 0: "print <<EOT;"   Ã¢â‚¬â€ heredoc marker
    //   line 1: "EOT"            Ã¢â‚¬â€ terminator
    //   line 2: ""               Ã¢â‚¬â€ trailing empty (after the final \r)
    let source = "print <<EOT;\rEOT\r";
    // Range covering the heredoc marker (line 0) through the terminator (line 1).
    // Before the fix this produced UnsafeRange because valid_range treated the
    // entire bare-CR document as one \n-less line and rejected line 1 as out of
    // bounds.
    let range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(1, 3));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(
        typed.outcome.reason,
        FormatReasonCode::LiteralPreservationUnsupported,
        "bare-CR heredoc range must be LiteralPreservationUnsupported, not UnsafeRange; got: {:?}",
        typed.outcome.reason,
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Refused);
    assert_eq!(typed.result.formatted, source);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
}

/// Bare-CR documents whose range refers to a line past the first one must still
/// pass the range-validity check when the range itself is syntactically
/// well-formed (i.e. the line exists in the CR-aware view of the document).
///
/// This is a companion to the heredoc test: it verifies the same `valid_range`
/// fix for a clean bare-CR source (no heredoc), confirming that the range
/// correctly proceeds to `format_range` and is not refused as `UnsafeRange`.
#[test]
fn format_range_typed_accepts_valid_range_on_bare_cr_document_without_heredoc() {
    let formatter = NativeFormatter::new();
    // Bare-CR document (no heredoc, no regex, no opaque construct):
    //   line 0: "my$x=1;"
    //   line 1: "my$y=2;"
    //   line 2: ""
    let source = "my$x=1;\rmy$y=2;\r";
    // Range covering only line 1 Ã¢â‚¬â€ valid in the CR-aware view.
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    // The range is valid and the source is clean, so the formatter must reach
    // a formatting decision (Applied or AlreadyFormatted) Ã¢â‚¬â€ not UnsafeRange.
    assert_ne!(
        typed.outcome.reason,
        FormatReasonCode::UnsafeRange,
        "a valid range on a bare-CR source must not be refused as UnsafeRange; got: {:?}",
        typed.outcome.reason,
    );
}

#[test]
fn reindents_misindented_sub_and_conditional_blocks() {
    let formatter = NativeFormatter::new();
    let source = "sub foo {\nmy $x = 1;\nif ($x) {\nmy $y = 2;\n}\n}\n";
    let expected = "sub foo {\n    my $x = 1;\n    if ($x) {\n        my $y = 2;\n    }\n}\n";

    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, expected);

    let second = formatter.format_document(&result.formatted, &FormatConfig::default());
    assert_eq!(second.formatted, expected, "reindent must be idempotent");
    assert!(!second.changed);
}

#[test]
fn reindent_formats_around_literal_preserve_regions() {
    // #17708: documents containing heredocs are formatted around the
    // preserved body instead of being refused wholesale. The heredoc body
    // stays byte-exact; the surrounding code reindents normally.
    let formatter = NativeFormatter::new();
    let source = "sub foo {\nmy $x = <<'EOF';\n  keep   me\nEOF\nmy $y = 1;\n}\n";
    let expected = "sub foo {\n    my $x = <<'EOF';\n  keep   me\nEOF\n    my $y = 1;\n}\n";

    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, expected);

    let second = formatter.format_document(&result.formatted, &FormatConfig::default());
    assert_eq!(second.formatted, expected, "preserve formatting must be idempotent");
    assert!(!second.changed);
}

#[test]
fn reindent_keeps_anonymous_sub_bodies_nested() {
    let formatter = NativeFormatter::new();
    // Already-correct nesting through a non-boundary opener must be a
    // no-op (regression: bodies were flattened to the outer level).
    let correct = "sub outer {\n    my $f = sub {\n        my $x = 1;\n    };\n    my $y = 2;\n}\n";
    let result = formatter.format_document(correct, &FormatConfig::default());
    assert_eq!(result.formatted, correct);
    assert!(!result.changed);

    let misindented = "sub outer {\nmy $f = sub {\nmy $x = 1;\n};\nmy $y = 2;\n}\n";
    let fixed = formatter.format_document(misindented, &FormatConfig::default());
    assert_eq!(fixed.formatted, correct);
    let second = formatter.format_document(&fixed.formatted, &FormatConfig::default());
    assert!(!second.changed);
}

#[test]
fn reindent_preserves_multiline_string_contents() {
    let formatter = NativeFormatter::new();
    // The indented `}` is string content, not a block closer.
    let source = "sub f {\nmy $s = \"hello\n    }\nworld\";\nreturn 1;\n}\n";
    let expected = "sub f {\n    my $s = \"hello\n    }\nworld\";\n    return 1;\n}\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, expected);
    let second = formatter.format_document(&result.formatted, &FormatConfig::default());
    assert!(!second.changed);
}

#[test]
fn reindent_survives_unsupported_inline_else_tail() {
    let formatter = NativeFormatter::new();
    let source = "sub f {\nif ($x) {\nreturn 1;\n} else { print 2; }\nreturn 3;\n}\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    // The unsupported tail line is preserved; following lines must return
    // to sub level, not stay at if level.
    assert!(result.formatted.ends_with("return 3;\n}\n"), "got: {:?}", result.formatted);
    assert!(result.formatted.contains("\n    return 3;\n"), "got: {:?}", result.formatted);
}
#[test]
fn typed_range_result_admits_rendered_block_interior_on_a_second_pass() {
    // #17713: range admission used to judge only the selected lines, so a
    // no-change range inside an already-formatted block still reported
    // UnsupportedSyntax whenever the selection cut a block boundary (body
    // plus tail, header plus body, a lone tail). The classification walk now
    // accounts for the enclosing block context, so every interior selection
    // of rendered bytes is NoChange — twice in a row.
    let formatter = NativeFormatter::new();
    let config = FormatConfig::default();
    let rendered = formatter.format_document("sub f{my$x=1;my$y=2;}\n", &config);
    assert_eq!(rendered.formatted, "sub f {\n    my $x = 1;\n    my $y = 2;\n}\n");

    let selections = [
        // body only
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(3, 0)),
        // body plus closing tail (opener outside the range)
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(4, 0)),
        // header plus body (tail outside the range)
        TextRange::new(TextPosition::new(0, 0), TextPosition::new(3, 0)),
        // lone closing tail
        TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0)),
        // whole block
        TextRange::new(TextPosition::new(0, 0), TextPosition::new(4, 0)),
    ];
    for range in selections {
        for _ in 0..2 {
            let typed = formatter.format_range_typed(
                &rendered.formatted,
                range,
                &config,
                &FormatContext::default(),
            );
            assert_eq!(
                typed.outcome.disposition,
                FormatDisposition::NoChange,
                "range {range:?} over rendered block must be NoChange, not {:?}/{:?}",
                typed.outcome.disposition,
                typed.outcome.reason
            );
            assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
            assert!(!typed.result.changed);
            assert!(typed.result.edits.is_empty());
            assert_eq!(typed.result.formatted, rendered.formatted);
        }
    }
}

#[test]
fn typed_range_result_refuses_unsupported_body_with_same_reason_as_document() {
    // A range over a construct the document path refuses must refuse with
    // the same typed reason — no silent divergence between targets.
    let formatter = NativeFormatter::new();
    let source = "sub f {\n    my %h = (a => 1, b => 2);\n}\n";

    let document = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(document.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(document.outcome.reason, FormatReasonCode::UnsupportedSyntax);

    for range in [
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
        TextRange::new(TextPosition::new(0, 0), TextPosition::new(3, 0)),
    ] {
        let typed = formatter.format_range_typed(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
        assert_eq!(typed.outcome.reason, document.outcome.reason);
        assert!(!typed.result.changed);
        assert!(typed.result.edits.is_empty());
    }
}

#[test]
fn typed_range_result_refuses_misindented_body_inside_block() {
    // Enclosing context cuts both ways: a body line at the wrong depth is
    // not already formatted just because the line alone is supported.
    let formatter = NativeFormatter::new();
    let source = "sub f {\nmy $x = 1;\n}\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_range_result_formats_admitted_range_inside_block() {
    let formatter = NativeFormatter::new();
    let source = "sub f {\n    my$x=1;\n    my $y = 2;\n}\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(typed.result.formatted, "sub f {\n    my $x = 1;\n    my $y = 2;\n}\n");
    assert!(typed.result.edits.iter().all(|edit| edit.range.start.line == 1));
}

#[test]
fn typed_range_result_partial_preserve_overlap_refuses_like_document_gate() {
    // A range that overlaps a preserve region refuses with the typed
    // literal-preservation reason; a range that excludes it is admitted.
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\nmy $matched = $text =~ /needle/i;\nmy $y = 2;\n";

    for range in [
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
        TextRange::new(TextPosition::new(0, 0), TextPosition::new(3, 0)),
    ] {
        let typed = formatter.format_range_typed(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
        assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported);
        assert_eq!(typed.result.formatted, source);
        assert!(!typed.result.changed);
    }

    for range in [
        TextRange::new(TextPosition::new(0, 0), TextPosition::new(1, 0)),
        TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0)),
    ] {
        let typed = formatter.format_range_typed(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    }
}

#[test]
fn typed_range_result_ignores_inline_clause_tail_before_the_range() {
    // Review PRRT_kwDOSid81M6rq9dA: an unselected inline `} else { ... }`
    // tail is not a rendered boundary, but it still closes the `if` block.
    // The admission walk must pop that block for context lines, or a later
    // correctly indented selection is compared against the stale opener and
    // refused as UnsupportedSyntax.
    let formatter = NativeFormatter::new();
    let source =
        "sub f {\n    if ($x) {\n        print 2;\n    } else { print 3; }\n    my $y = 2;\n}\n";
    // Select only `my $y = 2;` (line 4).
    let range = TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
    assert_eq!(typed.result.formatted, source);
}

#[test]
fn typed_range_result_still_refuses_misindented_line_after_inline_clause_tail() {
    // Negative control for the context-pop fix above: honouring the inline
    // tail's close must not admit a selection outside the enclosing layout.
    let formatter = NativeFormatter::new();
    let source =
        "sub f {\n    if ($x) {\n        print 2;\n    } else { print 3; }\nmy $y = 2;\n}\n";
    let range = TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_range_result_ignores_multiline_string_content_before_the_range() {
    // Review PRRT_kwDOSid81M6rq9dr: physical lines inside a multiline
    // string are literal bytes, not code. A string-content line shaped like
    // a block header must not be pushed as an opener for a later range.
    let formatter = NativeFormatter::new();
    let source = "sub f {\n    my $s = \"hello\n    if ($x) {\nworld\";\n    my $y = 2;\n}\n";
    // Select only `my $y = 2;` (line 4).
    let range = TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
    assert_eq!(typed.result.formatted, source);
}

#[test]
fn formats_list_operator_statements_inside_blocks() {
    let formatter = NativeFormatter::new();
    let source = "sub f {\nprint$x;\nprintf \"%d\",  42;\nsay $x, $y;\n}\n";
    let expected = "sub f {\n    print $x;\n    printf \"%d\", 42;\n    say $x, $y;\n}\n";

    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, expected);

    let second = formatter.format_document(&result.formatted, &FormatConfig::default());
    assert_eq!(second.formatted, expected);
    assert!(!second.changed);
}

#[test]
fn range_format_reindents_relative_to_enclosing_block() {
    let formatter = NativeFormatter::new();
    let source = "sub f {\nmy $x = 1;\nmy $y = 2;\n}\n";
    // Range covering only the two body lines (lines 1..2).
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 10));
    let result = formatter.format_range(source, range, &FormatConfig::default());
    assert_eq!(result.formatted, "sub f {\n    my $x = 1;\n    my $y = 2;\n}\n");
    assert_eq!(result.edits.len(), 2);
}

#[test]
fn comment_braces_do_not_open_blocks() {
    let formatter = NativeFormatter::new();
    let source = "# note {\nmy$x=1;\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, "# note {\nmy $x = 1;\n");

    let ranged = formatter.format_range(
        source,
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 7)),
        &FormatConfig::default(),
    );
    assert_eq!(ranged.formatted, "# note {\nmy $x = 1;\n");
}
