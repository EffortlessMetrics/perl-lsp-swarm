use super::common::ensure_version_matches;
use super::super::model::{GatePolicyFile, LintLedger, RustToolchainFile, RustVersion};
use super::super::read::{read_toml, read_toml_as, read_yaml_as, value_at};
use super::super::{CLIPPY_CONFIG, GATE_POLICY, LINT_LEDGER, ROOT_MANIFEST, RUST_TOOLCHAIN};
use color_eyre::eyre::{Result, bail, eyre};
use std::path::Path;
use toml::Value;

const TEST_CARVEOUTS: &[&str] = &[
    "allow-unwrap-in-tests",
    "allow-expect-in-tests",
    "allow-panic-in-tests",
    "allow-indexing-slicing-in-tests",
    "allow-dbg-in-tests",
];

pub(super) fn validate_policy_header(ledger: &LintLedger) -> Result<()> {
    if ledger.schema != 2 {
        bail!("{} schema must be 2", LINT_LEDGER);
    }
    if !ledger.policy.panic_free_tests {
        bail!("{} must set policy.panic_free_tests = true", LINT_LEDGER);
    }
    if ledger.policy.allow_test_carveouts {
        bail!("{} must set policy.allow_test_carveouts = false", LINT_LEDGER);
    }
    if ledger.policy.suppression_style != "expect-with-reason" {
        bail!("{} must set policy.suppression_style = \"expect-with-reason\"", LINT_LEDGER);
    }
    if ledger.policy.blanket_categories {
        bail!("{} must set policy.blanket_categories = false", LINT_LEDGER);
    }
    Ok(())
}

pub(super) fn validate_msrv_sources(
    root: &Path,
    cargo: &Value,
    ledger: &LintLedger,
) -> Result<()> {
    let cargo_msrv = value_at(cargo, &["workspace", "package", "rust-version"])?
        .as_str()
        .ok_or_else(|| eyre!("workspace.package.rust-version must be a string"))?;
    let expected = RustVersion::from_text(cargo_msrv)?;

    ensure_version_matches(LINT_LEDGER, expected, &ledger.msrv)?;

    let clippy = read_toml(root.join(CLIPPY_CONFIG))?;
    let clippy_msrv = clippy
        .get("msrv")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("{CLIPPY_CONFIG} must define msrv as a string"))?;
    ensure_version_matches(CLIPPY_CONFIG, expected, clippy_msrv)?;

    let toolchain: RustToolchainFile = read_toml_as(root.join(RUST_TOOLCHAIN))?;
    ensure_version_matches(RUST_TOOLCHAIN, expected, &toolchain.toolchain.channel)?;

    let gate_policy: GatePolicyFile = read_yaml_as(root.join(GATE_POLICY))?;
    ensure_version_matches(GATE_POLICY, expected, &gate_policy.global.toolchain.msrv)?;

    Ok(())
}

pub(super) fn validate_workspace_members_inherit_lints(
    root: &Path,
    cargo: &Value,
) -> Result<()> {
    let members = value_at(cargo, &["workspace", "members"])?
        .as_array()
        .ok_or_else(|| eyre!("workspace.members must be an array"))?;
    let mut missing = Vec::new();

    for member in members {
        let Some(member_path) = member.as_str() else {
            bail!("workspace.members must contain only strings");
        };
        let manifest_path = root.join(member_path).join(ROOT_MANIFEST);
        let manifest = read_toml(manifest_path.clone())?;
        let inherits = value_at(&manifest, &["lints", "workspace"])
            .and_then(|value| {
                value.as_bool().ok_or_else(|| eyre!("[lints].workspace must be boolean"))
            })
            .unwrap_or(false);
        if !inherits {
            missing.push(manifest_path.display().to_string());
        }
    }

    if !missing.is_empty() {
        bail!("workspace members missing [lints] workspace = true: {}", missing.join(", "));
    }
    Ok(())
}

pub(super) fn validate_clippy_config(root: &Path, ledger: &LintLedger) -> Result<()> {
    let path = root.join(CLIPPY_CONFIG);
    let config = read_toml(path.clone())?;
    validate_clippy_config_value(&config, ledger)
        .map_err(|err| eyre!("{}: {err}", path.display()))
}

pub(crate) fn validate_clippy_config_value(
    config: &Value,
    ledger: &LintLedger,
) -> Result<()> {
    if ledger.policy.allow_test_carveouts {
        bail!("lint policy must disable test carveouts before validating clippy.toml");
    }
    for carveout in TEST_CARVEOUTS {
        if config.get(*carveout).is_some() {
            bail!("clippy.toml must not contain test carveout {carveout}");
        }
    }
    Ok(())
}
