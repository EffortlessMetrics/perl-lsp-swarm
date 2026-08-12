//! Framework-neutral test discovery identity.
//!
//! This module owns the transport-free identity and snapshot contract shared by
//! code lenses, Test Explorer, run-at-cursor, runner planning, and debug target
//! construction. It describes discovered source items only: it does not run
//! tests, parse TAP, or contain VS Code/LSP types.
//!
//! Test items reference, but do not implement, the broader `source_identity.v1`
//! contract tracked by #4835/#4851. [`SourceIdentityRef`] is an opaque SHA-256
//! reference to that future canonical envelope; no path, URI, workspace root, or
//! other free-form source string is frozen into this wire format.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Confidence, Digest, SourceRange};

/// Schema version for serialized [`TestItemSnapshot`] values.
pub const TEST_ITEM_SCHEMA_VERSION: u32 = 1;

/// Schema version of the canonical source-identity reference embedded here.
pub const SOURCE_IDENTITY_REF_SCHEMA_VERSION: u32 = 1;

const TEST_ITEM_ID_DOMAIN: &[u8] = b"perl-lsp:test-item-id:v1";

/// Opaque reference to a canonical `source_identity.v1` envelope.
///
/// The reference intentionally contains only a version and collision-resistant
/// digest. The source-identity owner remains responsible for workspace/root,
/// logical path, origin, physical location, content revision, redaction, and
/// mapping semantics. This prevents TestItem v1 from inventing a path-derived
/// identity before #4851 lands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentityRef {
    /// Referenced source-identity schema version.
    schema_version: u32,
    /// SHA-256 digest of the canonical, domain-separated source-identity envelope.
    digest_sha256: [u8; 32],
}

impl SourceIdentityRef {
    /// Build a v1 source-identity reference from the canonical envelope digest.
    #[must_use]
    pub const fn from_sha256(digest_sha256: [u8; 32]) -> Self {
        Self {
            schema_version: SOURCE_IDENTITY_REF_SCHEMA_VERSION,
            digest_sha256,
        }
    }

    /// Referenced source-identity schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Canonical envelope digest.
    #[must_use]
    pub const fn digest_sha256(&self) -> &[u8; 32] {
        &self.digest_sha256
    }
}

impl std::fmt::Display for SourceIdentityRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "source_identity.v{}:sha256:", self.schema_version)?;
        for byte in self.digest_sha256 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Stable identity of one discovered test item.
///
/// IDs use SHA-256 over domain-separated, length-prefixed structural fields:
/// source-identity reference, parent ID, item kind, and producer structural key.
/// Display names are deliberately absent, so duplicate labels and label edits do
/// not collapse or redefine an item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TestItemId(String);

impl TestItemId {
    /// Derive an item ID from canonical structural identity fields.
    #[must_use]
    pub fn new(
        source_ref: &SourceIdentityRef,
        parent_id: Option<&Self>,
        kind: TestItemKind,
        structural_key: &str,
    ) -> Self {
        let mut hasher = Sha256::new();
        update_hash_field(&mut hasher, TEST_ITEM_ID_DOMAIN);
        update_hash_field(&mut hasher, &source_ref.schema_version.to_be_bytes());
        update_hash_field(&mut hasher, source_ref.digest_sha256());
        update_hash_field(
            &mut hasher,
            parent_id.map_or(&[][..], |parent| parent.as_str().as_bytes()),
        );
        update_hash_field(&mut hasher, kind.identity_tag().as_bytes());
        update_hash_field(&mut hasher, structural_key.as_bytes());
        Self(format!("test_item.v1:sha256:{}", hex_digest(hasher.finalize().as_slice())))
    }

    /// Stable string representation, including the algorithm and domain version.
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

fn update_hash_field(hasher: &mut Sha256, field: &[u8]) {
    let length = u64::try_from(field.len()).unwrap_or(u64::MAX);
    hasher.update(length.to_be_bytes());
    hasher.update(field);
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
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
#[serde(tag = "state", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum TestItemName {
    /// Statically known display name.
    Named(String),
    /// Runtime-computed or otherwise non-static name.
    Dynamic,
}

/// Framework/module identity contributing the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct TestItem {
    /// Stable structural item identity.
    pub id: TestItemId,
    /// Parent item; only the file item is parentless.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<TestItemId>,
    /// Stable sibling order assigned by the discovery producer.
    pub order_in_parent: u32,
    /// Opaque canonical source-identity reference.
    pub source_ref: SourceIdentityRef,
    /// Content digest for the generation that produced this item.
    pub source_digest: Digest,
    /// Accepted source/document generation.
    pub generation: u64,
    /// Deterministic, host-path-free structural key assigned by the producer.
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
            &self.source_ref,
            self.parent_id.as_ref(),
            self.kind,
            self.structural_key.as_str(),
        )
    }

    fn identity_material_eq(&self, other: &Self) -> bool {
        self.source_ref == other.source_ref
            && self.parent_id == other.parent_id
            && self.kind == other.kind
            && self.structural_key == other.structural_key
    }

    fn discovery_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.parent_id == other.parent_id
            && self.order_in_parent == other.order_in_parent
            && self.source_ref == other.source_ref
            && self.structural_key == other.structural_key
            && self.kind == other.kind
            && self.name == other.name
            && self.name_range == other.name_range
            && self.range == other.range
            && self.framework == other.framework
            && self.confidence == other.confidence
            && self.capabilities == other.capabilities
            && self.limitations == other.limitations
    }
}

/// Deterministic discovery snapshot for one logical source generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestItemSnapshot {
    /// Snapshot schema version.
    pub schema_version: u32,
    /// Opaque canonical source-identity reference.
    pub source_ref: SourceIdentityRef,
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
        source_ref: SourceIdentityRef,
        source_digest: Digest,
        generation: u64,
        source_len: u32,
        mut items: Vec<TestItem>,
    ) -> Self {
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Self {
            schema_version: TEST_ITEM_SCHEMA_VERSION,
            source_ref,
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
        if self.source_ref.schema_version() != SOURCE_IDENTITY_REF_SCHEMA_VERSION {
            return Err(TestItemValidationError::UnsupportedSourceIdentitySchema {
                observed: self.source_ref.schema_version(),
            });
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
        let root = roots[0];
        if root.range.start_byte != 0 || root.range.end_byte != self.source_len {
            return Err(TestItemValidationError::FileDoesNotCoverSource {
                item_id: root.id.clone(),
                source_len: self.source_len,
            });
        }
        if root.order_in_parent != 0 {
            return Err(TestItemValidationError::InvalidFileOrder {
                item_id: root.id.clone(),
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
        if item.source_ref != self.source_ref
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
        if item.structural_key.is_empty()
            || item.structural_key.chars().any(char::is_control)
        {
            return Err(TestItemValidationError::InvalidStructuralKey {
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
        if matches!(&item.name, TestItemName::Named(name) if name.is_empty()) {
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
    /// The file item is returned for gaps, end-of-file, and an empty source.
    /// Offsets beyond the source are rejected. Callers that consume untrusted
    /// serialized snapshots should call [`Self::validate`] first.
    #[must_use]
    pub fn nearest_at(&self, byte_offset: u32) -> Option<&TestItem> {
        if byte_offset > self.source_len {
            return None;
        }
        let root = self
            .items
            .iter()
            .find(|item| item.kind == TestItemKind::File && item.parent_id.is_none());
        if self.source_len == 0 || byte_offset == self.source_len {
            return root;
        }

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
            .or(root)
    }

    /// Compare semantic discovery facts by stable item identity.
    ///
    /// Both snapshots must be valid, refer to the same canonical logical source,
    /// and advance generation strictly. Source digest and generation are snapshot
    /// freshness, not item-shape changes, so they do not churn every stable item.
    pub fn diff(&self, newer: &Self) -> Result<TestItemDelta, TestItemDeltaError> {
        self.validate().map_err(TestItemDeltaError::InvalidOlderSnapshot)?;
        newer.validate().map_err(TestItemDeltaError::InvalidNewerSnapshot)?;
        if self.source_ref != newer.source_ref {
            return Err(TestItemDeltaError::DifferentSource);
        }
        if newer.generation <= self.generation {
            return Err(TestItemDeltaError::NonMonotonicGeneration {
                older: self.generation,
                newer: newer.generation,
            });
        }

        let old: BTreeMap<TestItemId, &TestItem> =
            self.items.iter().map(|item| (item.id.clone(), item)).collect();
        let new: BTreeMap<TestItemId, &TestItem> =
            newer.items.iter().map(|item| (item.id.clone(), item)).collect();

        for (id, item) in &new {
            if let Some(previous) = old.get(id)
                && !previous.identity_material_eq(item)
            {
                return Err(TestItemDeltaError::IdentityCollision {
                    item_id: id.clone(),
                });
            }
        }

        let added = new
            .keys()
            .filter(|id| !old.contains_key(*id))
            .cloned()
            .collect();
        let removed = old
            .keys()
            .filter(|id| !new.contains_key(*id))
            .cloned()
            .collect();
        let changed = new
            .iter()
            .filter_map(|(id, item)| {
                old.get(id)
                    .is_some_and(|previous| !previous.discovery_eq(item))
                    .then(|| id.clone())
            })
            .collect();

        Ok(TestItemDelta {
            old_generation: self.generation,
            new_generation: newer.generation,
            added,
            changed,
            removed,
        })
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
#[serde(deny_unknown_fields)]
pub struct TestItemDelta {
    /// Previous snapshot generation.
    pub old_generation: u64,
    /// New snapshot generation.
    pub new_generation: u64,
    /// Newly discovered item IDs.
    pub added: Vec<TestItemId>,
    /// Existing IDs whose semantic facts or capabilities changed.
    pub changed: Vec<TestItemId>,
    /// Removed item IDs.
    pub removed: Vec<TestItemId>,
}

/// Failure to compare two TestItem snapshots safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestItemDeltaError {
    /// Older snapshot is invalid.
    InvalidOlderSnapshot(TestItemValidationError),
    /// Newer snapshot is invalid.
    InvalidNewerSnapshot(TestItemValidationError),
    /// Snapshots refer to different logical sources.
    DifferentSource,
    /// Newer generation did not advance strictly.
    NonMonotonicGeneration {
        /// Older generation.
        older: u64,
        /// Candidate newer generation.
        newer: u64,
    },
    /// The same opaque ID resolved to different retained identity material.
    IdentityCollision {
        /// Colliding item ID.
        item_id: TestItemId,
    },
}

impl std::fmt::Display for TestItemDeltaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOlderSnapshot(error) => write!(formatter, "invalid older snapshot: {error}"),
            Self::InvalidNewerSnapshot(error) => write!(formatter, "invalid newer snapshot: {error}"),
            Self::DifferentSource => formatter.write_str("cannot diff snapshots for different sources"),
            Self::NonMonotonicGeneration { older, newer } => write!(
                formatter,
                "newer generation must be greater than older generation ({newer} <= {older})"
            ),
            Self::IdentityCollision { item_id } => {
                write!(formatter, "identity material changed under stable item ID {item_id}")
            }
        }
    }
}

impl std::error::Error for TestItemDeltaError {}

/// Validation failure for a discovery snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestItemValidationError {
    /// Snapshot schema is not supported by this build.
    UnsupportedSchema {
        /// Observed schema version.
        observed: u32,
    },
    /// Referenced source-identity schema is not supported.
    UnsupportedSourceIdentitySchema {
        /// Observed schema version.
        observed: u32,
    },
    /// Items are not strictly sorted by ID.
    NonCanonicalItemOrder,
    /// Two items carry the same ID.
    DuplicateItemId,
    /// Snapshot does not contain exactly one parentless file item.
    InvalidRootCount {
        /// Number of parentless items observed.
        observed: usize,
    },
    /// File root does not span exactly `0..source_len`.
    FileDoesNotCoverSource {
        /// Offending root item.
        item_id: TestItemId,
        /// Snapshot source length.
        source_len: u32,
    },
    /// File root does not use sibling order zero.
    InvalidFileOrder {
        /// Offending file item.
        item_id: TestItemId,
    },
    /// Item source/digest/generation differs from its snapshot.
    SnapshotIdentityMismatch {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Stored item ID does not match its retained structural identity material.
    ItemIdMismatch {
        /// Offending item.
        item_id: TestItemId,
    },
    /// Structural key is empty or contains control characters.
    InvalidStructuralKey {
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

    fn source() -> &'static str {
        "subtest 'outer' => sub {\n    subtest 'same' => sub { ok(1) };\n    subtest 'same' => sub { ok(1) };\n};\n"
    }

    fn source_ref(seed: u8) -> SourceIdentityRef {
        SourceIdentityRef::from_sha256([seed; 32])
    }

    #[allow(clippy::too_many_arguments)]
    fn item(
        source_ref: &SourceIdentityRef,
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
        let id = TestItemId::new(source_ref, parent_id.as_ref(), kind, structural_key);
        TestItem {
            id,
            parent_id,
            order_in_parent: order,
            source_ref: source_ref.clone(),
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

    fn line_span(text: &str, occurrence: usize) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let starts: Vec<usize> = text.match_indices("subtest 'same'").map(|(index, _)| index).collect();
        let start = *starts
            .get(occurrence)
            .ok_or_else(|| io::Error::other("missing subtest occurrence"))?;
        let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = text[start..].find('\n').map_or(text.len(), |index| start + index);
        Ok((u32::try_from(line_start)?, u32::try_from(line_end)?))
    }

    fn snapshot_with_ref(
        generation: u64,
        source_ref: SourceIdentityRef,
    ) -> Result<TestItemSnapshot, Box<dyn std::error::Error>> {
        let text = source();
        let digest = Digest::of(text);
        let index = Utf8LineIndex::new(text);
        let source_len = u32::try_from(text.len())?;
        let file = item(
            &source_ref,
            &digest,
            generation,
            None,
            0,
            TestItemKind::File,
            TestItemName::Named("t/example.t".to_string()),
            "file",
            index.source_range(0, source_len),
            None,
        );
        let outer_name_start = u32::try_from(text.find("'outer'").ok_or_else(|| {
            io::Error::other("missing outer name")
        })?)?;
        let outer = item(
            &source_ref,
            &digest,
            generation,
            Some(file.id.clone()),
            0,
            TestItemKind::Subtest,
            TestItemName::Named("outer".to_string()),
            "subtest:0",
            index.source_range(0, source_len.saturating_sub(1)),
            Some(index.source_range(
                outer_name_start.saturating_add(1),
                outer_name_start.saturating_add(6),
            )),
        );
        let (first_start, first_end) = line_span(text, 0)?;
        let (second_start, second_end) = line_span(text, 1)?;
        let first_name = u32::try_from(text[first_start as usize..]
            .find("'same'")
            .ok_or_else(|| io::Error::other("missing first name"))?)?
            .saturating_add(first_start)
            .saturating_add(1);
        let second_name = u32::try_from(text[second_start as usize..]
            .find("'same'")
            .ok_or_else(|| io::Error::other("missing second name"))?)?
            .saturating_add(second_start)
            .saturating_add(1);
        let first = item(
            &source_ref,
            &digest,
            generation,
            Some(outer.id.clone()),
            0,
            TestItemKind::Subtest,
            TestItemName::Named("same".to_string()),
            "subtest:1",
            index.source_range(first_start, first_end),
            Some(index.source_range(first_name, first_name.saturating_add(4))),
        );
        let second = item(
            &source_ref,
            &digest,
            generation,
            Some(outer.id.clone()),
            1,
            TestItemKind::Subtest,
            TestItemName::Named("same".to_string()),
            "subtest:2",
            index.source_range(second_start, second_end),
            Some(index.source_range(second_name, second_name.saturating_add(4))),
        );
        Ok(TestItemSnapshot::new(
            source_ref,
            digest,
            generation,
            source_len,
            vec![second, outer, file, first],
        ))
    }

    fn snapshot(generation: u64) -> Result<TestItemSnapshot, Box<dyn std::error::Error>> {
        snapshot_with_ref(generation, source_ref(1))
    }

    fn empty_snapshot(generation: u64) -> TestItemSnapshot {
        let source_ref = source_ref(1);
        let digest = Digest::of("");
        let range = Utf8LineIndex::new("").source_range(0, 0);
        let file = item(
            &source_ref,
            &digest,
            generation,
            None,
            0,
            TestItemKind::File,
            TestItemName::Named("t/empty.t".to_string()),
            "file",
            range,
            None,
        );
        TestItemSnapshot::new(source_ref, digest, generation, 0, vec![file])
    }

    fn root_mut(snapshot: &mut TestItemSnapshot) -> Result<&mut TestItem, Box<dyn std::error::Error>> {
        snapshot
            .items
            .iter_mut()
            .find(|item| item.kind == TestItemKind::File)
            .ok_or_else(|| io::Error::other("missing file item").into())
    }

    #[test]
    fn validates_nested_duplicate_names_with_distinct_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7)?;
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
    fn ids_are_domain_separated_and_source_scoped() {
        let first_source = source_ref(1);
        let second_source = source_ref(2);
        let root = TestItemId::new(&first_source, None, TestItemKind::File, "file");
        let child = TestItemId::new(
            &first_source,
            Some(&root),
            TestItemKind::Subtest,
            "subtest:1",
        );
        let different_key = TestItemId::new(
            &first_source,
            Some(&root),
            TestItemKind::Subtest,
            "subtest:2",
        );
        let different_source = TestItemId::new(
            &second_source,
            Some(&root),
            TestItemKind::Subtest,
            "subtest:1",
        );

        assert!(root.as_str().starts_with("test_item.v1:sha256:"));
        assert_ne!(child, different_key);
        assert_ne!(child, different_source);
    }

    #[test]
    fn validation_detects_identity_material_substitution()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut snapshot = snapshot(7)?;
        let target = snapshot
            .items
            .iter_mut()
            .find(|item| item.kind == TestItemKind::Subtest)
            .ok_or_else(|| io::Error::other("missing subtest"))?;
        target.structural_key.push_str(":substituted");

        assert!(matches!(
            snapshot.validate(),
            Err(TestItemValidationError::ItemIdMismatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn nearest_item_returns_the_narrowest_containing_item()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7)?;
        let target = snapshot
            .items
            .iter()
            .find(|item| item.structural_key == "subtest:1")
            .ok_or_else(|| io::Error::other("missing first subtest"))?;
        let nearest = snapshot
            .nearest_at(target.range.start_byte.saturating_add(1))
            .ok_or_else(|| io::Error::other("expected a containing item"))?;
        assert_eq!(nearest.id, target.id);
        Ok(())
    }

    #[test]
    fn nearest_item_returns_file_at_eof_and_for_empty_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7)?;
        let eof = snapshot
            .nearest_at(snapshot.source_len)
            .ok_or_else(|| io::Error::other("missing EOF fallback"))?;
        assert_eq!(eof.kind, TestItemKind::File);
        assert!(snapshot.nearest_at(snapshot.source_len.saturating_add(1)).is_none());

        let empty = empty_snapshot(1);
        empty.validate()?;
        let empty_item = empty
            .nearest_at(0)
            .ok_or_else(|| io::Error::other("missing empty-file fallback"))?;
        assert_eq!(empty_item.kind, TestItemKind::File);
        Ok(())
    }

    #[test]
    fn validation_requires_file_root_to_cover_complete_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut missing_prefix = snapshot(7)?;
        root_mut(&mut missing_prefix)?.range.start_byte = 1;
        assert!(matches!(
            missing_prefix.validate(),
            Err(TestItemValidationError::FileDoesNotCoverSource { .. })
        ));

        let mut missing_suffix = snapshot(7)?;
        root_mut(&mut missing_suffix)?.range.end_byte = missing_suffix.source_len.saturating_sub(1);
        assert!(matches!(
            missing_suffix.validate(),
            Err(TestItemValidationError::FileDoesNotCoverSource { .. })
        ));
        Ok(())
    }

    #[test]
    fn validation_rejects_stale_item_generation() -> Result<(), Box<dyn std::error::Error>> {
        let mut snapshot = snapshot(7)?;
        snapshot.items[0].generation = 6;
        assert!(matches!(
            snapshot.validate(),
            Err(TestItemValidationError::SnapshotIdentityMismatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn diff_preserves_stable_ids_across_generations()
    -> Result<(), Box<dyn std::error::Error>> {
        let old = snapshot(7)?;
        let mut newer = snapshot(8)?;
        let target = newer
            .items
            .iter_mut()
            .find(|item| item.kind == TestItemKind::Subtest && item.order_in_parent == 1)
            .ok_or_else(|| io::Error::other("missing target subtest"))?;
        target.capabilities.debuggable = true;

        let delta = old.diff(&newer)?;
        assert!(delta.added.is_empty());
        assert!(delta.removed.is_empty());
        assert_eq!(delta.changed.len(), 1);
        assert_eq!((delta.old_generation, delta.new_generation), (7, 8));
        Ok(())
    }

    #[test]
    fn equivalent_new_generation_does_not_churn_items()
    -> Result<(), Box<dyn std::error::Error>> {
        let old = snapshot(7)?;
        let newer = snapshot(8)?;
        let delta = old.diff(&newer)?;
        assert!(delta.added.is_empty());
        assert!(delta.changed.is_empty());
        assert!(delta.removed.is_empty());
        Ok(())
    }

    #[test]
    fn diff_rejects_cross_source_and_non_monotonic_subjects()
    -> Result<(), Box<dyn std::error::Error>> {
        let old = snapshot(7)?;
        let other_source = snapshot_with_ref(8, source_ref(2))?;
        assert!(matches!(
            old.diff(&other_source),
            Err(TestItemDeltaError::DifferentSource)
        ));

        let equal_generation = snapshot(7)?;
        assert!(matches!(
            old.diff(&equal_generation),
            Err(TestItemDeltaError::NonMonotonicGeneration { .. })
        ));
        let reversed = snapshot(6)?;
        assert!(matches!(
            old.diff(&reversed),
            Err(TestItemDeltaError::NonMonotonicGeneration { .. })
        ));
        Ok(())
    }

    #[test]
    fn diff_rejects_invalid_snapshot_subjects() -> Result<(), Box<dyn std::error::Error>> {
        let old = snapshot(7)?;
        let mut invalid = snapshot(8)?;
        invalid.items[0].generation = 7;
        assert!(matches!(
            old.diff(&invalid),
            Err(TestItemDeltaError::InvalidNewerSnapshot(_))
        ));
        Ok(())
    }

    #[test]
    fn serde_roundtrip_is_deterministic_and_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot(7)?;
        let encoded = serde_json::to_string(&snapshot)?;
        let decoded: TestItemSnapshot = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, snapshot);
        assert_eq!(serde_json::to_string(&decoded)?, encoded);

        let mut top_level_extra = serde_json::to_value(&snapshot)?;
        top_level_extra
            .as_object_mut()
            .ok_or_else(|| io::Error::other("snapshot must serialize as an object"))?
            .insert("future_authority".to_string(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<TestItemSnapshot>(top_level_extra).is_err());

        let mut nested_extra = serde_json::to_value(&snapshot)?;
        let first_item = nested_extra
            .get_mut("items")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|items| items.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("missing serialized item"))?;
        first_item.insert("future_identity".to_string(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<TestItemSnapshot>(nested_extra).is_err());
        Ok(())
    }

    #[test]
    fn source_identity_reference_is_versioned_and_path_free() {
        let reference = source_ref(0xab);
        let rendered = reference.to_string();
        assert!(rendered.starts_with("source_identity.v1:sha256:"));
        assert!(!rendered.contains('/') && !rendered.contains('\\'));
        assert_eq!(reference.schema_version(), SOURCE_IDENTITY_REF_SCHEMA_VERSION);
    }
}
