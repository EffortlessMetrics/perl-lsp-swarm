//! End-to-end proof of the #11918 production cut.
//!
//! For one exact source/generation, the checked built-in overlap observations
//! join the native candidates in ONE normalization call, so a reviewed
//! core/native alias pair becomes exactly one logical product row before LSP
//! projection. Before this cut, the pull transport showed both spellings
//! separately (the push transport only collapsed them via the #5088 XOR
//! coincidence dedup).

use lsp_types::{NumberOrString, Uri};
use perl_lsp::features::diagnostics::{PullDiagnosticsContext, PullDiagnosticsProvider};

fn items_from_report(
    report: lsp_types::DocumentDiagnosticReport,
) -> Result<Vec<lsp_types::Diagnostic>, Box<dyn std::error::Error>> {
    match report {
        lsp_types::DocumentDiagnosticReport::Full(full) => {
            Ok(full.full_document_diagnostic_report.items)
        }
        lsp_types::DocumentDiagnosticReport::Unchanged(_) => {
            Err("expected Full report, got Unchanged".into())
        }
    }
}

fn items_with_code<'a>(
    items: &'a [lsp_types::Diagnostic],
    code: &str,
) -> Vec<&'a lsp_types::Diagnostic> {
    items
        .iter()
        .filter(|diagnostic| {
            matches!(&diagnostic.code, Some(NumberOrString::String(value)) if value == code)
        })
        .collect()
}

#[test]
fn exact_system_document_yields_one_merged_product_row() -> Result<(), Box<dyn std::error::Error>> {
    let uri: Uri = "file:///cut_system_doc.pl".parse()?;
    let content = "system('ls -la');\n";

    let provider = PullDiagnosticsProvider::new();
    let items = items_from_report(provider.get_document_diagnostics_with_context(
        &uri,
        content,
        None,
        &PullDiagnosticsContext::new(),
        None,
    ))?;

    let pl603_rows = items_with_code(&items, "PL603");
    assert_eq!(
        pl603_rows.len(),
        1,
        "exactly one logical PL603 product row may exist after the cut: {items:#?}"
    );
    assert!(
        items_with_code(&items, "native.security.system_exec").is_empty(),
        "the native spelling must survive only as a contributor, not a second row: {items:#?}"
    );

    // Built-in origin wins presentation precedence: the merged row carries
    // the core emitter's message, and the matched declarations project to the
    // same LSP warning scale the ordinary diagnostic always had.
    let row = pl603_rows[0];
    assert!(
        row.message.contains("system() executes a shell command"),
        "merged presentation must keep the built-in emitter message: {}",
        row.message
    );
    assert_eq!(row.severity, Some(lsp_types::DiagnosticSeverity::WARNING));
    Ok(())
}

#[test]
fn legacy_engine_keeps_the_ordinary_core_row_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let uri: Uri = "file:///cut_legacy_doc.pl".parse()?;
    let content = "system('ls -la');\n";

    let mut context = PullDiagnosticsContext::new();
    context.critic_engine = perl_lsp_rs_core::config::CriticEngine::Legacy;
    assert_eq!(context.critic_engine, perl_lsp_rs_core::config::CriticEngine::Legacy);

    let provider = PullDiagnosticsProvider::new();
    let items = items_from_report(
        provider.get_document_diagnostics_with_context(&uri, content, None, &context, None),
    )?;

    assert_eq!(
        items_with_code(&items, "PL603").len(),
        1,
        "core-only control remains one valid row: {items:#?}"
    );
    Ok(())
}

#[test]
fn excluding_the_alias_spelling_leaves_the_policy_filtered_behavior_intact()
-> Result<(), Box<dyn std::error::Error>> {
    let uri: Uri = "file:///cut_exclude_doc.pl".parse()?;
    let content = "system('ls -la');\n";

    let mut context = PullDiagnosticsContext::new();
    context.native_critic_exclude = vec!["PL603".to_string()];

    let provider = PullDiagnosticsProvider::new();
    let items = items_from_report(
        provider.get_document_diagnostics_with_context(&uri, content, None, &context, None),
    )?;

    // The merged row is policy-filtered, so nothing is promoted and the
    // ordinary core diagnostic stands exactly as before the cut existed.
    assert_eq!(
        items_with_code(&items, "PL603").len(),
        1,
        "policy-filtered merges must not lose the ordinary diagnostic: {items:#?}"
    );
    assert!(
        items_with_code(&items, "native.security.system_exec").is_empty(),
        "an excluded alias pair must not leak the native spelling: {items:#?}"
    );
    Ok(())
}

#[test]
fn backtick_and_readpipe_retain_separate_canonical_rows() -> Result<(), Box<dyn std::error::Error>>
{
    let uri: Uri = "file:///cut_backtick_readpipe.pl".parse()?;
    let content = "my $tick = `ls -la`;\nmy $pipe = readpipe('ls -la');\n";

    let provider = PullDiagnosticsProvider::new();
    let items = items_from_report(provider.get_document_diagnostics_with_context(
        &uri,
        content,
        None,
        &PullDiagnosticsContext::new(),
        None,
    ))?;

    assert_eq!(
        items_with_code(&items, "PL601").len(),
        1,
        "backtick keeps its own canonical finding: {items:#?}"
    );
    assert_eq!(
        items_with_code(&items, "PL606").len(),
        1,
        "readpipe keeps its own canonical finding: {items:#?}"
    );
    assert!(
        items_with_code(&items, "native.security.backtick_exec").is_empty()
            && items_with_code(&items, "native.security.qx_readpipe").is_empty(),
        "native siblings must remain contributors only: {items:#?}"
    );
    Ok(())
}

#[test]
fn exact_qx_document_yields_one_merged_pl601_row() -> Result<(), Box<dyn std::error::Error>> {
    let uri: Uri = "file:///cut_qx_doc.pl".parse()?;
    let content = "my $out = qx(ls -la);\n";

    let provider = PullDiagnosticsProvider::new();
    let items = items_from_report(provider.get_document_diagnostics_with_context(
        &uri,
        content,
        None,
        &PullDiagnosticsContext::new(),
        None,
    ))?;

    assert_eq!(
        items_with_code(&items, "PL601").len(),
        1,
        "exactly one logical PL601 qx row may exist after the cut: {items:#?}"
    );
    assert!(
        items_with_code(&items, "native.security.qx_readpipe").is_empty(),
        "the native qx spelling must survive only as a contributor: {items:#?}"
    );
    Ok(())
}
