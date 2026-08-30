#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{
    FormatContext, FormatDisposition, FormatReasonCode, FormatRequestTarget,
};
use perl_lsp_perltidy::{FormatConfig, NativeFormatter, PerlFormatter, TextPosition, TextRange};
use perl_parser_core::SourceRegionIndex;

#[test]
fn document_formatting_ignores_marker_text_inside_string_and_comment() {
    let formatter = NativeFormatter::new();
    let source = "my$x=\"<<EOF\"; # docs <<NOTE\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.formatted, "my $x = \"<<EOF\"; # docs <<NOTE\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn typed_document_formatting_ignores_marker_text_inside_string_and_comment() {
    let formatter = NativeFormatter::new();
    let source = "my$x=\"<<EOF\"; # docs <<NOTE\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(typed.result.formatted, "my $x = \"<<EOF\"; # docs <<NOTE\n");
    assert!(typed.result.diagnostics.is_empty());
}

#[test]
fn range_formatting_refuses_completed_heredoc_body() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nmy$x=1;\nEOF\nmy$y=2;\n";
    let body = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, body, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(result.diagnostics.first().is_some_and(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn typed_range_formatting_reports_heredoc_literal_preservation() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nmy$x=1;\nEOF\nmy$y=2;\n";
    let body = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        body,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported);
    assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range: body });
    assert!(!typed.result.changed);
    assert_eq!(typed.result.formatted, source);
}

#[test]
fn compat_and_typed_ranges_gate_the_same_requests() {
    let formatter = NativeFormatter::new();
    let cases = [
        (
            "print <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n",
            [
                TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
                TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0)),
                TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0)),
            ],
        ),
        (
            "print <<'EOF';\rraw { text }\rEOF\rmy$x=1;\r",
            [
                TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
                TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0)),
                TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0)),
            ],
        ),
    ];

    for (source, ranges) in cases {
        for range in ranges {
            let compat = formatter.format_range(source, range, &FormatConfig::default());
            let typed = formatter.format_range_typed(
                source,
                range,
                &FormatConfig::default(),
                &FormatContext::default(),
            );
            let compat_gated = compat
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "native.format.literal_preserve_region");
            let typed_gated =
                typed.outcome.reason == FormatReasonCode::LiteralPreservationUnsupported;

            assert_eq!(compat_gated, typed_gated, "range {range:?} in {source:?}");
        }
    }
}

#[test]
fn range_formatting_refuses_heredoc_terminator_line() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nbody\nEOF\nmy$y=2;\n";
    let terminator = TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0));

    let result = formatter.format_range(source, terminator, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.first().is_some_and(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn range_formatting_refuses_empty_heredoc_terminator_line() {
    let formatter = NativeFormatter::new();
    let source = "print <<A;\nA\nmy$x=1;\n";
    let terminator = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, terminator, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(result.diagnostics.first().is_some_and(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn range_formatting_refuses_crlf_heredoc_terminator_line() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\r\nraw { text }\r\nEOF\r\nmy$x=1;\r\n";
    let terminator = TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0));

    let result = formatter.format_range(source, terminator, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn range_formatting_refuses_final_heredoc_terminator_without_newline() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw { text }\nEOF";
    let terminator = TextRange::new(TextPosition::new(2, 0), TextPosition::new(2, 3));

    let result = formatter.format_range(source, terminator, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn range_formatting_refuses_empty_final_heredoc_terminator() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nEOF";
    let terminator = TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 3));

    let result = formatter.format_range(source, terminator, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn range_formatting_uses_utf16_columns_without_losing_heredoc_boundary() {
    let formatter = NativeFormatter::new();
    let source = "my $face = \"😀\";\nprint <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";
    let body = TextRange::new(TextPosition::new(2, 1), TextPosition::new(3, 1));

    let result = formatter.format_range(source, body, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));
}

#[test]
fn empty_heredocs_keep_each_terminator_protected_and_following_code_eligible() {
    let formatter = NativeFormatter::new();
    let source = "print <<A, <<B;\nA\nB\nmy$x=2;\n";

    for line in [1, 2] {
        let terminator = TextRange::new(TextPosition::new(line, 0), TextPosition::new(line + 1, 0));
        let result = formatter.format_range(source, terminator, &FormatConfig::default());

        assert!(!result.changed, "empty heredoc terminator must be preserved");
        assert_eq!(result.formatted, source);
        assert!(result.edits.is_empty());
        assert!(result.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == "native.format.literal_preserve_region"
                && diagnostic.message.contains("heredoc")
        }));
    }

    let following_code = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));
    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(result.changed, "code after the final empty terminator remains eligible");
    assert_eq!(result.formatted, "print <<A, <<B;\nA\nB\nmy $x = 2;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn range_formatting_after_heredoc_terminator_remains_eligible() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nmy$x=1;\nEOF\nmy$y=2;\n";
    let following_code = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));

    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.formatted, "print <<'EOF';\nmy$x=1;\nEOF\nmy $y = 2;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn range_formatting_does_not_treat_identifier_shift_as_heredoc() {
    let formatter = NativeFormatter::new();
    let source = "my $x = $a << EOF;\nmy$y=1;\n";
    let following_code = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "native.format.parse_incomplete" })
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "native.format.literal_preserve_region" })
    );
}

#[test]
fn incomplete_near_miss_queued_heredocs_are_not_completed_spans() {
    let formatter = NativeFormatter::new();
    let source = "print <<A, <<B;\nbody\nA trailing\n";
    let near_miss = TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0));

    let result = formatter.format_range(source, near_miss, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "native.format.literal_preserve_region" })
    );
}

#[test]
fn malformed_heredoc_opener_does_not_create_completed_preserve_span() {
    let formatter = NativeFormatter::new();
    let source = "print <<;\nmy$x=1;\n";
    let following_code = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "native.format.parse_error" })
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "native.format.literal_preserve_region" })
    );
}

#[test]
fn quote_like_and_comment_markers_do_not_hide_following_code() {
    let formatter = NativeFormatter::new();
    let source = "print <<A, q{<<B}; # comment <<C\nbody\nA\nmy$x=1;\n";
    let following_code = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));

    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(result.changed, "code after the real terminator remains eligible");
    assert_eq!(result.formatted, "print <<A, q{<<B}; # comment <<C\nbody\nA\nmy $x = 1;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn multiline_literal_markers_do_not_hide_following_code() {
    let formatter = NativeFormatter::new();

    for source in ["my $text = \"\n<<FAKE\n\";\nmy$x=1;\n", "my $text = q{\n<<FAKE\n};\nmy$x=1;\n"]
    {
        let following_code = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));
        let result = formatter.format_range(source, following_code, &FormatConfig::default());

        assert!(result.changed, "following code must remain format-eligible: {source:?}");
        assert_eq!(result.formatted, source.replace("my$x=1;", "my $x = 1;"));
        assert!(result.diagnostics.is_empty());
    }
}

#[test]
fn multiple_heredocs_preserve_each_body_and_terminator() {
    let formatter = NativeFormatter::new();
    let source = "print <<A, <<B;\nfirst\nA\nsecond\nB\nmy$x=2;\n";

    for range in [
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
        TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0)),
        TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0)),
        TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0)),
    ] {
        let result = formatter.format_range(source, range, &FormatConfig::default());

        assert!(!result.changed, "completed second heredoc span must be preserved");
        assert_eq!(result.formatted, source);
        assert!(result.edits.is_empty());
        assert!(result.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == "native.format.literal_preserve_region"
                && diagnostic.message.contains("heredoc")
        }));
    }

    let following_code = TextRange::new(TextPosition::new(5, 0), TextPosition::new(6, 0));
    let result = formatter.format_range(source, following_code, &FormatConfig::default());

    assert!(result.changed, "code after the second terminator remains eligible");
    assert_eq!(result.formatted, "print <<A, <<B;\nfirst\nA\nsecond\nB\nmy $x = 2;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn lexer_spans_are_exact_for_queued_empty_and_partial_heredocs()
-> Result<(), Box<dyn std::error::Error>> {
    let queued = "print <<A, <<B;\nfirst\nA\nB\n";
    let queued_regions = SourceRegionIndex::build(queued).completed_heredoc_spans();
    let first_start = queued.find("first").ok_or("missing first body")?;
    let first_end = queued.find("A\n").ok_or("missing first terminator")?;
    let empty_start = queued.rfind("B\n").ok_or("missing empty terminator")?;
    assert_eq!(
        queued_regions.iter().map(|region| (region.start, region.end)).collect::<Vec<_>>(),
        vec![(first_start, first_end), (empty_start, empty_start)]
    );

    let partial = "print <<A, <<B;\nfirst\nA\npartial";
    let partial_regions = SourceRegionIndex::build(partial).completed_heredoc_spans();
    assert_eq!(partial_regions.len(), 1);
    assert_eq!(
        partial_regions.first().map(|region| (region.start, region.end)),
        Some((
            partial.find("first").ok_or("missing partial body")?,
            partial.find("A\n").ok_or("missing partial terminator")?,
        ))
    );
    Ok(())
}

#[test]
fn range_formatting_refuses_bare_cr_heredoc_body_and_terminator() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\rraw { text }\rEOF\rmy$x=1;\r";

    for range in [
        TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0)),
        TextRange::new(TextPosition::new(2, 0), TextPosition::new(3, 0)),
    ] {
        let result = formatter.format_range(source, range, &FormatConfig::default());
        assert!(!result.changed);
        assert!(result.edits.is_empty());
        assert!(result.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == "native.format.literal_preserve_region"
        }));
    }
}

#[test]
fn utf8_nonzero_columns_and_adjacent_code_are_independent_controls() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1; # é\nprint <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";
    let body = TextRange::new(TextPosition::new(2, 1), TextPosition::new(3, 1));
    let refused = formatter.format_range(source, body, &FormatConfig::default());
    assert!(!refused.changed);
    assert!(
        refused.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == "native.format.literal_preserve_region"
        })
    );

    let before = TextRange::new(TextPosition::new(0, 0), TextPosition::new(1, 0));
    let before_result = formatter.format_range(source, before, &FormatConfig::default());
    assert!(before_result.changed);
    assert!(before_result.diagnostics.is_empty());

    let after = TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0));
    let after_result = formatter.format_range(source, after, &FormatConfig::default());
    assert!(after_result.changed);
    assert!(after_result.diagnostics.is_empty());
}

#[test]
fn utf16_nonzero_columns_refuse_heredoc_without_hiding_adjacent_code() {
    let formatter = NativeFormatter::new();
    let source = "my $😀 = 1;\nprint <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";
    let body = TextRange::new(TextPosition::new(2, 1), TextPosition::new(3, 1));
    let refused = formatter.format_range(source, body, &FormatConfig::default());
    assert!(!refused.changed);
    assert!(
        refused.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == "native.format.literal_preserve_region"
        })
    );

    let after = TextRange::new(TextPosition::new(4, 0), TextPosition::new(5, 0));
    let after_result = formatter.format_range(source, after, &FormatConfig::default());
    assert!(after_result.changed);
    assert!(after_result.diagnostics.is_empty());
}

#[test]
fn trailing_trivia_after_heredoc_does_not_extend_preserve_span() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nbody\nEOF\n\n# trailing note\nmy$x=1;\n";
    let following = TextRange::new(TextPosition::new(3, 0), TextPosition::new(6, 0));

    let result = formatter.format_range(source, following, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.formatted, "print <<'EOF';\nbody\nEOF\n\n# trailing note\nmy $x = 1;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn unclosed_heredoc_body_remains_owned_by_parse_gate() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nmy$x=1;\n";
    let body = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, body, &FormatConfig::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "native.format.literal_preserve_region" })
    );
}
