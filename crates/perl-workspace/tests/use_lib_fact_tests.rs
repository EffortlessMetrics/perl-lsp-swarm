//! Acceptance tests for `UseLibFact` extraction, storage, and query.
//!
//! Tests cover:
//! - Static literal path extraction (`use lib 'path'`)
//! - `no lib` emitting `is_active: false` facts
//! - Dynamic args (`use lib $var`, `use lib @dirs`) emitting no fact
//! - `qw(...)` list args emitting one fact per word
//! - Round-trip through `ImportExportIndex` storage and `use_lib_paths()` query
//! - End-to-end: `WorkspaceIndex::index_file` wires use-lib facts into
//!   `use_lib_paths()` query results

use perl_semantic_facts::{AnchorId, Confidence, FileId, Provenance, UseLibFact};
use perl_workspace::Parser;
use perl_workspace::semantic::imports::ImportExportIndex;
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::semantic::workspace_import_extractor::extract_use_lib_facts;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

// ── Parsing helper ──────────────────────────────────────────────────────────

fn parse_and_extract_use_lib(code: &str) -> Vec<UseLibFact> {
    let file_id = FileId(1);
    let mut parser = Parser::new(code);
    match parser.parse() {
        Ok(ast) => extract_use_lib_facts(&ast, file_id),
        Err(_) => Vec::new(),
    }
}

// ── Static literal tests ─────────────────────────────────────────────────────

#[test]
fn test_use_lib_static_literal_emits_active_fact() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib 'lib';");

    assert_eq!(facts.len(), 1, "expected exactly one UseLibFact");
    let fact = facts.first().ok_or("no fact")?;
    assert_eq!(fact.path, "lib");
    assert!(fact.is_active, "use lib should emit is_active: true");
    assert_eq!(fact.confidence, Confidence::High);
    assert_eq!(fact.provenance, Provenance::ExactAst);
    assert_eq!(fact.file_id, FileId(1));
    Ok(())
}

#[test]
fn test_use_lib_relative_path_emits_active_fact() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib '../lib';");

    assert_eq!(facts.len(), 1, "expected exactly one UseLibFact");
    let fact = facts.first().ok_or("no fact")?;
    assert_eq!(fact.path, "../lib");
    assert!(fact.is_active);
    assert_eq!(fact.confidence, Confidence::High);
    assert_eq!(fact.provenance, Provenance::ExactAst);
    Ok(())
}

// ── no lib cancellation tests ─────────────────────────────────────────────────

#[test]
fn test_no_lib_emits_inactive_fact() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("no lib 'x';");

    assert_eq!(facts.len(), 1, "expected exactly one UseLibFact");
    let fact = facts.first().ok_or("no fact")?;
    assert_eq!(fact.path, "x");
    assert!(!fact.is_active, "no lib should emit is_active: false");
    assert_eq!(fact.confidence, Confidence::High);
    assert_eq!(fact.provenance, Provenance::ExactAst);
    Ok(())
}

#[test]
fn test_use_lib_then_no_lib_emits_two_facts() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib '../lib'; no lib '../lib';");

    assert_eq!(facts.len(), 2, "expected two UseLibFacts (per-statement, not net state)");

    let active_fact = facts.iter().find(|f| f.is_active).ok_or("no active fact")?;
    let inactive_fact = facts.iter().find(|f| !f.is_active).ok_or("no inactive fact")?;

    assert_eq!(active_fact.path, "../lib");
    assert_eq!(inactive_fact.path, "../lib");
    Ok(())
}

// ── Dynamic arg tests ─────────────────────────────────────────────────────────

#[test]
fn test_use_lib_dynamic_var_skipped() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib $path;");
    assert!(facts.is_empty(), "dynamic $var arg should emit no UseLibFact");
    Ok(())
}

#[test]
fn test_use_lib_dynamic_array_skipped() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib @dirs;");
    assert!(facts.is_empty(), "dynamic @array arg should emit no UseLibFact");
    Ok(())
}

#[test]
fn test_use_lib_multiple_dynamic_args_skipped() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib $dir; use lib @paths;");
    assert!(facts.is_empty(), "all dynamic args should emit no UseLibFact");
    Ok(())
}

// ── qw() list tests ──────────────────────────────────────────────────────────

#[test]
fn test_use_lib_qw_list_emits_one_fact_per_word() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib qw(lib ../lib);");

    assert_eq!(facts.len(), 2, "qw(lib ../lib) should emit two UseLibFacts");
    let paths: Vec<&str> = facts.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"lib"), "should contain 'lib'");
    assert!(paths.contains(&"../lib"), "should contain '../lib'");
    for fact in &facts {
        assert!(fact.is_active, "all qw() entries should be active");
        assert_eq!(fact.confidence, Confidence::High);
        assert_eq!(fact.provenance, Provenance::ExactAst);
    }
    Ok(())
}

// ── Double-quoted string tests ────────────────────────────────────────────────

#[test]
fn test_use_lib_double_quoted_string() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib(r#"use lib "../lib";"#);

    assert_eq!(facts.len(), 1);
    let fact = facts.first().ok_or("no fact")?;
    assert_eq!(fact.path, "../lib");
    assert!(fact.is_active);
    Ok(())
}

// ── Storage round-trip tests ──────────────────────────────────────────────────

#[test]
fn test_import_export_index_use_lib_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = ImportExportIndex::new();
    let file_id = FileId(42);
    let facts = vec![
        UseLibFact::new(
            "lib".to_string(),
            true,
            file_id,
            Some(AnchorId(100)),
            Provenance::ExactAst,
            Confidence::High,
        ),
        UseLibFact::new(
            "../lib".to_string(),
            true,
            file_id,
            Some(AnchorId(200)),
            Provenance::ExactAst,
            Confidence::High,
        ),
    ];

    index.add_file_use_lib("file:///lib/Main.pm", file_id, facts.clone());

    let result = index.get_use_lib_for_file(file_id);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].path, "lib");
    assert_eq!(result[1].path, "../lib");
    Ok(())
}

#[test]
fn test_import_export_index_use_lib_remove() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = ImportExportIndex::new();
    let file_id = FileId(42);
    let facts = vec![UseLibFact::new(
        "lib".to_string(),
        true,
        file_id,
        None,
        Provenance::ExactAst,
        Confidence::High,
    )];

    index.add_file_use_lib("file:///lib/Main.pm", file_id, facts);
    assert_eq!(index.get_use_lib_for_file(file_id).len(), 1);

    index.remove_file_use_lib("file:///lib/Main.pm");
    assert!(index.get_use_lib_for_file(file_id).is_empty());
    Ok(())
}

#[test]
fn test_import_export_index_use_lib_unknown_file_returns_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let index = ImportExportIndex::new();
    let result = index.get_use_lib_for_file(FileId(999));
    assert!(result.is_empty());
    Ok(())
}

#[test]
fn test_import_export_index_use_lib_remove_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = ImportExportIndex::new();
    let file_id = FileId(1);
    let facts = vec![UseLibFact::new(
        "lib".to_string(),
        true,
        file_id,
        None,
        Provenance::ExactAst,
        Confidence::High,
    )];
    index.add_file_use_lib("file:///lib/Main.pm", file_id, facts);
    index.remove_file_use_lib("file:///lib/Main.pm");
    // Second remove should be a no-op.
    index.remove_file_use_lib("file:///lib/Main.pm");
    assert!(index.get_use_lib_for_file(file_id).is_empty());
    Ok(())
}

// ── Anchor presence tests ─────────────────────────────────────────────────────

#[test]
fn test_use_lib_fact_has_anchor() -> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib 'lib';");
    let fact = facts.first().ok_or("no fact")?;
    // anchor_id should be Some (derived from start byte offset)
    assert!(fact.anchor_id.is_some(), "UseLibFact should have an anchor_id");
    Ok(())
}

// ── Mixed static/dynamic tests ────────────────────────────────────────────────

#[test]
fn test_use_lib_mixed_static_and_dynamic_only_static_emitted()
-> Result<(), Box<dyn std::error::Error>> {
    let facts = parse_and_extract_use_lib("use lib 'lib'; use lib $dir; use lib '../lib';");
    assert_eq!(facts.len(), 2, "only static args should emit facts");
    let paths: Vec<&str> = facts.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"lib"));
    assert!(paths.contains(&"../lib"));
    Ok(())
}

// ── End-to-end integration: WorkspaceIndex wiring ────────────────────────────

/// Proves that `WorkspaceIndex::index_file` wires `use lib` facts through
/// to `use_lib_paths()` in production.  This is the key requirement of #894:
/// indexing a real file containing `use lib '...'` must make those paths
/// available via the `SemanticQueries` interface without any manual wiring
/// by the caller.
#[test]
fn test_workspace_index_wires_use_lib_into_query() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///test_use_lib_wiring.pl";
    let code = "use lib 'lib';\nuse lib '../vendor';\nuse lib $dynamic;\n1;\n";

    index.index_file_str(uri, code).map_err(|e| e)?;

    let result = index
        .with_semantic_queries_for_uri(uri, |file_id, queries| queries.use_lib_paths(file_id))
        .ok_or("file was not indexed")?;

    // Static args only — $dynamic must be skipped.
    assert_eq!(result.len(), 2, "expected 2 use-lib facts (dynamic arg skipped), got {result:#?}");

    let paths: Vec<&str> = result.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"lib"), "should contain 'lib', got {paths:#?}");
    assert!(paths.contains(&"../vendor"), "should contain '../vendor', got {paths:#?}");

    // All facts should be active (use lib, not no lib).
    for fact in &result {
        assert!(fact.is_active, "all use lib facts should be active, got {fact:?}");
        assert_eq!(fact.provenance, Provenance::ExactAst);
        assert_eq!(fact.confidence, Confidence::High);
    }

    Ok(())
}

/// Proves that `remove_file` also cleans up use-lib facts so they do not
/// linger after the file is removed from the index.
#[test]
fn test_workspace_index_remove_file_clears_use_lib() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///test_use_lib_remove.pl";
    let code = "use lib 'lib';\n1;\n";

    index.index_file_str(uri, code).map_err(|e| e)?;

    // Verify facts are present after indexing.
    let before = index
        .with_semantic_queries_for_uri(uri, |file_id, queries| queries.use_lib_paths(file_id))
        .ok_or("file was not indexed")?;
    assert_eq!(before.len(), 1, "should have 1 use-lib fact before removal");

    index.remove_file(uri);

    // After removal, `with_semantic_queries_for_uri` returns None (file gone from shards).
    let after =
        index.with_semantic_queries_for_uri(uri, |file_id, queries| queries.use_lib_paths(file_id));
    assert!(after.is_none(), "queries for removed file should return None");

    Ok(())
}

/// Proves that re-indexing a file with different `use lib` paths replaces
/// the stale facts (incremental re-index correctness).
#[test]
fn test_workspace_index_reindex_replaces_use_lib_facts() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = "file:///test_use_lib_reindex.pl";

    // First index: single path.
    index.index_file_str(uri, "use lib 'old_path';\n1;\n").map_err(|e| e)?;

    let v1 = index
        .with_semantic_queries_for_uri(uri, |file_id, queries| queries.use_lib_paths(file_id))
        .ok_or("file not indexed (v1)")?;
    assert_eq!(v1.len(), 1);
    assert_eq!(v1.first().ok_or("no fact")?.path, "old_path");

    // Re-index with different path.
    index.index_file_str(uri, "use lib 'new_path';\n1;\n").map_err(|e| e)?;

    let v2 = index
        .with_semantic_queries_for_uri(uri, |file_id, queries| queries.use_lib_paths(file_id))
        .ok_or("file not indexed (v2)")?;
    assert_eq!(v2.len(), 1);
    assert_eq!(v2.first().ok_or("no fact")?.path, "new_path");

    Ok(())
}
