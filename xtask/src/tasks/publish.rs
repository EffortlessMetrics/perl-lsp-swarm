//! Publishing functionality for crates and VSCode extension

use crate::utils::{project_root, run_cargo_metadata};
use color_eyre::eyre::{bail, eyre, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

/// Non-publishing package rehearsal over the publish allowlist.
///
/// Direct local publication is disabled (#16802). The reviewed release
/// transaction (#13768 / #6055) owns candidate admission and publication;
/// `release-orchestration.yml` is its existing workflow entry point.
/// This local dry run neither proves release readiness nor authorizes a release.
pub fn publish_crates(dry_run: bool) -> Result<()> {
    if !dry_run {
        bail!(
            "Direct local publication to crates.io is disabled. Follow the reviewed release \
             transaction (#13768 / #6055) and its `release-orchestration.yml` entry point (see \
             docs/release/RUNBOOK.md). `cargo xtask publish-crates --dry-run` is only a \
             non-publishing package rehearsal; it does not establish release readiness."
        );
    }

    println!("📦 Package rehearsal only (dry run; no publication or release-readiness claim)");

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
    println!("✅ Package rehearsal completed; no publication or release readiness established.");

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
         the reviewed release transaction (#13768 / #6055), including `publish-extension.yml` \
         (see docs/release/RUNBOOK.md); local argv or environment credentials grant no authority."
    );
}

/// Fail-closed refusal for the legacy `publish-release` helper (#17904 / #16802).
///
/// `Publish to crates.io` requires sealed-handoff inputs this command never supplies;
/// the reviewed release transaction owns publication through `release-orchestration.yml`.
pub fn publish_release_refusal() -> &'static str {
    "The legacy `publish-release` workflow dispatch is disabled. It does not supply the \
     required `expected_sha`, `candidate_run_id`, and `manifest_sha256` inputs for \
     `Publish to crates.io`. Follow the reviewed release transaction (#13768 / #6055) and \
     its `release-orchestration.yml` entry point (see docs/release/RUNBOOK.md)."
}

pub fn publish_release(version: String, dry_run: bool, git_ref: Option<String>) -> Result<()> {
    let _ = (version, dry_run, git_ref);
    bail!("{}", publish_release_refusal())
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
    use super::{publish_crates, publish_release, publish_release_refusal, publish_vscode};

    #[test]
    fn direct_crates_publication_refuses_without_dry_run() {
        let err = publish_crates(false).expect_err("direct crates.io publication must refuse");
        let message = format!("{err:#}");
        assert!(
            message.contains("#13768 / #6055") && message.contains("release-orchestration.yml"),
            "refusal must route to the reviewed release transaction: {message}"
        );
        assert!(
            message.contains("dry-run"),
            "refusal must name the remaining non-publishing rehearsal: {message}"
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

    #[test]
    fn legacy_publish_release_refuses_before_dispatch() {
        let refusal = publish_release_refusal();
        assert!(
            refusal.contains("release-orchestration.yml"),
            "refusal must route to the reviewed release orchestration workflow: {refusal}"
        );
        assert!(
            refusal.contains("expected_sha"),
            "refusal must name the missing sealed-handoff inputs: {refusal}"
        );

        let err = publish_release("0.0.0".into(), false, Some("v0.0.0".into()))
            .expect_err("legacy publish-release dispatch must refuse before spawning gh");
        let message = format!("{err:#}");
        assert!(
            message.contains("release-orchestration.yml"),
            "publish_release must fail closed with orchestration routing: {message}"
        );
        assert!(
            !message.contains("workflow dispatch failed"),
            "must not reach gh workflow dispatch: {message}"
        );
    }
}
