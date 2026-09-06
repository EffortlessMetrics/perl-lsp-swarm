//! Contract tests for the scalar mutation domain (#10736, S0 of the #8364 train).
//!
//! Negative controls: a claim carrying only an observed value, a display-style
//! name, or a public handle cannot bind a target; equal values in different
//! locations, and one shared referent reached through two locations, stay
//! distinct targets; non-canonical integer spellings (`+5`, `007`, `-0`,
//! `1_000`) are refused; a before-dispatch refusal can never report possible
//! application; `EngineRejectedWithoutMutation` is not a possible application;
//! success cannot be built without an observed read-back; receipts never carry
//! key or value payload; a widened writability default cannot appear.
//!
//! Positive controls: exact integers and decimals round-trip without `f64`
//! rounding; interpolation- and structured-shaped strings stay inert data;
//! empty, punctuation, backslash, and Unicode hash keys round-trip exactly;
//! `setVariable` and `setExpression` share one operation type while keeping
//! distinct origins; receipt projections are deterministic.

use perl_dap::mutation::{
    ExactDecimal, ExactInteger, InspectedValueIdentity, MUTATION_SCALAR_VALUE_SCHEMA_VERSION,
    MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION, MutationDeadline, MutationLocationKind,
    MutationMember, MutationOperation, MutationOrigin, MutationOutcome, MutationTarget,
    MutationTargetBindingError, MutationTargetCandidate, MutationTargetCohort, MutationValue,
    MutationValueKind, MutationValueProfile, ObservedReadBack, ResponseValueFormat,
    StructuredMutationLimits, WritabilityDisposition, parse_structured_mutation,
};

type TestResult<T = ()> = Result<T, String>;

/// A minimal writable lexical-scalar claim: the shape acquisition produces.
fn lexical_candidate(frame: &str, binding: &str) -> MutationTargetCandidate {
    MutationTargetCandidate {
        session_generation: Some(7),
        suspension_generation: Some(3),
        value_authority_generation: Some(11),
        frame_identity: frame.to_string(),
        binding_identity: binding.to_string(),
        kind: Some(MutationLocationKind::CurrentFrameLexicalScalar),
        member: Some(MutationMember::WholeScalar),
        inspected_value: None,
        writability: WritabilityDisposition::Writable,
        backend_mode: "native".to_string(),
    }
}

fn bind(candidate: &MutationTargetCandidate) -> TestResult<MutationTarget> {
    candidate.bind().map_err(|error| error.to_string())
}

fn binding_error(candidate: &MutationTargetCandidate) -> TestResult<MutationTargetBindingError> {
    candidate.bind().err().ok_or_else(|| "expected the claim to be refused".to_string())
}

fn integer(canonical: &str) -> TestResult<MutationValue> {
    ExactInteger::admitted(canonical)
        .map(MutationValue::ExactInteger)
        .ok_or_else(|| format!("expected {canonical:?} to be canonical integer text"))
}

fn operation(
    origin: MutationOrigin,
    target: MutationTarget,
    value: MutationValue,
) -> MutationOperation {
    MutationOperation::new(
        42,
        origin,
        target,
        value,
        11,
        MutationDeadline::default(),
        ResponseValueFormat::default(),
    )
}

// ---------------------------------------------------------------------------
// Target identity
// ---------------------------------------------------------------------------

#[test]
fn observed_value_alone_cannot_bind_a_target() -> TestResult {
    // A value-graph observation is not an address. The candidate has no field
    // for a DAP frameId, variablesReference, display name, or evaluateName, so
    // the only thing a caller can offer here is the observation itself.
    let mut candidate = lexical_candidate("frame#1", "");
    candidate.inspected_value = Some(InspectedValueIdentity {
        value_node: "node-1".to_string(),
        referent: Some("SCALAR(0x1)".to_string()),
        value_authority_generation: 11,
    });

    let error = binding_error(&candidate)?;
    if error != MutationTargetBindingError::MissingBindingIdentity {
        return Err(format!("expected a missing-binding refusal, got {error:?}"));
    }
    Ok(())
}

#[test]
fn equal_values_in_different_locations_remain_distinct_targets() -> TestResult {
    let first = bind(&lexical_candidate("frame#1", "pad:$count@0"))?;
    let second = bind(&lexical_candidate("frame#1", "pad:$total@1"))?;

    // Both hold the same value; neither fact makes them one target.
    let value = integer("5")?;
    let _ = operation(MutationOrigin::SetVariable, first.clone(), value.clone());
    let _ = operation(MutationOrigin::SetVariable, second.clone(), value);

    if first == second {
        return Err("two distinct bindings collapsed into one target".to_string());
    }
    if first.location().binding_identity() == second.location().binding_identity() {
        return Err("distinct bindings shared a binding identity".to_string());
    }
    Ok(())
}

#[test]
fn same_spelling_in_two_frames_is_two_targets() -> TestResult {
    let first = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let second = bind(&lexical_candidate("frame#2", "pad:$x@0"))?;

    if first == second {
        return Err("the same spelling in two frames collapsed into one target".to_string());
    }
    Ok(())
}

#[test]
fn one_referent_through_two_locations_stays_two_targets() -> TestResult {
    let shared = InspectedValueIdentity {
        value_node: "node-9".to_string(),
        referent: Some("ARRAY(0xdead)".to_string()),
        value_authority_generation: 11,
    };

    let mut first = lexical_candidate("frame#1", "pad:@rows@0");
    first.kind = Some(MutationLocationKind::CurrentFrameArrayElement);
    first.member = Some(MutationMember::ArrayIndex(0));
    first.inspected_value = Some(shared.clone());

    let mut second = first.clone();
    second.member = Some(MutationMember::ArrayIndex(1));

    let bound_first = bind(&first)?;
    let bound_second = bind(&second)?;

    // Same referent, same container, different cell: still two targets.
    if bound_first == bound_second {
        return Err("two container cells collapsed into one target".to_string());
    }
    if bound_first.location().referent_identity() != bound_second.location().referent_identity() {
        return Err("the shared referent was not preserved on both locations".to_string());
    }
    Ok(())
}

#[test]
fn hash_key_data_round_trips_exactly() -> TestResult {
    // Client display escaping is a rendering concern; key data is bytes.
    let keys = ["", " ", "0", "-", "\\", "\"", "'", "a\\b", "ключ", "🔑", "a;b", "$x", "  DB<1>  "];
    for key in keys {
        let mut candidate = lexical_candidate("frame#1", "pad:%opts@2");
        candidate.kind = Some(MutationLocationKind::CurrentFrameHashEntry);
        candidate.member = Some(MutationMember::HashKey(key.to_string()));
        candidate.inspected_value = Some(InspectedValueIdentity {
            value_node: "node-2".to_string(),
            referent: Some("HASH(0xbeef)".to_string()),
            value_authority_generation: 11,
        });

        let target = bind(&candidate)?;
        match target.location().member() {
            MutationMember::HashKey(observed) if observed == key => {}
            other => return Err(format!("key {key:?} did not round-trip, got {other:?}")),
        }
    }
    Ok(())
}

#[test]
fn container_member_without_a_proven_referent_is_refused() -> TestResult {
    let mut candidate = lexical_candidate("frame#1", "pad:@rows@0");
    candidate.kind = Some(MutationLocationKind::CurrentFrameArrayElement);
    candidate.member = Some(MutationMember::ArrayIndex(0));
    candidate.inspected_value = None;

    let error = binding_error(&candidate)?;
    if error != MutationTargetBindingError::MissingReferentForContainerMember {
        return Err(format!("expected a missing-referent refusal, got {error:?}"));
    }
    Ok(())
}

#[test]
fn member_selector_must_match_the_location_kind() -> TestResult {
    let mut candidate = lexical_candidate("frame#1", "pad:$x@0");
    candidate.member = Some(MutationMember::ArrayIndex(0));

    let error = binding_error(&candidate)?;
    if error != MutationTargetBindingError::MemberKindMismatch {
        return Err(format!("expected a member/kind mismatch, got {error:?}"));
    }
    Ok(())
}

#[test]
fn deferred_location_kinds_are_representable_but_never_bind() -> TestResult {
    for kind in [MutationLocationKind::PackageScalar, MutationLocationKind::NonCurrentFrameScalar] {
        if kind.is_supported_in_v1() {
            return Err(format!("{kind:?} must not be supported in v1"));
        }
        let mut candidate = lexical_candidate("frame#1", "pad:$x@0");
        candidate.kind = Some(kind);

        let error = binding_error(&candidate)?;
        if error != MutationTargetBindingError::UnsupportedLocationKind(kind) {
            return Err(format!(
                "expected an unsupported-kind refusal for {kind:?}, got {error:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn writability_fails_closed_and_uncertainty_never_binds() -> TestResult {
    // The default is NotProven, so a field-by-field claim cannot drift open.
    if WritabilityDisposition::default() != WritabilityDisposition::NotProven {
        return Err("writability default must be NotProven".to_string());
    }
    for disposition in [
        WritabilityDisposition::ReadOnly,
        WritabilityDisposition::Unaddressable,
        WritabilityDisposition::NotProven,
    ] {
        let mut candidate = lexical_candidate("frame#1", "pad:$x@0");
        candidate.writability = disposition;
        let error = binding_error(&candidate)?;
        if error != MutationTargetBindingError::NotWritable(disposition) {
            return Err(format!("expected a not-writable refusal, got {error:?}"));
        }
    }
    Ok(())
}

#[test]
fn a_value_observed_under_another_authority_is_refused() -> TestResult {
    let mut candidate = lexical_candidate("frame#1", "pad:$x@0");
    candidate.inspected_value = Some(InspectedValueIdentity {
        value_node: "node-3".to_string(),
        referent: None,
        value_authority_generation: 10, // candidate expects 11
    });

    let error = binding_error(&candidate)?;
    if error != MutationTargetBindingError::StaleValueObservation {
        return Err(format!("expected a stale-observation refusal, got {error:?}"));
    }
    Ok(())
}

#[test]
fn generations_are_carried_onto_the_bound_target() -> TestResult {
    let target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    if target.location().session_generation() != 7 || target.location().suspension_generation() != 3
    {
        return Err("the target lost its acquisition generations".to_string());
    }
    if target.cohort() != MutationTargetCohort::CurrentFrameLexicalScalar {
        return Err(format!("unexpected cohort {:?}", target.cohort()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Value algebra
// ---------------------------------------------------------------------------

#[test]
fn exact_integers_round_trip_without_float_normalization() -> TestResult {
    let long_digits = "1".repeat(256);
    for canonical in ["0", "5", "-5", "9007199254740993", &long_digits] {
        let value = integer(canonical)?;
        match &value {
            MutationValue::ExactInteger(exact) if exact.canonical() == canonical => {}
            other => return Err(format!("{canonical:?} did not round-trip, got {other:?}")),
        }
    }

    // 9007199254740993 is the first integer f64 cannot represent; a float
    // round-trip would have silently returned ...992.
    let exact = ExactInteger::admitted("9007199254740993")
        .ok_or_else(|| "expected canonical admission".to_string())?;
    if exact.canonical() != "9007199254740993" {
        return Err("exact integer was rounded".to_string());
    }
    if exact.significant_digits() != 16 {
        return Err(format!("unexpected digit count {}", exact.significant_digits()));
    }
    Ok(())
}

#[test]
fn non_canonical_integer_spellings_are_refused() -> TestResult {
    // Normalization belongs to the value parser (#10745); admission is exact.
    for spelling in ["+5", "007", "-0", "1_000", "", "-", "5 ", " 5", "5.0", "0x10", "1e3", "٥"] {
        if ExactInteger::admitted(spelling).is_some() {
            return Err(format!("{spelling:?} must not be canonical integer text"));
        }
    }
    Ok(())
}

#[test]
fn negative_zero_policy_is_pinned() -> TestResult {
    // Integer: Perl's -0 is numerically 0, so the domain keeps one spelling.
    if ExactInteger::admitted("-0").is_some() {
        return Err("integer -0 must not be admitted".to_string());
    }
    // Decimal: signed zero is observable, so -0.0 stays distinct from 0.0.
    let negative =
        ExactDecimal::admitted("-0.0").ok_or_else(|| "expected -0.0 to be admitted".to_string())?;
    let positive =
        ExactDecimal::admitted("0.0").ok_or_else(|| "expected 0.0 to be admitted".to_string())?;
    if negative == positive {
        return Err("-0.0 and 0.0 must remain distinct decimals".to_string());
    }
    Ok(())
}

#[test]
fn exact_decimals_keep_precision_f64_would_lose() -> TestResult {
    let canonical = "0.1234567890123456789012345678901234567890";
    let decimal = ExactDecimal::admitted(canonical)
        .ok_or_else(|| "expected canonical decimal admission".to_string())?;
    if decimal.canonical() != canonical {
        return Err("decimal precision was lost".to_string());
    }
    let value = MutationValue::ExactDecimal(decimal);
    if value.kind() != MutationValueKind::ExactDecimal {
        return Err("unexpected decimal cohort".to_string());
    }
    Ok(())
}

#[test]
fn string_data_stays_inert_whatever_it_resembles() -> TestResult {
    // None of these acquire interpolation, command, or structured meaning by
    // being stored: a MutationValue is data, not Perl source.
    let hostile = [
        "$foo",
        "@ARGV",
        "%ENV",
        "system(\"rm -rf /\")",
        "; print 1;",
        "`id`",
        "$(id)",
        "DB<1>",
        "json:[1,2]",
        "\\x{263A}",
        "",
        "☃ combining é",
    ];
    for text in hostile {
        let value = MutationValue::UnicodeString(text.to_string());
        if value.kind() != MutationValueKind::UnicodeString {
            return Err(format!("{text:?} changed cohort"));
        }
        if value.profile() != MutationValueProfile::ScalarV1 {
            return Err(format!("{text:?} left the scalar profile"));
        }
        match &value {
            MutationValue::UnicodeString(stored) if stored == text => {}
            other => return Err(format!("{text:?} did not round-trip, got {other:?}")),
        }
    }
    Ok(())
}

#[test]
fn scalar_and_structured_profiles_cannot_be_confused() -> TestResult {
    // The same bytes are a structured payload under one profile and inert
    // string data under the other. There is no conversion between them.
    let raw = "json:[1,2]";
    let structured = parse_structured_mutation(raw, &StructuredMutationLimits::default())
        .map_err(|error| error.to_string())?;
    if structured.schema_version() != MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION {
        return Err("unexpected structured schema version".to_string());
    }

    let scalar = MutationValue::UnicodeString(raw.to_string());
    if scalar.profile() != MutationValueProfile::ScalarV1 {
        return Err("structured-shaped text left the scalar profile".to_string());
    }
    if MutationValueProfile::ScalarV1.schema_version() != MUTATION_SCALAR_VALUE_SCHEMA_VERSION {
        return Err("scalar profile version drifted".to_string());
    }
    if MutationValueProfile::ScalarV1 == MutationValueProfile::StructuredV1 {
        return Err("the two profiles must remain distinct".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Operation identity
// ---------------------------------------------------------------------------

#[test]
fn set_variable_and_set_expression_share_one_operation_type() -> TestResult {
    let target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let value = integer("1")?;

    let from_variable = operation(MutationOrigin::SetVariable, target.clone(), value.clone());
    let from_expression = operation(MutationOrigin::SetExpression, target, value);

    if from_variable.origin() == from_expression.origin() {
        return Err("the two admission origins collapsed".to_string());
    }
    if from_variable.target() != from_expression.target()
        || from_variable.value() != from_expression.value()
    {
        return Err("the shared lower operation diverged".to_string());
    }
    Ok(())
}

#[test]
fn response_format_cannot_change_the_assigned_value() -> TestResult {
    let target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let value = integer("255")?;

    let plain = MutationOperation::new(
        1,
        MutationOrigin::SetVariable,
        target.clone(),
        value.clone(),
        11,
        MutationDeadline::default(),
        ResponseValueFormat { hex: false },
    );
    let hex = MutationOperation::new(
        1,
        MutationOrigin::SetVariable,
        target,
        value,
        11,
        MutationDeadline::default(),
        ResponseValueFormat { hex: true },
    );

    if plain.value() != hex.value() {
        return Err("a response format changed the assigned data".to_string());
    }
    if plain.response_format() == hex.response_format() {
        return Err("the requested format was not retained".to_string());
    }
    Ok(())
}

#[test]
fn operation_authority_is_taken_from_the_bound_target() -> TestResult {
    let target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let op = operation(MutationOrigin::SetVariable, target, integer("1")?);

    if op.expected_session_generation() != 7 || op.expected_suspension_generation() != 3 {
        return Err("the operation invented authority the target never had".to_string());
    }
    if op.expected_value_authority_generation() != 11 {
        return Err("the operation lost its value authority".to_string());
    }
    if op.value_profile() != MutationValueProfile::ScalarV1 {
        return Err("unexpected operation value profile".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Outcome model
// ---------------------------------------------------------------------------

fn before_dispatch_outcomes() -> Vec<MutationOutcome> {
    vec![
        MutationOutcome::Unsupported {
            backend: "mock".to_string(),
            mode: "native".to_string(),
            profile: MutationValueProfile::ScalarV1,
        },
        MutationOutcome::PolicyRefused,
        MutationOutcome::NotInitialized,
        MutationOutcome::NotStopped,
        MutationOutcome::StaleSession,
        MutationOutcome::StaleSuspension,
        MutationOutcome::StaleValueAuthority,
        MutationOutcome::UnknownOrWrongContainerMember,
        MutationOutcome::ReadOnlyOrUnaddressable(WritabilityDisposition::ReadOnly),
        MutationOutcome::UnsupportedFrameOrTargetCohort,
        MutationOutcome::ValueParseRefused,
        MutationOutcome::TimeoutBeforeDispatch,
        MutationOutcome::CancelledBeforeDispatch,
        MutationOutcome::TransportFailureBeforeDispatch,
    ]
}

fn after_dispatch_outcomes() -> Vec<MutationOutcome> {
    vec![
        MutationOutcome::EngineRejectedWithoutMutation,
        MutationOutcome::indeterminate_after_dispatch(),
        MutationOutcome::ReadBackMissingAfterPossibleMutation,
        MutationOutcome::ReadBackMalformedAfterPossibleMutation,
        MutationOutcome::ReadBackTargetMismatch,
        MutationOutcome::ReadBackOpaqueOrResourceLimited,
    ]
}

fn success_outcome() -> MutationOutcome {
    MutationOutcome::SuccessWithObservedReadBack(ObservedReadBack {
        observed_value: MutationValue::UnicodeString("observed".to_string()),
        observed_binding_identity: "pad:$x@0".to_string(),
        observed_value_authority_generation: 12,
    })
}

#[test]
fn before_and_after_dispatch_failures_cannot_collapse() -> TestResult {
    for outcome in before_dispatch_outcomes() {
        if !outcome.is_before_dispatch() {
            return Err(format!("{outcome:?} should be decided before dispatch"));
        }
        if outcome.possible_application() {
            return Err(format!("{outcome:?} claimed a write may have landed"));
        }
        if outcome.invalidates_value_authority() {
            return Err(format!("{outcome:?} invalidated authority without dispatching"));
        }
    }
    for outcome in after_dispatch_outcomes() {
        if outcome.is_before_dispatch() {
            return Err(format!("{outcome:?} should be decided after dispatch"));
        }
    }
    if success_outcome().is_before_dispatch() {
        return Err("success cannot be a before-dispatch outcome".to_string());
    }
    Ok(())
}

#[test]
fn engine_rejection_differs_from_possible_application() -> TestResult {
    let rejected = MutationOutcome::EngineRejectedWithoutMutation;
    if rejected.possible_application() {
        return Err("a proven engine rejection must not be a possible application".to_string());
    }
    if rejected.invalidates_value_authority() {
        return Err("a proven engine rejection must not invalidate authority".to_string());
    }

    let indeterminate = MutationOutcome::indeterminate_after_dispatch();
    if !indeterminate.possible_application() {
        return Err("an indeterminate dispatch must be a possible application".to_string());
    }
    if !indeterminate.invalidates_value_authority() {
        return Err("an indeterminate dispatch must invalidate authority".to_string());
    }
    if rejected == indeterminate {
        return Err("the two after-dispatch outcomes collapsed".to_string());
    }
    Ok(())
}

#[test]
fn possible_mutation_without_read_back_invalidates_authority() -> TestResult {
    for outcome in [
        MutationOutcome::ReadBackMissingAfterPossibleMutation,
        MutationOutcome::ReadBackMalformedAfterPossibleMutation,
        MutationOutcome::ReadBackTargetMismatch,
        MutationOutcome::ReadBackOpaqueOrResourceLimited,
    ] {
        if !outcome.invalidates_value_authority() {
            return Err(format!("{outcome:?} must invalidate the old value authority"));
        }
        if outcome.observed_read_back().is_some() {
            return Err(format!("{outcome:?} must carry no read-back"));
        }
    }
    Ok(())
}

#[test]
fn success_requires_an_observed_read_back() -> TestResult {
    let success = success_outcome();
    let read_back = success
        .observed_read_back()
        .ok_or_else(|| "success must carry an observed read-back".to_string())?;

    // The payload is what was observed, not what was requested.
    if read_back.observed_binding_identity != "pad:$x@0" {
        return Err("the read-back lost its observed binding identity".to_string());
    }
    if !success.is_success() || success.invalidates_value_authority() {
        return Err("a confirmed success must not invalidate authority".to_string());
    }

    // No other variant can present itself as a success.
    for outcome in before_dispatch_outcomes().into_iter().chain(after_dispatch_outcomes()) {
        if outcome.is_success() || outcome.observed_read_back().is_some() {
            return Err(format!("{outcome:?} must not be a success"));
        }
    }
    Ok(())
}

#[test]
fn indeterminate_after_dispatch_always_states_possible_application() -> TestResult {
    match MutationOutcome::indeterminate_after_dispatch() {
        MutationOutcome::IndeterminateAfterDispatch { possible_application } => {
            if !possible_application {
                return Err("possible_application must be true".to_string());
            }
            Ok(())
        }
        other => Err(format!("unexpected outcome {other:?}")),
    }
}

#[test]
fn outcome_receipt_classes_are_a_closed_unique_vocabulary() -> TestResult {
    let mut classes = Vec::new();
    for outcome in before_dispatch_outcomes()
        .into_iter()
        .chain(after_dispatch_outcomes())
        .chain([success_outcome()])
    {
        classes.push(outcome.receipt_class());
    }
    let total = classes.len();
    classes.sort_unstable();
    classes.dedup();
    if classes.len() != total {
        return Err("two outcomes shared a receipt class".to_string());
    }
    if total != 21 {
        return Err(format!("expected 21 outcome variants, saw {total}"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt projections
// ---------------------------------------------------------------------------

#[test]
fn receipts_redact_private_key_and_value_payload() -> TestResult {
    let secret_key = "api-token";
    let secret_value = "hunter2";

    let mut candidate = lexical_candidate("frame#1", "pad:%creds@4");
    candidate.kind = Some(MutationLocationKind::CurrentFrameHashEntry);
    candidate.member = Some(MutationMember::HashKey(secret_key.to_string()));
    candidate.inspected_value = Some(InspectedValueIdentity {
        value_node: "node-4".to_string(),
        referent: Some("HASH(0xfeed)".to_string()),
        value_authority_generation: 11,
    });

    let target = bind(&candidate)?;
    let op = operation(
        MutationOrigin::SetVariable,
        target,
        MutationValue::UnicodeString(secret_value.to_string()),
    );

    let receipt = op.receipt_projection();
    let rendered = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;

    if rendered.contains(secret_key) || rendered.contains(secret_value) {
        return Err(format!("receipt leaked private payload: {rendered}"));
    }
    // Identity and shape survive redaction.
    if receipt.target.key_bytes != Some(secret_key.len()) {
        return Err("receipt lost the redacted key length".to_string());
    }
    if receipt.value.payload_bytes != secret_value.len() {
        return Err("receipt lost the redacted value length".to_string());
    }
    if receipt.target.cohort != MutationTargetCohort::CurrentFrameHashEntry {
        return Err("receipt lost the target cohort".to_string());
    }
    if receipt.value.kind != MutationValueKind::UnicodeString {
        return Err("receipt lost the value cohort".to_string());
    }
    Ok(())
}

#[test]
fn outcome_receipts_carry_classification_not_observed_data() -> TestResult {
    let receipt = success_outcome().receipt_projection();
    let rendered = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;

    if rendered.contains("observed\"") && rendered.contains("pad:$x@0") {
        return Err(format!("outcome receipt leaked observed data: {rendered}"));
    }
    if rendered.contains("pad:$x@0") {
        return Err(format!("outcome receipt leaked binding identity: {rendered}"));
    }
    if receipt.class != "success_with_observed_read_back" || !receipt.possible_application {
        return Err("outcome receipt lost its classification".to_string());
    }
    match receipt.read_back_value {
        Some(value) if value.kind == MutationValueKind::UnicodeString => {}
        other => return Err(format!("unexpected read-back projection {other:?}")),
    }
    Ok(())
}

#[test]
fn receipt_projection_is_deterministic() -> TestResult {
    let target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let op = operation(MutationOrigin::SetVariable, target, integer("5")?);

    let first = serde_json::to_string(&op.receipt_projection()).map_err(|e| e.to_string())?;
    let second = serde_json::to_string(&op.receipt_projection()).map_err(|e| e.to_string())?;
    if first != second {
        return Err("receipt projection was not deterministic".to_string());
    }

    // A different value in the same location produces a different receipt.
    let other_target = bind(&lexical_candidate("frame#1", "pad:$x@0"))?;
    let other = operation(MutationOrigin::SetVariable, other_target, integer("12345")?);
    let rendered = serde_json::to_string(&other.receipt_projection()).map_err(|e| e.to_string())?;
    if rendered == first {
        return Err("receipts did not discriminate value size".to_string());
    }
    Ok(())
}
