//! Integration tests: PL303 cross-file role-conflict detection through WorkspaceIndex.
//!
//! These tests verify the full production path:
//!
//!   WorkspaceIndex (multi-file)
//!   → with_semantic_queries_for_uri → WorkspaceSemanticQueries (PackageGraphIndex wired)
//!   → DiagnosticsProvider::get_diagnostics_with_path_and_semantics
//!   → check_role_conflicts → transitive_role_methods
//!   → PL303 diagnostic
//!
//! This closes the "Reachable" axis from issue #4497: the substrate was landed
//! in #4471; these tests exercise the live-server path end-to-end.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Index `other_files` and `consumer_source` into a `WorkspaceIndex`, then
/// return PL303 diagnostics for the consumer produced through the
/// workspace-semantic diagnostics path (the same path used in the live server).
fn pl303_via_workspace(
    consumer_uri: &str,
    consumer_source: &str,
    other_files: &[(&str, &str)],
) -> Result<Vec<Diagnostic>> {
    let index = WorkspaceIndex::new();

    for &(uri, source) in other_files {
        let url = Url::parse(uri)?;
        index
            .index_file(url, source.to_string())
            .map_err(|e| -> Box<dyn std::error::Error> { format!("index_file({uri}): {e}").into() })?;
    }

    let consumer_url = Url::parse(consumer_uri)?;
    index
        .index_file(consumer_url, consumer_source.to_string())
        .map_err(|e| -> Box<dyn std::error::Error> { format!("index_file(consumer): {e}").into() })?;

    let output = Parser::new(consumer_source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new(&ast, consumer_source.to_string());

    let diags = index
        .with_semantic_queries_for_uri(consumer_uri, |file_id, queries| {
            provider.get_diagnostics_with_path_and_semantics(
                &ast,
                &output.diagnostics,
                consumer_source,
                None,
                &[],
                None,
                file_id,
                &queries,
            )
        })
        .ok_or("consumer URI not indexed after index_file")?;

    Ok(diags.into_iter().filter(|d| d.code.as_deref() == Some("PL303")).collect())
}

#[test]
fn cross_file_moo_role_conflict_fires_pl303() -> Result<()> {
    // Two Moo roles, each defining the same method, are consumed together.
    // Their definitions live in separate files (not in the consumer's file).
    // The workspace-semantic path must resolve the methods and emit PL303.
    let role_a = (
        "file:///lib/MyApp/RoleA.pm",
        "package MyApp::RoleA;\nuse Moo::Role;\nsub shared_method { 'A' }\n",
    );
    let role_b = (
        "file:///lib/MyApp/RoleB.pm",
        "package MyApp::RoleB;\nuse Moo::Role;\nsub shared_method { 'B' }\n",
    );
    let consumer_uri = "file:///lib/MyApp/Consumer.pm";
    let consumer_source = "package MyApp::Consumer;\nuse Moo;\nwith 'MyApp::RoleA', 'MyApp::RoleB';\n";

    let diags = pl303_via_workspace(consumer_uri, consumer_source, &[role_a, role_b])?;

    assert!(
        !diags.is_empty(),
        "cross-file Moo role conflict must emit PL303 through the workspace path, got none"
    );
    assert!(
        diags[0].message.contains("shared_method"),
        "PL303 must name the conflicting method; got: {}",
        diags[0].message
    );
    Ok(())
}

#[test]
fn cross_file_diamond_composition_is_not_a_conflict() -> Result<()> {
    // RoleA and RoleB both compose a shared ancestor (Base) which provides
    // shared_method. Diamond composition means both get the method from the
    // same origin. That is NOT a conflict — Perl resolution is deterministic.
    let base = (
        "file:///lib/MyApp/Base.pm",
        "package MyApp::Base;\nuse Moo::Role;\nsub shared_method { 'base' }\n",
    );
    let role_a = (
        "file:///lib/MyApp/RoleA.pm",
        "package MyApp::RoleA;\nuse Moo::Role;\nwith 'MyApp::Base';\n",
    );
    let role_b = (
        "file:///lib/MyApp/RoleB.pm",
        "package MyApp::RoleB;\nuse Moo::Role;\nwith 'MyApp::Base';\n",
    );
    let consumer_uri = "file:///lib/MyApp/Consumer.pm";
    let consumer_source =
        "package MyApp::Consumer;\nuse Moo;\nwith 'MyApp::RoleA', 'MyApp::RoleB';\n";

    let diags =
        pl303_via_workspace(consumer_uri, consumer_source, &[base, role_a, role_b])?;

    assert!(
        diags.is_empty(),
        "diamond composition (same-origin method) must NOT emit PL303; got: {diags:?}"
    );
    Ok(())
}

#[test]
fn unresolved_cross_file_roles_stay_conservative() -> Result<()> {
    // Consumer references roles that are never indexed. The lint must not
    // guess a conflict — stay silent when roles cannot be resolved.
    let consumer_uri = "file:///lib/MyApp/Consumer.pm";
    let consumer_source = "package MyApp::Consumer;\nuse Moo;\nwith 'MyApp::RoleA', 'MyApp::RoleB';\n";

    // No role files indexed — neither role can be resolved.
    let diags = pl303_via_workspace(consumer_uri, consumer_source, &[])?;

    assert!(
        diags.is_empty(),
        "unresolved cross-file roles must not emit PL303 (conservative); got: {diags:?}"
    );
    Ok(())
}
