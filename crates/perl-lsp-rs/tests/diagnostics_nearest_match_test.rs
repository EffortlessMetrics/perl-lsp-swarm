use lsp_types::{DocumentDiagnosticReport, NumberOrString};
use perl_lsp::features::diagnostics::PullDiagnosticsProvider;

fn items_from_report(
    report: DocumentDiagnosticReport,
) -> Result<Vec<lsp_types::Diagnostic>, Box<dyn std::error::Error>> {
    match report {
        DocumentDiagnosticReport::Full(full) => Ok(full.full_document_diagnostic_report.items),
        DocumentDiagnosticReport::Unchanged(_) => {
            Err("expected Full diagnostic report, got Unchanged".into())
        }
    }
}

fn has_code(diag: &lsp_types::Diagnostic, code: &str) -> bool {
    matches!(&diag.code, Some(NumberOrString::String(s)) if s == code)
}

fn undeclared_variable_diagnostics(content: &str) -> Result<Vec<lsp_types::Diagnostic>, Box<dyn std::error::Error>> {
    let provider = PullDiagnosticsProvider::new();
    let uri: lsp_types::Uri = "file:///nearest_match_1770.pl".parse()?;
    let items = items_from_report(provider.get_document_diagnostics(&uri, content, None, None))?;
    Ok(items
        .into_iter()
        .filter(|d| has_code(d, "PL103"))
        .collect())
}

#[test]
fn test_undefined_variable_suggestion_single_match() -> Result<(), Box<dyn std::error::Error>> {
    let with_strict = "use strict;\nuse warnings;\nmy $count = 1;\nprint $cunt;\n";
    let strict_diags = undeclared_variable_diagnostics(with_strict)?;
    let strict_match = strict_diags.iter().find(|d| d.message.contains("$cunt"));
    let strict_diag = strict_match.ok_or_else(|| {
        format!(
            "expected PL103 for typo under strict; got: {strict_diags:#?}"
        )
    })?;

    let message = strict_diag.message.to_ascii_lowercase();
    let related = strict_diag
        .related_information
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry.message.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mentions_suggestion = message.contains("did you mean")
        || related.iter().any(|entry| entry.contains("did you mean"));
    assert!(
        mentions_suggestion && (message.contains("$count") || related.iter().any(|e| e.contains("$count"))),
        "strict diagnostic should suggest $count; message={:?} related={related:?}",
        strict_diag.message
    );

    let without_strict = "my $count = 1;\nprint $cunt;\n";
    let relaxed_diags = undeclared_variable_diagnostics(without_strict)?;
    assert!(
        relaxed_diags.is_empty(),
        "without use strict, undeclared-variable diagnostics should not fire: {relaxed_diags:#?}"
    );

    Ok(())
}
