//! Mutation operation identity (#10736).
//!
//! `setVariable` and `setExpression` are different admission origins that
//! converge on one lower operation. Keeping the origin on the operation is
//! what lets later evidence say which frontend request produced a write
//! without maintaining two parallel lower paths.

use serde::Serialize;

use super::scalar_value::{MutationValue, MutationValueProfile};
use super::target::MutationTarget;

/// Which frontend request admitted this operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationOrigin {
    /// DAP `setVariable`.
    SetVariable,
    /// DAP `setExpression`.
    SetExpression,
}

/// Response rendering options requested by the client.
///
/// Held beside the operation and never inside [`MutationValue`]: a display
/// format is a rendering request for the *response*, and letting it reach the
/// assigned data is how a debugger ends up writing what it meant to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ResponseValueFormat {
    /// Client asked for hexadecimal rendering in the response.
    pub hex: bool,
}

/// Deadline and cancellation identity for one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct MutationDeadline {
    /// Correlation identity of the originating request.
    pub request_id: u64,
    /// Deadline in milliseconds since operation admission, when bounded.
    pub deadline_millis: Option<u64>,
    /// Cancellation identity the frontend may signal on.
    pub cancellation_id: Option<u64>,
}

/// One admitted scalar mutation operation.
///
/// Sealed: [`MutationOperation::new`] is the only constructor, and it takes an
/// already-bound [`MutationTarget`] and an already-admitted [`MutationValue`],
/// so an operation cannot exist for an unbound target or unparsed text.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationOperation {
    operation_id: u64,
    origin: MutationOrigin,
    backend_mode: String,
    expected_session_generation: u64,
    expected_suspension_generation: u64,
    expected_value_authority_generation: u64,
    target: MutationTarget,
    value: MutationValue,
    value_profile: MutationValueProfile,
    deadline: MutationDeadline,
    response_format: ResponseValueFormat,
}

impl MutationOperation {
    /// Build an operation from a bound target and an admitted scalar value.
    ///
    /// Every expected generation — session, suspension, and value authority —
    /// is read from the target's own location provenance rather than accepted
    /// as a separate argument, so an operation cannot claim authority the
    /// target was never bound under.
    pub fn new(
        operation_id: u64,
        origin: MutationOrigin,
        target: MutationTarget,
        value: MutationValue,
        deadline: MutationDeadline,
        response_format: ResponseValueFormat,
    ) -> Self {
        let expected_session_generation = target.location().session_generation();
        let expected_suspension_generation = target.location().suspension_generation();
        let expected_value_authority_generation = target.location().value_authority_generation();
        let backend_mode = target.backend_mode().to_string();
        let value_profile = value.profile();
        Self {
            operation_id,
            origin,
            backend_mode,
            expected_session_generation,
            expected_suspension_generation,
            expected_value_authority_generation,
            target,
            value,
            value_profile,
            deadline,
            response_format,
        }
    }

    /// Correlation identity of this operation.
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }

    /// Frontend origin that admitted this operation.
    pub fn origin(&self) -> MutationOrigin {
        self.origin
    }

    /// Backend/mode cell this operation runs against.
    pub fn backend_mode(&self) -> &str {
        &self.backend_mode
    }

    /// Session generation this operation expects to still be current.
    pub fn expected_session_generation(&self) -> u64 {
        self.expected_session_generation
    }

    /// Suspension generation this operation expects to still be current.
    pub fn expected_suspension_generation(&self) -> u64 {
        self.expected_suspension_generation
    }

    /// Value authority generation this operation expects to still be current.
    pub fn expected_value_authority_generation(&self) -> u64 {
        self.expected_value_authority_generation
    }

    /// The bound writable subject.
    pub fn target(&self) -> &MutationTarget {
        &self.target
    }

    /// The admitted typed value.
    pub fn value(&self) -> &MutationValue {
        &self.value
    }

    /// Profile that admitted the value.
    pub fn value_profile(&self) -> MutationValueProfile {
        self.value_profile
    }

    /// Deadline and cancellation identity.
    pub fn deadline(&self) -> MutationDeadline {
        self.deadline
    }

    /// Requested response rendering. Never part of the assigned data.
    pub fn response_format(&self) -> ResponseValueFormat {
        self.response_format
    }

    /// Receipt-safe projection of the whole operation.
    pub fn receipt_projection(&self) -> MutationOperationReceipt {
        MutationOperationReceipt {
            operation_id: self.operation_id,
            origin: self.origin,
            value: self.value.receipt_projection(),
            target: self.target.receipt_projection(),
            value_profile: self.value_profile,
        }
    }
}

/// Redacted projection of an operation for receipts and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationOperationReceipt {
    /// Correlation identity.
    pub operation_id: u64,
    /// Frontend origin.
    pub origin: MutationOrigin,
    /// Redacted value projection.
    pub value: super::scalar_value::MutationValueReceipt,
    /// Redacted target projection.
    pub target: super::target::MutationTargetReceipt,
    /// Admitting value profile.
    pub value_profile: MutationValueProfile,
}
