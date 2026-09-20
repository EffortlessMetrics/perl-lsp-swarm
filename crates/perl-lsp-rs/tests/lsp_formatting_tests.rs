//! Tests for the proven manual whole-document formatting route plus
//! withdrawal controls for the secondary edit routes (#11955).
//!
//! Validates:
//! - Full document formatting (`textDocument/formatting`, still live)
//! - Formatting options (tabSize, insertSpaces)
//! - Capability advertisement agreeing with runtime reachability
//! - Graceful handling when formatter produces no changes
//! - Withdrawn routes (`textDocument/rangeFormatting`) refusing with the
//!   truthful unadvertised disposition instead of producing edits

mod support;
use serde_json::json;
use support::lsp_harness::LspHarness;
use support::test_helpers::apply_text_edits;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Test whole-document formatting request structure and response
#[test]
fn test_formatting_whole_document() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_format.pl";
    harness.open(
        doc_uri,
        r#"sub hello{my$name=shift;return 1;}
sub world{return "world";}
"#,
    )?;

    let response = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;

    let edits = response.as_array().ok_or("formatting response must be an array")?;
    assert_eq!(edits.len(), 1, "native formatting should produce one document edit");
    let formatted = apply_text_edits(
        r#"sub hello{my$name=shift;return 1;}
sub world{return "world";}
"#,
        edits,
    );
    // Pins the current whole-document outcome including the true-EOF
    // correction: one final newline after the last sub, not two.
    assert_eq!(
        formatted,
        concat!(
            "sub hello {\n",
            "    my $name = shift;\n",
            "    return 1;\n",
            "}\n",
            "sub world {\n",
            "    return \"world\";\n",
            "}\n",
        )
    );

    Ok(())
}

/// Withdrawal control (#11955): `textDocument/rangeFormatting` must refuse
/// with the truthful unadvertised disposition — never edits, never a
/// successful empty — while manual whole-document formatting stays live for
/// the same document and the refused request leaves the buffer untouched.
#[test]
fn test_formatting_range_is_withdrawn_and_leaves_source_unchanged() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_range_withdrawn.pl";
    let source = "\n\
# Well-formatted section\n\
sub clean_func {\n\
    my $x = 1;\n\
    return $x;\n\
}\n\
\n\
# Poorly formatted line\n\
sub messy{my$y=2;return$y;}\n\
";
    harness.open(doc_uri, source)?;

    let withdrawn_response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/rangeFormatting",
        "params": {
            "textDocument": { "uri": doc_uri },
            "range": {
                "start": { "line": 7, "character": 0 },
                "end": { "line": 7, "character": 26 }
            },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        },
    }));
    let error = withdrawn_response
        .get("error")
        .ok_or("withdrawn rangeFormatting must return an error, not a result")?;
    assert_eq!(error["code"], -32601, "refusal must be MethodNotFound (-32601)");
    assert!(
        withdrawn_response.get("result").is_none(),
        "a refusal must not carry a successful result payload"
    );

    // Invalid geometry variants are equally refused before any parsing.
    for bad_range in [
        json!({"start": {"line": 99, "character": 0}, "end": {"line": 99, "character": 5}}),
        json!({"start": {"line": 7, "character": 10}, "end": {"line": 7, "character": 2}}),
    ] {
        let response = harness.request_raw(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/rangeFormatting",
            "params": {
                "textDocument": { "uri": doc_uri },
                "range": bad_range,
                "options": {"tabSize": 4, "insertSpaces": true}
            },
        }));
        assert_eq!(
            response.pointer("/error/code").and_then(|code| code.as_i64()),
            Some(-32601),
            "invalid geometry cannot yield an edit on a withdrawn route"
        );
    }

    // Manual whole-document formatting remains available and still sees the
    // original bytes: formatting output contains the messy line's content,
    // proving no withdrawn request mutated the document generation.
    let manual = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {"tabSize": 4, "insertSpaces": true}
        }),
    )?;
    let edits = manual.as_array().ok_or("formatting response must be an array")?;
    let formatted = apply_text_edits(source, edits).replace(char::is_whitespace, "");
    assert!(
        formatted.contains("submessy{"),
        "manual formatting must still observe the original source bytes"
    );

    Ok(())
}

/// Test that formatting options like tabSize are passed through
#[test]
fn test_formatting_options_tab_size() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_tab_size.pl";
    harness.open(
        doc_uri,
        r#"sub test{my$x=1;return$x;}
"#,
    )?;

    let source = r#"sub test{my$x=1;return$x;}
"#;

    let response_2 = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 2,
                "insertSpaces": true
            }
        }),
    )?;

    harness.open("file:///test_tab_size_8.pl", source)?;

    let response_8 = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": "file:///test_tab_size_8.pl" },
            "options": {
                "tabSize": 8,
                "insertSpaces": true
            }
        }),
    )?;

    let edits_2 = response_2.as_array().ok_or("tabSize 2 response must be an array")?;
    let edits_8 = response_8.as_array().ok_or("tabSize 8 response must be an array")?;
    let formatted_2 = apply_text_edits(source, edits_2);
    let formatted_8 = apply_text_edits(source, edits_8);
    assert!(
        formatted_2.contains("\n  my $x = 1;"),
        "tabSize 2 should use two-space indentation, got:\n{formatted_2}"
    );
    assert!(
        formatted_8.contains("\n        my $x = 1;"),
        "tabSize 8 should use eight-space indentation, got:\n{formatted_8}"
    );

    Ok(())
}

/// Test that capability advertisement agrees with runtime reachability (#11955):
/// whole-document formatting stays advertised; withdrawn range formatting must
/// not be promised.
#[test]
fn test_formatting_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    let init_response = harness.initialize(None)?;

    let capabilities = &init_response["capabilities"];

    // Check for documentFormattingProvider
    let has_formatting = capabilities.get("documentFormattingProvider").is_some();
    assert!(
        has_formatting,
        "Server should advertise documentFormattingProvider capability. Capabilities: {:?}",
        capabilities
    );

    // Withdrawn route (#11955): advertisement must not promise range formatting.
    assert!(
        capabilities.get("documentRangeFormattingProvider").is_none(),
        "documentRangeFormattingProvider is withdrawn and must not be advertised. Capabilities: {:?}",
        capabilities
    );

    Ok(())
}

/// Test formatting on already well-formatted code returns empty edits.
#[test]
fn test_formatting_well_formatted_code() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_clean.pl";
    harness.open(
        doc_uri,
        r#"use strict;
use warnings;

sub clean_function {
    my $x = 1;
    my $y = 2;
    return $x + $y;
}
1;
"#,
    )?;

    let response = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;

    let edits = response.as_array().ok_or("formatting response must be an array")?;
    assert!(edits.is_empty(), "well-formatted native code should not be edited");

    Ok(())
}

/// Test that formatting requests return no edits when formatting is disabled at runtime.
#[test]
fn test_formatting_disabled_via_configuration_returns_no_edits() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "formatting": {
                        "enabled": false
                    }
                }
            }
        }),
    );

    let doc_uri = "file:///test_formatting_disabled.pl";
    harness.open(doc_uri, "sub messy{my$x=1;return$x;}\n")?;

    let response = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;

    let edits = response.as_array().ok_or("formatting response must be an array")?;
    assert!(edits.is_empty(), "expected no formatting edits when formatting is disabled");

    Ok(())
}

/// Test that configured native formatter policy fields route through LSP document formatting.
#[test]
fn test_native_formatting_policies_route_through_document_formatting() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "formatting": {
                        "engine": "native",
                        "maximumLineLength": 20,
                        "openingBraceOnNewLine": true,
                        "cuddledElse": false,
                        "spaceAfterKeyword": false,
                        "addTrailingCommas": true
                    }
                }
            }
        }),
    );

    let doc_uri = "file:///test_native_formatting_policies.pl";
    let source = "if($ok){return foo($alpha,$beta,$gamma);}else{return bar();}\n";
    harness.open(doc_uri, source)?;

    let response = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;
    let edits = response.as_array().ok_or("formatting response must be an array")?;
    let formatted = edits
        .first()
        .and_then(|edit| edit.get("newText"))
        .and_then(|text| text.as_str())
        .ok_or("formatting edit must include newText")?;

    assert_eq!(
        formatted,
        concat!(
            "if($ok)\n",
            "{\n",
            "    return foo(\n",
            "    $alpha,\n",
            "    $beta,\n",
            "    $gamma,\n",
            ");\n",
            "}\n",
            "else\n",
            "{\n",
            "    return bar();\n",
            "}\n",
        )
    );

    Ok(())
}

/// Withdrawal control (#11955): formatter policy configuration cannot re-arm
/// the withdrawn range route — it refuses identically regardless of engine or
/// style settings.
#[test]
fn test_native_formatting_policies_route_through_range_formatting() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "formatting": {
                        "engine": "native",
                        "openingBraceOnNewLine": true,
                        "cuddledElse": false,
                        "spaceAfterKeyword": false
                    }
                }
            }
        }),
    );

    let doc_uri = "file:///test_native_range_formatting_policies.pl";
    harness.open(doc_uri, "my $prefix = 1;\nif($ok){return 1;}else{return 0;}\n")?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/rangeFormatting",
        "params": {
            "textDocument": { "uri": doc_uri },
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 34 }
            },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        },
    }));

    assert_eq!(
        response.pointer("/error/code").and_then(|code| code.as_i64()),
        Some(-32601),
        "withdrawn rangeFormatting must refuse even with live formatting configuration"
    );
    assert!(response.get("result").is_none(), "refusal must not carry an edit payload");

    Ok(())
}

/// Test formatting on a file with only comments
#[test]
fn test_formatting_comments_only() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_comments.pl";
    harness.open(
        doc_uri,
        r#"#!/usr/bin/perl
# This file has only comments
# No actual code to format
# Just checking that formatting handles this gracefully
"#,
    )?;

    let response = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": doc_uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;

    let edits = response.as_array().ok_or("formatting response must be an array")?;
    assert!(edits.is_empty(), "comment-only native formatting should return no edits");

    Ok(())
}
