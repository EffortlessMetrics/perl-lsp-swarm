//! Integration tests for PL303 cross-file role-conflict detection reachability.
//!
//! Verifies that the production diagnostics path fires PL303 when cross-file
//! or transitive role method conflicts exist, using the real
//! `build_role_scoped_package_graph` + `with_semantic_queries_for_uri_and_graph`
//! path introduced in #4497.
//!
//! # Test cases
//!
//! 1. Cross-file direct conflict: two roles defined in separate files both
//!    provide the same method → PL303 fires.
//! 2. Diamond composition: two roles both compose the same base role that
//!    provides the method → same origin → no conflict → PL303 suppressed.
//! 3. Unresolved/external role: one role not indexed → stays conservative,
//!    no PL303 for the unresolvable role.
//! 4. Fast path: consumer with no `with` clauses → `consumed_role_names`
//!    returns empty → no scoped graph built.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::role_graph_scope::{
    build_role_scoped_package_graph, consumed_role_names,
};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn index_file(index: &WorkspaceIndex, uri: &str, source: &str) -> Result<()> {
    index.index_file(Url::parse(uri)?, source.to_string())?;
    Ok(())
}

fn parse_arc(source: &str) -> Arc<perl_parser_core::ast::Node> {
    let output = perl_parser::Parser::new(source).parse_with_recovery();
    Arc::new(output.ast)
}

fn pl303_diags_cross_file(
    index: &WorkspaceIndex,
    consumer_uri: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let ast = parse_arc(source);
    let role_names = consumed_role_names(&ast);
    let scoped_graph = build_role_scoped_package_graph(index, &role_names);

    index
        .with_semantic_queries_for_uri_and_graph(consumer_uri, &scoped_graph, |file_id, queries| {
            let provider = DiagnosticsProvider::new(&ast, source.to_string());
            let all_diags = provider.get_diagnostics_with_search_context_and_semantics(
                &ast,
                &[],
                source,
                None,
                &[],
                None,
                file_id,
                &queries,
            );
            all_diags.into_iter().filter(|d| d.code.as_deref() == Some("PL303")).collect()
        })
        .unwrap_or_default()
}

// ── Case 1: cross-file direct conflict ──

#[test]
fn cross_file_conflict_emits_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    let role_a_source = r#"
package MyApp::RoleA;
use Role::Tiny;
sub shared_method { 'from_A' }
1;
"#;
    let role_b_source = r#"
package MyApp::RoleB;
use Role::Tiny;
sub shared_method { 'from_B' }
1;
"#;
    let consumer_source = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'MyApp::RoleA', 'MyApp::RoleB';
1;
"#;

    index_file(&index, "file:///test/RoleA.pm", role_a_source)?;
    index_file(&index, "file:///test/RoleB.pm", role_b_source)?;
    index_file(&index, "file:///test/Consumer.pm", consumer_source)?;

    let diags = pl303_diags_cross_file(&index, "file:///test/Consumer.pm", consumer_source);

    assert_eq!(diags.len(), 1, "cross-file conflict should emit exactly one PL303, got: {diags:?}");
    assert!(
        diags[0].message.contains("shared_method"),
        "PL303 message should name the conflicting method: {}",
        diags[0].message
    );
    Ok(())
}

// ── Case 2: diamond composition — same origin, no conflict ──

#[test]
fn diamond_composition_does_not_emit_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    let base_role_source = r#"
package MyApp::BaseRole;
use Role::Tiny;
sub shared_method { 'from_base' }
1;
"#;
    let role_a_source = r#"
package MyApp::DiamondA;
use Role::Tiny;
use Role::Tiny::With;
with 'MyApp::BaseRole';
1;
"#;
    let role_b_source = r#"
package MyApp::DiamondB;
use Role::Tiny;
use Role::Tiny::With;
with 'MyApp::BaseRole';
1;
"#;
    let consumer_source = r#"
package MyApp::DiamondConsumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'MyApp::DiamondA', 'MyApp::DiamondB';
1;
"#;

    index_file(&index, "file:///test/BaseRole.pm", base_role_source)?;
    index_file(&index, "file:///test/DiamondA.pm", role_a_source)?;
    index_file(&index, "file:///test/DiamondB.pm", role_b_source)?;
    index_file(&index, "file:///test/DiamondConsumer.pm", consumer_source)?;

    let diags = pl303_diags_cross_file(&index, "file:///test/DiamondConsumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "diamond composition (same origin) must not emit PL303, got: {diags:?}"
    );
    Ok(())
}

// ── Case 3: unresolved / not-indexed role stays conservative ──

#[test]
fn unresolved_role_does_not_emit_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    // Only index the consumer; the role itself is not indexed (external/unknown).
    let consumer_source = r#"
package MyApp::ExtConsumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'Some::External::Role';
1;
"#;

    index_file(&index, "file:///test/ExtConsumer.pm", consumer_source)?;

    let diags = pl303_diags_cross_file(&index, "file:///test/ExtConsumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "unresolved external role must not emit spurious PL303, got: {diags:?}"
    );
    Ok(())
}

// ── Case 4: consumed_role_names fast path ──

#[test]
fn no_with_clauses_returns_empty_role_names() {
    let source = r#"
package My::Plain;
use strict;
use warnings;
sub foo { 1 }
1;
"#;
    let ast = parse_arc(source);
    let role_names = consumed_role_names(&ast);
    assert!(
        role_names.is_empty(),
        "file with no 'with' clauses should produce no role names, got: {role_names:?}"
    );
}
