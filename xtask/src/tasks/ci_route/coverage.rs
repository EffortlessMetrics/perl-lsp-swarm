use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct CoverageProofPackReceipt {
    pub(super) id: String,
    pub(super) files: Vec<String>,
    pub(super) commands: Vec<String>,
    pub(super) coverage_filters: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoveragePackManifest {
    pub(super) pack: Vec<CoveragePack>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct CoveragePack {
    pub(super) id: String,
    pub(super) files: Vec<String>,
    pub(super) commands: Vec<String>,
    pub(super) coverage_filters: Vec<String>,
    #[serde(default = "default_lcov")]
    pub(super) lcov: bool,
}

const COVERAGE_PACKS_TOML: &str = include_str!("../../../../.ci/coverage-packs.toml");
pub(super) const NON_LCOV_COVERAGE_SKIP_REASON: &str =
    "non-LCOV CI policy/routing surface; covered by focused CI gates";
pub(super) const NON_SOURCE_LCOV_COVERAGE_SKIP_REASON: &str =
    "LCOV coverage pack matched only non-source files; covered by focused CI gates";

#[cfg(test)]
pub(super) fn coverage_proof_pack_receipts(
    selector: &[String],
) -> Result<Vec<CoverageProofPackReceipt>> {
    let manifest = coverage_pack_manifest()?;
    let changed_files: Vec<String> = manifest
        .pack
        .iter()
        .filter(|pack| selector.iter().any(|selected| selected == &pack.id))
        .flat_map(|pack| pack.files.iter().cloned())
        .filter(|path| is_lcov_source_path(path))
        .collect();
    let (_, _, proof_packs) = coverage_proof_pack_selection(selector, &changed_files)?;
    Ok(proof_packs)
}

pub(super) fn coverage_proof_pack_selection(
    selector: &[String],
    changed_files: &[String],
) -> Result<(Vec<String>, BTreeMap<String, String>, Vec<CoverageProofPackReceipt>)> {
    let manifest = coverage_pack_manifest()?;
    let packs_by_id: BTreeMap<&str, &CoveragePack> =
        manifest.pack.iter().map(|pack| (pack.id.as_str(), pack)).collect();
    let mut selected = Vec::new();
    let mut skipped = BTreeMap::new();
    let mut proof_packs = Vec::new();
    for pack_id in selector {
        let Some(pack) = packs_by_id.get(pack_id.as_str()) else {
            bail!("coverage pack `{pack_id}` is missing from .ci/coverage-packs.toml");
        };
        if !pack.lcov {
            skipped.insert(pack_id.clone(), NON_LCOV_COVERAGE_SKIP_REASON.to_string());
            continue;
        }
        if !pack_matches_lcov_source(pack, changed_files) {
            skipped.insert(pack_id.clone(), NON_SOURCE_LCOV_COVERAGE_SKIP_REASON.to_string());
            continue;
        }
        selected.push(pack_id.clone());
        proof_packs.push(CoverageProofPackReceipt {
            id: pack.id.clone(),
            files: pack.files.clone(),
            commands: pack.commands.clone(),
            coverage_filters: pack.coverage_filters.clone(),
        });
    }
    Ok((selected, skipped, proof_packs))
}

fn pack_matches_lcov_source(pack: &CoveragePack, paths: &[String]) -> bool {
    paths.iter().any(|path| {
        is_lcov_source_path(path)
            && pack.files.iter().any(|pattern| matches_coverage_pattern(path, pattern))
    })
}

fn is_lcov_source_path(path: &str) -> bool {
    path.ends_with(".rs")
        && !path.starts_with("xtask/tests/")
        && !path.contains("/tests/")
        && (path.starts_with("xtask/src/") || path.starts_with("crates/"))
}

fn matches_coverage_pattern(path: &str, pattern: &str) -> bool {
    let normalized_pattern = pattern.replace('\\', "/");
    if let Some(suffix) = normalized_pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{suffix}"));
    }
    if normalized_pattern.ends_with('/') {
        return path.starts_with(&normalized_pattern);
    }
    path == normalized_pattern || path.starts_with(&normalized_pattern)
}

pub(super) fn coverage_pack_manifest() -> Result<CoveragePackManifest> {
    parse_coverage_pack_manifest(COVERAGE_PACKS_TOML)
}

fn default_lcov() -> bool {
    true
}

pub(super) fn parse_coverage_pack_manifest(contents: &str) -> Result<CoveragePackManifest> {
    let manifest: CoveragePackManifest = toml::from_str(contents)?;
    let mut ids = BTreeSet::new();
    for pack in &manifest.pack {
        if pack.id.trim().is_empty() {
            bail!("coverage pack id must not be empty");
        }
        if pack.files.is_empty() {
            bail!("coverage pack `{}` must list at least one file", pack.id);
        }
        if pack.commands.is_empty() {
            bail!("coverage pack `{}` must list at least one command", pack.id);
        }
        if pack.coverage_filters.is_empty() {
            bail!("coverage pack `{}` must list at least one coverage filter", pack.id);
        }
        if !ids.insert(pack.id.as_str()) {
            bail!("duplicate coverage pack id `{}`", pack.id);
        }
    }
    Ok(manifest)
}
