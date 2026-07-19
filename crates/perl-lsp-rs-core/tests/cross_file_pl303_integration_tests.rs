//! Integration tests: cross-file PL303 role-conflict detection via WorkspaceIndex.
//!
//! Verifies that the production diagnostics path (WorkspaceIndex →
//! `with_semantic_queries_for_uri` → `DiagnosticsProvider::
//! get_diagnostics_with_path_and_semantics`) fires PL303 for roles defined
//! in separate files, handles diamond composition correctly, and stays
//! conservative for unresolved roles.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

/// Index all `files` into a real `WorkspaceIndex`, then run the full
/// production diagnostics path for `consuming_uri` and return only PL303
/// diagnostics.
fn cross_file_pl303(files: &[(&str, &str)], consuming_uri: &str) -> Vec<Diagnostic> {
    let index = WorkspaceIndex::new();
    for (uri, source) in files {
        let _ = index.index_file_str(uri, source);
    }
    let consuming_source = match files.iter().find(|(u, _)| *u == consuming_uri) {
        Some((_, s)) => *s,
        None => return Vec::new(),
    };
    let output = Parser::new(consuming_source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    let provider = DiagnosticsProvider::new(&ast, consuming_source.to_string());
    index
        .with_semantic_queries_for_uri(consuming_uri, |file_id, queries| {
            provider.get_diagnostics_with_path_and_semantics(
                &ast,
                &output.diagnostics,
                consuming_source,
                None,
                &[],
                None,
                file_id,
                &queries,
            )
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("PL303"))
        .collect()
}

/// Two cross-file roles both defining the same method emit exactly one PL303.
#[test]
fn cross_file_roles_with_conflicting_methods_emit_pl303() {
    let files = &[
        ("file:///test/RoleA.pm", "package App::RoleA;\nuse Moo::Role;\nsub dispatch { 'A' }\n"),
        ("file:///test/RoleB.pm", "package App::RoleB;\nuse Moo::Role;\nsub dispatch { 'B' }\n"),
        (
            "file:///test/Consumer.pm",
            "package App::Consumer;\nuse Moo;\nwith 'App::RoleA', 'App::RoleB';\n",
        ),
    ];

    let diags = cross_file_pl303(files, "file:///test/Consumer.pm");

    assert_eq!(
        diags.len(),
        1,
        "two cross-file roles both defining `dispatch` should emit exactly one PL303: {diags:?}",
    );
    assert!(
        diags[0].message.contains("dispatch"),
        "PL303 message should name the conflicting method: {}",
        diags[0].message,
    );
}

/// Diamond composition — two roles both compose the same base role — resolves
/// `shared_method` to a single origin and must not emit PL303.
#[test]
fn diamond_composition_via_workspace_does_not_emit_pl303() {
    let files = &[
        (
            "file:///test/BaseRole.pm",
            "package App::BaseRole;\nuse Moo::Role;\nsub shared_method { 'base' }\n",
        ),
        ("file:///test/RoleA.pm", "package App::RoleA;\nuse Moo::Role;\nwith 'App::BaseRole';\n"),
        ("file:///test/RoleB.pm", "package App::RoleB;\nuse Moo::Role;\nwith 'App::BaseRole';\n"),
        (
            "file:///test/Consumer.pm",
            "package App::Consumer;\nuse Moo;\nwith 'App::RoleA', 'App::RoleB';\n",
        ),
    ];

    let diags = cross_file_pl303(files, "file:///test/Consumer.pm");

    assert!(
        diags.is_empty(),
        "diamond composition (same origin via App::BaseRole) should not emit PL303: {diags:?}",
    );
}

/// A role that was never indexed contributes no methods; the lint stays
/// conservative and must not emit PL303 for the unresolved side.
#[test]
fn unresolved_cross_file_role_stays_conservative() {
    let files = &[
        ("file:///test/RoleA.pm", "package App::RoleA;\nuse Moo::Role;\nsub process { 'A' }\n"),
        // App::RoleExternal is intentionally not indexed.
        (
            "file:///test/Consumer.pm",
            "package App::Consumer;\nuse Moo;\nwith 'App::RoleA', 'App::RoleExternal';\n",
        ),
    ];

    let diags = cross_file_pl303(files, "file:///test/Consumer.pm");

    assert!(diags.is_empty(), "unresolved cross-file role should not trigger PL303: {diags:?}",);
}

/// When the consumer class defines the conflicted method itself, the lint
/// treats the class's own definition as the resolution and must not emit PL303.
#[test]
fn consumer_override_suppresses_cross_file_pl303() {
    let files = &[
        ("file:///test/RoleA.pm", "package App::RoleA;\nuse Moo::Role;\nsub dispatch { 'A' }\n"),
        ("file:///test/RoleB.pm", "package App::RoleB;\nuse Moo::Role;\nsub dispatch { 'B' }\n"),
        (
            "file:///test/Consumer.pm",
            "package App::Consumer;\nuse Moo;\nwith 'App::RoleA', 'App::RoleB';\nsub dispatch { 'mine' }\n",
        ),
    ];

    let diags = cross_file_pl303(files, "file:///test/Consumer.pm");

    assert!(diags.is_empty(), "consumer overriding `dispatch` should suppress PL303: {diags:?}",);
}
