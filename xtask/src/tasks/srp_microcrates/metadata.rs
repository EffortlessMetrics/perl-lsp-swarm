use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub(super) struct Metadata {
    pub(super) packages: Vec<Package>,
    pub(super) workspace_members: Vec<String>,
    pub(super) workspace_root: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Package {
    pub(super) name: String,
    pub(super) id: String,
    pub(super) manifest_path: String,
    pub(super) dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Dependency {
    pub(super) name: String,
}

pub(super) fn cargo_metadata() -> Result<Metadata> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .wrap_err("failed to execute cargo metadata")?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout).wrap_err("failed to parse cargo metadata JSON")
}
