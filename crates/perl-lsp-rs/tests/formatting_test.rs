//! Integration tests for code formatting

use perl_lsp::convert::{WirePosition, WireRange};
use perl_lsp::features::formatting::{CodeFormatter, FormatContext, FormattingOptions};
use perl_lsp_rs_core::tooling::perltidy::native::{FormatDisposition, FormatReasonCode};
use perl_tdd_support::must;

#[test]
fn test_basic_formatting() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };

    // Test simple unformatted code supported by the native formatter.
    let code = "my$x=1;\n";

    let edits = must(formatter.format_document(code, &options));
    assert_eq!(edits.len(), 1, "native default formatting should return one edit");
    assert_eq!(edits[0].new_text, "my $x = 1;\n");
}

#[test]
fn test_range_formatting() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };

    // Multi-line code
    let code = "my $x = 1;\nsub test{return$x;}\nmy $y = 2;";

    // Format only the middle line. The endpoint is the exact UTF-16 line length.
    let range = WireRange { start: WirePosition::new(1, 0), end: WirePosition::new(1, 19) };

    let edits = must(formatter.format_range(code, &range, &options));
    assert_eq!(edits.len(), 1, "native default range formatting should return one edit");
    assert_eq!(edits[0].new_text, "sub test {\n    return $x;\n}");
}

#[test]
fn test_formatting_preserves_trailing_comment() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "my$x=1; # keep\n";

    let edits = must(formatter.format_document(code, &options));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "my $x = 1; # keep\n");
}

#[test]
fn test_formatting_preserves_simple_block_trailing_comment() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "if($ok){return 1;} # if tail\n";

    let edits = must(formatter.format_document(code, &options));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "if ($ok) {\n    return 1;\n} # if tail\n");
}

#[test]
fn test_range_formatting_preserves_trailing_comment() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "my$x=1; # keep\nmy$y=2;\n";
    let range = WireRange { start: WirePosition::new(0, 0), end: WirePosition::new(0, 14) };

    let edits = must(formatter.format_range(code, &range, &options));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "my $x = 1; # keep");
}

#[test]
fn test_range_formatting_keeps_neighboring_leading_comment_outside_selected_line() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "# applies to next declaration\nmy$x=1;\nmy$y=2;\n";
    let range = WireRange { start: WirePosition::new(1, 0), end: WirePosition::new(1, 7) };

    let edits = must(formatter.format_range(code, &range, &options));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "my $x = 1;");
    assert_eq!(edits[0].range.start.line, 1);
    assert_eq!(edits[0].range.end.line, 1);
}

#[test]
fn test_range_formatting_preserves_simple_block_trailing_comment() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "if($ok){return 1;} # if tail\nmy$z=3;\n";
    let range = WireRange { start: WirePosition::new(0, 0), end: WirePosition::new(0, 28) };

    let edits = must(formatter.format_range(code, &range, &options));

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "if ($ok) {\n    return 1;\n} # if tail");
}

#[test]
fn test_range_formatting_rejects_one_past_end_without_clamping() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };

    for (code, line, exact_end) in [
        ("my $x = 1;\nsub test{return$x;}\nmy $y = 2;", 1, 19),
        ("if($ok){return 1;} # if tail\nmy$z=3;\n", 0, 28),
    ] {
        let range = WireRange {
            start: WirePosition::new(line, 0),
            end: WirePosition::new(line, exact_end + 1),
        };
        let decision = must(formatter.format_range_decision(
            code,
            &range,
            &options,
            &FormatContext::default(),
        ));

        assert_eq!(
            decision.outcome.disposition,
            FormatDisposition::Refused,
            "a one-past-end range must refuse rather than clamp: {code:?}"
        );
        assert_eq!(decision.outcome.reason, FormatReasonCode::UnsafeRange);
        assert!(
            decision.document.edits.is_empty(),
            "a one-past-end range must not emit edits: {code:?}"
        );
        assert_eq!(decision.document.text, code);
    }
}

#[test]
fn test_range_formatting_uses_utf16_columns_for_non_bmp_text() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let code = "my$x=1; # 😀\n";
    let selected_line = "my$x=1; # 😀";
    let exact_end = selected_line.encode_utf16().count();
    assert_eq!(exact_end, 12, "the emoji occupies two UTF-16 code units");
    assert_ne!(exact_end, selected_line.len(), "UTF-16 is not byte length");

    let range =
        WireRange { start: WirePosition::new(0, 0), end: WirePosition::new(0, exact_end as u32) };
    let decision =
        must(formatter.format_range_decision(code, &range, &options, &FormatContext::default()));

    assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(decision.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(decision.document.edits.len(), 1);
    assert_eq!(decision.document.edits[0].new_text, "my $x = 1; # 😀");
}

#[test]
fn test_public_range_formatting_replay_preserves_non_bmp_prefix_and_crlf()
-> Result<(), Box<dyn std::error::Error>> {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let source = "my $emoji = \"😀\";\r\nwhile($n){next;} # 😀\r\n";
    let selected_line = "while($n){next;} # 😀";
    let range = WireRange {
        start: WirePosition::new(1, 0),
        end: WirePosition::new(1, selected_line.encode_utf16().count() as u32),
    };

    let edits = must(formatter.format_range(source, &range, &options));
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.range.end.character, selected_line.encode_utf16().count() as u32);
    assert_ne!(selected_line.len(), selected_line.encode_utf16().count());
    let start = utf16_offset(source, edit.range.start.line, edit.range.start.character);
    let end = utf16_offset(source, edit.range.end.line, edit.range.end.character);
    let expected_start =
        source.find("while").ok_or_else(|| std::io::Error::other("edited line must be present"))?;
    assert_eq!(start, expected_start);
    assert_eq!(end - start, selected_line.len());
    let mut replayed = source.to_string();
    replayed.replace_range(start..end, &edit.new_text);

    assert_eq!(replayed, "my $emoji = \"😀\";\r\nwhile ($n) {\r\n    next;\r\n} # 😀\r\n");
    Ok(())
}

#[test]
fn test_public_range_formatting_replay_preserves_true_eof_crlf_with_trim_options() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: Some(true),
        insert_final_newline: Some(true),
        trim_final_newlines: Some(true),
    };
    let source = "my $before=1;\nmy$x=1;  \r\n";
    let range = WireRange { start: WirePosition::new(1, 0), end: WirePosition::new(2, 0) };

    let edits = must(formatter.format_range(source, &range, &options));
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let start = utf16_offset(source, edit.range.start.line, edit.range.start.character);
    let end = utf16_offset(source, edit.range.end.line, edit.range.end.character);
    let mut replayed = source.to_string();
    replayed.replace_range(start..end, &edit.new_text);

    assert_eq!(edit.new_text, "my $x = 1;\r\n");
    assert_eq!(replayed, "my $before=1;\nmy $x = 1;\r\n");
    assert!(edit.new_text.ends_with("\r\n"));
    assert!(!edit.new_text.ends_with("\n\n"));
}

#[test]
fn test_public_range_formatting_replay_infers_crlf_for_unterminated_final_line() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let source = "my $before=1;\r\nwhile($n){next;}";
    let selected_line = "while($n){next;}";
    let range = WireRange {
        start: WirePosition::new(1, 0),
        end: WirePosition::new(1, selected_line.encode_utf16().count() as u32),
    };

    let edits = must(formatter.format_range(source, &range, &options));
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let start = utf16_offset(source, edit.range.start.line, edit.range.start.character);
    let end = utf16_offset(source, edit.range.end.line, edit.range.end.character);
    let mut replayed = source.to_string();
    replayed.replace_range(start..end, &edit.new_text);

    assert_eq!(edit.new_text, "while ($n) {\r\n    next;\r\n}");
    assert_eq!(replayed, "my $before=1;\r\nwhile ($n) {\r\n    next;\r\n}");
    assert!(!edit.new_text.ends_with('\n'));
}

#[test]
fn test_public_range_formatting_replay_infers_prefix_ending_for_unterminated_final_line_with_insert()
 {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: Some(true),
        trim_final_newlines: None,
    };

    for (label, source, expected_edit, expected_document) in [
        (
            "LF prefix",
            "my $a=1;\r\nmy $b=2;\nwhile($n){next;}",
            "while ($n) {\n    next;\n}\n",
            "my $a=1;\r\nmy $b=2;\nwhile ($n) {\n    next;\n}\n",
        ),
        (
            "CRLF prefix",
            "my $a=1;\nmy $b=2;\r\nwhile($n){next;}",
            "while ($n) {\r\n    next;\r\n}\r\n",
            "my $a=1;\nmy $b=2;\r\nwhile ($n) {\r\n    next;\r\n}\r\n",
        ),
    ] {
        let selected_line = "while($n){next;}";
        let range = WireRange {
            start: WirePosition::new(2, 0),
            end: WirePosition::new(2, selected_line.encode_utf16().count() as u32),
        };

        let edits = must(formatter.format_range(source, &range, &options));
        assert_eq!(edits.len(), 1, "{label}");
        let edit = &edits[0];
        let start = utf16_offset(source, edit.range.start.line, edit.range.start.character);
        let end = utf16_offset(source, edit.range.end.line, edit.range.end.character);
        let mut replayed = source.to_string();
        replayed.replace_range(start..end, &edit.new_text);

        assert_eq!(edit.new_text, expected_edit, "{label}");
        assert_eq!(replayed, expected_document, "{label}");
    }
}

#[test]
fn test_public_document_formatting_replays_mixed_prefix_ending_for_unterminated_final_line_with_insert()
 {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: Some(true),
        trim_final_newlines: None,
    };

    for (label, source, expected) in [
        (
            "CRLF then LF",
            "my $a=1;\r\nmy $b=2;\nwhile($n){next;}",
            "my $a = 1;\r\nmy $b = 2;\nwhile ($n) {\n    next;\n}\n",
        ),
        (
            "LF then CRLF",
            "my $a=1;\nmy $b=2;\r\nwhile($n){next;}",
            "my $a = 1;\nmy $b = 2;\r\nwhile ($n) {\r\n    next;\r\n}\r\n",
        ),
    ] {
        let edits = must(formatter.format_document(source, &options));
        assert_eq!(edits.len(), 1, "{label}");
        let edit = &edits[0];
        let start = utf16_offset(source, edit.range.start.line, edit.range.start.character);
        let end = utf16_offset(source, edit.range.end.line, edit.range.end.character);
        let mut replayed = source.to_string();
        replayed.replace_range(start..end, &edit.new_text);

        assert_eq!(edit.new_text, expected, "{label}");
        assert_eq!(replayed, expected, "{label}");
    }
}

fn utf16_offset(source: &str, line: u32, character: u32) -> usize {
    let mut offset = 0;
    for (line_index, line_text) in source.split_inclusive('\n').enumerate() {
        if line_index == line as usize {
            let content = line_text.strip_suffix('\n').unwrap_or(line_text);
            let content = content.strip_suffix('\r').unwrap_or(content);
            return offset
                + content
                    .char_indices()
                    .scan(0, |units, (byte, ch)| {
                        if *units >= character as usize {
                            None
                        } else {
                            *units += ch.len_utf16();
                            Some(byte + ch.len_utf8())
                        }
                    })
                    .last()
                    .unwrap_or(0);
        }
        offset += line_text.len();
    }
    offset
}

#[test]
fn test_formatting_returns_no_edits_for_literal_preserve_regions() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: Some(true),
        insert_final_newline: Some(true),
        trim_final_newlines: None,
    };

    for code in [
        "my $matched = $text =~ /needle/i;   \n",
        "$text =~ s/foo/bar/g;   \n",
        "$text =~ tr/a-z/A-Z/;   \n",
        "my @words = qw(alpha beta gamma);   \n",
        "print <<'EOF';   \nraw { text }\nEOF\n",
        "my $x = 1;   \n__DATA__\nraw fixture bytes\n",
        "my $x = 1;   \n__END__\nraw fixture bytes\n",
        "format STDOUT =\n@<<<<\n$name\n.\nwrite;   \n",
        "=pod\n\n=head1 NAME   \n\n=cut\n\nmy $x = 1;   \n",
    ] {
        let edits = must(formatter.format_document(code, &options));
        assert!(edits.is_empty(), "literal-preserve source should not produce edits: {code:?}");
    }
}

#[test]
fn test_empty_document() {
    let formatter = CodeFormatter::new();
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };

    let edits = must(formatter.format_document("", &options));
    assert!(edits.is_empty(), "native default formatting should not edit an empty document");
}
