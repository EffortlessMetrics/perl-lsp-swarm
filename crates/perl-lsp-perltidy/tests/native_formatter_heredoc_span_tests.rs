#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{
    FormatContext, FormatDisposition, FormatReasonCode, FormatRequestTarget,
};
use perl_lsp_perltidy::{FormatConfig, NativeFormatter, PerlFormatter, TextPosition, TextRange};

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
fn multiple_heredocs_preserve_the_second_body_and_terminator_only() {
    let formatter = NativeFormatter::new();
    let source = "print <<A, <<B;\nfirst\nA\nsecond\nB\nmy$x=2;\n";

    for range in [
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
