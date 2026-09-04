//! Perl-application projection of [`CancellationError`] onto the Perl error
//! taxonomy (#13997).
//!
//! [`CancellationError`] states cancellation *mechanism* facts: a lock was not
//! acquired, a request was rejected as malformed, a provider was not
//! registered, an operation exceeded its budget. Those facts are
//! language-neutral and belong to the runtime.
//!
//! [`ErrorCategory`] is this repository's *operational judgment* about what a
//! Perl-product consumer should do next. Implementing
//! `perl_parser_core::ErrorClass` directly on [`CancellationError`] bound the
//! runtime type to that judgment, which is the dependency #7611 must cut.
//!
//! This module is the one Perl-owned adapter that keeps the historical mapping
//! reachable for product logging and routing without binding the runtime type
//! to the taxonomy. Callers that need a category for a cancellation failure
//! call [`cancellation_error_category`]; nothing else may re-derive it.
//!
//! # Convergence and removal
//!
//! The mapping below is preserved unchanged from the removed
//! `impl ErrorClass for CancellationError` so that this PR moves ownership
//! without moving behavior. It is explicitly **not** the final request-terminal
//! model:
//!
//! - `InvalidRequest("Request was cancelled")` is what the `check_cancellation!`
//!   macro returns, yet it maps to [`ErrorCategory::Protocol`] — a cancelled
//!   request is not a client protocol violation. #9648 and #7103 own that
//!   semantic correction.
//! - #7612 owns retiring this adapter once the Perl taxonomy has a single
//!   application-side owner.
//!
//! Until then this file is the only editable category map for
//! [`CancellationError`]. Do not add a second one, and do not infer a category
//! from the error's `Display` text.

use crate::protocol::error_disposition::{Disposition, disposition_for};
use crate::runtime::cancellation::CancellationError;
use perl_parser_core::ErrorCategory;

/// Canonical identity of this adapter.
///
/// The #4982 error inventory names the adapter that classifies
/// [`CancellationError`]. It references this constant rather than repeating the
/// path, so the inventory row and the adapter cannot drift into two separately
/// editable statements of the same fact.
pub const CANCELLATION_ERROR_ADAPTER: &str =
    "perl_lsp_rs_core::protocol::cancellation_error_category";

/// Projects a [`CancellationError`] onto the Perl product's [`ErrorCategory`].
///
/// The match is exhaustive over [`CancellationError`] on purpose: a new
/// cancellation variant must fail this crate's build and force an explicit
/// operational decision rather than silently inheriting a neighbouring
/// category.
#[must_use]
pub fn cancellation_error_category(error: &CancellationError) -> ErrorCategory {
    match error {
        // Internal lock or routing failures — our bug.
        CancellationError::LockError(_) | CancellationError::ProviderNotFound(_) => {
            ErrorCategory::Bug
        }
        // Malformed request from the client.
        CancellationError::InvalidRequest(_) => ErrorCategory::Protocol,
        // Operation may succeed if retried.
        CancellationError::Timeout(_) => ErrorCategory::Transient,
    }
}

/// Canonical consumer action for a [`CancellationError`].
///
/// Composes [`cancellation_error_category`] with the canonical
/// [`disposition_for`] mapping so consumers never pair a cancellation error
/// with a hand-rolled disposition.
#[must_use]
pub fn cancellation_error_disposition(error: &CancellationError) -> Disposition {
    disposition_for(cancellation_error_category(error))
}
