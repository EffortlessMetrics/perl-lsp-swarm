//! Pull-diagnostic public boundary for #16670.
//!
//! Push diagnostics in this crate compose the same `DiagnosticsProvider`
//! mapping; this file proves the pull report agrees on absence/presence,
//! code, and message for the require-module operand vs expression control.

use lsp_types::{NumberOrString, Uri};
use perl_lsp::features::diagnostics::PullDiagnosticsProvider;

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

fn has_pl109_for(items: &[lsp_types::Diagnostic], name: &str) -> bool {
    items.iter().any(|diag| {
        matches!(&diag.code, Some(NumberOrString::String(code)) if code == "PL109")
            && diag.message.contains(name)
    })
}

fn pull(
    uri: &str,
    content: &str,
) -> Result<Vec<lsp_types::Diagnostic>, Box<dyn std::error::Error>> {
    let uri: Uri = uri.parse()?;
    items_from_report(
        PullDiagnosticsProvider::new().get_document_diagnostics(&uri, content, None, None),
    )
}

#[test]
fn pull_require_dbi_does_not_emit_pl109() -> Result<(), Box<dyn std::error::Error>> {
    let items = pull("file:///require_dbi.pl", "use strict;\nrequire DBI;\n")?;
    assert!(
        !has_pl109_for(&items, "DBI"),
        "pull diagnostics must not report require DBI as PL109: {items:#?}"
    );
    Ok(())
}

#[test]
fn pull_expression_dbi_emits_pl109() -> Result<(), Box<dyn std::error::Error>> {
    let items = pull("file:///expr_dbi.pl", "use strict;\nmy $x = DBI;\n")?;
    assert!(
        has_pl109_for(&items, "DBI"),
        "pull diagnostics must report expression DBI as PL109: {items:#?}"
    );
    Ok(())
}

#[test]
fn pull_edit_require_to_expression_and_back_toggles_pl109() -> Result<(), Box<dyn std::error::Error>>
{
    let uri = "file:///edit_dbi.pl";
    let require_form = "use strict;\nrequire DBI;\n";
    let expression_form = "use strict;\nmy $x = DBI;\n";

    assert!(!has_pl109_for(&pull(uri, require_form)?, "DBI"));
    assert!(has_pl109_for(&pull(uri, expression_form)?, "DBI"));
    assert!(
        !has_pl109_for(&pull(uri, require_form)?, "DBI"),
        "re-pulling require DBI must clear PL109"
    );
    Ok(())
}

#[test]
fn pull_demo_database_pm_does_not_emit_pl109_for_dbi() -> Result<(), Box<dyn std::error::Error>> {
    let source = include_str!("../../../demo_workspace/lib/Database.pm");
    let items = pull("file:///demo_workspace/lib/Database.pm", source)?;
    assert!(
        !has_pl109_for(&items, "DBI"),
        "demo Database.pm pull diagnostics must not emit PL109 for DBI: {items:#?}"
    );
    Ok(())
}

#[test]
fn pull_parenthesized_require_dbi_emits_pl109() -> Result<(), Box<dyn std::error::Error>> {
    let items = pull("file:///require_paren_dbi.pl", "use strict;\nrequire(DBI);\n")?;
    assert!(
        has_pl109_for(&items, "DBI"),
        "pull diagnostics must report require(DBI) as PL109: {items:#?}"
    );
    Ok(())
}
