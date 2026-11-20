//! Versioned complete workspace-index memory snapshot (denominator v2).
#![expect(
    missing_docs,
    reason = "policy:17198-memory-evidence: v2 snapshot field names are the receipt vocabulary"
)]

use super::memory_inventory::WORKSPACE_INDEX_RETAINED_INVENTORY;
use super::memory_weight::{
    CollectionWeight, MemoryWeightError, checked_add, hashmap_capacity_slack, string_logical,
    vec_capacity, vec_logical,
};
use super::workspace_index::{FileFactShard, WorkspaceSymbol};
use crate::workspace::document_store::DocumentStore;
use std::collections::HashMap;
use std::mem::size_of;

/// Schema identity for deterministic retained-weight receipts.
pub const WORKSPACE_INDEX_MEMORY_SCHEMA_V2: &str = "workspace-index-memory/v2";

/// Whether the snapshot covers every inventoried owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryEvidenceCompleteness {
    Complete,
    InstrumentFailed(MemoryWeightError),
}

/// Complete retained denominator for one live [`WorkspaceIndex`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIndexMemorySnapshotV2 {
    pub schema_id: &'static str,
    pub completeness: MemoryEvidenceCompleteness,
    pub file_count: usize,
    pub symbol_count: usize,
    pub file_projection: CollectionWeight,
    pub definition_symbols: CollectionWeight,
    pub search_index: CollectionWeight,
    pub global_references: CollectionWeight,
    pub fact_shards: CollectionWeight,
    pub semantic_reference_index: CollectionWeight,
    pub semantic_import_export_index: CollectionWeight,
    pub semantic_package_graph_index: CollectionWeight,
    pub document_source: CollectionWeight,
    pub document_line_geometry: CollectionWeight,
    pub document_map_capacity: CollectionWeight,
    pub workspace_folders: CollectionWeight,
    pub lifecycle_guards: CollectionWeight,
}

impl WorkspaceIndexMemorySnapshotV2 {
    /// Sum logical retained bytes across measured families (excludes transient owners).
    pub fn total_logical_retained_bytes(&self) -> Result<usize, MemoryWeightError> {
        if !matches!(self.completeness, MemoryEvidenceCompleteness::Complete) {
            return Err(MemoryWeightError::Overflow);
        }
        let families = [
            self.file_projection,
            self.definition_symbols,
            self.search_index,
            self.global_references,
            self.fact_shards,
            self.semantic_reference_index,
            self.semantic_import_export_index,
            self.semantic_package_graph_index,
            self.document_source,
            self.document_line_geometry,
        ];
        families.iter().try_fold(0usize, |acc, row| checked_add(acc, row.logical_bytes))
    }

    pub fn total_capacity_slack_bytes(&self) -> Result<usize, MemoryWeightError> {
        let families = [
            self.file_projection,
            self.definition_symbols,
            self.search_index,
            self.global_references,
            self.fact_shards,
            self.semantic_reference_index,
            self.semantic_import_export_index,
            self.semantic_package_graph_index,
            self.document_source,
            self.document_line_geometry,
            self.document_map_capacity,
            self.workspace_folders,
            self.lifecycle_guards,
        ];
        families.iter().try_fold(0usize, |acc, row| checked_add(acc, row.capacity_bytes))
    }

    pub fn is_evidence_complete(&self) -> bool {
        matches!(self.completeness, MemoryEvidenceCompleteness::Complete)
    }

    pub fn inventory_row_count(&self) -> usize {
        WORKSPACE_INDEX_RETAINED_INVENTORY.len()
    }
}

pub(crate) fn estimate_fact_shard(shard: &FileFactShard) -> Result<usize, MemoryWeightError> {
    let mut bytes = string_logical(&shard.source_uri);
    bytes = checked_add(bytes, size_of::<FileFactShard>())?;
    bytes = checked_add(bytes, vec_logical(shard.anchors.as_slice())?)?;
    for anchor in &shard.anchors {
        bytes = checked_add(bytes, size_of_val(anchor))?;
    }
    bytes = checked_add(bytes, vec_logical(shard.entities.as_slice())?)?;
    for entity in &shard.entities {
        bytes = checked_add(bytes, string_logical(&entity.canonical_name))?;
        bytes = checked_add(bytes, size_of_val(entity))?;
    }
    bytes = checked_add(bytes, vec_logical(shard.occurrences.as_slice())?)?;
    for occ in &shard.occurrences {
        bytes = checked_add(bytes, size_of_val(occ))?;
    }
    bytes = checked_add(bytes, vec_logical(shard.edges.as_slice())?)?;
    for edge in &shard.edges {
        bytes = checked_add(bytes, size_of_val(edge))?;
    }
    Ok(bytes)
}

pub(crate) fn estimate_search_index(
    search_index: &HashMap<String, Vec<WorkspaceSymbol>>,
) -> Result<CollectionWeight, MemoryWeightError> {
    let mut logical = 0usize;
    let mut capacity = hashmap_capacity_slack(search_index)?;
    let mut live_entries = 0usize;
    for (key, symbols) in search_index {
        logical = checked_add(logical, string_logical(key))?;
        capacity = checked_add(capacity, vec_capacity(symbols)?)?;
        for sym in symbols {
            logical = checked_add(logical, symbol_logical_bytes(sym)?)?;
            live_entries += 1;
        }
    }
    Ok(CollectionWeight { live_entries, logical_bytes: logical, capacity_bytes: capacity })
}

pub(crate) fn symbol_logical_bytes(sym: &WorkspaceSymbol) -> Result<usize, MemoryWeightError> {
    let mut bytes = string_logical(&sym.name) + string_logical(&sym.uri);
    bytes = checked_add(bytes, sym.qualified_name.as_deref().map_or(0, str::len))?;
    bytes = checked_add(bytes, sym.documentation.as_deref().map_or(0, str::len))?;
    bytes = checked_add(bytes, sym.container_name.as_deref().map_or(0, str::len))?;
    bytes = checked_add(bytes, size_of::<WorkspaceSymbol>())?;
    Ok(bytes)
}

pub(crate) fn estimate_document_store(
    store: &DocumentStore,
) -> Result<(CollectionWeight, CollectionWeight, CollectionWeight), MemoryWeightError> {
    store.memory_retained_weight()
}

impl WorkspaceIndexMemorySnapshotV2 {
    pub fn instrument_failed(error: MemoryWeightError) -> Self {
        Self {
            schema_id: WORKSPACE_INDEX_MEMORY_SCHEMA_V2,
            completeness: MemoryEvidenceCompleteness::InstrumentFailed(error),
            file_count: 0,
            symbol_count: 0,
            file_projection: CollectionWeight::empty(),
            definition_symbols: CollectionWeight::empty(),
            search_index: CollectionWeight::empty(),
            global_references: CollectionWeight::empty(),
            fact_shards: CollectionWeight::empty(),
            semantic_reference_index: CollectionWeight::empty(),
            semantic_import_export_index: CollectionWeight::empty(),
            semantic_package_graph_index: CollectionWeight::empty(),
            document_source: CollectionWeight::empty(),
            document_line_geometry: CollectionWeight::empty(),
            document_map_capacity: CollectionWeight::empty(),
            workspace_folders: CollectionWeight::empty(),
            lifecycle_guards: CollectionWeight::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace::memory_inventory::WORKSPACE_INDEX_RETAINED_INVENTORY;

    #[test]
    fn workspace_index_inventory_covers_known_retained_fields() {
        assert!(
            WORKSPACE_INDEX_RETAINED_INVENTORY.len() >= 14,
            "inventory must list every production retained WorkspaceIndex owner"
        );
    }
}
