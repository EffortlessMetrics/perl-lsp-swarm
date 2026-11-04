//! Read-only workspace manifest / `Cargo.lock` coherence (#13411).
//!
//! The semantic contract is that `cargo metadata --locked` can resolve the
//! exact committed workspace graph without mutating the lockfile.

use std::path::Path;
use std::process::Command;

use color_eyre::eyre::{Context, Result, bail};

pub const REMEDIATION: &str = "Run `cargo check --workspace --locked` (or `cargo update -w` when \
                                intentionally changing dependencies) and commit the resulting \
                                Cargo.lock.";

/// Normalized coherence outcomes for receipts and gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoherenceStatus {
    Coherent,
    StaleLock,
    InvalidManifest,
    ResolutionNotProven,
}

impl CoherenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coherent => "COHERENT",
            Self::StaleLock => "STALE_LOCK",
            Self::InvalidManifest => "INVALID_MANIFEST",
            Self::ResolutionNotProven => "RESOLUTION_NOT_PROVEN",
        }
    }
}

/// Whether a changed path may alter the resolved Cargo graph.
pub fn is_graph_affecting_path(path: &str) -> bool {
    let path = path.trim_start_matches("./");
    match path {
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml" => true,
        ".cargo/config.toml" | ".cargo/config" => true,
        _ if path.ends_with("/Cargo.toml") => true,
        _ => false,
    }
}

/// Run `cargo metadata --locked` against `root` (or `--manifest-path` when set).
pub fn check_locked_resolution(
    root: &Path,
    manifest_path: Option<&Path>,
) -> Result<CoherenceStatus> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--locked", "--format-version", "1"]);
    if let Some(manifest) = manifest_path {
        cmd.arg("--manifest-path").arg(manifest);
    }
    cmd.current_dir(root);
    let output =
        cmd.output().with_context(|| "spawning `cargo metadata --locked --format-version 1`")?;
    if output.status.success() {
        return Ok(CoherenceStatus::Coherent);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    if is_stale_lock_message(&combined) {
        return Ok(CoherenceStatus::StaleLock);
    }
    if combined.contains("failed to parse manifest")
        || combined.contains("TOML parse error")
        || combined.contains("could not parse")
    {
        return Ok(CoherenceStatus::InvalidManifest);
    }
    Ok(CoherenceStatus::ResolutionNotProven)
}

fn is_stale_lock_message(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("lock file")
        || lower.contains("cargo.lock")
        || lower.contains("needs to be updated")
        || lower.contains("requires `cargo update`")
        || lower.contains("requires cargo update")
        || lower.contains("not found in lock file")
        || lower.contains("not included in the lockfile")
        || lower.contains("did not match any package")
        || lower.contains("no matching package named")
}

/// CLI entry for `cargo xtask cargo-lock-coherence`.
pub fn run_cli() -> Result<()> {
    let root = crate::utils::project_root()?;
    run_for_root(&root)
}

pub fn run_for_root(root: &Path) -> Result<()> {
    match check_locked_resolution(root, None)? {
        CoherenceStatus::Coherent => {
            println!("cargo-lock-coherence: {}", CoherenceStatus::Coherent.as_str());
            Ok(())
        }
        status => {
            eprintln!("cargo-lock-coherence: {}", status.as_str());
            eprintln!("{REMEDIATION}");
            bail!("manifest/lock coherence check failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_minimal_workspace(dir: &Path, manifest: &str, lock: &str) {
        fs::create_dir_all(dir.join("src")).expect("fixture src dir");
        fs::write(dir.join("Cargo.toml"), manifest).expect("fixture manifest");
        fs::write(dir.join("Cargo.lock"), lock).expect("fixture lock");
        fs::write(dir.join("src/lib.rs"), "").expect("fixture lib");
    }

    #[test]
    fn coherent_fixture_passes_locked_metadata() -> Result<()> {
        let dir = tempfile::tempdir()?;
        write_minimal_workspace(
            dir.path(),
            "[package]\nname = \"coherent-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
            "version = 3\n\n[[package]]\nname = \"coherent-fixture\"\nversion = \"0.0.0\"\n",
        );
        assert_eq!(check_locked_resolution(dir.path(), None)?, CoherenceStatus::Coherent);
        Ok(())
    }

    #[test]
    fn missing_lock_edge_fails_as_stale_lock() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("dep/src"))?;
        fs::write(
            dir.path().join("dep/Cargo.toml"),
            "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )?;
        fs::write(dir.path().join("dep/src/lib.rs"), "")?;
        write_minimal_workspace(
            dir.path(),
            "[package]\nname = \"root-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
             [dev-dependencies]\ndep = { path = \"dep\" }\n",
            "version = 3\n\n[[package]]\nname = \"root-fixture\"\nversion = \"0.0.0\"\n",
        );
        let status = check_locked_resolution(dir.path(), None)?;
        assert_ne!(status, CoherenceStatus::Coherent);
        assert_eq!(status, CoherenceStatus::StaleLock);
        Ok(())
    }

    #[test]
    fn graph_classifier_skips_unrelated_docs() {
        assert!(!is_graph_affecting_path("docs/foo.md"));
        assert!(is_graph_affecting_path("crates/xtask/Cargo.toml"));
        assert!(is_graph_affecting_path("Cargo.lock"));
    }
}
