//! Integration tests for PL303 cross-file role conflict detection.
//!
//! Verifies that the end-to-end production path fires PL303 when a class
//! consumes roles defined in separate workspace files that provide the same
//! method from distinct origins, and stays conservative (no false positive)
//! for roles that are unresolvable or share a common ancestor.
//!
//! These tests replicate the live-server code path:
//!
//!   WorkspaceIndex::with_semantic_queries_for_uri
//!   → WorkspaceSemanticQueries (with_package_graph)
//!   → DiagnosticsProvider::get_diagnostics_with_path_and_semantics
//!   → check_role_conflicts → PL303
//!
//! Closing the Reachable axis for issue #4497.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Run the production diagnostics path for `consumer_source` against a
/// pre-populated `WorkspaceIndex` and return only PL303 diagnostics.
///
/// Mirrors the live-server sequence: index a consumer file, fetch workspace
/// semantic queries for it, then call `get_diagnostics_with_path_and_semantics`
/// with those queries so `transitive_role_methods` uses the package graph.
fn pl303_diags_workspace(
    index: &WorkspaceIndex,
    consumer_uri: &str,
    consumer_source: &str,
) -> Vec<Diagnostic> {
    let output = Parser::new(consumer_source).parse_with_recovery();
    let parse_errors = output.diagnostics;
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new(&ast, consumer_source.to_string());

    let all_diags = index
        .with_semantic_queries_for_uri(consumer_uri, |file_id, queries| {
            provider.get_diagnostics_with_path_and_semantics(
                &ast,
                &parse_errors,
                consumer_source,
                None,
                &[],
                None,
                file_id,
                &queries,
            )
        })
        .unwrap_or_else(|| {
            // Fallback used only if the consumer was not indexed, which is a
            // test-setup error in practice; kept for completeness.
            provider.get_diagnostics_with_path(
                &ast,
                &parse_errors,
                consumer_source,
                None,
                &[],
                None,
            )
        });

    all_diags.into_iter().filter(|d| d.code.as_deref() == Some("PL303")).collect()
}

// ---------------------------------------------------------------------------
// Test 1: Cross-file direct role method conflict fires PL303
// ---------------------------------------------------------------------------

/// A class that consumes two roles each defined in a separate indexed file,
/// where both roles provide the same method under distinct origins, produces
/// exactly one PL303 diagnostic through the production workspace path.
#[test]
fn cross_file_role_conflict_fires_pl303() -> Result<()> {
    let role_a_source = r#"
package MyApp::RoleA;
use Role::Tiny;
sub shared_method { 'from_a' }
"#;
    let role_b_source = r#"
package MyApp::RoleB;
use Role::Tiny;
sub shared_method { 'from_b' }
"#;
    let consumer_source = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'MyApp::RoleA', 'MyApp::RoleB';
"#;

    let index = WorkspaceIndex::new();
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleA.pm")?, role_a_source.to_string())?;
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleB.pm")?, role_b_source.to_string())?;
    index.index_file(
        url::Url::parse("file:///lib/MyApp/Consumer.pm")?,
        consumer_source.to_string(),
    )?;

    let diags = pl303_diags_workspace(&index, "file:///lib/MyApp/Consumer.pm", consumer_source);

    assert_eq!(
        diags.len(),
        1,
        "cross-file role method conflict should emit exactly one PL303; got: {diags:?}"
    );
    let diag = &diags[0];
    assert!(
        diag.message.contains("shared_method"),
        "PL303 message should name the conflicting method: {}",
        diag.message
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 2: Diamond composition does not fire PL303
// ---------------------------------------------------------------------------

/// A class consumes two roles (RoleA and RoleB) that each compose a shared
/// ancestor role (RoleBase). Neither RoleA nor RoleB directly defines the
/// contested method; it lives only on RoleBase. The workspace resolver returns
/// no methods for roles that don't directly define them and whose composition
/// edges are not yet modelled in the package graph — staying conservative and
/// producing no false PL303.
#[test]
fn diamond_composition_does_not_fire_pl303() -> Result<()> {
    let role_base_source = r#"
package MyApp::RoleBase;
use Role::Tiny;
sub shared_method { 'from_base' }
"#;
    // RoleA and RoleB compose RoleBase but define no methods of their own.
    let role_a_source = r#"
package MyApp::RoleA;
use Role::Tiny::With;
with 'MyApp::RoleBase';
"#;
    let role_b_source = r#"
package MyApp::RoleB;
use Role::Tiny::With;
with 'MyApp::RoleBase';
"#;
    let consumer_source = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'MyApp::RoleA', 'MyApp::RoleB';
"#;

    let index = WorkspaceIndex::new();
    index.index_file(
        url::Url::parse("file:///lib/MyApp/RoleBase.pm")?,
        role_base_source.to_string(),
    )?;
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleA.pm")?, role_a_source.to_string())?;
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleB.pm")?, role_b_source.to_string())?;
    index.index_file(
        url::Url::parse("file:///lib/MyApp/Consumer.pm")?,
        consumer_source.to_string(),
    )?;

    let diags = pl303_diags_workspace(&index, "file:///lib/MyApp/Consumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "diamond composition should not produce a false PL303; got: {diags:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 3: Roles not in the workspace stay conservative
// ---------------------------------------------------------------------------

/// When a consumer class uses roles that are not indexed in the workspace,
/// the resolver returns empty for each — no methods, no conflict. The lint
/// never guesses PL303 for unresolvable roles.
#[test]
fn external_roles_not_in_workspace_do_not_fire_pl303() -> Result<()> {
    let consumer_source = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'External::RoleX', 'External::RoleY';
"#;

    let index = WorkspaceIndex::new();
    index.index_file(
        url::Url::parse("file:///lib/MyApp/Consumer.pm")?,
        consumer_source.to_string(),
    )?;

    let diags = pl303_diags_workspace(&index, "file:///lib/MyApp/Consumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "unresolved external roles should not produce PL303 (conservative); got: {diags:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 4: Class-defined method suppresses cross-file conflict
// ---------------------------------------------------------------------------

/// When the consuming class defines the contested method itself, no PL303 is
/// emitted even if two indexed roles both provide the same method. The class
/// override suppresses the warning in the cross-file production path.
#[test]
fn class_defined_method_suppresses_cross_file_pl303() -> Result<()> {
    let role_a_source = r#"
package MyApp::RoleA;
use Role::Tiny;
sub contested { 'a' }
"#;
    let role_b_source = r#"
package MyApp::RoleB;
use Role::Tiny;
sub contested { 'b' }
"#;
    // Consumer defines `contested` itself — overrides the conflict.
    let consumer_source = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Role::Tiny::With;
with 'MyApp::RoleA', 'MyApp::RoleB';
sub contested { 'consumer' }
"#;

    let index = WorkspaceIndex::new();
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleA.pm")?, role_a_source.to_string())?;
    index.index_file(url::Url::parse("file:///lib/MyApp/RoleB.pm")?, role_b_source.to_string())?;
    index.index_file(
        url::Url::parse("file:///lib/MyApp/Consumer.pm")?,
        consumer_source.to_string(),
    )?;

    let diags = pl303_diags_workspace(&index, "file:///lib/MyApp/Consumer.pm", consumer_source);

    assert!(
        diags.is_empty(),
        "class-defined method should suppress cross-file PL303; got: {diags:?}"
    );
    Ok(())
}
