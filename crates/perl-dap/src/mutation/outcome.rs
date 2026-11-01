//! Mutation outcome model (#10736).
//!
//! The load-bearing distinction is **when** the outcome was decided:
//!
//! ```text
//! before dispatch   nothing was written; the debuggee is untouched
//! after dispatch    a write may already have happened
//! ```
//!
//! Collapsing those two is the defect this model exists to prevent. A timeout
//! *before* the command reached the engine is an ordinary refusal; a timeout
//! *after* it did is an unknown that must invalidate the old value authority.
//! A single "error" bucket cannot express that difference, which is why
//! `Result<String, String>` is explicitly not the outcome type.
//!
//! Success is equally constrained: it carries an *observed read-back*, never
//! the value the client requested. Echoing the request would let a backend
//! that silently ignored the write report success.

use std::fmt;

use serde::Serialize;

use super::scalar_value::{MutationValue, MutationValueProfile};
use super::target::RefusedWritability;

/// Explicit claim that a dispatched mutation may already have been applied.
///
/// A plain `bool` here would be forgeable: a caller could build an
/// indeterminate outcome asserting `false` while every accessor on it reported
/// `true`, producing a record that contradicts itself. This type has exactly
/// one value, so the claim is explicit in the data and cannot be negated. The
/// only way to obtain one is
/// [`MutationOutcome::indeterminate_after_dispatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PossibleApplication(());

/// A value observed by reading the target back after a mutation.
///
/// # What this type does not prove
///
/// Constructing one does not verify that the engine really reported this
/// value, that it came from the operation's target, or that it was observed
/// under a current authority. Those checks belong to the read-back
/// interpreter (#10926), which is outside this contract; the fields carry the
/// evidence that interpreter must validate before it may build
/// [`MutationOutcome::SuccessWithObservedReadBack`].
///
/// Constructed from what the engine reported on re-inspection, never from the
/// operation's requested value.
#[derive(Clone, PartialEq)]
pub struct ObservedReadBack {
    /// The value observed in the target after the write.
    pub observed_value: MutationValue,
    /// Storage binding identity that was read back.
    pub observed_binding_identity: String,
    /// Value-authority generation the read-back was observed under.
    pub observed_value_authority_generation: u64,
}

impl fmt::Debug for ObservedReadBack {
    /// Redacted: the observed value is debuggee data, and the binding identity
    /// is a storage spelling. Reports only that a read-back exists and the
    /// authority it was observed under.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservedReadBack")
            .field("observed_value", &self.observed_value)
            .field("observed_binding_identity", &"<redacted>")
            .field("observed_value_authority_generation", &self.observed_value_authority_generation)
            .finish()
    }
}

/// Terminal outcome of one scalar mutation operation.
///
/// Variants are grouped by decision point; see
/// [`MutationOutcome::is_before_dispatch`] and
/// [`MutationOutcome::possible_application`].
#[derive(Debug, Clone, PartialEq)]
pub enum MutationOutcome {
    // ---- decided before dispatch: nothing was written ----
    /// The backend/mode/profile cell does not implement scalar mutation.
    Unsupported {
        /// Backend name that refused.
        backend: String,
        /// Backend mode cell.
        mode: String,
        /// Value profile that was offered.
        profile: MutationValueProfile,
    },
    /// Policy refused the operation.
    PolicyRefused,
    /// The session is not initialized.
    NotInitialized,
    /// The debuggee is not stopped.
    NotStopped,
    /// The session generation moved on.
    StaleSession,
    /// The suspension generation moved on.
    StaleSuspension,
    /// The value authority generation moved on.
    StaleValueAuthority,
    /// The container or member no longer resolves, or resolves elsewhere.
    UnknownOrWrongContainerMember,
    /// The location exists but cannot be written.
    ///
    /// Carries [`RefusedWritability`], which has no `Writable` value, so this
    /// refusal cannot contradict itself.
    ReadOnlyOrUnaddressable(RefusedWritability),
    /// The frame or target cohort is outside the supported table.
    UnsupportedFrameOrTargetCohort,
    /// The value text was refused by the value parser.
    ValueParseRefused,
    /// The deadline elapsed before the command reached the engine.
    TimeoutBeforeDispatch,
    /// The request was cancelled before the command reached the engine.
    CancelledBeforeDispatch,
    /// Transport failed before the command reached the engine.
    TransportFailureBeforeDispatch,

    // ---- decided after dispatch ----
    /// The engine answered, rejecting the write, and proved nothing changed.
    EngineRejectedWithoutMutation,
    /// The command was dispatched and the result is unknown.
    ///
    /// The field states possible application explicitly, and
    /// [`PossibleApplication`] has no `false` value, so this variant cannot be
    /// built claiming the debuggee was left untouched.
    IndeterminateAfterDispatch {
        /// Explicit, unforgeable "a write may have landed" claim.
        possible_application: PossibleApplication,
    },
    /// The write may have landed but no read-back was returned.
    ReadBackMissingAfterPossibleMutation,
    /// The write may have landed but the read-back could not be parsed.
    ReadBackMalformedAfterPossibleMutation,
    /// The read-back described a different location than the target.
    ReadBackTargetMismatch,
    /// The read-back was opaque or hit a resource limit.
    ReadBackOpaqueOrResourceLimited,
    /// The write landed and was confirmed by observing the target.
    SuccessWithObservedReadBack(ObservedReadBack),
}

impl MutationOutcome {
    /// Construct the indeterminate-after-dispatch outcome.
    ///
    /// The only way to obtain a [`PossibleApplication`], so an indeterminate
    /// outcome can never be built claiming the debuggee was left untouched.
    pub fn indeterminate_after_dispatch() -> Self {
        Self::IndeterminateAfterDispatch { possible_application: PossibleApplication(()) }
    }

    /// Whether this outcome was decided before the command reached the engine.
    ///
    /// `true` means the debuggee is provably untouched.
    pub fn is_before_dispatch(&self) -> bool {
        matches!(
            self,
            Self::Unsupported { .. }
                | Self::PolicyRefused
                | Self::NotInitialized
                | Self::NotStopped
                | Self::StaleSession
                | Self::StaleSuspension
                | Self::StaleValueAuthority
                | Self::UnknownOrWrongContainerMember
                | Self::ReadOnlyOrUnaddressable(_)
                | Self::UnsupportedFrameOrTargetCohort
                | Self::ValueParseRefused
                | Self::TimeoutBeforeDispatch
                | Self::CancelledBeforeDispatch
                | Self::TransportFailureBeforeDispatch
        )
    }

    /// Whether a write may already have been applied to the debuggee.
    ///
    /// Every before-dispatch refusal is `false`. `EngineRejectedWithoutMutation`
    /// is also `false`, because the engine answered and proved it. Everything
    /// else after dispatch is `true`, including success.
    pub fn possible_application(&self) -> bool {
        if self.is_before_dispatch() {
            return false;
        }
        !matches!(self, Self::EngineRejectedWithoutMutation)
    }

    /// Whether the old value authority must be invalidated.
    ///
    /// True whenever a write may have landed but no trustworthy replacement
    /// value was observed.
    pub fn invalidates_value_authority(&self) -> bool {
        self.possible_application() && !matches!(self, Self::SuccessWithObservedReadBack(_))
    }

    /// The observed read-back, when the outcome carries one.
    pub fn observed_read_back(&self) -> Option<&ObservedReadBack> {
        match self {
            Self::SuccessWithObservedReadBack(read_back) => Some(read_back),
            _ => None,
        }
    }

    /// Whether this outcome is a confirmed success.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::SuccessWithObservedReadBack(_))
    }

    /// Stable receipt class name for this outcome.
    ///
    /// A closed vocabulary so receipts and logs never carry free text authored
    /// by the debuggee.
    pub fn receipt_class(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported",
            Self::PolicyRefused => "policy_refused",
            Self::NotInitialized => "not_initialized",
            Self::NotStopped => "not_stopped",
            Self::StaleSession => "stale_session",
            Self::StaleSuspension => "stale_suspension",
            Self::StaleValueAuthority => "stale_value_authority",
            Self::UnknownOrWrongContainerMember => "unknown_or_wrong_container_member",
            Self::ReadOnlyOrUnaddressable(_) => "read_only_or_unaddressable",
            Self::UnsupportedFrameOrTargetCohort => "unsupported_frame_or_target_cohort",
            Self::ValueParseRefused => "value_parse_refused",
            Self::TimeoutBeforeDispatch => "timeout_before_dispatch",
            Self::CancelledBeforeDispatch => "cancelled_before_dispatch",
            Self::TransportFailureBeforeDispatch => "transport_failure_before_dispatch",
            Self::EngineRejectedWithoutMutation => "engine_rejected_without_mutation",
            Self::IndeterminateAfterDispatch { .. } => "indeterminate_after_dispatch",
            Self::ReadBackMissingAfterPossibleMutation => {
                "read_back_missing_after_possible_mutation"
            }
            Self::ReadBackMalformedAfterPossibleMutation => {
                "read_back_malformed_after_possible_mutation"
            }
            Self::ReadBackTargetMismatch => "read_back_target_mismatch",
            Self::ReadBackOpaqueOrResourceLimited => "read_back_opaque_or_resource_limited",
            Self::SuccessWithObservedReadBack(_) => "success_with_observed_read_back",
        }
    }

    /// Receipt-safe projection: classification only, never observed data.
    pub fn receipt_projection(&self) -> MutationOutcomeReceipt {
        MutationOutcomeReceipt {
            class: self.receipt_class(),
            before_dispatch: self.is_before_dispatch(),
            possible_application: self.possible_application(),
            invalidates_value_authority: self.invalidates_value_authority(),
            read_back_value: self
                .observed_read_back()
                .map(|r| r.observed_value.receipt_projection()),
            unsupported_cell: match self {
                Self::Unsupported { backend, mode, profile } => Some(UnsupportedCellReceipt {
                    backend: backend.clone(),
                    mode: mode.clone(),
                    profile: *profile,
                }),
                _ => None,
            },
        }
    }
}

/// The exact backend/mode/profile cell that refused as unsupported.
///
/// Backend and mode are this adapter's own identifiers, never debuggee data,
/// so they are receipt-safe. They are retained because the backend seam
/// promises to name the cell that refused, and the raw outcome is
/// deliberately not serializable — without this the durable evidence would
/// say only "unsupported" and lose which capability cell it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsupportedCellReceipt {
    /// Backend name that refused.
    pub backend: String,
    /// Backend mode cell.
    pub mode: String,
    /// Value profile that was offered.
    pub profile: MutationValueProfile,
}

/// Redacted projection of an outcome for receipts and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationOutcomeReceipt {
    /// Closed-vocabulary outcome class.
    pub class: &'static str,
    /// Whether the outcome was decided before dispatch.
    pub before_dispatch: bool,
    /// Whether a write may already have been applied.
    pub possible_application: bool,
    /// Whether the old value authority must be invalidated.
    pub invalidates_value_authority: bool,
    /// Redacted projection of the observed read-back value, when any.
    pub read_back_value: Option<super::scalar_value::MutationValueReceipt>,
    /// The exact cell that refused, populated only for
    /// [`MutationOutcome::Unsupported`].
    pub unsupported_cell: Option<UnsupportedCellReceipt>,
}
