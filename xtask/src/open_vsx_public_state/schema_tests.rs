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
    let emitted = serde_json::to_value(receipt(AVAILABLE_EXACT)?)?;
    validate(&validator, &emitted, "emitted")?;

    // No observation can produce `available_exact` any more, so the shape is
    // built here rather than emitted. The contract still has to describe it for
    // #9138, which holds the resolved candidate authority this tool does not.
    let mut baseline = emitted;
    baseline["state"] = Value::String("available_exact".to_owned());
    baseline["blockers"] = serde_json::json!([]);
    validate(&validator, &baseline, "constructed available_exact")?;

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
fn the_observation_contract_binds_activity_evidence_to_an_attempted_request() -> Result<()> {
    // Raised in review: the contract coupled `status` and `elapsed_ms` to the
    // outcome but left `response_bytes`, `truncated`, `error_kind` and
    // `redirects` free, so a producer following the published contract alone
    // could emit an unattempted request that also reported bytes or an error —
    // a shape the classifier refuses as `unattempted_request_reports_activity`.
    // The schema is the durable artifact; a later rewrite that dropped the
    // redundant code guard would otherwise start admitting these silently.
    let validator = validator(OBSERVATION_SCHEMA)?;
    let baseline: Value = serde_json::from_str(AVAILABLE_EXACT)?;
    validate(&validator, &baseline, "baseline")?;

    // An unattempted request, otherwise well formed: the activity fields below
    // are the only thing under test.
    let unattempted = {
        let mut document = baseline.clone();
        let transport = &mut document["cells"]["listing"]["transport"];
        transport["outcome"] = Value::String("not_attempted".to_owned());
        transport["status"] = Value::Null;
        transport["elapsed_ms"] = Value::Null;
        transport["response_bytes"] = Value::Null;
        document
    };
    validate(&validator, &unattempted, "unattempted baseline")?;

    let activity: &[(&str, Value)] = &[
        ("bytes", serde_json::json!(0)),
        ("an error", serde_json::json!("timeout")),
        ("a truncated read", serde_json::json!(true)),
        ("a redirect", serde_json::json!(1)),
    ];
    let fields = ["response_bytes", "error_kind", "truncated", "redirects"];
    for ((label, value), field) in activity.iter().zip(fields) {
        let mut reports_activity = unattempted.clone();
        reports_activity["cells"]["listing"]["transport"][field] = value.clone();
        if validator.is_valid(&reports_activity) {
            bail!("an unattempted request reporting {label} passed validation");
        }
    }

    // The converse must NOT hold. Each shape below is one the classifier
    // accepts, so a contract that required an attempted request to report bytes
    // or an error would refuse observations the code reads happily — including
    // the affirmative `404` that the whole absence path is built on.
    let mut absent = baseline.clone();
    absent["cells"]["listing"]["transport"]["status"] = serde_json::json!(404);
    absent["cells"]["listing"]["transport"]["response_bytes"] = Value::Null;
    validate(&validator, &absent, "404 with no byte count")?;

    let mut unreachable = baseline.clone();
    let transport = &mut unreachable["cells"]["listing"]["transport"];
    transport["outcome"] = Value::String("transport_error".to_owned());
    transport["status"] = Value::Null;
    transport["response_bytes"] = Value::Null;
    transport["error_kind"] = Value::String("dns".to_owned());
    validate(&validator, &unreachable, "transport error that read nothing")?;

    let mut truncated_without_a_named_error = baseline;
    truncated_without_a_named_error["cells"]["listing"]["transport"]["truncated"] =
        Value::Bool(true);
    validate(&validator, &truncated_without_a_named_error, "truncated read with no error_kind")?;
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

    // The published vocabulary is exactly what this classifier can emit, plus
    // `available_exact` — which the contract defines for consumers holding a
    // resolved candidate authority and which this classifier cannot produce.
    // Asserting the relationship rather than mere containment catches a state
    // added to either side, and catches this classifier gaining the ability to
    // claim exact approval without that being a deliberate decision.
    let Some(states) = receipt_schema["properties"]["state"]["enum"].as_array() else {
        bail!("receipt contract does not enumerate the state vocabulary");
    };
    let mut declared: Vec<&str> = states.iter().filter_map(Value::as_str).collect();
    let mut known: Vec<&str> =
        super::model::PublicState::ALL.into_iter().map(super::model::PublicState::key).collect();
    known.push("available_exact");
    declared.sort_unstable();
    known.sort_unstable();
    if declared != known {
        bail!(
            "contract states {declared:?} do not match the classifier's {known:?} plus \
             available_exact"
        );
    }
    Ok(())
}
