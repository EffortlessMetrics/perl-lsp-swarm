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

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::resolution::ModuleUriResolution;

use super::{ModuleRequest, ModuleRequestError, ModuleRequestKind, RequestBoundary};

/// Whether a request is an exact lookup subject.
///
/// Only a bareword module or a literal relative file names something a search
/// can be complete *about*. A partially static or dynamic operand has no exact
/// subject, so no search over it can have a complete denominator — which is why
/// the exact-outcome constructors refuse one.
fn is_exact_lookup_subject(request: &ModuleRequest) -> bool {
    matches!(
        request.kind(),
        ModuleRequestKind::BarewordModule | ModuleRequestKind::LiteralRelativeFile
    )
}

/// Evidence that a complete search selected a module.
///
/// The fields and constructors are private. An exact outcome is a resolver
/// result, not a value an API consumer may mint from a URI. M02 (#8521) will
/// provide the resolver-side construction path.
///
/// This is a *separate type* from [`AbsenceEvidence`] on purpose. When both
/// exact variants carried one interchangeable payload, safe downstream code
/// could match `Resolved(e)` and rewrap it as `NotFound(e)` — relabelling a
/// module that was found as a proven absence, which
/// [`ModuleResolutionOutcome::has_complete_denominator`], `Display`, and
/// [`uri_resolution_from_outcome`] would all then believe, because they trust
/// the outer variant. Distinct types make that rewrap a compile error rather
/// than a convention.
///
/// It also holds `selected_uri` as a plain `String` rather than an `Option`:
/// a resolution that selected nothing is not a resolution.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedEvidence {
    request: ModuleRequest,
    selected_uri: String,
}

impl ResolvedEvidence {
    /// The validated request whose complete search produced this evidence.
    #[must_use]
    pub fn request(&self) -> &ModuleRequest {
        &self.request
    }

    /// The URI the complete search selected.
    #[must_use]
    pub fn selected_uri(&self) -> &str {
        &self.selected_uri
    }
}

/// Evidence that a complete search found nothing.
///
/// Carries no URI at all, so it cannot be rewrapped into a resolution: there is
/// no winner to invent. See [`ResolvedEvidence`] for why the two are distinct
/// types.
#[derive(Clone, PartialEq, Eq)]
pub struct AbsenceEvidence {
    request: ModuleRequest,
}

impl AbsenceEvidence {
    /// The validated request whose complete search produced this evidence.
    #[must_use]
    pub fn request(&self) -> &ModuleRequest {
        &self.request
    }
}

/// Outcome of a module-resolution attempt.
///
/// `NotFound` is an *exact* answer and is only correct when every authorized
/// root was inspected. Searches cut short by a budget, an I/O limit, or a
/// missing include environment report their own state instead.
///
/// Exact variants carry opaque resolver evidence and cannot be minted from an
/// arbitrary URI or unit value:
///
/// ```compile_fail
/// use perl_module::ModuleResolutionOutcome;
///
/// let _ = ModuleResolutionOutcome::Resolved("file:///does-not-exist.pm".to_owned());
/// let _ = ModuleResolutionOutcome::NotFound;
/// ```
///
/// That case pins the *shape*: it fails because the variants no longer take a
/// `String` or a unit. It would keep passing even if the evidence types were
/// fully public, so on its own it does not prove the guarantee. This one does —
/// it assembles the evidence the current shape actually asks for, and fails
/// only because the fields are private:
///
/// ```compile_fail
/// use perl_module::{ModuleRequest, ModuleResolutionOutcome, ResolvedEvidence};
///
/// let Ok(request) = ModuleRequest::bareword("Foo::Bar") else { return };
/// let forged = ResolvedEvidence { request, selected_uri: "file:///nope.pm".to_string() };
/// let _ = ModuleResolutionOutcome::Resolved(forged);
/// ```
///
/// Evidence cannot be *relabelled* either. `Resolved` and `NotFound` take
/// different types, so a consumer holding a real resolution cannot rewrap it as
/// a proven absence — which would otherwise turn a module that was found into
/// one proven missing, and `has_complete_denominator`, `Display` and
/// [`uri_resolution_from_outcome`] would all believe it:
///
/// ```compile_fail
/// use perl_module::ModuleResolutionOutcome;
///
/// fn relabel(outcome: ModuleResolutionOutcome) -> ModuleResolutionOutcome {
///     match outcome {
///         ModuleResolutionOutcome::Resolved(evidence) => {
///             ModuleResolutionOutcome::NotFound(evidence)
///         }
///         other => other,
///     }
/// }
/// ```
///
/// Reading one is unaffected — a consumer still matches and navigates:
///
/// ```
/// use perl_module::{ModuleUriResolution, outcome_from_uri_resolution};
///
/// let outcome = outcome_from_uri_resolution(&ModuleUriResolution::Resolved(
///     "file:///w/Foo.pm".to_string(),
/// ));
/// assert_eq!(outcome.resolved_uri(), Some("file:///w/Foo.pm"));
/// assert!(outcome.is_resolved());
/// // ...but the adapter cannot establish exactness, so it does not claim it.
/// assert!(!outcome.has_complete_denominator());
/// ```
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum ModuleResolutionOutcome {
    /// An existing module was selected. Carries the winning URI.
    ///
    /// This is an *exact* selection: only a search that can prove it inspected
    /// every root of higher precedence may report it, because the winner is
    /// defined by precedence order rather than by mere existence.
    Resolved(ResolvedEvidence),
    /// A module was found, but the search was never proven to have inspected
    /// every higher-precedence root, so this may not be the precedence winner.
    ///
    /// The resolver orders include roots by precedence and then silently skips
    /// any whose joined path fails workspace-boundary validation
    /// (`full_path_for_root` returns `None` and the traversal `continue`s), and
    /// a probe that fails with an I/O error is indistinguishable from a root
    /// that simply does not hold the module. Either way an earlier root can go
    /// uninspected while a later one supplies the answer.
    ///
    /// The URI is still the module the legacy resolver would have opened, so
    /// navigation stays correct; what is not proven is that Perl would load
    /// *this* file rather than one behind the skipped root.
    NotProvenPrecedence(String),
    /// The request was valid, the denominator was complete, and nothing matched.
    ///
    /// This is an *exact* absence. Only a search that can prove it inspected every
    /// authorized root may report it.
    NotFound(AbsenceEvidence),
    /// Nothing matched, but the search denominator was never proven complete.
    ///
    /// This is the strongest claim a three-state legacy result supports. The
    /// current resolver skips any include root whose joined path fails
    /// workspace-boundary validation, and treats an I/O error from a filesystem
    /// probe as "absent"; neither is recorded, so a miss it reports may have left
    /// roots uninspected.
    NotProvenAbsent,
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
    /// Create an exact resolved outcome from resolver-owned evidence.
    ///
    /// This remains crate-private until the M02 resolver owns this boundary.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the M02 seam (#8521): the resolver does not yet consume `ModuleRequest`, so \
                      nothing mints exact evidence on a production path. Kept private and \
                      unused rather than shipping an exact outcome no search established. \
                      `expect` rather than `allow` so it warns and is removed the moment \
                      #8521 calls it; scoped to non-test builds because the in-crate tests \
                      do exercise it."
        )
    )]
    pub(crate) fn resolved(request: ModuleRequest, uri: String) -> Option<Self> {
        is_exact_lookup_subject(&request)
            .then_some(Self::Resolved(ResolvedEvidence { request, selected_uri: uri }))
    }

    /// Create an exact absence outcome from resolver-owned evidence.
    ///
    /// This remains crate-private until the M02 resolver owns this boundary.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the M02 seam (#8521): the resolver does not yet consume `ModuleRequest`, so \
                      nothing mints exact evidence on a production path. Kept private and \
                      unused rather than shipping an exact outcome no search established. \
                      `expect` rather than `allow` so it warns and is removed the moment \
                      #8521 calls it; scoped to non-test builds because the in-crate tests \
                      do exercise it."
        )
    )]
    pub(crate) fn not_found(request: ModuleRequest) -> Option<Self> {
        is_exact_lookup_subject(&request).then_some(Self::NotFound(AbsenceEvidence { request }))
    }

    /// `true` when a module was found, whether or not the winner is proven exact.
    ///
    /// Both [`Self::Resolved`] and [`Self::NotProvenPrecedence`] carry a URI a
    /// consumer can open, so navigation must not turn on this distinction. Use
    /// [`Self::has_complete_denominator`] to tell the exact selection from the
    /// unproven one.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved(_) | Self::NotProvenPrecedence(_))
    }

    /// The winning URI, when one was selected.
    #[must_use]
    pub fn resolved_uri(&self) -> Option<&str> {
        match self {
            Self::Resolved(evidence) => Some(evidence.selected_uri()),
            Self::NotProvenPrecedence(uri) => Some(uri.as_str()),
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
        matches!(self, Self::Resolved(_) | Self::NotFound(_))
    }

    /// Stable identifier for the *outcome class*, for evidence rows and diagnostics.
    ///
    /// This names which of the nine states was reached, not why the underlying
    /// request was refused. Use [`Self::cause_boundary_id`] for the nested
    /// classification an `InvalidRequest` or `Dynamic` outcome carries.
    #[must_use]
    pub const fn boundary_id(&self) -> &'static str {
        match self {
            Self::Resolved(_) => "module_resolution.resolved",
            Self::NotProvenPrecedence(_) => "module_resolution.not_proven_precedence",
            Self::NotFound(_) => "module_resolution.not_found",
            Self::NotProvenAbsent => "module_resolution.not_proven_absent",
            Self::InvalidRequest(_) => "module_resolution.invalid_request",
            Self::Dynamic(_) => "module_resolution.dynamic",
            Self::Ambiguous => "module_resolution.ambiguous",
            Self::OutsideAuthority => "module_resolution.outside_authority",
            Self::EnvironmentUnavailable => "module_resolution.environment_unavailable",
            Self::TimedOut => "module_resolution.timed_out",
            Self::IoLimited => "module_resolution.io_limited",
        }
    }

    /// The nested classification this outcome carries, when it carries one.
    ///
    /// `InvalidRequest` and `Dynamic` wrap a more specific boundary than the
    /// outcome class alone. Reporting only [`Self::boundary_id`] for them would
    /// flatten "this was never a valid module name because it contains a path
    /// separator" down to "invalid request" — the same erasure this vocabulary
    /// exists to prevent — so the nested id is reachable without pattern
    /// matching. Outcomes with no nested cause return `None`.
    #[must_use]
    pub const fn cause_boundary_id(&self) -> Option<&'static str> {
        match self {
            Self::InvalidRequest(error) => Some(error.boundary_id()),
            Self::Dynamic(boundary) => Some(boundary.boundary_id()),
            _ => None,
        }
    }
}

impl fmt::Display for ModuleResolutionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved(_) => f.write_str("resolved <redacted>"),
            Self::NotProvenPrecedence(_) => f.write_str(
                "found <redacted>, but higher-precedence roots were not proven inspected",
            ),
            Self::NotFound(_) => f.write_str("not found"),
            Self::NotProvenAbsent => {
                f.write_str("no candidate matched, but the denominator was not proven complete")
            }
            Self::InvalidRequest(error) => write!(f, "invalid request ({})", error.boundary_id()),
            Self::Dynamic(boundary) => write!(f, "dynamic request: {boundary}"),
            Self::Ambiguous => f.write_str("ambiguous"),
            Self::OutsideAuthority => f.write_str("outside granted authority"),
            Self::EnvironmentUnavailable => f.write_str("include environment unavailable"),
            Self::TimedOut => f.write_str("search budget expired"),
            Self::IoLimited => f.write_str("stopped by I/O limits"),
        }
    }
}

impl fmt::Debug for ResolvedEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedEvidence")
            .field("request", &self.request)
            .field("selected_uri", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for AbsenceEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbsenceEvidence").field("request", &self.request).finish()
    }
}

impl fmt::Debug for ModuleResolutionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ModuleResolutionOutcome");
        debug.field("kind", &self.boundary_id());
        if let Some(cause) = self.cause_boundary_id() {
            debug.field("cause", &cause);
        }
        debug.finish()
    }
}

impl Serialize for ResolvedEvidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ResolvedEvidence", 2)?;
        state.serialize_field("kind", "resolved")?;
        state.serialize_field("request", &self.request)?;
        state.end()
    }
}

impl Serialize for AbsenceEvidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AbsenceEvidence", 2)?;
        state.serialize_field("kind", "not_found")?;
        state.serialize_field("request", &self.request)?;
        state.end()
    }
}

impl Serialize for ModuleResolutionOutcome {
    /// Serialize the outcome class and nested classification without exposing
    /// source expressions, validated file names, or selected physical URIs.
    /// This is intentionally one-way; exact evidence remains resolver-owned.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ModuleResolutionOutcome", 2)?;
        state.serialize_field("kind", self.boundary_id())?;
        state.serialize_field("cause", &self.cause_boundary_id())?;
        state.end()
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
/// adapter never manufactures them.
///
/// It also refuses to manufacture either *exact* answer, because the legacy enum
/// carries no completeness signal in either direction.
///
/// A legacy `NotFound` widens to [`ModuleResolutionOutcome::NotProvenAbsent`],
/// never to [`ModuleResolutionOutcome::NotFound`]: the resolver behind that enum
/// skips any include root whose joined path fails workspace-boundary validation
/// (`full_path_for_root` returns `None` and the traversal `continue`s) and cannot
/// tell a filesystem probe's I/O error from a genuine absence. Neither leaves a
/// completeness signal, so the miss it reports is not a proven absence. Widening
/// it to the exact state would let a consumer report "this module does not exist"
/// on evidence that never established it.
///
/// A legacy `Resolved` widens to
/// [`ModuleResolutionOutcome::NotProvenPrecedence`] for the *same* reason applied
/// to the winning side. The resolver sorts roots by precedence and then skips
/// them under exactly the conditions above, so a match returned from a later root
/// is not provably the winner: an earlier root may have been skipped rather than
/// searched and found wanting. The URI is preserved unchanged — this moves no
/// resolution decision — but the exact claim is withheld.
///
/// This is why [`ModuleResolutionOutcome::is_resolved`] covers both states while
/// [`ModuleResolutionOutcome::has_complete_denominator`] separates them: an
/// adapter-fed consumer should still navigate, and should not also claim the
/// selection was proven exact.
#[must_use]
pub fn outcome_from_uri_resolution(resolution: &ModuleUriResolution) -> ModuleResolutionOutcome {
    match resolution {
        ModuleUriResolution::Resolved(uri) => {
            ModuleResolutionOutcome::NotProvenPrecedence(uri.clone())
        }
        ModuleUriResolution::NotFound => ModuleResolutionOutcome::NotProvenAbsent,
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
///
/// Both `NotFound` and `NotProvenAbsent` narrow to the legacy `NotFound`, and
/// both `Resolved` and `NotProvenPrecedence` narrow to the legacy `Resolved`.
/// That is not erasure but the direction of the conversion: each legacy state
/// means the weaker of its pair, so the exact member is representable in it and
/// merely stops being *marked* exact. Widening back therefore yields the unproven
/// member of the pair, and a completeness claim is never round-tripped into
/// existence.
#[must_use]
pub fn uri_resolution_from_outcome(
    outcome: &ModuleResolutionOutcome,
) -> Option<ModuleUriResolution> {
    match outcome {
        ModuleResolutionOutcome::Resolved(evidence) => {
            Some(ModuleUriResolution::Resolved(evidence.selected_uri().to_owned()))
        }
        ModuleResolutionOutcome::NotProvenPrecedence(uri) => {
            Some(ModuleUriResolution::Resolved(uri.clone()))
        }
        ModuleResolutionOutcome::NotFound(_) | ModuleResolutionOutcome::NotProvenAbsent => {
            Some(ModuleUriResolution::NotFound)
        }
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
    use crate::request::ModuleRequest;

    /// The exact states are unforgeable outside this crate, so the in-crate
    /// tests mint them here. This also keeps `expect()` out of the tests, which
    /// the repository's Clippy configuration denies.
    fn probe_request() -> ModuleRequest {
        match ModuleRequest::bareword("Foo") {
            Ok(request) => request,
            Err(error) => unreachable!("`Foo` is a valid bareword module: {error}"),
        }
    }

    fn exact_resolution(uri: &str) -> ModuleResolutionOutcome {
        match ModuleResolutionOutcome::resolved(probe_request(), uri.to_string()) {
            Some(outcome) => outcome,
            None => unreachable!("a bareword request is an exact lookup subject"),
        }
    }

    fn exact_absence() -> ModuleResolutionOutcome {
        match ModuleResolutionOutcome::not_found(probe_request()) {
            Some(outcome) => outcome,
            None => unreachable!("a bareword request is an exact lookup subject"),
        }
    }

    use crate::request::{ModuleName, ModuleRequestError, RequestBoundary};

    /// An operand with no exact lookup subject can never have a complete
    /// denominator, so it must not be able to reach an exact outcome at all.
    #[test]
    fn an_inexact_request_cannot_produce_an_exact_outcome() {
        let dynamic = ModuleRequest::dynamic("$class", None, RequestBoundary::RuntimeString);
        let partial = ModuleRequest::partially_static(
            "Foo::$leaf",
            vec!["Foo::".to_string()],
            None,
            RequestBoundary::VariableInterpolation,
        );

        for inexact in [dynamic, partial] {
            let kind = inexact.kind();
            assert_eq!(
                ModuleResolutionOutcome::resolved(inexact.clone(), "file:///w/Foo.pm".to_string()),
                None,
                "{kind:?} has no exact lookup subject, so it cannot be an exact resolution"
            );
            assert_eq!(
                ModuleResolutionOutcome::not_found(inexact),
                None,
                "{kind:?} has no exact lookup subject, so it cannot be a proven absence"
            );
        }
    }

    /// Both exact lookup subjects must still be able to reach an exact outcome,
    /// or the guard above would be refusing everything.
    #[test]
    fn both_exact_subjects_can_still_reach_an_exact_outcome() {
        let bareword = probe_request();
        let file = match ModuleRequest::quoted_require("Foo/Bar.pm") {
            Ok(request) => request,
            Err(error) => unreachable!("`Foo/Bar.pm` is a valid literal file: {error}"),
        };

        for exact in [bareword, file] {
            let kind = exact.kind();
            assert!(
                ModuleResolutionOutcome::resolved(exact.clone(), "file:///w/Foo.pm".to_string())
                    .is_some_and(|outcome| outcome.has_complete_denominator()),
                "{kind:?} is an exact lookup subject"
            );
            assert!(
                ModuleResolutionOutcome::not_found(exact)
                    .is_some_and(|outcome| outcome.has_complete_denominator()),
                "{kind:?} is an exact lookup subject"
            );
        }
    }

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

        assert_eq!(
            outcome_from_uri_resolution(&ModuleUriResolution::NotFound),
            ModuleResolutionOutcome::NotProvenAbsent,
            "a legacy miss cannot widen into a proven absence"
        );
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
    fn only_verified_answers_claim_a_complete_denominator() -> Result<(), ModuleRequestError> {
        assert!(exact_absence().has_complete_denominator());
        assert!(exact_resolution("file:///w/Foo.pm").has_complete_denominator());

        for truncated in [
            ModuleResolutionOutcome::NotProvenAbsent,
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
        Ok(())
    }

    #[test]
    fn boundary_ids_are_namespaced_and_distinct() -> Result<(), ModuleRequestError> {
        let outcomes = [
            exact_resolution("file:///w/Foo.pm"),
            exact_absence(),
            ModuleResolutionOutcome::NotProvenAbsent,
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
        Ok(())
    }

    #[test]
    fn nested_causes_stay_reachable_without_pattern_matching() {
        let invalid = ModuleName::parse("Foo/Bar")
            .err()
            .map(ModuleRequestError::InvalidModuleName)
            .map(ModuleResolutionOutcome::InvalidRequest);

        assert_eq!(
            invalid.as_ref().map(ModuleResolutionOutcome::boundary_id),
            Some("module_resolution.invalid_request"),
            "the outcome class stays stable"
        );
        assert_eq!(
            invalid.as_ref().and_then(ModuleResolutionOutcome::cause_boundary_id),
            Some("module_name.path_separator"),
            "the nested reason must not be flattened away by the outcome class"
        );

        let dynamic = ModuleResolutionOutcome::Dynamic(RequestBoundary::ComputedExpression);
        assert_eq!(dynamic.cause_boundary_id(), Some("request_boundary.computed_expression"));

        for causeless in [
            exact_absence(),
            ModuleResolutionOutcome::NotProvenAbsent,
            ModuleResolutionOutcome::TimedOut,
            ModuleResolutionOutcome::Ambiguous,
            exact_resolution("file:///w/Foo.pm"),
        ] {
            assert_eq!(
                causeless.cause_boundary_id(),
                None,
                "{causeless:?} carries no nested classification to report"
            );
        }
    }

    #[test]
    fn display_debug_and_serde_redact_resolution_evidence() -> Result<(), Box<dyn std::error::Error>>
    {
        let outcome = match ModuleResolutionOutcome::resolved(
            probe_request(),
            "file:///private/customer/secret.pm".to_owned(),
        ) {
            Some(outcome) => outcome,
            None => return Err("the exact probe request must produce evidence".into()),
        };

        let display = outcome.to_string();
        let debug = format!("{outcome:?}");
        let serialized = serde_json::to_string(&outcome)?;

        for representation in [display, debug, serialized] {
            assert!(!representation.contains("private/customer"));
            assert!(!representation.contains("secret.pm"));
        }
        Ok(())
    }
}
