use perl_lsp_perltidy::{FinalNewline, FormatConfig, NativeFormatter, PerlFormatter};

fn fmt(source: &str, config: &FormatConfig) -> String {
    NativeFormatter::new().format_document(source, config).formatted
}

#[test]
fn trim_final_newline_does_not_strip_data_payload_newline() {
    let source = "my $x = 1;\n__DATA__\npayload\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };
    assert_eq!(fmt(source, &config), source);
}

#[test]
fn trim_final_newline_does_not_strip_end_payload_newline() {
    let source = "my $x = 1;\n__END__\npayload\n\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };
    assert_eq!(fmt(source, &config), source);
}

#[test]
fn punctuation_quoted_heredoc_label_body_is_preserved() {
    let source = "print <<\"=\";\nmy $z=1;\n=\nmy $x=1;\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("my $z=1;\n=\n"), "body must be byte-exact: {out:?}");
}

#[test]
fn pod_cutlery_does_not_end_pod_region() {
    let source = "=pod\n=cutlery is not a cut\nmy $z=1;\n=cut\nmy $x=1;\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("my $z=1;\n"), "POD body preserved: {out:?}");
}

#[test]
fn preserved_regex_header_opens_indent_for_body() {
    let source = "if ($x =~ /}/) {\nmy $y = 1;\n}\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("\n    my $y = 1;\n"), "body indented under regex header: {out:?}");
}

#[test]
fn multiple_heredocs_on_one_line_bodies_are_preserved_fifo() {
    let source = "print <<A, <<B;\nmy $a=1;\nA\nmy $b=2;\nB\nmy $x=1;\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("my $a=1;\nA\nmy $b=2;\nB\n"), "both bodies byte-exact: {out:?}");
}

#[test]
fn plain_heredoc_indented_body_line_matching_label_does_not_end_region() {
    let source = "print <<A;\n  A\nmy $z=1;\nA\nmy $x=1;\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("my $z=1;\nA\n"), "indented label-like body preserved: {out:?}");
}

#[test]
fn indented_heredoc_terminator_may_be_indented() {
    let source = "print <<~A;\n  my $z=1;\n  A\nmy $x=1;\n";
    let out = fmt(source, &FormatConfig::default());
    assert!(out.contains("  my $z=1;\n  A\n"), "{out:?}");
}
