//! Bind canonical product identities to the Cargo workspace members that actually build them.
//!
//! `product_identity` validates the selected manifest contents. This companion check closes the
//! remaining source-authority seam by resolving Cargo metadata and proving those manifests are
//! active workspace members rather than matching decoy files elsewhere in the repository.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CONTRACT_PATH: &str = "policy/product-identity.toml";

#[derive(Debug, Deserialize)]
struct ProductIdentityContract {
    server: ServerIdentity,
    debug_adapter: DebugAdapterIdentity,
}

#[derive(Debug, Deserialize)]
struct ServerIdentity {
    cargo_package: String,
    package_manifest: PathBuf,
    implementation_crate: String,
    implementation_manifest: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DebugAdapterIdentity {
    cargo_package: String,
    package_manifest: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
}

/// Prove every governed Cargo manifest is the active workspace member for its declared package.
pub fn check(repo_root: &Path) -> Result<()> {
    let contract = load_contract(repo_root)?;
    let workspace_members = load_workspace_members(repo_root)?;
    let canonical_root = fs::canonicalize(repo_root)
        .wrap_err_with(|| format!("canonicalizing repository root {}", repo_root.display()))?;

    let governed = [
        (
            "primary server",
            contract.server.package_manifest.as_path(),
            contract.server.cargo_package.as_str(),
        ),
        (
            "server implementation",
            contract.server.implementation_manifest.as_path(),
            contract.server.implementation_crate.as_str(),
        ),
        (
            "debug adapter",
            contract.debug_adapter.package_manifest.as_path(),
            contract.debug_adapter.cargo_package.as_str(),
        ),
    ];
    let mut seen_manifests = BTreeSet::new();

    for (label, relative_manifest, expected_package) in governed {
        validate_manifest_path(relative_manifest)?;
        let manifest = fs::canonicalize(repo_root.join(relative_manifest)).wrap_err_with(|| {
            format!("canonicalizing {label} manifest {}", relative_manifest.display())
        })?;
        if !manifest.starts_with(&canonical_root) {
            bail!(
                "{label} manifest {} resolves outside repository root {}",
                relative_manifest.display(),
                canonical_root.display()
            );
        }
        if !seen_manifests.insert(manifest.clone()) {
            bail!(
                "governed product roles must use distinct Cargo manifests; duplicate {}",
                relative_manifest.display()
            );
        }

        let Some(actual_package) = workspace_members.get(&manifest) else {
            bail!(
                "{label} manifest {} is not an active Cargo workspace member",
                relative_manifest.display()
            );
        };
        if actual_package != expected_package {
            bail!(
                "{label} workspace member package drifted: manifest {} resolves to {:?}, expected {:?}",
                relative_manifest.display(),
                actual_package,
                expected_package
            );
        }
    }

    println!(
        "Product identity workspace binding: primary {}, implementation {}, DAP {}",
        contract.server.cargo_package,
        contract.server.implementation_crate,
        contract.debug_adapter.cargo_package
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

fn load_workspace_members(repo_root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let manifest_path = repo_root.join("Cargo.toml");
    let output = Command::new(&cargo)
        .current_dir(repo_root)
        .args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"])
        .arg(&manifest_path)
        .output()
        .wrap_err_with(|| {
            format!("running {:?} metadata for {}", cargo, manifest_path.display())
        })?;

    if !output.status.success() {
        bail!(
            "cargo metadata failed for product identity workspace binding: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .wrap_err("parsing cargo metadata for product identity workspace binding")?;
    let member_ids = metadata.workspace_members.into_iter().collect::<BTreeSet<_>>();
    let mut members = BTreeMap::new();

    for package in metadata.packages {
        if !member_ids.contains(&package.id) {
            continue;
        }
        let manifest = fs::canonicalize(&package.manifest_path).wrap_err_with(|| {
            format!("canonicalizing workspace member manifest {}", package.manifest_path.display())
        })?;
        if let Some(previous) = members.insert(manifest.clone(), package.name.clone()) {
            bail!(
                "cargo metadata returned duplicate workspace manifest {} for {:?} and {:?}",
                manifest.display(),
                previous,
                package.name
            );
        }
    }

    if members.is_empty() {
        return Err(eyre!("cargo metadata returned no active workspace members"));
    }
    Ok(members)
}

fn validate_manifest_path(path: &Path) -> Result<()> {
    let raw = path
        .to_str()
        .ok_or_else(|| eyre!("governed manifest path is not valid UTF-8: {path:?}"))?;
    let invalid_segment =
        raw.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.contains('\\')
        || raw.contains(':')
        || invalid_segment
        || path.file_name().and_then(|name| name.to_str()) != Some("Cargo.toml")
    {
        bail!(
            "governed manifest path must be a canonical repository-relative Cargo.toml path: {path:?}"
        );
    }
    Ok(())
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
cargo_package = "perllsp"
package_manifest = "crates/perllsp/Cargo.toml"
implementation_crate = "perl-lsp-rs"
implementation_manifest = "crates/perl-lsp-rs/Cargo.toml"

[debug_adapter]
cargo_package = "perl-dap"
package_manifest = "crates/perl-dap/Cargo.toml"
"#;

    #[test]
    fn governed_manifests_are_active_workspace_members() -> Result<()> {
        let repo = fixture_repo()?;
        check(repo.path())
    }

    #[test]
    fn matching_decoy_manifest_is_rejected() -> Result<()> {
        let repo = fixture_repo()?;
        write_package(repo.path(), "decoy/perllsp", "perllsp", true)?;
        let contract = CONTRACT.replace(
            "package_manifest = \"crates/perllsp/Cargo.toml\"",
            "package_manifest = \"decoy/perllsp/Cargo.toml\"",
        );
        write(repo.path(), CONTRACT_PATH, &contract)?;

        expect_failure(repo.path(), "is not an active Cargo workspace member")
    }

    #[test]
    fn governed_roles_cannot_share_one_manifest() -> Result<()> {
        let repo = fixture_repo()?;
        let contract = CONTRACT
            .replace("cargo_package = \"perl-dap\"", "cargo_package = \"perllsp\"")
            .replace(
                "package_manifest = \"crates/perl-dap/Cargo.toml\"",
                "package_manifest = \"crates/perllsp/Cargo.toml\"",
            );
        write(repo.path(), CONTRACT_PATH, &contract)?;

        expect_failure(repo.path(), "must use distinct Cargo manifests")
    }

    #[test]
    fn traversal_manifest_path_is_rejected() -> Result<()> {
        let repo = fixture_repo()?;
        let contract = CONTRACT.replace(
            "package_manifest = \"crates/perllsp/Cargo.toml\"",
            "package_manifest = \"../outside/Cargo.toml\"",
        );
        write(repo.path(), CONTRACT_PATH, &contract)?;

        expect_failure(repo.path(), "canonical repository-relative Cargo.toml path")
    }

    fn fixture_repo() -> Result<TempDir> {
        let repo = TempDir::new()?;
        write(repo.path(), CONTRACT_PATH, CONTRACT)?;
        write(
            repo.path(),
            "Cargo.toml",
            r#"[workspace]
members = ["crates/perllsp", "crates/perl-lsp-rs", "crates/perl-dap"]
resolver = "2"
"#,
        )?;
        write_package(repo.path(), "crates/perllsp", "perllsp", true)?;
        write_package(repo.path(), "crates/perl-lsp-rs", "perl-lsp-rs", false)?;
        write_package(repo.path(), "crates/perl-dap", "perl-dap", true)?;
        Ok(repo)
    }

    fn write_package(root: &Path, relative: &str, name: &str, binary: bool) -> Result<()> {
        write(
            root,
            &format!("{relative}/Cargo.toml"),
            &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        )?;
        let source = if binary { "src/main.rs" } else { "src/lib.rs" };
        write(root, &format!("{relative}/{source}"), "")
    }

    fn expect_failure(repo: &Path, expected: &str) -> Result<()> {
        let error = match check(repo) {
            Ok(()) => bail!("workspace identity drift should fail"),
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
