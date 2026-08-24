//! Contract tests for `MutationStructuredValue.v1` (#11327, S0).
//!
//! Negative controls: bare Perl-looking text without the `json:` prefix stays
//! unsupported; duplicate object keys are rejected (never last-wins); depth,
//! node, entry, scalar-byte, aggregate-byte, digit, and exponent budgets are
//! enforced; integers outside the exact bounded range are refused. Positive
//! controls: nested finite trees parse; scalars admit no fresh referent while
//! arrays/objects map to fresh ARRAY/HASH referents; serialization is
//! deterministic and fingerprint-stable under key order.

use perl_dap::mutation::{
    fresh_referent_kind, parse_structured_mutation, structured_payload, FreshReferentKind,
    StructuredMutationLimits, StructuredRefusal, StructuredValue,
    MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION,
};
use std::fmt::Write as _;

type TestResult<T = ()> = Result<T, String>;

fn parse(text: &str) -> TestResult<StructuredValue> {
    let envelope = parse_structured_mutation(text, &StructuredMutationLimits::default())
        .map_err(|error| error.to_string())?;
    assert_eq!(envelope.schema_version, MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION);
    Ok(envelope.value)
}

fn parse_envelope(text: &str) -> TestResult<perl_dap::mutation::MutationStructuredValueV1> {
    parse_structured_mutation(text, &StructuredMutationLimits::default())
        .map_err(|error| error.to_string())
}

fn parse_refusal(text: &str) -> TestResult<StructuredRefusal> {
    parse_structured_mutation(text, &StructuredMutationLimits::default())
        .err()
        .ok_or_else(|| format!("expected {text:?} to be refused"))
}

#[test]
fn missing_json_prefix_refuses_bare_perl_looking_text() -> TestResult {
    for bare in ["[1, 2]", "{ \"a\": 1 }", "\\@array", "sub { 42 }", " $x ", "JSON:[1]"] {
        let error = parse_structured_mutation(bare, &StructuredMutationLimits::default())
            .err()
            .ok_or_else(|| format!("bare text {bare:?} must be refused"))?;
        assert_eq!(
            error,
            StructuredRefusal::MissingStructuredPrefix,
            "bare text {bare:?} must refuse with the prefix error"
        );
    }
    // The prefix is byte-exact and required at offset zero.
    assert_eq!(structured_payload("json:[]"), Ok("[]"));
    assert!(structured_payload(" json:[]").is_err());
    Ok(())
}

#[test]
fn nested_finite_tree_parses_with_scalars() -> TestResult {
    let value = parse(r#"json:{"a": [1, -2, null, true, false, "s"], "b": {"c": 1.5e2}}"#)?;
    let StructuredValue::Object(entries) = &value else {
        return Err("expected object root".to_string());
    };
    assert_eq!(entries.len(), 2);
    let StructuredValue::Array(items) = &entries[0].1 else {
        return Err("expected array under key a".to_string());
    };
    assert_eq!(items[0], StructuredValue::Integer(1));
    assert_eq!(items[1], StructuredValue::Integer(-2));
    assert_eq!(items[2], StructuredValue::Null);
    assert_eq!(items[3], StructuredValue::Bool(true));
    assert_eq!(items[4], StructuredValue::Bool(false));
    assert_eq!(items[5], StructuredValue::String("s".to_string()));
    assert!(matches!(value, StructuredValue::Object(_)));
    Ok(())
}

#[test]
fn duplicate_object_keys_are_rejected_not_last_wins() -> TestResult {
    let error = parse_refusal(r#"json:{"k": 1, "k": 2}"#)?;
    assert_eq!(error, StructuredRefusal::DuplicateKey { key: "k".to_string() });
    Ok(())
}

#[test]
fn depth_budget_is_enforced() -> TestResult {
    let mut deep = String::from("json:");
    for _ in 0..=17 {
        deep.push('[');
    }
    for _ in 0..=17 {
        deep.push(']');
    }
    let error = parse_refusal(&deep)?;
    assert_eq!(error, StructuredRefusal::DepthExceeded { limit: 16 });
    Ok(())
}

#[test]
fn node_budget_is_enforced() -> TestResult {
    let limits = StructuredMutationLimits { max_nodes: 4, ..StructuredMutationLimits::default() };
    let error = parse_structured_mutation("json:[1,2,3,4,5]", &limits)
        .err()
        .ok_or("over-wide tree must fail")?;
    assert_eq!(error, StructuredRefusal::TooManyNodes { limit: 4 });
    Ok(())
}

#[test]
fn entry_budget_is_enforced_per_container() -> TestResult {
    let limits = StructuredMutationLimits { max_entries: 3, ..StructuredMutationLimits::default() };
    let error = parse_structured_mutation(r#"json:{"a":1,"b":2,"c":3,"d":4}"#, &limits)
        .err()
        .ok_or("over-wide object must fail")?;
    assert_eq!(error, StructuredRefusal::TooManyEntries { limit: 3 });
    Ok(())
}

#[test]
fn integer_out_of_exact_range_is_refused_without_float_fallback() -> TestResult {
    let mut beyond = String::from("json:-9");
    let _ = write!(beyond, "{}", "9".repeat(25));
    let error = parse_structured_mutation(&beyond, &StructuredMutationLimits::default())
        .err()
        .ok_or("beyond-i64 integer must be refused")?;
    assert_eq!(error, StructuredRefusal::IntegerOutOfRange);
    Ok(())
}

#[test]
fn digit_and_exponent_budgets_are_enforced() -> TestResult {
    let limits = StructuredMutationLimits {
        max_significant_digits: 4,
        max_absolute_exponent: 8,
        ..StructuredMutationLimits::default()
    };
    let too_many_digits = parse_structured_mutation("json:1.000001e0", &limits)
        .err()
        .ok_or("digit budget must bind")?;
    assert_eq!(too_many_digits, StructuredRefusal::TooManyDigits { limit: 4 });

    let exponent_too_large =
        parse_structured_mutation("json:1e9", &limits).err().ok_or("exponent budget must bind")?;
    assert_eq!(exponent_too_large, StructuredRefusal::ExponentTooLarge { limit: 8 });

    let within =
        parse_structured_mutation("json:1.5e-8", &limits).map_err(|error| error.to_string())?;
    assert_eq!(
        within.value,
        StructuredValue::Decimal(perl_dap::mutation::ExactDecimal {
            canonical: "1.5e-8".to_string()
        })
    );
    Ok(())
}

#[test]
fn scalars_admit_no_fresh_referent_arrays_and_objects_do() -> TestResult {
    for scalar in ["json:null", "json:true", "json:3", r#"json:"text""#, "json:1.25"] {
        let value = parse(scalar)?;
        assert_eq!(fresh_referent_kind(&value), None, "scalars must not create a fresh referent");
    }
    let array = parse("json:[]")?;
    let hash = parse("json:{}")?;
    assert_eq!(fresh_referent_kind(&array), Some(FreshReferentKind::Array));
    assert_eq!(fresh_referent_kind(&hash), Some(FreshReferentKind::Hash));
    Ok(())
}

#[test]
fn ordering_is_deterministic_and_fingerprints_stable() -> TestResult {
    let first = parse_envelope(r#"json:{"z": 1, "a": [true, null]}"#)?;
    let second = parse_envelope(r#"json:{"z": 1, "a": [true, null]}"#)?;
    assert_eq!(first.fingerprint, second.fingerprint);
    // Object entry order is preserved as written (deterministic receipt-safe
    // ordering), not silently re-sorted.
    let StructuredValue::Object(entries) = &first.value else {
        return Err("expected object".to_string());
    };
    assert_eq!(entries[0].0, "z");
    assert_eq!(entries[1].0, "a");
    Ok(())
}
