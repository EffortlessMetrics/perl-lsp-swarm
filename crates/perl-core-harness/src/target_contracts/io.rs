//! Offline target-matrix and topology-drift loading.

use crate::model::{
    TargetMatrixIndex, TargetMatrixPart, TargetTopologyDrift, UpstreamTargetMatrix,
};
use color_eyre::eyre::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn read_matrix(path: &Path) -> Result<UpstreamTargetMatrix> {
    if path.is_dir() {
        return read_matrix_bundle(path);
    }
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let matrix: UpstreamTargetMatrix =
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    matrix.validate().map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(matrix)
}

pub fn read_drift(path: &Path) -> Result<TargetTopologyDrift> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))
}

fn read_matrix_bundle(path: &Path) -> Result<UpstreamTargetMatrix> {
    let index_path = path.join("index.json");
    let bytes =
        fs::read(&index_path).with_context(|| format!("reading {}", index_path.display()))?;
    let index: TargetMatrixIndex = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding {}", index_path.display()))?;
    index.validate().map_err(|error| color_eyre::eyre::eyre!(error))?;
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
                bail!("target matrix directory contains non-file {}", entry.path().display());
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
        let bytes =
            fs::read(&part_path).with_context(|| format!("reading {}", part_path.display()))?;
        let part: TargetMatrixPart = serde_json::from_slice(&bytes)
            .with_context(|| format!("decoding {}", part_path.display()))?;
        parts.push(part);
    }
    index.assemble(parts).map_err(|error| color_eyre::eyre::eyre!(error))
}
