//! Coverage, determinism, and negative-control tests for the final-surface
//! inventory (#9662).
//!
//! Test assertions here intentionally use `expect`/`panic!` for invariant
//! failures; the crate-wide production bans do not apply to this test-only
//! module (same precedent as `src/platform.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use super::{
    INVENTORY_ISSUE, SurfaceKind, SurfaceRow, coverage_errors, final_surface_rows,
    flatten_surface_pointers, owned_surface_pointers, render_final_surface_inventory_json,
    render_with_rows, static_surface_census,
};

fn artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/specs/lsp-final-surface-inventory.json")
}

#[test]
fn render_succeeds_and_is_byte_identical_across_runs() {
    let first = render_final_surface_inventory_json().expect("ledger must pass its own coverage");
    let second = render_final_surface_inventory_json().expect("second render must also pass");
    assert_eq!(first, second, "rendered inventory must be byte-identical");
    assert!(first.ends_with('\n'), "artifact must end with a single trailing newline");
    assert!(!first.contains("\r\n"), "artifact must use LF newlines");
}

#[test]
fn checked_in_artifact_matches_generated_output() {
    let path = artifact_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing or unreadable {} ({error}); run `cargo test -p perl-lsp-rs-core --lib \
             final_surface_inventory::tests::regenerate_checked_in_artifact --locked -- --ignored` \
             to (re)generate it",
            path.display()
        )
    });
    let generated =
        render_final_surface_inventory_json().expect("ledger must pass its own coverage");
    assert!(
        existing.replace("\r\n", "\n") == generated,
        "{} is stale; regenerate via `cargo test -p perl-lsp-rs-core --lib \
         final_surface_inventory::tests::regenerate_checked_in_artifact --locked -- --ignored`",
        path.display()
    );
}

/// Regeneration entry point, kept next to the census it runs.
///
/// Opt-in (`--ignored`) so ordinary test runs only *check* staleness. This
/// mirrors the repo's xtask generator convention; it lives in the owning
/// crate because current-main `xtask` does not compile on Windows stable
/// (pre-existing unstable `windows_by_handle` usage in
/// `generate_semantic_snapshot.rs`), and this way one render API owns both
/// check and write paths.
#[test]
#[ignore = "writes docs/specs/lsp-final-surface-inventory.json; explicit regeneration only"]
fn regenerate_checked_in_artifact() {
    let path = artifact_path();
    let generated =
        render_final_surface_inventory_json().expect("ledger must pass its own coverage");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    }
    std::fs::write(&path, &generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

#[test]
fn negative_control_unregistered_pointer_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .filter(|row| {
            !(row.kind == SurfaceKind::CapabilityField && row.protocol_field == "hoverProvider")
        })
        .collect();
    let error = render_with_rows(&rows).expect_err("dropping an owner row must fail the check");
    assert!(
        error.problems.iter().any(|problem| problem.contains("unmapped census pointer")
            && problem.contains("hoverProvider")),
        "error must name the unregistered pointer, got: {error}"
    );
}

#[test]
fn negative_control_duplicate_surface_id_fails_render() {
    let mut rows = final_surface_rows();
    let duplicate = rows
        .iter()
        .find(|row| row.kind == SurfaceKind::CapabilityField)
        .expect("capability rows exist")
        .clone();
    rows.push(duplicate);
    let error = render_with_rows(&rows).expect_err("duplicate surface_id must fail the check");
    assert!(
        error.problems.iter().any(|problem| problem.contains("duplicate surface_id")),
        "error must report the duplicate id, got: {error}"
    );
}

#[test]
fn negative_control_stale_capability_row_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.kind == SurfaceKind::CapabilityField && row.protocol_field == "monikerProvider" {
                let mut stale = row.clone();
                stale.protocol_field = "bogusProvider";
                stale
            } else {
                row
            }
        })
        .collect();
    let error =
        render_with_rows(&rows).expect_err("renaming a pointer without census backing must fail");
    assert!(
        error.problems.iter().any(|problem| problem.contains("stale capability row")),
        "error must flag the stale row, got: {error}"
    );
}

#[test]
fn negative_control_hidden_mutation_must_be_ledgered() {
    // The runtime mutation surface is discriminated end-to-end by the
    // perl-lsp-rs final-surface census tests; here we guarantee the ledger
    // structurally represents the exact initialize-time override sites so
    // runtime diffs always have a row to land on.
    let rows = final_surface_rows();
    for surface_id in [
        "mut.handle_initialize.textDocumentSyncOverride",
        "mut.handle_initialize.positionEncodingPin",
        "mut.handle_initialize.workspaceReplacement",
        "mut.handle_initialize.fileOperationsIntersection",
        "mut.handle_initialize.codeActionDocumentationInsert",
        "mut.handle_initialize.experimentalPerlInlineCompletionStreamMerge",
        "mut.handle_initialize.declarationProviderRewrite",
        "mut.handle_initialize.inlineCompletionTriState",
    ] {
        let row = rows
            .iter()
            .find(|row| row.surface_id == surface_id)
            .unwrap_or_else(|| panic!("mutation row {surface_id} missing from ledger"));
        assert!(
            !row.additional_owned_pointers.is_empty() || row.rewrites_surface_pointer.is_some(),
            "mutation row {surface_id} owns no pointers"
        );
    }
}

#[test]
fn command_rows_are_parity_with_supported_commands() {
    let rows = final_surface_rows();
    let errors = coverage_errors(&rows);
    let command_errors: Vec<_> = errors
        .into_iter()
        .filter(|problem| problem.contains("command") || problem.contains("cmd."))
        .collect();
    assert!(
        command_errors.is_empty(),
        "execute-command identities drifted from SUPPORTED_COMMANDS: {command_errors:?}"
    );
}

#[test]
fn suppression_rows_mirror_apply_disabled_feature_id_arms() {
    const EXPECTED_IDS: &[&str] = &[
        "lsp.completion",
        "lsp.hover",
        "lsp.definition",
        "lsp.declaration",
        "lsp.references",
        "lsp.document_symbol",
        "lsp.workspace_symbol",
        "lsp.code_action",
        "lsp.code_lens",
        "lsp.rename",
        "lsp.folding_range",
        "lsp.selection_range",
        "lsp.linked_editing_range",
        "lsp.inlay_hint",
        "lsp.semantic_tokens",
        "lsp.call_hierarchy",
        "lsp.type_hierarchy",
        "lsp.pull_diagnostics",
        "lsp.document_color",
        "lsp.signature_help",
        "lsp.document_highlight",
        "lsp.formatting",
        "lsp.range_formatting",
        "lsp.ranges_formatting",
        "lsp.on_type_formatting",
        "lsp.document_link",
        "lsp.inline_completion",
        "lsp.inline_value",
        "lsp.notebook_document_sync",
        "lsp.notebook_cell_execution",
        "lsp.implementation",
        "lsp.type_definition",
        "lsp.execute_command",
        "lsp.moniker",
    ];
    let ledgered: Vec<String> = final_surface_rows()
        .iter()
        .filter(|row| row.surface_id.starts_with("sup.disabledFeature."))
        .map(|row| {
            row.protocol_field
                .strip_prefix("initializationOptions.disabledFeatures:")
                .unwrap_or(row.protocol_field)
                .to_string()
        })
        .collect();
    let mut sorted_ledger = ledgered.clone();
    sorted_ledger.sort();
    let mut sorted_expected: Vec<&str> = EXPECTED_IDS.to_vec();
    sorted_expected.sort();
    assert_eq!(
        sorted_ledger, sorted_expected,
        "suppression ledger diverged from apply_disabled_feature_id arms"
    );
    assert_eq!(ledgered.len(), 34, "expected exactly 34 disabled-feature arms");
}

#[test]
fn profile_snapshots_differ_where_profiles_differ() {
    let census = static_surface_census();
    let all = census.get("all").expect("all profile censused");
    let production = census.get("production").expect("production profile censused");
    let ga_lock = census.get("ga-lock").expect("ga-lock profile censused");

    assert!(
        all.iter().any(|pointer| pointer.starts_with("notebookDocumentSync.")),
        "all profile must expose preview notebook sync"
    );
    assert!(
        !production.iter().any(|pointer| pointer.starts_with("notebookDocumentSync.")),
        "production must not expose notebook sync"
    );
    assert!(
        production.iter().any(|pointer| *pointer == "inlineValueProvider"),
        "production advertises inline values"
    );
    assert!(
        !ga_lock.iter().any(|pointer| *pointer == "inlineValueProvider"),
        "ga-lock suppresses inline values"
    );
}

#[test]
fn competing_builder_diff_preserves_known_dual_writers() {
    let rendered = render_final_surface_inventory_json().expect("render must succeed");
    for dual_writer in [
        "cap.workspaceSymbolProvider.resolveProvider",
        "cap.experimental.typeHierarchyProvider",
        "cap.typeHierarchyProvider.workDoneProgressOptions",
        "cap.documentRangeFormattingProvider.rangesSupport",
        "cap.textDocumentSync.save",
        "cap.declarationProvider",
        "cap.inlineCompletionProvider",
    ] {
        assert!(
            rendered.contains(dual_writer),
            "competing builder diff must preserve {dual_writer}"
        );
    }
}

#[test]
fn ownership_is_bijective_with_census_right_now() {
    let rows = final_surface_rows();
    let errors = coverage_errors(&rows);
    assert!(errors.is_empty(), "ledger/census bijection broken:\n{}", errors.join("\n"));
}

#[test]
fn flattener_is_deterministic_and_presence_aware() {
    let value = serde_json::json!({
        "a": {"b": true, "c": {}, "d": [1, 2], "e": [{"f": "x"}]}
    });
    let pointers = flatten_surface_pointers(&value);
    let expected: Vec<&str> = vec!["a.b", "a.c", "a.d[]", "a.e[]", "a.e[].f"];
    let collected: Vec<String> = pointers.into_iter().collect();
    assert_eq!(collected, expected);
    let owned = owned_surface_pointers(&final_surface_rows());
    assert!(!owned.is_empty());
    assert_eq!(INVENTORY_ISSUE, "#9662");
}
