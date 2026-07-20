//! Integration tests for PL303 cross-file role-conflict detection via the
//! production wiring path: real `WorkspaceIndex` + `build_role_scoped_package_graph`
//! + `with_semantic_queries_for_uri_and_graph`.
//!
//! These tests close the **Reachable** axis for issue #4497, proving that PL303
//! fires end-to-end through real workspace data — not just through unit-test
//! resolvers backed by manually-constructed graphs.
//!
//! # What is being tested
//!
//! The production path previously passed the internal `PackageGraphIndex` to
//! `WorkspaceSemanticQueries`, but that index only holds `Inherits` edges (the
//! HIR lowerer does not emit `ComposesRole` edges). As a result,
//! `transitive_role_methods` could not resolve methods supplied by roles that
//! are themselves composed through `with` — the lint degraded to same-file
//! analysis only. The new path (`build_role_scoped_package_graph` +
//! `with_semantic_queries_for_uri_and_graph`) builds a request-scoped graph
//! containing real `ComposesRole` edges for the roles consumed by a file,
//! enabling both direct and transitive cross-file conflict detection.

use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic,
    role_conflicts::check_role_conflicts,
    role_graph_scope::{build_role_scoped_package_graph, consumed_role_names},
};
use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{Parser, symbol::SymbolExtractor};
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn index_file(index: &WorkspaceIndex, uri: &str, source: &str) -> Result<()> {
    let url = url::Url::parse(uri)?;
    index.index_file(url, source.to_string()).map_err(|e| format!("index_file: {e}").into())
}

fn parse_source(source: &str) -> Result<Node> {
    Parser::new(source).parse().map_err(|e| format!("parse: {e:?}").into())
}

fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code.as_deref() == Some(code))
}

/// Run `check_role_conflicts` against `source` through the real wiring:
/// WorkspaceIndex → `build_role_scoped_package_graph` →
/// `with_semantic_queries_for_uri_and_graph` → `transitive_role_methods`.
fn diags_for_consumer(
    index: &WorkspaceIndex,
    consumer_uri: &str,
    consumer_source: &str,
) -> Result<Vec<Diagnostic>> {
    let ast = parse_source(consumer_source)?;
    let symbol_table = SymbolExtractor::new_with_source(consumer_source).extract(&ast);

    let role_names = consumed_role_names(&ast);
    let scoped_graph = build_role_scoped_package_graph(index, &role_names, consumer_uri);

    let diagnostics = index
        .with_semantic_queries_for_uri_and_graph(
            consumer_uri,
            &scoped_graph,
            |_file_id, queries| {
                let mut diags = Vec::new();
                check_role_conflicts(
                    &ast,
                    &symbol_table,
                    &|role| queries.transitive_role_methods(role),
                    &mut diags,
                );
                diags
            },
        )
        .unwrap_or_default();

    Ok(diagnostics)
}

// ── Case 1: Direct cross-file conflict fires PL303 ──────────────────────────
//
// RoleA and RoleB are each defined in their own file, both provide `run`.
// Consumer.pm consumes both: PL303 must fire.

#[test]
fn direct_cross_file_role_conflict_fires_pl303() -> Result<()> {
    const URI_ROLE_A: &str = "file:///test/pl303_direct/RoleA.pm";
    const URI_ROLE_B: &str = "file:///test/pl303_direct/RoleB.pm";
    const URI_CONSUMER: &str = "file:///test/pl303_direct/Consumer.pm";

    let source_role_a = "package RoleA;\nuse Moo::Role;\nsub run { return 'a' }\n1;\n";
    let source_role_b = "package RoleB;\nuse Moo::Role;\nsub run { return 'b' }\n1;\n";
    let source_consumer = "package Consumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let index = WorkspaceIndex::new();
    index_file(&index, URI_ROLE_A, source_role_a)?;
    index_file(&index, URI_ROLE_B, source_role_b)?;
    index_file(&index, URI_CONSUMER, source_consumer)?;

    let diags = diags_for_consumer(&index, URI_CONSUMER, source_consumer)?;

    assert!(
        has_code(&diags, "PL303"),
        "direct cross-file conflict: RoleA and RoleB both provide `run` — expected PL303; got {diags:?}"
    );
    Ok(())
}

// ── Case 2: Transitive cross-file conflict fires PL303 ──────────────────────
//
// This is the case that specifically requires `ComposesRole` edges in the graph.
// RoleA composes RoleABase (which has `run`); RoleB composes RoleBBase (which
// has `run`). Without the scoped graph, `transitive_role_methods` can't traverse
// the composition and returns [] for RoleA and RoleB — PL303 would silently fail.
// With the scoped graph the traversal finds both origins and PL303 fires.

#[test]
fn transitive_cross_file_role_conflict_fires_pl303() -> Result<()> {
    const URI_BASE_A: &str = "file:///test/pl303_transitive/RoleABase.pm";
    const URI_BASE_B: &str = "file:///test/pl303_transitive/RoleBBase.pm";
    const URI_ROLE_A: &str = "file:///test/pl303_transitive/RoleA.pm";
    const URI_ROLE_B: &str = "file:///test/pl303_transitive/RoleB.pm";
    const URI_CONSUMER: &str = "file:///test/pl303_transitive/Consumer.pm";

    let source_base_a = "package RoleABase;\nuse Moo::Role;\nsub run { return 'base_a' }\n1;\n";
    let source_base_b = "package RoleBBase;\nuse Moo::Role;\nsub run { return 'base_b' }\n1;\n";
    // RoleA itself has no `sub run`; it is provided via composition with RoleABase.
    let source_role_a = "package RoleA;\nuse Moo::Role;\nwith 'RoleABase';\n1;\n";
    let source_role_b = "package RoleB;\nuse Moo::Role;\nwith 'RoleBBase';\n1;\n";
    let source_consumer = "package Consumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let index = WorkspaceIndex::new();
    index_file(&index, URI_BASE_A, source_base_a)?;
    index_file(&index, URI_BASE_B, source_base_b)?;
    index_file(&index, URI_ROLE_A, source_role_a)?;
    index_file(&index, URI_ROLE_B, source_role_b)?;
    index_file(&index, URI_CONSUMER, source_consumer)?;

    let diags = diags_for_consumer(&index, URI_CONSUMER, source_consumer)?;

    assert!(
        has_code(&diags, "PL303"),
        "transitive cross-file conflict: RoleA (via RoleABase) and RoleB (via RoleBBase) \
         both provide `run` — expected PL303; got {diags:?}"
    );
    Ok(())
}

// ── Case 3: Diamond composition does NOT fire PL303 ─────────────────────────
//
// RoleA and RoleB both compose SharedRole, which provides `run`. Both resolve
// `run` to the same origin (SharedRole), so there is no true conflict.

#[test]
fn diamond_composition_does_not_fire_pl303() -> Result<()> {
    const URI_SHARED: &str = "file:///test/pl303_diamond/SharedRole.pm";
    const URI_ROLE_A: &str = "file:///test/pl303_diamond/RoleA.pm";
    const URI_ROLE_B: &str = "file:///test/pl303_diamond/RoleB.pm";
    const URI_CONSUMER: &str = "file:///test/pl303_diamond/Consumer.pm";

    let source_shared = "package SharedRole;\nuse Moo::Role;\nsub run { return 'shared' }\n1;\n";
    let source_role_a = "package RoleA;\nuse Moo::Role;\nwith 'SharedRole';\n1;\n";
    let source_role_b = "package RoleB;\nuse Moo::Role;\nwith 'SharedRole';\n1;\n";
    let source_consumer = "package Consumer;\nuse Moo;\nwith 'RoleA', 'RoleB';\n1;\n";

    let index = WorkspaceIndex::new();
    index_file(&index, URI_SHARED, source_shared)?;
    index_file(&index, URI_ROLE_A, source_role_a)?;
    index_file(&index, URI_ROLE_B, source_role_b)?;
    index_file(&index, URI_CONSUMER, source_consumer)?;

    let diags = diags_for_consumer(&index, URI_CONSUMER, source_consumer)?;

    assert!(
        !has_code(&diags, "PL303"),
        "diamond composition: RoleA and RoleB both compose SharedRole (same origin) — \
         expected no PL303; got {diags:?}"
    );
    Ok(())
}

// ── Case 4: Unresolved external role stays conservative ──────────────────────
//
// Consumer consumes an external role (ExternalRole::NotInIndex) that is not
// indexed anywhere. The lint must stay conservative and must not emit PL303.

#[test]
fn unresolved_external_role_emits_no_pl303() -> Result<()> {
    const URI_ROLE_A: &str = "file:///test/pl303_external/RoleA.pm";
    const URI_CONSUMER: &str = "file:///test/pl303_external/Consumer.pm";

    let source_role_a = "package RoleA;\nuse Moo::Role;\nsub run { return 'a' }\n1;\n";
    // ExternalRole::NotInIndex is not indexed — the workspace cannot resolve it.
    let source_consumer =
        "package Consumer;\nuse Moo;\nwith 'RoleA', 'ExternalRole::NotInIndex';\n1;\n";

    let index = WorkspaceIndex::new();
    index_file(&index, URI_ROLE_A, source_role_a)?;
    index_file(&index, URI_CONSUMER, source_consumer)?;

    let diags = diags_for_consumer(&index, URI_CONSUMER, source_consumer)?;

    assert!(
        !has_code(&diags, "PL303"),
        "unresolved external role: only one resolved provider — expected no PL303; got {diags:?}"
    );
    Ok(())
}

// ── Case 5: consumed_role_names returns empty for a plain file ───────────────
//
// A file with no `with` clauses should return an empty vec from
// `consumed_role_names`, allowing the caller to skip graph construction
// entirely (the fast path).

#[test]
fn consumed_role_names_empty_for_plain_module() -> Result<()> {
    let source = "package Foo;\nuse strict;\nuse warnings;\nsub bar { 1 }\n1;\n";
    let ast = parse_source(source)?;
    let names = consumed_role_names(&ast);
    assert!(
        names.is_empty(),
        "plain module with no `with` should produce no role names; got {names:?}"
    );
    Ok(())
}
