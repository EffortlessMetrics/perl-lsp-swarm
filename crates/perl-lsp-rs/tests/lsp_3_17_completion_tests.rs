//! LSP 3.17 Completion Contract Tests
//!
//! Tests for textDocument/completion.

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ==================== COMPLETION ====================

#[test]
fn test_completion_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "print $")?;

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 0, "character": 7 },
            "context": {
                "triggerKind": 1,  // Invoked
                "triggerCharacter": "$"
            }
        }),
    )?;

    // Response can be array or CompletionList
    assert!(response.is_array() || (response.is_object() && response.get("items").is_some()));
    Ok(())
}

// ==================== TRIGGER CHARACTER TESTS (#5295) ====================
//
// The server advertises trigger characters: $ @ % - > : / \ " '
// These tests exercise the trigger-character dispatch path with
// triggerKind=2 (TriggerCharacter), which was previously untested.

/// Extract completion items from either an array or CompletionList response.
fn extract_items(response: &serde_json::Value) -> Vec<&serde_json::Value> {
    if let Some(items) = response.get("items").and_then(|v| v.as_array()) {
        items.iter().collect()
    } else if let Some(arr) = response.as_array() {
        arr.iter().collect()
    } else {
        Vec::new()
    }
}

fn has_label_containing(items: &[&serde_json::Value], needle: &str) -> bool {
    items.iter().any(|item| {
        item.get("label").and_then(|v| v.as_str()).is_some_and(|label| label.contains(needle))
    })
}

#[test]
fn test_trigger_dollar_completes_scalar_variables() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my $name = 'Bob';\nprint $")?;

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 1, "character": 7 },
            "context": {
                "triggerKind": 2,  // TriggerCharacter
                "triggerCharacter": "$"
            }
        }),
    )?;

    let items = extract_items(&response);
    assert!(
        has_label_containing(&items, "$name"),
        "trigger '$' should complete scalar variables including $name; got: {response}"
    );
    Ok(())
}

#[test]
fn test_trigger_arrow_completes_methods() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(
        "file:///test.pl",
        "package Foo;\nsub bar { }\nsub baz { }\n1;\nmy $f = Foo->new;\n$f->",
    )?;

    // Position is right after "->" on line 5 (0-indexed)
    let line = 5;
    let char_pos = 4; // after "$f->"

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": line, "character": char_pos },
            "context": {
                "triggerKind": 2,  // TriggerCharacter
                "triggerCharacter": ">"
            }
        }),
    )?;

    let items = extract_items(&response);
    // The server should offer method completions (at minimum, not return empty)
    assert!(
        !items.is_empty(),
        "trigger '>' should produce method completions; got empty response: {response}"
    );
    Ok(())
}

#[test]
fn test_trigger_percent_completes_hash_variables() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my %config = ();\nprint %")?;

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 1, "character": 7 },
            "context": {
                "triggerKind": 2,  // TriggerCharacter
                "triggerCharacter": "%"
            }
        }),
    )?;

    let items = extract_items(&response);
    assert!(
        has_label_containing(&items, "%config"),
        "trigger '%' should complete hash variables including %config; got: {response}"
    );
    Ok(())
}

#[test]
fn test_trigger_at_completes_array_variables() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my @items = ();\nprint @")?;

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 1, "character": 7 },
            "context": {
                "triggerKind": 2,  // TriggerCharacter
                "triggerCharacter": "@"
            }
        }),
    )?;

    let items = extract_items(&response);
    assert!(
        has_label_containing(&items, "@items"),
        "trigger '@' should complete array variables including @items; got: {response}"
    );
    Ok(())
}
