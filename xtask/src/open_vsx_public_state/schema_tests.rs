//! Conformance between the emitted receipt, the observation fixtures, and the
//! published contracts under `schemas/`.
//!
//! Validation goes through the `jsonschema` crate rather than a hand-rolled
//! subset checker, so a contract that drifts away from what the code emits is
//! caught by the same rules an external consumer would apply.

use super::test_support::{
    AVAILABLE_EXACT, DIGEST_MISMATCH, INCIDENT, INSTRUMENT_INCOMPLETE, LISTING_MISSING,
    NAMESPACE_ABSENT, RATE_LIMITED, UNPLANNED_URL, receipt,
};
use color_eyre::eyre::{Result, bail};
use serde_json::Value;

const OBSERVATION_SCHEMA: &str =
    include_str!("../../../schemas/open_vsx_public_state_observation.v1.schema.json");
const RECEIPT_SCHEMA: &str = include_str!("../../../schemas/open_vsx_public_state.v1.schema.json");

const FIXTURES: &[(&str, &str)] = &[
    ("incident", INCIDENT),
    ("available_exact", AVAILABLE_EXACT),
    ("listing_missing", LISTING_MISSING),
    ("rate_limited", RATE_LIMITED),
    ("namespace_absent", NAMESPACE_ABSENT),
    ("digest_mismatch", DIGEST_MISMATCH),
    ("unplanned_url", UNPLANNED_URL),
    ("instrument_incomplete", INSTRUMENT_INCOMPLETE),
];

fn validator(raw: &str) -> Result<jsonschema::Validator> {
    let schema: Value = serde_json::from_str(raw)?;
    jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("compiling schema: {error}"))
}

fn validate(validator: &jsonschema::Validator, document: &Value, label: &str) -> Result<()> {
    let errors: Vec<String> =
        validator.iter_errors(document).map(|error| format!("{error}")).collect();
    if !errors.is_empty() {
        bail!("{label} does not conform: {}", errors.join("; "));
    }
    Ok(())
}

#[test]
fn every_observation_fixture_conforms_to_the_observation_contract() -> Result<()> {
    let validator = validator(OBSERVATION_SCHEMA)?;
    for (label, raw) in FIXTURES {
        let document: Value = serde_json::from_str(raw)?;
        validate(&validator, &document, label)?;
    }
    Ok(())
}

#[test]
fn every_emitted_receipt_conforms_to_the_receipt_contract() -> Result<()> {
    let validator = validator(RECEIPT_SCHEMA)?;
    for (label, raw) in FIXTURES {
        let document = serde_json::to_value(receipt(raw)?)?;
        validate(&validator, &document, label)?;
    }
    Ok(())
}

#[test]
fn the_receipt_contract_rejects_a_missing_or_extra_field() -> Result<()> {
    let validator = validator(RECEIPT_SCHEMA)?;
    let baseline = serde_json::to_value(receipt(AVAILABLE_EXACT)?)?;
    validate(&validator, &baseline, "baseline")?;

    let mut missing = baseline.clone();
    let Some(object) = missing.as_object_mut() else {
        bail!("receipt is not an object");
    };
    object.remove("state");
    if validator.is_valid(&missing) {
        bail!("a receipt without a state passed validation");
    }

    let mut extra = baseline;
    let Some(object) = extra.as_object_mut() else {
        bail!("receipt is not an object");
    };
    object.insert("ovsx_pat".to_owned(), Value::String("smuggled".to_owned()));
    if validator.is_valid(&extra) {
        bail!("a receipt with an undeclared field passed validation");
    }
    Ok(())
}

#[test]
fn the_observation_contract_rejects_an_unsanctioned_method_or_state() -> Result<()> {
    let validator = validator(OBSERVATION_SCHEMA)?;
    let baseline: Value = serde_json::from_str(AVAILABLE_EXACT)?;
    validate(&validator, &baseline, "baseline")?;

    let mut mutating = baseline.clone();
    mutating["cells"]["listing"]["transport"]["method"] = Value::String("POST".to_owned());
    if validator.is_valid(&mutating) {
        bail!("the observation contract admitted a non-GET request");
    }

    let mut smuggled = baseline;
    smuggled["cells"]["listing"]["transport"]["authorization"] =
        Value::String("Bearer token".to_owned());
    if validator.is_valid(&smuggled) {
        bail!("the observation contract admitted a credential field");
    }
    Ok(())
}

#[test]
fn the_receipt_contract_enforces_the_invariants_the_classifier_guarantees() -> Result<()> {
    // Raised in review: a schema-valid receipt could previously contradict the
    // classifier — claiming available_exact while carrying blockers, or a
    // subject version with no plan digest. A consumer reading only the contract
    // had no way to know those shapes were impossible.
    let validator = validator(RECEIPT_SCHEMA)?;
    let baseline = serde_json::to_value(receipt(AVAILABLE_EXACT)?)?;
    validate(&validator, &baseline, "baseline")?;

    let mut exact_with_blockers = baseline.clone();
    exact_with_blockers["blockers"] = serde_json::json!([{
        "code": "invented", "message": "invented", "owner": "#9923"
    }]);
    if validator.is_valid(&exact_with_blockers) {
        bail!("available_exact with blockers passed validation");
    }

    let mut exact_without_bytes = baseline.clone();
    exact_without_bytes["public_bytes"] = Value::Null;
    if validator.is_valid(&exact_without_bytes) {
        bail!("available_exact without proven public bytes passed validation");
    }

    let mut unblocked_failure = baseline.clone();
    unblocked_failure["state"] = Value::String("extension_missing".to_owned());
    unblocked_failure["blockers"] = serde_json::json!([]);
    if validator.is_valid(&unblocked_failure) {
        bail!("a non-exact state with no blocker passed validation");
    }

    let mut half_a_plan = baseline.clone();
    half_a_plan["probe_plan_digest"] = Value::Null;
    if validator.is_valid(&half_a_plan) {
        bail!("a subject version with no plan digest passed validation");
    }

    let mut reordered = baseline;
    let Some(cells) = reordered["cells"].as_array().cloned() else {
        bail!("cells must be an array");
    };
    let mut swapped = cells;
    swapped.swap(0, 1);
    reordered["cells"] = Value::Array(swapped);
    if validator.is_valid(&reordered) {
        bail!("a receipt whose surfaces are out of order passed validation");
    }
    Ok(())
}

#[test]
fn the_observation_contract_couples_an_outcome_to_its_status() -> Result<()> {
    let validator = validator(OBSERVATION_SCHEMA)?;
    let baseline: Value = serde_json::from_str(AVAILABLE_EXACT)?;
    validate(&validator, &baseline, "baseline")?;

    let mut status_without_response = baseline.clone();
    status_without_response["cells"]["listing"]["transport"]["outcome"] =
        Value::String("transport_error".to_owned());
    if validator.is_valid(&status_without_response) {
        bail!("a transport error carrying an HTTP status passed validation");
    }

    let mut response_without_status = baseline;
    response_without_status["cells"]["listing"]["transport"]["status"] = Value::Null;
    if validator.is_valid(&response_without_status) {
        bail!("an HTTP response with no status passed validation");
    }
    Ok(())
}

#[test]
fn the_contracts_declare_the_versions_the_code_emits() -> Result<()> {
    let receipt_schema: Value = serde_json::from_str(RECEIPT_SCHEMA)?;
    let observation_schema: Value = serde_json::from_str(OBSERVATION_SCHEMA)?;

    let receipt_version = receipt_schema["properties"]["schema_version"]["const"].as_str();
    if receipt_version != Some(super::model::RECEIPT_SCHEMA_VERSION) {
        bail!("receipt contract declares {receipt_version:?}");
    }
    let observation_version = observation_schema["properties"]["schema_version"]["const"].as_str();
    if observation_version != Some(super::model::OBSERVATION_SCHEMA_VERSION) {
        bail!("observation contract declares {observation_version:?}");
    }

    // Every state the classifier can produce must be nameable in the contract.
    let Some(states) = receipt_schema["properties"]["state"]["enum"].as_array() else {
        bail!("receipt contract does not enumerate the state vocabulary");
    };
    let declared: Vec<&str> = states.iter().filter_map(Value::as_str).collect();
    for expected in [
        "available_exact",
        "available_identity_not_proven",
        "listing_missing_version_retrievable",
        "extension_missing",
        "namespace_or_publisher_problem",
        "provider_not_proven",
        "invalid",
    ] {
        if !declared.contains(&expected) {
            bail!("receipt contract omits the {expected} state");
        }
    }
    if declared.len() != 7 {
        bail!("receipt contract declares an unexpected state vocabulary: {declared:?}");
    }
    Ok(())
}
