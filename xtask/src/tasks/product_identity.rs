//! Validate the canonical product, package, executable, extension, and DAP identity map.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CONTRACT_PATH: &str = "policy/product-identity.toml";
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductIdentityContract {
    schema_version: u32,
    product: ProductIdentity,
    server: ServerIdentity,
    extension: ExtensionIdentity,
    debug_adapter: DebugAdapterIdentity,
    #[serde(default)]
    conflicts: Vec<IdentityConflict>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductIdentity {
    name: String,
    public_repository: String,
    development_repository: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerIdentity {
    primary_executable: String,
    cargo_package: String,
    package_manifest: PathBuf,
    implementation_crate: String,
    implementation_manifest: PathBuf,
    compatibility_executable: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionIdentity {
    publisher: String,
    package_name: String,
    id: String,
    display_name: String,
    package_manifest: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DebugAdapterIdentity {
    executable: String,
    cargo_package: String,
    package_manifest: PathBuf,
    maturity: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityConflict {
    identity: String,
    relation: String,
    remediation: String,
}

/// Validate the canonical identity map against package and extension metadata.
pub fn check(repo_root: &Path) -> Result<()> {
    let contract = load_contract(repo_root)?;
    validate_contract(repo_root, &contract)?;

    println!(
        "Product identity check: {} -> {} (server {}, extension {}, DAP {})",
        contract.product.name,
        contract.product.public_repository,
        contract.server.primary_executable,
        contract.extension.id,
        contract.debug_adapter.executable
    );
    Ok(())
}

fn load_contract(repo_root: &Path) -> Result<ProductIdentityContract> {
    let path = repo_root.join(CONTRACT_PATH);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading product identity contract {}", path.display()))?;
    toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing product identity contract {}", path.display()))
}

fn validate_contract(repo_root: &Path, contract: &ProductIdentityContract) -> Result<()> {
    if contract.schema_version != SUPPORTED_SCHEMA_VERSION {
        bail!(
            "unsupported product identity schema version {}; expected {}",
            contract.schema_version,
            SUPPORTED_SCHEMA_VERSION
        );
    }

    require_non_empty("product.name", &contract.product.name)?;
    require_non_empty(
        "product.public_repository",
        &contract.product.public_repository,
    )?;
    require_non_empty(
        "product.development_repository",
        &contract.product.development_repository,
    )?;

    let expected_extension_id = format!(
        "{}.{}",
        contract.extension.publisher, contract.extension.package_name
    );
    if contract.extension.id != expected_extension_id {
        bail!(
            "extension id {:?} does not match publisher/package identity {:?}",
            contract.extension.id,
            expected_extension_id
        );
    }

    let public_repository_url = format!(
        "https://github.com/{}",
        contract.product.public_repository
    );
    let root_manifest = read_toml(repo_root, Path::new("Cargo.toml"))?;
    let workspace_repository = toml_string(
        &root_manifest,
        &["workspace", "package", "repository"],
        "workspace package repository",
    )?;
    require_equal(
        "workspace repository",
        workspace_repository,
        &public_repository_url,
    )?;

    validate_cargo_identity(
        repo_root,
        "primary server",
        &contract.server.package_manifest,
        &contract.server.cargo_package,
        &contract.server.primary_executable,
    )?;
    validate_cargo_identity(
        repo_root,
        "server implementation",
        &contract.server.implementation_manifest,
        &contract.server.implementation_crate,
        &contract.server.compatibility_executable,
    )?;
    validate_cargo_identity(
        repo_root,
        "debug adapter",
        &contract.debug_adapter.package_manifest,
        &contract.debug_adapter.cargo_package,
        &contract.debug_adapter.executable,
    )?;

    if contract.debug_adapter.maturity != "preview" {
        bail!(
            "debug adapter maturity must remain \"preview\" until behavior-backed promotion; found {:?}",
            contract.debug_adapter.maturity
        );
    }

    validate_extension(repo_root, &contract.extension, &public_repository_url)?;
    validate_conflicts(contract)?;
    Ok(())
}

fn validate_cargo_identity(
    repo_root: &Path,
    label: &str,
    manifest_path: &Path,
    expected_package: &str,
    expected_binary: &str,
) -> Result<()> {
    validate_relative_path(manifest_path)?;
    let manifest = read_toml(repo_root, manifest_path)?;
    let package_name = toml_string(&manifest, &["package", "name"], "Cargo package name")?;
    require_equal(
        &format!("{label} Cargo package"),
        package_name,
        expected_package,
    )?;

    let binary_names = cargo_binary_names(&manifest)?;
    if !binary_names.contains(expected_binary) {
        bail!(
            "{label} manifest {} does not declare binary {:?}; found {:?}",
            manifest_path.display(),
            expected_binary,
            binary_names
        );
    }
    Ok(())
}

fn validate_extension(
    repo_root: &Path,
    identity: &ExtensionIdentity,
    expected_repository_url: &str,
) -> Result<()> {
    validate_relative_path(&identity.package_manifest)?;
    let path = repo_root.join(&identity.package_manifest);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading VS Code extension manifest {}", path.display()))?;
    let manifest: JsonValue = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing VS Code extension manifest {}", path.display()))?;

    require_equal(
        "extension package name",
        json_string(&manifest, &["name"], "extension package name")?,
        &identity.package_name,
    )?;
    require_equal(
        "extension publisher",
        json_string(&manifest, &["publisher"], "extension publisher")?,
        &identity.publisher,
    )?;
    require_equal(
        "extension display name",
        json_string(&manifest, &["displayName"], "extension display name")?,
        &identity.display_name,
    )?;
    require_equal(
        "extension repository",
        json_string(
            &manifest,
            &["repository", "url"],
            "extension repository URL",
        )?,
        expected_repository_url,
    )?;
    Ok(())
}

fn validate_conflicts(contract: &ProductIdentityContract) -> Result<()> {
    let mut seen = BTreeSet::new();
    for conflict in &contract.conflicts {
        require_non_empty("conflict.identity", &conflict.identity)?;
        require_non_empty("conflict.relation", &conflict.relation)?;
        require_non_empty("conflict.remediation", &conflict.remediation)?;
        if !seen.insert(conflict.identity.as_str()) {
            bail!("duplicate product identity conflict {:?}", conflict.identity);
        }
    }

    let product_package_identity = format!("crates.io/{}", contract.product.name);
    let has_external_conflict = contract.conflicts.iter().any(|conflict| {
        conflict.identity == product_package_identity && conflict.relation == "different_project"
    });
    if !has_external_conflict {
        bail!(
            "product identity contract must classify {:?} as a different project",
            product_package_identity
        );
    }

    let primary_package_identity = format!("crates.io/{}", contract.server.cargo_package);
    if contract
        .conflicts
        .iter()
        .any(|conflict| conflict.identity == primary_package_identity)
    {
        bail!(
            "primary server package {:?} cannot also be classified as an identity conflict",
            primary_package_identity
        );
    }
    Ok(())
}

fn read_toml(repo_root: &Path, relative_path: &Path) -> Result<toml::Value> {
    validate_relative_path(relative_path)?;
    let path = repo_root.join(relative_path);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading TOML identity source {}", path.display()))?;
    toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing TOML identity source {}", path.display()))
}

fn cargo_binary_names(manifest: &toml::Value) -> Result<BTreeSet<&str>> {
    let bins = manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| eyre!("Cargo manifest does not declare any [[bin]] targets"))?;
    let mut names = BTreeSet::new();
    for bin in bins {
        let name = bin
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| eyre!("Cargo [[bin]] target is missing a string name"))?;
        names.insert(name);
    }
    Ok(names)
}

fn toml_string<'a>(
    value: &'a toml::Value,
    path: &[&str],
    description: &str,
) -> Result<&'a str> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .ok_or_else(|| eyre!("missing {description} at {}", path.join(".")))?;
    }
    current
        .as_str()
        .ok_or_else(|| eyre!("{description} at {} is not a string", path.join(".")))
}

fn json_string<'a>(
    value: &'a JsonValue,
    path: &[&str],
    description: &str,
) -> Result<&'a str> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .ok_or_else(|| eyre!("missing {description} at {}", path.join(".")))?;
    }
    current
        .as_str()
        .ok_or_else(|| eyre!("{description} at {} is not a string", path.join(".")))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("identity source path must be a non-empty repository-relative path: {path:?}");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        bail!("identity source path may not escape the repository: {path:?}");
    }
    Ok(())
}

fn require_equal(label: &str, found: &str, expected: &str) -> Result<()> {
    if found != expected {
        bail!("{label} drifted: found {found:?}, expected {expected:?}");
    }
    Ok(())
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;
    use color_eyre::eyre::{Result, bail};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const CONTRACT: &str = r#"
schema_version = 1

[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"

[server]
primary_executable = "perllsp"
cargo_package = "perllsp"
package_manifest = "crates/perllsp/Cargo.toml"
implementation_crate = "perl-lsp-rs"
implementation_manifest = "crates/perl-lsp-rs/Cargo.toml"
compatibility_executable = "perl-lsp"

[extension]
publisher = "EffortlessMetrics"
package_name = "perl-lsp-rs"
id = "EffortlessMetrics.perl-lsp-rs"
display_name = "Perl Language Server (perl-lsp)"
package_manifest = "vscode-extension/package.json"

[debug_adapter]
executable = "perl-dap"
cargo_package = "perl-dap"
package_manifest = "crates/perl-dap/Cargo.toml"
maturity = "preview"

[[conflicts]]
identity = "crates.io/perl-lsp"
relation = "different_project"
remediation = "Install perllsp."
"#;

    #[test]
    fn coherent_identity_contract_passes() -> Result<()> {
        let repo = fixture_repo()?;
        check(repo.path())
    }

    #[test]
    fn extension_identity_drift_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "vscode-extension/package.json",
            r#"{
  "name": "wrong-extension",
  "displayName": "Perl Language Server (perl-lsp)",
  "publisher": "EffortlessMetrics",
  "repository": {"url": "https://github.com/EffortlessMetrics/perl-lsp"}
}"#,
        )?;

        let error = match check(repo.path()) {
            Ok(()) => bail!("extension identity drift should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains("extension package name drifted") {
            bail!("unexpected error: {error:#}");
        }
        Ok(())
    }

    #[test]
    fn missing_primary_binary_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            "[package]\nname = \"perllsp\"\n\n[[bin]]\nname = \"wrong\"\n",
        )?;

        let error = match check(repo.path()) {
            Ok(()) => bail!("missing primary binary should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains("does not declare binary \"perllsp\"") {
            bail!("unexpected error: {error:#}");
        }
        Ok(())
    }

    fn fixture_repo() -> Result<TempDir> {
        let repo = TempDir::new()?;
        write(repo.path(), "policy/product-identity.toml", CONTRACT)?;
        write(
            repo.path(),
            "Cargo.toml",
            "[workspace.package]\nrepository = \"https://github.com/EffortlessMetrics/perl-lsp\"\n",
        )?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            "[package]\nname = \"perllsp\"\n\n[[bin]]\nname = \"perllsp\"\n",
        )?;
        write(
            repo.path(),
            "crates/perl-lsp-rs/Cargo.toml",
            "[package]\nname = \"perl-lsp-rs\"\n\n[[bin]]\nname = \"perl-lsp\"\n",
        )?;
        write(
            repo.path(),
            "crates/perl-dap/Cargo.toml",
            "[package]\nname = \"perl-dap\"\n\n[[bin]]\nname = \"perl-dap\"\n",
        )?;
        write(
            repo.path(),
            "vscode-extension/package.json",
            r#"{
  "name": "perl-lsp-rs",
  "displayName": "Perl Language Server (perl-lsp)",
  "publisher": "EffortlessMetrics",
  "repository": {"url": "https://github.com/EffortlessMetrics/perl-lsp"}
}"#,
        )?;
        Ok(repo)
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
