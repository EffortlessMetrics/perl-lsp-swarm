//! Falsifiers for the workspace-index memory denominator (#17198).

#![cfg(feature = "memory-profiling")]

use perl_semantic_facts::{
    AnchorId, Confidence, EntityFact, EntityId, EntityKind, FileId, ImportKind, ImportSpec,
    ImportSymbols, OccurrenceFact, OccurrenceId, OccurrenceKind, PackageEdge, PackageEdgeKind,
    Provenance,
};
use perl_symbol::SymbolKind;
use perl_workspace::position::{Position, Range};
use perl_workspace::workspace::memory::{
    MemoryEvidenceCompleteness, MemorySnapshot, WorkspaceIndexMemorySnapshotV2,
};
use perl_workspace::workspace::memory_weight::MemoryWeightError;
use perl_workspace::workspace::workspace_index::{FileFactShard, WorkspaceIndex, WorkspaceSymbol};
use perl_workspace::workspace_index::memory_falsifiers::{
    leak_fact_shard, leak_import_row, leak_package_graph_row, leak_search_index_bucket,
    leak_semantic_reference_row,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn empty_shard(uri: &str) -> FileFactShard {
    FileFactShard {
        source_uri: uri.to_string(),
        file_id: FileId(99),
        content_hash: 1,
        producer_schema_version: 1,
        anchors_hash: None,
        entities_hash: None,
        occurrences_hash: None,
        edges_hash: None,
        anchors: vec![],
        entities: vec![EntityFact {
            id: EntityId(1),
            kind: EntityKind::Subroutine,
            canonical_name: "Leak::Func".to_string(),
            anchor_id: Some(AnchorId(1)),
            scope_id: None,
            provenance: Provenance::SearchFallback,
            confidence: Confidence::Low,
        }],
        occurrences: vec![OccurrenceFact {
            id: OccurrenceId(1),
            kind: OccurrenceKind::Call,
            entity_id: Some(EntityId(1)),
            anchor_id: AnchorId(1),
            scope_id: None,
            provenance: Provenance::SearchFallback,
            confidence: Confidence::Low,
        }],
        edges: vec![],
    }
}

#[test]
fn falsifier_search_index_leak_increments_search_projection() -> TestResult {
    let index = WorkspaceIndex::new();
    let baseline = MemorySnapshot::capture(&index);
    leak_search_index_bucket(
        &index,
        "orphan_bucket".to_string(),
        WorkspaceSymbol {
            name: "orphan".to_string(),
            uri: "file:///orphan.pm".to_string(),
            qualified_name: None,
            kind: SymbolKind::Subroutine,
            range: Range {
                start: Position { line: 0, column: 0, byte: 0 },
                end: Position { line: 0, column: 0, byte: 0 },
            },
            documentation: None,
            container_name: None,
            has_body: false,
            workspace_folder_uri: None,
            is_lexical: false,
        },
    );
    let after = MemorySnapshot::capture(&index);
    assert!(
        after.v2.search_index.logical_bytes > baseline.v2.search_index.logical_bytes,
        "search_index leak must increase the search projection"
    );
    Ok(())
}

#[test]
fn falsifier_fact_shard_leak_increments_shard_projection() -> TestResult {
    let index = WorkspaceIndex::new();
    let baseline = MemorySnapshot::capture(&index);
    leak_fact_shard(&index, empty_shard("file:///leak.pm"));
    let after = MemorySnapshot::capture(&index);
    assert!(after.v2.fact_shards.logical_bytes > baseline.v2.fact_shards.logical_bytes);
    Ok(())
}

#[test]
fn falsifier_semantic_reference_leak_increments_reference_projection() -> TestResult {
    let index = WorkspaceIndex::new();
    let baseline = MemorySnapshot::capture(&index);
    leak_semantic_reference_row(&index, empty_shard("file:///ref-leak.pm"));
    let after = MemorySnapshot::capture(&index);
    assert!(
        after.v2.semantic_reference_index.logical_bytes
            > baseline.v2.semantic_reference_index.logical_bytes
    );
    Ok(())
}

#[test]
fn falsifier_import_leak_increments_import_projection() -> TestResult {
    let index = WorkspaceIndex::new();
    let baseline = MemorySnapshot::capture(&index);
    leak_import_row(
        &index,
        "file:///import-leak.pm",
        FileId(7),
        ImportSpec {
            module: "Leak::Import".to_string(),
            kind: ImportKind::Use,
            symbols: ImportSymbols::None,
            provenance: Provenance::ImportExportInference,
            confidence: Confidence::Medium,
            file_id: Some(FileId(7)),
            anchor_id: Some(AnchorId(1)),
            scope_id: None,
            span_start_byte: None,
        },
    );
    let after = MemorySnapshot::capture(&index);
    assert!(
        after.v2.semantic_import_export_index.logical_bytes
            > baseline.v2.semantic_import_export_index.logical_bytes
    );
    Ok(())
}

#[test]
fn falsifier_package_graph_leak_increments_graph_projection() -> TestResult {
    let index = WorkspaceIndex::new();
    let baseline = MemorySnapshot::capture(&index);
    leak_package_graph_row(
        &index,
        "file:///pkg-leak.pm",
        FileId(8),
        PackageEdge::new(
            "Leak::Child".to_string(),
            "Leak::Parent".to_string(),
            PackageEdgeKind::Inherits,
            Some(AnchorId(1)),
            Provenance::NameHeuristic,
            Confidence::Low,
        ),
    );
    let after = MemorySnapshot::capture(&index);
    assert!(
        after.v2.semantic_package_graph_index.logical_bytes
            > baseline.v2.semantic_package_graph_index.logical_bytes
    );
    Ok(())
}

#[test]
fn instrument_failed_snapshot_is_not_complete_and_totals_do_not_pass() {
    let failed = WorkspaceIndexMemorySnapshotV2::instrument_failed(MemoryWeightError::Overflow);
    assert!(!failed.is_evidence_complete());
    assert!(matches!(
        failed.completeness,
        MemoryEvidenceCompleteness::InstrumentFailed(MemoryWeightError::Overflow)
    ));
    assert!(failed.total_logical_retained_bytes().is_err());
}
