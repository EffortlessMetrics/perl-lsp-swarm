//! Typed reference index for cross-file reference lookups.
//!
//! Maintains three projections over [`ReferenceEdge`] entries:
//!
//! - `references_by_name` — keyed by the bare or qualified symbol key, for
//!   name-based lookups (e.g. find-references by symbol name). It holds only
//!   occurrences whose canonical name was actually derived.
//! - `references_by_entity` — keyed by [`EntityId`], for entity-based lookups
//!   (e.g. find all references to a specific declaration).
//! - `unresolved_by_occurrence` — keyed by [`UnresolvedOccurrenceKey`], for
//!   occurrences with no derivable canonical name. These are kept for
//!   diagnostics, explanation, and later rebinding; they are deliberately not
//!   reachable through the name projection, because "we could not resolve this"
//!   is not a name.
//!
//! All three support incremental add/remove via [`ReferenceIndex::add_file`]
//! and [`ReferenceIndex::remove_file`], keyed by the file's source URI.

use perl_semantic_facts::{
    AnchorId, EdgeKind, EntityId, FileId, OccurrenceId, OccurrenceKind, ReferenceEdge,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::workspace::workspace_index::FileFactShard;

/// Identity of a reference occurrence that has no derivable canonical name.
///
/// An [`AnchorId`] on its own is not an occurrence identity: it carries no
/// source domain, so keying unresolved occurrences by the anchor number alone
/// merges occurrences from unrelated files into one bucket.
///
/// Global anchor uniqueness is a per-producer accident, not a contract the
/// index may rely on. The two producers that currently attach occurrences
/// (`perl_symbol::surface::facts` reference anchors and
/// `semantic::eval_sub_extractor`) do hash `file_id` into the anchor id, but
/// the same workspace already mints file-independent anchor ids elsewhere —
/// `semantic::facts::import_anchor_base_id` derives an import anchor from the
/// import's index alone, so every file's Nth import anchor shares one id.
/// This key removes the index's dependence on that accident.
///
/// # Scope limitation
///
/// This key is scoped by *source file identity* only. It carries no project
/// root, resolution domain, or accepted generation, so it distinguishes
/// occurrences within one indexed workspace state but cannot by itself
/// distinguish two generations of the same file or the same logical file under
/// two roots. Adding those components is owned by the entity-catalog and
/// fingerprint work in perl-lsp-swarm#8083 / #8054, not by this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnresolvedOccurrenceKey {
    /// File containing the unresolved occurrence.
    pub file_id: FileId,
    /// The occurrence itself.
    pub occurrence_id: OccurrenceId,
    /// Source anchor for the occurrence.
    pub anchor_id: AnchorId,
}

impl UnresolvedOccurrenceKey {
    /// Build a key for one occurrence in one file.
    pub fn new(file_id: FileId, occurrence_id: OccurrenceId, anchor_id: AnchorId) -> Self {
        Self { file_id, occurrence_id, anchor_id }
    }
}

/// Cross-file reference index backed by three `HashMap`s.
///
/// Populated from [`FileFactShard`] occurrences and edges during workspace
/// indexing. Supports incremental updates: call [`remove_file`](Self::remove_file)
/// to purge stale entries, then [`add_file`](Self::add_file) to insert fresh ones.
#[derive(Debug, Default)]
pub struct ReferenceIndex {
    /// Symbol-key → reference edges. The key is the bare or qualified name
    /// carried on each [`ReferenceEdge::symbol_key`].
    ///
    /// Only occurrences with a derived canonical name appear here; unresolved
    /// occurrences live in `unresolved_by_occurrence` instead.
    references_by_name: HashMap<String, Vec<Arc<ReferenceEdge>>>,

    /// Entity → reference edges. One entry per target candidate in each
    /// [`ReferenceEdge::target_candidates`].
    ///
    /// Each `Arc<ReferenceEdge>` is shared with `references_by_name`, so an
    /// edge with N target candidates costs one allocation instead of N+1.
    references_by_entity: HashMap<EntityId, Vec<Arc<ReferenceEdge>>>,

    /// Scoped occurrence identity → reference edges for occurrences with no
    /// derivable canonical name.
    ///
    /// A `Vec` value rather than a single edge because a malformed or
    /// hand-built shard may repeat one occurrence/anchor pair; those rows stay
    /// distinct entries rather than silently overwriting one another.
    unresolved_by_occurrence: HashMap<UnresolvedOccurrenceKey, Vec<Arc<ReferenceEdge>>>,

    /// Tracks which file URIs have been indexed so that [`remove_file`](Self::remove_file)
    /// can efficiently purge stale entries.
    indexed_files: HashMap<String, FileId>,
}

impl ReferenceIndex {
    /// Create an empty reference index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Index all reference-like occurrences from a [`FileFactShard`].
    ///
    /// For each non-definition occurrence in the shard, a [`ReferenceEdge`] is
    /// synthesized and inserted into both lookup maps. Edge facts with kind
    /// [`EdgeKind::References`] are consulted to populate `target_candidates`.
    pub fn add_file(&mut self, shard: &FileFactShard) {
        // Record the file so remove_file can match by URI later.
        self.indexed_files.insert(shard.source_uri.clone(), shard.file_id);

        // Build a quick lookup: occurrence_id → list of target entity IDs from
        // Reference edges in the shard.
        let mut edge_targets: HashMap<u64, Vec<EntityId>> = HashMap::new();
        for edge in &shard.edges {
            if edge.kind == EdgeKind::References
                && let Some(occ_id) = edge.via_occurrence_id
            {
                edge_targets.entry(occ_id.0).or_default().push(edge.to_entity_id);
            }
        }

        // A unique occurrence ID allows the candidate vector to move out of
        // the temporary lookup. Preserve the previous clone-based behavior
        // for malformed or hand-built shards that repeat an ID so every
        // occurrence still sees the same targets.
        let mut occurrence_counts = HashMap::new();
        for occ in &shard.occurrences {
            if occ.kind != OccurrenceKind::Definition {
                *occurrence_counts.entry(occ.id.0).or_default() += 1;
            }
        }

        for occ in &shard.occurrences {
            // Skip definition occurrences — they are not references.
            if occ.kind == OccurrenceKind::Definition {
                continue;
            }

            // Build the target_candidates list from edges, falling back to the
            // occurrence's own entity_id when no edge exists.
            let target_candidates = if occurrence_counts.get(&occ.id.0) == Some(&1) {
                edge_targets
                    .remove(&occ.id.0)
                    .unwrap_or_else(|| occ.entity_id.into_iter().collect())
            } else {
                edge_targets
                    .get(&occ.id.0)
                    .cloned()
                    .unwrap_or_else(|| occ.entity_id.into_iter().collect())
            };

            // Derive the canonical name from the occurrence's entity when the
            // declaring entity is available. An occurrence with no derivable
            // name is not given a synthetic one: it is routed to the scoped
            // unresolved store below, leaving the name projection exact.
            let canonical_name = Self::derive_canonical_name(shard, occ);

            // Wrap in Arc so the name and entity indexes share one allocation.
            // target_candidates is moved in (no clone); subsequent access via Deref.
            let ref_edge = Arc::new(ReferenceEdge::new(
                occ.id,
                occ.anchor_id,
                shard.file_id,
                canonical_name.clone().unwrap_or_default(),
                target_candidates,
                occ.kind,
                occ.provenance,
                occ.confidence,
            ));

            // Insert into entity index — one Arc::clone per target candidate
            // instead of a full ReferenceEdge clone. An occurrence can have
            // target candidates while its declaration lives in another shard,
            // so this is independent of whether a name was derived.
            for entity_id in &ref_edge.target_candidates {
                self.references_by_entity
                    .entry(*entity_id)
                    .or_default()
                    .push(Arc::clone(&ref_edge));
            }

            // Insert into the name or unresolved projection after the entity
            // index has borrowed the Arc, so this final insertion can move it
            // without another clone.
            match canonical_name {
                Some(name) => self.references_by_name.entry(name).or_default().push(ref_edge),
                None => self
                    .unresolved_by_occurrence
                    .entry(UnresolvedOccurrenceKey::new(shard.file_id, occ.id, occ.anchor_id))
                    .or_default()
                    .push(ref_edge),
            }
        }
    }

    /// Remove all reference entries that originated from the given file URI.
    ///
    /// This is the "remove" half of incremental re-indexing: call this before
    /// [`add_file`](Self::add_file) with the updated shard.
    pub fn remove_file(&mut self, source_uri: &str) {
        let file_id = match self.indexed_files.remove(source_uri) {
            Some(id) => id,
            None => return,
        };

        // Retain only entries from other files.
        for refs in self.references_by_name.values_mut() {
            refs.retain(|r| r.file_id != file_id);
        }
        // Remove empty buckets to keep the map tidy.
        self.references_by_name.retain(|_, v| !v.is_empty());

        for refs in self.references_by_entity.values_mut() {
            refs.retain(|r| r.file_id != file_id);
        }
        self.references_by_entity.retain(|_, v| !v.is_empty());

        // Unresolved buckets need no element filter: unlike a name or entity
        // key, which can legitimately hold rows from several files, this key
        // carries the `file_id` its rows were inserted with, so a bucket is
        // wholly owned by one file and the key alone decides.
        self.unresolved_by_occurrence.retain(|key, _| key.file_id != file_id);
    }

    /// Look up all reference edges for a given symbol key (bare or qualified name).
    ///
    /// Returns `Arc<ReferenceEdge>` entries; callers may access fields via
    /// [`Deref`](std::ops::Deref) without unwrapping.
    pub fn get_by_name(&self, symbol_key: &str) -> &[Arc<ReferenceEdge>] {
        self.references_by_name.get(symbol_key).map(Vec::as_slice).unwrap_or_default()
    }

    /// Look up all reference edges targeting a given entity.
    ///
    /// Returns `Arc<ReferenceEdge>` entries shared with the name index; no
    /// additional allocation per lookup.
    pub fn get_by_entity(&self, entity_id: EntityId) -> &[Arc<ReferenceEdge>] {
        self.references_by_entity.get(&entity_id).map(Vec::as_slice).unwrap_or_default()
    }

    /// Look up the reference edges for one unresolved occurrence.
    ///
    /// Unresolved occurrences are addressable only through their scoped
    /// identity; they never satisfy a name lookup.
    pub fn get_unresolved(&self, key: &UnresolvedOccurrenceKey) -> &[Arc<ReferenceEdge>] {
        self.unresolved_by_occurrence.get(key).map(Vec::as_slice).unwrap_or_default()
    }

    /// Return the number of distinct symbol keys in the name index.
    ///
    /// Counts derived canonical names only — unresolved occurrences are
    /// counted by [`unresolved_count`](Self::unresolved_count).
    pub fn name_count(&self) -> usize {
        self.references_by_name.len()
    }

    /// Return the number of distinct unresolved occurrences.
    pub fn unresolved_count(&self) -> usize {
        self.unresolved_by_occurrence.len()
    }

    /// Return the number of distinct entities in the entity index.
    pub fn entity_count(&self) -> usize {
        self.references_by_entity.len()
    }

    // ── Private helpers ──

    /// Derive the canonical name for an occurrence, if one is available.
    ///
    /// Returns the canonical name of the occurrence's entity when that entity
    /// is present in the same shard. Returns `None` otherwise — including for a
    /// resolved cross-file target whose declaration lives in another shard,
    /// which this layer cannot name without a project entity catalog
    /// (perl-lsp-swarm#8083). `None` means "no name derived here", never "no
    /// target": the entity projection is populated independently.
    fn derive_canonical_name(
        shard: &FileFactShard,
        occ: &perl_semantic_facts::OccurrenceFact,
    ) -> Option<String> {
        let entity_id = occ.entity_id?;
        shard
            .entities
            .iter()
            .find(|e| e.id == entity_id)
            .map(|entity| entity.canonical_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::facts::PRODUCER_SCHEMA_VERSION;
    use perl_semantic_facts::{
        AnchorFact, AnchorId, Confidence, EdgeFact, EdgeId, EntityFact, EntityKind, OccurrenceFact,
        OccurrenceId, Provenance, ScopeId,
    };

    /// Build a minimal `FileFactShard` with one entity, one reference occurrence,
    /// and one `References` edge linking them.
    fn sample_shard() -> FileFactShard {
        let file_id = FileId(1);
        let entity_id = EntityId(100);
        let anchor_def = AnchorId(10);
        let anchor_ref = AnchorId(20);
        let occ_id = OccurrenceId(400);

        FileFactShard {
            source_uri: "file:///lib/Foo.pm".to_string(),
            file_id,
            content_hash: 999,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![
                AnchorFact {
                    id: anchor_def,
                    file_id,
                    span_start_byte: 0,
                    span_end_byte: 10,
                    scope_id: None,
                    provenance: Provenance::ExactAst,
                    confidence: Confidence::High,
                },
                AnchorFact {
                    id: anchor_ref,
                    file_id,
                    span_start_byte: 50,
                    span_end_byte: 55,
                    scope_id: None,
                    provenance: Provenance::ExactAst,
                    confidence: Confidence::High,
                },
            ],
            entities: vec![EntityFact {
                id: entity_id,
                kind: EntityKind::Subroutine,
                canonical_name: "Foo::bar".to_string(),
                anchor_id: Some(anchor_def),
                scope_id: Some(ScopeId(1)),
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            occurrences: vec![OccurrenceFact {
                id: occ_id,
                kind: OccurrenceKind::Call,
                entity_id: Some(entity_id),
                anchor_id: anchor_ref,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            edges: vec![EdgeFact {
                id: EdgeId(500),
                kind: EdgeKind::References,
                from_entity_id: EntityId(0), // caller entity (not relevant here)
                to_entity_id: entity_id,
                via_occurrence_id: Some(occ_id),
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
        }
    }

    #[test]
    fn add_file_populates_name_index() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        let refs = index.get_by_name("Foo::bar");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, OccurrenceKind::Call);
        assert_eq!(refs[0].symbol_key, "Foo::bar");
        Ok(())
    }

    #[test]
    fn add_file_populates_entity_index() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        let refs = index.get_by_entity(EntityId(100));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].occurrence_id, OccurrenceId(400));
        Ok(())
    }

    #[test]
    fn remove_file_clears_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        assert_eq!(index.name_count(), 1);
        assert_eq!(index.entity_count(), 1);

        index.remove_file("file:///lib/Foo.pm");

        assert_eq!(index.name_count(), 0);
        assert_eq!(index.entity_count(), 0);
        assert!(index.get_by_name("Foo::bar").is_empty());
        assert!(index.get_by_entity(EntityId(100)).is_empty());
        Ok(())
    }

    #[test]
    fn remove_file_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        index.remove_file("file:///lib/Foo.pm");
        // Second remove should be a no-op.
        index.remove_file("file:///lib/Foo.pm");

        assert_eq!(index.name_count(), 0);
        assert_eq!(index.entity_count(), 0);
        Ok(())
    }

    #[test]
    fn remove_unknown_file_is_noop() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        index.remove_file("file:///nonexistent.pm");

        // Original entries should still be present.
        assert_eq!(index.name_count(), 1);
        assert_eq!(index.entity_count(), 1);
        Ok(())
    }

    #[test]
    fn definition_occurrences_are_excluded() -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(2);
        let entity_id = EntityId(200);
        let anchor_id = AnchorId(30);

        let shard = FileFactShard {
            source_uri: "file:///lib/Defs.pm".to_string(),
            file_id,
            content_hash: 111,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![AnchorFact {
                id: anchor_id,
                file_id,
                span_start_byte: 0,
                span_end_byte: 5,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            entities: vec![EntityFact {
                id: entity_id,
                kind: EntityKind::Subroutine,
                canonical_name: "Defs::init".to_string(),
                anchor_id: Some(anchor_id),
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            occurrences: vec![OccurrenceFact {
                id: OccurrenceId(600),
                kind: OccurrenceKind::Definition,
                entity_id: Some(entity_id),
                anchor_id,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            edges: vec![],
        };

        let mut index = ReferenceIndex::new();
        index.add_file(&shard);

        // Definition occurrences should not appear in the reference index.
        assert_eq!(index.name_count(), 0);
        assert_eq!(index.entity_count(), 0);
        Ok(())
    }

    #[test]
    fn multiple_files_coexist() -> Result<(), Box<dyn std::error::Error>> {
        let shard_a = sample_shard();

        let file_id_b = FileId(2);
        let entity_id = EntityId(100); // same target entity
        let occ_id_b = OccurrenceId(700);
        let anchor_b = AnchorId(40);

        let shard_b = FileFactShard {
            source_uri: "file:///lib/Bar.pm".to_string(),
            file_id: file_id_b,
            content_hash: 888,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![AnchorFact {
                id: anchor_b,
                file_id: file_id_b,
                span_start_byte: 10,
                span_end_byte: 18,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            entities: vec![EntityFact {
                id: entity_id,
                kind: EntityKind::Subroutine,
                canonical_name: "Foo::bar".to_string(),
                anchor_id: None,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            occurrences: vec![OccurrenceFact {
                id: occ_id_b,
                kind: OccurrenceKind::Call,
                entity_id: Some(entity_id),
                anchor_id: anchor_b,
                scope_id: None,
                provenance: Provenance::NameHeuristic,
                confidence: Confidence::Medium,
            }],
            edges: vec![],
        };

        let mut index = ReferenceIndex::new();
        index.add_file(&shard_a);
        index.add_file(&shard_b);

        // Both files contribute references to the same name.
        assert_eq!(index.get_by_name("Foo::bar").len(), 2);
        // Both files contribute references to the same entity.
        assert_eq!(index.get_by_entity(entity_id).len(), 2);

        // Remove one file — only its entries should disappear.
        index.remove_file("file:///lib/Foo.pm");
        assert_eq!(index.get_by_name("Foo::bar").len(), 1);
        assert_eq!(index.get_by_entity(entity_id).len(), 1);
        assert_eq!(index.get_by_name("Foo::bar")[0].file_id, file_id_b);

        Ok(())
    }

    #[test]
    fn incremental_reindex_replaces_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        assert_eq!(index.get_by_name("Foo::bar").len(), 1);

        // Simulate re-indexing: remove old, add updated shard.
        index.remove_file("file:///lib/Foo.pm");

        // Updated shard with a different occurrence.
        let file_id = FileId(1);
        let entity_id = EntityId(100);
        let updated_shard = FileFactShard {
            source_uri: "file:///lib/Foo.pm".to_string(),
            file_id,
            content_hash: 1000,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![AnchorFact {
                id: AnchorId(50),
                file_id,
                span_start_byte: 60,
                span_end_byte: 68,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            entities: vec![EntityFact {
                id: entity_id,
                kind: EntityKind::Subroutine,
                canonical_name: "Foo::bar".to_string(),
                anchor_id: None,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            occurrences: vec![OccurrenceFact {
                id: OccurrenceId(800),
                kind: OccurrenceKind::Read,
                entity_id: Some(entity_id),
                anchor_id: AnchorId(50),
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            edges: vec![],
        };

        index.add_file(&updated_shard);

        let refs = index.get_by_name("Foo::bar");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].occurrence_id, OccurrenceId(800));
        assert_eq!(refs[0].kind, OccurrenceKind::Read);
        Ok(())
    }

    /// Build a shard whose only occurrence resolves to no local canonical name,
    /// at the shared anchor id 60.
    ///
    /// Every unresolved fixture is constructed whole rather than mutated after
    /// the fact, so each field a case depends on is visible at its one
    /// construction site.
    ///
    /// * `occurrence_entity` — what the occurrence claims to target. `None` is
    ///   an occurrence with no target at all; `Some(id)` with no matching row in
    ///   `entities` is a declaration that lives in another shard.
    /// * `entities` — the local entity rows, which decide whether
    ///   `derive_canonical_name`'s `e.id == entity_id` lookup hits or misses.
    /// * `edges` — `EdgeKind::References` edges supplying target candidates.
    ///
    /// Both files deliberately use one anchor number: the index must not depend
    /// on cross-file anchor uniqueness, which no producer contract guarantees.
    fn unresolved_shard_with(
        source_uri: &str,
        file_id: FileId,
        occ_id: OccurrenceId,
        occurrence_entity: Option<EntityId>,
        entities: Vec<EntityFact>,
        edges: Vec<EdgeFact>,
    ) -> FileFactShard {
        let anchor_id = AnchorId(60);

        FileFactShard {
            source_uri: source_uri.to_string(),
            file_id,
            content_hash: 222,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![AnchorFact {
                id: anchor_id,
                file_id,
                span_start_byte: 0,
                span_end_byte: 8,
                scope_id: None,
                provenance: Provenance::NameHeuristic,
                confidence: Confidence::Low,
            }],
            entities,
            occurrences: vec![OccurrenceFact {
                id: occ_id,
                kind: OccurrenceKind::Call,
                entity_id: occurrence_entity,
                anchor_id,
                scope_id: None,
                provenance: Provenance::NameHeuristic,
                confidence: Confidence::Low,
            }],
            edges,
        }
    }

    /// The simplest unresolved shard: no target, no local entity row, no edge.
    fn unresolved_shard(source_uri: &str, file_id: FileId, occ_id: OccurrenceId) -> FileFactShard {
        unresolved_shard_with(source_uri, file_id, occ_id, None, Vec::new(), Vec::new())
    }

    /// A `References` edge from the synthetic caller sentinel to `target`.
    fn reference_edge(edge_id: EdgeId, occ_id: OccurrenceId, target: EntityId) -> EdgeFact {
        EdgeFact {
            id: edge_id,
            kind: EdgeKind::References,
            from_entity_id: EntityId(0),
            to_entity_id: target,
            via_occurrence_id: Some(occ_id),
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    /// A subroutine entity row declared locally under `canonical_name`.
    fn declared_entity(id: EntityId, canonical_name: &str) -> EntityFact {
        EntityFact {
            id,
            kind: EntityKind::Subroutine,
            canonical_name: canonical_name.to_string(),
            anchor_id: None,
            scope_id: None,
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    #[test]
    fn unresolved_occurrence_leaves_the_name_projection() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut index = ReferenceIndex::new();
        index.add_file(&unresolved_shard(
            "file:///lib/Unresolved.pm",
            FileId(3),
            OccurrenceId(900),
        ));

        // The retired anchor pseudo-name must not be reachable, and neither
        // must the empty spelling the edge now carries: "we could not resolve
        // this" is not a name, so the name projection stays exact.
        assert!(index.get_by_name("__unresolved_anchor_60").is_empty());
        assert!(index.get_by_name("").is_empty());
        assert_eq!(index.name_count(), 0);

        // The occurrence is still retained, addressable by its scoped identity.
        let key = UnresolvedOccurrenceKey::new(FileId(3), OccurrenceId(900), AnchorId(60));
        let unresolved = index.get_unresolved(&key);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].confidence, Confidence::Low);
        assert_eq!(unresolved[0].symbol_key, "");
        assert_eq!(index.unresolved_count(), 1);

        // No entity-based entries since there are no target candidates.
        assert_eq!(index.entity_count(), 0);
        Ok(())
    }

    #[test]
    fn unresolved_occurrences_in_two_files_do_not_share_one_bucket()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&unresolved_shard("file:///lib/A.pm", FileId(3), OccurrenceId(900)));
        index.add_file(&unresolved_shard("file:///lib/B.pm", FileId(4), OccurrenceId(901)));

        // Negative control for the retired anchor-only key: both files use
        // AnchorId(60), so an anchor-keyed store returns one merged bucket of
        // two edges — as `references_by_name` did under
        // `__unresolved_anchor_60`. Scoped identity keeps them apart.
        assert_eq!(index.unresolved_count(), 2);
        assert_eq!(index.name_count(), 0);

        let key_a = UnresolvedOccurrenceKey::new(FileId(3), OccurrenceId(900), AnchorId(60));
        let key_b = UnresolvedOccurrenceKey::new(FileId(4), OccurrenceId(901), AnchorId(60));
        assert_ne!(key_a, key_b);

        let refs_a = index.get_unresolved(&key_a);
        let refs_b = index.get_unresolved(&key_b);
        assert_eq!(refs_a.len(), 1);
        assert_eq!(refs_b.len(), 1);
        assert_eq!(refs_a[0].file_id, FileId(3));
        assert_eq!(refs_b[0].file_id, FileId(4));
        Ok(())
    }

    #[test]
    fn unresolved_name_does_not_suppress_a_cross_file_entity_target()
    -> Result<(), Box<dyn std::error::Error>> {
        // An occurrence whose declaration lives in another shard: the edge
        // names the target entity, but no local entity row supplies a name.
        let target = EntityId(500);
        let shard = unresolved_shard_with(
            "file:///lib/Caller.pm",
            FileId(5),
            OccurrenceId(902),
            None,
            Vec::new(),
            vec![reference_edge(EdgeId(1500), OccurrenceId(902), target)],
        );

        let mut index = ReferenceIndex::new();
        index.add_file(&shard);

        // Entity resolution is unaffected by the missing name...
        let by_entity = index.get_by_entity(target);
        assert_eq!(by_entity.len(), 1);
        assert_eq!(by_entity[0].target_candidates, vec![target]);

        // ...and the occurrence does not acquire a name it does not have.
        assert_eq!(index.name_count(), 0);
        let key = UnresolvedOccurrenceKey::new(FileId(5), OccurrenceId(902), AnchorId(60));
        let unresolved = index.get_unresolved(&key);
        assert_eq!(unresolved.len(), 1);
        // One allocation is shared across projections, as for named edges.
        assert!(Arc::ptr_eq(&by_entity[0], &unresolved[0]));
        Ok(())
    }

    #[test]
    fn entity_declared_in_another_shard_yields_no_synthesized_name()
    -> Result<(), Box<dyn std::error::Error>> {
        // The occurrence names its target, but the declaration lives in another
        // shard: `entity_id` is `Some` and the local `find` fails. This is the
        // branch `derive_canonical_name` documents and the one #8083's entity
        // catalog will eventually resolve — until then it must produce *no*
        // name at all, not a stand-in derived from the entity id.
        let absent_target = EntityId(901);
        // Local rows on both sides of the target id, so the `e.id == entity_id`
        // scan walks past a lower and a higher row without matching either.
        // Together with the equal case in `add_file_populates_name_index`
        // (occurrence and row both `EntityId(100)`), that covers the equality
        // boundary from below, at, and above.
        let declared_below = EntityId(900);
        let declared_above = EntityId(902);
        let shard = unresolved_shard_with(
            "file:///lib/Importer.pm",
            FileId(6),
            OccurrenceId(903),
            Some(absent_target),
            vec![
                declared_entity(declared_below, "Importer::below"),
                declared_entity(declared_above, "Importer::above"),
            ],
            Vec::new(),
        );
        assert!(
            declared_below < absent_target && absent_target < declared_above,
            "the fixture must straddle the target id, not match it"
        );

        let mut index = ReferenceIndex::new();
        index.add_file(&shard);

        // No name is invented for the missing declaration — under any spelling.
        assert_eq!(index.name_count(), 0, "no name may be synthesized from an absent declaration");
        assert!(index.get_by_name("Importer::below").is_empty());
        assert!(index.get_by_name("Importer::above").is_empty());

        let key = UnresolvedOccurrenceKey::new(FileId(6), OccurrenceId(903), AnchorId(60));
        let unresolved = index.get_unresolved(&key);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].symbol_key, "");

        // The occurrence's own target still reaches the entity projection: an
        // underivable name never suppresses a known target.
        assert_eq!(index.get_by_entity(absent_target).len(), 1);
        Ok(())
    }

    #[test]
    fn remove_file_purges_only_its_own_unresolved_rows() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&unresolved_shard("file:///lib/A.pm", FileId(3), OccurrenceId(900)));
        index.add_file(&unresolved_shard("file:///lib/B.pm", FileId(4), OccurrenceId(901)));

        index.remove_file("file:///lib/A.pm");

        let key_a = UnresolvedOccurrenceKey::new(FileId(3), OccurrenceId(900), AnchorId(60));
        let key_b = UnresolvedOccurrenceKey::new(FileId(4), OccurrenceId(901), AnchorId(60));
        assert!(index.get_unresolved(&key_a).is_empty());
        assert_eq!(index.get_unresolved(&key_b).len(), 1);
        assert_eq!(index.unresolved_count(), 1);
        Ok(())
    }

    #[test]
    fn edge_targets_populate_candidates() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ReferenceIndex::new();
        index.add_file(&sample_shard());

        let refs = index.get_by_entity(EntityId(100));
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target_candidates, vec![EntityId(100)]);
        Ok(())
    }

    #[test]
    fn duplicate_occurrence_ids_retain_edge_targets() -> Result<(), Box<dyn std::error::Error>> {
        let mut shard = sample_shard();
        shard.entities.push(EntityFact {
            id: EntityId(101),
            kind: EntityKind::Subroutine,
            canonical_name: "Foo::bar".to_string(),
            anchor_id: None,
            scope_id: None,
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        });
        shard.occurrences.push(OccurrenceFact {
            id: OccurrenceId(400),
            kind: OccurrenceKind::Call,
            entity_id: Some(EntityId(101)),
            anchor_id: AnchorId(21),
            scope_id: None,
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        });

        let mut index = ReferenceIndex::new();
        index.add_file(&shard);

        let by_name = index.get_by_name("Foo::bar");
        assert_eq!(by_name.len(), 2);
        assert!(
            by_name.iter().all(|reference| { reference.target_candidates == vec![EntityId(100)] })
        );
        assert_eq!(index.get_by_entity(EntityId(100)).len(), 2);
        assert!(index.get_by_entity(EntityId(101)).is_empty());
        Ok(())
    }

    #[test]
    fn multiple_edge_targets_produce_multiple_entity_entries()
    -> Result<(), Box<dyn std::error::Error>> {
        let file_id = FileId(4);
        let occ_id = OccurrenceId(1000);
        let anchor_id = AnchorId(70);
        let entity_a = EntityId(300);
        let entity_b = EntityId(301);

        let shard = FileFactShard {
            source_uri: "file:///lib/Ambiguous.pm".to_string(),
            file_id,
            content_hash: 333,
            producer_schema_version: PRODUCER_SCHEMA_VERSION,
            anchors_hash: None,
            entities_hash: None,
            occurrences_hash: None,
            edges_hash: None,
            anchors: vec![AnchorFact {
                id: anchor_id,
                file_id,
                span_start_byte: 0,
                span_end_byte: 5,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::High,
            }],
            entities: vec![
                EntityFact {
                    id: entity_a,
                    kind: EntityKind::Subroutine,
                    canonical_name: "ambig_func".to_string(),
                    anchor_id: None,
                    scope_id: None,
                    provenance: Provenance::ExactAst,
                    confidence: Confidence::High,
                },
                EntityFact {
                    id: entity_b,
                    kind: EntityKind::Subroutine,
                    canonical_name: "ambig_func".to_string(),
                    anchor_id: None,
                    scope_id: None,
                    provenance: Provenance::NameHeuristic,
                    confidence: Confidence::Low,
                },
            ],
            occurrences: vec![OccurrenceFact {
                id: occ_id,
                kind: OccurrenceKind::Call,
                entity_id: Some(entity_a),
                anchor_id,
                scope_id: None,
                provenance: Provenance::ExactAst,
                confidence: Confidence::Medium,
            }],
            edges: vec![
                EdgeFact {
                    id: EdgeId(1001),
                    kind: EdgeKind::References,
                    from_entity_id: EntityId(0),
                    to_entity_id: entity_a,
                    via_occurrence_id: Some(occ_id),
                    provenance: Provenance::ExactAst,
                    confidence: Confidence::High,
                },
                EdgeFact {
                    id: EdgeId(1002),
                    kind: EdgeKind::References,
                    from_entity_id: EntityId(0),
                    to_entity_id: entity_b,
                    via_occurrence_id: Some(occ_id),
                    provenance: Provenance::NameHeuristic,
                    confidence: Confidence::Low,
                },
            ],
        };

        let mut index = ReferenceIndex::new();
        index.add_file(&shard);

        // Both entities should have entries in the entity index.
        let refs_a = index.get_by_entity(entity_a);
        assert_eq!(refs_a.len(), 1);
        assert_eq!(refs_a[0].target_candidates.len(), 2);

        let refs_b = index.get_by_entity(entity_b);
        assert_eq!(refs_b.len(), 1);
        assert_eq!(refs_b[0].target_candidates.len(), 2);

        // Name index should have one entry (same symbol key).
        let refs_name = index.get_by_name("ambig_func");
        assert_eq!(refs_name.len(), 1);
        assert_eq!(refs_name[0].target_candidates.len(), 2);
        assert!(Arc::ptr_eq(&refs_a[0], &refs_name[0]));
        assert!(Arc::ptr_eq(&refs_b[0], &refs_name[0]));

        Ok(())
    }
}
