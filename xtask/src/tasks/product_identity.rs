//! Validate the canonical product, package, executable, extension, and DAP identity map.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let repository_context = resolve_repository_context(repo_root)?;
    check_with_repository_context(repo_root, repository_context.as_deref())
}

fn check_with_repository_context(
    repo_root: &Path,
    repository_context: Option<&str>,
) -> Result<()> {
    let contract = load_contract(repo_root)?;
    validate_contract(repo_root, &contract, repository_context)?;

    let context = repository_context.unwrap_or("unbound-local-checkout");
    println!(
        concat!(
            "Product identity check: {} -> {} (development {}, server {}, ",
            "extension {}, DAP {}, context {})"
        ),
        contract.product.name,
        contract.product.public_repository,
        contract.product.development_repository,
        contract.server.primary_executable,
        contract.extension.id,
        contract.debug_adapter.executable,
        context
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

fn validate_contract(
    repo_root: &Path,
    contract: &ProductIdentityContract,
    repository_context: Option<&str>,
) -> Result<()> {
    if contract.schema_version != SUPPORTED_SCHEMA_VERSION {
        bail!(
            "unsupported product identity schema version {}; expected {}",
            contract.schema_version,
            SUPPORTED_SCHEMA_VERSION
        );
    }

    require_non_empty("product.name", &contract.product.name)?;
    validate_repository_slug(
        "product.public_repository",
        &contract.product.public_repository,
    )?;
    validate_repository_slug(
        "product.development_repository",
        &contract.product.development_repository,
    )?;
    if contract.product.public_repository == contract.product.development_repository {
        bail!(
            "public and development repositories must remain distinct; both are {:?}",
            contract.product.public_repository
        );
    }
    validate_repository_context(contract, repository_context)?;

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

    let public_repository_url = repository_url(&contract.product.public_repository);
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
        workspace_repository,
    )?;
    validate_cargo_identity(
        repo_root,
        "server implementation",
        &contract.server.implementation_manifest,
        &contract.server.implementation_crate,
        &contract.server.compatibility_executable,
        workspace_repository,
    )?;
    validate_cargo_identity(
        repo_root,
        "debug adapter",
        &contract.debug_adapter.package_manifest,
        &contract.debug_adapter.cargo_package,
        &contract.debug_adapter.executable,
        workspace_repository,
    )?;
    validate_facade_dependency(
        repo_root,
        &contract.server.package_manifest,
        &contract.server.implementation_crate,
    )?;

    if contract.debug_adapter.maturity != "preview" {
        bail!(
            concat!(
                "debug adapter maturity must remain \"preview\" until behavior-backed ",
                "promotion; found {:?}"
            ),
            contract.debug_adapter.maturity
        );
    }

    validate_extension(repo_root, &contract.extension, &public_repository_url)?;
    validate_conflicts(contract)?;
    Ok(())
}

fn validate_repository_context(
    contract: &ProductIdentityContract,
    repository_context: Option<&str>,
) -> Result<()> {
    let Some(repository_context) = repository_context else {
        return Ok(());
    };
    validate_repository_slug("repository context", repository_context)?;
    if repository_context != contract.product.public_repository
        && repository_context != contract.product.development_repository
    {
        bail!(
            "checkout repository {:?} is not declared as either public {:?} or development {:?}",
            repository_context,
            contract.product.public_repository,
            contract.product.development_repository
        );
    }
    Ok(())
}

fn validate_cargo_identity(
    repo_root: &Path,
    label: &str,
    manifest_path: &Path,
    expected_package: &str,
    expected_binary: &str,
    workspace_repository: &str,
) -> Result<()> {
    validate_relative_path(manifest_path)?;
    let manifest = read_toml(repo_root, manifest_path)?;
    let package_name = toml_string(&manifest, &["package", "name"], "Cargo package name")?;
    require_equal(
        &format!("{label} Cargo package"),
        package_name,
        expected_package,
    )?;

    let package_repository = effective_package_repository(&manifest, workspace_repository)?;
    require_equal(
        &format!("{label} package repository"),
        package_repository,
        workspace_repository,
    )?;

    let binary_names = cargo_binary_names(repo_root, manifest_path, &manifest, package_name)?;
    if !binary_names.contains(expected_binary) {
        bail!(
            "{label} manifest {} does not expose binary {:?}; found {:?}",
            manifest_path.display(),
            expected_binary,
            binary_names
        );
    }
    Ok(())
}

fn validate_facade_dependency(
    repo_root: &Path,
    facade_manifest_path: &Path,
    implementation_crate: &str,
) -> Result<()> {
    let manifest = read_toml(repo_root, facade_manifest_path)?;
    let dependencies = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            eyre!(
                "primary server manifest {} has no [dependencies] table",
                facade_manifest_path.display()
            )
        })?;

    let has_implementation = dependencies.iter().any(|(alias, value)| {
        let effective_package = value
            .as_table()
            .and_then(|table| table.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(alias);
        effective_package == implementation_crate
    });
    if !has_implementation {
        bail!(
            "primary server package does not depend on declared implementation crate {:?}",
            implementation_crate
        );
    }
    Ok(())
}

fn effective_package_repository<'a>(
    manifest: &'a toml::Value,
    workspace_repository: &'a str,
) -> Result<&'a str> {
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| eyre!("Cargo manifest has no [package] table"))?;
    let repository = package
        .get("repository")
        .ok_or_else(|| eyre!("Cargo package repository is missing"))?;

    if let Some(value) = repository.as_str() {
        return Ok(value);
    }

    let inherits_workspace = repository
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if inherits_workspace {
        return Ok(workspace_repository);
    }

    bail!(
        concat!(
            "Cargo package repository must be a string or inherit workspace.repository; ",
            "found {repository:?}"
        )
    )
}

fn cargo_binary_names(
    repo_root: &Path,
    manifest_path: &Path,
    manifest: &toml::Value,
    package_name: &str,
) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();

    if let Some(bins) = manifest.get("bin") {
        let bins = bins
            .as_array()
            .ok_or_else(|| eyre!("Cargo bin targets are not an array"))?;
        for bin in bins {
            let name = bin
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| eyre!("Cargo [[bin]] target is missing a string name"))?;
            names.insert(name.to_string());
        }
    }

    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| eyre!("Cargo manifest has no [package] table"))?;
    let autobins = package
        .get("autobins")
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    if autobins {
        let manifest_dir = repo_root.join(
            manifest_path
                .parent()
                .ok_or_else(|| eyre!("Cargo manifest path has no parent"))?,
        );
        if manifest_dir.join("src/main.rs").is_file() {
            names.insert(package_name.to_string());
        }
        let bin_dir = manifest_dir.join("src/bin");
        if bin_dir.is_dir() {
            for entry in fs::read_dir(&bin_dir)
                .wrap_err_with(|| format!("reading implicit Cargo bins in {}", bin_dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|value| value.to_str()) == Some("rs")
                {
                    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                        names.insert(stem.to_string());
                    }
                } else if path.is_dir() && path.join("main.rs").is_file() {
                    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }

    Ok(names)
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

fn resolve_repository_context(repo_root: &Path) -> Result<Option<String>> {
    if let Ok(value) = env::var("GITHUB_REPOSITORY") {
        let value = value.trim();
        if !value.is_empty() {
            validate_repository_slug("GITHUB_REPOSITORY", value)?;
            return Ok(Some(value.to_string()));
        }
    }

    let output = match Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).wrap_err("running git to resolve repository context"),
    };
    if !output.status.success() {
        return Ok(None);
    }

    let remote = String::from_utf8(output.stdout).wrap_err("decoding git origin URL")?;
    let remote = remote.trim();
    if remote.is_empty() {
        return Ok(None);
    }
    let repository = parse_github_repository(remote).ok_or_else(|| {
        eyre!(
            "origin remote {:?} is not a supported github.com repository URL",
            remote
        )
    })?;
    validate_repository_slug("origin repository", &repository)?;
    Ok(Some(repository))
}

fn parse_github_repository(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches('/');
    let path = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git://github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| trimmed.strip_prefix(prefix))?;
    Some(path.trim_end_matches(".git").trim_end_matches('/').to_string())
}

fn repository_url(repository: &str) -> String {
    format!("https://github.com/{repository}")
}

fn validate_repository_slug(label: &str, value: &str) -> Result<()> {
    let Some((owner, repository)) = value.split_once('/') else {
        bail!("{label} must use owner/repository syntax; found {value:?}");
    };
    if repository.contains('/')
        || !valid_repository_segment(owner)
        || !valid_repository_segment(repository)
    {
        bail!("{label} must use owner/repository syntax; found {value:?}");
    }
    Ok(())
}

fn valid_repository_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
}

fn read_toml(repo_root: &Path, relative_path: &Path) -> Result<toml::Value> {
    validate_relative_path(relative_path)?;
    let path = repo_root.join(relative_path);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading TOML identity source {}", path.display()))?;
    toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing TOML identity source {}", path.display()))
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
    let raw = path
        .to_str()
        .ok_or_else(|| eyre!("identity source path is not valid UTF-8: {path:?}"))?;
    let invalid_segment = raw
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.contains('\\')
        || raw.contains(':')
        || invalid_segment
    {
        bail!("identity source path must use canonical repository-relative syntax: {path:?}");
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
    use super::{check_with_repository_context, parse_github_repository};
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
    fn coherent_identity_contract_passes_in_both_declared_repositories() -> Result<()> {
        let repo = fixture_repo()?;
        check_with_repository_context(
            repo.path(),
            Some("EffortlessMetrics/perl-lsp-swarm"),
        )?;
        check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp"))
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

        expect_failure(repo.path(), "extension package name drifted")
    }

    #[test]
    fn package_repository_override_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            r#"[package]
name = "perllsp"
repository = "https://github.com/other/project"

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
        )?;

        expect_failure(repo.path(), "primary server package repository drifted")
    }

    #[test]
    fn undeclared_repository_context_fails() -> Result<()> {
        let repo = fixture_repo()?;
        let error = match check_with_repository_context(repo.path(), Some("other/project")) {
            Ok(()) => bail!("undeclared repository context should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains("is not declared as either public") {
            bail!("unexpected error: {error:#}");
        }
        Ok(())
    }

    #[test]
    fn identical_public_and_development_repositories_fail() -> Result<()> {
        let repo = fixture_repo()?;
        let contract = CONTRACT.replace(
            "development_repository = \"EffortlessMetrics/perl-lsp-swarm\"",
            "development_repository = \"EffortlessMetrics/perl-lsp\"",
        );
        write(repo.path(), "policy/product-identity.toml", &contract)?;

        expect_failure(repo.path(), "public and development repositories must remain distinct")
    }

    #[test]
    fn missing_facade_dependency_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
serde = "1"
"#,
        )?;

        expect_failure(
            repo.path(),
            "does not depend on declared implementation crate",
        )
    }

    #[test]
    fn same_named_dependency_alias_cannot_hide_another_package() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { package = "different-project", version = "1" }
"#,
        )?;

        expect_failure(
            repo.path(),
            "does not depend on declared implementation crate",
        )
    }

    #[test]
    fn implicit_primary_binary_is_accepted() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            r#"[package]
name = "perllsp"
repository.workspace = true

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
        )?;

        check_with_repository_context(
            repo.path(),
            Some("EffortlessMetrics/perl-lsp-swarm"),
        )
    }

    #[test]
    fn missing_primary_binary_fails() -> Result<()> {
        let repo = fixture_repo()?;
        write(
            repo.path(),
            "crates/perllsp/Cargo.toml",
            r#"[package]
name = "perllsp"
repository.workspace = true
autobins = false

[[bin]]
name = "wrong"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
        )?;

        expect_failure(repo.path(), "does not expose binary \"perllsp\"")
    }

    #[test]
    fn github_remote_forms_resolve_to_repository_slug() {
        for remote in [
            "https://github.com/EffortlessMetrics/perl-lsp-swarm.git",
            "ssh://git@github.com/EffortlessMetrics/perl-lsp-swarm.git",
            "git@github.com:EffortlessMetrics/perl-lsp-swarm.git",
        ] {
            assert_eq!(
                parse_github_repository(remote).as_deref(),
                Some("EffortlessMetrics/perl-lsp-swarm")
            );
        }
    }

    fn expect_failure(repo: &Path, expected: &str) -> Result<()> {
        let error = match check_with_repository_context(
            repo,
            Some("EffortlessMetrics/perl-lsp-swarm"),
        ) {
            Ok(()) => bail!("identity drift should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains(expected) {
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
            r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
        )?;
        write(repo.path(), "crates/perllsp/src/main.rs", "fn main() {}\n")?;
        write(
            repo.path(),
            "crates/perl-lsp-rs/Cargo.toml",
            r#"[package]
name = "perl-lsp-rs"
repository.workspace = true

[[bin]]
name = "perl-lsp"
"#,
        )?;
        write(
            repo.path(),
            "crates/perl-lsp-rs/src/main.rs",
            "fn main() {}\n",
        )?;
        write(
            repo.path(),
            "crates/perl-dap/Cargo.toml",
            r#"[package]
name = "perl-dap"
repository.workspace = true

[[bin]]
name = "perl-dap"
"#,
        )?;
        write(repo.path(), "crates/perl-dap/src/main.rs", "fn main() {}\n")?;
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
