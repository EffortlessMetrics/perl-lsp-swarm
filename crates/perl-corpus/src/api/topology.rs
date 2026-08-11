//! Versioned, deterministic identity for corpus assets.
//!
//! This is the first topology authority for the checked-in `test_corpus/` and
//! crate-local fuzz populations. Discovery is deliberately strict: selected
//! assets cannot disappear behind lossy names, filesystem errors, symlinks, or
//! non-regular file types.

use crate::files::{CorpusLayer, CorpusPaths};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

const TEST_EXTENSIONS: &[&str] = &["pl", "pm", "plx", "t", "psgi", "cgi"];

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
    /// The serialized topology schema is not supported by this implementation.
    UnsupportedSchemaVersion {
        /// Schema version found in the topology.
        found: u32,
        /// Schema version supported by this implementation.
        supported: u32,
    },
    /// Two discovered or deserialized assets resolve to the same stable identity.
    DuplicateAssetId {
        /// Duplicated asset ID.
        id: String,
    },
    /// Serialized assets are not in strictly ascending stable-ID order.
    AssetOrder {
        /// ID immediately preceding the out-of-order asset.
        previous: String,
        /// Out-of-order asset ID.
        current: String,
    },
    /// The requested asset is not an exact member of the topology.
    AssetNotInTopology {
        /// Requested asset ID.
        id: String,
    },
    /// A deserialized topology has not been bound to a runtime checkout root.
    RootNotBound,
    /// A required asset is absent from the bound checkout root.
    RequiredAssetMissing {
        /// Missing asset ID.
        id: String,
        /// Resolved checkout path.
        path: PathBuf,
    },
    /// A selected corpus entry is a symlink and cannot receive checkout-stable identity.
    SymlinkUnsupported {
        /// Rejected symlink path.
        path: PathBuf,
    },
    /// A selected corpus entry exists but is not a regular file.
    UnsupportedFileType {
        /// Rejected path.
        path: PathBuf,
    },
    /// Filesystem discovery or resolution failed.
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
            Self::UnsupportedSchemaVersion { found, supported } => {
                write!(
                    formatter,
                    "unsupported corpus topology schema version {found}; supported version is {supported}"
                )
            }
            Self::DuplicateAssetId { id } => {
                write!(formatter, "duplicate corpus asset ID: {id}")
            }
            Self::AssetOrder { previous, current } => {
                write!(
                    formatter,
                    "corpus asset IDs are not strictly ordered: {previous:?} before {current:?}"
                )
            }
            Self::AssetNotInTopology { id } => {
                write!(formatter, "corpus asset is not a member of this topology: {id}")
            }
            Self::RootNotBound => formatter.write_str("corpus topology has no bound runtime root"),
            Self::RequiredAssetMissing { id, path } => {
                write!(formatter, "required corpus asset {id} is missing at {}", path.display())
            }
            Self::SymlinkUnsupported { path } => {
                write!(formatter, "corpus asset symlink is unsupported: {}", path.display())
            }
            Self::UnsupportedFileType { path } => {
                write!(formatter, "corpus asset is not a regular file: {}", path.display())
            }
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
        let mut assets = collect_layer_assets(
            paths,
            &paths.test_corpus,
            CorpusAssetLayer::TestCorpus,
            classify_test_asset,
        )?;
        assets.extend(collect_layer_assets(
            paths,
            &paths.fuzz,
            CorpusAssetLayer::Fuzz,
            classify_fuzz_asset,
        )?);
        assets.sort_by(|left, right| left.id.cmp(&right.id));

        let topology = Self {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets,
            root: Some(paths.root.clone()),
        };
        topology.validate()?;
        Ok(topology)
    }

    /// Validate the serialized, checkout-independent topology contract.
    pub fn validate(&self) -> Result<(), CorpusTopologyError> {
        if self.schema_version != CORPUS_TOPOLOGY_SCHEMA_VERSION {
            return Err(CorpusTopologyError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            });
        }

        let mut previous: Option<&CorpusAsset> = None;
        for asset in &self.assets {
            validate_asset_identity(asset)?;
            if let Some(previous) = previous {
                if previous.id == asset.id {
                    return Err(CorpusTopologyError::DuplicateAssetId { id: asset.id.clone() });
                }
                if previous.id > asset.id {
                    return Err(CorpusTopologyError::AssetOrder {
                        previous: previous.id.clone(),
                        current: asset.id.clone(),
                    });
                }
            }
            previous = Some(asset);
        }

        Ok(())
    }

    /// Bind a runtime checkout root after loading and validating a serialized topology.
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Result<Self, CorpusTopologyError> {
        self.validate()?;
        self.root = Some(root.into());
        Ok(self)
    }

    /// Return the runtime checkout root used to resolve assets.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Resolve an exact topology member's checked-in path from its stable identity.
    pub fn asset_path(&self, asset: &CorpusAsset) -> Result<PathBuf, CorpusTopologyError> {
        self.validate()?;
        validate_asset_identity(asset)?;

        let member = self
            .assets
            .binary_search_by(|candidate| candidate.id.cmp(&asset.id))
            .ok()
            .and_then(|index| self.assets.get(index));
        if member != Some(asset) {
            return Err(CorpusTopologyError::AssetNotInTopology { id: asset.id.clone() });
        }

        let root = self.root.as_deref().ok_or(CorpusTopologyError::RootNotBound)?;
        let resolved = root.join(Path::new(&asset.relative_path));
        validate_resolved_asset(root, asset, &resolved)?;
        Ok(resolved)
    }
}

fn asset_from_path(
    paths: &CorpusPaths,
    path: &Path,
    layer: CorpusAssetLayer,
    kind: CorpusAssetKind,
) -> Result<CorpusAsset, CorpusTopologyError> {
    let relative = path.strip_prefix(&paths.root).map_err(|_| {
        CorpusTopologyError::PathOutsideRoot { path: path.to_path_buf(), root: paths.root.clone() }
    })?;
    let relative_path = canonical_relative_path(relative)?;

    Ok(CorpusAsset {
        id: relative_path.clone(),
        layer,
        kind,
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

fn validate_resolved_asset(
    root: &Path,
    asset: &CorpusAsset,
    resolved: &Path,
) -> Result<(), CorpusTopologyError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CorpusTopologyError::SymlinkUnsupported {
                path: root.to_path_buf(),
            });
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(CorpusTopologyError::UnsupportedFileType {
                path: root.to_path_buf(),
            });
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && asset.requirement == AssetRequirement::Optional =>
        {
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CorpusTopologyError::RequiredAssetMissing {
                id: asset.id.clone(),
                path: resolved.to_path_buf(),
            });
        }
        Err(error) => {
            return Err(CorpusTopologyError::Io {
                path: root.to_path_buf(),
                message: error.to_string(),
            });
        }
    }

    let mut current = root.to_path_buf();
    let mut components = Path::new(&asset.relative_path).components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(value) = component else {
            return Err(CorpusTopologyError::InvalidRelativePath {
                path: asset.relative_path.clone(),
                reason: "non_canonical",
            });
        };
        current.push(value);
        let is_final = components.peek().is_none();

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CorpusTopologyError::SymlinkUnsupported { path: current });
            }
            Ok(metadata) if is_final && metadata.is_file() => return Ok(()),
            Ok(metadata) if !is_final && metadata.is_dir() => {}
            Ok(_) => {
                return Err(CorpusTopologyError::UnsupportedFileType { path: current });
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && asset.requirement == AssetRequirement::Optional =>
            {
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(CorpusTopologyError::RequiredAssetMissing {
                    id: asset.id.clone(),
                    path: resolved.to_path_buf(),
                });
            }
            Err(error) => {
                return Err(CorpusTopologyError::Io {
                    path: current,
                    message: error.to_string(),
                });
            }
        }
    }

    Err(CorpusTopologyError::InvalidRelativePath {
        path: asset.relative_path.clone(),
        reason: "empty_path",
    })
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

fn collect_layer_assets(
    paths: &CorpusPaths,
    root: &Path,
    layer: CorpusAssetLayer,
    classify: fn(&Path) -> Option<CorpusAssetKind>,
) -> Result<Vec<CorpusAsset>, CorpusTopologyError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CorpusTopologyError::SymlinkUnsupported {
                path: root.to_path_buf(),
            });
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(CorpusTopologyError::UnsupportedFileType {
                path: root.to_path_buf(),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(CorpusTopologyError::Io {
                path: root.to_path_buf(),
                message: error.to_string(),
            });
        }
    }

    let mut assets = Vec::new();
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
            let file_name = entry.file_name();
            if is_ignored_name(&file_name) {
                continue;
            }

            let file_type = entry.file_type().map_err(|error| CorpusTopologyError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;
            let kind = classify(&path);

            if file_type.is_symlink() {
                return Err(CorpusTopologyError::SymlinkUnsupported { path });
            }
            if file_type.is_dir() {
                if kind.is_some() {
                    return Err(CorpusTopologyError::UnsupportedFileType { path });
                }
                stack.push(path);
            } else if file_type.is_file() {
                if let Some(kind) = kind {
                    assets.push(asset_from_path(paths, &path, layer, kind)?);
                }
            } else if kind.is_some() {
                return Err(CorpusTopologyError::UnsupportedFileType { path });
            }
        }
    }

    assets.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(assets)
}

fn is_ignored_name(name: &OsStr) -> bool {
    name.as_encoded_bytes()
        .first()
        .is_some_and(|byte| *byte == b'.' || *byte == b'_')
}

fn classify_test_asset(path: &Path) -> Option<CorpusAssetKind> {
    has_allowed_extension(path, TEST_EXTENSIONS).then_some(CorpusAssetKind::PerlSource)
}

fn classify_fuzz_asset(path: &Path) -> Option<CorpusAssetKind> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("pl") => {
            Some(CorpusAssetKind::PerlSource)
        }
        Some(extension) if extension.eq_ignore_ascii_case("txt") => {
            Some(CorpusAssetKind::TextFixture)
        }
        None if path
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"crash-")) =>
        {
            Some(CorpusAssetKind::PerlSource)
        }
        _ => None,
    }
}

fn has_allowed_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions.iter().any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(path: &Path, source: &str) {
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, source).expect("write fixture");
    }

    fn asset(relative_path: &str) -> CorpusAsset {
        CorpusAsset {
            id: relative_path.to_string(),
            layer: CorpusAssetLayer::TestCorpus,
            kind: CorpusAssetKind::PerlSource,
            relative_path: relative_path.to_string(),
            requirement: AssetRequirement::Required,
        }
    }

    fn topology_with(assets: Vec<CorpusAsset>) -> CorpusTopology {
        CorpusTopology {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets,
            root: None,
        }
    }

    #[test]
    fn topology_includes_supported_test_and_fuzz_assets() {
        let root = tempfile::tempdir().expect("temporary directory");
        let test_file = root.path().join("test_corpus/nested/case.pl");
        let test_module = root.path().join("test_corpus/nested/Case.pm");
        let fuzz_perl = root.path().join("crates/perl-corpus/fuzz/seed.pl");
        let fuzz_text = root.path().join("crates/perl-corpus/fuzz/heredoc_validation.txt");
        let fuzz_crash = root.path().join("crates/perl-corpus/fuzz/crash-deadbeef");
        let fuzz_unclassified = root.path().join("crates/perl-corpus/fuzz/notes");
        let fuzz_readme = root.path().join("crates/perl-corpus/fuzz/README.md");

        write_fixture(&test_file, "my $x = 1;");
        write_fixture(&test_module, "package Case; 1;");
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
                "test_corpus/nested/Case.pm",
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

        let rebound = loaded.with_root(first_root.path()).expect("bind topology root");
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

        for relative_path in ["../outside.pl", "/tmp/outside.pl", "test_corpus/./case.pl"] {
            let rejected = asset(relative_path);
            let topology = CorpusTopology {
                schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
                assets: vec![rejected.clone()],
                root: Some(root.path().to_path_buf()),
            };
            assert!(
                matches!(
                    topology.asset_path(&rejected),
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
        let topology = CorpusTopology {
            schema_version: CORPUS_TOPOLOGY_SCHEMA_VERSION,
            assets: vec![mismatch.clone()],
            root: Some(root.path().to_path_buf()),
        };
        assert!(matches!(
            topology.asset_path(&mismatch),
            Err(CorpusTopologyError::AssetIdentityMismatch { .. })
        ));
    }

    #[test]
    fn binding_rejects_unsupported_schema_versions() {
        for found in [0, CORPUS_TOPOLOGY_SCHEMA_VERSION + 1] {
            let mut topology = topology_with(Vec::new());
            topology.schema_version = found;
            assert_eq!(
                topology.with_root("."),
                Err(CorpusTopologyError::UnsupportedSchemaVersion {
                    found,
                    supported: CORPUS_TOPOLOGY_SCHEMA_VERSION,
                })
            );
        }
    }

    #[test]
    fn validation_rejects_duplicate_and_unsorted_asset_ids() {
        let duplicate = asset("test_corpus/a.pl");
        assert_eq!(
            topology_with(vec![duplicate.clone(), duplicate]).validate(),
            Err(CorpusTopologyError::DuplicateAssetId {
                id: "test_corpus/a.pl".to_string(),
            })
        );

        assert_eq!(
            topology_with(vec![asset("test_corpus/z.pl"), asset("test_corpus/a.pl")]).validate(),
            Err(CorpusTopologyError::AssetOrder {
                previous: "test_corpus/z.pl".to_string(),
                current: "test_corpus/a.pl".to_string(),
            })
        );
    }

    #[test]
    fn resolution_requires_exact_topology_membership() {
        let root = tempfile::tempdir().expect("temporary directory");
        let included = asset("test_corpus/included.pl");
        write_fixture(&root.path().join(&included.relative_path), "1;");
        let topology = topology_with(vec![included])
            .with_root(root.path())
            .expect("bind topology root");
        let outsider = asset("test_corpus/outsider.pl");

        assert_eq!(
            topology.asset_path(&outsider),
            Err(CorpusTopologyError::AssetNotInTopology { id: outsider.id })
        );
    }

    #[test]
    fn required_assets_must_exist_after_rebinding() {
        let source_root = tempfile::tempdir().expect("source temporary directory");
        let empty_root = tempfile::tempdir().expect("empty temporary directory");
        write_fixture(&source_root.path().join("test_corpus/case.pl"), "1;");
        let discovered =
            CorpusTopology::from_paths(&CorpusPaths::from_root(source_root.path().to_path_buf()))
                .expect("discover topology");
        let payload = serde_json::to_string(&discovered).expect("serialize topology");
        let rebound = serde_json::from_str::<CorpusTopology>(&payload)
            .expect("deserialize topology")
            .with_root(empty_root.path())
            .expect("bind topology root");
        let required = rebound.assets.first().expect("required asset");
        let expected_path = empty_root.path().join(&required.relative_path);

        assert_eq!(
            rebound.asset_path(required),
            Err(CorpusTopologyError::RequiredAssetMissing {
                id: required.id.clone(),
                path: expected_path,
            })
        );
    }

    #[test]
    fn optional_assets_may_be_absent_after_rebinding() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut optional = asset("test_corpus/optional.pl");
        optional.requirement = AssetRequirement::Optional;
        let topology = topology_with(vec![optional.clone()])
            .with_root(root.path())
            .expect("bind topology root");

        assert_eq!(
            topology.asset_path(&optional),
            Ok(root.path().join("test_corpus/optional.pl"))
        );
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
    fn non_utf8_selected_asset_path_fails_closed() {
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

    #[cfg(unix)]
    #[test]
    fn non_utf8_fuzz_metadata_does_not_enter_identity_validation() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let mut path = root.path().join("crates/perl-corpus/fuzz");
        fs::create_dir_all(&path).expect("create fuzz directory");
        path.push(OsString::from_vec(vec![b'n', 0xff, b'.', b'm', b'd']));
        fs::write(&path, "metadata").expect("write non-UTF-8 metadata");

        let topology =
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()))
                .expect("ignore non-asset metadata");
        assert!(topology.assets.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entries_fail_closed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary directory");
        let target = root.path().join("target.pl");
        let link = root.path().join("test_corpus/linked.pl");
        write_fixture(&target, "1;");
        fs::create_dir_all(link.parent().expect("link parent")).expect("create link parent");
        symlink(&target, &link).expect("create source symlink");

        assert_eq!(
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf())),
            Err(CorpusTopologyError::SymlinkUnsupported { path: link })
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_non_regular_entries_fail_closed() {
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().expect("temporary directory");
        let socket = root.path().join("test_corpus/socket.pl");
        fs::create_dir_all(socket.parent().expect("socket parent"))
            .expect("create socket parent");
        let _listener = UnixListener::bind(&socket).expect("bind Unix socket");

        assert_eq!(
            CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf())),
            Err(CorpusTopologyError::UnsupportedFileType { path: socket })
        );
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
