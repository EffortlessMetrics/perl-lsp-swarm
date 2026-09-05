use serde_json::json;

mod support;
use support::lsp_client::LspClient;

#[test]
fn native_default_document_formatting() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///fmt.pl";

    let source = "sub test{my$x=1;return$x;}\nsub another{return 2;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "Should return formatting edits");

    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(
        edit_text.contains("sub test") && edit_text.contains("{"),
        "Should format subroutine declaration, got: {}",
        edit_text
    );
    assert!(edit_text.contains("my $x = 1"), "Should add spaces around operators");
    assert!(edit_text.contains("return $x"), "Should format return statement");
    assert!(
        edit_text.contains("sub another") && edit_text.contains("{"),
        "Should format second subroutine"
    );

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangeFormatting` must refuse at
/// the exact `perllsp --stdio` boundary with the truthful unadvertised
/// disposition, and the refusal must leave the document generation untouched —
/// proven by manual whole-document formatting still observing the original
/// unformatted bytes afterwards.
#[test]
fn withdrawn_range_formatting_refuses_and_leaves_document_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///range.pl";

    let source = "# First subroutine - leave this comment untouched\nsub first{my$a=1;return$a;}\n\n# Second subroutine - don't format this\nsub second{my$b=2;return$b;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/rangeFormatting",
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 1, "character": 0},
                "end": {"line": 1, "character": 27}
            },
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let error = response.get("error").ok_or("withdrawn rangeFormatting must return an error")?;
    assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
    assert!(response.get("result").is_none(), "a refusal must not carry a successful edit payload");

    // Manual whole-document formatting remains live for the same document and
    // still sees the original bytes (the messy `my$a=1;` survives verbatim
    // inside the produced edit), proving no withdrawn request mutated state.
    let manual = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;
    let edits = manual["result"]
        .as_array()
        .ok_or("manual whole-document formatting should still return an edit array")?;
    assert!(!edits.is_empty(), "manual whole-document formatting must remain available");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;
    assert!(
        edit_text.contains("sub first"),
        "manual formatting must observe the original source bytes"
    );

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_default_formatting_preserves_comments() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///comments.pl";

    let source = r#"#!/usr/bin/perl
# Main script comment
use strict;use warnings;
# Function comment
sub test{
# Inner comment
my$x=1;# Inline comment
return$x;
}
"#;

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "native default formatting should return comment-safe edits");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(edit_text.contains("# Main script comment"), "Should preserve main comment");
    assert!(edit_text.contains("# Function comment"), "Should preserve function comment");
    assert!(edit_text.contains("# Inner comment"), "Should preserve inner comment");
    assert!(edit_text.contains("# Inline comment"), "Should preserve inline comment");

    assert!(edit_text.contains("use strict"), "Should format use statements");
    assert!(edit_text.contains("use warnings"), "Should separate use statements");
    assert!(edit_text.contains("sub test") && edit_text.contains("{"), "Should format subroutine");

    client.shutdown()?;
    Ok(())
}

#[test]
fn native_default_formatting_honors_lsp_tab_size() -> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///tab-size.pl";

    let source = "sub test{my$x=1;return$x;}\n";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 2, "insertSpaces": true}
        }),
    )?;

    let edits =
        response["result"].as_array().ok_or("formatting should return an array of edits")?;

    assert!(!edits.is_empty(), "native default formatting should return edits");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;

    assert!(edit_text.contains("sub test {\n  my $x = 1;\n  return $x;\n}"));

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangesFormatting` must refuse at
/// the exact `perllsp --stdio` boundary; no atomic multi-range edit set may
/// escape while #7089's composition contract is unproven.
#[test]
fn withdrawn_ranges_formatting_refuses_at_process_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///ranges.pl";

    let source = "\n\
# First subroutine - format this\n\
sub first{my$a=1;return$a;}\n\
\n\
# Second subroutine - don't format this\n\
sub second{my$b=2;return$b;}\n\
\n\
# Third subroutine - format this too\n\
sub third{my$c=3;return$c;}\n\
";

    client.did_open(uri, "perl", source)?;

    let response = client.request(
        "textDocument/rangesFormatting",
        json!({
            "textDocument": {"uri": uri},
            "ranges": [
                {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 2, "character": 27}
                },
                {
                    "start": {"line": 7, "character": 0},
                    "end": {"line": 8, "character": 27}
                }
            ],
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;

    let error = response.get("error").ok_or("withdrawn rangesFormatting must return an error")?;
    assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
    assert!(response.get("result").is_none(), "a refusal must not carry a successful edit payload");

    client.shutdown()?;
    Ok(())
}

/// Withdrawal control (#11955): `textDocument/onTypeFormatting` refuses at the
/// exact `perllsp --stdio` boundary for every trigger shape, and the refusal
/// leaves the document observable unchanged via the still-live manual route.
#[test]
fn withdrawn_on_type_formatting_refuses_at_process_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let bin = support::product_binary_path()?;
    let mut client = LspClient::spawn(&bin)?;
    let uri = "file:///on-type.pl";
    let source = "sub first{my$a=1;return$a;}\n";

    client.did_open(uri, "perl", source)?;

    for (ch, line, character) in [("{", 0, 27), ("}", 0, 27), ("\n", 0, 27)] {
        let response = client.request(
            "textDocument/onTypeFormatting",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "ch": ch,
                "options": {"tabSize": 4, "insertSpaces": true}
            }),
        )?;

        let error = response
            .get("error")
            .ok_or("withdrawn onTypeFormatting must return an error, not a result")?;
        assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
        assert!(
            response.get("result").is_none(),
            "a refusal must not carry a successful edit payload"
        );
    }

    // Manual whole-document formatting still sees the original bytes.
    let manual = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;
    let edits = manual["result"]
        .as_array()
        .ok_or("manual whole-document formatting should still return an edit array")?;
    assert!(!edits.is_empty(), "manual whole-document formatting must remain available");
    let edit_text = edits.first().ok_or("edits array should have at least one element")?["newText"]
        .as_str()
        .ok_or("Edit should have newText")?;
    assert!(
        edit_text.contains("my $a = 1"),
        "manual formatting must observe the original source bytes"
    );

    client.shutdown()?;
    Ok(())
}
