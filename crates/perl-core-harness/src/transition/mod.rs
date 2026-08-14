//! Transition classification for compile observations against retained ratchets.

mod classify;
mod model;
mod validate;

pub use classify::{Classification, classify_transition};
pub use model::{AcceptedBaseline, TransitionRunState};
pub use validate::{
    EvidenceValidationError, ValidatedCompileBaselineV2, ValidatedRunReport,
    validate_accepted_baseline, validate_compile_baseline_v2, validate_run_report,
};
