use super::super::model::{LintLedger, RustVersion};
use super::super::validate::{ensure_version_matches, validate_clippy_config_value};
use super::ledger_with;
use color_eyre::eyre::{Result, bail};
use toml::Value;

#[test]
fn clippy_test_carveouts_are_rejected() -> Result<()> {
    let config = toml::from_str::<Value>(
        r#"
        msrv = "1.95"
        allow-panic-in-tests = true
        "#,
    )?;
    let ledger = ledger_with(Vec::new());

    let result = validate_clippy_config_value(&config, &ledger);
    let Err(error) = result else {
        bail!("test carveout should fail");
    };
    assert!(error.to_string().contains("allow-panic-in-tests"));
    Ok(())
}

#[test]
fn msrv_drift_is_rejected() -> Result<()> {
    let expected = RustVersion::from_text("1.95")?;
    let result = ensure_version_matches("rust-toolchain.toml", expected, "1.94.1");
    let Err(error) = result else {
        bail!("MSRV drift should fail");
    };
    assert!(error.to_string().contains("does not match product version"));
    Ok(())
}

#[test]
fn malformed_or_overlong_versions_are_rejected() {
    assert!(RustVersion::from_text("1").is_err());
    assert!(RustVersion::from_text("1.95.0.1").is_err());
    assert!(RustVersion::from_text("1.x").is_err());
}

#[test]
fn unknown_ledger_fields_fail_closed() {
    let result = toml::from_str::<LintLedger>(
        r#"
        schema = 2
        msrv = "1.95"
        unexpected = true

        [policy]
        panic_free_tests = true
        allow_test_carveouts = false
        suppression_style = "expect-with-reason"
        blanket_categories = false
        "#,
    );
    assert!(result.is_err());
}
