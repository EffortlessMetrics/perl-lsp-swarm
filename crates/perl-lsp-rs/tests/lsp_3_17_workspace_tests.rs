//! LSP 3.17 Workspace Feature Contract Tests
//!
//! Tests for workspace/symbol, workspaceSymbol/resolve, workspace/executeCommand,
//! workspace/didChangeWorkspaceFolders, file operations, and watched files.

mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn partial_progress_symbol_rows(notifications: &[Value], token: &str) -> Vec<Value> {
    notifications
        .iter()
        .filter(|notification| {
            notification.pointer("/params/token").and_then(Value::as_str) == Some(token)
        })
        .filter_map(|notification| notification.pointer("/params/value"))
        .filter_map(Value::as_array)
        .flat_map(|chunk| chunk.iter().cloned())
        .collect()
}

// ==================== WORKSPACE FEATURES ====================

#[test]
fn test_workspace_symbol_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test_symbol_3_17.pl";
    harness.open(uri, "sub test { return 1; }\nsub other { return 2; }\n")?;

    let response = harness
        .request(
            "workspace/symbol",
            json!({
                "query": "test",
                "workDoneToken": "symbol-1"
            }),
        )
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    // Exact: only the `test` sub matches; null or empty fails (#17685).
    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names.len(), symbols.len(), "every symbol must carry a name: {response:?}");
    assert_eq!(names, vec!["test"], "got: {response:?}");
    assert_eq!(symbols[0]["kind"].as_u64(), Some(12), "got: {symbols:?}");
    assert_eq!(symbols[0]["location"]["uri"].as_str(), Some(uri), "got: {symbols:?}");
    Ok(())
}

#[test]
fn test_workspace_symbol_partial_result_streaming_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test_partial_result_3_17.pl";
    harness.open(uri, "sub test { return 1; }\nsub other { return 2; }\n")?;

    let _ = harness.drain_notifications(None, 100);

    let token = "ws-symbol-partial-1776";
    let final_response = harness.request_with_timeout(
        "workspace/symbol",
        json!({
            "query": "test",
            "partialResultToken": token
        }),
        Duration::from_secs(5),
    )?;

    assert!(
        final_response.is_null(),
        "final workspace/symbol response must be null when partialResultToken is set; got {final_response:?}"
    );

    let progress = harness.drain_notifications(Some("$/progress"), 500);
    let matching: Vec<_> = progress
        .iter()
        .filter(|notification| {
            notification.pointer("/params/token").and_then(Value::as_str) == Some(token)
        })
        .collect();
    assert!(
        !matching.is_empty(),
        "expected workspace/symbol partial $/progress for token {token}; notifications: {progress:?}"
    );

    let streamed_rows = partial_progress_symbol_rows(&progress, token);
    let matching_rows: Vec<_> = streamed_rows
        .iter()
        .filter(|symbol| symbol.pointer("/location/uri").and_then(Value::as_str) == Some(uri))
        .filter(|symbol| symbol["name"].as_str() == Some("test"))
        .collect();
    assert!(
        !matching_rows.is_empty(),
        "streamed partial results must include the open-document symbol; got {streamed_rows:?}"
    );
    assert!(
        matching_rows.iter().all(|symbol| symbol["name"].as_str() == Some("test")),
        "every streamed row for this document must be the expected symbol"
    );

    Ok(())
}

#[test]
fn test_workspace_symbol_resolve_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test_resolve_3_17.pl";
    harness.open(uri, "sub test { return 1; }\n")?;

    // The advertised resolveProvider makes this spec method part of the
    // contract: it must answer (not -32601) and keep the symbol identity.
    let symbol = harness
        .request(
            "workspaceSymbol/resolve",
            json!({
                "name": "test",
                "kind": 12,
                "location": {
                    "uri": uri,
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 8 }
                    }
                }
            }),
        )
        .map_err(|e| format!("workspaceSymbol/resolve request failed: {e}"))?;

    assert_eq!(symbol["name"].as_str(), Some("test"), "got: {symbol:?}");
    assert_eq!(symbol["kind"].as_i64(), Some(12), "got: {symbol:?}");
    assert_eq!(symbol["detail"].as_str(), Some("sub test"), "got: {symbol:?}");
    Ok(())
}

#[test]
fn test_execute_command_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    let response = harness.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.extractVariable",
            "arguments": [
                "file:///test.pl",
                { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }
            ],
            "workDoneToken": "cmd-1"
        }),
    );

    // May fail if command not supported
    if let Ok(_result) = response {
        // Result can be any value
    }
    Ok(())
}

#[test]
fn test_workspace_folders_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // Notify of workspace folder changes
    harness.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({
            "event": {
                "added": [
                    { "uri": "file:///workspace2", "name": "Workspace 2" }
                ],
                "removed": []
            }
        }),
    );

    // Server can also request current folders (if needed)
    // This would be a server->client request, so we skip it in tests
    Ok(())
}

#[test]
fn test_file_operations_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // willCreateFiles
    let response = harness.request(
        "workspace/willCreateFiles",
        json!({
            "files": [
                { "uri": "file:///new.pl" }
            ]
        }),
    );

    if let Ok(edit) = response {
        assert!(edit.is_null() || edit.is_object());
    }

    // didCreateFiles
    harness.notify(
        "workspace/didCreateFiles",
        json!({
            "files": [
                { "uri": "file:///new.pl" }
            ]
        }),
    );

    // willRenameFiles
    let response = harness.request(
        "workspace/willRenameFiles",
        json!({
            "files": [
                { "oldUri": "file:///old.pl", "newUri": "file:///new.pl" }
            ]
        }),
    );

    if let Ok(edit) = response {
        assert!(edit.is_null() || edit.is_object());
    }

    // didRenameFiles
    harness.notify(
        "workspace/didRenameFiles",
        json!({
            "files": [
                { "oldUri": "file:///old.pl", "newUri": "file:///new.pl" }
            ]
        }),
    );

    // willDeleteFiles
    let response = harness.request(
        "workspace/willDeleteFiles",
        json!({
            "files": [
                { "uri": "file:///delete.pl" }
            ]
        }),
    );

    if let Ok(edit) = response {
        assert!(edit.is_null() || edit.is_object());
    }

    // didDeleteFiles
    harness.notify(
        "workspace/didDeleteFiles",
        json!({
            "files": [
                { "uri": "file:///delete.pl" }
            ]
        }),
    );
    Ok(())
}

#[test]
fn test_watched_files_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    harness.notify(
        "workspace/didChangeWatchedFiles",
        json!({
            "changes": [
                { "uri": "file:///test.pl", "type": 2 },  // Changed
                { "uri": "file:///new.pl", "type": 1 },   // Created
                { "uri": "file:///old.pl", "type": 3 }    // Deleted
            ]
        }),
    );
    Ok(())
}
