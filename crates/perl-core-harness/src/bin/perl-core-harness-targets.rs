#![allow(clippy::print_stdout)]

//! Validate the versioned upstream Perl target matrix without executing Perl.

#[path = "perl-core-harness-targets/model.rs"]
mod model;
#[path = "perl-core-harness-targets/contract.rs"]
mod contract;
#[path = "perl-core-harness-targets/matrix.rs"]
mod matrix;

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
#[path = "perl-core-harness-targets/tests.rs"]
mod tests;
