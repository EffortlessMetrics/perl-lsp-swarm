#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{FormatContext, FormatDisposition, FormatLineEndingDisposition};
use perl_lsp_perltidy::{
    FinalNewline, FormatConfig, NativeFormatter, PerlFormatter, TextPosition, TextRange,
};

fn assert_crlf_only(text: &str) {
    assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
    assert!(!text.contains("\r\r\n"));
}

#[test]
fn document_block_layout_preserves_crlf_for_generated_lines() {
    let formatter = NativeFormatter::new();
    let source = "while($n){next;}\r\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.formatted, "while ($n) {\r\n    next;\r\n}\r\n");
    assert_crlf_only(&result.formatted);
}

#[test]
fn wrapped_expression_layout_preserves_crlf_for_generated_lines() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { line_width: 18, indent_width: 2, ..FormatConfig::default() };
    let source = "my$result=foo($alpha,$beta,$gamma);\r\n";

    let result = formatter.format_document(source, &config);

    assert!(result.changed);
    assert_eq!(
        result.formatted,
        concat!("my $result = foo(\r\n", "  $alpha,\r\n", "  $beta,\r\n", "  $gamma\r\n", ");\r\n",)
    );
    assert_crlf_only(&result.formatted);
}

#[test]
fn range_block_layout_preserves_crlf_in_result_and_edit() {
    let formatter = NativeFormatter::new();
    let source = concat!("my $before=1;\r\n", "while($n){next;}\r\n", "my $after=2;\r\n",);
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 16));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].new_text, "while ($n) {\r\n    next;\r\n}");
    assert_eq!(
        result.formatted,
        concat!(
            "my $before=1;\r\n",
            "while ($n) {\r\n",
            "    next;\r\n",
            "}\r\n",
            "my $after=2;\r\n",
        )
    );
    assert_crlf_only(&result.formatted);
}

#[test]
fn typed_outcome_reports_crlf_preserved_after_generated_layout() {
    let formatter = NativeFormatter::new();
    let source = "while($n){next;}\r\n";

    let result = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(result.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(result.outcome.safety.line_endings, FormatLineEndingDisposition::Preserved);
    assert_crlf_only(&result.result.formatted);
}

#[test]
fn unterminated_last_line_inherits_crlf_document_convention() {
    let formatter = NativeFormatter::new();
    let source = "my $before = 1;\r\nwhile($n){next;}";

    let result = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(result.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(
        result.result.formatted,
        concat!("my $before = 1;\r\n", "while ($n) {\r\n", "    next;\r\n", "}",)
    );
    assert_eq!(result.outcome.safety.line_endings, FormatLineEndingDisposition::Preserved);
    assert_crlf_only(&result.result.formatted);
    assert!(!result.result.formatted.ends_with('\n'));
}

#[test]
fn range_on_unterminated_last_line_inherits_crlf_for_result_and_edit() {
    let formatter = NativeFormatter::new();
    let source = "my $before = 1;\r\nwhile($n){next;}";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 16));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(result.changed);
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].new_text, "while ($n) {\r\n    next;\r\n}");
    assert_eq!(
        result.formatted,
        concat!("my $before = 1;\r\n", "while ($n) {\r\n", "    next;\r\n", "}",)
    );
    assert_crlf_only(&result.formatted);
    assert!(!result.formatted.ends_with('\n'));
}

#[test]
fn generated_layout_keeps_lf_sources_lf_only() {
    let formatter = NativeFormatter::new();
    let source = "while($n){next;}\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.formatted, "while ($n) {\n    next;\n}\n");
    assert!(!result.formatted.contains('\r'));
}

#[test]
fn insert_final_newline_uses_crlf_after_generated_layout() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig {
        final_newline: perl_lsp_perltidy::FinalNewline::Insert,
        ..FormatConfig::default()
    };
    let source = "while($n){next;}\r\n";

    let result = formatter.format_document(source, &config);

    assert_eq!(result.formatted, "while ($n) {\r\n    next;\r\n}\r\n");
    assert_crlf_only(&result.formatted);
}

#[test]
fn insert_final_newline_handles_empty_document() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    let result = formatter.format_document("", &config);

    assert!(result.changed);
    assert_eq!(result.formatted, "\n");
}

#[test]
fn insert_final_newline_is_idempotent_for_crlf_layout() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };
    let source = "while($n){next;}\r\n";

    let first = formatter.format_document(source, &config);
    let second = formatter.format_document(&first.formatted, &config);

    assert_eq!(first.formatted, "while ($n) {\r\n    next;\r\n}\r\n");
    assert_eq!(second.formatted, first.formatted);
    assert!(!second.changed);
}

#[test]
fn insert_final_newline_refuses_malformed_document_without_edit() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };
    let source = "my $x = ;\r\n";

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "native.format.parse_error");
}
