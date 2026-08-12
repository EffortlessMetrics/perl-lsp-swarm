//! Transition classification for compile observations against retained ratchets.

mod classify;
mod model;
#[allow(clippy::too_many_arguments)]
mod validation;

pub use classify::{Classification, classify_transition, classify_validated_transition};
pub use model::{AcceptedBaseline, TransitionRunState};
pub use validation::{
    EvidenceSubject, EvidenceValidationError, EvidenceValidationKind,
    ValidatedAcceptedBaseline, ValidatedRunReport, validate_accepted_baseline,
    validate_run_report,
};
