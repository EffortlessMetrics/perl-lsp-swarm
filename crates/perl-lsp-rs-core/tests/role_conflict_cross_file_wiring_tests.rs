//! Integration tests for PL303 cross-file role-conflict detection wiring.
//!
//! Exercises the full path introduced by issue #4497:
//!
//!   consumed_role_names(ast)
//!   → build_role_scoped_package_graph(index, roles, current_uri)
//!   → WorkspaceIndex::with_semantic_queries_for_uri_and_graph(uri, &graph, |fid, q| ...)
//!   → DiagnosticsProvider::get_diagnostics_with_search_context_and_semantics(...)
//!   → PL303 fires (or not) as expected
//!
//! These are distinct from the same-file tests in `role_tiny_conflict_tests.rs`
//! because the role definitions live in separately-indexed workspace files.

#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use std::sync::Arc;

    use perl_lsp_rs_core::providers::diagnostics::role_graph_scope::{
        build_role_scoped_package_graph, consumed_role_names,
    };
    use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
    use perl_workspace::workspace_index::WorkspaceIndex;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn pl303_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags.iter().filter(|d| d.code.as_deref() == Some("PL303")).collect()
    }

    /// Index several source strings into a `WorkspaceIndex` and return it.
    fn build_index(files: &[(&str, &str)]) -> Result<WorkspaceIndex, Box<dyn std::error::Error>> {
        let index = WorkspaceIndex::new();
        for (uri, src) in files {
            index
                .index_file_str(uri, src)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        }
        Ok(index)
    }

    /// Parse `source`, run the full cross-file diagnostic path, and return all
    /// diagnostics emitted for the consumer file.
    fn diagnostics_for_consumer(
        index: &WorkspaceIndex,
        consumer_uri: &str,
        consumer_source: &str,
    ) -> Vec<Diagnostic> {
        let output = perl_parser::Parser::new(consumer_source).parse_with_recovery();
        let ast = Arc::new(output.ast);
        let provider = DiagnosticsProvider::new(&ast, consumer_source.to_string());

        let consumed_roles = consumed_role_names(&ast);
        if consumed_roles.is_empty() {
            return provider.get_diagnostics(&ast, &output.diagnostics, consumer_source, None);
        }

        let role_graph = build_role_scoped_package_graph(index, &consumed_roles, consumer_uri);

        index
            .with_semantic_queries_for_uri_and_graph(
                consumer_uri,
                &role_graph,
                |file_id, queries| {
                    provider.get_diagnostics_with_search_context_and_semantics(
                        &ast,
                        &output.diagnostics,
                        consumer_source,
                        None,
                        &[],
                        None,
                        file_id,
                        &queries,
                    )
                },
            )
            .unwrap_or_else(|| {
                provider.get_diagnostics(&ast, &output.diagnostics, consumer_source, None)
            })
    }

    // ── Test 1: two cross-file roles conflict → PL303 ────────────────────────

    #[test]
    fn cross_file_role_conflict_emits_pl303() -> TestResult {
        let role_a_uri = "file:///lib/MyApp/RoleA.pm";
        let role_a_src = r#"
package MyApp::RoleA;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'A' }
1;
"#;

        let role_b_uri = "file:///lib/MyApp/RoleB.pm";
        let role_b_src = r#"
package MyApp::RoleB;
use strict;
use warnings;
use Moo::Role;
sub shared_method { 'B' }
1;
"#;

        let consumer_uri = "file:///lib/MyApp/Consumer.pm";
        let consumer_src = r#"
package MyApp::Consumer;
use strict;
use warnings;
use Moo;
with 'MyApp::RoleA', 'MyApp::RoleB';
1;
"#;

        let index = build_index(&[
            (role_a_uri, role_a_src),
            (role_b_uri, role_b_src),
            (consumer_uri, consumer_src),
        ])?;

        let diags = diagnostics_for_consumer(&index, consumer_uri, consumer_src);
        let pl303 = pl303_diags(&diags);

        assert_eq!(
            pl303.len(),
            1,
            "cross-file role conflict should emit exactly one PL303; got: {pl303:?}"
        );
        let msg = &pl303[0].message;
        assert!(
            msg.contains("shared_method"),
            "PL303 message should name the conflicting method: {msg}"
        );
        Ok(())
    }

    // ── Test 2: consumed_role_names extracts role names from `with` clause ───

    #[test]
    fn consumed_role_names_extracts_with_clause_roles() {
        let source = r#"
package MyApp::Widget;
use Moo;
with 'MyApp::Printable', 'MyApp::Serializable';
1;
"#;
        let output = perl_parser::Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);

        let roles = consumed_role_names(&ast);
        assert!(
            roles.contains(&"MyApp::Printable".to_string()),
            "consumed_role_names must extract MyApp::Printable; got: {roles:?}"
        );
        assert!(
            roles.contains(&"MyApp::Serializable".to_string()),
            "consumed_role_names must extract MyApp::Serializable; got: {roles:?}"
        );
    }

    // ── Test 3: no roles consumed → fast path, no graph built ────────────────

    #[test]
    fn consumed_role_names_empty_for_plain_package() {
        let source = r#"
package MyApp::Plain;
use strict;
sub hello { 1 }
1;
"#;
        let output = perl_parser::Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);

        let roles = consumed_role_names(&ast);
        assert!(
            roles.is_empty(),
            "package with no `with` clause should return empty roles; got: {roles:?}"
        );
    }

    // ── Test 4: unindexed cross-file role stays conservative (no false PL303) ─

    #[test]
    fn unindexed_cross_file_role_is_conservative() -> TestResult {
        // Only the consumer is indexed. The role file is absent from the index,
        // so the scoped graph has no edges for it.  The conservative behaviour is
        // to emit NO PL303 for a role we can't inspect.
        let consumer_uri = "file:///lib/App/NeedsExternal.pm";
        let consumer_src = r#"
package App::NeedsExternal;
use strict;
use warnings;
use Moo;
with 'External::Role::NotIndexed';
1;
"#;

        let index = build_index(&[(consumer_uri, consumer_src)])?;
        let diags = diagnostics_for_consumer(&index, consumer_uri, consumer_src);
        let pl303 = pl303_diags(&diags);

        assert!(pl303.is_empty(), "unindexed role must not produce false PL303; got: {pl303:?}");
        Ok(())
    }

    // ── Test 5: scoped graph construction succeeds for a simple role ──────────

    #[test]
    fn build_role_scoped_package_graph_succeeds_for_indexed_role() -> TestResult {
        let role_uri = "file:///lib/My/SimpleRole.pm";
        let role_src = r#"
package My::SimpleRole;
use Moo::Role;
sub do_thing { 1 }
1;
"#;
        let consumer_uri = "file:///lib/My/Consumer.pm";
        let consumer_src = r#"
package My::Consumer;
use Moo;
with 'My::SimpleRole';
1;
"#;

        let index = build_index(&[(role_uri, role_src), (consumer_uri, consumer_src)])?;

        let output = perl_parser::Parser::new(consumer_src).parse_with_recovery();
        let ast = Arc::new(output.ast);
        let consumed_roles = consumed_role_names(&ast);

        assert!(
            !consumed_roles.is_empty(),
            "consumer_src must have at least one role; got: {consumed_roles:?}"
        );

        // Build the scoped graph — must complete without panic.
        let _graph = build_role_scoped_package_graph(&index, &consumed_roles, consumer_uri);
        Ok(())
    }
}
