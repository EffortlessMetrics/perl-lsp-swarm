//! Integration tests for PL303 cross-file role-conflict detection (#4497).
//!
//! These tests exercise the full production diagnostics path, closing the
//! **Reachable** axis for issue #4497:
//!
//!   `WorkspaceIndex::index_file` (two+ files)
//!   → `with_semantic_queries_for_uri` → `WorkspaceSemanticQueries::with_package_graph`
//!   → `DiagnosticsProvider::get_diagnostics_with_path_and_semantics`
//!   → `check_role_conflicts` (via `transitive_role_methods` scanning fact shards)
//!   → PL303
//!
//! Unit tests in `role_conflicts.rs` verify the lint logic with a manually-wired
//! resolver. These tests verify the resolver is **reachable** from the live server
//! diagnostics path when roles are defined in separate files.
//!
//! # Coverage
//!
//! 1. Cross-file direct conflict → PL303 fires (roles in separate files).
//! 2. Consumer overrides the conflicting method → PL303 suppressed.
//! 3. Diamond composition (roles share a common ancestor role, neither defines
//!    the method directly) → no PL303 (conservative: without ComposesRole
//!    traversal, roles appear to provide no method → no false positive).
//! 4. External / unresolved role not indexed → conservative, no PL303.
//! 5. Non-overlapping methods in separate-file roles → no PL303.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::DiagnosticsProvider;
use perl_parser::{ParseOutput, Parser};
use perl_workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ── helpers ──────────────────────────────────────────────────────────────────

fn file_url(path: &str) -> Result<url::Url> {
    Ok(url::Url::parse(&format!("file://{path}"))?)
}

/// Index `files` into a fresh `WorkspaceIndex`, then run diagnostics on the
/// `consumer_path` file via the real `WorkspaceSemanticQueries` path.
///
/// Mirrors the live push-diagnostics flow in `runtime/diagnostics.rs`:
/// `workspace_index.with_semantic_queries_for_uri(uri, |file_id, queries| {
///     provider.get_diagnostics_with_path_and_semantics(...)
/// })`.
fn workspace_diagnostics(
    files: &[(&str, &str)],
    consumer_path: &str,
) -> Result<Vec<perl_lsp_rs_core::providers::diagnostics::Diagnostic>> {
    let index = WorkspaceIndex::new();
    for (path, source) in files {
        index.index_file(file_url(path)?, (*source).to_string())?;
    }

    let consumer_source = files
        .iter()
        .find(|(p, _)| *p == consumer_path)
        .map(|(_, s)| *s)
        .ok_or("consumer_path not found in files list")?;

    let consumer_uri = format!("file://{consumer_path}");

    let ParseOutput { ast: raw_ast, diagnostics: parse_errors, .. } =
        Parser::new(consumer_source).parse_with_recovery();
    let ast = Arc::new(raw_ast);
    let provider = DiagnosticsProvider::new(&ast, consumer_source.to_string());

    index
        .with_semantic_queries_for_uri(&consumer_uri, |file_id, queries| {
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
        .ok_or_else(|| format!("consumer URI {consumer_uri} was not indexed").into())
}

fn pl303_codes(
    diags: &[perl_lsp_rs_core::providers::diagnostics::Diagnostic],
) -> Vec<&perl_lsp_rs_core::providers::diagnostics::Diagnostic> {
    diags.iter().filter(|d| d.code.as_deref() == Some("PL303")).collect()
}

// ── test 1: cross-file direct conflict fires PL303 ───────────────────────────

/// Two roles in separate files both provide `shared_method`; a class consumes
/// both.  PL303 must fire through the production diagnostics path.
#[test]
fn cross_file_role_conflict_fires_pl303() -> Result<()> {
    const ROLE_A: &str = r"
package MyApp::RoleA;
use Moo::Role;
sub shared_method { 'from A' }
1;
";

    const ROLE_B: &str = r"
package MyApp::RoleB;
use Moo::Role;
sub shared_method { 'from B' }
1;
";

    const CONSUMER: &str = r"
package MyApp::Consumer;
use Moo;
with 'MyApp::RoleA', 'MyApp::RoleB';
1;
";

    let diags = workspace_diagnostics(
        &[("/test/RoleA.pm", ROLE_A), ("/test/RoleB.pm", ROLE_B), ("/test/Consumer.pm", CONSUMER)],
        "/test/Consumer.pm",
    )?;

    let pl303 = pl303_codes(&diags);
    assert!(
        !pl303.is_empty(),
        "cross-file conflict (shared_method in RoleA + RoleB) must produce PL303; \
         all diags: {diags:?}"
    );

    let msg = &pl303[0].message;
    assert!(
        msg.contains("shared_method"),
        "PL303 message must name the conflicting method; got: {msg}"
    );
    assert!(
        msg.contains("MyApp::Consumer"),
        "PL303 message must name the consuming class; got: {msg}"
    );

    Ok(())
}

// ── test 2: consumer override suppresses PL303 ───────────────────────────────

/// When the consuming class defines the conflicting method itself, no PL303
/// must be emitted — even when the roles are in separate files.
#[test]
fn consumer_override_suppresses_cross_file_pl303() -> Result<()> {
    const ROLE_A: &str = r"
package MyApp::OverrideRoleA;
use Moo::Role;
sub colliding { 'A' }
1;
";

    const ROLE_B: &str = r"
package MyApp::OverrideRoleB;
use Moo::Role;
sub colliding { 'B' }
1;
";

    // Consumer defines `colliding` itself — conflict is resolved.
    const CONSUMER: &str = r"
package MyApp::OverrideConsumer;
use Moo;
with 'MyApp::OverrideRoleA', 'MyApp::OverrideRoleB';
sub colliding { 'mine' }
1;
";

    let diags = workspace_diagnostics(
        &[
            ("/test/OverrideRoleA.pm", ROLE_A),
            ("/test/OverrideRoleB.pm", ROLE_B),
            ("/test/OverrideConsumer.pm", CONSUMER),
        ],
        "/test/OverrideConsumer.pm",
    )?;

    let pl303 = pl303_codes(&diags);
    assert!(
        pl303.is_empty(),
        "consumer defining the conflicting method must suppress PL303; \
         got: {pl303:?}"
    );

    Ok(())
}

// ── test 3: diamond composition is conservative — no false positive ───────────

/// BaseRole defines `diamond_method`.  RoleC and RoleD each compose BaseRole
/// but do not define the method themselves.  Consumer composes RoleC and RoleD.
///
/// Without full `ComposesRole` traversal in the workspace package graph, neither
/// RoleC nor RoleD appear to provide `diamond_method` directly (their fact
/// shards have no subroutine of that name), so PL303 must NOT fire.
/// This is the correct conservative behaviour: no false positive on diamonds.
#[test]
fn diamond_composition_does_not_fire_pl303() -> Result<()> {
    const BASE_ROLE: &str = r"
package MyApp::Diamond::Base;
use Moo::Role;
sub diamond_method { 'base' }
1;
";

    // RoleC composes Base but adds no method of its own.
    const ROLE_C: &str = r"
package MyApp::Diamond::RoleC;
use Moo::Role;
with 'MyApp::Diamond::Base';
1;
";

    // RoleD composes Base but adds no method of its own.
    const ROLE_D: &str = r"
package MyApp::Diamond::RoleD;
use Moo::Role;
with 'MyApp::Diamond::Base';
1;
";

    const CONSUMER: &str = r"
package MyApp::Diamond::Consumer;
use Moo;
with 'MyApp::Diamond::RoleC', 'MyApp::Diamond::RoleD';
1;
";

    let diags = workspace_diagnostics(
        &[
            ("/test/DiamondBase.pm", BASE_ROLE),
            ("/test/DiamondRoleC.pm", ROLE_C),
            ("/test/DiamondRoleD.pm", ROLE_D),
            ("/test/DiamondConsumer.pm", CONSUMER),
        ],
        "/test/DiamondConsumer.pm",
    )?;

    let pl303 = pl303_codes(&diags);
    assert!(
        pl303.is_empty(),
        "diamond composition (RoleC and RoleD both composed from Base) \
         must not fire PL303; got: {pl303:?}"
    );

    Ok(())
}

// ── test 4: external/unindexed role stays conservative ───────────────────────

/// A consumer composes an external role (not indexed in the workspace) alongside
/// a local role.  PL303 must NOT fire: we cannot know whether the external role
/// provides a conflicting method, so we stay conservative.
#[test]
fn unindexed_external_role_stays_conservative() -> Result<()> {
    const LOCAL_ROLE: &str = r"
package MyApp::External::LocalRole;
use Moo::Role;
sub some_method { 'local' }
1;
";

    // ExternalRole from CPAN is not indexed — only LocalRole is.
    const CONSUMER: &str = r"
package MyApp::External::Consumer;
use Moo;
with 'CPAN::SomeExternalRole', 'MyApp::External::LocalRole';
1;
";

    let diags = workspace_diagnostics(
        &[("/test/LocalRole.pm", LOCAL_ROLE), ("/test/ExternalConsumer.pm", CONSUMER)],
        "/test/ExternalConsumer.pm",
    )?;

    let pl303 = pl303_codes(&diags);
    assert!(
        pl303.is_empty(),
        "unindexed external role must not produce a speculative PL303; \
         got: {pl303:?}"
    );

    Ok(())
}

// ── test 5: non-overlapping methods in separate files — no PL303 ─────────────

/// Two roles in separate files each provide a different method; consumer composes
/// both.  No method overlap → no PL303.
#[test]
fn non_overlapping_cross_file_roles_no_pl303() -> Result<()> {
    const ROLE_A: &str = r"
package MyApp::Distinct::RoleA;
use Moo::Role;
sub method_a { 'A' }
1;
";

    const ROLE_B: &str = r"
package MyApp::Distinct::RoleB;
use Moo::Role;
sub method_b { 'B' }
1;
";

    const CONSUMER: &str = r"
package MyApp::Distinct::Consumer;
use Moo;
with 'MyApp::Distinct::RoleA', 'MyApp::Distinct::RoleB';
1;
";

    let diags = workspace_diagnostics(
        &[
            ("/test/DistinctRoleA.pm", ROLE_A),
            ("/test/DistinctRoleB.pm", ROLE_B),
            ("/test/DistinctConsumer.pm", CONSUMER),
        ],
        "/test/DistinctConsumer.pm",
    )?;

    let pl303 = pl303_codes(&diags);
    assert!(
        pl303.is_empty(),
        "distinct methods across separate-file roles must not fire PL303; \
         got: {pl303:?}"
    );

    Ok(())
}
