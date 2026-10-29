//! LSP 3.17 Rename Contract Tests
//!
//! Tests for textDocument/rename and textDocument/prepareRename.
//!
//! These tests assert exact results (edit count, URIs, UTF-16 ranges, new
//! text, placeholder). A null, empty, or off-by-one response fails: the
//! suites previously accepted `null` or any object, so a broken rename
//! handler stayed green (#17685).

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Extract the single-URI edit list from a rename WorkspaceEdit.
fn exact_edits(response: &Value, uri: &str) -> Result<Vec<(u64, u64, u64, String)>, String> {
    if response.is_null() {
        return Err("rename returned null for a renameable variable".to_string());
    }
    let changes = response
        .get("changes")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("rename response has no changes object: {response}"))?;
    if changes.len() != 1 || !changes.contains_key(uri) {
        return Err(format!("rename changes must key exactly {uri}, got: {response}"));
    }
    let edits = changes[uri]
        .as_array()
        .ok_or_else(|| format!("rename edits for {uri} are not an array: {response}"))?;
    edits
        .iter()
        .map(|edit| {
            let range = edit.get("range").ok_or("edit missing range")?;
            let line = range["start"]["line"].as_u64().ok_or("edit start.line missing")?;
            let start =
                range["start"]["character"].as_u64().ok_or("edit start.character missing")?;
            let end = range["end"]["character"].as_u64().ok_or("edit end.character missing")?;
            if range["end"]["line"].as_u64() != Some(line) {
                return Err("edit must stay on one line".to_string());
            }
            let new_text = edit["newText"].as_str().ok_or("edit newText missing")?.to_string();
            Ok((line, start, end, new_text))
        })
        .collect()
}

// ==================== RENAME ====================

#[test]
fn test_rename_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test.pl";
    harness.open(uri, "my $old = 1;\n$old++;")?;

    let response = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 4 },
            "newName": "new"
        }),
    )?;

    // Declaration `my $old` (chars 3..7) plus the `$old++` use (chars 0..4),
    // each rewritten with the sigil preserved.
    assert_eq!(
        exact_edits(&response, uri)?,
        vec![(0, 3, 7, "$new".to_string()), (1, 0, 4, "$new".to_string()),],
        "rename must return exactly the declaration and use edits, got: {response}"
    );
    Ok(())
}

/// Given a variable used exactly 3 times on lines after an emoji-prefixed
/// declaration line, When rename is requested, Then exactly those 3 edits
/// come back at exact UTF-16 ranges.
///
/// The emoji (U+1F600) is 4 bytes / 1 char / 2 UTF-16 units, so byte- or
/// char-based positions land off by one and fail this test.
#[test]
fn test_rename_3_17_emoji_utf16_exact_edits() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test_emoji_rename.pl";
    harness.open(uri, "my $pad = \"\u{1F600}\"; my $old = 1;\n$old++;\nprint $old;\n")?;

    let response = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 20 },
            "newName": "new"
        }),
    )?;

    assert_eq!(
        exact_edits(&response, uri)?,
        vec![
            (0, 19, 23, "$new".to_string()),
            (1, 0, 4, "$new".to_string()),
            (2, 6, 10, "$new".to_string()),
        ],
        "rename must return exactly 3 UTF-16 edits, got: {response}"
    );
    Ok(())
}

#[test]
fn test_prepare_rename_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///test_prepare_rename.pl";
    harness.open(uri, "my $pad = \"\u{1F600}\"; my $old = 1;\n$old++;\n")?;

    let response = harness.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 20 }
        }),
    )?;

    if response.is_null() {
        return Err("prepareRename returned null for a renameable variable".into());
    }
    // {range, placeholder} form: the exact $old span (UTF-16 19..23, shifted
    // by the emoji's second UTF-16 unit) and the current name as placeholder.
    assert_eq!(response["placeholder"].as_str(), Some("$old"), "got: {response}");
    assert_eq!(response["range"]["start"]["line"].as_u64(), Some(0), "got: {response}");
    assert_eq!(response["range"]["start"]["character"].as_u64(), Some(19), "got: {response}");
    assert_eq!(response["range"]["end"]["line"].as_u64(), Some(0), "got: {response}");
    assert_eq!(response["range"]["end"]["character"].as_u64(), Some(23), "got: {response}");
    Ok(())
}
