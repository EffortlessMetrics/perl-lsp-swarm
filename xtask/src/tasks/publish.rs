//! Publishing functionality for crates and VSCode extension

use crate::utils::{project_root, run_cargo_metadata};
use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

/// Read-only packaging verification over the publish allowlist.
///
/// Direct local publication to crates.io is disabled (#15393): public
/// mutation must go through the release controller's gated
/// `publish-crates.yml` workflow, which verifies the sealed-handoff
/// eligibility inputs (exact candidate SHA, candidate run, terminal manifest
/// digest) before publishing. Releases route through
/// `cargo xtask publish-release <version>`.
pub fn publish_crates(dry_run: bool) -> Result<()> {
    if !dry_run {
        bail!(
            "Direct local publication to crates.io is disabled. Dispatch the gated \"Publish to \
             crates.io\" workflow instead: `cargo xtask publish-release <version>` (see \
             docs/release/RUNBOOK.md). `cargo xtask publish-crates --dry-run` remains available \
             as a read-only packaging verifier."
        );
    }

    println!("📦 Verifying crates.io packaging (dry run; nothing is published)");

    let publish_targets = load_publish_targets()?;

    for target in &publish_targets {
        println!("Verifying {}...", target.name);
        let crate_dir = target.manifest_path.parent().ok_or_else(|| {
            eyre!(
                "Invalid manifest path for publish target '{}': {:?}",
                target.name,
                target.manifest_path
            )
        })?;

        let output = Command::new("cargo")
            .current_dir(crate_dir)
            .args(["publish", "--no-verify", "--dry-run"])
            .output()?;
        if !output.status.success() {
            bail!(
                "Dry-run packaging failed for {}: {}",
                target.name,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        println!("✅ {} packaged", target.name);
    }
    println!();
    println!("✅ All crates packaged successfully (dry run; nothing was published).");

    Ok(())
}

#[derive(Deserialize)]
struct CargoMetadata {
    metadata: Option<WorkspaceMetadata>,
    packages: Vec<MetadataPackage>,
}

#[derive(Deserialize)]
struct WorkspaceMetadata {
    publish: Option<PublishMetadata>,
}

#[derive(Deserialize)]
struct PublishMetadata {
    allow: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct MetadataPackage {
    name: String,
    manifest_path: PathBuf,
}

struct PublishTarget {
    name: String,
    manifest_path: PathBuf,
}

fn load_publish_targets() -> Result<Vec<PublishTarget>> {
    let bytes = run_cargo_metadata(true)?;
    let metadata: CargoMetadata = serde_json::from_slice(&bytes)?;

    let allowlist = metadata
        .metadata
        .and_then(|workspace| workspace.publish)
        .and_then(|publish| publish.allow)
        .ok_or_else(|| {
            eyre!(
                "Publish allowlist missing. Add [workspace.metadata.publish.allow] in the workspace Cargo.toml."
            )
        })?;

    if allowlist.is_empty() {
        bail!("Publish allowlist is empty. Add crates to [workspace.metadata.publish.allow].");
    }

    let mut package_map = HashMap::new();
    for package in metadata.packages {
        package_map.insert(package.name, package.manifest_path);
    }

    let mut seen = HashSet::new();
    let mut targets = Vec::new();

    for crate_name in allowlist {
        if !seen.insert(crate_name.clone()) {
            continue;
        }

        let manifest_path = package_map.get(&crate_name).ok_or_else(|| {
            eyre!(
                "Crate '{}' listed in [workspace.metadata.publish.allow] is not a workspace member.",
                crate_name
            )
        })?;

        targets.push(PublishTarget { name: crate_name, manifest_path: manifest_path.clone() });
    }

    Ok(targets)
}

/// Direct marketplace publication is disabled (#15393): an ambient local
/// build must never be published from a developer checkout, and publisher
/// credentials must stay inside the GitHub environment/OIDC boundary instead
/// of argv or inherited child-process environments. The gated
/// `publish-extension.yml` release workflow owns marketplace publication.
pub fn publish_vscode() -> Result<()> {
    bail!(
        "Direct `publish-vscode` marketplace publication is disabled. Publication runs through \
         the gated `publish-extension.yml` release workflow with sealed VSIX artifacts (see \
         docs/release/RUNBOOK.md); publisher credentials stay in the GitHub environment boundary."
    );
}

pub fn publish_release(version: String, dry_run: bool, git_ref: Option<String>) -> Result<()> {
    let root = project_root()?;
    let ref_name = git_ref.unwrap_or_else(|| format!("v{version}"));

    let status = Command::new("gh")
        .current_dir(&root)
        .args([
            "workflow",
            "run",
            "Publish to crates.io",
            "--ref",
            &ref_name,
            "-f",
            &format!("version={version}"),
            "-f",
            &format!("dry_run={dry_run}"),
        ])
        .status()?;

    if !status.success() {
        bail!("publish-release workflow dispatch failed");
    }

    println!("Dispatched \"Publish to crates.io\" for {version} on ref {ref_name}.");
    Ok(())
}

pub fn smoke_test_release(version: String) -> Result<()> {
    let root = project_root()?;
    // Use a relative path with forward slashes so bash (Git Bash / MSYS2 on
    // Windows) does not interpret backslashes in an absolute Windows path as
    // escape sequences. `current_dir(&root)` below anchors the relative path.
    let script = std::path::PathBuf::from("scripts/smoke-test-release.sh");
    let status = Command::new("bash")
        .arg(&script)
        .arg(version)
        .current_dir(&root)
        .env("XTASK_SMOKE_TEST_RELEASE", "1")
        .status()?;

    if !status.success() {
        bail!("smoke-test-release failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{publish_crates, publish_vscode};

    #[test]
    fn direct_crates_publication_refuses_without_dry_run() {
        let err = publish_crates(false).expect_err("direct crates.io publication must refuse");
        let message = format!("{err:#}");
        assert!(
            message.contains("publish-release"),
            "refusal must route to the gated workflow dispatch: {message}"
        );
        assert!(
            message.contains("dry-run"),
            "refusal must name the remaining read-only verifier: {message}"
        );
    }

    #[test]
    fn direct_vscode_publication_refuses() {
        let err = publish_vscode().expect_err("direct marketplace publication must refuse");
        let message = format!("{err:#}");
        assert!(
            message.contains("publish-extension.yml"),
            "refusal must route to the gated release workflow: {message}"
        );
    }
}
