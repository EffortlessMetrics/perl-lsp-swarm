//! Versioned, deterministic identity for corpus assets.
//!
//! This is the first topology authority for the populations currently discovered by
//! the files module. Additional corpus layers can add asset kinds here without
//! creating another path list.

use crate::files::{CorpusLayer, CorpusPaths, get_corpus_files_from};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Version of the serialized corpus topology contract.
pub const CORPUS_TOPOLOGY_SCHEMA_VERSION: u32 = 1;

/// A typed kind of corpus asset.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CorpusAssetKind {
    /// Plain Perl source consumed as a parser or fuzz input.
    PerlSource,
    /// Sectioned Tree-sitter-style corpus text.
    SectionedCorpus,
}

/// Whether an asset is required for the current topology.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum AssetRequirement {
    /// The asset is part of the checked-in topology contract.
    Required,
    /// The asset may be absent without invalidating the topology.
    Optional,
}

/// One corpus asset with a stable identity independent of absolute checkout paths.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CorpusAsset {
    /// Stable ID, formed from the normalized path relative to the topology root.
    pub id: String,
    /// Current discovery layer.
    pub layer: CorpusLayer,
    /// Semantic asset kind.
    pub kind: CorpusAssetKind,
    /// Path relative to the topology root, using slash separators.
    pub relative_path: String,
    /// Whether absence of the asset is a topology error.
    pub requirement: AssetRequirement,
}

/// Deterministic, versioned topology for currently supported corpus assets.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CorpusTopology {
    /// Serialized topology schema version.
    pub schema_version: u32,
    /// Root used to resolve the relative asset paths.
    pub root: PathBuf,
    /// Assets in stable ID order.
    pub assets: Vec<CorpusAsset>,
}

impl CorpusTopology {
    /// Discover the topology using the normal corpus-root precedence rules.
    #[must_use]
    pub fn discover() -> Self {
        Self::from_paths(&CorpusPaths::discover())
    }

    /// Build a topology from an explicit corpus root.
    #[must_use]
    pub fn from_paths(paths: &CorpusPaths) -> Self {
        let mut assets = get_corpus_files_from(paths)
            .into_iter()
            .filter_map(|file| {
                let relative_path = file.path.strip_prefix(&paths.root).ok()?;
                let relative_path = normalize_relative_path(relative_path);
                let kind = asset_kind(&file.path);
                Some(CorpusAsset {
                    id: relative_path.clone(),
                    layer: file.layer,
                    kind,
                    relative_path,
                    requirement: AssetRequirement::Required,
                })
            })
            .collect::<Vec<_>>();
        assets.sort_by(|left, right| left.id.cmp(&right.id));

        Self {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            root: paths.root.clone(),
            assets,
        }
    }

    /// Resolve an asset's checked-in path from its stable ID.
    #[must_use]
    pub fn asset_path(&self, asset: &CorpusAsset) -> PathBuf {
        self.root.join(&asset.relative_path)
    }
}

fn asset_kind(path: &Path) -> CorpusAssetKind {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("txt") => CorpusAssetKind::SectionedCorpus,
        _ => CorpusAssetKind::PerlSource,
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn topology_has_stable_ids_and_relative_paths() {
        let root = tempfile::tempdir().expect("temporary directory");
        let test_file = root.path().join("test_corpus").join("nested").join("case.pl");
        let fuzz_file = root.path().join("crates/perl-corpus/fuzz").join("seed.pl");
        fs::create_dir_all(test_file.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(fuzz_file.parent().expect("parent")).expect("mkdir");
        fs::write(&test_file, "my $x = 1;").expect("write");
        fs::write(&fuzz_file, "my $y = 2;").expect("write");

        let paths = CorpusPaths::from_root(root.path().to_path_buf());
        let topology = CorpusTopology::from_paths(&paths);

        assert_eq!(topology.schema_version, CORPUS_TOPOLOGY_SCHEMA_VERSION);
        assert_eq!(topology.assets.len(), 2);
        assert_eq!(topology.assets[0].id, "crates/perl-corpus/fuzz/seed.pl");
        assert_eq!(topology.assets[1].id, "test_corpus/nested/case.pl");
        assert!(topology.assets.iter().all(|asset| !asset.relative_path.starts_with('/')));
        assert!(topology.assets.iter().all(|asset| topology.asset_path(asset).is_file()));
    }

    #[test]
    fn topology_order_and_serialization_are_deterministic() {
        let root = tempfile::tempdir().expect("temporary directory");
        let first = root.path().join("test_corpus").join("z.pl");
        let second = root.path().join("test_corpus").join("a.pl");
        fs::create_dir_all(first.parent().expect("parent")).expect("mkdir");
        fs::write(&first, "1;").expect("write");
        fs::write(&second, "2;").expect("write");

        let paths = CorpusPaths::from_root(root.path().to_path_buf());
        let topology = CorpusTopology::from_paths(&paths);
        let json = serde_json::to_string(&topology).expect("serialize");

        assert_eq!(topology.assets[0].id, "test_corpus/a.pl");
        assert_eq!(topology.assets[1].id, "test_corpus/z.pl");
        assert_eq!(json, serde_json::to_string(&CorpusTopology::from_paths(&paths)).expect("serialize"));
    }
}
