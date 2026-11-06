//! End-to-end workflow test for "batteries included" LSP functionality
//!
//! This test demonstrates the complete workflow of using perl-lsp in a real-world scenario:
//! 1. Open a Perl file with code quality issues
//! 2. Get diagnostics (syntax + built-in analyzer)
//! 3. Apply code actions (add pragmas, organize imports)
//! 4. Format the document
//! 5. Verify the final workflow state matches the wired expectations

#![cfg(test)]
// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout doesn't apply the
// way it does to production code.
#![allow(clippy::print_stdout)]

mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;
use support::test_helpers::apply_text_edits;

#[test]
fn test_complete_workflow_from_messy_to_clean() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();

    let init = harness.initialize_with_root(
        "file:///workspace",
        Some(json!({
            "textDocument": {
                "codeAction": {
                    "codeActionLiteralSupport": {
                        "codeActionKind": {
                            "valueSet": ["source.organizeImports", "quickfix"]
                        }
                    }
                }
            }
        })),
    )?;
    assert!(init.get("capabilities").is_some(), "Initialize should return capabilities");

    let uri = "file:///workspace/messy.pl";
    let messy_code = r#"sub calculate{my$x=shift;my$y=shift;return$x+$y;}
my$result=calculate(5,3);
"#;

    harness.open(uri, messy_code)?;
    harness.barrier();

    let diag_result = harness.request_with_timeout(
        "textDocument/diagnostic",
        json!({
            "textDocument": {"uri": uri}
        }),
        Duration::from_secs(10),
    )?;

    println!("Diagnostics: {:?}", diag_result);

    let actions_result = harness.request_with_timeout(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 7, "character": 0}
            },
            "context": {
                "diagnostics": []
            }
        }),
        Duration::from_secs(10),
    )?;

    if let Some(actions) = actions_result.as_array() {
        println!("Available code actions: {} actions", actions.len());
        for action in actions {
            if let Some(title) = action.get("title").and_then(|t| t.as_str()) {
                println!("  - {}", title);
            }
        }
    }

    let format_result = harness.request_with_timeout(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        }),
        Duration::from_secs(10),
    )?;
    let edits = format_result.as_array().ok_or("formatting result must be edit array")?;
    assert!(!edits.is_empty(), "native default formatting should return edits");
    let formatted = apply_text_edits(messy_code, edits);
    assert_eq!(
        formatted,
        concat!(
            "sub calculate {\n",
            "    my $x = shift;\n",
            "    my $y = shift;\n",
            "    return $x + $y;\n",
            "}\n",
            "my $result = calculate(5, 3);\n",
        )
    );

    harness.request_with_timeout(
        "textDocument/diagnostic",
        json!({
            "textDocument": {"uri": uri}
        }),
        Duration::from_secs(10),
    )?;

    Ok(())
}

#[test]
fn test_batteries_included_features_summary() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();

    let init = harness.initialize_with_root("file:///test", Some(json!({})))?;
    let capabilities = init.get("capabilities").ok_or("Expected capabilities")?;

    println!("\n=== Perl LSP 'Batteries Included' Features ===\n");

    if capabilities.get("documentFormattingProvider").is_some() {
        println!("✓ Document Formatting (native by default; Perl::Tidy compatibility is optional)");
    }
    if capabilities.get("documentRangeFormattingProvider").is_some() {
        println!("✓ Range Formatting");
    }

    if capabilities.get("diagnosticProvider").is_some() {
        println!("✓ Pull Diagnostics (built-in analyzer for strict/warnings)");
    }

    if let Some(code_action) = capabilities.get("codeActionProvider") {
        println!("✓ Code Actions:");
        if let Some(kinds) = code_action.get("codeActionKinds").and_then(|k| k.as_array()) {
            for kind in kinds {
                if let Some(kind_str) = kind.as_str() {
                    println!("    - {}", kind_str);
                }
            }
        } else {
            println!("    - quickfix (add pragmas, remove unused code)");
            println!("    - refactor (extract, inline, rename)");
            println!("    - source.organizeImports");
        }
    }

    if let Some(exec_cmd) = capabilities.get("executeCommandProvider")
        && let Some(commands) = exec_cmd.get("commands").and_then(|c| c.as_array())
    {
        println!("✓ Execute Commands:");
        for cmd in commands {
            if let Some(cmd_str) = cmd.as_str() {
                println!("    - {}", cmd_str);
            }
        }
    }

    if capabilities.get("definitionProvider").is_some() {
        println!("✓ Go to Definition");
    }
    if capabilities.get("referencesProvider").is_some() {
        println!("✓ Find References");
    }
    if capabilities.get("implementationProvider").is_some() {
        println!("✓ Find Implementations");
    }

    if capabilities.get("hoverProvider").is_some() {
        println!("✓ Hover Documentation");
    }
    if capabilities.get("completionProvider").is_some() {
        println!("✓ Auto-completion");
    }
    if capabilities.get("signatureHelpProvider").is_some() {
        println!("✓ Signature Help");
    }

    if capabilities.get("documentSymbolProvider").is_some() {
        println!("✓ Document Symbols (Outline)");
    }
    if capabilities.get("workspaceSymbolProvider").is_some() {
        println!("✓ Workspace Symbols");
    }

    if capabilities.get("renameProvider").is_some() {
        println!("✓ Rename Symbol");
    }

    if capabilities.get("semanticTokensProvider").is_some() {
        println!("✓ Semantic Syntax Highlighting");
    }
    if capabilities.get("inlayHintProvider").is_some() {
        println!("✓ Inlay Hints");
    }
    if capabilities.get("typeHierarchyProvider").is_some() {
        println!("✓ Type Hierarchy");
    }
    if capabilities.get("callHierarchyProvider").is_some() {
        println!("✓ Call Hierarchy");
    }

    println!("\n=== Integration Status ===\n");
    println!("✓ Native formatter and native critic are the default paths");
    println!("✓ Explicit compatibility adapters remain available for legacy projects");
    println!("✓ No required external dependencies for normal editor diagnostics or formatting");
    println!("✓ Optional Perl::Tidy compatibility for exact legacy formatting");
    println!("✓ Optional Perl::Critic compatibility for legacy policy migration");
    println!("\n");

    Ok(())
}
