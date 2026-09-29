#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_lsp_perltidy::native::{
    EditSpec, FormatContext, FormatDisposition, PositionEncoding, apply_edits_exact,
};
use perl_lsp_perltidy::{
    FinalNewline, FormatConfig, FormatterMode, NativeFormatter, PerlFormatter, TextPosition,
    TextRange,
};

#[test]
fn native_formatter_leaves_clean_source_unchanged_before_layout_passes_exist() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn native_formatter_can_apply_final_newline_policy_after_clean_parse() {
    let formatter = NativeFormatter::new();
    let insert = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };
    let trim = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let inserted = formatter.format_document("my $x = 1;", &insert);
    let trimmed = formatter.format_document("my $x = 1;\n\n", &trim);

    assert!(inserted.changed);
    assert_eq!(inserted.formatted, "my $x = 1;\n");
    assert!(trimmed.changed);
    assert_eq!(trimmed.formatted, "my $x = 1;");
}

#[test]
fn native_formatter_skips_edits_when_source_has_parse_diagnostics() {
    let formatter = NativeFormatter::new();
    let source = "my $x = ;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "native.format.parse_error");
    assert!(result.diagnostics[0].message.contains("does not parse cleanly"));
}

#[test]
fn native_formatter_reports_utf16_parse_error_range() {
    let formatter = NativeFormatter::new();
    let source = "my $face = \"😀\";\nmy $x = ;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.diagnostics.len(), 1);
    assert!(result.diagnostics[0].range.is_some());
}

#[test]
fn native_formatter_refuses_pod_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "=pod\n\n=head1 NAME\n\n=cut\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("POD"));
}

#[test]
fn native_formatter_trims_only_safe_eof_after_pod() -> Result<(), Box<dyn std::error::Error>> {
    let formatter = NativeFormatter::new();
    let source = "=pod\n\n=head1 NAME\n\n=cut\n\nmy $x = 1;\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert_eq!(result.formatted, "=pod\n\n=head1 NAME\n\n=cut\n\nmy $x = 1;");
    assert_eq!(result.edits.len(), 1);
    let edit = &result.edits[0];
    assert_eq!(edit.range.start.line, 6);
    assert_eq!(edit.range.end.line, 7);
    let applied = apply_edits_exact(
        source,
        &[EditSpec::new(
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
            edit.new_text.clone(),
        )],
        PositionEncoding::Utf16CodeUnits,
    )?;
    assert_eq!(applied, result.formatted);
    assert!(formatter.format_document(&result.formatted, &config).edits.is_empty());
    Ok(())
}

#[test]
fn native_formatter_trims_safe_line_only_when_heredoc_body_has_spaces()
-> Result<(), Box<dyn std::error::Error>> {
    let formatter = NativeFormatter::new();
    let source = "my $before = 1;  \nmy $text = <<'EOF';\nraw  \nEOF\n";
    let config = FormatConfig { trim_trailing_whitespace: true, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert_eq!(result.formatted, "my $before = 1;\nmy $text = <<'EOF';\nraw  \nEOF\n");
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].range.start.line, 0);
    let edit = &result.edits[0];
    let applied = apply_edits_exact(
        source,
        &[EditSpec::new(
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
            edit.new_text.clone(),
        )],
        PositionEncoding::Utf16CodeUnits,
    )?;
    assert_eq!(applied, result.formatted);
    assert!(formatter.format_document(&result.formatted, &config).edits.is_empty());
    Ok(())
}

#[test]
fn native_formatter_trim_config_applies_to_clean_document() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { trim_trailing_whitespace: true, ..FormatConfig::default() };
    let source = "my $x = 1;  \n";

    let result = formatter.format_document(source, &config);

    assert_eq!(result.formatted, "my $x = 1;\n");
    assert_eq!(result.edits.len(), 1);
}

#[test]
fn native_formatter_insert_final_newline_preserves_existing_terminal_run() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    for ending in ["\n\n", "\r\n\r\n"] {
        let source = format!("my $q = qw(a);{ending}my $x = 1;{ending}");
        let result = formatter.format_document(&source, &config);

        assert_eq!(result.formatted, source, "Insert must preserve an existing terminal run");
        assert!(result.edits.is_empty(), "no terminal bytes need an edit: {result:?}");

        let clean_source = format!("my $x = 1;{ending}");
        let clean_result = formatter.format_document(&clean_source, &config);
        assert_eq!(clean_result.formatted, clean_source);
        assert!(clean_result.edits.is_empty());
    }
}

#[test]
fn native_formatter_trim_eof_uses_cr_aware_utf16_position() -> Result<(), Box<dyn std::error::Error>>
{
    let formatter = NativeFormatter::new();
    let source = "my $q = qw(a);\rmy $x = 1;\nmy $y = 2;\n\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert_eq!(result.formatted, "my $q = qw(a);\rmy $x = 1;\nmy $y = 2;");
    assert_eq!(result.edits.len(), 1);
    let edit = &result.edits[0];
    assert_eq!(edit.range.start, TextPosition::new(2, 10));
    let applied = apply_edits_exact(
        source,
        &[EditSpec::new(
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
            edit.new_text.clone(),
        )],
        PositionEncoding::Utf16CodeUnits,
    )?;
    assert_eq!(applied, result.formatted);
    Ok(())
}

#[test]
fn native_formatter_scoped_line_after_bare_cr_uses_lsp_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let formatter = NativeFormatter::new();
    let source = "my $q = qw(a);\rmy$z=3;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.formatted, "my $q = qw(a);\rmy $z = 3;\n");
    assert_eq!(result.edits.len(), 1);
    let edit = &result.edits[0];
    assert_eq!(edit.range.start, TextPosition::new(1, 0));
    assert_eq!(edit.range.end, TextPosition::new(1, 7));
    let applied = apply_edits_exact(
        source,
        &[EditSpec::new(
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
            edit.new_text.clone(),
        )],
        PositionEncoding::Utf16CodeUnits,
    )?;
    assert_eq!(applied, result.formatted);
    Ok(())
}

#[test]
fn native_range_trim_refuses_heredoc_body_selected_without_opener() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw  \nEOF\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 5));
    let config = FormatConfig { trim_trailing_whitespace: true, ..FormatConfig::default() };

    let result = formatter.format_range(source, range, &config);

    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn native_range_trim_refuses_mixed_eol_empty_heredoc_terminator() {
    let formatter = NativeFormatter::new();
    let source = "my $x=1;\rprint <<\"my$x=1;\";\nmy$x=1;\nmy $y=2;\n__DATA__\nmy$x=1;\n";
    let range = TextRange::new(TextPosition::new(2, 0), TextPosition::new(2, 7));
    let config = FormatConfig { trim_trailing_whitespace: true, ..FormatConfig::default() };

    let result = formatter.format_range(source, range, &config);

    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn native_formatter_refuses_heredoc_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nraw { text }\nEOF\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("heredoc"));
}

#[test]
fn native_formatter_formats_clean_prefix_before_heredoc_without_touching_literal() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\nmy $text = <<'EOF';\nraw { text }\nEOF\n";
    let suffix = "my $text = <<'EOF';\nraw { text }\nEOF\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(result.changed, "a complete heredoc must not hide an earlier safe edit: {result:?}");
    assert_eq!(result.formatted, format!("my $x = 1;\n{suffix}"));
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].range.start, TextPosition::new(0, 0));
    assert_eq!(result.edits[0].range.end.line, 0);
    assert!(result.formatted.ends_with(suffix));
    assert!(
        formatter.format_document(&result.formatted, &FormatConfig::default()).edits.is_empty()
    );
}

#[test]
fn native_formatter_formats_both_sides_of_heredoc_with_scoped_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let formatter = NativeFormatter::new();
    let source =
        "my$before=1;\r\nmy $text = <<'EOF';\r\nraw 😀 { text }  \r\nEOF\r\nmy$after=2;\r\n";
    let literal = "my $text = <<'EOF';\r\nraw 😀 { text }  \r\nEOF\r\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(result.changed, "complete heredoc should admit adjacent code: {result:?}");
    assert_eq!(result.formatted, format!("my $before = 1;\r\n{literal}my $after = 2;\r\n"));
    assert_eq!(result.edits.len(), 2);
    assert_eq!(result.edits[0].range.start.line, 0);
    assert_eq!(result.edits[0].range.end.line, 0);
    assert_eq!(result.edits[1].range.start.line, 4);
    assert_eq!(result.edits[1].range.end.line, 4);
    assert!(result.formatted.contains(literal));
    let edits: Vec<_> = result
        .edits
        .iter()
        .map(|edit| {
            EditSpec::new(
                edit.range.start.line,
                edit.range.start.character,
                edit.range.end.line,
                edit.range.end.character,
                edit.new_text.clone(),
            )
        })
        .collect();
    let applied = apply_edits_exact(source, &edits, PositionEncoding::Utf16CodeUnits)?;
    assert_eq!(applied, result.formatted, "returned edits must reproduce exact CRLF bytes");
    assert!(
        formatter.format_document(&result.formatted, &FormatConfig::default()).edits.is_empty()
    );
    Ok(())
}

#[test]
fn native_formatter_keeps_typed_refusal_for_literal_only_document() {
    let formatter = NativeFormatter::new();
    let source = "my $text = <<'EOF';\nraw { text }\nEOF\n";
    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert!(typed.result.edits.is_empty());
    assert_eq!(typed.result.formatted, source);
    assert_eq!(typed.result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn native_formatter_refuses_unterminated_heredoc_despite_safe_prefix() {
    let formatter = NativeFormatter::new();
    let source = "my$before=1;\nmy $text = <<'EOF';\nunterminated\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(!result.changed, "unterminated literal cannot admit a prefix edit: {result:?}");
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn native_formatter_preserves_empty_heredoc_terminator_that_looks_like_code() {
    let formatter = NativeFormatter::new();
    let source = "print <<'my$x=1;';\nmy$x=1;\n__DATA__\nmy$x=1;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.formatted, source, "empty heredoc terminator and DATA tail are opaque");
    assert!(result.edits.is_empty(), "opaque lines must not acquire scoped edits: {result:?}");
}

#[test]
fn native_formatter_edits_before_empty_heredoc_without_touching_terminator()
-> Result<(), Box<dyn std::error::Error>> {
    let formatter = NativeFormatter::new();
    let source = "my$before=1;\nprint <<'my$x=1;';\nmy$x=1;\n__DATA__\nmy$x=1;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(
        result.formatted,
        "my $before = 1;\nprint <<'my$x=1;';\nmy$x=1;\n__DATA__\nmy$x=1;\n"
    );
    assert_eq!(result.edits.len(), 1);
    let edit = &result.edits[0];
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(edit.range.end.line, 0);
    let applied = apply_edits_exact(
        source,
        &[EditSpec::new(
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
            edit.new_text.clone(),
        )],
        PositionEncoding::Utf16CodeUnits,
    )?;
    assert_eq!(applied, result.formatted);
    Ok(())
}

#[test]
fn native_formatter_formats_real_demo_module_outside_qw_line() {
    let formatter = NativeFormatter::new();
    let fixture = include_str!("../../../demo_workspace/lib/Utils.pm");
    let source = fixture.replace("my $sum_val = sum(@$data);", "my$sum_val=sum(@$data);");
    assert_ne!(source, fixture, "fixture must contain the selected declaration");

    let result = formatter.format_document(&source, &FormatConfig::default());

    assert!(result.changed, "ordinary code after qw should still format: {result:?}");
    assert!(result.formatted.contains("my $sum_val = sum(@$data);"));
    assert!(result.formatted.contains("use List::Util qw(max min sum);"));
    assert!(
        formatter.format_document(&result.formatted, &FormatConfig::default()).edits.is_empty()
    );
}

#[test]
fn native_formatter_preserves_pod_and_regex_bytes_between_safe_lines() {
    let formatter = NativeFormatter::new();
    let source = "my$before=1;\n=pod\n  raw { text }  \n=cut\nmy $pattern = /a{2}/;\nmy$after=2;\n";
    let opaque = "=pod\n  raw { text }  \n=cut\nmy $pattern = /a{2}/;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(result.changed, "safe lines around POD and regex should admit edits: {result:?}");
    assert_eq!(result.formatted, format!("my $before = 1;\n{opaque}my $after = 2;\n"));
    assert_eq!(result.edits.len(), 2);
    assert!(result.edits.iter().all(|edit| matches!(edit.range.start.line, 0 | 5)));
    assert!(
        formatter.format_document(&result.formatted, &FormatConfig::default()).edits.is_empty()
    );
}

#[test]
fn native_formatter_refuses_data_section_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\n__DATA__\nraw\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("DATA/END section"));
}

#[test]
fn native_formatter_refuses_end_section_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\n__END__   \nraw\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("DATA/END section"));
}

#[test]
fn native_formatter_refuses_regex_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "my $matched = $text =~ /needle/i;\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("regex literal"));
}

#[test]
fn native_formatter_refuses_substitution_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "$text =~ s/foo/bar/g;\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("substitution operator"));
}

#[test]
fn native_formatter_refuses_transliteration_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "$text =~ tr/a-z/A-Z/;\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("transliteration operator"));
}

#[test]
fn native_formatter_refuses_quote_like_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "my @words = qw(alpha beta gamma);\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("quote-like operator"));
}

#[test]
fn native_formatter_refuses_format_body_until_preservation_pass_exists() {
    let formatter = NativeFormatter::new();
    let source = "format STDOUT =\n@<<<<\n$name\n.\n";
    let config = FormatConfig { final_newline: FinalNewline::Trim, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    assert!(result.diagnostics[0].message.contains("format body"));
}

#[test]
fn native_formatter_does_not_edit_code_looking_format_body_lines() {
    let formatter = NativeFormatter::new();
    let source = "my$before=1;\nformat STDOUT =\nmy$x=2;\n.\nmy$after=3;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn native_formatter_refuses_commented_format_declaration_and_body() {
    let formatter = NativeFormatter::new();
    for declaration in [
        "format STDOUT = # note",
        "format STDOUT =  ",
        "LABEL: format STDOUT =",
        "my $x=1; format STDOUT =",
    ] {
        let source = format!("my$before=1;\n{declaration}\nmy$x=2;\n.\nmy$after=3;\n");
        let result = formatter.format_document(&source, &FormatConfig::default());

        assert!(!result.changed, "format body must not be treated as safe code: {result:?}");
        assert_eq!(result.formatted, source);
        assert!(result.edits.is_empty());
        assert_eq!(result.diagnostics[0].code, "native.format.literal_preserve_region");
    }
}

#[test]
fn native_formatter_reports_parse_error_before_format_body_refusal() {
    let formatter = NativeFormatter::new();
    let source = "format STDOUT =\n.\nmy $x = ;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert_eq!(result.diagnostics[0].code, "native.format.parse_error");
}

#[test]
fn native_formatter_does_not_treat_bitshift_as_heredoc() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1 << 2;";
    let config = FormatConfig { final_newline: FinalNewline::Insert, ..FormatConfig::default() };

    let result = formatter.format_document(source, &config);

    assert!(result.changed);
    assert_eq!(result.formatted, "my $x = 1 << 2;\n");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn native_range_formatter_is_parse_gated_but_does_not_rewrite_yet() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\nmy $y = 2;\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(1, 10));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.edits.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn native_formatter_off_mode_never_parses_or_edits() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { mode: FormatterMode::Off, ..FormatConfig::default() };
    let source = "my $x = ;\n";

    let result = formatter.format_document(source, &config);

    assert!(!result.changed);
    assert_eq!(result.formatted, source);
    assert!(result.diagnostics.is_empty());
}

// ── range-format preserve-gate scoping (the fix for the over-conservative bail-out) ──

/// Core property: formatting a clean line range succeeds even when the document
/// has a regex on another line.  Before the fix, `validate_clean_parse` was
/// called on the full source, so a regex anywhere in the document would silently
/// abort range formatting — even if the requested lines were completely clean.
#[test]
fn range_format_clean_lines_succeeds_when_regex_is_elsewhere_in_document() {
    let formatter = NativeFormatter::new();
    // Line 0 (0-based) has a regex; line 1 is a clean declaration.
    // A `sub` line with `{` and `}` on one line is something the formatter
    // can reformat — use a simple sub that the formatter will recognise.
    let source = "my $ok = $t =~ /needle/;\nsub   foo{}\n";
    // Request formatting of line 1 only.
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    // Must not produce a literal_preserve_region diagnostic — the regex is not
    // in the requested range.
    assert!(
        result.diagnostics.iter().all(|d| d.code != "native.format.literal_preserve_region"),
        "should not bail with literal_preserve_region when regex is outside the range; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}

/// Range-format that overlaps a regex must still return unchanged with the
/// preserve-region diagnostic — the bail-out is correct when the construct IS
/// in the requested range.
#[test]
fn range_format_bails_when_range_itself_contains_regex() {
    let formatter = NativeFormatter::new();
    // Line 0 is clean; line 1 has a regex.
    let source = "my $x = 1;\nmy $ok = $t =~ /needle/;\n";
    // Request formatting of line 1 (where the regex lives).
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(!result.changed);
    assert!(result.edits.is_empty());
    assert_eq!(
        result.diagnostics.iter().find(|d| d.code == "native.format.literal_preserve_region"),
        result.diagnostics.first(),
        "expected a literal_preserve_region diagnostic"
    );
    assert!(
        result.diagnostics.first().is_some_and(|d| d.message.contains("regex literal")),
        "diagnostic should mention regex literal; got: {:?}",
        result.diagnostics,
    );
}

/// Whole-document formatting behavior is unregressed: a document with a regex
/// anywhere must still produce a literal_preserve_region diagnostic when
/// format_document is called.
#[test]
fn document_format_still_bails_on_regex_anywhere_in_document() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\nmy $ok = $t =~ /needle/;\n";

    let result = formatter.format_document(source, &FormatConfig::default());

    assert!(!result.changed);
    assert!(result.edits.is_empty());
    assert!(
        result.diagnostics.iter().any(|d| d.code == "native.format.literal_preserve_region"),
        "format_document should still bail for regex anywhere in the document; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}

/// Heredoc on a different line than the requested range — range-format should
/// proceed (the heredoc is outside the range).
#[test]
fn range_format_clean_lines_succeeds_when_heredoc_is_elsewhere_in_document() {
    let formatter = NativeFormatter::new();
    // The declaration follows the complete heredoc terminator.
    let source = "print <<'EOF';\nraw\nEOF\nmy$x=1;\n";
    let range = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert_eq!(result.formatted, "print <<'EOF';\nraw\nEOF\nmy $x = 1;\n");
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].range.start.line, 3);
    assert!(
        result.diagnostics.iter().all(|d| d.code != "native.format.literal_preserve_region"),
        "should not bail with literal_preserve_region when heredoc is outside the range; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}

/// Range-format that covers a line with a heredoc marker must still bail.
#[test]
fn range_format_bails_when_range_contains_heredoc() {
    let formatter = NativeFormatter::new();
    // Line 0 is clean; line 1 has a heredoc start.
    let source = "my $x = 1;\nprint <<'EOF';\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(!result.changed);
    assert!(result.edits.is_empty());
    assert!(
        result
            .diagnostics
            .first()
            .is_some_and(|d| d.code == "native.format.literal_preserve_region"
                && d.message.contains("heredoc")),
        "expected heredoc literal_preserve_region diagnostic; got: {:?}",
        result.diagnostics,
    );
}

/// A POD block outside the range must not block range-format of clean lines.
#[test]
fn range_format_clean_lines_succeeds_when_pod_is_elsewhere_in_document() {
    let formatter = NativeFormatter::new();
    // Line 0 has a POD marker; line 1 is a clean declaration.
    let source = "=head1 NAME\nmy $x = 1;\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(
        result.diagnostics.iter().all(|d| d.code != "native.format.literal_preserve_region"),
        "should not bail with literal_preserve_region when POD is outside the range; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}

/// qw() (quote-words, a quote-like operator) outside the range — range-format
/// of a clean line should succeed.
#[test]
fn range_format_clean_lines_succeeds_when_qw_is_elsewhere_in_document() {
    let formatter = NativeFormatter::new();
    // Line 0 has qw(); line 1 is clean.
    let source = "my @words = qw(alpha beta);\nmy $x = 1;\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let result = formatter.format_range(source, range, &FormatConfig::default());

    assert!(
        result.diagnostics.iter().all(|d| d.code != "native.format.literal_preserve_region"),
        "should not bail with literal_preserve_region when qw() is outside the range; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}
