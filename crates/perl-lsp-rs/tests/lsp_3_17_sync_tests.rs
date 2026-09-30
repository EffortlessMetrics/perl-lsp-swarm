//! LSP 3.17 Text Synchronization Contract Tests
//!
//! Tests for didOpen, didChange, willSave, willSaveWaitUntil, didSave, and didClose.

mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

// ==================== TEXT SYNCHRONIZATION ====================

#[test]
fn test_text_document_sync_full() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // didOpen
    harness.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": "file:///test.pl",
                "languageId": "perl",
                "version": 1,
                "text": "my $x = 42;\n"
            }
        }),
    );

    // didChange (full content — the v0.18 supported envelope)
    harness.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": "file:///test.pl",
                "version": 2
            },
            "contentChanges": [
                { "text": "my $x = 43;\nmy $y = $x;\n" }
            ]
        }),
    );

    // willSave
    harness.notify(
        "textDocument/willSave",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "reason": 1  // Manual
        }),
    );

    // willSaveWaitUntil - expects response
    let edits = harness.request(
        "textDocument/willSaveWaitUntil",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "reason": 1
        }),
    );

    if let Ok(edits) = edits {
        assert!(edits.is_array() || edits.is_null());
    }

    // didSave
    harness.notify(
        "textDocument/didSave",
        json!({
            "textDocument": { "uri": "file:///test.pl", "version": 4 },
            "text": "my $x = 43;\nmy $y = $x;\n"  // optional
        }),
    );

    // didClose
    harness.notify(
        "textDocument/didClose",
        json!({
            "textDocument": { "uri": "file:///test.pl" }
        }),
    );
    Ok(())
}

#[test]
fn full_did_change_reindexes_document_symbols_end_to_end() -> TestResult {
    let mut harness = LspHarness::new_raw();
    harness.initialize_ready("file:///workspace", None)?;

    let uri = "file:///workspace/text_sync_lifecycle.pl";
    harness.open(uri, "sub old_symbol { return 1; }\n")?;

    let before = harness.document_symbols(uri)?;
    let before_symbols = before
        .as_array()
        .ok_or_else(|| format!("documentSymbol response must be an array: {before}"))?;
    let before_names: Vec<&str> =
        before_symbols.iter().filter_map(|symbol| symbol.get("name")?.as_str()).collect();
    assert_eq!(
        before_names,
        vec!["old_symbol"],
        "symbols before full-document edit: {before_symbols:?}"
    );

    harness.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "sub new_symbol { return 1; }\n" }]
        }),
    );

    let after = harness.document_symbols(uri)?;
    let after_symbols = after
        .as_array()
        .ok_or_else(|| format!("documentSymbol response must be an array: {after}"))?;
    let after_names: Vec<&str> =
        after_symbols.iter().filter_map(|symbol| symbol.get("name")?.as_str()).collect();
    assert_eq!(
        after_names,
        vec!["new_symbol"],
        "symbols after full-document edit: {after_symbols:?}"
    );

    harness.wait_for_symbol("new_symbol", Some(uri), Duration::from_secs(2))?;
    Ok(())
}

#[test]
fn close_after_full_change_removes_reindexed_symbol_end_to_end() -> TestResult {
    let mut harness = LspHarness::new_raw();
    harness.initialize_ready("file:///workspace", None)?;

    let uri = "file:///workspace/close_after_range_change.pl";
    harness.open(uri, "sub stale_symbol { return 1; }\n")?;
    harness.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "sub closed_symbol { return 1; }\n" }]
        }),
    );

    harness.wait_for_symbol("closed_symbol", Some(uri), Duration::from_secs(2))?;
    harness.close(uri)?;

    let symbols = harness.request_with_timeout(
        "workspace/symbol",
        json!({ "query": "closed_symbol" }),
        Duration::from_secs(2),
    )?;
    let entries = symbols
        .as_array()
        .ok_or_else(|| format!("workspace/symbol response must be an array: {symbols}"))?;
    assert_eq!(
        entries.len(),
        0,
        "closed full-change symbol should be removed from workspace index: {entries:?}"
    );
    Ok(())
}
