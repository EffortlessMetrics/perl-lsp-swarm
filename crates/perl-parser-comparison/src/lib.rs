//! Differential parser evidence for exact current, historical, native, and
//! experimental Perl parser subjects.
//!
//! The comparison model keeps five propositions separate:
//!
//! 1. [`HarnessOutcome`] records whether trustworthy subject output returned.
//! 2. [`SubjectDisposition`] records what a completed parser subject reported.
//! 3. [`InstrumentState`] records whether required evidence is complete.
//! 4. [`ObservationDisposition`] records whether one plane is observable.
//! 5. [`ScoredComparison`] records reviewed conformance for an exact observer
//!    and expectation.
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
//! | current upstream Tree-sitter | `ts-parser-perl` via `current_upstream` |
//! | native Tree-sitter facade | separate compatibility subject |
//!
//! # Grammar isolation
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
//!
//! # Usage
//!
//! ```no_run
//! use perl_parser_comparison::{
//!     ConformanceOutcome, ObserverId, ObservationPlane, ReviewedExpectationId,
//!     ScoredComparison, SemanticFingerprint, SubjectDisposition, execute_v3,
//! };
//!
//! let execution = execute_v3("my $x = 42;")?;
//! assert_eq!(
//!     execution.subject_disposition(),
//!     Some(&SubjectDisposition::AcceptedClean),
//! );
//!
//! let comparison = ScoredComparison::matches_expected(
//!     &execution,
//!     ObserverId::new("assignment-shape.v1")?,
//!     ReviewedExpectationId::new("assignment-shape.expected.v1")?,
//!     ObservationPlane::Structure,
//!     SemanticFingerprint::new("assignment(variable,integer)")?,
//!     SemanticFingerprint::new("assignment(variable,integer)")?,
//! )?;
//! assert_eq!(comparison.outcome(), ConformanceOutcome::MatchesExpected);
//! # Ok::<(), Box<dyn std::error::Error>>(())
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

#[cfg(feature = "historical")]
pub mod corpus_walker;
#[cfg(feature = "current-upstream")]
pub mod current_upstream;
pub mod evidence;
pub mod evidence_payload;
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
pub use evidence::{
    BoundedText, ComparisonModelError, ConformanceOutcome, DiagnosticSummary, DivergencePath,
    EvidenceValueError, HarnessFailure, HarnessOutcome, InstrumentState, MismatchClass,
    MismatchDetail, NonDecisiveOutcome, ObservationDisposition, ObservationPlane, ObserverId,
    ReviewedExpectationId, ScoredComparison, SemanticFingerprint, StableId, SubjectDisposition,
    SubjectExecution, SubjectRole,
};
pub use evidence_payload::{
    AttachmentPrivacy, BoundedAttachment, EvidenceKind, EvidencePayloadError, EvidenceRef,
    ObligationRef, ObserverManifestRef, SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION,
    SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION, SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION,
    SemanticDigest, SourceCaseRef, SubjectConformanceEvidence, SubjectExecutionEvidence,
    SubjectManifestRef, SubjectObservationEvidence, parser_comparison_evidence_schema_json,
};
#[cfg(feature = "historical")]
pub use harness::{ParseResult, ParserLabel, execute_v1, execute_v3, parse_v1, parse_v2, parse_v3};
#[cfg(feature = "historical")]
pub use outcomes::Verdict;
