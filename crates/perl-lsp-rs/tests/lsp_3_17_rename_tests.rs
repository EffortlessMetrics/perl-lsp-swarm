//! LSP 3.17 Rename Contract Tests
//!
//! Tests for textDocument/rename and textDocument/prepareRename.
//!
//! These tests assert non-null, exact results — edit count, URIs, UTF-16
//! ranges, and new text — so a server that returns `null`, an empty edit, or
//! off-by-one ranges fails instead of passing vacuously (#17685).

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Extract the single-URI edit list from a rename `WorkspaceEdit`, asserting
/// the response is a non-null object whose `changes` map has exactly the
/// expected URI.
fn edits_for(response: &Value, uri: &str) -> TestResult<Vec<Value>> {
    assert!(!response.is_null(), "rename must not return null, got: {response:?}");
    let changes = response
        .get("changes")
        .and_then(Value::as_object)
        .ok_or("rename response should include a `changes` object")?;
    assert_eq!(changes.len(), 1, "rename should edit exactly one URI, got: {changes:?}");
    let edits = changes
        .get(uri)
        .and_then(Value::as_array)
        .ok_or("rename response should include edits for the renamed document")?;
    Ok(edits.clone())
}

fn edit(start_line: u64, start_char: u64, end_line: u64, end_char: u64, new_text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": start_line, "character": start_char },
            "end": { "line": end_line, "character": end_char }
        },
        "newText": new_text
    })
}

// ==================== RENAME ====================

#[test]
fn test_rename_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my $old = 1;\n$old++;")?;

    let response = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 0, "character": 4 },
            "newName": "new"
        }),
    )?;

    // `$old` occurs exactly twice: the declaration (line 0, UTF-16 3..7) and
    // the increment (line 1, UTF-16 0..4). Both edits carry the sigil.
    let edits = edits_for(&response, "file:///test.pl")?;
    assert_eq!(
        edits,
        vec![edit(0, 3, 0, 7, "$new"), edit(1, 0, 1, 4, "$new")],
        "rename should return exactly the two $old edits at exact ranges"
    );
    Ok(())
}

#[test]
fn test_prepare_rename_3_17() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///test.pl", "my $var = 1;")?;

    let response = harness.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 0, "character": 4 }
        }),
    )?;

    // A concrete range/placeholder for `$var` (line 0, UTF-16 3..7), not null.
    assert_eq!(
        response,
        json!({
            "range": {
                "start": { "line": 0, "character": 3 },
                "end": { "line": 0, "character": 7 }
            },
            "placeholder": "$var"
        }),
        "prepareRename should return the exact $var range and placeholder"
    );
    Ok(())
}

// ==================== UTF-16 (emoji / CJK) ====================
//
// Fixture (three occurrences of `$counter`):
//   line 0: `my $counter = 1;`
//   line 1: `print "🎉漢"; $counter++;`
//   line 2: `print $counter;`
//
// On line 1 the prefix before `$counter` is `print "` (7 UTF-16 units) +
// `🎉` (2 UTF-16 units, 1 char, 4 bytes) + `漢` (1 UTF-16 unit, 1 char,
// 3 bytes) + `"; ` (3 units), so `$counter` starts at UTF-16 character 13.
// A char-counting handler would say 12 and a byte-counting handler 17; only
// UTF-16 gives 13. `$counter` spans 8 UTF-16 units including the sigil.

const EMOJI_FIXTURE: &str =
    "my $counter = 1;\nprint \"\u{1F389}\u{6F22}\"; $counter++;\nprint $counter;\n";
const EMOJI_URI: &str = "file:///emoji_rename.pl";

fn expected_counter_edits() -> Vec<Value> {
    vec![
        edit(0, 3, 0, 11, "$renamed"),
        edit(1, 13, 1, 21, "$renamed"),
        edit(2, 6, 2, 14, "$renamed"),
    ]
}

#[test]
fn test_rename_emoji_cjk_utf16_exact_edits() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(EMOJI_URI, EMOJI_FIXTURE)?;

    // Requested from the declaration on line 0.
    let response = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": EMOJI_URI },
            "position": { "line": 0, "character": 4 },
            "newName": "renamed"
        }),
    )?;
    let edits = edits_for(&response, EMOJI_URI)?;
    assert_eq!(
        edits,
        expected_counter_edits(),
        "rename should return exactly 3 edits at exact UTF-16 ranges, got: {edits:?}"
    );

    // Requested from the emoji/CJK-prefixed occurrence itself (UTF-16
    // character 14 is inside `$counter` on line 1): the incoming position
    // conversion must land on the same three edits.
    let from_prefixed = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": EMOJI_URI },
            "position": { "line": 1, "character": 14 },
            "newName": "renamed"
        }),
    )?;
    let edits = edits_for(&from_prefixed, EMOJI_URI)?;
    assert_eq!(
        edits,
        expected_counter_edits(),
        "rename from the emoji/CJK line should return the same 3 exact edits, got: {edits:?}"
    );
    Ok(())
}

#[test]
fn test_prepare_rename_emoji_cjk_utf16() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(EMOJI_URI, EMOJI_FIXTURE)?;

    let response = harness.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": EMOJI_URI },
            "position": { "line": 1, "character": 14 }
        }),
    )?;

    assert_eq!(
        response,
        json!({
            "range": {
                "start": { "line": 1, "character": 13 },
                "end": { "line": 1, "character": 21 }
            },
            "placeholder": "$counter"
        }),
        "prepareRename on the emoji/CJK line should return the exact UTF-16 range"
    );
    Ok(())
}
