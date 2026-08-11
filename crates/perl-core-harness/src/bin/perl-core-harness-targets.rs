#![allow(clippy::print_stdout)]

//! Validate the versioned upstream Perl target matrix without executing Perl.

#[path = "perl-core-harness-targets/model.rs"]
mod model;
#[path = "perl-core-harness-targets/contract.rs"]
mod contract;
#[path = "perl-core-harness-targets/matrix.rs"]
mod matrix;

use color_eyre::eyre::{Context, Result, bail};
use model::{
    TargetMatrixIndex, TargetMatrixPart, TargetTopologyDrift, UpstreamTargetMatrix,
};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        bail!(
            "usage: perl-core-harness-targets check <matrix.json|matrix-directory> [drift.json]"
        );
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
    if path.is_dir() {
        return read_matrix_bundle(path);
    }
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let matrix: UpstreamTargetMatrix = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding {}", path.display()))?;
    matrix
        .validate()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(matrix)
}

fn read_matrix_bundle(path: &Path) -> Result<UpstreamTargetMatrix> {
    let index_path = path.join("index.json");
    let bytes = fs::read(&index_path)
        .with_context(|| format!("reading {}", index_path.display()))?;
    let index: TargetMatrixIndex = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding {}", index_path.display()))?;
    index
        .validate()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let expected_files = std::iter::once("index.json".to_string())
        .chain(index.target_files.iter().cloned())
        .collect::<BTreeSet<_>>();
    let actual_files = fs::read_dir(path)
        .with_context(|| format!("reading matrix directory {}", path.display()))?
        .map(|entry| {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading file type for {}", entry.path().display()))?;
            if !file_type.is_file() {
                bail!(
                    "target matrix directory contains non-file {}",
                    entry.path().display()
                );
            }
            Ok(entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual_files != expected_files {
        bail!(
            "target matrix directory members differ from index: expected {expected_files:?}, actual {actual_files:?}"
        );
    }
    let mut parts = Vec::with_capacity(index.target_files.len());
    for relative in &index.target_files {
        let part_path = path.join(relative);
        let bytes = fs::read(&part_path)
            .with_context(|| format!("reading {}", part_path.display()))?;
        let part: TargetMatrixPart = serde_json::from_slice(&bytes)
            .with_context(|| format!("decoding {}", part_path.display()))?;
        parts.push(part);
    }
    index
        .assemble(parts)
        .map_err(|error| color_eyre::eyre::eyre!(error))
}

fn read_drift(path: &Path) -> Result<TargetTopologyDrift> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))
}

#[cfg(test)]
#[path = "perl-core-harness-targets/tests.rs"]
mod tests;
