//! Versioned, deterministic identity for corpus assets.
//!
//! This is the first topology authority for the populations already discovered by
//! [`crate::files`] plus the checked-in extensionless and text fuzz regressions.
//! Additional corpus layers remain explicit follow-up work under issue #6699.

use crate::files::{CorpusLayer, CorpusPaths, get_corpus_files_from};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Version of the serialized corpus topology contract.
pub const CORPUS_TOPOLOGY_SCHEMA_VERSION: u32 = 1;

/// A typed corpus layer independent of the legacy discovery representation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CorpusAssetLayer {
    /// Gap-coverage and integration fixtures rooted at `test_corpus/`.
    TestCorpus,
    /// Checked-in fuzz regression fixtures.
    Fuzz,
}

impl From<CorpusLayer> for CorpusAssetLayer {
    fn from(layer: CorpusLayer) -> Self {
        match layer {
            CorpusLayer::TestCorpus => Self::TestCorpus,
            CorpusLayer::Fuzz => Self::Fuzz,
        }
    }
}

/// A typed kind of corpus asset.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CorpusAssetKind {
    /// Perl source consumed as a parser or fuzz input, regardless of file extension.
    PerlSource,
    /// Text fixture containing one or more corpus regression cases.
    TextFixture,
}

/// Whether an asset is required for the current topology.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AssetRequirement {
    /// The asset is part of the checked-in topology contract.
    Required,
    /// The asset may be absent without invalidating the topology.
    Optional,
}

/// One corpus asset with a stable identity independent of absolute checkout paths.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CorpusAsset {
    /// Stable ID formed from the normalized path relative to the topology root.
    pub id: String,
    /// Corpus layer that owns the asset.
    pub layer: CorpusAssetLayer,
    /// Semantic asset kind.
    pub kind: CorpusAssetKind,
    /// Path relative to the topology root, using slash separators.
    pub relative_path: String,
    /// Whether absence of the asset is a topology error.
    pub requirement: AssetRequirement,
}

/// Deterministic, versioned topology for the currently supported corpus assets.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CorpusTopology {
    /// Serialized topology schema version.
    pub schema_version: u32,
    /// Assets in stable ID order.
    pub assets: Vec<CorpusAsset>,
    /// Runtime checkout root. Absolute host paths are excluded from serialization.
    #[serde(skip, default)]
    root: PathBuf,
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
            .filter_map(|file| asset_from_path(paths, &file.path, file.layer.into()))
            .collect::<Vec<_>>();

        assets.extend(
            collect_additional_fuzz_assets(&paths.fuzz)
                .into_iter()
                .filter_map(|path| asset_from_path(paths, &path, CorpusAssetLayer::Fuzz)),
        );

        assets.sort_by(|left, right| left.id.cmp(&right.id));

        Self {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets,
            root: paths.root.clone(),
        }
    }

    /// Bind a runtime checkout root after loading a serialized topology.
    #[must_use]
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self
    }

    /// Return the runtime checkout root used to resolve assets.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve an asset's checked-in path from its stable identity.
    #[must_use]
    pub fn asset_path(&self, asset: &CorpusAsset) -> PathBuf {
        self.root.join(&asset.relative_path)
    }
}

fn asset_from_path(
    paths: &CorpusPaths,
    path: &Path,
    layer: CorpusAssetLayer,
) -> Option<CorpusAsset> {
    let relative_path = path.strip_prefix(&paths.root).ok()?;
    let relative_path = normalize_relative_path(relative_path);

    Some(CorpusAsset {
        id: relative_path.clone(),
        layer,
        kind: asset_kind(path),
        relative_path,
        requirement: AssetRequirement::Required,
    })
}

fn asset_kind(path: &Path) -> CorpusAssetKind {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("txt") => CorpusAssetKind::TextFixture,
        _ => CorpusAssetKind::PerlSource,
    }
}

fn collect_additional_fuzz_assets(root: &Path) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();

            if file_name.starts_with('.') || file_name.starts_with('_') {
                continue;
            }

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_additional_fuzz_asset(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    files.dedup();
    files
}

fn is_additional_fuzz_asset(path: &Path) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        None => path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("crash-")),
        Some(extension) => extension.eq_ignore_ascii_case("txt"),
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(path: &Path, source: &str) {
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, source).expect("write fixture");
    }

    #[test]
    fn topology_includes_supported_test_and_fuzz_assets() {
        let root = tempfile::tempdir().expect("temporary directory");
        let test_file = root.path().join("test_corpus/nested/case.pl");
        let fuzz_perl = root.path().join("crates/perl-corpus/fuzz/seed.pl");
        let fuzz_text = root.path().join("crates/perl-corpus/fuzz/heredoc_validation.txt");
        let fuzz_crash = root.path().join("crates/perl-corpus/fuzz/crash-deadbeef");
        let fuzz_unclassified = root.path().join("crates/perl-corpus/fuzz/notes");
        let fuzz_readme = root.path().join("crates/perl-corpus/fuzz/README.md");

        write_fixture(&test_file, "my $x = 1;");
        write_fixture(&fuzz_perl, "my $y = 2;");
        write_fixture(&fuzz_text, "xqN<<\"");
        write_fixture(&fuzz_crash, "xqN<<\"");
        write_fixture(&fuzz_unclassified, "metadata without a declared kind");
        write_fixture(&fuzz_readme, "metadata only");

        let topology = CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()));
        let ids = topology.assets.iter().map(|asset| asset.id.as_str()).collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "crates/perl-corpus/fuzz/crash-deadbeef",
                "crates/perl-corpus/fuzz/heredoc_validation.txt",
                "crates/perl-corpus/fuzz/seed.pl",
                "test_corpus/nested/case.pl",
            ]
        );
        assert!(!ids.iter().any(|id| id.ends_with("README.md")));
        assert!(!ids.iter().any(|id| id.ends_with("/notes")));

        let text = topology
            .assets
            .iter()
            .find(|asset| asset.id.ends_with("heredoc_validation.txt"))
            .expect("text fixture");
        assert_eq!(text.kind, CorpusAssetKind::TextFixture);
        assert_eq!(text.layer, CorpusAssetLayer::Fuzz);

        let crash = topology
            .assets
            .iter()
            .find(|asset| asset.id.ends_with("crash-deadbeef"))
            .expect("extensionless crash fixture");
        assert_eq!(crash.kind, CorpusAssetKind::PerlSource);
        assert!(topology.asset_path(crash).is_file());
    }

    #[test]
    fn topology_serialization_is_checkout_independent_and_deterministic() {
        let first_root = tempfile::tempdir().expect("first temporary directory");
        let second_root = tempfile::tempdir().expect("second temporary directory");

        write_fixture(&first_root.path().join("test_corpus/z.pl"), "1;");
        write_fixture(&first_root.path().join("test_corpus/a.pl"), "2;");
        write_fixture(&second_root.path().join("test_corpus/a.pl"), "2;");
        write_fixture(&second_root.path().join("test_corpus/z.pl"), "1;");

        let first = CorpusTopology::from_paths(&CorpusPaths::from_root(first_root.path().to_path_buf()));
        let second =
            CorpusTopology::from_paths(&CorpusPaths::from_root(second_root.path().to_path_buf()));
        let first_json = serde_json::to_string(&first).expect("serialize first topology");
        let second_json = serde_json::to_string(&second).expect("serialize second topology");

        assert_ne!(first.root(), second.root());
        assert_eq!(first_json, second_json);
        assert!(!first_json.contains(&first_root.path().to_string_lossy().to_string()));
        assert_eq!(
            first.assets.iter().map(|asset| asset.id.as_str()).collect::<Vec<_>>(),
            vec!["test_corpus/a.pl", "test_corpus/z.pl"]
        );

        let loaded: CorpusTopology = serde_json::from_str(&first_json).expect("load topology");
        assert_eq!(loaded.root(), Path::new(""));
        let rebound = loaded.with_root(first_root.path());
        assert!(rebound.assets.iter().all(|asset| rebound.asset_path(asset).is_file()));
    }

    #[test]
    fn normalized_ids_use_forward_slashes() {
        assert_eq!(
            normalize_relative_path(Path::new("test_corpus\\nested\\case.pl")),
            "test_corpus/nested/case.pl"
        );
    }
}
