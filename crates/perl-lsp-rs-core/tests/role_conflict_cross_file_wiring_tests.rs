//! Integration tests for PL303 cross-file role-conflict detection via the
//! production diagnostics wiring.
//!
//! These tests exercise the full path from `role_graph_scope` helpers through
//! `WorkspaceIndex::with_semantic_queries_for_uri_and_graph` to
//! `check_role_conflicts`, closing the Reachable axis for issue #4497.
//!
//! # What is tested
//!
//! 1. Cross-file conflict: two roles defined in separate files both provide the
//!    same method → PL303 fires on the consuming file.
//! 2. Diamond composition (two consumed roles sharing one ancestor) → PL303 does
//!    NOT fire because both providers resolve to the same origin.
//! 3. Unresolved / not-indexed role → no guessed PL303 (conservative).
//! 4. File with no `with` clauses → `consumed_role_names` returns empty vec.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider, role_graph_scope};
use perl_parser::Parser;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

fn parse_for_test(source: &str) -> Arc<perl_parser_core::ast::Node> {
    let output = Parser::new(source).parse_with_recovery();
    Arc::new(output.ast)
}

fn index_file(index: &WorkspaceIndex, uri: &str, source: &str) -> TestResult {
    let url = url::Url::parse(uri)?;
    index
        .index_file(url, source.to_string())
        .map_err(|e| format!("index_file failed for {uri}: {e}").into())
}

fn pl303_diags(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics.iter().filter(|d| d.code.as_deref() == Some("PL303")).collect()
}

fn run_diagnostics_with_scoped_graph(
    index: &WorkspaceIndex,
    consumer_uri: &str,
    consumer_source: &str,
    ast: &Arc<perl_parser_core::ast::Node>,
) -> TestResult<Vec<Diagnostic>> {
    let seed_roles = role_graph_scope::consumed_role_names(ast);
    let scoped_graph = role_graph_scope::build_role_scoped_package_graph(index, &seed_roles);

    index
        .with_semantic_queries_for_uri_and_graph(consumer_uri, &scoped_graph, |file_id, queries| {
            let provider = DiagnosticsProvider::new(ast, consumer_source.to_string());
            provider.get_diagnostics_with_path_and_semantics(
                ast,
                &[],
                consumer_source,
                None,
                &[],
                None,
                file_id,
                &queries,
            )
        })
        .ok_or_else(|| {
            format!("with_semantic_queries_for_uri_and_graph returned None for {consumer_uri}")
                .into()
        })
}

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

// ── Test 1: cross-file conflict fires PL303 ──────────────────────────────────

#[test]
fn cross_file_role_conflict_fires_pl303() -> TestResult {
    let role_a_source = r#"
package App::RoleA;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'from A' }
1;
"#;
    let role_b_source = r#"
package App::RoleB;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'from B' }
1;
"#;
    let consumer_source = r#"
package App::Consumer;
use strict;
use warnings;
use Moo;
with 'App::RoleA', 'App::RoleB';
1;
"#;

    let role_a_uri = "file:///workspace/lib/App/RoleA.pm";
    let role_b_uri = "file:///workspace/lib/App/RoleB.pm";
    let consumer_uri = "file:///workspace/lib/App/Consumer.pm";

    let index = WorkspaceIndex::new();
    index_file(&index, role_a_uri, role_a_source)?;
    index_file(&index, role_b_uri, role_b_source)?;
    index_file(&index, consumer_uri, consumer_source)?;

    let ast = parse_for_test(consumer_source);
    let diagnostics =
        run_diagnostics_with_scoped_graph(&index, consumer_uri, consumer_source, &ast)?;

    let pl303 = pl303_diags(&diagnostics);
    assert!(
        !pl303.is_empty(),
        "cross-file role conflict should emit PL303; got diagnostics: {diagnostics:?}"
    );
    assert!(
        pl303.iter().any(|d| d.message.contains("shared_method")),
        "PL303 message should name the conflicting method; got: {:?}",
        pl303
    );

    Ok(())
}

// ── Test 2: diamond composition does NOT fire PL303 ──────────────────────────

#[test]
fn diamond_composition_no_pl303() -> TestResult {
    // Base defines the method; BothA and BothB each compose Base.
    // Consumer consumes BothA and BothB — both trace to a single origin (Base),
    // so shared_method is NOT a conflict.
    let base_source = r#"
package App::Base;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'from Base' }
1;
"#;
    let both_a_source = r#"
package App::BothA;
use strict;
use warnings;
use Moo::Role;
with 'App::Base';
1;
"#;
    let both_b_source = r#"
package App::BothB;
use strict;
use warnings;
use Moo::Role;
with 'App::Base';
1;
"#;
    let consumer_source = r#"
package App::DiamondConsumer;
use strict;
use warnings;
use Moo;
with 'App::BothA', 'App::BothB';
1;
"#;

    let base_uri = "file:///workspace/lib/App/Base.pm";
    let both_a_uri = "file:///workspace/lib/App/BothA.pm";
    let both_b_uri = "file:///workspace/lib/App/BothB.pm";
    let consumer_uri = "file:///workspace/lib/App/DiamondConsumer.pm";

    let index = WorkspaceIndex::new();
    index_file(&index, base_uri, base_source)?;
    index_file(&index, both_a_uri, both_a_source)?;
    index_file(&index, both_b_uri, both_b_source)?;
    index_file(&index, consumer_uri, consumer_source)?;

    let ast = parse_for_test(consumer_source);
    let diagnostics =
        run_diagnostics_with_scoped_graph(&index, consumer_uri, consumer_source, &ast)?;

    let pl303 = pl303_diags(&diagnostics);
    assert!(pl303.is_empty(), "diamond composition should not emit PL303; got: {pl303:?}");

    Ok(())
}

// ── Test 3: unresolved role stays conservative ────────────────────────────────

#[test]
fn unresolved_role_no_guessed_pl303() -> TestResult {
    // RoleC is not indexed. RoleD is indexed with shared_method.
    // Because RoleC cannot be resolved, no conflict is guessed — conservative.
    let role_d_source = r#"
package App::RoleD;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'from D' }
1;
"#;
    let consumer_source = r#"
package App::ConservativeConsumer;
use strict;
use warnings;
use Moo;
with 'App::RoleC', 'App::RoleD';
1;
"#;

    let role_d_uri = "file:///workspace/lib/App/RoleD.pm";
    let consumer_uri = "file:///workspace/lib/App/ConservativeConsumer.pm";

    let index = WorkspaceIndex::new();
    // RoleC is deliberately NOT indexed — simulates external/unknown role
    index_file(&index, role_d_uri, role_d_source)?;
    index_file(&index, consumer_uri, consumer_source)?;

    let ast = parse_for_test(consumer_source);
    let diagnostics =
        run_diagnostics_with_scoped_graph(&index, consumer_uri, consumer_source, &ast)?;

    let pl303 = pl303_diags(&diagnostics);
    assert!(pl303.is_empty(), "unresolved role should produce no PL303; got: {pl303:?}");

    Ok(())
}

// ── Test 4: fast path — no `with` clauses, seed_roles is empty ───────────────

#[test]
fn no_with_clauses_seed_roles_empty() {
    let source = r#"
package App::Plain;
use strict;
use warnings;
use Moo;
sub my_method { 1 }
1;
"#;
    let ast = parse_for_test(source);
    let seed_roles = role_graph_scope::consumed_role_names(&ast);
    assert!(
        seed_roles.is_empty(),
        "file with no `with` clauses should produce empty seed_roles; got: {seed_roles:?}"
    );
}
