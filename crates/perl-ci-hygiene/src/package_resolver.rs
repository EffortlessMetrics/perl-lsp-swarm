//! Cargo metadata package identity shared by administrative consumers.

use color_eyre::eyre::{Context, Result, eyre};
use duct::cmd;
use std::collections::BTreeSet;
use std::path::Path;

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

/// Map crate directories to package names by reading each Cargo.toml.
///
/// Uses `cargo metadata` to get authoritative package names and manifest paths,
/// then matches them against the changed crate directories.
pub fn resolve_package_names(
    project_root: &Path,
    crate_dirs: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let output = cmd("cargo", &["metadata", "--no-deps", "--format-version", "1"])
        .dir(project_root)
        .stdout_capture()
        .stderr_capture()
        .run()
        .context("Failed to run cargo metadata")?;

    let stdout =
        String::from_utf8(output.stdout).context("cargo metadata output was not valid UTF-8")?;

    let metadata: serde_json::Value =
        serde_json::from_str(&stdout).context("Failed to parse cargo metadata JSON")?;

    let packages = metadata
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| eyre!("cargo metadata missing 'packages' array"))?;

    let root_normalized = normalize_metadata_path(&project_root.to_string_lossy());
    let mut names = BTreeSet::new();

    for package in packages {
        let manifest_path = match package.get("manifest_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };

        let pkg_name = match package.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };

        // Convert absolute manifest path to a relative crate directory
        // e.g., "/path/to/project/crates/perl-parser/Cargo.toml" -> "crates/perl-parser"
        // Normalize separators to forward slashes for cross-platform compatibility (Windows uses backslash).
        let manifest_normalized = normalize_metadata_path(manifest_path);
        let relative = manifest_normalized
            .strip_prefix(root_normalized.as_str())
            .and_then(|p| p.strip_prefix('/'))
            .and_then(|p| p.strip_suffix("/Cargo.toml"));

        if let Some(rel_dir) = relative
            && crate_dirs.contains(rel_dir)
        {
            names.insert(pkg_name.to_string());
        }
    }

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::normalize_metadata_path;

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
}

/// Resolve one changed crate directory to its authoritative Cargo package name.
pub fn resolve_single_package_name(project_root: &Path, crate_dir: &str) -> Result<String> {
    let normalized = crate_dir.replace('\\', "/");
    let mut dirs = BTreeSet::new();
    dirs.insert(normalized.trim_end_matches('/').to_string());
    let names = resolve_package_names(project_root, &dirs)?;
    names
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("No workspace package found for crate directory: {crate_dir}"))
}
