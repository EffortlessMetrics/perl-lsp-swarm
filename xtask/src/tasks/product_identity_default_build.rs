//! Prove the canonical product identities are present in the default Cargo build graph.
//!
//! Source and workspace-member checks establish which packages Cargo resolves. This companion
//! check closes the activation seam: the facade implementation dependency must not be optional
//! and disabled, and governed binaries must not exist only behind non-default required features.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const CONTRACT_PATH: &str = "policy/product-identity.toml";

#[derive(Debug, Deserialize)]
struct ProductIdentityContract {
    server: ServerIdentity,
    debug_adapter: DebugAdapterIdentity,
}

#[derive(Debug, Deserialize)]
struct ServerIdentity {
    primary_executable: String,
    package_manifest: PathBuf,
    implementation_crate: String,
    implementation_manifest: PathBuf,
    compatibility_executable: String,
}

#[derive(Debug, Deserialize)]
struct DebugAdapterIdentity {
    executable: String,
    package_manifest: PathBuf,
}

#[derive(Debug, Default)]
struct DefaultFeatureState {
    features: BTreeSet<String>,
    dependencies: BTreeSet<String>,
}

pub(super) fn check(repo_root: &Path) -> Result<()> {
    let contract = load_contract(repo_root)?;
    let workspace = read_toml(repo_root, Path::new("Cargo.toml"))?;
    let facade = read_toml(repo_root, &contract.server.package_manifest)?;

    validate_default_implementation_dependency(
        &workspace,
        &facade,
        &contract.server.implementation_crate,
    )?;
    validate_default_binary(
        repo_root,
        "primary server",
        &contract.server.package_manifest,
        &contract.server.primary_executable,
    )?;
    validate_default_binary(
        repo_root,
        "server compatibility",
        &contract.server.implementation_manifest,
        &contract.server.compatibility_executable,
    )?;
    validate_default_binary(
        repo_root,
        "debug adapter",
        &contract.debug_adapter.package_manifest,
        &contract.debug_adapter.executable,
    )?;

    println!(
        "Product identity default build: implementation {}, binaries [{}, {}, {}]",
        contract.server.implementation_crate,
        contract.server.primary_executable,
        contract.server.compatibility_executable,
        contract.debug_adapter.executable
    );
    Ok(())
}

fn validate_default_implementation_dependency(
    workspace: &toml::Value,
    facade: &toml::Value,
    implementation_crate: &str,
) -> Result<()> {
    let dependencies = facade
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| eyre!("primary server manifest has no [dependencies] table"))?;
    let default_state = default_feature_state(facade)?;
    let workspace_dependencies = workspace
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table);
    let mut matching_dependency = false;

    for (alias, member_specification) in dependencies {
        let effective_specification = if inherits_workspace(member_specification) {
            workspace_dependencies
                .and_then(|dependencies| dependencies.get(alias))
                .ok_or_else(|| eyre!("workspace dependency {alias:?} is missing"))?
        } else {
            member_specification
        };
        if dependency_package_name(alias, effective_specification) != implementation_crate {
            continue;
        }
        matching_dependency = true;

        let optional = dependency_optional(member_specification)
            || dependency_optional(effective_specification);
        if !optional || default_state.dependencies.contains(alias) {
            return Ok(());
        }
    }

    if matching_dependency {
        bail!(
            "primary server implementation dependency {implementation_crate:?} is optional but not enabled by default features"
        );
    }
    bail!(
        "primary server manifest has no dependency resolving to implementation crate {implementation_crate:?}"
    )
}

fn validate_default_binary(
    repo_root: &Path,
    label: &str,
    manifest_path: &Path,
    expected_binary: &str,
) -> Result<()> {
    let manifest = read_toml(repo_root, manifest_path)?;
    let default_state = default_feature_state(&manifest)?;
    let mut explicit_match = false;
    let mut unavailable_requirements = BTreeSet::new();

    if let Some(bins) = manifest.get("bin") {
        let bins = bins
            .as_array()
            .ok_or_else(|| eyre!("{label} Cargo bin targets are not an array"))?;
        for bin in bins {
            if bin.get("name").and_then(toml::Value::as_str) != Some(expected_binary) {
                continue;
            }
            explicit_match = true;
            let required = required_features(bin, label, expected_binary)?;
            if required.iter().all(|feature| default_state.features.contains(feature)) {
                return Ok(());
            }
            unavailable_requirements.extend(
                required
                    .into_iter()
                    .filter(|feature| !default_state.features.contains(feature)),
            );
        }
    }

    if explicit_match {
        bail!(
            "{label} binary {expected_binary:?} is unavailable in the default feature set; missing required features {unavailable_requirements:?}"
        );
    }

    if implicit_binary_exists(repo_root, manifest_path, &manifest, expected_binary)? {
        return Ok(());
    }
    bail!(
        "{label} manifest {} does not expose default-build binary {expected_binary:?}",
        manifest_path.display()
    )
}

fn default_feature_state(manifest: &toml::Value) -> Result<DefaultFeatureState> {
    let Some(features) = manifest.get("features") else {
        return Ok(DefaultFeatureState::default());
    };
    let features = features
        .as_table()
        .ok_or_else(|| eyre!("Cargo [features] value is not a table"))?;
    let mut state = DefaultFeatureState::default();
    let mut queue = VecDeque::new();

    if let Some(default) = features.get("default") {
        for entry in feature_entries(default, "features.default")? {
            activate_feature_entry(entry, features, &mut state, &mut queue);
        }
    }

    while let Some(feature) = queue.pop_front() {
        let Some(entries) = features.get(&feature) else {
            continue;
        };
        for entry in feature_entries(entries, &format!("features.{feature}"))? {
            activate_feature_entry(entry, features, &mut state, &mut queue);
        }
    }
    Ok(state)
}

fn activate_feature_entry(
    entry: &str,
    features: &toml::map::Map<String, toml::Value>,
    state: &mut DefaultFeatureState,
    queue: &mut VecDeque<String>,
) {
    if let Some(dependency) = entry.strip_prefix("dep:") {
        if !dependency.is_empty() {
            state.dependencies.insert(dependency.to_string());
        }
        return;
    }
    if let Some((dependency, _)) = entry.split_once('/') {
        let weak = dependency.ends_with('?');
        let dependency = dependency.trim_end_matches('?');
        if !weak && !dependency.is_empty() {
            state.dependencies.insert(dependency.to_string());
        }
        return;
    }

    if state.features.insert(entry.to_string()) {
        if features.contains_key(entry) {
            queue.push_back(entry.to_string());
        } else {
            state.dependencies.insert(entry.to_string());
        }
    }
}

fn feature_entries<'a>(value: &'a toml::Value, label: &str) -> Result<Vec<&'a str>> {
    value
        .as_array()
        .ok_or_else(|| eyre!("Cargo {label} must be an array"))?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .ok_or_else(|| eyre!("Cargo {label} contains a non-string feature entry"))
        })
        .collect()
}

fn required_features(bin: &toml::Value, label: &str, binary: &str) -> Result<Vec<String>> {
    let Some(required) = bin.get("required-features") else {
        return Ok(Vec::new());
    };
    required
        .as_array()
        .ok_or_else(|| eyre!("{label} binary {binary:?} required-features is not an array"))?
        .iter()
        .map(|feature| {
            feature
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| eyre!("{label} binary {binary:?} has a non-string required feature"))
        })
        .collect()
}

fn implicit_binary_exists(
    repo_root: &Path,
    manifest_path: &Path,
    manifest: &toml::Value,
    expected_binary: &str,
) -> Result<bool> {
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| eyre!("Cargo manifest has no [package] table"))?;
    if package
        .get("autobins")
        .and_then(toml::Value::as_bool)
        == Some(false)
    {
        return Ok(false);
    }
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre!("Cargo package has no string name"))?;
    let manifest_dir = repo_root.join(
        manifest_path
            .parent()
            .ok_or_else(|| eyre!("Cargo manifest path has no parent"))?,
    );
    if expected_binary == package_name && manifest_dir.join("src/main.rs").is_file() {
        return Ok(true);
    }
    Ok(manifest_dir.join("src/bin").join(format!("{expected_binary}.rs")).is_file()
        || manifest_dir.join("src/bin").join(expected_binary).join("main.rs").is_file())
}

fn inherits_workspace(specification: &toml::Value) -> bool {
    specification
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn dependency_optional(specification: &toml::Value) -> bool {
    specification
        .as_table()
        .and_then(|table| table.get("optional"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn dependency_package_name<'a>(alias: &'a str, specification: &'a toml::Value) -> &'a str {
    specification
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(toml::Value::as_str)
        .unwrap_or(alias)
}

fn load_contract(repo_root: &Path) -> Result<ProductIdentityContract> {
    let path = repo_root.join(CONTRACT_PATH);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading product identity contract {}", path.display()))?;
    toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing product identity contract {}", path.display()))
}

fn read_toml(repo_root: &Path, relative: &Path) -> Result<toml::Value> {
    let path = repo_root.join(relative);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading Cargo manifest {}", path.display()))?;
    toml::from_str(&raw).wrap_err_with(|| format!("parsing Cargo manifest {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{CONTRACT_PATH, check};
    use color_eyre::eyre::{Result, bail};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const CONTRACT: &str = r#"
[server]
primary_executable = "perllsp"
package_manifest = "crates/perllsp/Cargo.toml"
implementation_crate = "perl-lsp-rs"
implementation_manifest = "crates/perl-lsp-rs/Cargo.toml"
compatibility_executable = "perl-lsp"

[debug_adapter]
executable = "perl-dap"
package_manifest = "crates/perl-dap/Cargo.toml"
"#;

    #[test]
    fn default_product_graph_passes() -> Result<()> {
        let repo = fixture_repo()?;
        check(repo.path())
    }

    #[test]
    fn optional_disabled_implementation_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            &facade_manifest("perl-lsp-rs = { workspace = true, optional = true }", ""),
        )?;
        expect_failure(repo.path(), "optional but not enabled by default features")
    }

    #[test]
    fn optional_implementation_enabled_transitively_by_default_passes() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            &facade_manifest(
                "perl-lsp-rs = { workspace = true, optional = true }",
                "[features]\ndefault = [\"server\"]\nserver = [\"dep:perl-lsp-rs\"]\n",
            ),
        )?;
        check(repo.path())
    }

    #[test]
    fn primary_binary_hidden_by_required_feature_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            &facade_manifest_with_bin(
                "perl-lsp-rs = { workspace = true }",
                "required-features = [\"internal-only\"]\n",
                "[features]\ninternal-only = []\n",
            ),
        )?;
        expect_failure(repo.path(), "binary \"perllsp\" is unavailable in the default feature set")
    }

    #[test]
    fn compatibility_binary_hidden_by_required_feature_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write_binary_manifest(
            repo.path(),
            "crates/perl-lsp-rs",
            "perl-lsp-rs",
            "perl-lsp",
            "required-features = [\"internal-only\"]\n",
            "[features]\ninternal-only = []\n",
        )?;
        expect_failure(repo.path(), "binary \"perl-lsp\" is unavailable in the default feature set")
    }

    #[test]
    fn dap_binary_hidden_by_required_feature_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write_binary_manifest(
            repo.path(),
            "crates/perl-dap",
            "perl-dap",
            "perl-dap",
            "required-features = [\"internal-only\"]\n",
            "[features]\ninternal-only = []\n",
        )?;
        expect_failure(repo.path(), "binary \"perl-dap\" is unavailable in the default feature set")
    }

    #[test]
    fn required_binary_feature_enabled_by_default_passes() -> Result<()> {
        let repo = fixture_repo()?;
        write_binary_manifest(
            repo.path(),
            "crates/perl-dap",
            "perl-dap",
            "perl-dap",
            "required-features = [\"shipping\"]\n",
            "[features]\ndefault = [\"shipping\"]\nshipping = []\n",
        )?;
        check(repo.path())
    }

    fn fixture_repo() -> Result<TempDir> {
        let repo = TempDir::new()?;
        write(repo.path(), CONTRACT_PATH, CONTRACT)?;
        write(
            repo.path(),
            "Cargo.toml",
            "[workspace.dependencies]\nperl-lsp-rs = { path = \"crates/perl-lsp-rs\" }\n",
        )?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            &facade_manifest("perl-lsp-rs = { workspace = true }", ""),
        )?;
        write(repo.path(), "crates/perllsp/src/main.rs", "")?;
        write_binary_manifest(
            repo.path(),
            "crates/perl-lsp-rs",
            "perl-lsp-rs",
            "perl-lsp",
            "",
            "",
        )?;
        write_binary_manifest(
            repo.path(),
            "crates/perl-dap",
            "perl-dap",
            "perl-dap",
            "",
            "",
        )?;
        Ok(repo)
    }

    fn facade_manifest(dependency: &str, features: &str) -> String {
        facade_manifest_with_bin(dependency, "", features)
    }

    fn facade_manifest_with_bin(dependency: &str, bin_extra: &str, features: &str) -> String {
        format!(
            "[package]\nname = \"perllsp\"\n\n[[bin]]\nname = \"perllsp\"\n{bin_extra}\n[dependencies]\n{dependency}\n\n{features}"
        )
    }

    fn write_binary_manifest(
        root: &Path,
        relative: &str,
        package: &str,
        binary: &str,
        bin_extra: &str,
        features: &str,
    ) -> Result<()> {
        write(
            root,
            &format!("{relative}/Cargo.toml"),
            &format!(
                "[package]\nname = {package:?}\n\n[[bin]]\nname = {binary:?}\n{bin_extra}\n{features}"
            ),
        )?;
        write(root, &format!("{relative}/src/main.rs"), "")
    }

    fn expect_failure(repo: &Path, expected: &str) -> Result<()> {
        let error = match check(repo) {
            Ok(()) => bail!("default-build identity drift should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains(expected) {
            bail!("unexpected error: {error:#}");
        }
        Ok(())
    }

    fn write(root: &Path, relative: &str, content: &str) -> Result<()> {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }
}
