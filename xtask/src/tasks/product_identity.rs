//! Validate the canonical product, package, executable, extension, and DAP identity map.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const CONTRACT_PATH: &str = "policy/product-identity.toml";
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
struct RepositoryContext {
    repository: String,
    authoritative: bool,
}

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
    check_with_resolved_repository_context(repo_root, repository_context.as_ref())
}

fn check_with_repository_context(repo_root: &Path, repository_context: Option<&str>) -> Result<()> {
    let repository_context = repository_context.map(|repository| RepositoryContext {
        repository: repository.to_string(),
        authoritative: true,
    });
    check_with_resolved_repository_context(repo_root, repository_context.as_ref())
}

fn check_with_resolved_repository_context(
    repo_root: &Path,
    resolved_context: Option<&RepositoryContext>,
) -> Result<()> {
    let contract = load_contract(repo_root)?;
    let repository_context = select_repository_context(&contract, resolved_context);
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

fn select_repository_context<'a>(
    contract: &ProductIdentityContract,
    resolved_context: Option<&'a RepositoryContext>,
) -> Option<&'a str> {
    let context = resolved_context?;
    if context.authoritative
        || context.repository == contract.product.public_repository
        || context.repository == contract.product.development_repository
    {
        Some(context.repository.as_str())
    } else {
        None
    }
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
    validate_repository_slug("product.public_repository", &contract.product.public_repository)?;
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

    let expected_extension_id =
        format!("{}.{}", contract.extension.publisher, contract.extension.package_name);
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
    require_equal("workspace repository", workspace_repository, &public_repository_url)?;

    let primary_manifest = validate_cargo_package_identity(
        repo_root,
        "primary server",
        &contract.server.package_manifest,
        &contract.server.cargo_package,
        workspace_repository,
    )?;
    validate_cargo_binary_identity(
        repo_root,
        "primary server",
        &contract.server.package_manifest,
        &primary_manifest,
        &contract.server.primary_executable,
    )?;
    validate_cargo_package_identity(
        repo_root,
        "server implementation",
        &contract.server.implementation_manifest,
        &contract.server.implementation_crate,
        workspace_repository,
    )?;
    let debug_adapter_manifest = validate_cargo_package_identity(
        repo_root,
        "debug adapter",
        &contract.debug_adapter.package_manifest,
        &contract.debug_adapter.cargo_package,
        workspace_repository,
    )?;
    validate_cargo_binary_identity(
        repo_root,
        "debug adapter",
        &contract.debug_adapter.package_manifest,
        &debug_adapter_manifest,
        &contract.debug_adapter.executable,
    )?;
    validate_facade_dependency(
        repo_root,
        &root_manifest,
        &contract.server.package_manifest,
        &contract.server.implementation_manifest,
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

fn validate_cargo_package_identity(
    repo_root: &Path,
    label: &str,
    manifest_path: &Path,
    expected_package: &str,
    workspace_repository: &str,
) -> Result<toml::Value> {
    validate_relative_path(manifest_path)?;
    let manifest = read_toml(repo_root, manifest_path)?;
    let package_name = toml_string(&manifest, &["package", "name"], "Cargo package name")?;
    require_equal(&format!("{label} Cargo package"), package_name, expected_package)?;

    let package_repository = effective_package_repository(&manifest, workspace_repository)?;
    require_equal(
        &format!("{label} package repository"),
        package_repository,
        workspace_repository,
    )?;

    Ok(manifest)
}

fn validate_cargo_binary_identity(
    repo_root: &Path,
    label: &str,
    manifest_path: &Path,
    manifest: &toml::Value,
    expected_binary: &str,
) -> Result<()> {
    let package_name = toml_string(manifest, &["package", "name"], "Cargo package name")?;
    let binary_names = cargo_binary_names(repo_root, manifest_path, manifest, package_name)?;
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
    root_manifest: &toml::Value,
    facade_manifest_path: &Path,
    implementation_manifest_path: &Path,
    implementation_crate: &str,
) -> Result<()> {
    let facade_manifest = read_toml(repo_root, facade_manifest_path)?;
    let dependencies =
        facade_manifest.get("dependencies").and_then(toml::Value::as_table).ok_or_else(|| {
            eyre!(
                "primary server manifest {} has no [dependencies] table",
                facade_manifest_path.display()
            )
        })?;

    let implementation_dir = implementation_manifest_path.parent().ok_or_else(|| {
        eyre!(
            "implementation manifest path {} has no parent directory",
            implementation_manifest_path.display()
        )
    })?;
    let expected_path = normalize_repo_relative(Path::new(""), implementation_dir)?;
    let mut matching_package_seen = false;

    for (alias, specification) in dependencies {
        let (matches_package, resolves_to_source) = dependency_source_match(
            root_manifest,
            facade_manifest_path,
            alias,
            specification,
            implementation_crate,
            &expected_path,
        )?;
        if !matches_package {
            continue;
        }
        matching_package_seen = true;
        if resolves_to_source {
            return Ok(());
        }
    }

    if matching_package_seen {
        bail!(
            concat!(
                "primary server package does not depend on declared implementation crate {:?} ",
                "through the governed in-tree workspace/path source"
            ),
            implementation_crate
        );
    }
    bail!(
        "primary server package does not depend on declared implementation crate {:?}",
        implementation_crate
    )
}

fn dependency_source_match(
    root_manifest: &toml::Value,
    facade_manifest_path: &Path,
    alias: &str,
    specification: &toml::Value,
    implementation_crate: &str,
    expected_path: &Path,
) -> Result<(bool, bool)> {
    let Some(table) = specification.as_table() else {
        return Ok((alias == implementation_crate, false));
    };

    if table.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        let workspace_dependencies = root_manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("dependencies"))
            .and_then(toml::Value::as_table)
            .ok_or_else(|| eyre!("workspace manifest has no [workspace.dependencies] table"))?;
        let workspace_specification = workspace_dependencies.get(alias).ok_or_else(|| {
            eyre!("workspace dependency {:?} referenced by the primary server is missing", alias)
        })?;
        let matches_package =
            dependency_package_name(alias, workspace_specification) == implementation_crate;
        let resolves_to_source = matches_package
            && dependency_path_matches(Path::new(""), workspace_specification, expected_path)?;
        return Ok((matches_package, resolves_to_source));
    }

    let matches_package = dependency_package_name(alias, specification) == implementation_crate;
    let facade_dir = facade_manifest_path.parent().ok_or_else(|| {
        eyre!(
            "primary server manifest path {} has no parent directory",
            facade_manifest_path.display()
        )
    })?;
    let resolves_to_source =
        matches_package && dependency_path_matches(facade_dir, specification, expected_path)?;
    Ok((matches_package, resolves_to_source))
}

fn dependency_package_name<'a>(alias: &'a str, specification: &'a toml::Value) -> &'a str {
    specification
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(toml::Value::as_str)
        .unwrap_or(alias)
}

fn dependency_path_matches(
    base: &Path,
    specification: &toml::Value,
    expected_path: &Path,
) -> Result<bool> {
    let Some(path) =
        specification.as_table().and_then(|table| table.get("path")).and_then(toml::Value::as_str)
    else {
        return Ok(false);
    };
    Ok(normalize_repo_relative(base, Path::new(path))? == expected_path)
}

fn normalize_repo_relative(base: &Path, path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in base.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!(
                        "Cargo dependency path escapes the repository: {}",
                        base.join(path).display()
                    );
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "Cargo dependency path must remain repository-relative: {}",
                    base.join(path).display()
                );
            }
        }
    }
    Ok(normalized)
}

fn effective_package_repository<'a>(
    manifest: &'a toml::Value,
    workspace_repository: &'a str,
) -> Result<&'a str> {
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| eyre!("Cargo manifest has no [package] table"))?;
    let repository =
        package.get("repository").ok_or_else(|| eyre!("Cargo package repository is missing"))?;

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

    bail!(concat!(
        "Cargo package repository must be a string or inherit workspace.repository; ",
        "found {repository:?}"
    ))
}

pub(crate) fn cargo_binary_names(
    repo_root: &Path,
    manifest_path: &Path,
    manifest: &toml::Value,
    package_name: &str,
) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();

    if let Some(bins) = manifest.get("bin") {
        let bins = bins.as_array().ok_or_else(|| eyre!("Cargo bin targets are not an array"))?;
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
    let autobins = package.get("autobins").and_then(toml::Value::as_bool).unwrap_or(true);
    // Cargo autodiscovers src/main.rs and src/bin/* only when no explicit
    // [[bin]] target is declared; an explicit list disables discovery, so a
    // stray src/main.rs beside [[bin]] targets is not another binary.
    if autobins && names.is_empty() {
        let manifest_dir = repo_root.join(
            manifest_path.parent().ok_or_else(|| eyre!("Cargo manifest path has no parent"))?,
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
                if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("rs")
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
        json_string(&manifest, &["repository", "url"], "extension repository URL")?,
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
    if contract.conflicts.iter().any(|conflict| conflict.identity == primary_package_identity) {
        bail!(
            "primary server package {:?} cannot also be classified as an identity conflict",
            primary_package_identity
        );
    }
    Ok(())
}

fn resolve_repository_context(repo_root: &Path) -> Result<Option<RepositoryContext>> {
    if let Ok(value) = env::var("GITHUB_REPOSITORY") {
        let value = value.trim();
        if !value.is_empty() {
            validate_repository_slug("GITHUB_REPOSITORY", value)?;
            return Ok(Some(RepositoryContext {
                repository: value.to_string(),
                authoritative: true,
            }));
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
    let Some(repository) = parse_github_repository(remote) else {
        return Ok(None);
    };
    validate_repository_slug("origin repository", &repository)?;
    Ok(Some(RepositoryContext { repository, authoritative: false }))
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

fn toml_string<'a>(value: &'a toml::Value, path: &[&str], description: &str) -> Result<&'a str> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .ok_or_else(|| eyre!("missing {description} at {}", path.join(".")))?;
    }
    current.as_str().ok_or_else(|| eyre!("{description} at {} is not a string", path.join(".")))
}

fn json_string<'a>(value: &'a JsonValue, path: &[&str], description: &str) -> Result<&'a str> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .ok_or_else(|| eyre!("missing {description} at {}", path.join(".")))?;
    }
    current.as_str().ok_or_else(|| eyre!("{description} at {} is not a string", path.join(".")))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    let raw =
        path.to_str().ok_or_else(|| eyre!("identity source path is not valid UTF-8: {path:?}"))?;
    let invalid_segment =
        raw.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..");
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
#[path = "product_identity_tests.rs"]
mod tests;
