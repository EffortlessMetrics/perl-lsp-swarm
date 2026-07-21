//! Integration tests for PL303 cross-file role-conflict detection through the
//! production diagnostics wiring.
//!
//! These tests verify the end-to-end path:
//!
//!   Multiple source files (consumer + role definitions)
//!   → WorkspaceIndex (fact shards populated per file)
//!   → `consumed_role_names` (extract roles from consumer AST)
//!   → `build_role_scoped_package_graph` (BFS over role sources → ComposesRole edges)
//!   → `WorkspaceIndex::with_semantic_queries_for_uri_and_graph`
//!   → `check_role_conflicts` via `transitive_role_methods`
//!   → PL303 diagnostic fires / does not fire per expectation
//!
//! Test cases:
//! 1. Cross-file conflict: RoleA and RoleB both define `shared_method` in
//!    separate files → PL303 fires.
//! 2. Diamond composition: RoleA and RoleB both compose RoleBase which defines
//!    `shared_method`; same origin → PL303 does NOT fire.
//! 3. Unresolved role (external, not in workspace) → conservative: no guessed PL303.

use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic,
    role_conflicts::check_role_conflicts,
    role_graph_scope::{build_role_scoped_package_graph, consumed_role_names},
};
use perl_semantic_analyzer::symbol::SymbolExtractor;
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn pl303_diagnostics_cross_file(
    index: &WorkspaceIndex,
    consumer_uri: &str,
    consumer_src: &str,
) -> Result<Vec<Diagnostic>> {
    let ast = {
        let mut parser = perl_semantic_analyzer::Parser::new(consumer_src);
        parser.parse().map_err(|e| format!("parse error: {e:?}"))?
    };
    let symbol_table = SymbolExtractor::new_with_source(consumer_src).extract(&ast);

    let role_names = consumed_role_names(&ast);
    let role_graph = build_role_scoped_package_graph(index, &role_names);

    let diags = index
        .with_semantic_queries_for_uri_and_graph(consumer_uri, &role_graph, |_file_id, queries| {
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            check_role_conflicts(
                &ast,
                &symbol_table,
                &|role| queries.transitive_role_methods(role),
                &mut diagnostics,
            );
            diagnostics
        })
        .unwrap_or_default();

    Ok(diags.into_iter().filter(|d| d.code.as_deref() == Some("PL303")).collect())
}

// ── Case 1: cross-file conflict fires PL303 ──────────────────────────────────

#[test]
fn cross_file_conflict_fires_pl303() -> Result<()> {
    let consumer_uri = "file:///test/xf_conflict/Consumer.pm";
    let role_a_uri = "file:///test/xf_conflict/RoleA.pm";
    let role_b_uri = "file:///test/xf_conflict/RoleB.pm";

    let consumer_src = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Moo;
with 'MyApp::RoleA', 'MyApp::RoleB';
"#;
    let role_a_src = r#"
package MyApp::RoleA;
use strict;
use warnings;
use Moo::Role;
sub shared_method { return 'from_A' }
"#;
    let role_b_src = r#"
package MyApp::RoleB;
use strict;
use warnings;
use Moo::Role;
sub shared_method { return 'from_B' }
"#;

    let index = WorkspaceIndex::new();
    index.index_file_str(role_a_uri, role_a_src)?;
    index.index_file_str(role_b_uri, role_b_src)?;
    index.index_file_str(consumer_uri, consumer_src)?;

    let pl303 = pl303_diagnostics_cross_file(&index, consumer_uri, consumer_src)?;

    assert_eq!(
        pl303.len(),
        1,
        "cross-file role conflict should produce exactly one PL303: {pl303:?}"
    );
    assert!(
        pl303[0].message.contains("shared_method"),
        "PL303 message should name the conflicting method: {}",
        pl303[0].message
    );
    assert!(
        pl303[0].message.contains("MyApp::Consumer"),
        "PL303 message should name the consuming class: {}",
        pl303[0].message
    );

    Ok(())
}

// ── Case 2: diamond composition does NOT fire PL303 ──────────────────────────

#[test]
fn diamond_composition_does_not_fire_pl303() -> Result<()> {
    let consumer_uri = "file:///test/xf_diamond/Consumer.pm";
    let role_a_uri = "file:///test/xf_diamond/RoleA.pm";
    let role_b_uri = "file:///test/xf_diamond/RoleB.pm";
    let role_base_uri = "file:///test/xf_diamond/RoleBase.pm";

    // RoleBase defines the method; RoleA and RoleB both compose RoleBase.
    let consumer_src = r#"
package Diamond::Consumer;
use strict;
use warnings;
use Moo;
with 'Diamond::RoleA', 'Diamond::RoleB';
"#;
    let role_a_src = r#"
package Diamond::RoleA;
use strict;
use warnings;
use Moo::Role;
with 'Diamond::RoleBase';
"#;
    let role_b_src = r#"
package Diamond::RoleB;
use strict;
use warnings;
use Moo::Role;
with 'Diamond::RoleBase';
"#;
    let role_base_src = r#"
package Diamond::RoleBase;
use strict;
use warnings;
use Moo::Role;
sub shared_method { return 'from_base' }
"#;

    let index = WorkspaceIndex::new();
    index.index_file_str(role_base_uri, role_base_src)?;
    index.index_file_str(role_a_uri, role_a_src)?;
    index.index_file_str(role_b_uri, role_b_src)?;
    index.index_file_str(consumer_uri, consumer_src)?;

    let pl303 = pl303_diagnostics_cross_file(&index, consumer_uri, consumer_src)?;

    assert!(
        pl303.is_empty(),
        "diamond composition (same origin) must not produce PL303: {pl303:?}"
    );

    Ok(())
}

// ── Case 3: unresolved external role stays conservative (no guessed PL303) ───

#[test]
fn unresolved_external_role_stays_conservative() -> Result<()> {
    let consumer_uri = "file:///test/xf_external/Consumer.pm";

    // External::SomeRole is not in the workspace — find_definition returns None.
    let consumer_src = r#"
package MyApp::External;
use strict;
use warnings;
use Moo;
with 'External::SomeRole', 'External::OtherRole';
"#;

    let index = WorkspaceIndex::new();
    index.index_file_str(consumer_uri, consumer_src)?;

    let pl303 = pl303_diagnostics_cross_file(&index, consumer_uri, consumer_src)?;

    assert!(
        pl303.is_empty(),
        "unresolved external roles must not produce guessed PL303: {pl303:?}"
    );

    Ok(())
}
