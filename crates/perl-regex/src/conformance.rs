//! Versioned regex-analysis conformance matrix vocabulary for `perl-regex`.
//!
//! This module defines stable public types that form the vocabulary for the
//! modifier/capture/reference conformance matrix described in issue #7036.
//! The machine-readable fixture files live under
//! `tests/fixtures/conformance/` as versioned JSON documents.
//! Integration tests in `tests/conformance_matrix_tests.rs` load each
//! fixture and verify that the expected typed facts match actual
//! [`crate::RegexAnalyzer`] output.
//!
//! # Schema version
//!
//! [`SCHEMA_VERSION`] guards against silent meaning drift.  An incompatible
//! change to field semantics requires incrementing the version and adding a
//! migration test that rejects the old version.
//!
//! # Vocabulary
//!
//! Each [`ConformanceCompleteness`] value states what the analysis can claim
//! about a concept.  [`OracleDisposition`] records whether a real-Perl
//! compile probe was run, is available to run, or is not applicable.

/// Current conformance-matrix schema version.
///
/// Increment this when any field meaning changes incompatibly.
/// Tests that load fixture files compare `schema_version` from the file
/// against this constant and fail if they differ.
pub const SCHEMA_VERSION: u32 = 1;

/// Claimed support boundary for a conformance concept.
///
/// `Unknown` is intentionally absent: every concept must carry an explicit
/// boundary.  Use [`Partial`](Self::Partial) when the boundary is
/// acknowledged but not yet fully characterised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConformanceCompleteness {
    /// The analyzer fully and correctly handles this concept.
    Proven,
    /// The analyzer is partially correct; known gaps are tracked.
    Partial,
    /// This row marks an explicit static-analysis boundary; the construct
    /// is recognised but deliberately not analyzed beyond this point.
    Boundary,
    /// The concept is intentionally outside the scope of this crate.
    Unsupported,
}

impl ConformanceCompleteness {
    /// Stable machine token for receipts and protocol adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Partial => "partial",
            Self::Boundary => "boundary",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Real-Perl compile-probe disposition for a conformance row.
///
/// A real-Perl probe validates that a modifier sequence or pattern is
/// accepted/rejected by `perl -c`.  For purely static analyzer claims
/// (extended-mode derivation, diagnostic count) the probe is
/// [`NotApplicable`](Self::NotApplicable).  Where the probe was not run
/// because no Perl runtime is available, use [`Unavailable`](Self::Unavailable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OracleDisposition {
    /// A real-Perl compile probe was or should be run for this concept.
    Required,
    /// A Perl runtime is unavailable in this environment.
    Unavailable,
    /// A real-Perl compile probe is not applicable for this purely static
    /// concept (e.g. the expected `ExtendedMode` value from modifier analysis).
    NotApplicable,
}

impl OracleDisposition {
    /// Stable machine token for receipts and protocol adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Unavailable => "unavailable",
            Self::NotApplicable => "not_applicable",
        }
    }
}
