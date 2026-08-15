use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::tasks::gates::{GatePolicy, load_policy_for_inspection};
use crate::utils::project_root;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GatePolicyProfile {
    Pr,
    Nightly,
    Release,
}

#[derive(Debug, Deserialize)]
struct RegistryFile {
    #[serde(rename = "gate", default)]
    gates: Vec<RegistryGate>,
}

#[derive(Debug, Deserialize)]
struct RegistryGate {
    id: String,
    blocking: bool,
}

pub fn check() -> Result<()> {
    let root = project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    let registry = load_registry(&root.join(".ci/GATE_REGISTRY.toml"))?;

    validate_msrv_authorities(&root, &policy)?;

    // CI Gate runs `cargo xtask gates` using `.ci/gate-policy.yaml`.
    // Ensure PR profile cannot be blocked by CPAN/parser ratchet wiring.
    let pr_effective = effective_required_gate_names(&policy, GatePolicyProfile::Pr)?;
    assert_required(&pr_effective, "common_corpus_clean")?;
    assert_not_required(&pr_effective, "cpan_corpus_ratchet")?;
    assert_not_required(&pr_effective, "parser_corpus_ratchet")?;

    // Keep legacy registry aligned for human readers and secondary tooling.
    assert_registry_not_blocking(&registry, "cpan-corpus-ratchet")?;
    assert_registry_not_blocking(&registry, "parser-corpus-ratchet")?;

    println!("✅ Gate policy check passed.");
    println!("   Source of truth: .ci/gate-policy.yaml (used by `cargo xtask gates`).");
    println!("   PR required includes common_corpus_clean, excludes CPAN/parser ratchets.");

    Ok(())
}

#[derive(Debug, Deserialize)]
struct RustToolchainFile {
    toolchain: RustToolchain,
}

#[derive(Debug, Deserialize)]
struct RustToolchain {
    channel: String,
}

fn validate_msrv_authorities(root: &Path, policy: &GatePolicy) -> Result<()> {
    let cargo_path = root.join("Cargo.toml");
    let cargo = read_toml(&cargo_path)?;
    let cargo_msrv = cargo
        .get("workspace")
        .and_then(|value| value.get("package"))
        .and_then(|value| value.get("rust-version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre!("workspace.package.rust-version must be a string"))?;
    let gate_msrv = policy
        .global
        .toolchain
        .as_ref()
        .and_then(|toolchain| toolchain.msrv.as_deref())
        .ok_or_else(|| eyre!(".ci/gate-policy.yaml global.toolchain.msrv is required"))?;
    let toolchain_path = root.join("rust-toolchain.toml");
    let toolchain: RustToolchainFile = read_toml_as(&toolchain_path)?;

    validate_msrv_values(cargo_msrv, gate_msrv, &toolchain.toolchain.channel)?;
    validate_matrix_msrv_legs(cargo_msrv, &matrix_toolchain_legs(policy))
}

/// Collect every pinned toolchain version declared by a gate matrix leg.
///
/// `GateDefinition::matrix` is deserialized as an untyped YAML value, so the
/// legs are read structurally here rather than by widening the shared policy
/// model. Named channels (`stable`, `beta`, `nightly`, ...) carry no MSRV
/// claim and are skipped; only numeric pins are returned.
fn matrix_toolchain_legs(policy: &GatePolicy) -> Vec<(String, String)> {
    let mut legs = Vec::new();
    for gate in &policy.gates {
        let Some(toolchains) = gate
            .matrix
            .as_ref()
            .and_then(|matrix| matrix.get("toolchain"))
            .and_then(serde_yaml_ng::Value::as_sequence)
        else {
            continue;
        };
        for entry in toolchains {
            let Some(value) = entry.as_str() else {
                continue;
            };
            if value.starts_with(|ch: char| ch.is_ascii_digit()) {
                legs.push((gate.name.clone(), value.to_owned()));
            }
        }
    }
    legs
}

fn validate_matrix_msrv_legs(cargo: &str, legs: &[(String, String)]) -> Result<()> {
    for (gate, value) in legs {
        if compare_versions(cargo, value)? != 0 {
            bail!(
                "Cargo.toml workspace.package.rust-version ({cargo}) must match \
                 .ci/gate-policy.yaml gates[{gate}].matrix.toolchain pin ({value})"
            );
        }
    }
    Ok(())
}

fn validate_msrv_values(cargo: &str, gate_policy: &str, rust_toolchain: &str) -> Result<()> {
    for (source, value) in [
        (".ci/gate-policy.yaml toolchain.msrv", gate_policy),
        ("rust-toolchain.toml toolchain.channel", rust_toolchain),
    ] {
        if compare_versions(cargo, value)? != 0 {
            bail!(
                "Cargo.toml workspace.package.rust-version ({cargo}) must match {source} ({value})"
            );
        }
    }
    Ok(())
}

fn compare_versions(left: &str, right: &str) -> Result<i8> {
    let left_parts = version_parts(left)?;
    let right_parts = version_parts(right)?;
    for (left_part, right_part) in left_parts.iter().zip(right_parts.iter()) {
        if left_part > right_part {
            return Ok(1);
        }
        if left_part < right_part {
            return Ok(-1);
        }
    }
    Ok(0)
}

fn version_parts(version: &str) -> Result<Vec<u64>> {
    let mut parts = Vec::new();
    for part in version.split('.') {
        parts.push(
            part.parse::<u64>()
                .map_err(|err| eyre!("invalid version component {part} in {version}: {err}"))?,
        );
    }
    while parts.len() < 3 {
        parts.push(0);
    }
    Ok(parts)
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    let content = fs::read_to_string(path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

fn read_toml_as<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(path)
        .map_err(|err| eyre!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| eyre!("failed to parse {}: {err}", path.display()))
}

pub fn effective(profile: GatePolicyProfile) -> Result<()> {
    let root = project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    let required = effective_required_gate_names(&policy, profile)?;
    let advisory = effective_advisory_gate_names(&policy, profile)?;

    println!("Source of truth: .ci/gate-policy.yaml");
    println!("Profile: {}", profile_label(profile));
    println!("Required gates ({}):", required.len());
    for gate in &required {
        println!("  - {gate}");
    }

    println!("Advisory gates ({}):", advisory.len());
    for gate in &advisory {
        println!("  - {gate}");
    }

    Ok(())
}

fn profile_label(profile: GatePolicyProfile) -> &'static str {
    match profile {
        GatePolicyProfile::Pr => "pr",
        GatePolicyProfile::Nightly => "nightly",
        GatePolicyProfile::Release => "release",
    }
}

fn load_registry(path: &Path) -> Result<RegistryFile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read gate registry from {}", path.display()))?;
    let registry: RegistryFile = toml::from_str(&content)
        .with_context(|| format!("Failed to parse gate registry from {}", path.display()))?;
    Ok(registry)
}

fn effective_required_gate_names(
    policy: &GatePolicy,
    profile: GatePolicyProfile,
) -> Result<Vec<String>> {
    let mut names = effective_gate_names(policy, profile, true)?;
    names.sort();
    Ok(names)
}

fn effective_advisory_gate_names(
    policy: &GatePolicy,
    profile: GatePolicyProfile,
) -> Result<Vec<String>> {
    let mut names = effective_gate_names(policy, profile, false)?;
    names.sort();
    Ok(names)
}

fn effective_gate_names(
    policy: &GatePolicy,
    profile: GatePolicyProfile,
    required: bool,
) -> Result<Vec<String>> {
    let allowed_tiers = match profile {
        GatePolicyProfile::Pr => ["pr_fast", "merge_gate"].as_slice(),
        GatePolicyProfile::Nightly => ["pr_fast", "merge_gate", "nightly"].as_slice(),
        GatePolicyProfile::Release => ["release"].as_slice(),
    };

    for tier in allowed_tiers {
        if !policy.tiers.contains_key(*tier) {
            bail!("Policy missing required tier '{tier}' for profile {}", profile_label(profile));
        }
    }

    Ok(policy
        .gates
        .iter()
        .filter(|gate| allowed_tiers.contains(&gate.tier.as_str()) && gate.required == required)
        .map(|gate| gate.name.clone())
        .collect())
}

fn assert_required(required: &[String], gate_name: &str) -> Result<()> {
    if required.iter().any(|name| name == gate_name) {
        Ok(())
    } else {
        bail!("Gate '{gate_name}' must be required in PR profile")
    }
}

fn assert_not_required(required: &[String], gate_name: &str) -> Result<()> {
    if required.iter().any(|name| name == gate_name) {
        bail!("Gate '{gate_name}' must not be required in PR profile")
    } else {
        Ok(())
    }
}

fn assert_registry_not_blocking(registry: &RegistryFile, gate_id: &str) -> Result<()> {
    let by_id: HashMap<&str, bool> =
        registry.gates.iter().map(|gate| (gate.id.as_str(), gate.blocking)).collect();

    match by_id.get(gate_id) {
        Some(true) => bail!("Registry gate '{gate_id}' must be non-blocking"),
        Some(false) => Ok(()),
        None => bail!("Registry gate '{gate_id}' missing; keep registry aligned with policy"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_enforces_cpan_non_blocking_for_pr_profile() -> Result<()> {
        check()
    }

    #[test]
    fn effective_pr_marks_common_required_and_cpan_advisory() -> Result<()> {
        let root = project_root()?;
        let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;

        let required = effective_required_gate_names(&policy, GatePolicyProfile::Pr)?;
        let advisory = effective_advisory_gate_names(&policy, GatePolicyProfile::Pr)?;

        assert!(required.iter().any(|name| name == "common_corpus_clean"));
        assert!(!required.iter().any(|name| name == "cpan_corpus_ratchet"));
        assert!(advisory.iter().any(|name| name == "cpan_corpus_ratchet"));
        Ok(())
    }

    #[test]
    fn msrv_accepts_equivalent_patch_precision() -> Result<()> {
        validate_msrv_values("1.95", "1.95.0", "1.95.0")
    }

    #[test]
    fn msrv_rejects_gate_policy_drift() -> Result<()> {
        let error = validate_msrv_values("1.95", "1.92.0", "1.95.0")
            .expect_err("gate-policy MSRV drift should be rejected");
        assert!(error.to_string().contains("gate-policy.yaml"));
        Ok(())
    }

    #[test]
    fn msrv_rejects_rust_toolchain_drift() -> Result<()> {
        let error = validate_msrv_values("1.95", "1.95.0", "1.92.0")
            .expect_err("rust-toolchain MSRV drift should be rejected");
        assert!(error.to_string().contains("rust-toolchain.toml"));
        Ok(())
    }

    #[test]
    fn msrv_rejects_matrix_leg_drift() -> Result<()> {
        let legs = [("full_matrix".to_owned(), "1.92.0".to_owned())];
        let error = validate_matrix_msrv_legs("1.95", &legs)
            .expect_err("matrix-only MSRV drift should be rejected");
        let message = error.to_string();
        assert!(
            message.contains("gates[full_matrix].matrix.toolchain"),
            "error must name the drifted matrix leg; got {message}"
        );
        Ok(())
    }

    #[test]
    fn msrv_matrix_legs_ignore_named_channels() -> Result<()> {
        // `stable`/`beta` legs float by design and carry no MSRV claim, so the
        // collector must not turn them into version-comparison failures.
        let legs = [("full_matrix".to_owned(), "1.95.0".to_owned())];
        validate_matrix_msrv_legs("1.95", &legs)
    }

    /// Guards against a vacuous matrix check: if the collector ever stops
    /// finding the pinned leg, `validate_matrix_msrv_legs` would pass on an
    /// empty list and the authority would silently lose its protection.
    #[test]
    fn msrv_matrix_collector_finds_the_real_pinned_leg() -> Result<()> {
        let root = project_root()?;
        let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;

        let legs = matrix_toolchain_legs(&policy);
        assert!(
            !legs.is_empty(),
            ".ci/gate-policy.yaml must declare at least one pinned matrix toolchain leg; \
             an empty collection makes the MSRV matrix check vacuous"
        );
        assert!(
            legs.iter().all(|(_, value)| !value.starts_with("stable")
                && !value.starts_with("beta")
                && !value.starts_with("nightly")),
            "collector must skip named channels; got {legs:?}"
        );
        Ok(())
    }
}
