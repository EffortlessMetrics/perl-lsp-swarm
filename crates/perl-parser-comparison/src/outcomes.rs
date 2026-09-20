//! Legacy outcome categories for differential parser testing.
//!
//! [`Verdict`] predates the generic evidence model. Some current adapters still
//! project clean parser acceptance to `Correct` for compatibility, so this enum
//! is not authoritative for new comparison, accuracy, or cleanliness claims.
//! New code uses [`HarnessOutcome`](crate::HarnessOutcome),
//! [`SubjectDisposition`](crate::SubjectDisposition), and an independent
//! [`ScoredComparison`](crate::ScoredComparison).

use std::fmt;

/// Legacy outcome category for a single parser on a single input.
///
/// Existing tests record this value for compatibility. The `Correct` variant
/// is historically overloaded and may mean only that the parser accepted the
/// input without its designated error signal. It must not be used as a new
/// correctness assertion without an independent observer and reviewed
/// expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// Legacy success-shaped value.
    ///
    /// Some old call sites use this for a structurally checked result; others
    /// receive it from clean parser acceptance alone. New comparison code must
    /// not infer correctness from this variant.
    Correct,

    /// Parser accepted the input but the AST is plausibly wrong.
    ///
    /// Example: Pest parses `map { a => $_ }` as a hash-ref rather than a
    /// block—the parse succeeds, but the semantic interpretation is wrong.
    WrongButPlausible,

    /// Parser accepted the input but key content is silently absent.
    ///
    /// Example: Pest accepts `<<A\nbody\nA\n` but the heredoc body is empty.
    SilentlyEmpty,

    /// Legacy parser-error-shaped value.
    ///
    /// This projection may combine rejection, recovery, setup, unsupported,
    /// process, and instrument states. New code must use the generic evidence
    /// axes instead.
    Errors,

    /// Parser or in-process harness panicked and the legacy harness caught it.
    Crashes,
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Correct => write!(formatter, "Correct"),
            Self::WrongButPlausible => write!(formatter, "WrongButPlausible"),
            Self::SilentlyEmpty => write!(formatter, "SilentlyEmpty"),
            Self::Errors => write!(formatter, "Errors"),
            Self::Crashes => write!(formatter, "Crashes"),
        }
    }
}
