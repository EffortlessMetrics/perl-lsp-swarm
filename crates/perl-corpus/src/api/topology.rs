//! Versioned, deterministic identity for corpus assets.
//!
//! This is the first topology authority for the populations already discovered by
//! [`crate::files`] plus the checked-in extensionless and text fuzz regressions.
//! Additional corpus layers remain explicit follow-up work under issue #6699.

use crate::files::{CorpusLayer, CorpusPaths, get_corpus_files_from};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    /// Path relative to the topology root, using slash separators between components.
    pub relative_path: String,
    /// Whether absence of the asset is a topology error.
    pub requirement: AssetRequirement,
}

/// Failure to discover, validate, or resolve a corpus topology asset.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusTopologyError {
    /// An asset path is outside the configured topology root.
    PathOutsideRoot {
        /// Rejected asset path.
        path: PathBuf,
        /// Configured topology root.
        root: PathBuf,
    },
    /// A path cannot be represented injectively as UTF-8 topology identity.
    NonUtf8Path {
        /// Rejected path.
        path: PathBuf,
    },
    /// A serialized relative path is absolute, traversing, empty, or non-canonical.
    InvalidRelativePath {
        /// Rejected serialized path.
        path: String,
        /// Stable machine-readable reason token.
        reason: &'static str,
    },
    /// A deserialized asset ID disagrees with its canonical relative path.
    AssetIdentityMismatch {
        /// Asset ID.
        id: String,
        /// Asset relative path.
        relative_path: String,
    },
    /// Two discovered assets resolve to the same stable identity.
    DuplicateAssetId {
        /// Duplicated asset ID.
        id: String,
    },
    /// A deserialized topology has not been bound to a runtime checkout root.
    RootNotBound,
    /// Filesystem discovery failed.
    Io {
        /// Path being inspected.
        path: PathBuf,
        /// Rendered operating-system error.
        message: String,
    },
}

impl fmt::Display for CorpusTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathOutsideRoot { path, root } => {
                write!(
                    formatter,
                    "corpus asset {} is outside root {}",
                    path.display(),
                    root.display()
                )
            }
            Self::NonUtf8Path { path } => {
                write!(formatter, "corpus path is not valid UTF-8: {}", path.display())
            }
            Self::InvalidRelativePath { path, reason } => {
                write!(formatter, "invalid corpus relative path {path:?}: {reason}")
            }
            Self::AssetIdentityMismatch { id, relative_path } => {
                write!(
                    formatter,
                    "corpus asset ID {id:?} does not match relative path {relative_path:?}"
                )
            }
            Self::DuplicateAssetId { id } => {
                write!(formatter, "duplicate corpus asset ID: {id}")
            }
            Self::RootNotBound => formatter.write_str("corpus topology has no bound runtime root"),
            Self::Io { path, message } => {
                write!(formatter, "failed to inspect corpus path {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for CorpusTopologyError {}

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
    root: Option<PathBuf>,
}

impl CorpusTopology {
    /// Discover the topology using the normal corpus-root precedence rules.
    pub fn discover() -> Result<Self, CorpusTopologyError> {
        Self::from_paths(&CorpusPaths::discover())
    }

    /// Build a topology from an explicit corpus root.
    pub fn from_paths(paths: &CorpusPaths) -> Result<Self, CorpusTopologyError> {
        let mut assets = Vec::new();

        for file in get_corpus_files_from(paths) {
            assets.push(asset_from_path(paths, &file.path, file.layer.into())?);
        }

        for path in collect_additional_fuzz_assets(&paths.fuzz)? {
            assets.push(asset_from_path(paths, &path, CorpusAssetLayer::Fuzz)?);
        }

        assets.sort_by(|left, right| left.id.cmp(&right.id));
        if let Some(duplicate) = assets.windows(2).find(|pair| pair[0].id == pair[1].id) {
            return Err(CorpusTopologyError::DuplicateAssetId { id: duplicate[0].id.clone() });
        }

        Ok(Self {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets,
            root: Some(paths.root.clone()),
        })
    }

    /// Bind a runtime checkout root after loading a serialized topology.
    #[must_use]
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = Some(root.into());
        self
    }

    /// Return the runtime checkout root used to resolve assets.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Resolve an asset's checked-in path from its stable identity.
    pub fn asset_path(&self, asset: &CorpusAsset) -> Result<PathBuf, CorpusTopologyError> {
        let root = self.root.as_deref().ok_or(CorpusTopologyError::RootNotBound)?;
        validate_asset_identity(asset)?;
        Ok(root.join(Path::new(&asset.relative_path)))
    }
}

fn asset_from_path(
    paths: &CorpusPaths,
    path: &Path,
    layer: CorpusAssetLayer,
) -> Result<CorpusAsset, CorpusTopologyError> {
    let relative = path.strip_prefix(&paths.root).map_err(|_| {
        CorpusTopologyError::PathOutsideRoot { path: path.to_path_buf(), root: paths.root.clone() }
    })?;
    let relative_path = canonical_relative_path(relative)?;

    Ok(CorpusAsset {
        id: relative_path.clone(),
        layer,
        kind: asset_kind(path),
        relative_path,
        requirement: AssetRequirement::Required,
    })
}

fn validate_asset_identity(asset: &CorpusAsset) -> Result<(), CorpusTopologyError> {
    if asset.id != asset.relative_path {
        return Err(CorpusTopologyError::AssetIdentityMismatch {
            id: asset.id.clone(),
            relative_path: asset.relative_path.clone(),
        });
    }

    let canonical = canonical_relative_path(Path::new(&asset.relative_path))?;
    if canonical != asset.relative_path {
        return Err(CorpusTopologyError::InvalidRelativePath {
            path: asset.relative_path.clone(),
            reason: "non_canonical",
        });
    }

    Ok(())
}

fn canonical_relative_path(path: &Path) -> Result<String, CorpusTopologyError> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| CorpusTopologyError::NonUtf8Path { path: path.to_path_buf() })?;
                parts.push(value);
            }
            Component::CurDir => {
                return Err(CorpusTopologyError::InvalidRelativePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "current_directory_component",
                });
            }
            Component::ParentDir => {
                return Err(CorpusTopologyError::InvalidRelativePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "parent_directory_component",
                });
            }
            Component::RootDir => {
                return Err(CorpusTopologyError::InvalidRelativePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "absolute_path",
                });
            }
            Component::Prefix(_) => {
                return Err(CorpusTopologyError::InvalidRelativePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "path_prefix",
                });
            }
        }
    }

    if parts.is_empty() {
        return Err(CorpusTopologyError::InvalidRelativePath {
            path: path.to_string_lossy().into_owned(),
            reason: "empty_path",
        });
    }

    Ok(parts.join("/"))
}

fn asset_kind(path: &Path) -> CorpusAssetKind {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("txt") => CorpusAssetKind::TextFixture,
        _ => CorpusAssetKind::PerlSource,
    }
}

fn collect_additional_fuzz_assets(root: &Path) -> Result<Vec<PathBuf>, CorpusTopologyError> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| CorpusTopologyError::Io {
            path: directory.clone(),
            message: error.to_string(),
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| CorpusTopologyError::Io {
                path: directory.clone(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            let file_name = entry
                .file_name()
                .into_string()
                .map_err(|_| CorpusTopologyError::NonUtf8Path { path: path.clone() })?;
            let file_type = entry.file_type().map_err(|error| CorpusTopologyError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;

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
    Ok(files)
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

        let topology =
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()))
                .expect("build topology");
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
        assert!(topology.asset_path(crash).expect("resolve crash fixture").is_file());
    }

    #[test]
    fn topology_serialization_is_checkout_independent_and_deterministic() {
        let first_root = tempfile::tempdir().expect("first temporary directory");
        let second_root = tempfile::tempdir().expect("second temporary directory");

        write_fixture(&first_root.path().join("test_corpus/z.pl"), "1;");
        write_fixture(&first_root.path().join("test_corpus/a.pl"), "2;");
        write_fixture(&second_root.path().join("test_corpus/a.pl"), "2;");
        write_fixture(&second_root.path().join("test_corpus/z.pl"), "1;");

        let first =
            CorpusTopology::from_paths(&CorpusPaths::from_root(first_root.path().to_path_buf()))
                .expect("build first topology");
        let second =
            CorpusTopology::from_paths(&CorpusPaths::from_root(second_root.path().to_path_buf()))
                .expect("build second topology");
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
        let asset = loaded.assets.first().expect("loaded asset");
        assert_eq!(loaded.asset_path(asset), Err(CorpusTopologyError::RootNotBound));

        let rebound = loaded.with_root(first_root.path());
        assert!(
            rebound
                .assets
                .iter()
                .all(|asset| rebound.asset_path(asset).is_ok_and(|path| path.is_file()))
        );
    }

    #[test]
    fn loaded_asset_paths_fail_closed() {
        let root = tempfile::tempdir().expect("temporary directory");
        let topology = CorpusTopology {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets: Vec::new(),
            root: Some(root.path().to_path_buf()),
        };

        for relative_path in ["../outside.pl", "/tmp/outside.pl", "test_corpus/./case.pl"] {
            let asset = CorpusAsset {
                id: relative_path.to_string(),
                layer: CorpusAssetLayer::TestCorpus,
                kind: CorpusAssetKind::PerlSource,
                relative_path: relative_path.to_string(),
                requirement: AssetRequirement::Required,
            };
            assert!(
                matches!(
                    topology.asset_path(&asset),
                    Err(CorpusTopologyError::InvalidRelativePath { .. })
                ),
                "path must fail closed: {relative_path}"
            );
        }

        let mismatch = CorpusAsset {
            id: "test_corpus/other.pl".to_string(),
            layer: CorpusAssetLayer::TestCorpus,
            kind: CorpusAssetKind::PerlSource,
            relative_path: "test_corpus/case.pl".to_string(),
            requirement: AssetRequirement::Required,
        };
        assert!(matches!(
            topology.asset_path(&mismatch),
            Err(CorpusTopologyError::AssetIdentityMismatch { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn literal_backslash_and_separator_paths_have_distinct_ids() {
        let root = tempfile::tempdir().expect("temporary directory");
        let literal = root.path().join("test_corpus/a\\b.pl");
        let nested = root.path().join("test_corpus/a/b.pl");
        write_fixture(&literal, "1;");
        write_fixture(&nested, "2;");

        let topology =
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()))
                .expect("build topology");
        let literal_asset = topology
            .assets
            .iter()
            .find(|asset| asset.id == "test_corpus/a\\b.pl")
            .expect("literal-backslash asset");
        let nested_asset = topology
            .assets
            .iter()
            .find(|asset| asset.id == "test_corpus/a/b.pl")
            .expect("nested asset");

        assert_ne!(literal_asset.id, nested_asset.id);
        assert_eq!(topology.asset_path(literal_asset).expect("resolve literal asset"), literal);
        assert_eq!(topology.asset_path(nested_asset).expect("resolve nested asset"), nested);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_asset_path_fails_closed() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let mut path = root.path().join("test_corpus");
        fs::create_dir_all(&path).expect("create corpus directory");
        path.push(OsString::from_vec(vec![b'b', 0xff, b'.', b'p', b'l']));
        fs::write(&path, "1;").expect("write non-UTF-8 fixture");

        assert!(matches!(
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf())),
            Err(CorpusTopologyError::NonUtf8Path { .. })
        ));
    }

    #[test]
    fn component_ids_use_forward_slashes() {
        let path = Path::new("test_corpus").join("nested").join("case.pl");
        assert_eq!(
            canonical_relative_path(&path).expect("canonical relative path"),
            "test_corpus/nested/case.pl"
        );
    }
}
