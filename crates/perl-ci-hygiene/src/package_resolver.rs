//! Cargo metadata package identity shared by administrative consumers.

use color_eyre::eyre::{Context, Result, eyre};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Align ordinary and verbatim Windows spellings emitted by Git and Cargo.
/// Keep unsupported device namespaces and path components unchanged.
pub(crate) fn normalize_metadata_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let Some(verbatim) = normalized.strip_prefix("//?/") else {
        return normalized;
    };
    let bytes = verbatim.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && bytes.get(2) == Some(&b'/')
    {
        return verbatim.to_string();
    }
    if let Some(unc) = verbatim.strip_prefix("UNC/") {
        let mut components = unc.split('/');
        if components.next().is_some_and(|server| !server.is_empty())
            && components.next().is_some_and(|share| !share.is_empty())
        {
            return format!("//{unc}");
        }
    }
    normalized
}

fn manifest_crate_directory<'a>(
    root_normalized: &str,
    manifest_normalized: &'a str,
) -> Option<&'a str> {
    manifest_normalized
        .strip_prefix(root_normalized.trim_end_matches('/'))
        .and_then(|path| path.strip_prefix('/'))
        .and_then(|path| path.strip_suffix("/Cargo.toml"))
}

/// Map crate directories to package names using Cargo's authoritative workspace metadata.
pub fn resolve_package_names(
    project_root: &Path,
    crate_dirs: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(project_root)
        .output()
        .context("Failed to run cargo metadata")?;
    if !output.status.success() {
        return Err(eyre!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse cargo metadata JSON")?;
    let packages = metadata
        .get("packages")
        .and_then(|packages| packages.as_array())
        .ok_or_else(|| eyre!("cargo metadata missing 'packages' array"))?;

    let root_normalized = normalize_metadata_path(&project_root.to_string_lossy());
    let mut names = BTreeSet::new();

    for package in packages {
        let Some(manifest_path) = package.get("manifest_path").and_then(|value| value.as_str())
        else {
            continue;
        };
        let Some(package_name) = package.get("name").and_then(|value| value.as_str()) else {
            continue;
        };

        let manifest_normalized = normalize_metadata_path(manifest_path);
        if let Some(relative_directory) =
            manifest_crate_directory(&root_normalized, &manifest_normalized)
            && crate_dirs.contains(relative_directory)
        {
            names.insert(package_name.to_string());
        }
    }

    Ok(names)
}

/// Resolve one workspace-relative crate directory to its authoritative Cargo package name.
pub fn resolve_single_package_name(project_root: &Path, crate_dir: &str) -> Result<String> {
    let normalized = crate_dir.replace('\\', "/");
    let mut directories = BTreeSet::new();
    directories.insert(normalized.trim_end_matches('/').to_string());
    resolve_package_names(project_root, &directories)?
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("No workspace package found for crate directory: {crate_dir}"))
}

#[cfg(test)]
mod tests {
    use super::{manifest_crate_directory, normalize_metadata_path};

    #[test]
    fn metadata_path_spellings_preserve_namespace_boundaries() {
        for (verbatim, ordinary) in [
            (r"\\?\F:\work\repo", "F:/work/repo"),
            (r"\\?\UNC\server\share\repo", "//server/share/repo"),
        ] {
            assert_eq!(normalize_metadata_path(verbatim), ordinary);
            assert_eq!(normalize_metadata_path(ordinary), ordinary);
        }
        for unsupported in [
            "//?/GLOBALROOT/device/repo",
            "//?/Volume{123}/repo",
            "//?/F:relative",
            "//?/UNC//share/repo",
            "//?/UNC/server//repo",
        ] {
            assert_eq!(normalize_metadata_path(unsupported), unsupported);
        }
    }

    #[test]
    fn manifest_directory_requires_a_root_segment_boundary() {
        assert_eq!(
            manifest_crate_directory(
                "/work/repo/",
                "/work/repo/crates/perl-token/Cargo.toml"
            ),
            Some("crates/perl-token")
        );
        assert_eq!(
            manifest_crate_directory(
                "/work/repo",
                "/work/repository/crates/perl-token/Cargo.toml"
            ),
            None
        );
    }
}
