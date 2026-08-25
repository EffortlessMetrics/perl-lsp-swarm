//! LSP 3.17 Formatting Contract Tests
//!
//! Whole-document formatting contract plus withdrawal controls for the
//! secondary edit routes (#11955).

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ==================== FORMATTING ====================

#[test]
fn test_formatting_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "sub test{my$x=1;return$x;}\n")?;

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "options": {
                "tabSize": 4,
                "insertSpaces": true,
                "trimTrailingWhitespace": true,
                "insertFinalNewline": true,
                "trimFinalNewlines": true
            }
        }),
    )?;

    let edits = result.as_array().ok_or("formatting should return an edit array")?;
    assert!(!edits.is_empty(), "native default formatting should return edits");
    let edit_text = edits[0]["newText"].as_str().ok_or("formatting edit should include newText")?;
    assert!(edit_text.contains("sub test {\n    my $x = 1;\n    return $x;\n}"));
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangeFormatting` refuses with
/// the truthful unadvertised disposition for every request shape — valid,
/// out-of-document, reversed — and never returns edits or a successful empty.
#[test]
fn test_range_formatting_is_withdrawn_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my$x=1;\nprint$x;")?;

    let ranges = [
        json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 7}}),
        json!({"start": {"line": 99, "character": 0}, "end": {"line": 99, "character": 5}}),
        json!({"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 0}}),
    ];
    for range in ranges {
        let response = harness.request_raw(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/rangeFormatting",
            "params": {
                "textDocument": { "uri": "file:///test.pl" },
                "range": range,
                "options": { "tabSize": 4, "insertSpaces": true }
            },
        }));

        let error =
            response.get("error").ok_or("withdrawn rangeFormatting must return an error")?;
        assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
        assert!(response.get("result").is_none(), "a refusal must not carry an edit payload");
    }
    Ok(())
}

/// Withdrawal control (#11955): comment-adjacent range shapes cannot yield
/// edits on the withdrawn route either.
#[test]
fn test_range_formatting_with_trailing_comment_is_refused_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///comment.pl", "my$x=1; # keep\nmy$y=2;")?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/rangeFormatting",
        "params": {
            "textDocument": { "uri": "file:///comment.pl" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 14 }
            },
            "options": { "tabSize": 4, "insertSpaces": true }
        },
    }));

    let error = response.get("error").ok_or("withdrawn rangeFormatting must return an error")?;
    assert_eq!(error["code"], -32601);
    assert!(response.get("result").is_none());
    Ok(())
}

/// Withdrawal control (#11955): a partial-line request cannot rewrite
/// neighboring leading comments or any bytes outside the interval — the route
/// refuses before geometry is even considered.
#[test]
fn test_range_formatting_neighboring_leading_comment_stays_intact_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let source = "# applies to next declaration\nmy$x=1;\nmy$y=2;";
    harness.open("file:///leading-comment.pl", source)?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/rangeFormatting",
        "params": {
            "textDocument": { "uri": "file:///leading-comment.pl" },
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 7 }
            },
            "options": { "tabSize": 4, "insertSpaces": true }
        },
    }));

    assert_eq!(
        response.pointer("/error/code").and_then(|code| code.as_i64()),
        Some(-32601),
        "partial-line requests cannot produce edits on the withdrawn route"
    );

    // The document generation is unchanged: manual whole-document formatting
    // still observes every original byte, including the leading comment.
    let manual = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": "file:///leading-comment.pl" },
            "options": { "tabSize": 4, "insertSpaces": true }
        }),
    )?;
    let edits = manual.as_array().ok_or("formatting should return an edit array")?;
    let formatted = support::test_helpers::apply_text_edits(source, edits);
    assert!(
        formatted.contains("# applies to next declaration"),
        "manual formatting must still see the original leading comment"
    );
    Ok(())
}

/// Withdrawal control (#11955): block-tail comment range shapes are refused
/// identically.
#[test]
fn test_range_formatting_block_tail_comment_is_refused_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///block-comment.pl", "if($ok){return 1;} # if tail\nmy$z=3;")?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/rangeFormatting",
        "params": {
            "textDocument": { "uri": "file:///block-comment.pl" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 28 }
            },
            "options": { "tabSize": 4, "insertSpaces": true }
        },
    }));

    let error = response.get("error").ok_or("withdrawn rangeFormatting must return an error")?;
    assert_eq!(error["code"], -32601);
    assert!(response.get("result").is_none());
    Ok(())
}

#[test]
fn test_formatting_returns_no_edits_for_literal_preserve_regions_3_17() -> TestResult {
    for (uri, source) in [
        ("file:///regex-format.pl", "my $matched = $text =~ /needle/i;   \n"),
        ("file:///substitution-format.pl", "$text =~ s/foo/bar/g;   \n"),
        ("file:///transliteration-format.pl", "$text =~ tr/a-z/A-Z/;   \n"),
        ("file:///quote-like-format.pl", "my @words = qw(alpha beta gamma);   \n"),
        ("file:///heredoc-format.pl", "print <<'EOF';   \nraw { text }\nEOF\n"),
        ("file:///data-format.pl", "my $x = 1;   \n__DATA__\nraw fixture bytes\n"),
        ("file:///end-format.pl", "my $x = 1;   \n__END__\nraw fixture bytes\n"),
        ("file:///format-body-format.pl", "format STDOUT =\n@<<<<\n$name\n.\nwrite;   \n"),
        ("file:///pod-format.pl", "=pod\n\n=head1 NAME   \n\n=cut\n\nmy $x = 1;   \n"),
    ] {
        let mut harness = LspHarness::new();
        harness.initialize(None)?;
        harness.open(uri, source)?;

        let result = harness.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": 4,
                    "insertSpaces": true,
                    "trimTrailingWhitespace": true,
                    "insertFinalNewline": true
                }
            }),
        )?;

        let edits = result.as_array().ok_or("formatting should return an edit array")?;
        assert!(edits.is_empty(), "literal-preserve source should not produce edits: {source:?}");
    }
    Ok(())
}

/// Withdrawal control (#11955): on-type formatting refuses while #9320's
/// cutover is unproven, including when the trigger arrives for a disabled
/// formatter.
#[test]
fn test_on_type_formatting_is_withdrawn_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "if (1) {")?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/onTypeFormatting",
        "params": {
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 0, "character": 8 },
            "ch": "{",
            "options": { "tabSize": 4, "insertSpaces": true }
        },
    }));

    let error = response.get("error").ok_or("withdrawn onTypeFormatting must refuse")?;
    assert_eq!(error["code"], -32601);
    assert!(response.get("result").is_none(), "refusal cannot carry edits");
    Ok(())
}
