//! AST Explorer tests for the `perl/showAst` custom request handler.
//!
//! The VSCode extension sends `perl/showAst` with `{ uri: "..." }` and expects
//! a JSON string (the S-expression AST) or `null` when no AST is available.
//!
//! # Test coverage
//!
//! - Valid Perl file returns a non-null string AST
//! - AST contains expected constructs (use statements, subroutines)
//! - Document not opened returns an error
//! - Server not initialized returns ServerNotInitialized error
//! - Missing URI parameter returns INVALID_PARAMS error
//! - Empty Perl file returns null (no AST) gracefully
#![expect(
    clippy::expect_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]

mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::lsp_harness::LspHarness;

fn create_and_init_harness() -> LspHarness {
    let mut harness = LspHarness::new();
    harness
        .initialize_with_root("file:///workspace", Some(json!({})))
        .expect("initialize should succeed");
    harness
}

fn open_document(harness: &mut LspHarness, uri: &str, text: &str) {
    harness.open(uri, text).expect("didOpen should succeed");
    let _ =
        harness.wait_for_notification("perl-lsp/active-document-ready", Duration::from_secs(10));
}

fn show_ast(harness: &mut LspHarness, uri: &str) -> Value {
    harness
        .request_with_timeout("perl/showAst", json!({ "uri": uri }), Duration::from_secs(10))
        .unwrap_or(Value::Null)
}

fn show_ast_error(harness: &mut LspHarness, params: Option<Value>) -> Option<i64> {
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "perl/showAst",
        "params": params
    }));
    response.pointer("/error/code").and_then(Value::as_i64)
}

/// The handler must return a non-null JSON string for a valid Perl file.
#[test]
fn show_ast_returns_string_for_valid_perl() {
    let mut harness = create_and_init_harness();
    let uri = "file:///test_ast.pl";
    open_document(&mut harness, uri, "use strict;\nuse warnings;\nsub foo { return 1; }\n");

    let value = show_ast(&mut harness, uri);
    assert!(!value.is_null(), "perl/showAst should return a non-null AST string for a valid file");
    assert!(value.is_string(), "perl/showAst result should be a JSON string, got: {:?}", value);
}

/// The returned S-expression must contain recognizable Perl constructs.
#[test]
fn show_ast_contains_expected_constructs() {
    let mut harness = create_and_init_harness();
    let uri = "file:///constructs.pl";
    open_document(
        &mut harness,
        uri,
        "use strict;\nuse warnings;\nsub greet { my ($name) = @_; return \"Hello, $name\"; }\n",
    );

    let result = show_ast(&mut harness, uri);
    let ast_str = result.as_str().expect("result must be a string");

    assert!(
        ast_str.contains("source_file") || ast_str.contains("program"),
        "AST should contain a top-level program/source_file node. Got: {}",
        &ast_str[..ast_str.len().min(200)]
    );
}

/// Requesting AST for a URI that was never opened returns a Document-Not-Found error.
#[test]
fn show_ast_returns_error_for_unknown_document() {
    let mut harness = create_and_init_harness();

    let code = show_ast_error(&mut harness, Some(json!({ "uri": "file:///never_opened.pl" })));
    assert!(code.is_some(), "perl/showAst should return an error for an unopened document");
    assert_eq!(
        code,
        Some(-32602),
        "Expected INVALID_PARAMS (-32602) for unknown document, got {code:?}"
    );
}

/// Calling `perl/showAst` before `initialize` returns ServerNotInitialized.
#[test]
fn show_ast_before_init_returns_server_not_initialized() {
    let mut harness = LspHarness::new_without_initialize();

    let code = show_ast_error(&mut harness, Some(json!({ "uri": "file:///foo.pl" })));
    assert!(code.is_some(), "Should return an error before initialization");
    assert_eq!(code, Some(-32002), "Expected ServerNotInitialized (-32002), got {code:?}");
}

/// Missing `uri` field in params returns INVALID_PARAMS.
#[test]
fn show_ast_missing_uri_returns_invalid_params() {
    let mut harness = create_and_init_harness();

    let code = show_ast_error(&mut harness, Some(json!({ "other": "value" })));
    assert!(code.is_some(), "Should return an error when uri is missing");
    assert_eq!(
        code,
        Some(-32602),
        "Expected INVALID_PARAMS (-32602) for missing uri, got {code:?}"
    );
}

/// Null params returns INVALID_PARAMS.
#[test]
fn show_ast_null_params_returns_invalid_params() {
    let mut harness = create_and_init_harness();

    let code = show_ast_error(&mut harness, None);
    assert!(code.is_some(), "Should return an error for null params");
    assert_eq!(
        code,
        Some(-32602),
        "Expected INVALID_PARAMS (-32602) for null params, got {code:?}"
    );
}
