//! Independence gate for the reference edit transaction model ([#7344]).
//!
//! The model's whole value is that it answers "what are the exact final bytes?"
//! without consulting the production edit path. If it ever delegates to that
//! path, a differential harness built on it would compare the production
//! applicator against itself: a defect would appear identically on both sides
//! and cancel out, and the proof would report agreement it never established.
//!
//! That property is not visible in any single behavioral assertion, so it is
//! gated structurally here. These tests fail if a later change wires the model
//! to production edit application, whether by import or by crate dependency.
//!
//! [#7344]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7344

/// The model's own source, read at compile time.
const MODEL_SOURCE: &str = include_str!("../src/reference_edit.rs");

/// This crate's manifest, read at compile time.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Type and function names that would indicate delegation to a production
/// applicator rather than an independent derivation.
const FORBIDDEN_SYMBOLS: &[&str] = &[
    "IncrementalEdit",
    "IncrementalEditSet",
    "IncrementalEditBatchError",
    "EditSet",
    "PositionMapper",
    "apply_edit",
    "apply_edit_utf8",
    "apply_to_position",
    "apply_to_range",
    "overlaps_range",
    "affected_ranges",
    "perl_parser",
];

/// Crates the model is allowed to import from.
///
/// Both are canonical authorities rather than edit-application code:
/// `perl-position-tracking` owns the accepted `lf-source-lines/v1` row geometry
/// (ADR-0048) and byte spans, and `perl-source-identity` owns content digests.
const ALLOWED_IMPORT_PREFIXES: &[&str] =
    &["use perl_position_tracking::", "use perl_source_identity::", "use thiserror::", "use std::"];

#[test]
fn the_model_names_no_production_edit_applicator() {
    for symbol in FORBIDDEN_SYMBOLS {
        assert!(
            !MODEL_SOURCE.contains(symbol),
            "reference_edit.rs references `{symbol}`. The reference model must derive final \
             bytes independently; delegating to production edit application would make source \
             equivalence self-referential (#7344).",
        );
    }
}

#[test]
fn the_model_imports_only_canonical_authorities() {
    for line in MODEL_SOURCE.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("use ") {
            continue;
        }
        assert!(
            ALLOWED_IMPORT_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix)),
            "reference_edit.rs has an unreviewed import: `{trimmed}`. Widening the model's \
             imports needs a deliberate decision, because every added dependency is a chance \
             to reuse the code the model exists to check (#7344).",
        );
    }
}

#[test]
fn the_crate_cannot_reach_the_incremental_applicator() {
    // `perl-parser` owns `crates/perl-parser/src/incremental/`, the production
    // incremental edit applicator. This crate must not depend on it, so the
    // applicator is unreachable from the model by construction rather than by
    // convention. `perl-parser-core` is a different crate and is permitted.
    for line in MANIFEST.lines() {
        let trimmed = line.trim_start();
        assert!(
            !trimmed.starts_with("perl-parser ="),
            "perl-tdd-support declares a dependency on perl-parser. That puts the production \
             incremental edit applicator within reach of the reference model (#7344).",
        );
    }
}
