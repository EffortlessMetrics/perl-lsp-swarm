//! The immutable [`SemanticQueryView`]: deterministic core indexes derived
//! from exactly one accepted [`ProjectModel`] generation (issue #8934).
//!
//! The view is a pure, side-effect-free materialization. The builder reads no
//! source, invokes no parser or analyzer, scans no filesystem, and mints no
//! new semantic identity: every row is keyed by the canonical [`FileId`],
//! [`SymbolId`], or [`PackageId`] already owned by the substrate, so local
//! table positions are trivially view-private and map straight back to
//! canonical identity.
//!
//! Each index family reports a typed [`IndexCompleteness`]:
//!
//! - [`IndexCompleteness::Complete`] — the admitted fact-family denominator
//!   was fully consumed;
//! - [`IndexCompleteness::Partial`] — rows exist but the model recorded
//!   limitations that bound them;
//! - [`IndexCompleteness::NotProven`] — the instrumentation behind a family
//!   is absent from every accepted generation today (occurrence and
//!   generated-member facts). Missing instrumentation is never reported as a
//!   legitimate zero.
//!
//! Output order and the view fingerprint are deterministic under input
//! ordering: all containers are ordered maps over canonical keys and rows are
//! sorted before insertion, so permuting the model's fact vectors produces a
//! byte-identical view. This guarantee covers the fingerprint and every index
//! table; it deliberately excludes [`SemanticQueryView::model_snapshot_identity`],
//! which tracks the raw model serialization (including fact-vector order) by
//! design.
//!
//! Generation semantics: `ProjectModel` carries per-file generation identity
//! only — there is no model-level accepted-generation counter. Independent
//! files legitimately sit at different generations in one steady-state model,
//! so this view does not enforce cross-shard generation uniformity. The
//! accepted-generation basis is [`SemanticQueryView::model_snapshot_identity`];
//! caller-supplied floors (`CheckedBuildInput::min_shard_generation`) are
//! per-shard floors, checked against each adopted shard individually.
//!
//! Non-goals: no package/relationship indexes, no query matching/ranking, no
//! LSP projection, no live publication, no persistence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::fact_classes::FactClasses;
use crate::file::{FileRole, ParseStatus};
use crate::id::{Digest, FileId, PackageId, SymbolId};
use crate::model::ProjectModel;
use crate::range::SourceRange;
use crate::shard::ShardError;
use crate::symbol::{SymbolFactKind, Visibility};
use crate::{SCHEMA_VERSION, fnv1a};

/// Why an index family cannot prove completeness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotProvenReason {
    /// The model generation never carried this fact family; no producer for
    /// it exists in any accepted generation yet.
    InstrumentationAbsent {
        /// The absent fact family, named for diagnostics.
        family: &'static str,
    },
    /// The generating request did not admit the fact class this index needs.
    FactClassNotAdmitted,
    /// An adopted shard never populated this fact class, so its contribution
    /// to the denominator is unproven: missing extraction is not zero.
    ShardClassNotPopulated {
        /// The fact family whose per-shard population is unproven.
        family: &'static str,
    },
}

impl fmt::Display for NotProvenReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstrumentationAbsent { family } => {
                write!(f, "no accepted generation carries {family}")
            }
            Self::FactClassNotAdmitted => {
                write!(f, "generating request did not admit the required fact class")
            }
            Self::ShardClassNotPopulated { family } => {
                write!(f, "adopted shards never populated {family} facts")
            }
        }
    }
}

/// Typed completeness of one index family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexCompleteness {
    /// Every row of the admitted denominator was indexed.
    Complete,
    /// Rows are indexed but bounded by recorded model limitations.
    Partial {
        /// Stable limitation ids bounding the family, sorted and deduped.
        limitation_ids: Vec<String>,
    },
    /// Completeness cannot be claimed; missing instrumentation is not zero.
    NotProven(NotProvenReason),
}

/// The answer to one view lookup, pairing rows with their completeness class.
///
/// An exact-empty answer is only ever delivered as [`IndexAnswer::Complete`]
/// with no rows: a legitimate empty requires a complete denominator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexAnswer<'v, T> {
    /// Complete-denominator rows (possibly legitimately empty).
    Complete(T),
    /// Rows bounded by recorded limitations.
    Partial {
        /// The matched rows.
        rows: T,
        /// Limitation ids qualifying the rows.
        limitation_ids: Vec<&'v str>,
    },
    /// The family has no provable denominator.
    NotProven(NotProvenReason),
}

impl<'v, T> IndexAnswer<'v, T> {
    /// The rows, if the family proved any denominator at all.
    #[must_use]
    pub fn rows(self) -> Option<T> {
        match self {
            Self::Complete(rows) | Self::Partial { rows, .. } => Some(rows),
            Self::NotProven(_) => None,
        }
    }

    /// The completeness class of this answer.
    #[must_use]
    pub fn completeness(&self) -> IndexCompleteness {
        match self {
            Self::Complete(_) => IndexCompleteness::Complete,
            Self::Partial { limitation_ids, .. } => IndexCompleteness::Partial {
                limitation_ids: limitation_ids.iter().map(|id| (*id).to_owned()).collect(),
            },
            Self::NotProven(reason) => IndexCompleteness::NotProven(*reason),
        }
    }

    /// Re-map the rows while preserving the completeness class.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> IndexAnswer<'v, U> {
        match self {
            Self::Complete(rows) => IndexAnswer::Complete(f(rows)),
            Self::Partial { rows, limitation_ids } => {
                IndexAnswer::Partial { rows: f(rows), limitation_ids }
            }
            Self::NotProven(reason) => IndexAnswer::NotProven(reason),
        }
    }
}

/// Why a model generation was rejected before materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewRejection {
    /// The model's root does not match the expected root.
    RootMismatch {
        /// The root the caller required.
        expected: String,
        /// The root the model carries.
        actual: String,
    },
    /// A shard was adopted under a different fact-schema version.
    SchemaIncompatible {
        /// The offending shard's relative path.
        path: String,
        /// The schema version the shard was adopted under.
        shard_schema: u32,
        /// The schema version this view materializes.
        view_schema: u32,
    },
    /// Part of the model was adopted through the ingestion API while other
    /// files carry no generation identity: not one accepted generation.
    MixedGenerationAdoption {
        /// Files lacking a shard state while others were adopted.
        unadopted_paths: Vec<String>,
    },
    /// An adopted shard sits below the caller's required per-shard floor.
    StaleGeneration {
        /// The offending shard's relative path.
        path: String,
        /// The minimum generation the caller required.
        floor: u64,
        /// The generation actually adopted.
        actual: u64,
    },
    /// The model serialization required for the accepted-generation basis
    /// could not be produced. The view refuses to accept a generation whose
    /// freshness anchor is unknown rather than fabricating a constant.
    SnapshotIdentityUnavailable {
        /// The underlying serialization failure, rendered for diagnostics.
        detail: String,
    },
}

impl fmt::Display for ViewRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMismatch { expected, actual } => {
                write!(f, "root mismatch: expected `{expected}`, model carries `{actual}`")
            }
            Self::SchemaIncompatible { path, shard_schema, view_schema } => write!(
                f,
                "shard `{path}` adopted under schema {shard_schema}, view requires {view_schema}"
            ),
            Self::MixedGenerationAdoption { unadopted_paths } => write!(
                f,
                "mixed generation adoption; unadopted files: {}",
                unadopted_paths.join(", ")
            ),
            Self::StaleGeneration { path, floor, actual } => {
                write!(f, "shard `{path}` generation {actual} is below required floor {floor}")
            }
            Self::SnapshotIdentityUnavailable { detail } => {
                write!(f, "model snapshot identity unavailable: {detail}")
            }
        }
    }
}

impl std::error::Error for ViewRejection {}

/// One source row: a logical source's current identity in this generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    /// Canonical file identity (path + content digest).
    pub file_id: FileId,
    /// Repo-relative, forward-slash path.
    pub relative_path: String,
    /// The file's role in the distribution.
    pub role: FileRole,
    /// Content digest at this generation (the logical revision identity).
    pub digest: Digest,
    /// Parse outcome backing the facts below this source.
    pub parse_status: ParseStatus,
    /// Shard state when the file was adopted through ingestion; `None` when
    /// the whole model came from the direct walker (no per-file generations).
    pub shard: Option<ShardIdentity>,
}

/// Generation identity for one adopted shard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardIdentity {
    /// Monotonic per-file generation at adoption.
    pub generation: u64,
    /// Fact-schema version at adoption (always [`SCHEMA_VERSION`] post-check).
    pub schema_version: u32,
    /// Adoption fingerprint.
    pub fingerprint: String,
}

/// One declaration contribution: a canonical entity declared in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationRow {
    /// A package declaration.
    Package {
        /// Canonical package identity.
        package_id: PackageId,
        /// Fully-qualified package name.
        name: String,
        /// Declared version, if known.
        version: Option<String>,
        /// Span of the declaration.
        range: SourceRange,
    },
    /// A symbol declaration (sub, method, variable, …).
    Symbol {
        /// Canonical symbol identity.
        symbol_id: SymbolId,
        /// Substrate symbol kind.
        kind: SymbolFactKind,
        /// Unqualified name.
        name: String,
        /// Package-qualified name.
        qualified_name: String,
        /// Enclosing package, if inside one.
        package: Option<String>,
        /// Reachability.
        visibility: Visibility,
        /// Span of the declaration.
        range: SourceRange,
    },
}

impl DeclarationRow {
    /// Inclusive start byte of the declaration span.
    #[must_use]
    pub fn start_byte(&self) -> u32 {
        match self {
            Self::Package { range, .. } | Self::Symbol { range, .. } => range.start_byte,
        }
    }

    /// Exclusive end byte of the declaration span.
    #[must_use]
    pub fn end_byte(&self) -> u32 {
        match self {
            Self::Package { range, .. } | Self::Symbol { range, .. } => range.end_byte,
        }
    }

    /// The canonical entity id behind this row, in prefixed string form.
    #[must_use]
    pub fn entity_key(&self) -> String {
        match self {
            Self::Package { package_id, .. } => package_id.as_str().to_owned(),
            Self::Symbol { symbol_id, .. } => symbol_id.as_str().to_owned(),
        }
    }

    /// The canonical entity id behind this row, borrowed: the allocation-
    /// free form used by ordering-sensitive internals.
    fn entity_key_str(&self) -> &str {
        match self {
            Self::Package { package_id, .. } => package_id.as_str(),
            Self::Symbol { symbol_id, .. } => symbol_id.as_str(),
        }
    }
}

/// One declaration anchor: `(byte interval) → canonical entity` within one
/// file revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorRow {
    /// Inclusive start byte.
    pub start_byte: u32,
    /// Exclusive end byte.
    pub end_byte: u32,
    /// The anchored canonical entity id (`sym:` / `pkg:` string form).
    pub entity_key: String,
}

/// Max-end augmented index over one file's start-sorted anchors: a segment
/// tree over `end_byte` whose node maxima let the overlap descent prune any
/// anchor range whose rows all end at or before the query start. Anchors
/// nest (a package span encloses its members), so end order is not monotone
/// over start order and node maxima — not end order — carry the soundness of
/// the descent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AnchorMaxEndIndex {
    /// Number of anchor rows covered.
    len: usize,
    /// 1-based recursive-layout segment tree; each node stores the maximum
    /// `end_byte` over its row range.
    tree: Vec<u32>,
}

impl AnchorMaxEndIndex {
    fn build(ends: &[u32]) -> Self {
        let len = ends.len();
        let mut tree = vec![0u32; 4 * len.max(1)];
        if len > 0 {
            build_max_ends(&mut tree, ends, 1, 0, len);
        }
        Self { len, tree }
    }

    /// Rows in `[0, right)` whose `end_byte` exceeds `start`, in row order,
    /// plus the descent probes spent. Every examined leaf is a reported hit;
    /// ranges whose maximum end is at or before `start` are pruned.
    fn collect_overlaps<'r>(
        &self,
        rows: &'r [AnchorRow],
        right: usize,
        start: u32,
    ) -> (Vec<&'r AnchorRow>, usize) {
        let mut hits = Vec::new();
        let mut probes = 0usize;
        if self.len == 0 || right == 0 {
            return (hits, probes);
        }
        // Explicit-stack pre-order descent with the right child pushed
        // first, so leaves are visited in row order.
        let mut stack = vec![(1usize, 0usize, self.len)];
        while let Some((node, nl, nr)) = stack.pop() {
            probes += 1;
            if nl >= right || self.tree[node] <= start {
                continue;
            }
            if nr - nl == 1 {
                hits.push(&rows[nl]);
                continue;
            }
            let mid = nl + (nr - nl) / 2;
            stack.push((2 * node + 1, mid, nr));
            stack.push((2 * node, nl, mid));
        }
        (hits, probes)
    }
}

fn build_max_ends(tree: &mut [u32], ends: &[u32], node: usize, nl: usize, nr: usize) {
    if nr - nl == 1 {
        tree[node] = ends[nl];
        return;
    }
    let mid = nl + (nr - nl) / 2;
    build_max_ends(tree, ends, 2 * node, nl, mid);
    build_max_ends(tree, ends, 2 * node + 1, mid, nr);
    tree[node] = tree[2 * node].max(tree[2 * node + 1]);
}

/// Lookup work evidence for one anchor query: proves hot lookups avoid a
/// full-set scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorLookupWork {
    /// Comparison probes spent by the right-cut binary search plus the
    /// max-end descent (logarithmic in the anchor set for few hits).
    pub probes: usize,
    /// Anchor rows the overlap filter actually examined; each examined row
    /// is a reported hit, so a prefix or full scan cannot hide behind this
    /// counter.
    pub scanned_rows: usize,
    /// Total anchor rows available for the queried file.
    pub candidate_rows: usize,
}

/// Work receipt for one build: rows and approximate bytes by index family.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewWorkReceipt {
    /// Per-family work, keyed by family name, sorted by key.
    pub families: BTreeMap<String, FamilyWork>,
    /// Total indexed rows across all families.
    pub total_rows: usize,
}

/// Work for one index family.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FamilyWork {
    /// Rows materialized in the family.
    pub rows: usize,
    /// Deterministic approximate byte cost (string lengths + fixed widths).
    pub approx_bytes: usize,
}

/// Explicit acceptance expectations for [`SemanticQueryView::build_checked`].
#[derive(Debug, Clone, Copy)]
pub struct CheckedBuildInput<'m> {
    /// The one accepted model generation.
    pub model: &'m ProjectModel,
    /// Required root, when the caller pins it.
    pub expected_root: Option<&'m str>,
    /// Minimum adopted generation every shard must individually meet (a
    /// per-shard floor; the model carries no model-level generation).
    pub min_shard_generation: Option<u64>,
}

/// The immutable semantic query view over one accepted generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticQueryView {
    /// The project/root identity the model was built from.
    pub root: String,
    /// Deterministic identity of the source model serialization.
    pub model_snapshot_identity: String,
    /// Fact classes the generating request admitted.
    pub requested_fact_classes: FactClasses,
    /// Logical source/path → current source identity, keyed by relative path.
    sources: BTreeMap<String, SourceEntry>,
    /// Logical source → ordered declaration contributions, keyed by file id.
    declarations_by_file: BTreeMap<FileId, Vec<DeclarationRow>>,
    /// Canonical symbol id → its current declaration contribution.
    symbols: BTreeMap<SymbolId, DeclarationRow>,
    /// Canonical package id → its current declaration contribution.
    packages: BTreeMap<PackageId, DeclarationRow>,
    /// Declaration anchors sorted by start byte, keyed by file id.
    anchors_by_file: BTreeMap<FileId, Vec<AnchorRow>>,
    /// Max-end augmented index per file (same keys as `anchors_by_file`),
    /// enabling nesting-safe overlap descent without full-set scans.
    anchor_max_ends: BTreeMap<FileId, AnchorMaxEndIndex>,
    /// Typed completeness per family, keyed by family name.
    completeness: BTreeMap<&'static str, IndexCompleteness>,
    /// Discovered-but-unread relative paths carried over from the model so
    /// path-scoped lookups can refuse a fabricated legitimate empty.
    unread_discovered: BTreeSet<String>,
    /// Structural limitation-to-path association (limitation id -> relative
    /// paths it bounds), joined from model-level limitations that declare
    /// paths and from adopted shard states. Ids absent from this map predate
    /// the structural field and fall back to the `<kind>:<path>` id
    /// convention at query time.
    limitation_paths: BTreeMap<String, BTreeSet<String>>,
    /// Build work receipt.
    work: ViewWorkReceipt,
    /// Deterministic view fingerprint (`fnv64:` form).
    fingerprint: String,
}

impl SemanticQueryView {
    /// Build the view from exactly one accepted ProjectModel generation.
    ///
    /// # Errors
    /// Rejects wrong-root, schema-incompatible, mixed-adoption, stale input,
    /// and an unavailable model snapshot identity (the freshness anchor)
    /// before any materialization.
    pub fn build(model: &ProjectModel) -> Result<Self, ViewRejection> {
        Self::build_checked(CheckedBuildInput {
            model,
            expected_root: None,
            min_shard_generation: None,
        })
    }

    /// Build with explicit acceptance expectations.
    ///
    /// # Errors
    /// Same rejections as [`Self::build`], plus the caller's root and
    /// generation-floor expectations.
    pub fn build_checked(
        CheckedBuildInput { model, expected_root, min_shard_generation }: CheckedBuildInput<'_>,
    ) -> Result<Self, ViewRejection> {
        validate_input(model, expected_root, min_shard_generation)?;
        let model_snapshot_identity = accepted_snapshot_identity(model.snapshot_identity())?;

        let sources = index_sources(model);
        let (declarations_by_file, symbols, packages) = index_declarations(model);
        let anchors_by_file = index_anchors(&declarations_by_file);
        let anchor_max_ends = anchor_max_end_index(&anchors_by_file);
        let completeness = classify_families(model);
        let unread_discovered = model.unread_discovered.clone();
        let limitation_paths = structural_limitation_paths(model);
        let work = measure_work(&sources, &declarations_by_file, &symbols, &packages);
        let fingerprint = fingerprint_view(
            model,
            &sources,
            &declarations_by_file,
            &completeness,
            &unread_discovered,
            &limitation_paths,
        );

        Ok(Self {
            root: model.root.clone(),
            model_snapshot_identity,
            requested_fact_classes: model.requested,
            sources,
            declarations_by_file,
            symbols,
            packages,
            anchors_by_file,
            anchor_max_ends,
            completeness,
            unread_discovered,
            limitation_paths,
            work,
            fingerprint,
        })
    }

    /// The deterministic view fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The build work receipt.
    #[must_use]
    pub const fn work(&self) -> &ViewWorkReceipt {
        &self.work
    }

    /// Typed completeness of one index family (`"sources"`,
    /// `"declarations"`, `"declaration_anchors"`, `"occurrences"`,
    /// `"generated"`).
    #[must_use]
    pub fn family_completeness(&self, family: &str) -> Option<&IndexCompleteness> {
        self.completeness.get(family)
    }

    /// Current source identity for a logical source path.
    #[must_use]
    pub fn source_by_path(&self, relative_path: &str) -> IndexAnswer<'_, Option<&SourceEntry>> {
        let Some(state) = self.family_completeness("sources") else {
            return IndexAnswer::NotProven(NotProvenReason::FactClassNotAdmitted);
        };
        let found = self.sources.get(relative_path);
        match state {
            IndexCompleteness::Complete => IndexAnswer::Complete(found),
            IndexCompleteness::Partial { limitation_ids } => match found {
                Some(entry) => {
                    let ids = self.limitations_bounding_path(
                        limitation_ids.iter().map(String::as_str),
                        relative_path,
                    );
                    if ids.is_empty() {
                        IndexAnswer::Complete(Some(entry))
                    } else {
                        IndexAnswer::Partial { rows: Some(entry), limitation_ids: ids }
                    }
                }
                None => {
                    if self.unread_discovered.contains(relative_path) {
                        // The walk discovered this path and the read failed:
                        // its absence is bounded, never a legitimate empty.
                        let ids = self.limitations_bounding_path(
                            limitation_ids.iter().map(String::as_str),
                            relative_path,
                        );
                        IndexAnswer::Partial { rows: None, limitation_ids: ids }
                    } else {
                        IndexAnswer::Complete(None)
                    }
                }
            },
            IndexCompleteness::NotProven(reason) => IndexAnswer::NotProven(*reason),
        }
    }

    /// All sources carrying one path role, in deterministic path order.
    #[must_use]
    pub fn sources_with_role(&self, role: FileRole) -> IndexAnswer<'_, Vec<&SourceEntry>> {
        let Some(state) = self.family_completeness("sources") else {
            return IndexAnswer::NotProven(NotProvenReason::FactClassNotAdmitted);
        };
        let rows: Vec<&SourceEntry> =
            self.sources.values().filter(|source| source.role == role).collect();
        match state {
            IndexCompleteness::Complete => IndexAnswer::Complete(rows),
            IndexCompleteness::Partial { limitation_ids } => {
                let mut ids: BTreeSet<&str> = BTreeSet::new();
                for entry in &rows {
                    ids.extend(self.limitations_bounding_path(
                        limitation_ids.iter().map(String::as_str),
                        &entry.relative_path,
                    ));
                }
                for path in &self.unread_discovered {
                    if FileRole::from_path(path) == role {
                        ids.extend(self.limitations_bounding_path(
                            limitation_ids.iter().map(String::as_str),
                            path,
                        ));
                    }
                }
                if ids.is_empty() {
                    IndexAnswer::Complete(rows)
                } else {
                    IndexAnswer::Partial { rows, limitation_ids: ids.into_iter().collect() }
                }
            }
            IndexCompleteness::NotProven(reason) => IndexAnswer::NotProven(*reason),
        }
    }

    /// Ordered declaration contributions of one logical source.
    ///
    /// Denominator semantics: the rows of one file within this generation.
    /// A `FileId` absent from this generation therefore answers
    /// [`IndexAnswer::Complete`] with zero rows — a legitimate exact empty,
    /// distinct from a not-proven family.
    #[must_use]
    pub fn declarations_in_file(&self, file_id: &FileId) -> IndexAnswer<'_, &[DeclarationRow]> {
        match self.declaration_state_for(file_id) {
            Ok(()) => IndexAnswer::Complete(
                self.declarations_by_file.get(file_id).map(Vec::as_slice).unwrap_or(&[]),
            ),
            Err(answer) => answer.map(|()| &[] as &[DeclarationRow]),
        }
    }

    /// The current declaration contribution of one canonical symbol.
    #[must_use]
    pub fn symbol_declaration(
        &self,
        symbol_id: &SymbolId,
    ) -> IndexAnswer<'_, Option<&DeclarationRow>> {
        match self.declaration_family_state() {
            Ok(()) => IndexAnswer::Complete(self.symbols.get(symbol_id)),
            Err(answer) => answer.map(|()| self.symbols.get(symbol_id)),
        }
    }

    /// The current declaration contribution of one canonical package.
    #[must_use]
    pub fn package_declaration(
        &self,
        package_id: &PackageId,
    ) -> IndexAnswer<'_, Option<&DeclarationRow>> {
        match self.declaration_family_state() {
            Ok(()) => IndexAnswer::Complete(self.packages.get(package_id)),
            Err(answer) => answer.map(|()| self.packages.get(package_id)),
        }
    }

    /// Declaration anchors overlapping `[start, end)` in one file revision.
    ///
    /// Anchors nest (a package span encloses its members), so end_byte is
    /// not monotone over start order and an end-driven cut alone is
    /// unsound. The lookup is therefore driven by the per-file max-end
    /// augmented index: the descent prunes every anchor range whose rows
    /// all end at or before `start`, and a start-sorted right cut bounds
    /// the reported range to anchors starting before `end`. Overlapping and
    /// nested anchors are all reported.
    ///
    /// As with [`Self::declarations_in_file`], a `FileId` absent from this
    /// generation is a legitimate exact empty: complete denominator, zero
    /// rows.
    ///
    /// The lookup spends `probes` comparisons against `candidate_rows`
    /// available rows and examines exactly the rows it reports
    /// (`scanned_rows`); a hot lookup never scans the full anchor set.
    #[must_use]
    pub fn anchors_overlapping(
        &self,
        file_id: &FileId,
        start: u32,
        end: u32,
    ) -> IndexAnswer<'_, (Vec<&AnchorRow>, AnchorLookupWork)> {
        let candidates = match self.declaration_state_for(file_id) {
            Ok(()) => self.anchors_by_file.get(file_id),
            Err(answer) => {
                return answer.map(|()| {
                    (Vec::new(), AnchorLookupWork { probes: 0, scanned_rows: 0, candidate_rows: 0 })
                });
            }
        };
        let Some(rows) = candidates else {
            return IndexAnswer::Complete((
                Vec::new(),
                AnchorLookupWork { probes: 0, scanned_rows: 0, candidate_rows: 0 },
            ));
        };

        // Right cut over the start-sorted vector (logarithmic descent):
        // every anchor starting at or after `end` is excluded by
        // construction. The max-end index then reports exactly the
        // remaining rows whose end passes `start` — including nested
        // enclosures — without scanning the excluded or pruned ranges.
        let mut probes = 0usize;
        let mut low = 0usize;
        let mut high = rows.len();
        while low < high {
            probes += 1;
            let mid = low + (high - low) / 2;
            if rows[mid].start_byte < end {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let (hits, descent_probes) = self
            .anchor_max_ends
            .get(file_id)
            .map_or_else(|| (Vec::new(), 0), |index| index.collect_overlaps(rows, low, start));
        probes += descent_probes;
        let scanned_rows = hits.len();
        IndexAnswer::Complete((
            hits,
            AnchorLookupWork { probes, scanned_rows, candidate_rows: rows.len() },
        ))
    }

    /// Typed occurrence contributions for a canonical entity.
    ///
    /// Occurrence facts do not exist in any accepted [`ProjectModel`]
    /// generation yet; this is always [`IndexAnswer::NotProven`] rather than a
    /// fabricated zero.
    #[must_use]
    pub fn occurrences_of(&self, _entity_key: &str) -> IndexAnswer<'static, ()> {
        IndexAnswer::NotProven(NotProvenReason::InstrumentationAbsent {
            family: "occurrence facts",
        })
    }

    /// Source-anchored generated contributions for a generator entity.
    ///
    /// Generated-member facts do not exist in any accepted [`ProjectModel`]
    /// generation yet; generated-no-source records can never receive
    /// fabricated anchors, so this stays [`IndexAnswer::NotProven`].
    #[must_use]
    pub fn generated_contributions_of(
        &self,
        _generator_entity_key: &str,
    ) -> IndexAnswer<'static, ()> {
        IndexAnswer::NotProven(NotProvenReason::InstrumentationAbsent {
            family: "generated-member facts",
        })
    }

    /// `Ok(())` when the declarations family admits complete-denominator
    /// answers for this file; otherwise the limiting answer.
    fn declaration_state_for(&self, file_id: &FileId) -> Result<(), IndexAnswer<'_, ()>> {
        match self.declaration_family_state() {
            Ok(()) => Ok(()),
            Err(IndexAnswer::Partial { limitation_ids, .. }) => match self.file_path_of(file_id) {
                Some(path) => {
                    let ids = self.limitations_bounding_path(limitation_ids.iter().copied(), path);
                    if ids.is_empty() {
                        Ok(())
                    } else {
                        Err(IndexAnswer::Partial { rows: (), limitation_ids: ids })
                    }
                }
                None => Ok(()),
            },
            Err(other) => Err(other),
        }
    }

    /// `Ok(())` when the declarations family proves its denominator;
    /// otherwise the typed limiting answer.
    fn declaration_family_state(&self) -> Result<(), IndexAnswer<'_, ()>> {
        match self.family_completeness("declarations") {
            Some(IndexCompleteness::Complete) => Ok(()),
            Some(IndexCompleteness::Partial { limitation_ids }) => {
                let ids: Vec<&str> = limitation_ids.iter().map(String::as_str).collect();
                Err(IndexAnswer::Partial { rows: (), limitation_ids: ids })
            }
            Some(IndexCompleteness::NotProven(reason)) => Err(IndexAnswer::NotProven(*reason)),
            None => Err(IndexAnswer::NotProven(NotProvenReason::FactClassNotAdmitted)),
        }
    }

    fn file_path_of(&self, file_id: &FileId) -> Option<&str> {
        self.sources
            .values()
            .find(|entry| &entry.file_id == file_id)
            .map(|e| e.relative_path.as_str())
    }

    /// The family limitation ids that bound one path.
    ///
    /// Structural association is authoritative: an id carried in
    /// [`Self::limitation_paths`] bounds the path only when its recorded
    /// paths contain it, so a valid non-suffixed id is never dropped. Ids
    /// absent from the structural map predate it and keep the textual
    /// `<kind>:<path>` convention (`:<path>` suffix), so legacy limitations
    /// bound exactly as before.
    fn limitations_bounding_path<'v>(
        &'v self,
        family_ids: impl Iterator<Item = &'v str>,
        path: &str,
    ) -> Vec<&'v str> {
        let suffix = format!(":{path}");
        family_ids
            .filter(|id| match self.limitation_paths.get(*id) {
                Some(paths) => paths.iter().any(|bounded| bounded == path),
                None => id.ends_with(&suffix),
            })
            .collect()
    }
}

/// The accepted-generation basis: the model serialization identity, or a
/// typed rejection when it cannot be produced. The view never accepts a
/// generation whose freshness anchor is unknown — a constant sentinel would
/// make unrelated failing generations indistinguishable.
fn accepted_snapshot_identity(
    identity: Result<String, ShardError>,
) -> Result<String, ViewRejection> {
    identity
        .map_err(|error| ViewRejection::SnapshotIdentityUnavailable { detail: error.to_string() })
}

fn validate_input(
    model: &ProjectModel,
    expected_root: Option<&str>,
    min_shard_generation: Option<u64>,
) -> Result<(), ViewRejection> {
    if let Some(expected) = expected_root
        && model.root != expected
    {
        return Err(ViewRejection::RootMismatch {
            expected: expected.to_owned(),
            actual: model.root.clone(),
        });
    }

    let mut unadopted: Vec<String> = Vec::new();
    for file in &model.files {
        match model.shard_states.get(&file.relative_path) {
            Some(state) => {
                if state.schema_version != SCHEMA_VERSION {
                    return Err(ViewRejection::SchemaIncompatible {
                        path: file.relative_path.clone(),
                        shard_schema: state.schema_version,
                        view_schema: SCHEMA_VERSION,
                    });
                }
                if let Some(floor) = min_shard_generation
                    && state.generation < floor
                {
                    return Err(ViewRejection::StaleGeneration {
                        path: file.relative_path.clone(),
                        floor,
                        actual: state.generation,
                    });
                }
            }
            None => unadopted.push(file.relative_path.clone()),
        }
    }

    if !model.shard_states.is_empty() && !unadopted.is_empty() {
        return Err(ViewRejection::MixedGenerationAdoption { unadopted_paths: unadopted });
    }
    Ok(())
}

fn index_sources(model: &ProjectModel) -> BTreeMap<String, SourceEntry> {
    let mut sources = BTreeMap::new();
    for file in &model.files {
        let shard = model.shard_states.get(&file.relative_path).map(|state| ShardIdentity {
            generation: state.generation,
            schema_version: state.schema_version,
            fingerprint: state.fingerprint.clone(),
        });
        sources.insert(
            file.relative_path.clone(),
            SourceEntry {
                file_id: file.file_id.clone(),
                relative_path: file.relative_path.clone(),
                role: file.role,
                digest: file.digest.clone(),
                parse_status: file.parse_status,
                shard,
            },
        );
    }
    sources
}

fn index_declarations(
    model: &ProjectModel,
) -> (
    BTreeMap<FileId, Vec<DeclarationRow>>,
    BTreeMap<SymbolId, DeclarationRow>,
    BTreeMap<PackageId, DeclarationRow>,
) {
    let mut by_file: BTreeMap<FileId, Vec<DeclarationRow>> = BTreeMap::new();

    for record in &model.packages {
        by_file.entry(record.file_id.clone()).or_default().push(DeclarationRow::Package {
            package_id: record.package_id.clone(),
            name: record.name.clone(),
            version: record.version.clone(),
            range: record.declaration_range,
        });
    }
    for record in &model.symbols {
        by_file.entry(record.file_id.clone()).or_default().push(DeclarationRow::Symbol {
            symbol_id: record.symbol_id.clone(),
            kind: record.kind,
            name: record.name.clone(),
            qualified_name: record.qualified_name.clone(),
            package: record.package.clone(),
            visibility: record.visibility,
            range: record.declaration_range,
        });
    }

    let mut symbols = BTreeMap::new();
    let mut packages = BTreeMap::new();
    for rows in by_file.values_mut() {
        // Borrowed keys: the comparator allocates nothing per comparison.
        rows.sort_by(|a, b| {
            (a.start_byte(), a.entity_key_str()).cmp(&(b.start_byte(), b.entity_key_str()))
        });
        for row in rows.iter() {
            match row {
                DeclarationRow::Package { package_id, .. } => {
                    packages.insert(package_id.clone(), row.clone());
                }
                DeclarationRow::Symbol { symbol_id, .. } => {
                    symbols.insert(symbol_id.clone(), row.clone());
                }
            }
        }
    }
    (by_file, symbols, packages)
}

fn index_anchors(
    declarations_by_file: &BTreeMap<FileId, Vec<DeclarationRow>>,
) -> BTreeMap<FileId, Vec<AnchorRow>> {
    let mut anchors = BTreeMap::new();
    for (file_id, rows) in declarations_by_file {
        let mut file_anchors: Vec<AnchorRow> = rows
            .iter()
            .map(|row| AnchorRow {
                start_byte: row.start_byte(),
                end_byte: row.end_byte(),
                entity_key: row.entity_key(),
            })
            .collect();
        file_anchors.sort_by(|a, b| {
            (a.start_byte, a.end_byte, &a.entity_key).cmp(&(
                b.start_byte,
                b.end_byte,
                &b.entity_key,
            ))
        });
        anchors.insert(file_id.clone(), file_anchors);
    }
    anchors
}

fn anchor_max_end_index(
    anchors_by_file: &BTreeMap<FileId, Vec<AnchorRow>>,
) -> BTreeMap<FileId, AnchorMaxEndIndex> {
    anchors_by_file
        .iter()
        .map(|(file_id, rows)| {
            let ends: Vec<u32> = rows.iter().map(|row| row.end_byte).collect();
            (file_id.clone(), AnchorMaxEndIndex::build(&ends))
        })
        .collect()
}

fn classify_families(model: &ProjectModel) -> BTreeMap<&'static str, IndexCompleteness> {
    let mut families = BTreeMap::new();
    families.insert("sources", sources_completeness(model));
    families.insert("declarations", declarations_completeness(model));
    families.insert(
        "declaration_anchors",
        families
            .get("declarations")
            .cloned()
            .unwrap_or(IndexCompleteness::NotProven(NotProvenReason::FactClassNotAdmitted)),
    );
    families.insert(
        "occurrences",
        IndexCompleteness::NotProven(NotProvenReason::InstrumentationAbsent {
            family: "occurrence facts",
        }),
    );
    families.insert(
        "generated",
        IndexCompleteness::NotProven(NotProvenReason::InstrumentationAbsent {
            family: "generated-member facts",
        }),
    );
    families
}

fn sources_completeness(model: &ProjectModel) -> IndexCompleteness {
    if !model.requested.contains(FactClasses::FILES) {
        return IndexCompleteness::NotProven(NotProvenReason::FactClassNotAdmitted);
    }
    partial_if_limited(model)
}

fn declarations_completeness(model: &ProjectModel) -> IndexCompleteness {
    if !model.requested.contains(FactClasses::SYMBOLS) {
        return IndexCompleteness::NotProven(NotProvenReason::FactClassNotAdmitted);
    }
    // A shard with explicit population evidence that never populated the
    // declarations class cannot back a proven-empty denominator: extraction
    // that never ran is not a proven zero. Legacy states without population
    // evidence retain the pre-evidence behavior.
    if model
        .shard_states
        .values()
        .any(|state| matches!(state.populated, Some(populated) if !populated.contains(FactClasses::SYMBOLS)))
    {
        return IndexCompleteness::NotProven(NotProvenReason::ShardClassNotPopulated {
            family: "declarations",
        });
    }
    // Every admitted file bounds the declaration denominator: a parse-failed
    // or recovered file may hide declarations that never became rows.
    partial_if_limited(model)
}

/// The family denominator: every admitted file plus every
/// discovered-but-unread path. An unread source is not an empty slot — it
/// bounds the family, because the walk saw it and the read failed.
fn partial_if_limited(model: &ProjectModel) -> IndexCompleteness {
    let paths: BTreeSet<&str> = model
        .files
        .iter()
        .map(|file| file.relative_path.as_str())
        .chain(model.unread_discovered.iter().map(String::as_str))
        .collect();
    let mut limitation_ids: BTreeSet<String> = BTreeSet::new();
    for limitation in &model.limitations {
        if !limitation.paths.is_empty() {
            // Structural association is authoritative: the limitation names
            // the paths it bounds, so no id-text convention is consulted.
            if limitation.paths.iter().any(|path| paths.contains(path.as_str())) {
                limitation_ids.insert(limitation.id.clone());
            }
            continue;
        }
        // Legacy ids keep the textual convention, preserved exactly: an id
        // bounds the family when any colon-remainder of the id names a
        // contributing path (the `<kind>:<relative path>` convention; the
        // remainder is checked at every colon so paths containing colons
        // still match, as with `ends_with`).
        let mut rest = limitation.id.as_str();
        while let Some(colon) = rest.find(':') {
            if paths.contains(&rest[colon + 1..]) {
                limitation_ids.insert(limitation.id.clone());
                break;
            }
            rest = &rest[colon + 1..];
        }
    }
    for path in &paths {
        if let Some(state) = model.shard_states.get(*path) {
            limitation_ids.extend(state.limitation_ids.iter().cloned());
        }
    }
    if limitation_ids.is_empty() {
        IndexCompleteness::Complete
    } else {
        IndexCompleteness::Partial { limitation_ids: limitation_ids.into_iter().collect() }
    }
}

/// Structural limitation-to-path association for one model: model-level
/// limitations that declare paths, joined with the per-shard associations
/// retained at adoption. Limitations that declare no paths and predate the
/// structural field are absent here and keep the textual fallback.
fn structural_limitation_paths(model: &ProjectModel) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for limitation in &model.limitations {
        if !limitation.paths.is_empty() {
            map.entry(limitation.id.clone()).or_default().extend(limitation.paths.iter().cloned());
        }
    }
    for state in model.shard_states.values() {
        for (id, paths) in &state.limitation_paths {
            map.entry(id.clone()).or_default().extend(paths.iter().cloned());
        }
    }
    map
}

fn approx_declaration_bytes(row: &DeclarationRow) -> usize {
    16 + match row {
        DeclarationRow::Package { name, version, .. } => {
            name.len() + version.as_ref().map_or(0, String::len)
        }
        DeclarationRow::Symbol { name, qualified_name, package, .. } => {
            name.len() + qualified_name.len() + package.as_ref().map_or(0, String::len)
        }
    }
}

fn measure_work(
    sources: &BTreeMap<String, SourceEntry>,
    declarations_by_file: &BTreeMap<FileId, Vec<DeclarationRow>>,
    symbols: &BTreeMap<SymbolId, DeclarationRow>,
    packages: &BTreeMap<PackageId, DeclarationRow>,
) -> ViewWorkReceipt {
    let mut work = ViewWorkReceipt::default();

    let source_rows = sources.len();
    let source_bytes: usize = sources
        .values()
        .map(|entry| {
            entry.relative_path.len()
                + entry.digest.as_str().len()
                + entry.file_id.as_str().len()
                + entry.shard.as_ref().map_or(0, |shard| shard.fingerprint.len() + 16)
                + 8
        })
        .sum();
    work.families
        .insert("sources".to_owned(), FamilyWork { rows: source_rows, approx_bytes: source_bytes });

    let file_rows: usize = declarations_by_file.values().map(Vec::len).sum();
    let file_bytes: usize =
        declarations_by_file.values().flatten().map(approx_declaration_bytes).sum();
    let entity_bytes: usize =
        symbols.values().chain(packages.values()).map(approx_declaration_bytes).sum();
    work.families.insert(
        "declarations".to_owned(),
        FamilyWork { rows: file_rows, approx_bytes: file_bytes + entity_bytes },
    );

    let anchor_rows: usize = anchors_by_file_rows(declarations_by_file);
    work.families.insert(
        "declaration_anchors".to_owned(),
        FamilyWork { rows: anchor_rows, approx_bytes: anchor_rows * 40 },
    );

    work.total_rows = work.families.values().map(|family| family.rows).sum();
    work
}

fn anchors_by_file_rows(declarations_by_file: &BTreeMap<FileId, Vec<DeclarationRow>>) -> usize {
    declarations_by_file.values().map(Vec::len).sum()
}

fn push_field(buf: &mut Vec<u8>, field: &str) {
    buf.extend_from_slice(field.as_bytes());
    buf.push(0);
}

fn push_u32(buf: &mut Vec<u8>, value: u32) {
    push_field(buf, &value.to_string());
}

fn push_completeness(buf: &mut Vec<u8>, state: &IndexCompleteness) {
    match state {
        IndexCompleteness::Complete => push_field(buf, "complete"),
        IndexCompleteness::Partial { limitation_ids } => {
            push_field(buf, "partial");
            for id in limitation_ids {
                push_field(buf, id);
            }
        }
        IndexCompleteness::NotProven(reason) => {
            push_field(buf, "not-proven");
            push_field(buf, &reason.to_string());
        }
    }
}

fn fingerprint_view(
    model: &ProjectModel,
    sources: &BTreeMap<String, SourceEntry>,
    declarations_by_file: &BTreeMap<FileId, Vec<DeclarationRow>>,
    completeness: &BTreeMap<&'static str, IndexCompleteness>,
    unread_discovered: &BTreeSet<String>,
    limitation_paths: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    let mut buf = Vec::new();
    push_field(&mut buf, "semantic-query-view");
    // v3: unread paths + structural limitation-path associations.
    push_field(&mut buf, "v3");
    push_field(&mut buf, &model.root);
    push_u32(&mut buf, model.requested.bits());

    for (path, entry) in sources {
        push_field(&mut buf, path);
        push_field(&mut buf, entry.file_id.as_str());
        push_field(&mut buf, entry.digest.as_str());
        if let Some(shard) = &entry.shard {
            push_field(&mut buf, &shard.generation.to_string());
            push_u32(&mut buf, shard.schema_version);
            push_field(&mut buf, &shard.fingerprint);
        }
    }

    for (file_id, rows) in declarations_by_file {
        push_field(&mut buf, file_id.as_str());
        for row in rows {
            push_field(&mut buf, &row.entity_key());
            push_u32(&mut buf, row.start_byte());
            push_u32(&mut buf, row.end_byte());
            match row {
                DeclarationRow::Package { name, version, .. } => {
                    push_field(&mut buf, name);
                    match version {
                        Some(version) => {
                            push_field(&mut buf, "some");
                            push_field(&mut buf, version);
                        }
                        None => push_field(&mut buf, "none"),
                    }
                }
                DeclarationRow::Symbol {
                    kind, name, qualified_name, package, visibility, ..
                } => {
                    push_field(&mut buf, name);
                    push_field(&mut buf, qualified_name);
                    match package {
                        Some(package) => {
                            push_field(&mut buf, "some");
                            push_field(&mut buf, package);
                        }
                        None => push_field(&mut buf, "none"),
                    }
                    push_field(&mut buf, &format!("{kind:?}"));
                    push_field(&mut buf, &format!("{visibility:?}"));
                }
            }
        }
    }

    for (family, state) in completeness {
        push_field(&mut buf, family);
        push_completeness(&mut buf, state);
    }

    push_field(&mut buf, "unread");
    push_u32(&mut buf, unread_discovered.len() as u32);
    for path in unread_discovered {
        push_field(&mut buf, path);
    }
    push_field(&mut buf, "limitation-paths");
    push_u32(&mut buf, limitation_paths.len() as u32);
    for (id, paths) in limitation_paths {
        push_field(&mut buf, id);
        push_u32(&mut buf, paths.len() as u32);
        for path in paths {
            push_field(&mut buf, path);
        }
    }

    format!("fnv64:{:016x}", fnv1a(&buf))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use crate::error::ModelLimitation;
    use crate::file::FileRecord;
    use crate::id::Digest;
    use crate::package::PackageRecord;
    use crate::symbol::SymbolRecord;
    use crate::{ProjectFactShard, ProjectModel};
    use serde_json::Value;

    fn range(start: u32, end: u32) -> SourceRange {
        SourceRange {
            start_byte: start,
            end_byte: end,
            start_line: 0,
            start_column_utf8: start,
            end_line: 0,
            end_column_utf8: end,
        }
    }

    fn file(path: &str, content: &str) -> FileRecord {
        FileRecord {
            file_id: FileId::new(path, &Digest::of(content)),
            relative_path: path.to_string(),
            role: FileRole::from_path(path),
            digest: Digest::of(content),
            parse_status: ParseStatus::Clean,
        }
    }

    fn symbol(
        path: &str,
        content: &str,
        package: Option<&str>,
        name: &str,
        qualified: &str,
        start: u32,
        end: u32,
    ) -> SymbolRecord {
        let file_id = FileId::new(path, &Digest::of(content));
        SymbolRecord {
            symbol_id: SymbolId::new(&file_id, "sub", qualified, start, end),
            file_id,
            kind: SymbolFactKind::Sub,
            package: package.map(str::to_string),
            name: name.to_string(),
            qualified_name: qualified.to_string(),
            declaration_range: range(start, end),
            visibility: Visibility::Public,
            confidence: crate::provenance::Confidence::High,
        }
    }

    fn package_record(
        path: &str,
        content: &str,
        name: &str,
        start: u32,
        end: u32,
    ) -> PackageRecord {
        let file_id = FileId::new(path, &Digest::of(content));
        PackageRecord {
            package_id: PackageId::new(&file_id, name, start),
            name: name.to_string(),
            file_id,
            declaration_range: range(start, end),
            version: None,
            parents: Vec::new(),
            roles: Vec::new(),
            confidence: crate::provenance::Confidence::High,
        }
    }

    fn model_with(
        files: Vec<FileRecord>,
        packages: Vec<PackageRecord>,
        symbols: Vec<SymbolRecord>,
    ) -> ProjectModel {
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files = files;
        model.packages = packages;
        model.symbols = symbols;
        model
    }

    #[test]
    fn declarations_are_ordered_and_canonically_addressable() {
        let a = file("lib/A.pm", "content-a");
        let b = file("lib/B.pm", "content-b");
        let pkg = package_record("lib/A.pm", "content-a", "A", 0, 9);
        let s1 = symbol("lib/A.pm", "content-a", Some("A"), "run", "A::run", 10, 20);
        let s2 = symbol("lib/B.pm", "content-b", Some("B"), "run", "B::run", 5, 15);
        let view = SemanticQueryView::build(&model_with(
            vec![a.clone(), b.clone()],
            vec![pkg.clone()],
            vec![s1.clone(), s2.clone()],
        ))
        .unwrap();

        let answer = view.declarations_in_file(&a.file_id);
        assert_eq!(answer.completeness(), IndexCompleteness::Complete);
        let rows = answer.rows().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].start_byte() <= rows[1].start_byte());

        let resolved = view.symbol_declaration(&s1.symbol_id).rows().and_then(|r| r).unwrap();
        assert_eq!(resolved.entity_key(), s1.symbol_id.as_str());
        let package_row = view.package_declaration(&pkg.package_id).rows().and_then(|r| r).unwrap();
        assert_eq!(package_row.entity_key(), pkg.package_id.as_str());
    }

    #[test]
    fn same_spelling_in_different_scopes_stays_distinct() {
        let content = "shared";
        let in_a = symbol("lib/A.pm", content, Some("A"), "run", "A::run", 0, 10);
        let in_b = symbol("lib/B.pm", content, Some("B"), "run", "B::run", 0, 10);
        let view = SemanticQueryView::build(&model_with(
            vec![file("lib/A.pm", content), file("lib/B.pm", content)],
            vec![],
            vec![in_a.clone(), in_b.clone()],
        ))
        .unwrap();
        assert_ne!(in_a.symbol_id, in_b.symbol_id);
        assert_eq!(view.symbols.len(), 2);
        let first = view.symbol_declaration(&in_a.symbol_id).rows().and_then(|r| r).unwrap();
        assert!(
            matches!(first, DeclarationRow::Symbol { qualified_name, .. } if qualified_name == "A::run"),
            "expected symbol row A::run, got {first:?}"
        );
    }

    #[test]
    fn identical_content_at_two_paths_stays_distinct() {
        let content = "same-bytes";
        let left = file("roots-left/lib/A.pm", content);
        let right = file("roots-right/lib/A.pm", content);
        assert_ne!(left.file_id, right.file_id);
        let sym_left = symbol("roots-left/lib/A.pm", content, None, "x", "X::x", 0, 8);
        let sym_right = symbol("roots-right/lib/A.pm", content, None, "x", "X::x", 0, 8);
        assert_ne!(sym_left.symbol_id, sym_right.symbol_id);
        let view = SemanticQueryView::build(&model_with(
            vec![left, right],
            vec![],
            vec![sym_left, sym_right],
        ))
        .unwrap();
        assert_eq!(view.sources.len(), 2);
        assert_eq!(view.work.families["declarations"].rows, 2);
    }

    #[test]
    fn duplicate_and_reopened_declarations_are_separate_rows() {
        let content = "dup";
        let first = symbol("lib/D.pm", content, Some("D"), "helper", "D::helper", 0, 40);
        let reopened = symbol("lib/D.pm", content, Some("D"), "helper", "D::helper", 50, 90);
        let view = SemanticQueryView::build(&model_with(
            vec![file("lib/D.pm", content)],
            vec![],
            vec![first.clone(), reopened.clone()],
        ))
        .unwrap();
        assert_ne!(first.symbol_id, reopened.symbol_id);
        let answer = view.declarations_in_file(&FileId::new("lib/D.pm", &Digest::of(content)));
        assert_eq!(answer.completeness(), IndexCompleteness::Complete);
        let rows = answer.rows().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].start_byte(), 0);
        assert_eq!(rows[1].start_byte(), 50);
    }

    #[test]
    fn occurrence_and_generated_families_are_not_proven_never_zero() {
        let view = SemanticQueryView::build(&model_with(
            vec![file("lib/A.pm", "a")],
            vec![],
            vec![symbol("lib/A.pm", "a", None, "f", "F::f", 0, 8)],
        ))
        .unwrap();

        for family in ["occurrences", "generated"] {
            assert!(
                matches!(
                    view.family_completeness(family),
                    Some(IndexCompleteness::NotProven(
                        NotProvenReason::InstrumentationAbsent { .. }
                    ))
                ),
                "expected not-proven family {family}, got {:?}",
                view.family_completeness(family)
            );
        }
        assert!(matches!(
            view.occurrences_of("sym:anything"),
            IndexAnswer::NotProven(NotProvenReason::InstrumentationAbsent { .. })
        ));
        assert!(matches!(
            view.generated_contributions_of("sym:generator"),
            IndexAnswer::NotProven(NotProvenReason::InstrumentationAbsent { .. })
        ));
    }

    #[test]
    fn unadmitted_fact_class_is_not_proven_not_empty() {
        let mut model = ProjectModel::empty("proj", FactClasses::FILES | FactClasses::SYNTAX);
        model.files.push(file("lib/A.pm", "a"));
        let view = SemanticQueryView::build(&model).unwrap();
        assert_eq!(
            view.family_completeness("declarations"),
            Some(&IndexCompleteness::NotProven(NotProvenReason::FactClassNotAdmitted))
        );
        assert!(matches!(
            view.declarations_in_file(&FileId::new("lib/A.pm", &Digest::of("a"))),
            IndexAnswer::NotProven(NotProvenReason::FactClassNotAdmitted)
        ));
        // Sources stay provable because FILES was admitted.
        assert_eq!(view.family_completeness("sources"), Some(&IndexCompleteness::Complete));
    }

    #[test]
    fn parse_failure_makes_affected_answers_partial() {
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/Good.pm", "good"));
        let mut bad = file("lib/Bad.pm", "bad");
        bad.parse_status = ParseStatus::Failed;
        model.files.push(bad);
        model.limitations.push(ModelLimitation {
            id: "parse-failed:lib/Bad.pm".to_string(),
            kind: "parse_failure".to_string(),
            message: "unbalanced braces".to_string(),
            paths: Vec::new(),
        });
        let good_sym = symbol("lib/Good.pm", "good", None, "ok", "OK::ok", 0, 8);
        model.symbols.push(good_sym);

        let view = SemanticQueryView::build(&model).unwrap();
        assert_eq!(
            view.family_completeness("declarations"),
            Some(&IndexCompleteness::Partial {
                limitation_ids: vec!["parse-failed:lib/Bad.pm".to_string()]
            })
        );

        let good_id = FileId::new("lib/Good.pm", &Digest::of("good"));
        assert!(matches!(view.declarations_in_file(&good_id), IndexAnswer::Complete(_)));

        let bad_entry_id = FileId::new("lib/Bad.pm", &Digest::of("bad"));
        match view.declarations_in_file(&bad_entry_id) {
            IndexAnswer::Partial { limitation_ids, .. } => {
                assert_eq!(limitation_ids, ["parse-failed:lib/Bad.pm"]);
            }
            other => assert!(
                matches!(other, IndexAnswer::Partial { .. }),
                "expected partial answer, got {other:?}"
            ),
        }
    }

    #[test]
    fn legitimate_exact_empty_requires_complete_denominator() {
        let view =
            SemanticQueryView::build(&model_with(vec![file("lib/A.pm", "a")], vec![], vec![]))
                .unwrap();
        assert_eq!(
            view.source_by_path("lib/Missing.pm").completeness(),
            IndexCompleteness::Complete
        );
        assert!(view.source_by_path("lib/Missing.pm").rows().is_none_or(|row| row.is_none()));
    }

    #[test]
    fn wrong_root_is_rejected() {
        let model = model_with(vec![], vec![], vec![]);
        assert_eq!(
            SemanticQueryView::build_checked(CheckedBuildInput {
                model: &model,
                expected_root: Some("other-root"),
                min_shard_generation: None,
            })
            .unwrap_err(),
            ViewRejection::RootMismatch {
                expected: "other-root".to_string(),
                actual: "proj".to_string()
            }
        );
    }

    #[test]
    fn schema_incompatible_shard_is_rejected() {
        let mut model = model_with(vec![file("lib/A.pm", "a")], vec![], vec![]);
        model.shard_states.insert(
            "lib/A.pm".to_string(),
            crate::ProjectShardState {
                generation: 3,
                producer: "test".to_string(),
                schema_version: SCHEMA_VERSION - 1,
                fingerprint: "fnv64:deadbeefdeadbeef".to_string(),
                limitation_ids: Vec::new(),
                populated: Some(FactClasses::NONE),
                limitation_paths: BTreeMap::new(),
            },
        );
        assert!(matches!(
            SemanticQueryView::build(&model),
            Err(ViewRejection::SchemaIncompatible { shard_schema, .. }) if shard_schema == SCHEMA_VERSION - 1
        ));
    }

    #[test]
    fn mixed_generation_adoption_is_rejected() {
        let mut model =
            model_with(vec![file("lib/A.pm", "a"), file("lib/B.pm", "b")], vec![], vec![]);
        model.shard_states.insert(
            "lib/A.pm".to_string(),
            crate::ProjectShardState {
                generation: 2,
                producer: "test".to_string(),
                schema_version: SCHEMA_VERSION,
                fingerprint: "fnv64:0000000000000001".to_string(),
                limitation_ids: Vec::new(),
                populated: Some(FactClasses::NONE),
                limitation_paths: BTreeMap::new(),
            },
        );
        match SemanticQueryView::build(&model) {
            Err(ViewRejection::MixedGenerationAdoption { unadopted_paths }) => {
                assert_eq!(unadopted_paths, ["lib/B.pm"]);
            }
            other => assert!(
                matches!(other, Err(ViewRejection::MixedGenerationAdoption { .. })),
                "expected mixed-adoption rejection, got {other:?}"
            ),
        }
    }

    #[test]
    fn stale_candidate_below_generation_floor_is_rejected() {
        let mut model = model_with(vec![file("lib/A.pm", "a")], vec![], vec![]);
        model.shard_states.insert(
            "lib/A.pm".to_string(),
            crate::ProjectShardState {
                generation: 4,
                producer: "test".to_string(),
                schema_version: SCHEMA_VERSION,
                fingerprint: "fnv64:0000000000000002".to_string(),
                limitation_ids: Vec::new(),
                populated: Some(FactClasses::NONE),
                limitation_paths: BTreeMap::new(),
            },
        );
        let err = SemanticQueryView::build_checked(CheckedBuildInput {
            model: &model,
            expected_root: None,
            min_shard_generation: Some(7),
        })
        .unwrap_err();
        assert_eq!(
            err,
            ViewRejection::StaleGeneration { path: "lib/A.pm".to_string(), floor: 7, actual: 4 }
        );
    }

    #[test]
    fn unavailable_snapshot_identity_is_typed_rejection_not_sentinel() {
        // Negative control at the mapping seam: a snapshot-serialization
        // failure must reject construction with the typed variant — never
        // accept the generation with a fabricated constant identity.
        let err = accepted_snapshot_identity(Err(ShardError::Serialization {
            message: "model serialization failed".to_string(),
        }))
        .unwrap_err();
        assert!(
            matches!(err, ViewRejection::SnapshotIdentityUnavailable { ref detail }
                if detail.contains("model serialization failed")),
            "expected SnapshotIdentityUnavailable carrying the underlying failure, got {err:?}"
        );
        assert!(err.to_string().contains("snapshot identity unavailable"));

        // The happy path passes the identity through unchanged.
        assert_eq!(accepted_snapshot_identity(Ok("fnv64:abc".to_string())).unwrap(), "fnv64:abc");

        // An accepted view carries exactly the model's serialization
        // identity as its freshness anchor.
        let model = model_with(vec![file("lib/A.pm", "a")], vec![], vec![]);
        let view = SemanticQueryView::build(&model).unwrap();
        assert_eq!(view.model_snapshot_identity, model.snapshot_identity().unwrap());
    }

    #[test]
    fn input_permutation_produces_identical_views() {
        let files = vec![file("lib/Z.pm", "zed"), file("lib/A.pm", "aye")];
        let symbols = vec![
            symbol("lib/Z.pm", "zed", Some("Z"), "b", "Z::b", 20, 30),
            symbol("lib/Z.pm", "zed", Some("Z"), "a", "Z::a", 0, 10),
            symbol("lib/A.pm", "aye", Some("A"), "m", "A::m", 4, 14),
        ];
        let packages = vec![
            package_record("lib/Z.pm", "zed", "Z", 0, 100),
            package_record("lib/A.pm", "aye", "A", 0, 50),
        ];

        let ordered = model_with(files.clone(), packages.clone(), symbols.clone());

        let mut reversed_files = files;
        reversed_files.reverse();
        let mut reversed_symbols = symbols;
        reversed_symbols.reverse();
        let mut reversed_packages = packages;
        reversed_packages.reverse();
        let permuted = model_with(reversed_files, reversed_packages, reversed_symbols);

        let left = SemanticQueryView::build(&ordered).unwrap();
        let right = SemanticQueryView::build(&permuted).unwrap();

        // The view fingerprint and every materialized table are identical;
        // `model_snapshot_identity` deliberately tracks raw model vector
        // order, so it is not part of the view's canonical output.
        assert_eq!(left.fingerprint(), right.fingerprint());
        assert_eq!(left.sources, right.sources);
        assert_eq!(left.declarations_by_file, right.declarations_by_file);
        assert_eq!(left.symbols, right.symbols);
        assert_eq!(left.packages, right.packages);
        assert_eq!(left.anchors_by_file, right.anchors_by_file);
        assert_eq!(left.anchor_max_ends, right.anchor_max_ends);
        assert_eq!(left.completeness, right.completeness);
        assert_eq!(left.work, right.work);
    }

    #[test]
    fn nested_anchor_enclosures_are_found() {
        // The review repro: package span encloses member spans; a query past
        // the member's end must still find the enclosing package.
        let content = "nested";
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/N.pm", content));
        let pkg = package_record("lib/N.pm", content, "N", 0, 100);
        let inner = symbol("lib/N.pm", content, Some("N"), "run", "N::run", 10, 20);
        model.packages.push(pkg);
        model.symbols.push(inner);

        let view = SemanticQueryView::build(&model).unwrap();
        let file_id = FileId::new("lib/N.pm", &Digest::of(content));

        let (hits, _) = view.anchors_overlapping(&file_id, 50, 60).rows().unwrap();
        assert_eq!(hits.len(), 1, "enclosing package anchor must be found");
        assert!(hits[0].entity_key.starts_with("pkg:"));

        // Query inside the member reports both nested anchors.
        let (both, _) = view.anchors_overlapping(&file_id, 12, 18).rows().unwrap();
        assert_eq!(both.len(), 2);

        // Deeper chain: only ancestors enclosing the query survive.
        let deep_pkg = package_record("lib/N.pm", content, "M", 5, 95);
        let deep_sub = symbol("lib/N.pm", content, Some("M"), "go", "M::go", 30, 40);
        model.packages.push(deep_pkg);
        model.symbols.push(deep_sub);
        let view2 = SemanticQueryView::build(&model).unwrap();
        let (deep_hits, _) = view2.anchors_overlapping(&file_id, 50, 60).rows().unwrap();
        assert_eq!(deep_hits.len(), 2, "package spans [0,100) and [5,95) enclose [50,60)");
    }

    #[test]
    fn anchor_interval_boundaries_are_half_open() {
        let content = "edges";
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/E.pm", content));
        // Anchor exactly left of the query and exactly right of it.
        model.symbols.push(symbol("lib/E.pm", content, None, "l", "L::l", 0, 10));
        model.symbols.push(symbol("lib/E.pm", content, None, "r", "R::r", 20, 30));
        let view = SemanticQueryView::build(&model).unwrap();
        let file_id = FileId::new("lib/E.pm", &Digest::of(content));

        // Touching on either edge does not overlap [10, 20).
        let (touching, _) = view.anchors_overlapping(&file_id, 10, 20).rows().unwrap();
        assert!(touching.is_empty(), "edge-touching anchors must not overlap");

        // One byte into each neighbor overlaps it.
        let (left_hit, _) = view.anchors_overlapping(&file_id, 9, 11).rows().unwrap();
        assert_eq!(left_hit.iter().filter(|row| row.start_byte == 0).count(), 1);
        let (right_hit, _) = view.anchors_overlapping(&file_id, 19, 21).rows().unwrap();
        assert_eq!(right_hit.iter().filter(|row| row.end_byte == 30).count(), 1);

        // Zero-width anchor at 15: reachable from strictly-inside queries.
        let mut with_point = ProjectModel::empty("proj", FactClasses::all());
        with_point.files.push(file("lib/P.pm", content));
        with_point.symbols.push(symbol("lib/P.pm", content, None, "p", "P::p", 15, 15));
        let point_view = SemanticQueryView::build(&with_point).unwrap();
        let point_id = FileId::new("lib/P.pm", &Digest::of(content));
        let (inside, _) = point_view.anchors_overlapping(&point_id, 14, 16).rows().unwrap();
        assert_eq!(inside.len(), 1, "zero-width anchor reachable from an enclosing query");
        let (at_start_edge, _) = point_view.anchors_overlapping(&point_id, 15, 16).rows().unwrap();
        assert!(
            at_start_edge.is_empty(),
            "zero-width anchor coincident with the query start is empty under half-open rules"
        );
    }

    #[test]
    fn hot_anchor_lookup_avoids_full_scan() {
        let content = "anchors";
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/Big.pm", content));
        let total = 64u32;
        let mut records = Vec::new();
        for i in 0..total {
            let start = i * 100;
            records.push(symbol(
                "lib/Big.pm",
                content,
                Some("Big"),
                &format!("s{i}"),
                &format!("Big::s{i}"),
                start,
                start + 10,
            ));
        }
        model.symbols = records;

        let view = SemanticQueryView::build(&model).unwrap();
        let file_id = FileId::new("lib/Big.pm", &Digest::of(content));
        let query_start = 63 * 100 - 1;
        let answer = view.anchors_overlapping(&file_id, query_start, query_start + 12);
        assert_eq!(answer.completeness(), IndexCompleteness::Complete);
        let (hits, work) = answer.rows().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(work.candidate_rows, total as usize);
        // Strict ratchet against prefix scans: this late-file query sits
        // past every earlier anchor's end, so the old full left-window
        // filter would report scanned_rows == candidate_rows. The max-end
        // descent examines exactly the rows it reports.
        assert_eq!(
            work.scanned_rows,
            hits.len(),
            "overlap filter examined non-hit rows: prefix-scan regression"
        );
        // Probes stay logarithmic: right-cut binary search (<= ceil(log2)+1)
        // plus a max-end descent bounded by the search path and pruned
        // siblings.
        let log2 = (work.candidate_rows as f64).log2().ceil() as usize;
        assert!(
            work.probes <= (log2 + 1) + 4 * (log2 + 2) * hits.len().max(1),
            "hot lookup spent {} probes on {} rows with {} hits",
            work.probes,
            work.candidate_rows,
            hits.len()
        );
        // A query beyond every anchor exits without scanning the window.
        let (_, beyond) =
            view.anchors_overlapping(&file_id, 70 * 100, 70 * 100 + 5).rows().unwrap();
        assert_eq!(beyond.scanned_rows, 0);

        // Boundary semantics: [start, end) overlap.
        let gap_answer = view.anchors_overlapping(&file_id, 11, 99);
        assert_eq!(gap_answer.completeness(), IndexCompleteness::Complete);
        let no_hits = gap_answer.rows().unwrap().0;
        assert!(no_hits.is_empty(), "interval between anchors must be legitimately empty");
    }

    #[test]
    fn edit_bump_and_identical_reingest_flow_through_generations() {
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let original = file("lib/V.pm", "version-one");
        let sub_v1 = symbol("lib/V.pm", "version-one", Some("V"), "go", "V::go", 0, 9);

        let mut model = ProjectModel::empty("proj", requested);
        let mut shard = ProjectFactShard::empty(original.clone(), 1, "test-producer", requested);
        shard.populated |= FactClasses::SYMBOLS;
        shard.source_len_bytes = 11;
        shard.symbols.push(sub_v1.clone());
        model.insert_or_replace(shard).unwrap();

        let view_v1 = SemanticQueryView::build(&model).unwrap();
        assert_eq!(
            view_v1
                .source_by_path("lib/V.pm")
                .rows()
                .and_then(|entry| entry.map(|e| e.digest.as_str().to_owned())),
            Some(Digest::of("version-one").as_str().to_owned())
        );

        // Identical content re-ingested at a later generation replaces the
        // shard state (the substrate advances generations), so the view's
        // source identity follows while declarations stay byte-identical.
        let mut same_model = model.clone();
        let mut identical =
            ProjectFactShard::empty(original.clone(), 2, "test-producer", requested);
        identical.populated |= FactClasses::SYMBOLS;
        identical.source_len_bytes = 11;
        identical.symbols.push(sub_v1.clone());
        same_model.insert_or_replace(identical).unwrap();
        let view_v2_same_content = SemanticQueryView::build(&same_model).unwrap();
        assert_ne!(view_v2_same_content.fingerprint(), view_v1.fingerprint());
        assert_eq!(
            view_v2_same_content.declarations_by_file, view_v1.declarations_by_file,
            "source-identical later generation keeps declaration rows stable"
        );
        assert_eq!(view_v2_same_content.anchors_by_file, view_v1.anchors_by_file);

        // A real edit bumps digest and generation; the view follows.
        let edited = file("lib/V.pm", "version-two");
        let sub_v2 = symbol("lib/V.pm", "version-two", Some("V"), "go", "V::go", 0, 9);
        let mut edited_shard =
            ProjectFactShard::empty(edited.clone(), 3, "test-producer", requested);
        edited_shard.populated |= FactClasses::SYMBOLS;
        edited_shard.source_len_bytes = 11;
        edited_shard.symbols.push(sub_v2.clone());
        model.insert_or_replace(edited_shard).unwrap();

        let view_v2 = SemanticQueryView::build(&model).unwrap();
        assert_ne!(view_v1.fingerprint(), view_v2.fingerprint());
        let generation = view_v2
            .source_by_path("lib/V.pm")
            .rows()
            .and_then(|entry| entry.and_then(|e| e.shard.as_ref()))
            .map(|shard| shard.generation);
        assert_eq!(generation, Some(3));

        // The superseded candidate is stale against the current floor.
        assert!(matches!(
            SemanticQueryView::build_checked(CheckedBuildInput {
                model: &same_model,
                expected_root: None,
                min_shard_generation: Some(3),
            }),
            Err(ViewRejection::StaleGeneration { actual: 2, .. })
        ));
    }

    #[test]
    fn work_receipt_records_rows_and_bytes_per_family() {
        let view = SemanticQueryView::build(&model_with(
            vec![file("lib/A.pm", "a"), file("lib/B.pm", "b")],
            vec![package_record("lib/A.pm", "a", "A", 0, 5)],
            vec![symbol("lib/A.pm", "a", Some("A"), "f", "A::f", 6, 16)],
        ))
        .unwrap();
        let receipt = view.work();
        assert_eq!(receipt.families["sources"].rows, 2);
        assert_eq!(receipt.families["declarations"].rows, 2);
        assert_eq!(receipt.families["declaration_anchors"].rows, 2);
        assert_eq!(receipt.total_rows, 6);
        assert!(receipt.families.values().all(|family| family.approx_bytes > 0));
    }

    #[test]
    fn rejection_displays_readably() {
        let rejection =
            ViewRejection::StaleGeneration { path: "lib/A.pm".to_string(), floor: 7, actual: 4 };
        assert!(rejection.to_string().contains("below required floor"));
    }

    #[test]
    fn unread_discovered_path_bounds_source_answers() {
        // Issue #13288 item 1: a discovered-but-unread path stays in the
        // source denominator, so its absence is bounded — never a fabricated
        // legitimate empty.
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/Good.pm", "good"));
        model.limitations.push(ModelLimitation {
            id: "read-failed:lib/Locked.pm".to_string(),
            kind: "read_failure".to_string(),
            message: "could not read `lib/Locked.pm`".to_string(),
            paths: vec!["lib/Locked.pm".to_string()],
        });
        model.unread_discovered.insert("lib/Locked.pm".to_string());
        let view = SemanticQueryView::build(&model).unwrap();

        assert!(matches!(
            view.family_completeness("sources"),
            Some(IndexCompleteness::Partial { .. })
        ));
        match view.source_by_path("lib/Locked.pm") {
            IndexAnswer::Partial { rows: None, limitation_ids } => {
                assert_eq!(limitation_ids, ["read-failed:lib/Locked.pm"]);
            }
            other => assert!(
                matches!(other, IndexAnswer::Partial { rows: None, .. }),
                "expected bounded absent answer, got {other:?}"
            ),
        }
        // A never-discovered path stays a legitimate exact empty.
        assert!(matches!(view.source_by_path("lib/NeverSeen.pm"), IndexAnswer::Complete(None)));
        match view.sources_with_role(FileRole::Lib) {
            IndexAnswer::Partial { rows, limitation_ids } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(limitation_ids, ["read-failed:lib/Locked.pm"]);
            }
            other => assert!(
                matches!(other, IndexAnswer::Partial { .. }),
                "expected partial library sources, got {other:?}"
            ),
        }
        assert!(matches!(
            view.sources_with_role(FileRole::Test),
            IndexAnswer::Complete(rows) if rows.is_empty()
        ));
    }

    #[test]
    fn view_fingerprint_includes_unread_paths() {
        let mut model = ProjectModel::empty("proj", FactClasses::all());
        model.files.push(file("lib/Good.pm", "good"));
        model.limitations.push(ModelLimitation {
            id: "read-failed:lib/Locked.pm".to_string(),
            kind: "read_failure".to_string(),
            message: "could not read `lib/Locked.pm`".to_string(),
            paths: vec!["lib/Locked.pm".to_string()],
        });
        model.unread_discovered.insert("lib/Locked.pm".to_string());
        let mut with_extra_unread = model.clone();
        with_extra_unread.unread_discovered.insert("lib/Other.pm".to_string());

        let base_view = SemanticQueryView::build(&model).unwrap();
        let extra_view = SemanticQueryView::build(&with_extra_unread).unwrap();
        assert_ne!(base_view.fingerprint(), extra_view.fingerprint());
    }

    #[test]
    fn view_fingerprint_separates_limitation_path_groups() {
        let model_with_paths = |a: &[&str], b: &[&str]| {
            let mut model = ProjectModel::empty("proj", FactClasses::all());
            model.unread_discovered.extend(["p", "b", "q"].map(String::from));
            model.limitations = vec![
                ModelLimitation {
                    id: "a".to_string(),
                    kind: "producer_gap".to_string(),
                    message: "first limitation".to_string(),
                    paths: a.iter().map(|path| (*path).to_string()).collect(),
                },
                ModelLimitation {
                    id: "b".to_string(),
                    kind: "producer_gap".to_string(),
                    message: "second limitation".to_string(),
                    paths: b.iter().map(|path| (*path).to_string()).collect(),
                },
            ];
            model
        };
        let model_a = model_with_paths(&["p", "b"], &["q"]);
        let model_b = model_with_paths(&["p"], &["b", "q"]);

        let view_a = SemanticQueryView::build(&model_a).unwrap();
        let view_b = SemanticQueryView::build(&model_b).unwrap();
        assert_ne!(view_a.fingerprint(), view_b.fingerprint());
    }

    #[test]
    fn view_fingerprint_separates_unread_and_limitation_sections() {
        let mut unread_model = ProjectModel::empty("proj", FactClasses::all());
        unread_model.unread_discovered.insert("a".to_string());
        unread_model.limitations.push(ModelLimitation {
            id: "b".to_string(),
            kind: "producer_gap".to_string(),
            message: "limitation".to_string(),
            paths: vec!["q".to_string()],
        });

        let mut limitation_model = ProjectModel::empty("proj", FactClasses::all());
        limitation_model.limitations.push(ModelLimitation {
            id: "a".to_string(),
            kind: "producer_gap".to_string(),
            message: "limitation".to_string(),
            paths: vec!["b".to_string(), "q".to_string()],
        });

        let unread_view = SemanticQueryView::build(&unread_model).unwrap();
        let limitation_view = SemanticQueryView::build(&limitation_model).unwrap();
        assert_ne!(unread_view.fingerprint(), limitation_view.fingerprint());
    }

    #[test]
    fn shard_without_populated_symbols_cannot_claim_complete_denominator() {
        // Issue #13288 item 2: a shard that never populated the declarations
        // class cannot back a proven-empty answer for its file; extraction
        // that never ran is not a proven zero.
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let mut model = ProjectModel::empty("proj", requested);
        let mut shard =
            ProjectFactShard::empty(file("lib/Quiet.pm", "quiet"), 1, "test-producer", requested);
        // populated stays without SYMBOLS: the producer never ran extraction.
        shard.source_len_bytes = 5;
        model.insert_or_replace(shard).unwrap();
        let view = SemanticQueryView::build(&model).unwrap();
        assert_eq!(
            view.family_completeness("declarations"),
            Some(&IndexCompleteness::NotProven(NotProvenReason::ShardClassNotPopulated {
                family: "declarations"
            }))
        );
        assert!(matches!(
            view.declarations_in_file(&FileId::new("lib/Quiet.pm", &Digest::of("quiet"))),
            IndexAnswer::NotProven(NotProvenReason::ShardClassNotPopulated { .. })
        ));

        // Negative control (#12063): a shard that DID populate the class with
        // zero rows is a legitimate exact empty and stays Complete.
        let mut model = ProjectModel::empty("proj", requested);
        let mut shard =
            ProjectFactShard::empty(file("lib/Empty.pm", "empty"), 1, "test-producer", requested);
        shard.populated |= FactClasses::SYMBOLS;
        shard.source_len_bytes = 5;
        model.insert_or_replace(shard).unwrap();
        let view = SemanticQueryView::build(&model).unwrap();
        assert_eq!(view.family_completeness("declarations"), Some(&IndexCompleteness::Complete));
    }

    #[test]
    fn legacy_persisted_shard_state_keeps_zero_rows_complete() {
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let mut model = ProjectModel::empty("proj", requested);
        let shard =
            ProjectFactShard::empty(file("lib/Empty.pm", "empty"), 1, "test-producer", requested);
        model.insert_or_replace(shard).unwrap();

        let state_json = serde_json::to_string(&model.shard_states["lib/Empty.pm"]).unwrap();
        assert!(!state_json.contains("limitation_paths"));
        let mut legacy_state = serde_json::to_value(&model.shard_states["lib/Empty.pm"]).unwrap();
        legacy_state.as_object_mut().unwrap().remove("populated");
        let decoded_state: crate::ProjectShardState = serde_json::from_value(legacy_state).unwrap();
        assert_eq!(decoded_state.populated, None);

        let mut persisted = serde_json::to_value(&model).unwrap();
        let states = persisted.get_mut("shard_states").and_then(Value::as_object_mut).unwrap();
        let state = states.get_mut("lib/Empty.pm").and_then(Value::as_object_mut).unwrap();
        state.remove("populated");
        let restored: ProjectModel = serde_json::from_value(persisted).unwrap();

        assert_eq!(restored.shard_states["lib/Empty.pm"].populated, None);
        let view = SemanticQueryView::build(&restored).unwrap();
        assert_eq!(view.family_completeness("declarations"), Some(&IndexCompleteness::Complete));
    }

    #[test]
    fn non_suffixed_shard_limitation_bounds_scoped_answers() {
        // Issue #13288 item 3: the limitation-to-path association is carried
        // structurally from shard ownership, so a valid non-suffixed id is
        // not dropped when answers are scoped (which used to upgrade Partial
        // to Complete).
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let mut model = ProjectModel::empty("proj", requested);
        let mut shard =
            ProjectFactShard::empty(file("lib/A.pm", "aaa"), 1, "test-producer", requested);
        shard.populated |= FactClasses::SYMBOLS;
        shard.source_len_bytes = 3;
        shard.limitations.push(ModelLimitation {
            id: "tokenizer-v2-gap".to_string(),
            kind: "producer_gap".to_string(),
            message: "tokenizer could not classify a region".to_string(),
            paths: Vec::new(),
        });
        model.insert_or_replace(shard).unwrap();
        let view = SemanticQueryView::build(&model).unwrap();

        assert_eq!(
            view.family_completeness("declarations"),
            Some(&IndexCompleteness::Partial {
                limitation_ids: vec!["tokenizer-v2-gap".to_string()]
            })
        );
        let file_id = FileId::new("lib/A.pm", &Digest::of("aaa"));
        match view.declarations_in_file(&file_id) {
            IndexAnswer::Partial { limitation_ids, .. } => {
                assert_eq!(
                    limitation_ids,
                    ["tokenizer-v2-gap"],
                    "non-suffixed id must bound the scoped answer"
                );
            }
            other => assert!(
                matches!(other, IndexAnswer::Partial { .. }),
                "expected partial scoped answer, got {other:?}"
            ),
        }
    }

    #[test]
    fn adoption_of_readable_shard_retires_unread_marker() {
        // A discovered-but-unread path that later becomes readable through
        // shard adoption leaves the unread denominator: the real file record
        // supersedes the walk-time marker.
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let mut model = ProjectModel::empty("proj", requested);
        model.limitations.push(ModelLimitation {
            id: "read-failed:lib/A.pm".to_string(),
            kind: "read_failure".to_string(),
            message: "transient read failure".to_string(),
            paths: vec!["lib/A.pm".to_string()],
        });
        model.unread_discovered.insert("lib/A.pm".to_string());
        let mut shard =
            ProjectFactShard::empty(file("lib/A.pm", "aaa"), 1, "test-producer", requested);
        shard.populated |= FactClasses::SYMBOLS;
        shard.source_len_bytes = 3;
        model.insert_or_replace(shard).unwrap();
        assert!(model.unread_discovered.is_empty());

        let view = SemanticQueryView::build(&model).unwrap();
        assert!(matches!(view.source_by_path("lib/A.pm"), IndexAnswer::Complete(_)));
    }

    #[test]
    fn adoption_of_one_multi_path_read_failure_preserves_other_paths() {
        let requested = FactClasses::FILES | FactClasses::SYMBOLS;
        let mut model = ProjectModel::empty("proj", requested);
        let limitation_id = "read-failed:shared".to_string();
        model.limitations.push(ModelLimitation {
            id: limitation_id.clone(),
            kind: "read_failure".to_string(),
            message: "shared read failure".to_string(),
            paths: vec!["lib/A.pm".to_string(), "lib/B.pm".to_string()],
        });
        model.unread_discovered.extend(["lib/A.pm", "lib/B.pm"].map(String::from));

        let mut shard =
            ProjectFactShard::empty(file("lib/A.pm", "aaa"), 1, "test-producer", requested);
        shard.populated |= FactClasses::SYMBOLS;
        shard.source_len_bytes = 3;
        model.insert_or_replace(shard).unwrap();

        assert_eq!(model.limitations.len(), 1);
        assert_eq!(model.limitations[0].paths, ["lib/B.pm"]);
        let view = SemanticQueryView::build(&model).unwrap();
        assert!(matches!(view.source_by_path("lib/A.pm"), IndexAnswer::Complete(_)));
        match view.source_by_path("lib/B.pm") {
            IndexAnswer::Partial { limitation_ids, .. } => {
                assert_eq!(limitation_ids, [limitation_id]);
            }
            other => assert!(
                matches!(other, IndexAnswer::Partial { .. }),
                "expected partial answer for unread path, got {other:?}"
            ),
        }
    }
}
