//! Per-category semantic fact invalidation planning.
//!
//! This module keeps the pure decision logic for incremental semantic shard
//! replacement separate from `WorkspaceIndex`, which remains responsible for
//! applying the plan to its stores and cross-file indexes.
//!
//! Whole-file `content_hash` equality is one input to reuse, never sufficient
//! on its own (#8054). A shard is observationally unchanged only when producer
//! schema, content digest, and every category fingerprint agree. Missing or
//! legacy fingerprints rebuild that category conservatively.

/// Per-category fact hashes for one semantic file shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardCategoryHashes {
    /// Whole-file content hash.
    pub content_hash: u64,
    /// Producer schema that minted the facts (not the fingerprint schema).
    pub producer_schema_version: u32,
    /// Anchor fact category hash.
    pub anchors_hash: Option<u64>,
    /// Entity fact category hash.
    pub entities_hash: Option<u64>,
    /// Occurrence fact category hash.
    pub occurrences_hash: Option<u64>,
    /// Edge fact category hash.
    pub edges_hash: Option<u64>,
}

/// Per-category incremental invalidation result.
///
/// Tracks which fact categories should be updated during a shard replacement so
/// callers can observe skip behavior in tests and apply only the required
/// cross-file index updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardReplaceResult {
    /// `true` when content digest, producer schema, and every category
    /// fingerprint prove the admitted shard is observationally unchanged.
    pub content_unchanged: bool,
    /// `true` when the anchors category should be re-indexed.
    pub anchors_updated: bool,
    /// `true` when the entities category should be re-indexed.
    pub entities_updated: bool,
    /// `true` when the occurrences category should be re-indexed.
    pub occurrences_updated: bool,
    /// `true` when the edges category should be re-indexed.
    pub edges_updated: bool,
}

fn all_categories_updated() -> ShardReplaceResult {
    ShardReplaceResult {
        content_unchanged: false,
        anchors_updated: true,
        entities_updated: true,
        occurrences_updated: true,
        edges_updated: true,
    }
}

/// Build the incremental replacement plan for old and new semantic shard
/// hashes.
pub fn plan_shard_replacement(
    old: Option<ShardCategoryHashes>,
    new: ShardCategoryHashes,
) -> ShardReplaceResult {
    let Some(old_hashes) = old else {
        return all_categories_updated();
    };

    if old_hashes.producer_schema_version != new.producer_schema_version {
        return all_categories_updated();
    }

    let anchors_updated = category_hash_changed(old_hashes.anchors_hash, new.anchors_hash);
    let entities_updated = category_hash_changed(old_hashes.entities_hash, new.entities_hash);
    let occurrences_updated =
        category_hash_changed(old_hashes.occurrences_hash, new.occurrences_hash);
    let edges_updated = category_hash_changed(old_hashes.edges_hash, new.edges_hash);

    let categories_observationally_equal =
        !anchors_updated && !entities_updated && !occurrences_updated && !edges_updated;

    if old_hashes.content_hash == new.content_hash && categories_observationally_equal {
        return ShardReplaceResult {
            content_unchanged: true,
            anchors_updated: false,
            entities_updated: false,
            occurrences_updated: false,
            edges_updated: false,
        };
    }

    ShardReplaceResult {
        content_unchanged: false,
        anchors_updated,
        entities_updated,
        occurrences_updated,
        edges_updated,
    }
}

/// Compare old and new per-category hashes.
///
/// Returns `true` when either hash is absent or when both hashes are present
/// but differ. Missing hashes represent legacy shards, so callers must
/// conservatively refresh that category.
pub fn category_hash_changed(old: Option<u64>, new: Option<u64>) -> bool {
    match (old, new) {
        (Some(o), Some(n)) => o != n,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(
        content_hash: u64,
        anchors_hash: Option<u64>,
        entities_hash: Option<u64>,
        occurrences_hash: Option<u64>,
        edges_hash: Option<u64>,
    ) -> ShardCategoryHashes {
        hashes_with_producer(
            1,
            content_hash,
            anchors_hash,
            entities_hash,
            occurrences_hash,
            edges_hash,
        )
    }

    fn hashes_with_producer(
        producer_schema_version: u32,
        content_hash: u64,
        anchors_hash: Option<u64>,
        entities_hash: Option<u64>,
        occurrences_hash: Option<u64>,
        edges_hash: Option<u64>,
    ) -> ShardCategoryHashes {
        ShardCategoryHashes {
            content_hash,
            producer_schema_version,
            anchors_hash,
            entities_hash,
            occurrences_hash,
            edges_hash,
        }
    }

    #[test]
    fn equal_content_with_changed_category_fingerprints_does_not_skip() {
        // #8054: identical source bytes must not authorize reuse when a
        // category payload (or its fingerprint) changed.
        let old = hashes(1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes(1, Some(11), Some(21), Some(31), Some(41));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(!plan.content_unchanged);
        assert!(plan.anchors_updated);
        assert!(plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(plan.edges_updated);
    }

    #[test]
    fn complete_subject_equality_skips_all_categories() {
        let old = hashes(1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes(1, Some(10), Some(20), Some(30), Some(40));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(plan.content_unchanged);
        assert!(!plan.anchors_updated);
        assert!(!plan.entities_updated);
        assert!(!plan.occurrences_updated);
        assert!(!plan.edges_updated);
    }

    #[test]
    fn equal_content_compares_each_category() {
        let old = hashes(1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes(1, Some(10), Some(21), None, Some(40));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(!plan.content_unchanged);
        assert!(!plan.anchors_updated);
        assert!(plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(!plan.edges_updated);
    }

    #[test]
    fn producer_schema_change_rebuilds_every_category() {
        let old = hashes_with_producer(1, 1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes_with_producer(2, 1, Some(10), Some(20), Some(30), Some(40));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(!plan.content_unchanged);
        assert!(plan.anchors_updated);
        assert!(plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(plan.edges_updated);
    }

    #[test]
    fn missing_legacy_fingerprint_forces_conservative_replacement() {
        let old = hashes(1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes(2, None, Some(20), None, Some(40));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(!plan.content_unchanged);
        assert!(plan.anchors_updated);
        assert!(!plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(!plan.edges_updated);
    }

    #[test]
    fn changed_content_compares_each_category() {
        let old = hashes(1, Some(10), Some(20), Some(30), Some(40));
        let new = hashes(2, Some(10), Some(21), None, Some(40));

        let plan = plan_shard_replacement(Some(old), new);

        assert!(!plan.content_unchanged);
        assert!(!plan.anchors_updated);
        assert!(plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(!plan.edges_updated);
    }

    #[test]
    fn first_insert_marks_all_categories_updated() {
        let new = hashes(7, Some(70), Some(80), Some(90), Some(100));

        let plan = plan_shard_replacement(None, new);

        assert!(!plan.content_unchanged);
        assert!(plan.anchors_updated);
        assert!(plan.entities_updated);
        assert!(plan.occurrences_updated);
        assert!(plan.edges_updated);
    }

    #[test]
    fn category_hash_changed_handles_all_hash_presence_combinations() {
        assert!(!category_hash_changed(Some(5), Some(5)));
        assert!(category_hash_changed(Some(5), Some(6)));
        assert!(category_hash_changed(None, Some(6)));
        assert!(category_hash_changed(Some(5), None));
        assert!(category_hash_changed(None, None));
    }
}
