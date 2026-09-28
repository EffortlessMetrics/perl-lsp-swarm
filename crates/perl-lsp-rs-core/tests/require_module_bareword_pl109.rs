//! Public diagnostic boundary for #16670: legal `require` module barewords
//! must not emit PL109, while the same spelling in expression position must.

use std::sync::Arc;

use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_lsp_rs_core::tooling::perl_critic::{
    CriticConfig, CriticContext, NativeCriticProfile, NativeCriticRegistry,
};
use perl_parser::Parser;

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn pl109_for<'a>(diags: &'a [Diagnostic], name: &str) -> Vec<&'a Diagnostic> {
    diags
        .iter()
        .filter(|diag| {
            diag.code.as_deref() == Some(DiagnosticCode::UnquotedBareword.as_str())
                && diag.message.contains(name)
        })
        .collect()
}

fn native_unquoted_names(source: &str) -> Vec<String> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = output.ast;
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    NativeCriticRegistry::for_profile(NativeCriticProfile::Strict)
        .check(&ctx)
        .into_iter()
        .filter(|finding| finding.rule_id == "native.syntax.unquoted_bareword")
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn require_dbi_does_not_emit_pl109() {
    let source = "use strict;\nrequire DBI;\n";
    let diags = diagnostics_for(source);
    assert!(pl109_for(&diags, "DBI").is_empty(), "require DBI must not emit PL109: {diags:?}");
    assert!(
        native_unquoted_names(source).iter().all(|message| !message.contains("DBI")),
        "native unquoted-bareword rule must also spare the require operand"
    );
}

#[test]
fn expression_dbi_emits_pl109_with_code_severity_and_token_range() {
    let source = "use strict;\nmy $x = DBI;\n";
    let diags = diagnostics_for(source);
    let hits = pl109_for(&diags, "DBI");
    assert_eq!(hits.len(), 1, "expression DBI must emit exactly one PL109: {diags:?}");
    let hit = hits[0];
    assert_eq!(hit.code.as_deref(), Some("PL109"));
    assert_eq!(hit.severity, DiagnosticSeverity::Error);
    assert_eq!(source.get(hit.range.0..hit.range.1), Some("DBI"));
    assert!(hit.message.contains("DBI"));
    assert!(hit.message.contains("use strict"));
}

#[test]
fn editing_require_into_expression_and_back_toggles_pl109() {
    let require_form = "use strict;\nrequire DBI;\n";
    let expression_form = "use strict;\nmy $x = DBI;\n";

    assert!(pl109_for(&diagnostics_for(require_form), "DBI").is_empty());
    let after_edit = diagnostics_for(expression_form);
    let hits = pl109_for(&after_edit, "DBI");
    assert_eq!(hits.len(), 1, "editing require DBI into my $x = DBI must produce PL109");
    assert_eq!(expression_form.get(hits[0].range.0..hits[0].range.1), Some("DBI"));
    assert!(
        pl109_for(&diagnostics_for(require_form), "DBI").is_empty(),
        "editing back to require DBI must clear PL109 without a process restart"
    );
}

#[test]
fn demo_database_pm_does_not_emit_pl109_for_dbi() {
    let source = include_str!("../../../demo_workspace/lib/Database.pm");
    let diags = diagnostics_for(source);
    assert!(
        pl109_for(&diags, "DBI").is_empty(),
        "demo Database.pm must not emit PL109 at DBI: {diags:?}"
    );
}

#[test]
fn nearby_expression_bareword_stays_diagnosed_beside_legal_require() {
    let source = "use strict;\nrequire DBI;\nmy $x = NearbyBare;\n";
    let diags = diagnostics_for(source);
    assert!(pl109_for(&diags, "DBI").is_empty(), "require operand must stay quiet: {diags:?}");
    let nearby = pl109_for(&diags, "NearbyBare");
    assert_eq!(nearby.len(), 1, "unrelated expression bareword must still emit PL109: {diags:?}");
    assert_eq!(source.get(nearby[0].range.0..nearby[0].range.1), Some("NearbyBare"));
}
