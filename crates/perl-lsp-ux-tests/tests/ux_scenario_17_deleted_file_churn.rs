#![expect(
    clippy::print_stderr,
    reason = "Scenario 17 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 17 — deleting a watched file evicts stale symbols and definition targets.
//!
//! Verifies that a real `workspace/didChangeWatchedFiles` Deleted event removes
//! stale search results and cross-file definitions from the UX surface.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::Value;
use serde_json::json;
use std::time::{Duration, Instant};

const MODULE_SOURCE: &str = "\
package ModuleGone;\n\
\n\
sub gone_value_4068 {\n\
    return 42;\n\
}\n\
\n\
1;\n\
";

const SCRIPT_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
use lib 'lib';\n\
use ModuleGone;\n\
\n\
my $value = ModuleGone::gone_value_4068();\n\
print \"$value\\n\";\n\
";

fn symbol_names(symbols: &[Value]) -> Vec<&str> {
    symbols.iter().filter_map(|symbol| symbol["name"].as_str()).collect()
}

fn source_reconciliation_scenario_17_deleted_module_evicted_from_symbols_and_definition()
-> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_17: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(20), ..Default::default() }
            .env("PERL_LSP_WORKSPACE", "1")
            .with_file("main.pl", SCRIPT_SOURCE)
            .with_file("lib/ModuleGone.pm", MODULE_SOURCE),
    )
    .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("main.pl", SCRIPT_SOURCE)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;
    let ready = harness.wait_for_index_ready(Duration::from_secs(20));
    assert!(
        ready.is_ok(),
        "Expected workspace index to become ready before querying ModuleGone: {ready:?}"
    );

    let cursor = harness.position_cursor("main.pl", 5, 25);
    let before_deadline = Instant::now() + Duration::from_secs(10);
    let mut symbols_before = Vec::new();
    let mut defs_before = Vec::new();
    while Instant::now() < before_deadline {
        symbols_before = harness
            .workspace_symbols("gone_value_4068")
            .map_err(|error| format!("workspace/symbol failed before delete: {error}"))?;
        defs_before = harness
            .definition_at(&cursor)
            .map_err(|error| format!("definition failed before delete: {error}"))?;

        if !symbols_before.is_empty() && !defs_before.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    assert!(
        !symbols_before.is_empty(),
        "Expected gone_value_4068 to be searchable before delete, got names {:?}",
        symbol_names(&symbols_before)
    );
    assert!(
        !defs_before.is_empty(),
        "Expected definition to resolve before delete, got {:?}",
        defs_before
    );
    harness.assert_normalized_eq(
        &defs_before[0],
        &json!({
            "uri": "file://$WORKSPACE/lib/ModuleGone.pm",
            "range": defs_before[0]["range"].clone(),
        }),
    );

    harness
        .workspace
        .delete("lib/ModuleGone.pm")
        .map_err(|error| format!("module delete failed: {error}"))?;
    harness
        .notify_watched_files(&[("lib/ModuleGone.pm", 3)])
        .map_err(|error| format!("didChangeWatchedFiles Deleted notification failed: {error}"))?;

    let after_deadline = Instant::now() + Duration::from_secs(10);
    let mut symbols_after = Vec::new();
    let mut defs_after = Vec::new();
    while Instant::now() < after_deadline {
        symbols_after = harness
            .workspace_symbols("gone_value_4068")
            .map_err(|error| format!("workspace/symbol failed after delete: {error}"))?;
        defs_after = harness
            .definition_at(&cursor)
            .map_err(|error| format!("definition failed after delete: {error}"))?;

        if symbols_after.is_empty() && defs_after.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    assert!(
        symbols_after.is_empty(),
        "Expected deleted symbol gone_value_4068 to disappear from workspace/symbol, got names {:?}",
        symbol_names(&symbols_after)
    );
    assert!(
        defs_after.is_empty(),
        "Expected deleted module definition target to disappear after delete, got {:?}",
        defs_after
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_17_deleted_module_evicted_from_symbols_and_definition() {
    use perl_lsp_ux_tests::{
        UxCiTier, UxComponent, UxEvidenceClass, missing_binary_skip,
        run_ux_scenario_with_evidence_class,
    };
    run_ux_scenario_with_evidence_class(
        "deleted_file_churn_freshness",
        "ux_scenario_17_deleted_file_churn.rs",
        "scenario_17_deleted_module_evicted_from_symbols_and_definition",
        UxCiTier::Pr,
        Some(UxComponent::WorkspaceSymbols),
        UxEvidenceClass::SemanticProof,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            // Preserve source assertions; their helper does not expose an exact timing boundary.
            source_reconciliation_scenario_17_deleted_module_evicted_from_symbols_and_definition()
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            recorder.check("current source assertions for scenario_17_deleted_module_evicted_from_symbols_and_definition completed successfully", true)?;
            // Aggregate completion records no request/first-useful timing boundary.
            Ok(())
        },
    );
}
