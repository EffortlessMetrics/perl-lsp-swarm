#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::{
    FormatConfig, FormatResult, NativeFormatter, PerlFormatter, TextPosition, TextRange,
};

const PRESERVE_CODE: &str = "native.format.literal_preserve_region";

fn line_range(line: u32) -> TextRange {
    TextRange::new(TextPosition::new(line, 0), TextPosition::new(line + 1, 0))
}

fn assert_heredoc_refusal(result: &FormatResult, source: &str) {
    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(
        result.diagnostics.first().is_some_and(|diagnostic| {
            diagnostic.code == PRESERVE_CODE && diagnostic.message.contains("heredoc")
        }),
        "expected a heredoc preservation diagnostic; got {:?}",
        result.diagnostics,
    );
}

#[test]
fn native_range_formatter_refuses_real_heredoc_opener() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";

    let result = formatter.format_range(source, line_range(0), &FormatConfig::default());

    assert_heredoc_refusal(&result, source);
}

#[test]
fn native_range_formatter_refuses_real_heredoc_body_with_utf8_prefix() {
    let formatter = NativeFormatter::new();
    let source = "my $face = \"😀\";\nprint <<'EOF';\nmy$x=1;\nEOF\n";

    let result = formatter.format_range(source, line_range(2), &FormatConfig::default());

    assert_heredoc_refusal(&result, source);
}

#[test]
fn marker_like_text_in_strings_and_comments_remains_format_eligible() {
    let formatter = NativeFormatter::new();
    let cases = [
        ("my$x=\"<<LABEL\";\n", "my $x = \"<<LABEL\";\n"),
        ("my$x=1; # <<LABEL\n", "my $x = 1; # <<LABEL\n"),
    ];

    for (source, expected) in cases {
        let result = formatter.format_document(source, &FormatConfig::default());

        assert!(result.changed, "source should remain format-eligible: {source:?}");
        assert_eq!(result.formatted, expected);
        assert!(result.diagnostics.is_empty());
    }
}

#[test]
fn code_after_heredoc_terminator_remains_range_format_eligible() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";

    let result = formatter.format_range(source, line_range(3), &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.formatted, "print <<'EOF';\nraw { text }\nEOF\nmy $x = 1;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn whole_document_with_real_heredoc_remains_conservative() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw { text }\nEOF\nmy$x=1;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_heredoc_refusal(&result, source);
}
