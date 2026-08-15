//! Differential parser test harness for current, historical, and native Perl
//! parser subjects.
//!
//! The legacy v1/v2/v3 harness remains available while current-upstream
//! migration proceeds. [`CurrentUpstreamAdapter`] is an additional exact-pinned
//! subject; it does not replace the historical `tree-sitter-perl-c` lane or the
//! native parser's Tree-sitter-style facade.
//!
//! # Legacy parsers
//!
//! | Label | Crate | Description |
//! |-------|-------|-------------|
//! | v1 | `tree-sitter-perl-c` | Historical C Tree-sitter snapshot |
//! | v2 | `perl-parser-pest` | Pest/PEG experimental parser |
//! | v3 | `perl-parser-core` | Recursive-descent native parser |
//!
//! # Current upstream subject
//!
//! ```no_run
//! use perl_parser_comparison::{
//!     CurrentUpstreamAdapter, CurrentUpstreamExecutionDisposition,
//! };
//!
//! let mut adapter = CurrentUpstreamAdapter::new()?;
//! let result = adapter.parse_str("my $x = 42;", None)?;
//! assert_eq!(
//!     result.disposition(),
//!     CurrentUpstreamExecutionDisposition::AcceptedClean,
//! );
//! # Ok::<(), perl_parser_comparison::CurrentUpstreamAdapterError>(())
//! ```

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

pub mod corpus_walker;
pub mod current_upstream;
pub mod harness;
pub mod outcomes;

pub use corpus_walker::{
    AggregateStats, DisagreementKind, FileRecord, classify, format_report, walk_corpora,
};
pub use current_upstream::{
    CurrentUpstreamAdapter, CurrentUpstreamAdapterError, CurrentUpstreamExecutionDisposition,
    CurrentUpstreamParse, CurrentUpstreamPinError, CurrentUpstreamSubjectIdentity, PACKAGE_CHECKSUM,
    PACKAGE_NAME, PACKAGE_REQUIREMENT, PACKAGE_VERSION, SUBJECT_IDENTITY_TOML,
    TREE_SITTER_LANGUAGE_VERSION, TREE_SITTER_RUNTIME_VERSION, UPSTREAM_COMMIT,
    UPSTREAM_REPOSITORY, UPSTREAM_RUST_VERSION, UPSTREAM_TAG, validate_exact_package_requirement,
};
pub use harness::{ParseResult, parse_v1, parse_v2, parse_v3};
pub use outcomes::Verdict;
