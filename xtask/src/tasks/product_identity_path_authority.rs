//! Reject governed identity paths that reach their authority through symlink aliases.
//!
//! Cargo metadata intentionally canonicalizes workspace member manifests. The canonical product
//! identity contract, however, publishes repository-relative authority paths. Those declared
//! paths must identify the tracked files directly rather than an alias that merely resolves to
//! the same workspace member.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CONTRACT_PATH: &str = "policy/product-identity.toml";

#[derive(Debug, Deserialize)]
struct ProductIdentityContract {
    server: ServerIdentity,
    debug_adapter: DebugAdapterIdentity,
}

#[derive(Debug, Deserialize)]
struct ServerIdentity {
    package_manifest: PathBuf,
    implementation_manifest: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DebugAdapterIdentity {
    package_manifest: PathBuf,
}

pub(super) fn check(repo_root: &Path) -> Result<()> {
    let contract = load_contract(repo_root)?;
    for (label, relative_path) in [
        ("primary server", contract.server.package_manifest.as_path()),
        ("server implementation", contract.server.implementation_manifest.as_path()),
        ("debug adapter", contract.debug_adapter.package_manifest.as_path()),
    ] {
        reject_symlink_components(repo_root, relative_path, label)?;
    }
    Ok(())
}

fn load_contract(repo_root: &Path) -> Result<ProductIdentityContract> {
    let path = repo_root.join(CONTRACT_PATH);
    let raw = fs::read_to_string(&path)
        .wrap_err_with(|| format!("reading product identity contract {}", path.display()))?;
    toml::from_str(&raw)
        .wrap_err_with(|| format!("parsing product identity contract {}", path.display()))
}

fn reject_symlink_components(repo_root: &Path, relative_path: &Path, label: &str) -> Result<()> {
    let mut current = repo_root.to_path_buf();
    for component in relative_path.components() {
        let Component::Normal(segment) = component else {
            bail!(
                "{label} manifest path must use canonical repository-relative components: {relative_path:?}"
            );
        };
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).wrap_err_with(|| {
            format!("reading {label} manifest path authority component {}", current.display())
        })?;
        if metadata.file_type().is_symlink() {
            bail!(
                "{label} manifest path {} uses symlink authority at {}",
                relative_path.display(),
                current.display()
            );
        }
    }
    if relative_path.file_name().and_then(|name| name.to_str()) != Some("Cargo.toml") {
        return Err(eyre!("{label} manifest authority must end in Cargo.toml: {relative_path:?}"));
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
package_manifest = "crates/perllsp/Cargo.toml"
implementation_manifest = "crates/perl-lsp-rs/Cargo.toml"

[debug_adapter]
package_manifest = "crates/perl-dap/Cargo.toml"
"#;

    #[test]
    fn direct_governed_paths_pass() -> Result<()> {
        let repo = fixture_repo()?;
        check(repo.path())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_manifest_alias_is_rejected() -> Result<()> {
        use std::os::unix::fs::symlink;

        let repo = fixture_repo()?;
        fs::create_dir_all(repo.path().join("decoy/perllsp"))?;
        symlink(
            repo.path().join("crates/perllsp/Cargo.toml"),
            repo.path().join("decoy/perllsp/Cargo.toml"),
        )?;
        let contract = CONTRACT.replace(
            "package_manifest = \"crates/perllsp/Cargo.toml\"",
            "package_manifest = \"decoy/perllsp/Cargo.toml\"",
        );
        write(repo.path(), CONTRACT_PATH, &contract)?;

        let error = match check(repo.path()) {
            Ok(()) => bail!("symlinked manifest authority should fail"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains("uses symlink authority") {
            bail!("unexpected error: {error:#}");
        }
        Ok(())
    }

    fn fixture_repo() -> Result<TempDir> {
        let repo = TempDir::new()?;
        write(repo.path(), CONTRACT_PATH, CONTRACT)?;
        for relative in [
            "crates/perllsp/Cargo.toml",
            "crates/perl-lsp-rs/Cargo.toml",
            "crates/perl-dap/Cargo.toml",
        ] {
            write(repo.path(), relative, "[package]\nname = \"fixture\"\n")?;
        }
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
