//! Semantic snapshot schema and stability rail for HIR lowering.
//!
//! # Purpose — SNAPSHOT, not gold
//!
//! This module records **deterministic HIR snapshots** produced by running
//! `lower_ast()` over a small corpus slice. The snapshots prove *stability*:
//! the same source produces the same HIR item sequence across runs and commits.
//!
//! **This is NOT correctness / curated-gold.** Gold corpus entries (independent
//! human labeling of expected semantic facts) are a separate, future schema.
//! See `docs/specs/PLSP-SPEC-0033-semantic-snapshot-rail.md` for the full
//! claim boundary.
//!
//! # KPI
//!
//! The KPI name is `semantic_snapshot_stability_rate` (fraction of snapshots
//! that match the recorded reference). It is NOT named `semantic_gold_pass_rate`.
//!
//! # Design
//!
//! Each snapshot entry records:
//! - `fixture_id` — stable identifier for the source fixture
//! - `source_hash` — stable, versioned digest of the source text (hex, lowercase)
//! - `hir_schema_version` — monotonic HIR schema model version string
//! - `hir_summary` — deterministic structural summary of the lowered `HirFile`
//!
//! The summary avoids raw source offsets (which change on whitespace edits) and
//! instead records item-kind sequences, scope counts, and stash metrics that
//! are stable across semantics-preserving reformatting.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// Monotonic HIR schema version string, bumped whenever the lowering model
/// changes in a way that alters snapshot structure.
///
/// Snapshots recorded under a different version are considered stale and must
/// be regenerated before comparison.
pub const HIR_SCHEMA_VERSION: &str = "hir.v1";

/// Stable source-digest algorithm recorded by snapshot manifests.
pub const SOURCE_HASH_ALGORITHM: &str = "fnv1a-128.v1";

/// A deterministic structural summary of one lowered `HirFile`.
///
/// This summary is intentionally coarse-grained: it records item-kind names
/// and graph sizes rather than raw source offsets so that semantics-preserving
/// whitespace changes do not cause drift.
///
/// NOTE: proves stability, not correctness. Curated-gold assertions belong in
/// a separate schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirSummary {
    /// Total number of HIR items lowered from the file.
    pub item_count: usize,
    /// Sequence of HIR item kind names in stable depth-first source order.
    pub item_kind_sequence: Vec<String>,
    /// Number of scope frames in the scope graph.
    pub scope_count: usize,
    /// Number of bindings recorded in the scope graph.
    pub binding_count: usize,
    /// Number of package stashes in the stash graph.
    pub package_count: usize,
    /// Number of stash glob slots across all packages.
    pub slot_count: usize,
    /// Number of compile-time directive effects recorded.
    pub directive_count: usize,
    /// Number of module requests recorded in the compile environment.
    pub module_request_count: usize,
    /// Number of dynamic-boundary markers.
    pub dynamic_boundary_count: usize,
}

/// One recorded snapshot entry for a single source fixture.
///
/// The `source_hash` + `hir_schema_version` key identifies the snapshot
/// uniquely. A check-mode comparison fails when the recorded summary differs
/// from the freshly computed one, indicating HIR drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Stable fixture identifier: normalized path relative to the fixture root.
    pub fixture_id: String,
    /// Stable source digest (32 lowercase hexadecimal characters, no prefix).
    ///
    /// Computed via [`source_hash`] using [`SOURCE_HASH_ALGORITHM`].
    pub source_hash: String,
    /// HIR schema version at snapshot generation time.
    pub hir_schema_version: String,
    /// Deterministic structural summary of the lowered HIR.
    ///
    /// Proves stability — not correctness. See module doc.
    pub hir_summary: HirSummary,
}

/// Exact-set validation failure for a recorded/fresh snapshot comparison.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotSetError {
    /// The recorded manifest contains no entries.
    EmptyRecorded,
    /// The freshly computed population contains no entries.
    EmptyFresh,
    /// The recorded manifest contains the same fixture ID more than once.
    DuplicateRecorded {
        /// Duplicated fixture ID.
        fixture_id: String,
    },
    /// The freshly computed population contains the same fixture ID more than once.
    DuplicateFresh {
        /// Duplicated fixture ID.
        fixture_id: String,
    },
    /// A recorded fixture is absent from the freshly computed population.
    MissingFresh {
        /// Recorded fixture ID absent from the fresh population.
        fixture_id: String,
    },
    /// A fresh fixture is absent from the recorded manifest.
    UnexpectedFresh {
        /// Fresh fixture ID absent from the recorded manifest.
        fixture_id: String,
    },
}

impl fmt::Display for SnapshotSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRecorded => formatter.write_str("recorded snapshot manifest is empty"),
            Self::EmptyFresh => formatter.write_str("fresh snapshot fixture population is empty"),
            Self::DuplicateRecorded { fixture_id } => {
                write!(formatter, "duplicate recorded snapshot fixture ID: {fixture_id}")
            }
            Self::DuplicateFresh { fixture_id } => {
                write!(formatter, "duplicate fresh snapshot fixture ID: {fixture_id}")
            }
            Self::MissingFresh { fixture_id } => {
                write!(
                    formatter,
                    "recorded snapshot fixture is missing from fresh input: {fixture_id}"
                )
            }
            Self::UnexpectedFresh { fixture_id } => {
                write!(
                    formatter,
                    "fresh snapshot fixture is absent from recorded manifest: {fixture_id}"
                )
            }
        }
    }
}

impl std::error::Error for SnapshotSetError {}

/// A snapshot manifest collecting all snapshot entries for a corpus slice.
///
/// Written by the `generate-semantic-snapshot` xtask subcommand in generate
/// mode, and read in check mode to detect HIR drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// Schema discriminator — always `"semantic_snapshot.v1"`.
    pub schema: String,
    /// KPI name for this stability rail.
    ///
    /// Always `"semantic_snapshot_stability_rate"`. NOT `"semantic_gold_pass_rate"`.
    pub kpi: String,
    /// Claim boundary: this proves stability, not correctness.
    pub claim_boundary: String,
    /// HIR schema version used for all entries in this manifest.
    pub hir_schema_version: String,
    /// Named, versioned source-digest algorithm used by every entry.
    pub source_hash_algorithm: String,
    /// Date of last generation (ISO 8601 `YYYY-MM-DD`).
    pub generated_on: String,
    /// Snapshot entries, one per fixture in the corpus slice.
    pub entries: Vec<SnapshotEntry>,
}

impl SnapshotManifest {
    /// Create a new manifest with the correct schema discriminators.
    pub fn new(generated_on: String) -> Self {
        Self {
            schema: "semantic_snapshot.v1".to_string(),
            kpi: "semantic_snapshot_stability_rate".to_string(),
            claim_boundary: concat!(
                "Snapshot proves deterministic HIR stability only. ",
                "This is NOT curated-gold correctness. ",
                "Curated gold (independent human labeling) is a separate, future schema.",
            )
            .to_string(),
            hir_schema_version: HIR_SCHEMA_VERSION.to_string(),
            source_hash_algorithm: SOURCE_HASH_ALGORITHM.to_string(),
            generated_on,
            entries: Vec::new(),
        }
    }

    /// Validate that recorded and fresh entries form the same non-empty unique ID set.
    ///
    /// This is deliberately separate from content comparison. A missing, added,
    /// or duplicated fixture is a population-integrity failure, not HIR drift.
    pub fn validate_exact_entry_set(
        &self,
        fresh_entries: &[SnapshotEntry],
    ) -> Result<(), SnapshotSetError> {
        if self.entries.is_empty() {
            return Err(SnapshotSetError::EmptyRecorded);
        }
        if fresh_entries.is_empty() {
            return Err(SnapshotSetError::EmptyFresh);
        }

        let mut recorded_ids = BTreeSet::new();
        let mut duplicate_recorded = BTreeSet::new();
        for entry in &self.entries {
            if !recorded_ids.insert(entry.fixture_id.as_str()) {
                duplicate_recorded.insert(entry.fixture_id.as_str());
            }
        }
        if let Some(fixture_id) = duplicate_recorded.first() {
            return Err(SnapshotSetError::DuplicateRecorded {
                fixture_id: (*fixture_id).to_string(),
            });
        }

        let mut fresh_ids = BTreeSet::new();
        let mut duplicate_fresh = BTreeSet::new();
        for entry in fresh_entries {
            if !fresh_ids.insert(entry.fixture_id.as_str()) {
                duplicate_fresh.insert(entry.fixture_id.as_str());
            }
        }
        if let Some(fixture_id) = duplicate_fresh.first() {
            return Err(SnapshotSetError::DuplicateFresh { fixture_id: (*fixture_id).to_string() });
        }

        if let Some(fixture_id) = recorded_ids.difference(&fresh_ids).next() {
            return Err(SnapshotSetError::MissingFresh { fixture_id: (*fixture_id).to_string() });
        }
        if let Some(fixture_id) = fresh_ids.difference(&recorded_ids).next() {
            return Err(SnapshotSetError::UnexpectedFresh {
                fixture_id: (*fixture_id).to_string(),
            });
        }

        Ok(())
    }

    /// Compute the `semantic_snapshot_stability_rate` KPI.
    ///
    /// Returns `(stable_count, total_count, rate)`. The KPI fails closed to zero
    /// when the recorded and fresh populations are empty, duplicated, or unequal.
    pub fn stability_rate(&self, fresh_entries: &[SnapshotEntry]) -> (usize, usize, f64) {
        let total = self.entries.len();
        if self.validate_exact_entry_set(fresh_entries).is_err() {
            return (0, total, 0.0);
        }

        let stable = self
            .entries
            .iter()
            .filter(|recorded| {
                fresh_entries.iter().any(|fresh| {
                    fresh.fixture_id == recorded.fixture_id
                        && fresh.source_hash == recorded.source_hash
                        && fresh.hir_schema_version == recorded.hir_schema_version
                        && fresh.hir_summary == recorded.hir_summary
                })
            })
            .count();
        let rate = stable as f64 / total as f64;
        (stable, total, rate)
    }
}

/// Compute the stable FNV-1a 128-bit digest of a source string.
///
/// The algorithm identity is [`SOURCE_HASH_ALGORITHM`]. Output is exactly 32
/// lowercase hexadecimal characters and is stable across Rust versions,
/// operating systems, and CPU architectures.
#[must_use]
pub fn source_hash(source: &str) -> String {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    let hash = source
        .as_bytes()
        .iter()
        .fold(OFFSET_BASIS, |hash, byte| (hash ^ u128::from(*byte)).wrapping_mul(PRIME));
    format!("{hash:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(fixture_id: &str) -> SnapshotEntry {
        SnapshotEntry {
            fixture_id: fixture_id.to_string(),
            source_hash: format!("hash-{fixture_id}"),
            hir_schema_version: HIR_SCHEMA_VERSION.to_string(),
            hir_summary: HirSummary {
                item_count: 1,
                item_kind_sequence: vec!["LiteralExpr".to_string()],
                scope_count: 1,
                binding_count: 0,
                package_count: 0,
                slot_count: 0,
                directive_count: 0,
                module_request_count: 0,
                dynamic_boundary_count: 0,
            },
        }
    }

    fn manifest_with(entries: Vec<SnapshotEntry>) -> SnapshotManifest {
        let mut manifest = SnapshotManifest::new("2026-06-21".to_string());
        manifest.entries = entries;
        manifest
    }

    #[test]
    fn source_hash_is_deterministic() {
        let source = "package Foo; sub bar { return 1; } 1;";
        assert_eq!(source_hash(source), source_hash(source));
    }

    #[test]
    fn source_hash_differs_for_different_sources() {
        let first = source_hash("my $x = 1;");
        let second = source_hash("my $y = 2;");
        assert_ne!(first, second, "distinct sources must produce distinct hashes");
    }

    #[test]
    fn source_hash_matches_portable_known_vector() {
        assert_eq!(source_hash("hello"), "e3e1efd54283d94f7081314b599d31b3");
    }

    #[test]
    fn manifest_schema_discriminators() {
        let manifest = SnapshotManifest::new("2026-06-21".to_string());
        assert_eq!(manifest.schema, "semantic_snapshot.v1");
        assert_eq!(manifest.kpi, "semantic_snapshot_stability_rate");
        assert_eq!(manifest.hir_schema_version, HIR_SCHEMA_VERSION);
        assert_eq!(manifest.source_hash_algorithm, SOURCE_HASH_ALGORITHM);
        assert!(
            manifest.claim_boundary.contains("stability"),
            "claim boundary must mention stability"
        );
        assert!(
            manifest.claim_boundary.contains("NOT"),
            "claim boundary must disclaim gold/correctness"
        );
    }

    #[test]
    fn stability_rate_empty_manifest_fails_closed() {
        let manifest = SnapshotManifest::new("2026-06-21".to_string());
        let (stable, total, rate) = manifest.stability_rate(&[]);
        assert_eq!(stable, 0);
        assert_eq!(total, 0);
        assert!(rate.abs() < f64::EPSILON);
    }

    #[test]
    fn exact_entry_set_rejects_empty_fresh_population() {
        let manifest = manifest_with(vec![sample_entry("foo")]);
        assert_eq!(manifest.validate_exact_entry_set(&[]), Err(SnapshotSetError::EmptyFresh));
    }

    #[test]
    fn exact_entry_set_rejects_duplicate_recorded_id() {
        let entry = sample_entry("foo");
        let manifest = manifest_with(vec![entry.clone(), entry]);
        assert_eq!(
            manifest.validate_exact_entry_set(&[sample_entry("foo")]),
            Err(SnapshotSetError::DuplicateRecorded { fixture_id: "foo".to_string() })
        );
    }

    #[test]
    fn exact_entry_set_rejects_duplicate_fresh_id() {
        let entry = sample_entry("foo");
        let manifest = manifest_with(vec![entry.clone()]);
        assert_eq!(
            manifest.validate_exact_entry_set(&[entry.clone(), entry]),
            Err(SnapshotSetError::DuplicateFresh { fixture_id: "foo".to_string() })
        );
    }

    #[test]
    fn exact_entry_set_rejects_missing_fresh_id() {
        let manifest = manifest_with(vec![sample_entry("foo"), sample_entry("bar")]);
        assert_eq!(
            manifest.validate_exact_entry_set(&[sample_entry("foo")]),
            Err(SnapshotSetError::MissingFresh { fixture_id: "bar".to_string() })
        );
    }

    #[test]
    fn exact_entry_set_rejects_unexpected_fresh_id() {
        let manifest = manifest_with(vec![sample_entry("foo")]);
        assert_eq!(
            manifest.validate_exact_entry_set(&[sample_entry("foo"), sample_entry("bar")]),
            Err(SnapshotSetError::UnexpectedFresh { fixture_id: "bar".to_string() })
        );
    }

    #[test]
    fn stability_rate_all_match() {
        let entry = sample_entry("foo");
        let manifest = manifest_with(vec![entry.clone()]);

        let (stable, total, rate) = manifest.stability_rate(&[entry]);
        assert_eq!(stable, 1);
        assert_eq!(total, 1);
        assert!((rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stability_rate_population_mismatch_fails_closed() {
        let manifest = manifest_with(vec![sample_entry("foo")]);
        let (stable, total, rate) =
            manifest.stability_rate(&[sample_entry("foo"), sample_entry("bar")]);
        assert_eq!(stable, 0);
        assert_eq!(total, 1);
        assert!(rate.abs() < f64::EPSILON);
    }

    #[test]
    fn stability_rate_mismatch_on_drift() {
        let recorded = sample_entry("foo");
        let drifted = SnapshotEntry {
            hir_summary: HirSummary {
                item_count: 5,
                item_kind_sequence: vec!["SubDecl".to_string(), "PackageDecl".to_string()],
                scope_count: 2,
                ..recorded.hir_summary.clone()
            },
            ..recorded.clone()
        };
        let manifest = manifest_with(vec![recorded]);
        let (stable, total, rate) = manifest.stability_rate(&[drifted]);
        assert_eq!(stable, 0);
        assert_eq!(total, 1);
        assert!(rate.abs() < f64::EPSILON);
    }

    #[test]
    fn snapshot_entry_round_trips_json() {
        let entry = SnapshotEntry {
            fixture_id: "sub_basic".to_string(),
            source_hash: source_hash("package Foo; sub bar { 1; } 1;"),
            hir_schema_version: HIR_SCHEMA_VERSION.to_string(),
            hir_summary: HirSummary {
                item_count: 3,
                item_kind_sequence: vec![
                    "PackageDecl".to_string(),
                    "SubDecl".to_string(),
                    "LiteralExpr".to_string(),
                ],
                scope_count: 2,
                binding_count: 0,
                package_count: 1,
                slot_count: 1,
                directive_count: 0,
                module_request_count: 0,
                dynamic_boundary_count: 0,
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SnapshotEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, entry);
    }
}
