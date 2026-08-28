#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{FormatContext, FormatDisposition, FormatLineEndingDisposition};
use perl_lsp_perltidy::{FormatConfig, NativeFormatter, PerlFormatter, TextPosition, TextRange};

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
fn generated_layout_keeps_lf_sources_lf_only() {
    let formatter = NativeFormatter::new();
    let source = "while($n){next;}\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.formatted, "while ($n) {\n    next;\n}\n");
    assert!(!result.formatted.contains('\r'));
}
