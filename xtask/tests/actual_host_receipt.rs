use serde_json::Value;
use std::error::Error;
use xtask::actual_host_receipt::{
    RegistrationState, ValidationPolicy, validate_receipt, validate_receipt_with_policy,
};

fn valid_fixture() -> Result<Value, serde_json::Error> {
    serde_json::from_str(include_str!("fixtures/actual_host_receipts/valid-eglot-manual.json"))
}

fn validation_error(
    result: Result<(), xtask::actual_host_receipt::ReceiptValidationError>,
) -> Result<String, Box<dyn Error>> {
    match result {
        Ok(()) => Err("receipt unexpectedly validated".into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn valid_manual_eglot_receipt_is_accepted() -> Result<(), Box<dyn Error>> {
    let receipt = valid_fixture()?;
    validate_receipt(&receipt)?;
    Ok(())
}

#[test]
fn missing_orphan_result_is_rejected_with_stable_path() -> Result<(), Box<dyn Error>> {
    let receipt: Value = serde_json::from_str(include_str!(
        "fixtures/actual_host_receipts/invalid-missing-orphan.json"
    ))?;

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.state_machine.orphan_result: missing required field"
    );
    Ok(())
}

#[test]
fn unobserved_pass_cannot_create_a_false_green() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["features"]["diagnostics"]["observed"] = Value::Bool(false);

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.features.diagnostics: outcome=passed requires observed=true"
    );
    Ok(())
}

#[test]
fn skipped_required_cell_needs_classification_and_reason() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["features"]["diagnostics"]["observed"] = Value::Bool(false);
    receipt["features"]["diagnostics"]["outcome"] = Value::String("skipped".into());

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.features.diagnostics.skip_classification: missing required field"
    );
    Ok(())
}

#[test]
fn extension_keys_must_be_namespaced() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["extensions"]["major_mode"] = Value::String("perl-mode".into());

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.extensions: key `major_mode` must be namespaced"
    );
    Ok(())
}

#[test]
fn manual_registration_cannot_satisfy_released_builtin_evidence() -> Result<(), Box<dyn Error>> {
    let receipt = valid_fixture()?;
    let policy = ValidationPolicy {
        minimum_registration_state: Some(RegistrationState::UpstreamBuiltinReleased),
    };

    assert_eq!(
        validation_error(validate_receipt_with_policy(&receipt, policy))?,
        "receipt.registration_state: `manual_client_registration` cannot satisfy required `upstream_builtin_released` evidence"
    );
    Ok(())
}

#[test]
fn receipt_version_must_be_one() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["receipt_version"] = Value::from(2);

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.receipt_version: expected `1`, found `2`"
    );
    Ok(())
}

#[test]
fn unknown_top_level_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["major_mode"] = Value::String("perl-mode".into());

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt: unknown field `major_mode`"
    );
    Ok(())
}

#[test]
fn single_character_extension_suffix_is_accepted() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["extensions"] = serde_json::json!({ "x.y": "ok" });
    validate_receipt(&receipt)?;
    Ok(())
}

#[test]
fn observed_without_advertised_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut receipt = valid_fixture()?;
    receipt["features"]["diagnostics"]["advertised"] = Value::Bool(false);
    receipt["features"]["diagnostics"]["observed"] = Value::Bool(true);
    receipt["features"]["diagnostics"]["outcome"] = Value::String("failed".into());

    assert_eq!(
        validation_error(validate_receipt(&receipt))?,
        "receipt.features.diagnostics: observed=true contradicts advertised=false"
    );
    Ok(())
}
