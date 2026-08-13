use serde_json::Value;
use xtask::actual_host_receipt::{
    RegistrationState, ValidationPolicy, validate_receipt, validate_receipt_with_policy,
};

fn valid_fixture() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/actual_host_receipts/valid-eglot-manual.json"
    ))
    .unwrap_or_else(|error| panic!("valid fixture must parse: {error}"))
}

#[test]
fn valid_manual_eglot_receipt_is_accepted() {
    let receipt = valid_fixture();
    assert_eq!(validate_receipt(&receipt), Ok(()));
}

#[test]
fn missing_orphan_result_is_rejected_with_stable_path() {
    let receipt: Value = serde_json::from_str(include_str!(
        "fixtures/actual_host_receipts/invalid-missing-orphan.json"
    ))
    .unwrap_or_else(|error| panic!("invalid fixture must still parse as JSON: {error}"));

    let error = validate_receipt(&receipt).unwrap_err();
    assert_eq!(
        error.to_string(),
        "receipt.state_machine.orphan_result: missing required field"
    );
}

#[test]
fn unobserved_pass_cannot_create_a_false_green() {
    let mut receipt = valid_fixture();
    receipt["features"]["diagnostics"]["observed"] = Value::Bool(false);

    let error = validate_receipt(&receipt).unwrap_err();
    assert_eq!(
        error.to_string(),
        "receipt.features.diagnostics: outcome=passed requires observed=true"
    );
}

#[test]
fn skipped_required_cell_needs_classification_and_reason() {
    let mut receipt = valid_fixture();
    receipt["features"]["diagnostics"]["observed"] = Value::Bool(false);
    receipt["features"]["diagnostics"]["outcome"] = Value::String("skipped".into());

    let error = validate_receipt(&receipt).unwrap_err();
    assert_eq!(
        error.to_string(),
        "receipt.features.diagnostics.skip_classification: missing required field"
    );
}

#[test]
fn extension_keys_must_be_namespaced() {
    let mut receipt = valid_fixture();
    receipt["extensions"]["major_mode"] = Value::String("perl-mode".into());

    let error = validate_receipt(&receipt).unwrap_err();
    assert_eq!(
        error.to_string(),
        "receipt.extensions: key `major_mode` must be namespaced"
    );
}

#[test]
fn manual_registration_cannot_satisfy_released_builtin_evidence() {
    let receipt = valid_fixture();
    let policy = ValidationPolicy {
        minimum_registration_state: Some(RegistrationState::UpstreamBuiltinReleased),
    };

    let error = validate_receipt_with_policy(&receipt, policy).unwrap_err();
    assert_eq!(
        error.to_string(),
        "receipt.registration_state: `manual_client_registration` cannot satisfy required `upstream_builtin_released` evidence"
    );
}
