#![deny(clippy::map_err_ignore)]
use perl_lsp_perltidy::{FormatConfig, NativeFormatter, PerlFormatter};

#[test]
fn array_last_index_keeps_control_block_indent() {
    let formatter = NativeFormatter::new();
    let source = "sub s {\nif ($#ARGV >= 0) {\n    my $x = 1;\n}\n}\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(result.formatted, "sub s {\n    if ($#ARGV >= 0) {\n        my $x = 1;\n    }\n}\n");
}

#[test]
fn array_last_index_sigil_is_not_split_as_trailing_comment() {
    let formatter = NativeFormatter::new();
    let source = "my $last = $#items;\nmy $x = 1; # real\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert!(result.formatted.contains("$#items"));
    assert!(!result.formatted.contains("$   #"));
    assert!(result.formatted.contains("# real"));
}

#[test]
fn bitwise_left_shift_is_not_treated_as_heredoc() {
    let formatter = NativeFormatter::new();
    let source = "sub s {\nmy $m = 1 << BITS;\n    my $y = 2;\n}\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert!(result.formatted.contains("my $m = 1 << BITS;"));
    assert!(
        result.formatted.contains("    my $y = 2;"),
        "lines after a bitwise shift must not be treated as heredoc body: {:?}",
        result.formatted
    );
}

#[test]
fn bitwise_shift_in_trailing_comment_does_not_open_heredoc() {
    let formatter = NativeFormatter::new();
    let source = "sub s {\nmy $x = 1; # note <<EOF is not here\n    my $y = 2;\n}\n";
    let result = formatter.format_document(source, &FormatConfig::default());
    assert_eq!(
        result.formatted,
        "sub s {\n    my $x = 1; # note <<EOF is not here\n    my $y = 2;\n}\n"
    );
}

#[test]
fn real_heredoc_openers_still_preserve_body() {
    let formatter = NativeFormatter::new();
    for source in [
        "my $t = <<EOF;\nbody\nEOF\n",
        "my $t = <<~EOF;\n  body\nEOF\n",
        "my $t = <<\"EOF\";\nbody\nEOF\n",
    ] {
        let result = formatter.format_document(source, &FormatConfig::default());
        assert_eq!(result.formatted, source, "heredoc literal must be preserved");
    }
}

#[test]
fn issue_18440_formatting_is_idempotent() {
    let formatter = NativeFormatter::new();
    let cfg = FormatConfig::default();
    let source = "sub s {\nif ($#ARGV >= 0) {\nmy $m = 1 << BITS;\n}\n}\n";
    let once = formatter.format_document(source, &cfg);
    let twice = formatter.format_document(&once.formatted, &cfg);
    assert_eq!(once.formatted, twice.formatted);
}
