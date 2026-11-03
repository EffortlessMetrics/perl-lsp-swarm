use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use crate::tasks::gates::{GatePolicy, load_policy_for_inspection};
use crate::utils::project_root;

const CLIPPY_TESTS_KERNEL_GATE: &str = "clippy_tests_kernel";
const CLIPPY_TESTS_KERNEL_RESIDUAL_PACKAGES: &[(&str, &str)] = &[
    ("perl-evidence-envelope", "#15677"),
    ("perl-parser", "#15677"),
    ("perl-core-harness", "#15677"),
    ("perl-parser-core", "#15677"),
    ("perl-lsp-rs-core", "#15677"),
    ("perl-corpus", "#15677"),
    ("perl-ripr-facts", "#15677"),
    ("perl-lsp-rs", "#15677"),
    ("perl-lsp-ux-tests", "#15613"),
    ("perl-dap", "#15677"),
    ("xtask", "#15677"),
];

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

#[derive(Debug, PartialEq, Eq)]
struct ClippyAllTargetsPartition {
    workspace: BTreeSet<String>,
    strict: BTreeSet<String>,
    residual: BTreeMap<String, String>,
}

pub fn check() -> Result<()> {
    let root = project_root()?;
    let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
    let registry = load_registry(&root.join(".ci/GATE_REGISTRY.toml"))?;

    validate_msrv_authorities(&root, &policy)?;
    let clippy_partition = validate_clippy_all_targets_partition(&root, &policy)?;

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
    println!(
        "   clippy_tests_kernel: {}/{} strict packages; {} residual packages.",
        clippy_partition.strict.len(),
        clippy_partition.workspace.len(),
        clippy_partition.residual.len()
    );
    for (package, owner) in &clippy_partition.residual {
        println!("     - {package}: {owner}");
    }

    Ok(())
}

fn validate_clippy_all_targets_partition(
    root: &Path,
    policy: &GatePolicy,
) -> Result<ClippyAllTargetsPartition> {
    let matching: Vec<_> =
        policy.gates.iter().filter(|gate| gate.name == CLIPPY_TESTS_KERNEL_GATE).collect();
    let gate = match matching.as_slice() {
        [gate] => *gate,
        [] => bail!("gate policy is missing '{CLIPPY_TESTS_KERNEL_GATE}'"),
        _ => bail!("gate policy defines '{CLIPPY_TESTS_KERNEL_GATE}' more than once"),
    };

    validate_clippy_command_contract(&gate.command)?;
    let strict = package_selectors(&gate.command)?;
    let residual = residual_package_map()?;
    let workspace = workspace_package_names(root)?;

    let partition = validate_package_partition(workspace, strict, residual)?;
    // The human-facing denominator must be rendered from the same derived
    // partition, so a package moving between cohorts cannot silently leave a
    // stale prose count behind (the 37/47 drift this check exists for).
    validate_clippy_description_counts(
        &gate.description,
        partition.strict.len(),
        partition.workspace.len(),
    )?;
    Ok(partition)
}

fn validate_clippy_description_counts(
    description: &str,
    strict_count: usize,
    workspace_count: usize,
) -> Result<()> {
    let expected = format!("({strict_count}/{workspace_count} crates)");
    // A bare `contains` check would accept a description that carries the
    // current denominator next to a stale one, so every `(N/M crates)` token
    // must agree with the derived counts.
    let tokens = clippy_denominator_tokens(description);
    if tokens.is_empty() || tokens.iter().any(|token| *token != expected) {
        bail!(
            "'{CLIPPY_TESTS_KERNEL_GATE}' description must carry exactly the current cohort \
             denominator '{expected}' derived from the strict cohort plus the cargo-metadata \
             workspace set; update the counts whenever package membership moves. \
             Found count token(s) {tokens:?} in {description:?}"
        );
    }
    Ok(())
}

fn clippy_denominator_tokens(description: &str) -> Vec<&str> {
    description
        .match_indices('(')
        .filter_map(|(index, _)| {
            let candidate = &description[index..];
            let end = candidate.find(" crates)")? + " crates)".len();
            let token = &candidate[..end];
            let inner = &token[1..token.len() - " crates)".len()];
            let (strict, workspace) = inner.split_once('/')?;
            let digits =
                |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            (digits(strict) && digits(workspace) && !workspace.contains('/')).then_some(token)
        })
        .collect()
}

fn validate_clippy_command_contract(command: &str) -> Result<()> {
    // Token checks below operate on whitespace-split words, so any shell
    // syntax would let a chained command forge coverage tokens (e.g. flags
    // smuggled in via `echo`). The gate only accepts a single cargo clippy
    // invocation.
    const SHELL_SYNTAX: [&str; 5] = [";", "|", "&", "`", "$("];
    if let Some(operator) = SHELL_SYNTAX.iter().find(|operator| command.contains(**operator)) {
        bail!(
            "'{CLIPPY_TESTS_KERNEL_GATE}' must be a single cargo clippy invocation, found shell syntax '{operator}'"
        );
    }
    let words: Vec<_> = command.split_whitespace().collect();
    if !words.contains(&"--all-targets") {
        bail!("'{CLIPPY_TESTS_KERNEL_GATE}' must retain --all-targets");
    }
    if !words.contains(&"--locked") {
        bail!("'{CLIPPY_TESTS_KERNEL_GATE}' must retain --locked");
    }
    if !words.windows(2).any(|window| window[0] == "-D" && window[1] == "warnings") {
        bail!("'{CLIPPY_TESTS_KERNEL_GATE}' must retain '-D warnings'");
    }
    Ok(())
}

fn package_selectors(command: &str) -> Result<BTreeSet<String>> {
    let mut words = command.split_whitespace();
    let mut packages = BTreeSet::new();

    while let Some(word) = words.next() {
        let package = match word {
            "-p" | "--package" => Some(words.next().ok_or_else(|| {
                eyre!("'{CLIPPY_TESTS_KERNEL_GATE}' has a dangling {word} selector")
            })?),
            _ => word.strip_prefix("--package="),
        };

        let Some(package) = package else {
            continue;
        };
        if package.is_empty() {
            bail!("'{CLIPPY_TESTS_KERNEL_GATE}' contains an empty package selector");
        }
        if !packages.insert(package.to_owned()) {
            bail!("'{CLIPPY_TESTS_KERNEL_GATE}' selects package '{package}' more than once");
        }
    }

    if packages.is_empty() {
        bail!("'{CLIPPY_TESTS_KERNEL_GATE}' must select at least one package");
    }
    Ok(packages)
}

fn residual_package_map() -> Result<BTreeMap<String, String>> {
    let mut residual = BTreeMap::new();
    for &(package, owner) in CLIPPY_TESTS_KERNEL_RESIDUAL_PACKAGES {
        if package.trim().is_empty() || owner.trim().is_empty() {
            bail!("clippy all-target residual rows require non-empty package and owner values");
        }
        if residual.insert(package.to_owned(), owner.to_owned()).is_some() {
            bail!("clippy all-target residual package '{package}' is listed more than once");
        }
    }
    Ok(residual)
}

fn workspace_package_names(root: &Path) -> Result<BTreeSet<String>> {
    // Derive membership from `cargo metadata`, not the workspace.members
    // array: Cargo also admits in-tree path dependencies as workspace
    // members, so the array alone would undercount the denominator and let
    // unlisted packages escape classification.
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version=1"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    workspace_package_names_from_metadata(&metadata)
}

fn workspace_package_names_from_metadata(metadata: &serde_json::Value) -> Result<BTreeSet<String>> {
    let members: BTreeSet<&str> = metadata
        .get("workspace_members")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| eyre!("cargo metadata: expected workspace_members array"))?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| eyre!("cargo metadata: expected packages array"))?;

    let mut names = BTreeSet::new();
    for package in packages {
        let id = package.get("id").and_then(serde_json::Value::as_str);
        let name = package.get("name").and_then(serde_json::Value::as_str);
        let (Some(id), Some(name)) = (id, name) else {
            bail!("cargo metadata: packages entries require string id and name");
        };
        if members.contains(id) && !names.insert(name.to_owned()) {
            bail!("workspace package name '{name}' is declared more than once");
        }
    }

    Ok(names)
}

fn validate_package_partition(
    workspace: BTreeSet<String>,
    strict: BTreeSet<String>,
    residual: BTreeMap<String, String>,
) -> Result<ClippyAllTargetsPartition> {
    let residual_names: BTreeSet<_> = residual.keys().cloned().collect();
    let overlap: Vec<_> = strict.intersection(&residual_names).cloned().collect();
    if !overlap.is_empty() {
        bail!("clippy all-target packages cannot be both strict and residual: {overlap:?}");
    }

    let unknown_strict: Vec<_> = strict.difference(&workspace).cloned().collect();
    if !unknown_strict.is_empty() {
        bail!("clippy_tests_kernel selects unknown workspace packages: {unknown_strict:?}");
    }
    let unknown_residual: Vec<_> = residual_names.difference(&workspace).cloned().collect();
    if !unknown_residual.is_empty() {
        bail!(
            "clippy all-target residual rows name unknown workspace packages: {unknown_residual:?}"
        );
    }

    let classified: BTreeSet<_> = strict.union(&residual_names).cloned().collect();
    let missing: Vec<_> = workspace.difference(&classified).cloned().collect();
    if !missing.is_empty() {
        bail!("workspace packages lack a clippy all-target disposition: {missing:?}");
    }

    Ok(ClippyAllTargetsPartition { workspace, strict, residual })
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

    fn package_set(packages: &[&str]) -> BTreeSet<String> {
        packages.iter().map(|package| (*package).to_owned()).collect()
    }

    fn residual_map(packages: &[(&str, &str)]) -> BTreeMap<String, String> {
        packages
            .iter()
            .map(|(package, owner)| ((*package).to_owned(), (*owner).to_owned()))
            .collect()
    }

    #[test]
    fn check_enforces_cpan_non_blocking_for_pr_profile() -> Result<()> {
        check()
    }

    #[test]
    fn clippy_kernel_partition_matches_current_workspace() -> Result<()> {
        let root = project_root()?;
        let policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
        let partition = validate_clippy_all_targets_partition(&root, &policy)?;

        // Exact current-workspace census: any package movement must update
        // these integers consciously, together with the gate description's
        // "(strict/workspace crates)" denominator (checked mechanically in
        // validate_clippy_all_targets_partition).
        assert_eq!(partition.workspace.len(), 48);
        assert_eq!(partition.strict.len(), 37);
        assert_eq!(partition.residual.len(), 11);
        let residual_names: BTreeSet<_> = partition.residual.keys().cloned().collect();
        assert!(partition.strict.is_disjoint(&residual_names));
        assert_eq!(partition.strict.len() + residual_names.len(), partition.workspace.len());
        assert_eq!(partition.residual.get("perl-lsp-ux-tests").map(String::as_str), Some("#15613"));
        Ok(())
    }

    #[test]
    fn clippy_description_rejects_stale_denominator() -> Result<()> {
        let root = project_root()?;
        let mut policy = load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
        let partition = validate_clippy_all_targets_partition(&root, &policy)?;

        let strict = partition.strict.len();
        let workspace = partition.workspace.len();
        let current = format!("({strict}/{workspace} crates)");
        let stale_workspace = workspace - 1;
        let stale = format!("({strict}/{stale_workspace} crates)");
        let gate = policy
            .gates
            .iter_mut()
            .find(|gate| gate.name == CLIPPY_TESTS_KERNEL_GATE)
            .ok_or_else(|| eyre!("'{CLIPPY_TESTS_KERNEL_GATE}' gate missing"))?;
        gate.description = gate.description.replace(&current, &stale);

        let error = match validate_clippy_all_targets_partition(&root, &policy) {
            Ok(_) => bail!("stale description denominator must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cohort denominator"));
        Ok(())
    }

    #[test]
    fn clippy_description_counts_reject_missing_or_mismatched_denominator() -> Result<()> {
        validate_clippy_description_counts("staged cohort (2/3 crates): rest", 2, 3)?;

        for (description, strict, workspace) in [
            ("staged cohort (3/3 crates)", 2, 3),
            ("no counts here", 2, 3),
            // A current denominator next to a stale one must not pass merely
            // because the expected token is present somewhere in the prose.
            ("known-clean cohort (2/3 crates); current inventory (2/4 crates)", 2, 3),
            ("known-clean cohort (2/4 crates); current inventory (2/3 crates)", 2, 3),
        ] {
            let error = match validate_clippy_description_counts(description, strict, workspace) {
                Ok(()) => bail!("description '{description}' must fail"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("cohort denominator"));
        }
        Ok(())
    }

    #[test]
    fn package_selector_parser_accepts_supported_forms() -> Result<()> {
        let packages = package_selectors(
            "cargo clippy -p alpha --package beta --package=gamma --all-targets --locked -- -D warnings",
        )?;
        assert_eq!(packages, package_set(&["alpha", "beta", "gamma"]));
        Ok(())
    }

    #[test]
    fn package_selector_parser_rejects_duplicate_and_dangling_selectors() -> Result<()> {
        let duplicate = match package_selectors("cargo clippy -p alpha --package alpha") {
            Ok(_) => bail!("duplicate package selector must fail"),
            Err(error) => error,
        };
        assert!(duplicate.to_string().contains("more than once"));

        let dangling = match package_selectors("cargo clippy --package") {
            Ok(_) => bail!("dangling package selector must fail"),
            Err(error) => error,
        };
        assert!(dangling.to_string().contains("dangling"));
        Ok(())
    }

    #[test]
    fn clippy_command_contract_rejects_missing_load_bearing_flags() -> Result<()> {
        for (command, expected) in [
            ("cargo clippy -p alpha --locked -- -D warnings", "--all-targets"),
            ("cargo clippy -p alpha --all-targets -- -D warnings", "--locked"),
            ("cargo clippy -p alpha --all-targets --locked", "-D warnings"),
        ] {
            let error = match validate_clippy_command_contract(command) {
                Ok(()) => bail!("missing {expected} must fail"),
                Err(error) => error,
            };
            assert!(error.to_string().contains(expected));
        }
        Ok(())
    }

    #[test]
    fn clippy_command_contract_rejects_shell_syntax() -> Result<()> {
        for command in [
            "cargo clippy -p alpha --locked -- -D warnings && echo -p beta --all-targets",
            "cargo clippy -p alpha --all-targets --locked -- -D warnings; cargo clippy -p beta",
            "cargo clippy -p alpha --all-targets --locked -- -D warnings | tee clippy.log",
        ] {
            let error = match validate_clippy_command_contract(command) {
                Ok(()) => bail!("shell syntax in command must fail: {command}"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("shell"));
        }
        Ok(())
    }

    #[test]
    fn workspace_members_come_from_cargo_metadata() -> Result<()> {
        let metadata = serde_json::json!({
            "packages": [
                {"id": "alpha 0.1.0 (path+file:///repo/crates/alpha)", "name": "alpha"},
                {"id": "helper 0.1.0 (path+file:///repo/crates/helper)", "name": "helper"}
            ],
            "workspace_members": [
                "alpha 0.1.0 (path+file:///repo/crates/alpha)",
                "helper 0.1.0 (path+file:///repo/crates/helper)"
            ]
        });
        let names = workspace_package_names_from_metadata(&metadata)?;
        assert_eq!(names, ["alpha".to_owned(), "helper".to_owned()].into_iter().collect());
        Ok(())
    }

    #[test]
    fn package_partition_rejects_overlap_unknown_and_missing_rows() -> Result<()> {
        let workspace = package_set(&["alpha", "beta"]);

        let overlap = match validate_package_partition(
            workspace.clone(),
            package_set(&["alpha"]),
            residual_map(&[("alpha", "#1"), ("beta", "#2")]),
        ) {
            Ok(_) => bail!("overlapping package partition must fail"),
            Err(error) => error,
        };
        assert!(overlap.to_string().contains("both strict and residual"));

        let unknown = match validate_package_partition(
            workspace.clone(),
            package_set(&["alpha"]),
            residual_map(&[("gamma", "#3")]),
        ) {
            Ok(_) => bail!("unknown residual package must fail"),
            Err(error) => error,
        };
        assert!(unknown.to_string().contains("unknown workspace packages"));

        let missing =
            match validate_package_partition(workspace, package_set(&["alpha"]), BTreeMap::new()) {
                Ok(_) => bail!("unclassified workspace package must fail"),
                Err(error) => error,
            };
        assert!(missing.to_string().contains("lack a clippy all-target disposition"));
        Ok(())
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
