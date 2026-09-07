//! Behaviour proof for validated module requests and typed outcomes (#8497).
//!
//! Each scenario states a distinction that the previous `&str` + three-state
//! contract could not express.

use perl_module::{
    LegacySeparatorProfile, ModuleName, ModuleNameError, ModuleRequest, ModuleRequestError,
    ModuleRequestKind, ModuleResolutionOutcome, ModuleUriResolution, PackageSeparatorForm,
    RequestBoundary, outcome_from_uri_resolution, uri_resolution_from_outcome,
};

#[test]
fn given_a_bareword_operand_when_classified_then_it_is_a_module_request()
-> Result<(), ModuleRequestError> {
    let request = ModuleRequest::bareword("My::Module")?;

    assert_eq!(request.kind(), ModuleRequestKind::BarewordModule);
    assert_eq!(request.module_name().map(ModuleName::canonical), Some("My::Module"));
    assert!(request.is_exact());
    Ok(())
}

#[test]
fn given_a_quoted_require_operand_when_classified_then_it_stays_a_filename()
-> Result<(), ModuleRequestError> {
    let request = ModuleRequest::quoted_require("My::Module")?;

    assert_eq!(request.kind(), ModuleRequestKind::LiteralRelativeFile);
    assert_eq!(
        request.module_name(),
        None,
        "Perl looks a quoted operand up as a filename; it must not gain module identity"
    );
    assert_eq!(request.literal_file().map(ToString::to_string), Some("My::Module".to_string()));
    Ok(())
}

#[test]
fn given_a_traversing_string_when_classified_then_it_is_invalid_not_missing() {
    let error = ModuleRequest::bareword("../../etc/passwd").err();

    assert_eq!(
        error.as_ref().map(ModuleRequestError::boundary_id),
        Some("module_name.path_separator"),
        "an invalid request must be classified, never reported as a valid missing module"
    );
}

#[test]
fn given_a_dynamic_operand_when_recorded_then_its_boundary_survives() {
    let request = ModuleRequest::dynamic("$class", None, RequestBoundary::VariableInterpolation);

    assert_eq!(request.kind(), ModuleRequestKind::Dynamic);
    assert!(!request.is_exact());
    assert_eq!(request.boundary(), Some(RequestBoundary::VariableInterpolation));
    assert_eq!(
        request.boundary().map(RequestBoundary::boundary_id),
        Some("request_boundary.variable_interpolation")
    );
}

#[test]
fn given_a_partially_static_operand_when_recorded_then_it_does_not_become_exact() {
    let request = ModuleRequest::partially_static(
        "\"My::$leaf\"",
        vec!["My::".to_string()],
        None,
        RequestBoundary::VariableInterpolation,
    );

    assert_eq!(request.kind(), ModuleRequestKind::PartiallyStatic);
    assert!(
        !request.is_exact(),
        "a recovered static fragment is evidence, not an exact lookup subject"
    );
    assert_eq!(request.module_name(), None);
}

#[test]
fn given_a_legacy_separator_when_validated_then_the_spelling_is_recorded()
-> Result<(), ModuleNameError> {
    let name = ModuleName::parse("My'Module")?;

    assert_eq!(name.canonical(), "My::Module");
    assert_eq!(
        name.separator_form(),
        PackageSeparatorForm::Legacy,
        "normalization must not erase which separator the source used"
    );
    assert_eq!(name.legacy_spelling(), "My'Module");
    Ok(())
}

#[test]
fn given_a_rejecting_target_profile_when_validating_then_legacy_names_are_refused() {
    assert_eq!(
        ModuleName::parse_with_profile("My'Module", LegacySeparatorProfile::Reject),
        Err(ModuleNameError::LegacySeparatorRejected),
        "the legacy separator is a target-profile decision, not an unconditional rewrite"
    );
}

#[test]
fn given_a_timed_out_search_when_reported_then_it_does_not_claim_an_exact_denominator() {
    let outcome = outcome_from_uri_resolution(&ModuleUriResolution::TimedOut);

    assert_eq!(outcome, ModuleResolutionOutcome::TimedOut);
    assert!(
        !outcome.has_complete_denominator(),
        "a truncated search must not be reported as a proven absence"
    );
}

#[test]
fn given_a_legacy_miss_when_widened_then_it_does_not_claim_a_proven_absence() {
    let outcome = outcome_from_uri_resolution(&ModuleUriResolution::NotFound);

    assert_eq!(
        outcome,
        ModuleResolutionOutcome::NotProvenAbsent,
        "the three-state resolver skips boundary-rejected roots without recording it, \
         so its miss is not a proven absence"
    );
    assert!(
        !outcome.has_complete_denominator(),
        "an unproven absence must never be reported as `this module does not exist`"
    );
}

#[test]
fn given_a_consumer_outside_the_resolver_when_a_miss_is_reported_then_it_is_never_exact() {
    // Renamed to match what this scenario can actually observe. The exact miss
    // still exists and still means what it says, but it is now established by
    // the crate that performs the search: `NotFound` carries private
    // `AbsenceEvidence`, so no consumer out here can construct one. That makes
    // every miss reachable from this file the unproven kind, and asserting on
    // the exact form from here is not possible — it is covered by the in-crate
    // constructor tests and by the `compile_fail` doctests on the evidence
    // types.
    let miss = outcome_from_uri_resolution(&ModuleUriResolution::NotFound);
    assert_eq!(miss, ModuleResolutionOutcome::NotProvenAbsent);
    assert!(!miss.has_complete_denominator());
    assert!(!ModuleResolutionOutcome::NotProvenAbsent.has_complete_denominator());
}

#[test]
fn given_an_outcome_the_legacy_enum_cannot_express_when_narrowed_then_it_refuses() {
    for outcome in [
        ModuleResolutionOutcome::Ambiguous,
        ModuleResolutionOutcome::OutsideAuthority,
        ModuleResolutionOutcome::EnvironmentUnavailable,
        ModuleResolutionOutcome::IoLimited,
        ModuleResolutionOutcome::Dynamic(RequestBoundary::RuntimeString),
    ] {
        assert_eq!(
            uri_resolution_from_outcome(&outcome),
            None,
            "{outcome:?} must not be silently collapsed into a legacy state"
        );
    }
}
