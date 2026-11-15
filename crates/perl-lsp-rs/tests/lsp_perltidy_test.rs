#![cfg(test)]

mod support;

use support::lsp_harness::LspHarness;

#[cfg(feature = "lsp-extras")]
use serde_json::json;

/// Test that pragma code actions are offered
/// NOTE: Feature incomplete - workspace_roots() returns empty, so code actions don't include pragma suggestions
#[cfg(feature = "lsp-extras")]
#[test]
fn test_pragma_code_actions() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    harness.initialize_default()?;

    // Open a document without pragmas
    let uri = "file:///test.pl";
    let text = "sub foo{my$x=1;return$x}";
    harness.open(uri, text)?;

    let actions = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            },
            "context": {"diagnostics": []}
        }),
    )?;

    let actions = actions.as_array().ok_or("Expected array of actions in code action result")?;

    // Look for pragma actions
    let has_strict_action = actions.iter().any(|a| a["title"].as_str() == Some("Add use strict;"));
    let has_warnings_action =
        actions.iter().any(|a| a["title"].as_str() == Some("Add use warnings;"));

    assert!(has_strict_action, "Expected 'Add use strict;' action");
    assert!(has_warnings_action, "Expected 'Add use warnings;' action");

    Ok(())
}

/// Test that formatting provider is advertised through the native formatter.
#[test]
fn test_formatting_provider_capability() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    let result = harness.initialize_default()?;

    let has_formatting =
        result["capabilities"]["documentFormattingProvider"].as_bool().unwrap_or(false);

    assert!(has_formatting, "documentFormattingProvider should be advertised natively");

    Ok(())
}
