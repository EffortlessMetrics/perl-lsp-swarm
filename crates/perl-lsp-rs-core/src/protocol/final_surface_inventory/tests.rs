//! Coverage, determinism, and negative-control tests for the final-surface
//! inventory (#9662).
//!
//! Test assertions here intentionally use `expect`/`panic!` for invariant
//! failures; the crate-wide production bans do not apply to this test-only
//! module (same precedent as `src/platform.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use super::{
    Disposition, INVENTORY_ISSUE, SurfaceKind, SurfaceRow, census_pointer_union, coverage_errors,
    coverage_errors_with_source_check, final_surface_rows, flatten_surface_pointers,
    owned_surface_pointers, render_final_surface_inventory_json, render_with_rows,
    static_surface_census,
};
use crate::protocol::capabilities::capabilities_json;

fn artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/specs/lsp-final-surface-inventory.json")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn render_succeeds_and_is_byte_identical_across_runs() {
    let first = render_final_surface_inventory_json().expect("ledger must pass its own coverage");
    let second = render_final_surface_inventory_json().expect("second render must also pass");
    assert_eq!(first, second, "rendered inventory must be byte-identical");
    assert!(first.ends_with('\n'), "artifact must end with a single trailing newline");
    assert!(!first.contains("\r\n"), "artifact must use LF newlines");
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
#[test]
fn wasm_inventory_is_target_scoped_instead_of_matching_host_artifact() {
    let generated =
        render_final_surface_inventory_json().expect("wasm inventory must render successfully");
    assert!(
        !generated.contains("cap.executeCommandProvider.commands[]"),
        "wasm inventory must omit the host-only execute-command capability row"
    );
    assert!(
        !generated.contains("\"kind\": \"command\""),
        "wasm inventory must omit host-only command rows"
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
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "writes docs/specs/lsp-final-surface-inventory.json; explicit regeneration only (#9662)"]
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
fn negative_control_unadvertised_capability_row_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "cap.hoverProvider" {
                let mut unadvertised = row.clone();
                unadvertised.disposition = Disposition::Unadvertised;
                unadvertised
            } else {
                row
            }
        })
        .collect();
    let error = render_with_rows(&rows).expect_err("capability rows must be advertised statically");
    assert!(
        error
            .problems
            .iter()
            .any(|problem| problem.contains("malformed capability row cap.hoverProvider")
                && problem.contains("disposition must be static")),
        "unadvertised capability row must be rejected, got: {error}"
    );
}

#[test]
fn negative_control_command_kind_must_have_command_identity() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "cap.hoverProvider" {
                let mut command = row.clone();
                command.kind = SurfaceKind::Command;
                command
            } else {
                row
            }
        })
        .collect();
    let error =
        render_with_rows(&rows).expect_err("a capability pointer is not a command identity");
    assert!(
        error.problems.iter().any(|problem| {
            problem.contains("malformed command row cap.hoverProvider")
                && problem.contains("cmd.<id>")
        }),
        "command-shaped capability row must be rejected, got: {error}"
    );
}

#[test]
fn negative_control_duplicate_primary_and_additional_pointer_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "mut.handle_initialize.workspaceReplacement" {
                let mut duplicate = row.clone();
                duplicate.additional_owned_pointers = &["workspace"];
                duplicate
            } else {
                row
            }
        })
        .collect();
    let error = render_with_rows(&rows)
        .expect_err("primary and additional ownership of one pointer must be rejected");
    assert!(
        error.problems.iter().any(|problem| {
            problem.contains("duplicate builder claim for pointer workspace")
                && problem.contains("mut.handle_initialize.workspaceReplacement")
        }),
        "duplicate ownership must name the claimant row, got: {error}"
    );
}

#[test]
fn negative_control_dynamic_registration_with_wrong_disposition_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "reg.perl-didChangeWatchedFiles" {
                let mut wrong = row.clone();
                wrong.disposition = Disposition::Unadvertised;
                wrong
            } else {
                row
            }
        })
        .collect();
    let error = render_with_rows(&rows)
        .expect_err("dynamic registration rows must retain the dynamic disposition");
    assert!(
        error.problems.iter().any(|problem| {
            problem.contains("malformed registration row reg.perl-didChangeWatchedFiles")
                && problem.contains("must be dynamic")
        }),
        "wrong dynamic-registration disposition must be rejected, got: {error}"
    );
}

#[test]
fn negative_control_suppression_with_wrong_disposition_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "sup.disabledFeature.lsp.completion" {
                let mut wrong = row.clone();
                wrong.disposition = Disposition::Static;
                wrong
            } else {
                row
            }
        })
        .collect();
    let error = render_with_rows(&rows)
        .expect_err("suppression rows must retain the unadvertised disposition");
    assert!(
        error.problems.iter().any(|problem| {
            problem.contains("malformed suppression row sup.disabledFeature.lsp.completion")
                && problem.contains("must be unadvertised")
        }),
        "wrong suppression disposition must be rejected, got: {error}"
    );
}

#[test]
fn negative_control_compatibility_with_wrong_disposition_fails_render() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "compat.client.jetbrains.watcherForceDisable" {
                let mut wrong = row.clone();
                wrong.disposition = Disposition::Static;
                wrong
            } else {
                row
            }
        })
        .collect();
    let error = render_with_rows(&rows)
        .expect_err("compatibility rows must retain the unadvertised disposition");
    assert!(
        error.problems.iter().any(|problem| {
            problem
                .contains("malformed compatibility row compat.client.jetbrains.watcherForceDisable")
                && problem.contains("must be unadvertised")
        }),
        "wrong compatibility disposition must be rejected, got: {error}"
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
            !row.additional_owned_pointers.is_empty()
                || row.rewrites_surface_pointer.is_some()
                || !row.protocol_field.starts_with("(rewrite)"),
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn command_inventory_is_scoped_to_non_wasm_advertisement() {
    let rows = final_surface_rows();
    assert!(rows.iter().any(|row| {
        row.surface_id == "cap.executeCommandProvider.commands[]"
            && row.build_profile_config_tool_inputs.contains(&"target_arch != wasm32")
    }));
    assert_eq!(
        rows.iter().filter(|row| row.kind == SurfaceKind::Command).count(),
        crate::protocol::capabilities::SUPPORTED_COMMANDS.len()
    );
}

#[cfg(target_arch = "wasm32")]
#[test]
fn command_inventory_omits_wasm_advertisement() {
    let rows = final_surface_rows();
    assert!(!rows.iter().any(|row| row.kind == SurfaceKind::Command));
    assert!(!rows.iter().any(|row| row.protocol_field == "executeCommandProvider.commands[]"));
    assert!(
        capabilities_json(crate::features::flags::BuildFlags::all())
            .get("executeCommandProvider")
            .is_none()
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
fn ownership_is_bijective_with_census_and_citations_right_now() {
    let rows = final_surface_rows();
    let errors = coverage_errors_with_source_check(&rows, &repo_root());
    assert!(errors.is_empty(), "ledger/census/citation bijection broken:\n{}", errors.join("\n"));
}

#[test]
fn negative_control_stale_citation_fails_source_check() {
    let rows: Vec<SurfaceRow> = final_surface_rows()
        .into_iter()
        .map(|row| {
            if row.surface_id == "reg.perl-inlineCompletion" {
                let mut moved = row.clone();
                moved.builder_mutator_path =
                    "crates/perl-lsp-rs/src/runtime/lifecycle/deleted_watchers.rs register_inline_completion_if_needed";
                moved
            } else {
                row
            }
        })
        .collect();
    let errors = coverage_errors_with_source_check(&rows, &repo_root());
    assert!(
        errors
            .iter()
            .any(|problem| problem.contains("stale citation")
                && problem.contains("deleted_watchers.rs")),
        "moved citation must fail the source check, got: {errors:?}"
    );
}

/// Review question (c): a new `FeatureProfile` kind (or a widened existing
/// one) must fail the inventory instead of silently shipping surface no
/// census profile covers.
#[test]
fn new_profile_kinds_cannot_silently_widen_surface() {
    use crate::features::policy::{FeatureProfile, flags_for_profile};

    let census = census_pointer_union();
    for profile in FeatureProfile::all() {
        let pointers = flatten_surface_pointers(&capabilities_json(flags_for_profile(*profile)));
        let widened: Vec<&String> =
            pointers.iter().filter(|pointer| !census.contains(*pointer)).collect();
        assert!(
            widened.is_empty(),
            "profile {profile:?} widens the final surface beyond the censused profiles: {widened:?}"
        );
    }
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
