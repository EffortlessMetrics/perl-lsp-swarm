//! Differential parser evidence for current, historical, native, and
//! experimental Perl parser subjects.
//!
//! The comparison model keeps two propositions separate:
//!
//! 1. [`ExecutionDisposition`] records what happened when a subject ran.
//! 2. [`ScoredComparison`] records whether an independent observer matched a
//!    reviewed expectation.
//!
//! A cleanly accepted parse is therefore not a correctness verdict by itself.
//! Existing [`Verdict`] and `parse_v*` APIs remain as explicitly lossy
//! compatibility surfaces while corpus and report consumers migrate.
//!
//! # Subjects
//!
//! | Role | Current implementation |
//! |------|------------------------|
//! | historical Tree-sitter C | `tree-sitter-perl-c` snapshot |
//! | experimental Pest | `perl-parser-pest` |
//! | native recursive descent | `perl-parser-core` |
//! | current upstream Tree-sitter | introduced by the exact-subject train |
//! | native Tree-sitter facade | separate compatibility subject |
//!
//! # Usage
//!
//! ```no_run
//! use perl_parser_comparison::{
//!     ExecutionDisposition, ObservationPlane, ScoredComparison, ScoredOutcome,
//!     execute_v3,
//! };
//!
//! let execution = execute_v3("my $x = 42;");
//! assert_eq!(execution.disposition(), ExecutionDisposition::AcceptedClean);
//!
//! let comparison = ScoredComparison::scored(
//!     "assignment-shape.v1",
//!     ObservationPlane::Structure,
//!     "expected-fingerprint",
//!     "expected-fingerprint",
//!     ScoredOutcome::MatchesExpected,
//!     None,
//! )?;
//! assert_eq!(comparison.outcome(), ScoredOutcome::MatchesExpected);
//! # Ok::<(), perl_parser_comparison::ComparisonModelError>(())
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
pub mod evidence;
pub mod harness;
pub mod outcomes;

pub use corpus_walker::{
    AggregateStats, DisagreementKind, FileRecord, classify, format_report, walk_corpora,
};
pub use evidence::{
    ComparisonModelError, DiagnosticSummary, ExecutionDisposition, InstrumentState,
    ObservationAvailability, ObservationPlane, ScoredComparison, ScoredOutcome, SubjectExecution,
    SubjectRole,
};
pub use harness::{
    ParseResult, ParserLabel, execute_v1, execute_v3, parse_v1, parse_v2, parse_v3,
};
pub use outcomes::Verdict;
