//! Transition classification for compile observations against retained ratchets.

mod classify;
mod model;
mod validation;

pub use classify::{
    Classification, classify_transition, classify_transition_with_context,
    classify_validated_transition,
};
pub use model::{AcceptedBaseline, TransitionRunState};
pub use validation::{
    COMPILER_COMPARISON_CONTEXT_SCHEMA_VERSION, CompilerComparisonContext, EvidenceSubject,
    EvidenceValidationError, EvidenceValidationKind, ValidatedAcceptedBaseline,
    ValidatedComparison, ValidatedRunReport, run_report_digest, validate_accepted_baseline,
    validate_comparison, validate_run_report,
};
