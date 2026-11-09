//! LSP 3.17 Semantic Tokens Contract Tests
//!
//! Tests for textDocument/semanticTokens/full and textDocument/semanticTokens/range.

mod support;

use serde_json::json;
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Keep workspace discovery confined to this fixture and finish canonical
/// initialization before queuing document requests. Return the workspace first
/// so callers drop the harness before removing its files.
fn open_semantic_fixture(
    source: &str,
) -> Result<(TempWorkspace, LspHarness, String), Box<dyn std::error::Error>> {
    let (mut harness, workspace) = LspHarness::with_workspace(&[("tokens.pl", source)])?;
    let uri = workspace.uri("tokens.pl");
    harness.open(&uri, source)?;
    Ok((workspace, harness, uri))
}

// ==================== SEMANTIC TOKENS (3.16+) ====================

#[test]
fn test_semantic_tokens_full_3_17() -> TestResult {
    let (_workspace, mut harness, uri) =
        open_semantic_fixture("package Foo;\nsub bar { my $var = 1; }")?;

    let response = harness.request(
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": { "uri": uri }
        }),
    )?;

    let data = response["data"].as_array().ok_or("full response missing token data")?;
    assert!(!data.is_empty(), "the open fixture must produce full tokens");
    Ok(())
}

#[test]
fn test_semantic_tokens_range_3_17() -> TestResult {
    let (_workspace, mut harness, uri) = open_semantic_fixture("my $x = 1;\nmy $y = 2;")?;

    let response = harness.request(
        "textDocument/semanticTokens/range",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 10 }
            }
        }),
    )?;

    let data = response["data"].as_array().ok_or("range response missing token data")?;
    assert!(!data.is_empty(), "the requested fixture range must contain tokens");
    Ok(())
}

// ==================== SEMANTIC TOKENS DELTA (3.17) ====================

/// The capability advertisement must announce delta support so clients send
/// `textDocument/semanticTokens/full/delta`.
#[test]
fn test_semantic_tokens_advertises_delta_3_17() -> TestResult {
    let workspace = TempWorkspace::new()?;
    let mut harness = LspHarness::new_raw();
    let init = harness.initialize_ready(&workspace.root_uri, None)?;

    let full = &init["capabilities"]["semanticTokensProvider"]["full"];
    assert_eq!(
        full["delta"],
        json!(true),
        "server must advertise semanticTokensProvider.full.delta = true; got {full:?}"
    );
    Ok(())
}

/// The full request must return a `resultId` so a later delta request can refer
/// to it.
#[test]
fn test_semantic_tokens_full_returns_result_id_3_17() -> TestResult {
    let (_workspace, mut harness, uri) =
        open_semantic_fixture("package Foo;\nsub bar { my $var = 1; }")?;

    let full = harness
        .request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": uri } }))?;

    assert!(full["data"].is_array(), "full result must carry token data");
    assert!(full["resultId"].as_str().is_some(), "full result must carry a resultId; got {full:?}");
    Ok(())
}

/// A delta request against an unchanged document returns an empty edit list.
#[test]
fn test_semantic_tokens_delta_unchanged_is_empty_3_17() -> TestResult {
    let (_workspace, mut harness, uri) =
        open_semantic_fixture("package Foo;\nsub bar { my $var = 1; }")?;

    let full = harness
        .request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": uri } }))?;
    let result_id = full["resultId"].as_str().ok_or("full result missing resultId")?;

    let delta = harness.request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": uri },
            "previousResultId": result_id
        }),
    )?;

    let edits = delta["edits"].as_array().ok_or("delta response missing edits array")?;
    assert!(edits.is_empty(), "unchanged document must yield no edits; got {edits:?}");
    assert!(delta["resultId"].as_str().is_some(), "delta response must carry a fresh resultId");
    Ok(())
}

/// A delta request after an edit returns a non-empty edit list describing the
/// change.
#[test]
fn test_semantic_tokens_delta_after_change_has_edits_3_17() -> TestResult {
    let (_workspace, mut harness, uri) =
        open_semantic_fixture("package Foo;\nsub bar { my $var = 1; }")?;

    let full = harness
        .request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": uri } }))?;
    let result_id = full["resultId"].as_str().ok_or("full result missing resultId")?.to_string();

    // Edit the document so the token stream changes.
    harness.change_full(&uri, 2, "package Foo;\nsub bar { my $var = 1; my $other = 2; }")?;

    let delta = harness.request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": uri },
            "previousResultId": result_id
        }),
    )?;

    let edits = delta["edits"].as_array().ok_or("delta response missing edits array")?;
    assert!(!edits.is_empty(), "a changed document must yield at least one edit");
    let edit = &edits[0];
    assert!(edit["start"].is_number(), "edit must carry a start offset");
    assert!(edit["deleteCount"].is_number(), "edit must carry a deleteCount");
    assert!(edit["data"].is_array(), "edit must carry replacement data");
    Ok(())
}

/// An unknown `previousResultId` falls back to a full token response so the
/// client can resynchronize.
#[test]
fn test_semantic_tokens_delta_unknown_id_returns_full_3_17() -> TestResult {
    let (_workspace, mut harness, uri) =
        open_semantic_fixture("package Foo;\nsub bar { my $var = 1; }")?;

    // Prime the cache with a full request first.
    harness
        .request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": uri } }))?;

    let delta = harness.request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": uri },
            "previousResultId": "does-not-exist"
        }),
    )?;

    assert!(
        delta["data"].is_array(),
        "unknown previousResultId must return a full token set; got {delta:?}"
    );
    assert!(delta["resultId"].as_str().is_some(), "full fallback must carry a fresh resultId");
    Ok(())
}
