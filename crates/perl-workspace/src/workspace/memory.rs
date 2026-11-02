//! Memory profiling infrastructure for large workspace symbol resolution.
//!
//! This module provides pure-Rust memory estimation for the workspace index,
//! enabling baseline documentation and scaling analysis without external
//! allocator tooling (heaptrack, valgrind, etc.).
//!
//! # Usage
//!
//! ```rust,ignore
//! use perl_workspace::workspace::memory::MemorySnapshot;
//! use perl_workspace::workspace_index::WorkspaceIndex;
//!
//! let index = WorkspaceIndex::new();
//! // ... index files ...
//! let snap = MemorySnapshot::capture(&index);
//! println!("{}", snap.to_report_string());
//! ```
//!
//! # Limitations
//!
//! Estimates use `String::len()` for heap strings and `std::mem::size_of` for
//! stack-allocated struct fields. They do not account for allocator overhead,
//! heap fragmentation, or internal `HashMap` bucket arrays. For precise
//! measurement, pair with `dhat` (Linux) or platform heap profilers.
//!
//! # Feature Gate
//!
//! This module is only compiled when the `memory-profiling` feature is enabled.

pub use super::memory_inventory::{
    RetainedDisposition, RetainedOwnerInventoryRow, WORKSPACE_INDEX_RETAINED_INVENTORY,
};
pub use super::memory_snapshot_v2::{
    MemoryEvidenceCompleteness, WORKSPACE_INDEX_MEMORY_SCHEMA_V2, WorkspaceIndexMemorySnapshotV2,
};
pub use super::memory_weight::{CollectionWeight, MemoryWeightError};
pub use super::workspace_index::memory_falsifiers::{
    leak_fact_shard, leak_import_row, leak_package_graph_row, leak_search_index_bucket,
    leak_semantic_reference_row,
};

use super::workspace_index::WorkspaceIndex;
use std::fmt;

/// A point-in-time estimate of workspace index memory usage.
///
/// Sizes are approximations based on summing the byte lengths of all
/// heap-allocated `String` values and the `size_of` stack portions of
/// composite structs. HashMap bucket overhead and allocator padding are
/// not included.
///
/// Use [`MemorySnapshot::capture`] to snapshot a live [`WorkspaceIndex`].
/// Legacy and versioned workspace memory snapshot (`memory-profiling` only).
#[derive(Clone, Debug)]
pub struct MemorySnapshot {
    /// Indexed file count at capture time.
    pub file_count: usize,
    /// Indexed symbol count at capture time.
    pub symbol_count: usize,
    /// Logical bytes in the per-file projection (legacy receipt field).
    pub files_bytes: usize,
    /// Logical bytes in the definition-symbol map (legacy receipt field).
    pub symbols_bytes: usize,
    /// Logical bytes in the global reference map (legacy receipt field).
    pub global_refs_bytes: usize,
    /// Document source plus line-geometry logical bytes (legacy aggregate).
    pub document_store_bytes: usize,
    /// Authoritative complete retained denominator.
    pub v2: WorkspaceIndexMemorySnapshotV2,
}

impl Default for MemorySnapshot {
    fn default() -> Self {
        Self {
            file_count: 0,
            symbol_count: 0,
            files_bytes: 0,
            symbols_bytes: 0,
            global_refs_bytes: 0,
            document_store_bytes: 0,
            v2: WorkspaceIndexMemorySnapshotV2 {
                schema_id: WORKSPACE_INDEX_MEMORY_SCHEMA_V2,
                completeness: MemoryEvidenceCompleteness::Complete,
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
            },
        }
    }
}

impl MemorySnapshot {
    /// Capture a memory snapshot of the current [`WorkspaceIndex`] state.
    ///
    /// This acquires read locks on all index components and walks their
    /// contents to estimate heap usage. Intended for offline profiling;
    /// do not call on the LSP hot path.
    pub fn capture(index: &WorkspaceIndex) -> Self {
        let v2 = index.memory_snapshot_v2();
        Self {
            file_count: v2.file_count,
            symbol_count: v2.symbol_count,
            files_bytes: v2.file_projection.logical_bytes,
            symbols_bytes: v2.definition_symbols.logical_bytes,
            global_refs_bytes: v2.global_references.logical_bytes,
            document_store_bytes: v2.document_source.logical_bytes
                + v2.document_line_geometry.logical_bytes,
            v2,
        }
    }

    /// Total estimated logical retained bytes across all measured families.
    pub fn total_estimated_bytes(&self) -> usize {
        self.v2.total_logical_retained_bytes().unwrap_or(0)
    }

    /// Family-aware scaling denominator for bytes-per-symbol guards.
    pub fn scaling_denominator_bytes(&self) -> usize {
        self.total_estimated_bytes()
    }

    /// Estimated bytes per indexed symbol.
    ///
    /// Returns 0 when no symbols are indexed.
    pub fn bytes_per_symbol(&self) -> usize {
        if self.symbol_count == 0 {
            return 0;
        }
        self.scaling_denominator_bytes() / self.symbol_count
    }

    /// Estimated bytes per indexed file.
    ///
    /// Returns 0 when no files are indexed.
    pub fn bytes_per_file(&self) -> usize {
        if self.file_count == 0 {
            return 0;
        }
        self.scaling_denominator_bytes() / self.file_count
    }

    /// Format this snapshot as a human-readable report string.
    pub fn to_report_string(&self) -> String {
        let total = self.total_estimated_bytes();
        format!(
            "MemorySnapshot {{ files: {} ({} B), symbols map: {} ({} B), \
             global_refs: {} B, doc_store: {} B, total: {} B, \
             {} symbols, {} B/symbol }}",
            self.file_count,
            self.files_bytes,
            self.symbol_count,
            self.symbols_bytes,
            self.global_refs_bytes,
            self.document_store_bytes,
            total,
            self.symbol_count,
            self.bytes_per_symbol(),
        )
    }
}

impl fmt::Display for MemorySnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_report_string())
    }
}

/// A collection of [`MemorySnapshot`]s at increasing file-count checkpoints.
///
/// Used to characterise scaling behaviour (linear, sub-linear, or unexpected
/// growth) without external tooling.
///
/// # Example
///
/// ```rust,ignore
/// use perl_workspace::workspace::memory::{MemorySnapshot, ScaleReport};
/// use perl_workspace::workspace_index::WorkspaceIndex;
///
/// let mut report = ScaleReport::new();
/// for scale in [100, 500, 1000] {
///     let index = WorkspaceIndex::new();
///     // ... index `scale` files ...
///     report.add_checkpoint(scale, MemorySnapshot::capture(&index));
/// }
/// println!("{}", report);
/// ```
#[derive(Debug, Default)]
pub struct ScaleReport {
    /// Ordered list of (file_count, snapshot) pairs.
    checkpoints: Vec<(usize, MemorySnapshot)>,
}

impl ScaleReport {
    /// Create an empty scale report.
    pub fn new() -> Self {
        Self { checkpoints: Vec::new() }
    }

    /// Add a checkpoint with the nominal file count and its snapshot.
    pub fn add_checkpoint(&mut self, file_count: usize, snap: MemorySnapshot) {
        self.checkpoints.push((file_count, snap));
    }

    /// Return a slice of all checkpoints.
    pub fn checkpoints(&self) -> &[(usize, MemorySnapshot)] {
        &self.checkpoints
    }

    /// Compute a rough scaling factor between the first and last checkpoint.
    ///
    /// Returns `None` if fewer than two checkpoints exist or memory is zero.
    pub fn scaling_factor(&self) -> Option<f64> {
        if self.checkpoints.len() < 2 {
            return None;
        }
        let first = self.checkpoints.first()?;
        let last = self.checkpoints.last()?;

        let mem_first = first.1.total_estimated_bytes();
        let mem_last = last.1.total_estimated_bytes();
        let file_first = first.0;
        let file_last = last.0;

        if mem_first == 0 || file_first == 0 || file_last == 0 {
            return None;
        }

        // Expected linear memory at last scale = mem_first * (file_last / file_first)
        // Actual / expected gives the scaling factor relative to linear
        let expected_linear = mem_first as f64 * (file_last as f64 / file_first as f64);
        Some(mem_last as f64 / expected_linear)
    }
}

impl fmt::Display for ScaleReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{:<10} {:>12} {:>12} {:>12} {:>14}",
            "files", "total_B", "symbols", "B/sym", "B/file"
        )?;
        writeln!(f, "{}", "-".repeat(64))?;
        for (file_count, snap) in &self.checkpoints {
            writeln!(
                f,
                "{:<10} {:>12} {:>12} {:>12} {:>14}",
                file_count,
                snap.total_estimated_bytes(),
                snap.symbol_count,
                snap.bytes_per_symbol(),
                snap.bytes_per_file(),
            )?;
        }
        if let Some(factor) = self.scaling_factor() {
            writeln!(f, "\nScaling factor vs linear: {:.2}x", factor)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CollectionWeight, MemoryEvidenceCompleteness, MemorySnapshot, ScaleReport,
        WORKSPACE_INDEX_MEMORY_SCHEMA_V2, WorkspaceIndexMemorySnapshotV2,
    };

    #[test]
    fn memory_snapshot_derived_metrics_handle_zero_counts() -> Result<(), Box<dyn std::error::Error>>
    {
        let snap = MemorySnapshot {
            file_count: 0,
            symbol_count: 0,
            files_bytes: 0,
            symbols_bytes: 0,
            global_refs_bytes: 0,
            document_store_bytes: 0,
            v2: WorkspaceIndexMemorySnapshotV2 {
                schema_id: WORKSPACE_INDEX_MEMORY_SCHEMA_V2,
                completeness: MemoryEvidenceCompleteness::Complete,
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
            },
        };

        assert_eq!(snap.total_estimated_bytes(), 0);
        assert_eq!(snap.bytes_per_symbol(), 0);
        assert_eq!(snap.bytes_per_file(), 0);

        Ok(())
    }

    #[test]
    fn memory_snapshot_report_includes_aggregated_values() -> Result<(), Box<dyn std::error::Error>>
    {
        let snap = MemorySnapshot {
            file_count: 2,
            symbol_count: 4,
            files_bytes: 100,
            symbols_bytes: 20,
            global_refs_bytes: 60,
            document_store_bytes: 20,
            v2: WorkspaceIndexMemorySnapshotV2 {
                schema_id: WORKSPACE_INDEX_MEMORY_SCHEMA_V2,
                completeness: MemoryEvidenceCompleteness::Complete,
                file_count: 2,
                symbol_count: 4,
                file_projection: CollectionWeight {
                    live_entries: 2,
                    logical_bytes: 100,
                    capacity_bytes: 0,
                },
                definition_symbols: CollectionWeight {
                    live_entries: 0,
                    logical_bytes: 20,
                    capacity_bytes: 0,
                },
                search_index: CollectionWeight::empty(),
                global_references: CollectionWeight {
                    live_entries: 0,
                    logical_bytes: 60,
                    capacity_bytes: 0,
                },
                fact_shards: CollectionWeight::empty(),
                semantic_reference_index: CollectionWeight::empty(),
                semantic_import_export_index: CollectionWeight::empty(),
                semantic_package_graph_index: CollectionWeight::empty(),
                document_source: CollectionWeight {
                    live_entries: 0,
                    logical_bytes: 15,
                    capacity_bytes: 0,
                },
                document_line_geometry: CollectionWeight {
                    live_entries: 0,
                    logical_bytes: 5,
                    capacity_bytes: 0,
                },
                document_map_capacity: CollectionWeight::empty(),
                workspace_folders: CollectionWeight::empty(),
                lifecycle_guards: CollectionWeight::empty(),
            },
        };

        assert_eq!(snap.total_estimated_bytes(), 200);
        assert_eq!(snap.bytes_per_symbol(), 50);
        assert_eq!(snap.bytes_per_file(), 100);

        let report = snap.to_report_string();
        assert!(report.contains("total: 200 B"));
        assert!(report.contains("4 symbols"));
        assert!(report.contains("50 B/symbol"));

        Ok(())
    }

    #[test]
    fn scale_report_scaling_factor_tracks_linear_vs_growth()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut linear = ScaleReport::new();
        linear.add_checkpoint(10, MemorySnapshot { files_bytes: 100, ..MemorySnapshot::default() });
        linear.add_checkpoint(20, MemorySnapshot { files_bytes: 200, ..MemorySnapshot::default() });

        let linear_factor =
            linear.scaling_factor().ok_or("expected linear scaling factor to exist")?;
        // Use a relative tolerance (1e-9) rather than f64::EPSILON (2.2e-16) so the
        // assertion remains valid if test values are changed to non-power-of-2 numbers
        // that introduce floating-point rounding error.
        assert!((linear_factor - 1.0).abs() < 1e-9, "expected ~1.0, got {linear_factor}");

        let mut super_linear = ScaleReport::new();
        super_linear
            .add_checkpoint(10, MemorySnapshot { files_bytes: 100, ..MemorySnapshot::default() });
        super_linear
            .add_checkpoint(20, MemorySnapshot { files_bytes: 300, ..MemorySnapshot::default() });

        let super_linear_factor =
            super_linear.scaling_factor().ok_or("expected super-linear scaling factor to exist")?;
        assert!(
            (super_linear_factor - 1.5).abs() < 1e-9,
            "expected ~1.5, got {super_linear_factor}"
        );

        Ok(())
    }

    #[test]
    fn scale_report_scaling_factor_returns_none_for_insufficient_data()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut report = ScaleReport::new();
        assert_eq!(report.scaling_factor(), None);

        report.add_checkpoint(0, MemorySnapshot { files_bytes: 100, ..MemorySnapshot::default() });
        report.add_checkpoint(20, MemorySnapshot { files_bytes: 200, ..MemorySnapshot::default() });
        assert_eq!(report.scaling_factor(), None);

        Ok(())
    }

    #[test]
    fn scale_report_display_contains_table_and_scaling_factor()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut report = ScaleReport::new();
        report.add_checkpoint(
            10,
            MemorySnapshot {
                file_count: 10,
                symbol_count: 10,
                files_bytes: 100,
                ..MemorySnapshot::default()
            },
        );
        report.add_checkpoint(
            20,
            MemorySnapshot {
                file_count: 20,
                symbol_count: 20,
                files_bytes: 200,
                ..MemorySnapshot::default()
            },
        );

        let display = format!("{report}");
        assert!(display.contains("files"));
        assert!(display.contains("total_B"));
        assert!(display.contains("Scaling factor vs linear: 1.00x"));

        Ok(())
    }
}
