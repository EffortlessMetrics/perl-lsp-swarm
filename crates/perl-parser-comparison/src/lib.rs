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
//! Enabling both grammar features is rejected at compile time. This keeps the
//! current and historical subjects distinct before worker-process migration.

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

#[cfg(all(feature = "historical", feature = "current-upstream"))]
compile_error!(
    "historical and current-upstream Tree-sitter Perl subjects export the same native symbol; build exactly one grammar feature per binary"
);

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
    CurrentUpstreamAdapter, CurrentUpstreamAdapterError, CurrentUpstreamExecutionDisposition,
    CurrentUpstreamParse, CurrentUpstreamPinError, CurrentUpstreamSubjectIdentity, PACKAGE_CHECKSUM,
    PACKAGE_NAME, PACKAGE_REQUIREMENT, PACKAGE_VERSION, SUBJECT_IDENTITY_TOML,
    TREE_SITTER_LANGUAGE_VERSION, TREE_SITTER_RUNTIME_VERSION, UPSTREAM_COMMIT,
    UPSTREAM_REPOSITORY, UPSTREAM_RUST_VERSION, UPSTREAM_TAG, validate_exact_package_requirement,
};
#[cfg(feature = "historical")]
pub use harness::{ParseResult, parse_v1, parse_v2, parse_v3};
#[cfg(feature = "historical")]
pub use outcomes::Verdict;
