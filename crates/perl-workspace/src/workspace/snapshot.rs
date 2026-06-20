//! Immutable, generation-numbered workspace semantic snapshot for atomic queries (#1601).
//!
//! `SemanticSnapshot` captures all semantic facts (file IDs, references, imports)
//! at one point in time, eliminating torn reads across concurrent updates.
//! Readers capture a single `Arc<SemanticSnapshot>` and query only that generation.

use std::collections::HashMap;

/// Lifecycle state of a snapshot.
///
/// Indicates whether the snapshot is still being built, degraded due to errors,
/// or ready for queries.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SnapshotLifecycle {
    /// Snapshot is still being assembled (index_file in progress)
    Building,
    /// Snapshot is complete but may be missing data due to errors
    Degraded,
    /// Snapshot is complete and ready for queries
    Ready,
}

/// Immutable, generation-numbered snapshot of workspace semantic facts.
///
/// All fields are public but snapshot itself is not `Clone` — callers hold `Arc<SemanticSnapshot>`.
/// This ensures readers never accidentally copy or hold multiple generations.
///
/// # Generation Semantics
///
/// - `generation` is incremented atomically with snapshot swap.
/// - Multiple readers can hold different generations simultaneously (safe under Arc).
/// - At publish time, a single Arc swap makes old snapshot unreachable to new readers.
pub struct SemanticSnapshot {
    /// Monotonically increasing generation counter (1-indexed).
    pub generation: u64,

    /// Lifecycle state of this snapshot.
    pub lifecycle: SnapshotLifecycle,

    /// File semantic bundles by normalized URI.
    /// TODO: Requires FileSemanticBundle from #1598.
    pub files: HashMap<String, std::sync::Arc<()>>,

    /// File IDs by normalized URI (enables file_id lookup without bundle).
    pub file_ids: HashMap<String, ()>,

    /// Semantic cross-file reference index (typed occurrences by name and entity).
    /// TODO: Requires ReferenceIndex from semantic module.
    pub references: (),

    /// Semantic cross-file import/export index.
    /// TODO: Requires ImportExportIndex from semantic module.
    pub imports: (),

    /// Workspace folder URIs for multi-root workspace support.
    pub workspace_roots: Vec<String>,
}

impl SemanticSnapshot {
    /// Create a new snapshot with the given generation and lifecycle state.
    ///
    /// # Arguments
    ///
    /// * `generation` — Monotonically increasing generation counter.
    /// * `lifecycle` — Current state (Building, Degraded, Ready).
    /// * `files` — File semantic bundles (will require FileSemanticBundle from #1598).
    /// * `file_ids` — Map of URIs to FileIds.
    /// * `references` — Semantic reference index (will require ReferenceIndex).
    /// * `imports` — Semantic import/export index (will require ImportExportIndex).
    /// * `workspace_roots` — Workspace folder URIs.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        generation: u64,
        lifecycle: SnapshotLifecycle,
        files: HashMap<String, std::sync::Arc<()>>,
        file_ids: HashMap<String, ()>,
        references: (),
        imports: (),
        workspace_roots: Vec<String>,
    ) -> Self {
        Self {
            generation,
            lifecycle,
            files,
            file_ids,
            references,
            imports,
            workspace_roots,
        }
    }

    /// Check if snapshot is in Ready state.
    pub fn is_ready(&self) -> bool {
        self.lifecycle == SnapshotLifecycle::Ready
    }
}

impl std::fmt::Debug for SemanticSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticSnapshot")
            .field("generation", &self.generation)
            .field("lifecycle", &self.lifecycle)
            .field("files_count", &self.files.len())
            .field("file_ids_count", &self.file_ids.len())
            .field("workspace_roots_count", &self.workspace_roots.len())
            .finish()
    }
}
