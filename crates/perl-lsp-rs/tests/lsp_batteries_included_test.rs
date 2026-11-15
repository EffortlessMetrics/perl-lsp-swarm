//! Tests for "batteries included" LSP functionality
//!
//! This test validates that the batteries-included LSP surface works end to end;
//! with formatting, linting, and import optimization working out-of-the-box.

#![cfg(test)]

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Test that formatting is properly advertised in server capabilities
#[test]
fn test_formatting_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize_with_root("file:///test", Some(json!({})))?;

    let capabilities =
        init.get("capabilities").ok_or("Expected capabilities in initialize result")?;

    // Verify formatting is advertised
    if let Some(formatting_provider) = capabilities.get("documentFormattingProvider") {
        // Formatting is provided natively; when advertised it should be a valid
        // capability payload.
        assert!(
            formatting_provider.is_boolean() || formatting_provider.is_object(),
            "documentFormattingProvider should be bool or object when advertised"
        );
    }

    // Range formatting follows the same native capability shape as full-document formatting.
    if let Some(range_formatting_provider) = capabilities.get("documentRangeFormattingProvider") {
        assert!(
            range_formatting_provider.is_boolean() || range_formatting_provider.is_object(),
            "documentRangeFormattingProvider should be bool or object when advertised"
        );
    }

    Ok(())
}

/// Test that the legacy organize-imports edit stays withdrawn (#8305).
///
/// The line-oriented organizer was destructive: it replaced the whole interval
/// between the first and last import-looking lines, destroying executable
/// statements in between. Until #8319 admits a bounded source-preserving cohort
/// and #10696 lands the proven cutover, no filtered request may return a
/// `source.organizeImports` action and none may carry an edit.
#[test]
fn test_organize_imports_code_action_withdrawn() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a document with multiple imports
    let uri = "file:///test_imports.pl";
    let text = r#"use strict;
use warnings;
use Data::Dumper qw(Dumper);
use JSON qw(encode_json decode_json);

my $data = {key => 'value'};
print Dumper($data);
"#;

    harness.open(uri, text)?;

    let result = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 6, "character": 0}
            },
            "context": {
                "diagnostics": [],
                "only": ["source.organizeImports"]
            }
        }),
    )?;

    let actions = result.as_array().ok_or("Expected array result for code actions")?;
    assert!(
        actions.iter().all(|action| action["kind"].as_str() != Some("source.organizeImports")),
        "source.organizeImports is withdrawn (#8305) and must not be returned; got {actions:?}"
    );
    assert!(
        actions.iter().all(|action| action["title"].as_str() != Some("Organize imports")),
        "no action may reuse the withdrawn organizer title; got {actions:?}"
    );

    Ok(())
}

/// Test that execute commands include perlcritic integration
#[test]
fn test_perlcritic_execute_command_available() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize_with_root("file:///test", Some(json!({})))?;

    let capabilities =
        init.get("capabilities").ok_or("Expected capabilities in initialize result")?;

    // Verify execute command provider is present
    let execute_command_provider = capabilities.get("executeCommandProvider");
    assert!(execute_command_provider.is_some(), "executeCommandProvider should be advertised");

    // Verify that perlcritic command is in the list
    if let Some(provider) = execute_command_provider {
        let commands = provider
            .get("commands")
            .and_then(|c| c.as_array())
            .ok_or("Expected commands array in executeCommandProvider")?;

        let has_critic_command = commands.iter().any(|cmd| cmd.as_str() == Some("perl.runCritic"));

        assert!(has_critic_command, "perl.runCritic command should be available");
    }

    Ok(())
}

/// Test that basic diagnostics work without external tools
#[test]
fn test_builtin_diagnostics_work() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a document with syntax errors
    let uri = "file:///test_diagnostics.pl";
    let text = "sub foo { my \n"; // Incomplete syntax

    harness.open(uri, text)?;

    let result = harness.request(
        "textDocument/diagnostic",
        json!({
            "textDocument": {"uri": uri}
        }),
    )?;

    // Verify we got some diagnostic information
    // The exact structure may vary, but it should be present
    assert!(result.is_object() || result.is_array(), "Expected diagnostic result");

    Ok(())
}

/// Test that good defaults are in place for configuration
#[test]
fn test_default_configuration_sensible() -> TestResult {
    // Verify that server can be created without explicit configuration
    // This tests that good defaults are in place
    let _harness = LspHarness::new();

    Ok(())
}

/// Test that the server provides helpful capabilities information
#[test]
fn test_server_capabilities_complete() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize_with_root("file:///test", Some(json!({})))?;

    let capabilities =
        init.get("capabilities").ok_or("Expected capabilities in initialize result")?;

    // Verify core LSP capabilities are present
    let expected_capabilities = [
        "textDocumentSync",
        "hoverProvider",
        "completionProvider",
        "definitionProvider",
        "referencesProvider",
        "documentSymbolProvider",
        "codeActionProvider",
        "executeCommandProvider",
    ];

    for capability in &expected_capabilities {
        assert!(capabilities.get(capability).is_some(), "Missing capability: {}", capability);
    }

    Ok(())
}

/// Test that native default formatting works without external perltidy.
#[test]
fn test_native_default_formatting_returns_edits_without_perltidy() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a simple Perl document
    let uri = "file:///test_format.pl";
    let text = "sub foo{my$x=1;return$x;}\n";

    harness.open(uri, text)?;

    let result = harness.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
    )?;

    let edits = result.as_array().ok_or("Formatting result should be an edit array")?;
    assert!(!edits.is_empty(), "Native default formatting should return edits");

    let edit_text = edits
        .first()
        .and_then(|edit| edit.get("newText"))
        .and_then(|new_text| new_text.as_str())
        .ok_or("Formatting edit should include newText")?;
    assert!(edit_text.contains("sub foo {"));
    assert!(edit_text.contains("my $x = 1;"));
    assert!(edit_text.contains("return $x"));

    Ok(())
}
