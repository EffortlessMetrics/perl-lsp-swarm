//! Acceptance tests for [#1761](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1761):
//! graceful degradation for string eval, `do EXPR`, and typeglob boundaries.

use perl_parser_core::Parser;
use perl_semantic_facts::{OccurrenceFact, OccurrenceKind, Provenance};
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn index_temp_source(label: &str, rel: &str, source: &str) -> Result<(WorkspaceIndex, String)> {
    let root = std::env::temp_dir().join(format!("issue-1761-{label}"));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, source)?;
    let uri = url::Url::from_file_path(&path)
        .map_err(|()| format!("fixture path cannot become file URI: {}", path.display()))?;
    let uri_string = uri.to_string();
    let index = WorkspaceIndex::new();
    index.index_initial_file(uri, source.to_string())?;
    Ok((index, uri_string))
}

fn byte_offset_in(source: &str, needle: &str) -> Result<u32> {
    let start =
        source.find(needle).ok_or_else(|| format!("needle {needle:?} not found in source"))?;
    u32::try_from(start).map_err(|_| "source offset exceeds u32".into())
}

/// [`WorkspaceIndex::with_semantic_queries_for_uri`] wraps the callback result in
/// `Option`; flatten once so callers test the query outcome, not index presence.
fn dynamic_boundary_at_indexed(
    index: &WorkspaceIndex,
    uri: &str,
    offset: u32,
    symbol: Option<&str>,
) -> Option<OccurrenceFact> {
    index
        .with_semantic_queries_for_uri(uri, |file_id, queries| {
            queries.dynamic_boundary_at(file_id, offset, symbol)
        })
        .flatten()
}

#[test]
fn issue_1761_eval_string_parses_and_publishes_dynamic_boundary() -> Result<()> {
    let source = "package App;\neval \"my \\$x = 1\";\n1;\n";
    let parse = Parser::new(source).parse_with_recovery();
    assert!(
        parse.diagnostics.is_empty(),
        "string eval should parse cleanly: {:?}",
        parse.diagnostics
    );

    let (index, uri) = index_temp_source("eval", "lib/App.pm", source)?;
    let offset = byte_offset_in(source, "my")?;
    let covered = dynamic_boundary_at_indexed(&index, &uri, offset, None);
    assert!(covered.is_some(), "string eval span should publish a dynamic boundary occurrence");
    Ok(())
}

#[test]
fn issue_1761_eval_variable_operand_is_not_boundary_coverage() -> Result<()> {
    let source = "package App;\neval $missing;\n1;\n";
    let (index, uri) = index_temp_source("eval-var", "lib/App.pm", source)?;
    let offset = byte_offset_in(source, "$missing")?;
    let covered = dynamic_boundary_at_indexed(&index, &uri, offset, Some("missing"));
    assert!(
        covered.is_none(),
        "variable operand of expression-form eval must not publish entity-less boundary coverage"
    );
    Ok(())
}

#[test]
fn issue_1761_do_file_operand_is_not_boundary_coverage() -> Result<()> {
    let source = "package App;\nmy $file = 'config.pl';\ndo $file;\n1;\n";
    let (index, uri) = index_temp_source("do-operand", "lib/App.pm", source)?;
    let do_operand = source.find("do $file").ok_or("do $file operand not found in fixture")?;
    let offset = u32::try_from(do_operand + "do ".len()).map_err(|_| "offset exceeds u32")?;
    let covered = dynamic_boundary_at_indexed(&index, &uri, offset, Some("file"));
    assert!(
        covered.is_none(),
        "variable operand of do EXPR must not publish entity-less boundary coverage"
    );
    Ok(())
}

#[test]
fn issue_1761_do_file_parses_and_publishes_dynamic_boundary() -> Result<()> {
    let source = "package App;\nmy $file = 'config.pl';\ndo $file;\n1;\n";
    let parse = Parser::new(source).parse_with_recovery();
    assert!(parse.diagnostics.is_empty(), "do EXPR should parse cleanly: {:?}", parse.diagnostics);

    let (index, uri) = index_temp_source("do-file", "lib/App.pm", source)?;
    let offset = byte_offset_in(source, "do")?;
    let covered = dynamic_boundary_at_indexed(&index, &uri, offset, None);
    assert!(
        covered.is_some(),
        "do EXPR should publish a dynamic boundary for the file-load expression"
    );
    Ok(())
}

#[test]
fn issue_1761_typeglob_assignment_publishes_dynamic_boundary() -> Result<()> {
    let source = "package App;\nno strict 'refs';\n*alias = \\&orig;\n1;\n";
    let (index, uri) = index_temp_source("typeglob", "lib/App.pm", source)?;
    let shard =
        index.file_fact_shard(&uri).ok_or("missing fact shard after indexing typeglob fixture")?;
    let has_typeglob_boundary = shard.occurrences.iter().any(|occ| {
        occ.provenance == Provenance::DynamicBoundary
            && matches!(
                occ.kind,
                OccurrenceKind::DynamicBoundary | OccurrenceKind::TypeglobReference
            )
    });
    assert!(
        has_typeglob_boundary,
        "typeglob assignment should register dynamic-boundary evidence: {:?}",
        shard.occurrences
    );
    Ok(())
}
