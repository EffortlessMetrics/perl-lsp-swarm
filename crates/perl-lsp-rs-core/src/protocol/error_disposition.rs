//! Canonical disposition mapping for ErrorCategory (#4981).
//!
//! Maps each [`ErrorCategory`] to an actionable disposition that consumers
//! (retry logic, user notifications, receipt layers, logging) reference
//! instead of reinventing prose routing for each category.
//!
//! This module is the single source of truth for "what should the consumer
//! DO when they see this category?" — the category itself is evidence, not
//! an action.

use perl_parser_core::ErrorCategory;

/// Recommended disposition for handling an error of a given category.
///
/// Each variant maps to a concrete consumer action:
/// - [`Disposition::Retry`] — the operation may succeed if retried (with optional backoff)
/// - [`Disposition::Repair`] — our code has a bug that needs fixing
/// - [`Disposition::NotifyUser`] — the user needs to correct their input or configuration
/// - [`Disposition::NotifyInfra`] — infrastructure is unavailable (missing tool, network)
/// - [`Disposition::Reject`] — the other side violated a protocol; reject and log
/// - [`Disposition::Cap`] — a resource limit was hit; degrade gracefully
/// - [`Disposition::Log`] — informational; no action needed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// The operation may succeed if retried. Apply exponential backoff
    /// for repeated Transient errors.
    Retry,
    /// Our code has a bug. Surface in development logs, file an issue,
    /// but do not retry — the same input will produce the same bug.
    Repair,
    /// The user's input or configuration is invalid. Show a user-facing
    /// diagnostic with a suggested correction.
    NotifyUser,
    /// An external dependency or infrastructure is unavailable. Log
    /// prominently and degrade gracefully (e.g., fall back to a
    /// simpler provider).
    NotifyInfra,
    /// The other side violated a protocol or format contract. Reject
    /// the request and log the violation.
    Reject,
    /// A configured safety limit was exceeded. Degrade gracefully
    /// (e.g., skip the feature rather than crash).
    Cap,
    /// Informational only. No action needed beyond normal logging.
    Log,
}

impl Disposition {
    /// Returns a stable machine-readable token for this disposition.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Repair => "repair",
            Self::NotifyUser => "notify_user",
            Self::NotifyInfra => "notify_infra",
            Self::Reject => "reject",
            Self::Cap => "cap",
            Self::Log => "log",
        }
    }

    /// Returns true if the consumer should retry the operation.
    #[must_use]
    pub const fn should_retry(self) -> bool {
        matches!(self, Self::Retry)
    }

    /// Returns true if this disposition requires user-facing communication.
    #[must_use]
    pub const fn is_user_visible(self) -> bool {
        matches!(self, Self::NotifyUser | Self::NotifyInfra)
    }
}

/// Map an [`ErrorCategory`] to its canonical [`Disposition`].
///
/// This is the single source of truth for category → action routing.
/// Consumers should call this function rather than matching on the
/// category directly, so that future category additions or disposition
/// changes are centralized here (#4981).
#[must_use]
pub fn disposition_for(category: ErrorCategory) -> Disposition {
    match category {
        ErrorCategory::Advisory => Disposition::Log,
        ErrorCategory::UserError => Disposition::NotifyUser,
        ErrorCategory::Bug => Disposition::Repair,
        ErrorCategory::Infra => Disposition::NotifyInfra,
        ErrorCategory::Transient => Disposition::Retry,
        ErrorCategory::Protocol => Disposition::Reject,
        ErrorCategory::ResourceLimit => Disposition::Cap,
        // ErrorCategory is #[non_exhaustive] — future variants default to
        // Repair so new categories surface as visible bugs until explicitly
        // mapped, rather than silently being treated as safe-to-ignore.
        _ => Disposition::Repair,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_logs() {
        assert_eq!(disposition_for(ErrorCategory::Advisory), Disposition::Log);
    }

    #[test]
    fn user_error_notifies_user() {
        assert_eq!(disposition_for(ErrorCategory::UserError), Disposition::NotifyUser);
        assert!(Disposition::NotifyUser.is_user_visible());
    }

    #[test]
    fn bug_needs_repair() {
        assert_eq!(disposition_for(ErrorCategory::Bug), Disposition::Repair);
        assert!(!Disposition::Repair.should_retry());
    }

    #[test]
    fn infra_notifies_infra() {
        assert_eq!(disposition_for(ErrorCategory::Infra), Disposition::NotifyInfra);
        assert!(Disposition::NotifyInfra.is_user_visible());
    }

    #[test]
    fn transient_should_retry() {
        assert_eq!(disposition_for(ErrorCategory::Transient), Disposition::Retry);
        assert!(Disposition::Retry.should_retry());
    }

    #[test]
    fn protocol_rejects() {
        assert_eq!(disposition_for(ErrorCategory::Protocol), Disposition::Reject);
    }

    #[test]
    fn resource_limit_caps() {
        assert_eq!(disposition_for(ErrorCategory::ResourceLimit), Disposition::Cap);
    }

    #[test]
    fn disposition_tokens_are_stable() {
        assert_eq!(Disposition::Retry.as_str(), "retry");
        assert_eq!(Disposition::Repair.as_str(), "repair");
        assert_eq!(Disposition::NotifyUser.as_str(), "notify_user");
        assert_eq!(Disposition::NotifyInfra.as_str(), "notify_infra");
        assert_eq!(Disposition::Reject.as_str(), "reject");
        assert_eq!(Disposition::Cap.as_str(), "cap");
        assert_eq!(Disposition::Log.as_str(), "log");
    }
}
