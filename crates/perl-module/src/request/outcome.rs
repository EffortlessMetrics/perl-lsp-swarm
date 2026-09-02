//! Typed module-resolution outcomes.
//!
//! The legacy [`ModuleUriResolution`] enum can only say *resolved*, *not found*,
//! or *timed out*. That collapses materially different states: a request that was
//! never valid, a request that is dynamic, a search whose denominator was never
//! completed, and a genuine complete miss all read as `NotFound`.
//!
//! [`ModuleResolutionOutcome`] keeps those states distinct, and records whether
//! the search denominator was complete enough for the answer to be exact.
//!
//! [`ModuleUriResolution`]: crate::ModuleUriResolution

use std::fmt;

use crate::resolution::ModuleUriResolution;

use super::{ModuleRequestError, RequestBoundary};

/// Outcome of a module-resolution attempt.
///
/// `NotFound` is an *exact* answer and is only correct when every authorized
/// root was inspected. Searches cut short by a budget, an I/O limit, or a
/// missing include environment report their own state instead.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleResolutionOutcome {
    /// An existing module was selected. Carries the winning URI.
    Resolved(String),
    /// The request was valid, the denominator was complete, and nothing matched.
    NotFound,
    /// The request never became a valid lookup subject.
    InvalidRequest(ModuleRequestError),
    /// The request is dynamic or only partially static; no exact lookup ran.
    Dynamic(RequestBoundary),
    /// More than one candidate matched and no rule selects a winner.
    Ambiguous,
    /// A candidate exists but lies outside the authority the caller granted.
    OutsideAuthority,
    /// The include environment needed to answer the request was unavailable.
    EnvironmentUnavailable,
    /// The search budget expired before the denominator was complete.
    TimedOut,
    /// I/O limits stopped the search before the denominator was complete.
    IoLimited,
}

impl ModuleResolutionOutcome {
    /// `true` only for [`Self::Resolved`].
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved(_))
    }

    /// The winning URI, when one was selected.
    #[must_use]
    pub fn resolved_uri(&self) -> Option<&str> {
        match self {
            Self::Resolved(uri) => Some(uri.as_str()),
            _ => None,
        }
    }

    /// Whether the search denominator was complete enough to make this answer exact.
    ///
    /// This is the honesty predicate a caller needs before reporting "this module
    /// does not exist". `Resolved` and `NotFound` are exact; every truncated,
    /// unattempted, or refused state is not.
    #[must_use]
    pub const fn has_complete_denominator(&self) -> bool {
        matches!(self, Self::Resolved(_) | Self::NotFound)
    }

    /// Stable identifier for evidence rows and diagnostics.
    #[must_use]
    pub const fn boundary_id(&self) -> &'static str {
        match self {
            Self::Resolved(_) => "module_resolution.resolved",
            Self::NotFound => "module_resolution.not_found",
            Self::InvalidRequest(_) => "module_resolution.invalid_request",
            Self::Dynamic(_) => "module_resolution.dynamic",
            Self::Ambiguous => "module_resolution.ambiguous",
            Self::OutsideAuthority => "module_resolution.outside_authority",
            Self::EnvironmentUnavailable => "module_resolution.environment_unavailable",
            Self::TimedOut => "module_resolution.timed_out",
            Self::IoLimited => "module_resolution.io_limited",
        }
    }
}

impl fmt::Display for ModuleResolutionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved(uri) => write!(f, "resolved to {uri}"),
            Self::NotFound => f.write_str("not found"),
            Self::InvalidRequest(error) => write!(f, "invalid request: {error}"),
            Self::Dynamic(boundary) => write!(f, "dynamic request: {boundary}"),
            Self::Ambiguous => f.write_str("ambiguous"),
            Self::OutsideAuthority => f.write_str("outside granted authority"),
            Self::EnvironmentUnavailable => f.write_str("include environment unavailable"),
            Self::TimedOut => f.write_str("search budget expired"),
            Self::IoLimited => f.write_str("stopped by I/O limits"),
        }
    }
}

/// Compatibility adapter: widen a legacy [`ModuleUriResolution`] into the typed
/// vocabulary.
///
/// # Caller inventory
///
/// Every current consumer of `resolve_module_uri`,
/// `resolve_module_uri_with_effective_inc`, and `resolve_module_path` — today
/// `perl-lsp-rs` (`runtime::language::{navigation, hover, virtual_content,
/// missing_module_lookup, dancer2}`, `runtime::lifecycle::module_resolution`)
/// and `perl-lsp-ux-tests`.
///
/// # Removal owner
///
/// #8521 (M02), which requires a validated request at the resolution entrypoint
/// and returns [`ModuleResolutionOutcome`] directly.
///
/// # Honesty boundary
///
/// The legacy enum cannot prove the invalid, dynamic, ambiguous,
/// outside-authority, environment-unavailable, or I/O-limited states, so this
/// adapter never manufactures them. It maps only the three states the legacy
/// enum can actually witness.
#[must_use]
pub fn outcome_from_uri_resolution(resolution: &ModuleUriResolution) -> ModuleResolutionOutcome {
    match resolution {
        ModuleUriResolution::Resolved(uri) => ModuleResolutionOutcome::Resolved(uri.clone()),
        ModuleUriResolution::NotFound => ModuleResolutionOutcome::NotFound,
        ModuleUriResolution::TimedOut => ModuleResolutionOutcome::TimedOut,
    }
}

/// Compatibility adapter: narrow a typed outcome back to the legacy enum.
///
/// # Caller inventory
///
/// Call sites that still hand a [`ModuleUriResolution`] to an unmigrated
/// consumer during the M01 → M02 transition.
///
/// # Removal owner
///
/// #8521 (M02), after which no caller needs the three-state enum.
///
/// # Honesty boundary
///
/// Returns `None` for every outcome the legacy enum cannot represent. The
/// adapter refuses rather than collapsing `InvalidRequest`, `Dynamic`,
/// `Ambiguous`, `OutsideAuthority`, `EnvironmentUnavailable`, or `IoLimited`
/// into `NotFound` — silently erasing a classification is exactly the defect
/// this vocabulary exists to prevent.
#[must_use]
pub fn uri_resolution_from_outcome(
    outcome: &ModuleResolutionOutcome,
) -> Option<ModuleUriResolution> {
    match outcome {
        ModuleResolutionOutcome::Resolved(uri) => Some(ModuleUriResolution::Resolved(uri.clone())),
        ModuleResolutionOutcome::NotFound => Some(ModuleUriResolution::NotFound),
        ModuleResolutionOutcome::TimedOut => Some(ModuleUriResolution::TimedOut),
        ModuleResolutionOutcome::InvalidRequest(_)
        | ModuleResolutionOutcome::Dynamic(_)
        | ModuleResolutionOutcome::Ambiguous
        | ModuleResolutionOutcome::OutsideAuthority
        | ModuleResolutionOutcome::EnvironmentUnavailable
        | ModuleResolutionOutcome::IoLimited => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModuleResolutionOutcome, ModuleUriResolution, outcome_from_uri_resolution,
        uri_resolution_from_outcome,
    };
    use crate::request::{ModuleName, ModuleRequestError, RequestBoundary};

    #[test]
    fn legacy_round_trip_preserves_the_three_witnessable_states() {
        let cases = [
            ModuleUriResolution::Resolved("file:///w/Foo/Bar.pm".to_string()),
            ModuleUriResolution::NotFound,
            ModuleUriResolution::TimedOut,
        ];

        for legacy in cases {
            let widened = outcome_from_uri_resolution(&legacy);
            assert_eq!(
                uri_resolution_from_outcome(&widened),
                Some(legacy.clone()),
                "widening then narrowing must be lossless for {legacy:?}"
            );
        }
    }

    #[test]
    fn narrowing_refuses_to_erase_a_classification() {
        let invalid_request = ModuleName::parse("../../etc/passwd")
            .err()
            .map(ModuleRequestError::InvalidModuleName)
            .map(ModuleResolutionOutcome::InvalidRequest);
        assert_eq!(
            invalid_request.as_ref().and_then(uri_resolution_from_outcome),
            None,
            "a traversal request must not narrow into a legacy `NotFound`"
        );
        assert!(invalid_request.is_some(), "traversal never validates");

        let unrepresentable = [
            ModuleResolutionOutcome::Dynamic(RequestBoundary::VariableInterpolation),
            ModuleResolutionOutcome::Ambiguous,
            ModuleResolutionOutcome::OutsideAuthority,
            ModuleResolutionOutcome::EnvironmentUnavailable,
            ModuleResolutionOutcome::IoLimited,
        ];

        for outcome in unrepresentable {
            assert_eq!(
                uri_resolution_from_outcome(&outcome),
                None,
                "{outcome:?} must not narrow into a legacy state it does not mean"
            );
        }
    }

    #[test]
    fn only_exact_answers_claim_a_complete_denominator() {
        assert!(ModuleResolutionOutcome::NotFound.has_complete_denominator());
        assert!(
            ModuleResolutionOutcome::Resolved("file:///w/Foo.pm".to_string())
                .has_complete_denominator()
        );

        for truncated in [
            ModuleResolutionOutcome::TimedOut,
            ModuleResolutionOutcome::IoLimited,
            ModuleResolutionOutcome::EnvironmentUnavailable,
            ModuleResolutionOutcome::Ambiguous,
            ModuleResolutionOutcome::OutsideAuthority,
            ModuleResolutionOutcome::Dynamic(RequestBoundary::RuntimeString),
        ] {
            assert!(
                !truncated.has_complete_denominator(),
                "{truncated:?} must not claim an exact denominator"
            );
        }
    }

    #[test]
    fn boundary_ids_are_namespaced_and_distinct() {
        let outcomes = [
            ModuleResolutionOutcome::Resolved(String::new()),
            ModuleResolutionOutcome::NotFound,
            ModuleResolutionOutcome::Dynamic(RequestBoundary::ComputedExpression),
            ModuleResolutionOutcome::Ambiguous,
            ModuleResolutionOutcome::OutsideAuthority,
            ModuleResolutionOutcome::EnvironmentUnavailable,
            ModuleResolutionOutcome::TimedOut,
            ModuleResolutionOutcome::IoLimited,
        ];

        let ids: Vec<&str> = outcomes.iter().map(ModuleResolutionOutcome::boundary_id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();

        assert_eq!(unique.len(), ids.len(), "each outcome needs its own boundary id");
        assert!(ids.iter().all(|id| id.starts_with("module_resolution.")));
    }
}
