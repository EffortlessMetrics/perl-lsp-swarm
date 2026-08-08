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
//! 5. Cycle regression: two cross-file roles that compose each other → BFS
//!    terminates and no spurious PL303.
//! 6. Multi-hop transitive conflict: consumer pulls a method transitively
//!    through a three-hop role chain; another direct role provides the same
//!    method with a different origin → PL303 fires.
//! 7. Stale-index invalidation: after re-indexing a role to remove a method,
//!    the conflict is no longer detected.
//! 8. Cross-file class-method suppression: consumer defines the method
//!    directly → suppresses PL303 even when both roles are cross-file.

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
            let provider = DiagnosticsProvider::new();
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

// ── Case 5: cycle regression — mutually composing cross-file roles ──
//
// Role A composes Role B and Role B composes Role A. This is invalid in Perl
// but the graph builder and transitive traversal must terminate rather than
// loop. Neither role provides a conflicting method, so no PL303 should fire
// and the test must complete in finite time.

#[test]
fn circular_role_composition_does_not_hang_or_emit_spurious_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    // Role A and B compose each other (a cycle). Each provides a *unique*
    // method so there is no real conflict from the consumer's perspective.
    let role_a_source = r#"
package Cycle::RoleA;
use Role::Tiny;
use Role::Tiny::With;
with 'Cycle::RoleB';
sub only_in_a { 1 }
1;
"#;
    let role_b_source = r#"
package Cycle::RoleB;
use Role::Tiny;
use Role::Tiny::With;
with 'Cycle::RoleA';
sub only_in_b { 1 }
1;
"#;
    let consumer_source = r#"
package Cycle::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'Cycle::RoleA', 'Cycle::RoleB';
1;
"#;

    index_file(&index, "file:///cycle/RoleA.pm", role_a_source)?;
    index_file(&index, "file:///cycle/RoleB.pm", role_b_source)?;
    index_file(&index, "file:///cycle/Consumer.pm", consumer_source)?;

    // Must complete without hanging or panicking.
    let diags = pl303_diags_cross_file(&index, "file:///cycle/Consumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "cyclic role graph with no method overlap must not emit PL303, got: {diags:?}"
    );
    Ok(())
}

// ── Case 6: multi-hop transitive conflict ──
//
// ConsumedRole::A composes ConsumedRole::B, which composes ConsumedRole::Base.
// ConsumedRole::Base defines `process`. DirectRole defines its own `process`.
// Consumer uses both ConsumedRole::A (transitively provides `process` via Base)
// and DirectRole (directly provides `process`). These are different origins, so
// PL303 must fire.

#[test]
fn transitive_three_hop_conflict_emits_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    let base_source = r#"
package Trans::Base;
use Role::Tiny;
sub process { 'base' }
1;
"#;
    // B composes Base, contributing `process` with origin Trans::Base.
    let role_b_source = r#"
package Trans::B;
use Role::Tiny;
use Role::Tiny::With;
with 'Trans::Base';
1;
"#;
    // A composes B, transitively contributing `process` from Trans::Base.
    let role_a_source = r#"
package Trans::A;
use Role::Tiny;
use Role::Tiny::With;
with 'Trans::B';
1;
"#;
    // DirectRole defines its own `process` (different origin).
    let direct_source = r#"
package Trans::DirectRole;
use Role::Tiny;
sub process { 'direct' }
1;
"#;
    // Consumer uses Trans::A (which transitively provides Trans::Base::process)
    // and Trans::DirectRole (which provides its own process). Different origins
    // → genuine conflict.
    let consumer_source = r#"
package Trans::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'Trans::A', 'Trans::DirectRole';
1;
"#;

    index_file(&index, "file:///trans/Base.pm", base_source)?;
    index_file(&index, "file:///trans/B.pm", role_b_source)?;
    index_file(&index, "file:///trans/A.pm", role_a_source)?;
    index_file(&index, "file:///trans/Direct.pm", direct_source)?;
    index_file(&index, "file:///trans/Consumer.pm", consumer_source)?;

    let diags = pl303_diags_cross_file(&index, "file:///trans/Consumer.pm", consumer_source);

    assert_eq!(
        diags.len(),
        1,
        "transitive three-hop conflict should emit exactly one PL303, got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("process"),
        "PL303 message should name the conflicting method `process`: {}",
        diags[0].message
    );
    Ok(())
}

// ── Case 7: stale-index invalidation ──
//
// After re-indexing a role to remove its method, the conflict must no longer
// be detected. Verifies that the scoped graph builder uses the latest indexed
// source and that stale method contributions are not carried forward.

#[test]
fn stale_index_after_role_method_removal_clears_pl303() -> Result<()> {
    let index = WorkspaceIndex::new();

    // Initial: Role A defines `shared_method` → conflict with Role B.
    let role_a_with_method = r#"
package Stale::RoleA;
use Role::Tiny;
sub shared_method { 'from_A' }
1;
"#;
    let role_b_source = r#"
package Stale::RoleB;
use Role::Tiny;
sub shared_method { 'from_B' }
1;
"#;
    let consumer_source = r#"
package Stale::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'Stale::RoleA', 'Stale::RoleB';
1;
"#;

    index_file(&index, "file:///stale/RoleA.pm", role_a_with_method)?;
    index_file(&index, "file:///stale/RoleB.pm", role_b_source)?;
    index_file(&index, "file:///stale/Consumer.pm", consumer_source)?;

    let before = pl303_diags_cross_file(&index, "file:///stale/Consumer.pm", consumer_source);
    assert_eq!(
        before.len(),
        1,
        "initial state: conflict should produce one PL303, got: {before:?}"
    );

    // Re-index Role A without `shared_method` — simulates a user editing the
    // file to remove the conflicting sub.
    let role_a_without_method = r#"
package Stale::RoleA;
use Role::Tiny;
sub different_method { 'from_A' }
1;
"#;
    index_file(&index, "file:///stale/RoleA.pm", role_a_without_method)?;

    let after = pl303_diags_cross_file(&index, "file:///stale/Consumer.pm", consumer_source);
    assert!(
        after.is_empty(),
        "after removing shared_method from RoleA, PL303 must not fire, got: {after:?}"
    );
    Ok(())
}

// ── Case 8: cross-file class-method suppression ──
//
// Both consumed roles are cross-file and both provide the same method, but
// the consuming class defines that method itself. PL303 must be suppressed.

#[test]
fn cross_file_conflict_suppressed_when_consumer_class_defines_method() -> Result<()> {
    let index = WorkspaceIndex::new();

    let role_a_source = r#"
package Suppress::RoleA;
use Role::Tiny;
sub render { 'A' }
1;
"#;
    let role_b_source = r#"
package Suppress::RoleB;
use Role::Tiny;
sub render { 'B' }
1;
"#;
    // Consumer defines its own `render`, resolving the conflict.
    let consumer_source = r#"
package Suppress::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'Suppress::RoleA', 'Suppress::RoleB';
sub render { 'mine' }
1;
"#;

    index_file(&index, "file:///suppress/RoleA.pm", role_a_source)?;
    index_file(&index, "file:///suppress/RoleB.pm", role_b_source)?;
    index_file(&index, "file:///suppress/Consumer.pm", consumer_source)?;

    let diags = pl303_diags_cross_file(&index, "file:///suppress/Consumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "consumer class defining the conflicting method must suppress PL303, got: {diags:?}"
    );
    Ok(())
}
