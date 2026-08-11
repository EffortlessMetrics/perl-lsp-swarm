#![allow(clippy::print_stdout)]

//! Validate the versioned upstream Perl target matrix without executing Perl.

#[path = "perl-core-harness-targets/model.rs"]
mod model;

use color_eyre::eyre::{Context, Result, bail};
use model::{TargetTopologyDrift, UpstreamTargetMatrix};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        bail!("usage: perl-core-harness-targets check <matrix.json> [drift.json]");
    };
    if command.as_os_str() != OsStr::new("check") {
        bail!("unsupported command {:?}; expected check", command);
    }
    let matrix_path = args
        .next()
        .map(PathBuf::from)
        .context("check requires a target matrix path")?;
    let drift_path = args.next().map(PathBuf::from);
    if let Some(extra) = args.next() {
        bail!("unexpected extra argument {:?}", extra);
    }

    let matrix = read_matrix(&matrix_path)?;
    let fingerprint = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    if let Some(path) = drift_path.as_deref() {
        let drift = read_drift(path)?;
        drift
            .validate_against(&matrix, &fingerprint)
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
    }
    println!("target matrix valid: {fingerprint}");
    Ok(())
}

fn read_matrix(path: &Path) -> Result<UpstreamTargetMatrix> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let matrix: UpstreamTargetMatrix = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding {}", path.display()))?;
    matrix
        .validate()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(matrix)
}

fn read_drift(path: &Path) -> Result<TargetTopologyDrift> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION;

    fn repo_file(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
    }

    #[test]
    fn checked_in_target_matrix_is_valid_and_stable() -> Result<()> {
        let matrix = read_matrix(&repo_file(
            ".ci/perl-core-harness/upstream-targets-5.42.2.v1.json",
        ))?;
        let first = matrix
            .fingerprint()
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        let second = matrix
            .fingerprint()
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        Ok(())
    }

    #[test]
    fn checked_in_blead_drift_is_bound_to_the_pinned_matrix() -> Result<()> {
        let matrix = read_matrix(&repo_file(
            ".ci/perl-core-harness/upstream-targets-5.42.2.v1.json",
        ))?;
        let drift = read_drift(&repo_file(
            ".ci/perl-core-harness/upstream-targets-blead-drift.v1.json",
        ))?;
        assert_eq!(drift.schema_version, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION);
        let fingerprint = matrix
            .fingerprint()
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        drift
            .validate_against(&matrix, &fingerprint)
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        Ok(())
    }

    #[test]
    fn drift_fails_when_the_pinned_fingerprint_changes() -> Result<()> {
        let matrix = read_matrix(&repo_file(
            ".ci/perl-core-harness/upstream-targets-5.42.2.v1.json",
        ))?;
        let mut drift = read_drift(&repo_file(
            ".ci/perl-core-harness/upstream-targets-blead-drift.v1.json",
        ))?;
        drift.pinned_matrix_fingerprint = "0".repeat(64);
        let fingerprint = matrix
            .fingerprint()
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        assert!(drift.validate_against(&matrix, &fingerprint).is_err());
        Ok(())
    }
}
