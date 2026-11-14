#![deny(clippy::map_err_ignore)]
use perl_lsp_perltidy::{FinalNewline, FormatConfig, NativeFormatter, PerlFormatter};

#[test]
fn array_last_index_sigil_preserves_control_block_indent() {
    let formatter = NativeFormatter::new();
    let source = "if ($#ARGV >= 0) {\nmy $x = 1;\n}\n";
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(result.changed);
    assert_eq!(
        result.formatted, "if ($#ARGV >= 0) {\n    my $x = 1;\n}\n",
        "must treat $# as code so the opening brace pushes control-block indent"
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn array_last_index_sigil_is_not_split_for_comment_alignment() {
    let formatter = NativeFormatter::new();
    let source = "if ($#a >= 0) {\nmy $y = 2; # real\n}\n";
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(result.changed);
    assert!(
        !result.formatted.contains("$   #"),
        "must not insert spaces before # inside $# sigil: {}",
        result.formatted
    );
    assert!(result.formatted.contains("$#a"));
    assert!(result.formatted.contains("# real"));
    assert!(result.diagnostics.is_empty());
}

#[test]
fn trailing_comment_heredoc_marker_does_not_preserve_following_blocks() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1; # see <<EOF\nif (1) {\nmy $inner = 2;\n}\n";
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(result.changed);
    assert_eq!(result.formatted, "my $x = 1; # see <<EOF\nif (1) {\n    my $inner = 2;\n}\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn quoted_and_indented_heredoc_openers_still_preserve_bodies() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    for source in [
        "print <<EOF;\n  body\nEOF\nmy $x = 1;\n",
        "print <<~EOF;\n  body\nEOF\nmy $x = 1;\n",
        "print <<\"EOF\";\n  body\nEOF\nmy $x = 1;\n",
    ] {
        let result = formatter.format_document(source, &config);
        assert!(result.formatted.contains("  body\n"), "heredoc body preserved for {source:?}");
        assert!(result.formatted.contains("my $x = 1;\n"));
        assert!(result.diagnostics.is_empty(), "source: {source:?}");
    }
}

#[test]
fn formatting_array_last_index_sources_is_idempotent() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };
    let source = "if ($#ARGV >= 0) {\n    my $x = 1;\n}\n";

    let first = formatter.format_document(source, &config);
    let second = formatter.format_document(&first.formatted, &config);
    assert!(!second.changed, "second pass changed output for {source:?}");
    assert_eq!(second.formatted, first.formatted);
}
