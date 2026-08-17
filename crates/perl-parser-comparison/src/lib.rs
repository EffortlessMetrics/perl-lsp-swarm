//! Differential parser test harness for exact current, historical, and native
//! Perl parser subjects.
//!
//! Native Tree-sitter grammars are feature-isolated because the current
//! upstream package and historical vendored snapshot export the same C symbol.
//! A single binary may therefore select exactly one native grammar subject:
//!
//! ```text
//! default / historical
//!   cargo test -p perl-parser-comparison
//!
//! current upstream
//!   cargo test -p perl-parser-comparison \
//!     --no-default-features --features current-upstream
//! ```
//!
//! Grammar exclusivity is enforced by per-binary feature selection: every
//! linking build (tests, bins, CI lanes) enables exactly one grammar feature,
//! and the historical-only targets are gated with `required-features`. The
//! workspace `--all-features` pass is check-only (no linking), so both modules
//! may type-check together there; any attempt to *link* both grammars into one
//! binary still fails loudly with a duplicate native symbol. The
//! current-upstream adapter never falls back to the historical grammar
//! regardless of feature combination.

#![deny(unreachable_pub)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
// Tests in this crate use assertion macros to preserve compact verdict receipts.
#![cfg_attr(test, allow(clippy::panic))]

#[cfg(feature = "historical")]
pub mod corpus_walker;
#[cfg(feature = "current-upstream")]
pub mod current_upstream;
#[cfg(feature = "historical")]
pub mod harness;
#[cfg(feature = "historical")]
pub mod outcomes;

#[cfg(feature = "historical")]
pub use corpus_walker::{
    AggregateStats, DisagreementKind, FileRecord, classify, format_report, walk_corpora,
};
#[cfg(feature = "current-upstream")]
pub use current_upstream::{
    BoundedSubjectText, CURRENT_UPSTREAM_SUBJECT, CurrentUpstreamAdapter,
    CurrentUpstreamAdapterError, CurrentUpstreamParse, CurrentUpstreamParseMode,
    CurrentUpstreamPinError, CurrentUpstreamQueryKind, CurrentUpstreamSubjectManifest,
    SUBJECT_MANIFEST_TOML, validate_exact_package_requirement,
};
#[cfg(feature = "historical")]
pub use harness::{ParseResult, parse_v1, parse_v2, parse_v3};
#[cfg(feature = "historical")]
pub use outcomes::Verdict;
