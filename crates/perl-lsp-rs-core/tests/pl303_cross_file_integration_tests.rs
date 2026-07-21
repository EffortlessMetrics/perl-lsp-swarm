//! Integration tests for PL303 cross-file role-conflict detection.
//!
//! Exercises the full production diagnostics path:
//!
//!   WorkspaceIndex::index_file  (multiple files)
//!   → WorkspaceIndex::with_semantic_queries_for_uri
//!       → WorkspaceSemanticQueries::with_package_graph
//!           → DiagnosticsProvider::get_diagnostics_with_search_context_and_semantics
//!               → check_role_conflicts / transitive_role_methods
//!                   → PL303 diagnostic emitted (or not)
//!
//! These tests close the "Reachable" axis for PL303: they assert that the
//! feature fires end-to-end through the live server path, not only in
//! unit tests that supply a stub resolver.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ── helper ──────────────────────────────────────────────────────────────────

/// Index `role_files` and `class_source` into a fresh `WorkspaceIndex`, then
/// run the production diagnostics path on `class_uri` and return every PL303
/// diagnostic emitted.
///
/// This is the minimal reproduction of what the live server does on every
/// `textDocument/publishDiagnostics` push for a file that consumes roles
/// defined in other workspace files.
fn pl303_cross_file(
    role_files: &[(&str, &str)],
    class_uri: &str,
    class_source: &str,
) -> Result<Vec<Diagnostic>> {
    let index = WorkspaceIndex::new();

    for (uri, source) in role_files {
        let url = Url::parse(uri)?;
        index
            .index_file(url, source.to_string())
            .map_err(|e| format!("index_file failed for {uri}: {e}"))?;
    }

    let class_url = Url::parse(class_uri)?;
    index
        .index_file(class_url, class_source.to_string())
        .map_err(|e| format!("index_file failed for {class_uri}: {e}"))?;

    let parse_output = perl_parser::Parser::new(class_source).parse_with_recovery();
    let ast = Arc::new(parse_output.ast);
    let provider = DiagnosticsProvider::new(&ast, class_source.to_string());

    let all_diags = index
        .with_semantic_queries_for_uri(class_uri, |file_id, queries| {
            provider.get_diagnostics_with_search_context_and_semantics(
                &ast,
                &parse_output.diagnostics,
                class_source,
                None,
                &[],
                None,
                file_id,
                &queries,
            )
        })
        .ok_or("no semantic queries available for class URI — URI not indexed")?;

    Ok(all_diags.into_iter().filter(|d| d.code.as_deref() == Some("PL303")).collect())
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Two roles defined in separate files both provide `shared_method`.
/// The consuming class is in a third file.  PL303 must fire once, naming the
/// conflicting method.
#[test]
fn cross_file_direct_conflict_fires_pl303() -> Result<()> {
    let role_a = "package RoleA;\nuse Role::Tiny;\nsub shared_method { 'A' }\n1;\n";
    let role_b = "package RoleB;\nuse Role::Tiny;\nsub shared_method { 'B' }\n1;\n";
    let class = "package MyConsumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let diags = pl303_cross_file(
        &[("file:///lib/RoleA.pm", role_a), ("file:///lib/RoleB.pm", role_b)],
        "file:///lib/MyConsumer.pm",
        class,
    )?;

    assert_eq!(
        diags.len(),
        1,
        "expected exactly one PL303 for direct cross-file conflict — got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("shared_method"),
        "PL303 message must name the conflicting method, got: {}",
        diags[0].message,
    );
    Ok(())
}

/// A transitive role-composition conflict: `RoleA` directly provides
/// `shared_method`; `RoleB` does not, but it composes `BaseRole` which
/// provides `shared_method`.  `MyConsumer` uses both `RoleA` and `RoleB`.
///
/// This test requires that `ComposesRole` edges appear in
/// `semantic_package_graph_index` (populated by
/// `role_composition_edges_from_ast`) so that
/// `transitive_role_methods("RoleB")` traverses the `RoleB → BaseRole`
/// edge and surfaces `shared_method` from `BaseRole`.
#[test]
fn cross_file_transitive_conflict_fires_pl303() -> Result<()> {
    let base_role = "package BaseRole;\nuse Role::Tiny;\nsub shared_method { 'base' }\n1;\n";
    let role_a = "package RoleA;\nuse Role::Tiny;\nsub shared_method { 'A' }\n1;\n";
    let role_b = "package RoleB;\nuse Role::Tiny;\nwith 'BaseRole';\n1;\n";
    let class = "package MyConsumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let diags = pl303_cross_file(
        &[
            ("file:///lib/BaseRole.pm", base_role),
            ("file:///lib/RoleA.pm", role_a),
            ("file:///lib/RoleB.pm", role_b),
        ],
        "file:///lib/MyConsumer.pm",
        class,
    )?;

    assert_eq!(diags.len(), 1, "expected one PL303 for transitive role conflict — got: {diags:?}");
    assert!(
        diags[0].message.contains("shared_method"),
        "PL303 message must name the conflicting method, got: {}",
        diags[0].message,
    );
    Ok(())
}

/// Diamond composition: `RoleA` and `RoleB` both compose `BaseRole`, which
/// is the sole provider of `the_method`.  Both consumers resolve the method
/// to the same origin (`BaseRole`), so it is NOT a conflict.  PL303 must
/// not fire.
#[test]
fn diamond_composition_does_not_fire_pl303() -> Result<()> {
    let base_role = "package BaseRole;\nuse Role::Tiny;\nsub the_method { 1 }\n1;\n";
    let role_a = "package RoleA;\nuse Role::Tiny;\nwith 'BaseRole';\n1;\n";
    let role_b = "package RoleB;\nuse Role::Tiny;\nwith 'BaseRole';\n1;\n";
    let class = "package MyConsumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let diags = pl303_cross_file(
        &[
            ("file:///lib/BaseRole.pm", base_role),
            ("file:///lib/RoleA.pm", role_a),
            ("file:///lib/RoleB.pm", role_b),
        ],
        "file:///lib/MyConsumer.pm",
        class,
    )?;

    assert_eq!(diags.len(), 0, "diamond composition must not produce PL303 — got: {diags:?}");
    Ok(())
}

/// A role that is listed in `with` but never indexed stays conservative:
/// the diagnostics path must not guess at its methods or emit spurious PL303.
#[test]
fn unresolved_cross_file_role_stays_conservative() -> Result<()> {
    // Only one of two roles is indexed; the other is external / not in workspace.
    let role_a = "package RoleA;\nuse Role::Tiny;\nsub shared_method { 'A' }\n1;\n";
    let class = "package MyConsumer;\nuse Moo;\nwith 'RoleA', 'ExternalRole';\n1;\n";

    let diags =
        pl303_cross_file(&[("file:///lib/RoleA.pm", role_a)], "file:///lib/MyConsumer.pm", class)?;

    // ExternalRole is unresolved — its methods are unknown, so no conflict is
    // detected even though RoleA provides shared_method.
    assert_eq!(diags.len(), 0, "unresolved role must not produce spurious PL303 — got: {diags:?}");
    Ok(())
}

/// When the consuming class itself defines a method that two roles both
/// provide, PL303 must be suppressed (the class resolves the conflict).
#[test]
fn class_method_suppresses_cross_file_pl303() -> Result<()> {
    let role_a = "package RoleA;\nuse Role::Tiny;\nsub shared_method { 'A' }\n1;\n";
    let role_b = "package RoleB;\nuse Role::Tiny;\nsub shared_method { 'B' }\n1;\n";
    // MyConsumer defines shared_method itself, resolving the conflict.
    let class =
        "package MyConsumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\nsub shared_method { 'mine' }\n1;\n";

    let diags = pl303_cross_file(
        &[("file:///lib/RoleA.pm", role_a), ("file:///lib/RoleB.pm", role_b)],
        "file:///lib/MyConsumer.pm",
        class,
    )?;

    assert_eq!(diags.len(), 0, "class-defined method must suppress PL303 — got: {diags:?}");
    Ok(())
}
