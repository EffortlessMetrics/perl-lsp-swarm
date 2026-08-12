//! Framework-neutral test discovery identity.
//!
//! This module owns the transport-free identity and snapshot contract shared by
//! code lenses, Test Explorer, run-at-cursor, runner planning, and debug target
//! construction. It describes discovered source items only: it does not run
//! tests, parse TAP, or contain VS Code/LSP types.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Confidence, Digest, SourceRange, fnv1a};

/// Schema version for serialized [`TestItemSnapshot`] values.
pub const TEST_ITEM_SCHEMA_VERSION: u32 = 1;

/// Stable identity of one discovered test item.
///
/// Identity is derived from the logical source identity, parent identity, item
/// kind, and a caller-supplied structural key. Display names are deliberately
/// absent, so duplicate labels and a label rename do not collapse or redefine
/// an item. The structural key must be deterministic and host-path-free.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TestItemId(String);

impl TestItemId {
    /// Derive an item ID from canonical structural identity fields.
    #[must_use]
    pub fn new(
        source_id: &str,
        parent_id: Option<&Self>,
        kind: TestItemKind,
        structural_key: &str,
    ) -> Self {
        let parent = parent_id.map_or("<root>", Self::as_str);
        let material = format!(
            "{source_id}\0{parent}\0{}\0{structural_key}",
            kind.identity_tag()
        );
        Self(format!("test:fnv64:{:016x}", fnv1a(material.as_bytes())))
    }

    /// Stable string representation, including the `test:fnv64:` prefix.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TestItemId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Kind of source item represented by a [`TestItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestItemKind {
    /// File-level runnable test item.
    File,
    /// Named Perl subroutine recognized under a reviewed test convention.
    NamedSubroutine,
    /// Framework subtest call.
    Subtest,
    /// Source-backed generated test item.
    Generated,
    /// Other reviewed framework-neutral item.
    Other,
}

impl TestItemKind {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::NamedSubroutine => "named_subroutine",
            Self::Subtest => "subtest",
            Self::Generated => "generated",
            Self::Other => "other",
        }
    }
}

/// Static or dynamic source name state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum TestItemName {
    /// Statically known display name.
    Named(String),
    /// Runtime-computed or otherwise non-static name.
    Dynamic,
}

/// Framework/module identity contributing the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestFrameworkIdentity {
    /// Stable framework family, such as `test2` or `test_more`.
    pub family: String,
    /// Activating module, such as `Test2::V0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Resolved framework/module version, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Operations a consumer may truthfully offer for an item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestItemCapabilities {
    /// The item can be submitted to a run planner.
    pub runnable: bool,
    /// The item can produce a real debug plan or delegation.
    pub debuggable: bool,
    /// A whole-file result can be focused on this item.
    pub focusable: bool,
    /// The real runner is proven to support selective execution of this item.
    pub selectively_runnable: bool,
}

/// One discovered test item with generation-owned source identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestItem {
    /// Stable structural item identity.
    pub id: TestItemId,
    /// Parent item; only the file item is parentless.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<TestItemId>,
    /// Stable sibling order assigned by the discovery producer.
    pub order_in_parent: u32,
    /// Logical, host-path-free source identity.
    pub source_id: String,
    /// Content digest for the generation that produced this item.
    pub source_digest: Digest,
    /// Accepted source/document generation.
    pub generation: u64,
    /// Deterministic structural key used to derive [`Self::id`].
    pub structural_key: String,
    /// Item kind.
    pub kind: TestItemKind,
    /// Static or dynamic name state.
    pub name: TestItemName,
    /// Range of the name expression, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_range: Option<SourceRange>,
    /// Full source range owned by the item.
    pub range: SourceRange,
    /// Framework provenance, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<TestFrameworkIdentity>,
    /// Confidence in the discovery fact.
    pub confidence: Confidence,
    /// Truthful operation capabilities.
    pub capabilities: TestItemCapabilities,
    /// Stable limitation or dynamic-boundary IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

impl TestItem {
    /// Recompute the item ID from the stored identity material.
    #[must_use]
    pub fn expected_id(&self) -> TestItemId {
        TestItemId::new(
            self.source_id.as_str(),
            self.parent_id.as_ref(),
            self.kind,
            self.structural_key.as_str(),
        )
    }
}

/// Deterministic discovery snapshot for one logical source generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestItemSnapshot {
    /// Snapshot schema version.
    pub schema_version: u32,
    /// Logical, host-path-free source identity.
    pub source_id: String,
    /// Content digest for this snapshot.
    pub source_digest: Digest,
    /// Accepted source/document generation.
    pub generation: u64,
    /// Source length in bytes, used to validate stored ranges.
    pub source_len: u32,
    /// Items in canonical ID order.
    pub items: Vec<TestItem>,
}

impl TestItemSnapshot {
    /// Build a snapshot and canonicalize item serialization order by ID.
    #[must_use]
    pub fn new(
        source_id: String,
        source_digest: Digest,
        generation: u64,
        source_len: u32,
        mut items: Vec<TestItem>,
    ) -> Self {
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Self {
            schema_version: TEST_ITEM_SCHEMA_VERSION,
            source_id,
            source_digest,
            generation,
            source_len,
            items,
        }
    }

    /// Validate identity, hierarchy, range, generation, and canonical-order invariants.
    pub fn validate(&self) -> Result<(), TestItemValidationError> {
        if self.schema_version != TEST_ITEM_SCHEMA_VERSION {
            return Err(TestItemValidationError::UnsupportedSchema {
                observed: self.schema_version,
            });
        }
        if self.source_id.is_empty() {
            return Err(TestItemValidationError::EmptySourceId);
        }
        if !self.items.windows(2).all(|pair| pair[0].id < pair[1].id) {
            return Err(TestItemValidationError::NonCanonicalItemOrder);
        }

        let by_id: BTreeMap<&TestItemId, &TestItem> =
            self.items.iter().map(|item| (&item.id, item)).collect();
        if by_id.len() != self.items.len() {
            return Err(TestItemValidationError::DuplicateItemId);
        }

        let roots: Vec<&TestItem> = self
            .items
            .iter()
            .filter(|item| item.parent_id.is_none())
            .collect();
        if roots.len() != 1 || roots[0].kind != TestItemKind::File {
            return Err(TestItemValidationError::InvalidRootCount {
                observed: roots.len(),
            });
        }

        let mut sibling_slots = BTreeSet::new();
        for item in &self.items {
            self.validate_item(item, &by_id)?;
            let slot = (item.parent_id.clone(), item.order_in_parent);
            if !sibling_slots.insert(slot) {
                return Err(TestItemValidationError::DuplicateSiblingOrder {
                    item_id: item.id.clone(),
                });
            }
        }

        for item in &self.items {
            let mut seen = BTreeSet::new();
            let mut cursor = item.parent_id.as_ref();
            while let Some(parent_id) = cursor {
                if !seen.insert(parent_id.clone()) {
                    return Err(TestItemValidationError::ParentCycle {
                        item_id: item.id.clone(),
                    });
                }
                cursor = by_id.get(parent_id).and_then(|parent| parent.parent_id.as_ref());
            }
        }

        Ok(())
    }

    fn validate_item(
        &self,
        item: &TestItem,
        by_id: &BTreeMap<&TestItemId, &TestItem>,
    ) -> Result<(), TestItemValidationError> {
        if item.source_id != self.source_id
            || item.source_digest != self.source_digest
            || item.generation != self.generation
        {
            return Err(TestItemValidationError::SnapshotIdentityMismatch {
                item_id: item.id.clone(),
            });
        }
        if item.id != item.expected_id() {
            return Err(TestItemValidationError::ItemIdMismatch {
                item_id: item.id.clone(),
            });
        }
        if !valid_range(item.range, self.source_len) {
            return Err(TestItemValidationError::InvalidRange {
                item_id: item.id.clone(),
            });
        }
        if let Some(name_range) = item.name_range
            && (!valid_range(name_range, self.source_len)
                || name_range.start_byte < item.range.start_byte
                || name_range.end_byte > item.range.end_byte)
        {
            return Err(TestItemValidationError::InvalidNameRange {
                item_id: item.id.clone(),
            });
        }
        if matches!(item.name, TestItemName::Named(ref name) if name.is_empty()) {
            return Err(TestItemValidationError::EmptyNamedItem {
                item_id: item.id.clone(),
            });
        }
        if item.kind == TestItemKind::File {
            if item.parent_id.is_some() {
                return Err(TestItemValidationError::FileHasParent {
                    item_id: item.id.clone(),
                });
            }
        } else {
            let parent_id = item
                .parent_id
                .as_ref()
                .ok_or_else(|| TestItemValidationError::MissingParent {
                    item_id: item.id.clone(),
                })?;
            let parent = by_id.get(parent_id).ok_or_else(|| {
                TestItemValidationError::UnknownParent {
                    item_id: item.id.clone(),
                    parent_id: parent_id.clone(),
                }
            })?;
            if item.range.start_byte < parent.range.start_byte
                || item.range.end_byte > parent.range.end_byte
            {
                return Err(TestItemValidationError::ChildOutsideParent {
                    item_id: item.id.clone(),
                    parent_id: parent_id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Return direct children in stable producer-assigned sibling order.
    #[must_use]
    pub fn children_of(&self, parent_id: &TestItemId) -> Vec<&TestItem> {
        let mut children: Vec<&TestItem> = self
            .items
            .iter()
            .filter(|item| item.parent_id.as_ref() == Some(parent_id))
            .collect();
        children.sort_by(|left, right| {
            left.order_in_parent
                .cmp(&right.order_in_parent)
                .then_with(|| left.id.cmp(&right.id))
        });
        children
    }

    /// Find the smallest source item containing `byte_offset`.
    ///
    /// The file item is returned as a fallback when no narrower item contains
    /// the position.
    #[must_use]
    pub fn nearest_at(&self, byte_offset: u32) -> Option<&TestItem> {
        self.items
            .iter()
            .filter(|item| {
                item.range.start_byte <= byte_offset && byte_offset < item.range.end_byte
            })
            .min_by(|left, right| {
                span_len(left.range)
                    .cmp(&span_len(right.range))
                    .then_with(|| left.id.cmp(&right.id))
            })
    }

    /// Compare two snapshots by stable item identity.
    #[must_use]
    pub fn diff(&self, newer: &Self) -> TestItemDelta {
        let old: BTreeMap<&TestItemId, &TestItem> =
            self.items.iter().map(|item| (&item.id, item)).collect();
        let new: BTreeMap<&TestItemId, &TestItem> =
            newer.items.iter().map(|item| (&item.id, item)).collect();

        let added = new
            .keys()
            .filter(|id| !old.contains_key(*id))
            .map(|id| (*id).clone())
            .collect();
        let removed = old
            .keys()
            .filter(|id| !new.contains_key(*id))
            .map(|id| (*id).clone())
            .collect();
        let changed = new
            .iter()
            .filter_map(|(id, item)| {
                old.get(id)
                    .is_some_and(|previous| *previous != *item)
                    .then(|| (*id).clone())
            })
            .collect();

        TestItemDelta {
            old_generation: self.generation,
            new_generation: newer.generation,
            added,
            changed,
            removed,
        }
    }
}

fn valid_range(range: SourceRange, source_len: u32) -> bool {
    range.start_byte <= range.end_byte && range.end_byte <= source_len
}

fn span_len(range: SourceRange) -> u32 {
    range.end_byte.saturating_sub(range.start_byte)
}

/// Deterministic item changes between two snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestItemDelta {
    /// Previous snapshot generation.
    pub old_generation: u64,
    /// New snapshot generation.
    pub new_generation: u64,
    /// Newly discovered item IDs.
    pub added: Vec<TestItemId>,
    /// Existing IDs whose facts or capabilities changed.
    pub changed: Vec<TestItemId>,
    /// Removed item IDs.
    pub removed: Vec<TestItemId>,
}

/// Validation failure for a discovery snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestItemValidationError {
    /// Snapshot schema is not supported by this build.
    UnsupportedSchema {
        /// Observed schema version.
        observed: u32,
    },
    /// Logical source identity is empty.
    EmptySourceId,
    /// Items are not strictly sorted by ID.
    NonCanonicalItemOrder,
    /// Two items carry the same ID.
    DuplicateItemId,
    /// Snapshot does not contain exactly one parentless file item.
    InvalidRootCount {
        /// Number of parentless items observed.
        observed: usize,
    },
    /// Item source/digest/generation differs from its snapshot.
    SnapshotIdentityMismatch {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Stored item ID does not match its structural identity material.
    ItemIdMismatch {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Item range is reversed or outside the source.
    InvalidRange {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Name range is invalid or outside the item range.
    InvalidNameRange {
        /// Offending item.
        item_id: TestItemId,
    },
    /// A statically named item has an empty name.
    EmptyNamedItem {
        /// Offending item.
        item_id: TestItemId,
    },
    /// File item incorrectly has a parent.
    FileHasParent {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Non-file item has no parent.
    MissingParent {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Parent ID is absent from the snapshot.
    UnknownParent {
        /// Offending item.
        item_id: TestItemId,
        /// Missing parent.
        parent_id: TestItemId,
    },
    /// Child range is not contained by its parent.
    ChildOutsideParent {
        /// Offending item.
        item_id: TestItemId,
        /// Parent item.
        parent_id: TestItemId,
    },
    /// Two siblings claim the same order slot.
    DuplicateSiblingOrder {
        /// Later item occupying the duplicate slot.
        item_id: TestItemId,
    },
    /// Parent links contain a cycle.
    ParentCycle {
        /// Item whose parent walk encountered a cycle.
        item_id: TestItemId,
    },
}

impl std::fmt::Display for TestItemValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid TestItem snapshot: {self:?}")
    }
}

impl std::error::Error for TestItemValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Utf8LineIndex;
    use std::io;

    const SOURCE_ID: &str = "workspace:root/t/example.t";

    fn source() -> &'static str {
        "subtest 'outer' => sub {\n    subtest 'same' => sub { ok(1) };\n    subtest 'same' => sub { ok(1) };\n};\n"
    }

    fn item(
        digest: &Digest,
        generation: u64,
        parent_id: Option<TestItemId>,
        order: u32,
        kind: TestItemKind,
        name: TestItemName,
        structural_key: &str,
        range: SourceRange,
        name_range: Option<SourceRange>,
    ) -> TestItem {
        let id = TestItemId::new(SOURCE_ID, parent_id.as_ref(), kind, structural_key);
        TestItem {
            id,
            parent_id,
            order_in_parent: order,
            source_id: SOURCE_ID.to_string(),
            source_digest: digest.clone(),
            generation,
            structural_key: structural_key.to_string(),
            kind,
            name,
            name_range,
            range,
            framework: Some(TestFrameworkIdentity {
                family: "test2".to_string(),
                module: Some("Test2::V0".to_string()),
                version: None,
            }),
            confidence: Confidence::High,
            capabilities: TestItemCapabilities {
                runnable: true,
                debuggable: kind == TestItemKind::File,
                focusable: kind == TestItemKind::Subtest,
                selectively_runnable: false,
            },
            limitations: Vec::new(),
        }
    }

    fn snapshot(generation: u64) -> TestItemSnapshot {
        let text = source();
        let digest = Digest::of(text);
        let index = Utf8LineIndex::new(text);
        let file = item(
            &digest,
            generation,
            None,
            0,
            TestItemKind::File,
            TestItemName::Named("t/example.t".to_string()),
            "file",
            index.source_range(0, text.len() as u32),
            None,
        );
        let outer = item(
            &digest,
            generation,
            Some(file.id.clone()),
            0,
            TestItemKind::Subtest,
            TestItemName::Named("outer".to_string()),
            "subtest:0",
            index.source_range(0, text.len() as u32 - 1),
            Some(index.source_range(8, 15)),
        );
        let first = item(
            &digest,
            generation,
            Some(outer.id.clone()),
            0,
            TestItemKind::Subtest,
            TestItemName::Named("same".to_string()),
            "subtest:1",
            index.source_range(30, 69),
            Some(index.source_range(42, 48)),
        );
        let second = item(
            &digest,
            generation,
            Some(outer.id.clone()),
            1,
            TestItemKind::Subtest,
            TestItemName::Named("same".to_string()),
            "subtest:2",
            index.source_range(74, 113),
            Some(index.source_range(86, 92)),
        );
        TestItemSnapshot::new(
            SOURCE_ID.to_string(),
            digest,
            generation,
            text.len() as u32,
            vec![second, outer, file, first],
        )
    }

    #[test]
    fn validates_nested_duplicate_names_with_distinct_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7);
        snapshot.validate()?;

        let file = snapshot
            .items
            .iter()
            .find(|item| item.kind == TestItemKind::File)
            .ok_or_else(|| io::Error::other("missing file item"))?;
        let outer = snapshot
            .children_of(&file.id)
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("missing outer item"))?;
        let children = snapshot.children_of(&outer.id);
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, TestItemName::Named("same".to_string()));
        assert_eq!(children[1].name, TestItemName::Named("same".to_string()));
        assert_ne!(children[0].id, children[1].id);
        Ok(())
    }

    #[test]
    fn nearest_item_returns_the_narrowest_containing_item()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7);
        let nearest = snapshot
            .nearest_at(50)
            .ok_or_else(|| io::Error::other("expected a containing item"))?;
        assert_eq!(nearest.name, TestItemName::Named("same".to_string()));
        Ok(())
    }

    #[test]
    fn validation_rejects_stale_item_generation() {
        let mut snapshot = snapshot(7);
        snapshot.items[0].generation = 6;
        assert!(matches!(
            snapshot.validate(),
            Err(TestItemValidationError::SnapshotIdentityMismatch { .. })
        ));
    }

    #[test]
    fn diff_preserves_stable_ids_and_reports_fact_changes() {
        let old = snapshot(7);
        let mut newer = snapshot(8);
        for item in &mut newer.items {
            item.generation = 8;
        }
        let target = newer
            .items
            .iter_mut()
            .find(|item| item.kind == TestItemKind::Subtest && item.order_in_parent == 1);
        if let Some(target) = target {
            target.capabilities.debuggable = true;
        }

        let delta = old.diff(&newer);
        assert!(delta.added.is_empty());
        assert!(delta.removed.is_empty());
        assert_eq!(delta.changed.len(), newer.items.len());
        assert_eq!((delta.old_generation, delta.new_generation), (7, 8));
    }

    #[test]
    fn serde_roundtrip_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7);
        let encoded = serde_json::to_string(&snapshot)?;
        let decoded: TestItemSnapshot = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, snapshot);
        assert_eq!(serde_json::to_string(&decoded)?, encoded);
        Ok(())
    }
}
