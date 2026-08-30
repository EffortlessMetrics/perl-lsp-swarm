#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one exact match, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def regex_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.DOTALL)
    if count != 1:
        raise SystemExit(f"{path}: expected one regex match, found {count}")
    path.write_text(updated, encoding="utf-8")


asset_path = r'''//! Portable, root-relative identity for one corpus asset.
//!
//! [`CorpusAssetPath`] owns durable member-path syntax. Portable parsing is
//! independent of the host path parser: `/` is the only serialized separator,
//! while a literal backslash remains component data. Host materialization is an
//! explicit fallible operation because not every portable identity can be
//! represented injectively on every operating system.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

/// A validated, portable path to a corpus asset relative to its corpus root.
///
/// The value contains one or more UTF-8 components. Its canonical serialized
/// representation joins those components with `/`, regardless of the current
/// host. The type proves only path shape; it does not prove that the path is a
/// selected member of a [`crate::CorpusTopology`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CorpusAssetPath {
    serialized: String,
    components: Vec<String>,
}

/// Failure to construct, parse, or materialize a portable corpus asset path.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusAssetPathError {
    /// No path component was supplied.
    Empty,
    /// The input contains an absolute root, drive, UNC, or verbatim prefix.
    AbsoluteOrPrefixed,
    /// A supplied component is empty.
    EmptyComponent {
        /// Zero-based component index.
        index: usize,
    },
    /// A supplied component is `.`.
    CurrentComponent {
        /// Zero-based component index.
        index: usize,
    },
    /// A supplied component is `..`.
    ParentComponent {
        /// Zero-based component index.
        index: usize,
    },
    /// A host path component cannot be represented as UTF-8.
    NonUtf8Component {
        /// Zero-based component index.
        index: usize,
    },
    /// A supplied component contains the portable `/` separator.
    SeparatorInComponent {
        /// Zero-based component index.
        index: usize,
    },
    /// A serialized path uses an alternate spelling such as a trailing or doubled `/`.
    NonCanonicalSerialization,
    /// The portable components cannot be represented injectively by this host path model.
    UnsupportedOnHost,
}

impl CorpusAssetPathError {
    /// Return the stable machine-readable reason token.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::AbsoluteOrPrefixed => "absolute_or_prefixed",
            Self::EmptyComponent { .. } => "empty_component",
            Self::CurrentComponent { .. } => "current_component",
            Self::ParentComponent { .. } => "parent_component",
            Self::NonUtf8Component { .. } => "non_utf8_component",
            Self::SeparatorInComponent { .. } => "separator_in_component",
            Self::NonCanonicalSerialization => "non_canonical_serialization",
            Self::UnsupportedOnHost => "unsupported_on_host",
        }
    }
}

impl fmt::Display for CorpusAssetPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid corpus asset path: {}", self.reason())
    }
}

impl std::error::Error for CorpusAssetPathError {}

impl CorpusAssetPath {
    /// Parse one canonical portable slash-delimited identity.
    ///
    /// This parser never delegates to [`Path`]. A backslash is ordinary
    /// component data unless the whole input begins with a UNC/verbatim prefix.
    pub fn parse(serialized: &str) -> Result<Self, CorpusAssetPathError> {
        if serialized.is_empty() {
            return Err(CorpusAssetPathError::Empty);
        }

        let bytes = serialized.as_bytes();
        let has_drive_prefix = bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':';
        if serialized.starts_with('/') || serialized.starts_with(r"\\") || has_drive_prefix {
            return Err(CorpusAssetPathError::AbsoluteOrPrefixed);
        }
        if serialized.ends_with('/') || serialized.contains("//") {
            return Err(CorpusAssetPathError::NonCanonicalSerialization);
        }

        let path = Self::try_from_components(serialized.split('/').map(str::to_owned))?;
        if path.serialized != serialized {
            return Err(CorpusAssetPathError::NonCanonicalSerialization);
        }
        Ok(path)
    }

    /// Construct a portable identity from actual host path components.
    ///
    /// Absolute, prefixed, traversing, empty, and non-UTF-8 host paths fail
    /// explicitly. Host separators are interpreted only here, while consuming
    /// the host's already-tokenized components.
    pub fn from_host_path(path: &Path) -> Result<Self, CorpusAssetPathError> {
        let mut components = Vec::new();
        for (index, component) in path.components().enumerate() {
            match component {
                Component::Normal(value) => {
                    let value = value
                        .to_str()
                        .ok_or(CorpusAssetPathError::NonUtf8Component { index })?;
                    components.push(value.to_owned());
                }
                Component::CurDir => {
                    return Err(CorpusAssetPathError::CurrentComponent { index });
                }
                Component::ParentDir => {
                    return Err(CorpusAssetPathError::ParentComponent { index });
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(CorpusAssetPathError::AbsoluteOrPrefixed);
                }
            }
        }
        Self::try_from_components(components)
    }

    /// Construct a portable identity from an ordered component sequence.
    pub fn try_from_components<I, S>(components: I) -> Result<Self, CorpusAssetPathError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let components = components.into_iter().map(Into::into).collect::<Vec<String>>();
        if components.is_empty() {
            return Err(CorpusAssetPathError::Empty);
        }

        for (index, component) in components.iter().enumerate() {
            if component.is_empty() {
                return Err(CorpusAssetPathError::EmptyComponent { index });
            }
            if component == "." {
                return Err(CorpusAssetPathError::CurrentComponent { index });
            }
            if component == ".." {
                return Err(CorpusAssetPathError::ParentComponent { index });
            }
            if component.contains('/') {
                return Err(CorpusAssetPathError::SeparatorInComponent { index });
            }
        }

        let serialized = components.join("/");
        Ok(Self { serialized, components })
    }

    /// Return the canonical portable slash-delimited identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Iterate over the ordered portable components.
    pub fn components(
        &self,
    ) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator + '_ {
        self.components.iter().map(String::as_str)
    }

    /// Materialize this identity as a relative host path without reinterpretation.
    ///
    /// The result is accepted only when converting it back through actual host
    /// components yields exactly the same portable identity.
    pub fn to_host_path(&self) -> Result<PathBuf, CorpusAssetPathError> {
        let mut path = PathBuf::new();
        for component in &self.components {
            path.push(component);
        }

        let round_trip =
            Self::from_host_path(&path).map_err(|_| CorpusAssetPathError::UnsupportedOnHost)?;
        if round_trip != *self {
            return Err(CorpusAssetPathError::UnsupportedOnHost);
        }
        Ok(path)
    }

    pub(crate) fn starts_with_components(&self, prefix: &[&str]) -> bool {
        self.components.len() >= prefix.len()
            && self
                .components()
                .zip(prefix.iter().copied())
                .all(|(component, expected)| component == expected)
    }
}

impl fmt::Display for CorpusAssetPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CorpusAssetPath {
    type Err = CorpusAssetPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for CorpusAssetPath {
    type Error = CorpusAssetPathError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&Path> for CorpusAssetPath {
    type Error = CorpusAssetPathError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        Self::from_host_path(value)
    }
}

impl Serialize for CorpusAssetPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CorpusAssetPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = String::deserialize(deserializer)?;
        Self::parse(&serialized).map_err(D::Error::custom)
    }
}
'''

asset_path_test = r'''use perl_corpus::{
    CorpusAsset, CorpusAssetPath, CorpusAssetPathError, CorpusPaths, CorpusTopology,
    CorpusTopologyError,
};
use std::fs;
use std::path::Path;

#[test]
fn fixed_portable_vector_round_trips_transparently_through_serde()
-> Result<(), Box<dyn std::error::Error>> {
    let path = CorpusAssetPath::parse("test_corpus/a/b.pl")?;

    assert_eq!(path.as_str(), "test_corpus/a/b.pl");
    assert_eq!(path.to_string(), "test_corpus/a/b.pl");
    assert_eq!(serde_json::to_string(&path)?, r#""test_corpus/a/b.pl""#);
    assert_eq!(serde_json::from_str::<CorpusAssetPath>(r#""test_corpus/a/b.pl""#)?, path);
    Ok(())
}

#[test]
fn portable_parser_treats_backslash_as_data_not_a_separator()
-> Result<(), Box<dyn std::error::Error>> {
    let literal = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;
    let nested = CorpusAssetPath::parse("test_corpus/a/b.pl")?;

    assert_ne!(literal, nested);
    assert_eq!(literal.components().collect::<Vec<_>>(), vec!["test_corpus", r"a\b.pl"]);
    assert_eq!(nested.components().collect::<Vec<_>>(), vec!["test_corpus", "a", "b.pl"]);
    Ok(())
}

#[test]
fn portable_parser_rejects_noncanonical_and_traversing_vectors_with_stable_reasons() {
    let vectors = [
        ("", "empty"),
        ("/tmp/case.pl", "absolute_or_prefixed"),
        ("C:/case.pl", "absolute_or_prefixed"),
        (r"\\server\share\case.pl", "absolute_or_prefixed"),
        (r"\\?\C:\case.pl", "absolute_or_prefixed"),
        ("test_corpus/", "non_canonical_serialization"),
        ("test_corpus//case.pl", "non_canonical_serialization"),
        ("test_corpus/./case.pl", "current_component"),
        ("test_corpus/../case.pl", "parent_component"),
    ];

    for (input, expected_reason) in vectors {
        let result = CorpusAssetPath::parse(input);
        assert_eq!(result.as_ref().map_err(CorpusAssetPathError::reason), Err(expected_reason));
    }
}

#[test]
fn component_constructor_rejects_empty_and_embedded_separator_components() {
    assert_eq!(
        CorpusAssetPath::try_from_components(["test_corpus", "", "case.pl"]),
        Err(CorpusAssetPathError::EmptyComponent { index: 1 })
    );
    assert_eq!(
        CorpusAssetPath::try_from_components(["test_corpus", "nested/case.pl"]),
        Err(CorpusAssetPathError::SeparatorInComponent { index: 1 })
    );
}

#[test]
fn host_components_and_portable_parsing_converge_when_representable()
-> Result<(), Box<dyn std::error::Error>> {
    let host = Path::new("test_corpus").join("nested").join("case.pl");
    let from_host = CorpusAssetPath::from_host_path(&host)?;
    let from_portable = CorpusAssetPath::parse("test_corpus/nested/case.pl")?;

    assert_eq!(from_host, from_portable);
    assert_eq!(from_portable.to_host_path()?, host);
    Ok(())
}

#[cfg(unix)]
#[test]
fn literal_backslash_component_materializes_injectively_on_unix()
-> Result<(), Box<dyn std::error::Error>> {
    let portable = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;
    let host = portable.to_host_path()?;

    assert_eq!(CorpusAssetPath::from_host_path(&host)?, portable);
    assert_eq!(host, Path::new("test_corpus").join(r"a\b.pl"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn literal_backslash_component_is_explicitly_unsupported_on_windows()
-> Result<(), Box<dyn std::error::Error>> {
    let portable = CorpusAssetPath::parse(r"test_corpus/a\b.pl")?;

    assert_eq!(portable.to_host_path(), Err(CorpusAssetPathError::UnsupportedOnHost));
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_host_component_fails_explicitly() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = Path::new("test_corpus").join(OsString::from_vec(vec![b'b', 0xff, b'.', b'p', b'l']));
    assert!(matches!(
        CorpusAssetPath::from_host_path(&path),
        Err(CorpusAssetPathError::NonUtf8Component { index: 1 })
    ));
}

#[test]
fn topology_membership_and_portable_shape_remain_distinct_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("test_corpus"))?;
    fs::create_dir_all(root.path().join("crates/perl-corpus/fuzz"))?;
    fs::write(root.path().join("test_corpus/included.pl"), "1;\n")?;

    let topology = CorpusTopology::from_paths(&CorpusPaths::from_root(root.path().to_path_buf()))?;
    let included = topology
        .assets
        .first()
        .ok_or_else(|| std::io::Error::other("expected one discovered corpus asset"))?;
    assert_eq!(included.portable_path()?.as_str(), "test_corpus/included.pl");
    assert_eq!(topology.member_path(included)?.as_str(), "test_corpus/included.pl");

    let outsider: CorpusAsset = serde_json::from_value(serde_json::json!({
        "id": "test_corpus/outsider.pl",
        "layer": "test_corpus",
        "kind": "perl_source",
        "relative_path": "test_corpus/outsider.pl",
        "requirement": "required"
    }))?;
    assert_eq!(outsider.portable_path()?.as_str(), "test_corpus/outsider.pl");
    assert_eq!(
        topology.member_path(&outsider),
        Err(CorpusTopologyError::AssetNotInTopology {
            id: "test_corpus/outsider.pl".to_owned()
        })
    );
    Ok(())
}
'''

Path("crates/perl-corpus/src/api/asset_path.rs").write_text(asset_path, encoding="utf-8")
Path("crates/perl-corpus/tests/corpus_asset_path.rs").write_text(asset_path_test, encoding="utf-8")

api = Path("crates/perl-corpus/src/api.rs")
replace_once(
    api,
    "pub(crate) mod root;\nmod topology;\n",
    "mod asset_path;\npub(crate) mod root;\nmod topology;\n",
)
replace_once(
    api,
    "pub use root::{CORPUS_ROOT_ENV, CorpusRoot, CorpusRootError, CorpusRootSource};\n",
    "pub use asset_path::{CorpusAssetPath, CorpusAssetPathError};\n"
    "pub use root::{CORPUS_ROOT_ENV, CorpusRoot, CorpusRootError, CorpusRootSource};\n",
)

topology = Path("crates/perl-corpus/src/api/topology.rs")
replace_once(
    topology,
    "use crate::files::{CorpusLayer, CorpusPaths};\n",
    "use super::asset_path::{CorpusAssetPath, CorpusAssetPathError};\n"
    "use crate::files::{CorpusLayer, CorpusPaths};\n",
)
replace_once(
    topology,
    "    /// Whether absence of the asset is a topology error.\n"
    "    pub requirement: AssetRequirement,\n"
    "}\n\n"
    "/// Failure to discover, validate, or resolve a corpus topology asset.\n",
    "    /// Whether absence of the asset is a topology error.\n"
    "    pub requirement: AssetRequirement,\n"
    "}\n\n"
    "impl CorpusAsset {\n"
    "    /// Validate and return this record's portable member-path identity.\n"
    "    ///\n"
    "    /// This proves the record's path syntax, duplicated v1 identity fields,\n"
    "    /// and declared layer prefix. It does not prove membership in a\n"
    "    /// particular [`CorpusTopology`].\n"
    "    pub fn portable_path(&self) -> Result<CorpusAssetPath, CorpusTopologyError> {\n"
    "        validate_asset_identity(self)\n"
    "    }\n"
    "}\n\n"
    "/// Failure to discover, validate, or resolve a corpus topology asset.\n",
)
replace_once(
    topology,
    '''    /// Resolve an exact topology member's checked-in path from its stable identity.
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
''',
    '''    /// Return one exact topology member's portable root-relative identity.
    ///
    /// A well-formed [`CorpusAssetPath`] alone is not membership evidence. This
    /// method validates the topology and requires exact record equality before
    /// returning the typed member identity used by later opening authorities.
    pub fn member_path(
        &self,
        asset: &CorpusAsset,
    ) -> Result<CorpusAssetPath, CorpusTopologyError> {
        self.validate()?;
        let path = validate_asset_identity(asset)?;

        let member = self
            .assets
            .binary_search_by(|candidate| candidate.id.cmp(&asset.id))
            .ok()
            .and_then(|index| self.assets.get(index));
        if member != Some(asset) {
            return Err(CorpusTopologyError::AssetNotInTopology { id: asset.id.clone() });
        }
        Ok(path)
    }

    /// Resolve an exact topology member's checked-in host path from its stable identity.
    pub fn asset_path(&self, asset: &CorpusAsset) -> Result<PathBuf, CorpusTopologyError> {
        let member_path = self.member_path(asset)?;
        let root = self.root.as_deref().ok_or(CorpusTopologyError::RootNotBound)?;
        let host_relative = member_path
            .to_host_path()
            .map_err(|error| invalid_asset_path(&asset.relative_path, error))?;
        let resolved = root.join(host_relative);
        validate_resolved_asset(root, asset, &member_path, &resolved)?;
        Ok(resolved)
    }
''',
)
replace_once(
    topology,
    '''    let relative_path = canonical_relative_path(relative)?;

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

    let required_prefix = layer_prefix(asset.layer);
    if !Path::new(&asset.relative_path).starts_with(required_prefix) {
        return Err(CorpusTopologyError::LayerPathMismatch {
            id: asset.id.clone(),
            layer: asset.layer,
            required_prefix,
        });
    }

    Ok(())
}

fn layer_prefix(layer: CorpusAssetLayer) -> &'static str {
    match layer {
        CorpusAssetLayer::TestCorpus => "test_corpus",
        CorpusAssetLayer::Fuzz => "crates/perl-corpus/fuzz",
    }
}
''',
    '''    let relative_path = asset_path_from_host(relative)?;
    let serialized = relative_path.as_str().to_owned();

    Ok(CorpusAsset {
        id: serialized.clone(),
        layer,
        kind,
        relative_path: serialized,
        requirement: AssetRequirement::Required,
    })
}

fn validate_asset_identity(asset: &CorpusAsset) -> Result<CorpusAssetPath, CorpusTopologyError> {
    let id = CorpusAssetPath::parse(&asset.id)
        .map_err(|error| invalid_asset_path(&asset.id, error))?;
    let relative_path = CorpusAssetPath::parse(&asset.relative_path)
        .map_err(|error| invalid_asset_path(&asset.relative_path, error))?;
    if id != relative_path {
        return Err(CorpusTopologyError::AssetIdentityMismatch {
            id: asset.id.clone(),
            relative_path: asset.relative_path.clone(),
        });
    }

    let required_prefix = layer_prefix(asset.layer);
    if !relative_path.starts_with_components(layer_prefix_components(asset.layer)) {
        return Err(CorpusTopologyError::LayerPathMismatch {
            id: asset.id.clone(),
            layer: asset.layer,
            required_prefix,
        });
    }

    Ok(relative_path)
}

fn invalid_asset_path(path: &str, error: CorpusAssetPathError) -> CorpusTopologyError {
    CorpusTopologyError::InvalidRelativePath {
        path: path.to_owned(),
        reason: error.reason(),
    }
}

fn asset_path_from_host(path: &Path) -> Result<CorpusAssetPath, CorpusTopologyError> {
    CorpusAssetPath::from_host_path(path).map_err(|error| match error {
        CorpusAssetPathError::NonUtf8Component { .. } => {
            CorpusTopologyError::NonUtf8Path { path: path.to_path_buf() }
        }
        other => CorpusTopologyError::InvalidRelativePath {
            path: path.to_string_lossy().into_owned(),
            reason: other.reason(),
        },
    })
}

fn layer_prefix(layer: CorpusAssetLayer) -> &'static str {
    match layer {
        CorpusAssetLayer::TestCorpus => "test_corpus",
        CorpusAssetLayer::Fuzz => "crates/perl-corpus/fuzz",
    }
}

fn layer_prefix_components(layer: CorpusAssetLayer) -> &'static [&'static str] {
    match layer {
        CorpusAssetLayer::TestCorpus => &["test_corpus"],
        CorpusAssetLayer::Fuzz => &["crates", "perl-corpus", "fuzz"],
    }
}
''',
)
replace_once(
    topology,
    '''fn validate_resolved_asset(
    root: &Path,
    asset: &CorpusAsset,
    resolved: &Path,
) -> Result<(), CorpusTopologyError> {
    validate_runtime_root_components(root)?;

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
''',
    '''fn validate_resolved_asset(
    root: &Path,
    asset: &CorpusAsset,
    member_path: &CorpusAssetPath,
    resolved: &Path,
) -> Result<(), CorpusTopologyError> {
    validate_runtime_root_components(root)?;

    let mut current = root.to_path_buf();
    let mut components = member_path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        let is_final = components.peek().is_none();
''',
)
regex_once(
    topology,
    r"\nfn canonical_relative_path\(path: &Path\) -> Result<String, CorpusTopologyError> \{.*?\n\}\n\nfn collect_layer_assets",
    "\nfn collect_layer_assets",
)
replace_once(
    topology,
    '''    fn component_ids_use_forward_slashes() {
        let path = Path::new("test_corpus").join("nested").join("case.pl");
        assert_eq!(
            canonical_relative_path(&path).expect("canonical relative path"),
            "test_corpus/nested/case.pl"
        );
    }
''',
    '''    fn component_ids_use_forward_slashes() {
        let path = Path::new("test_corpus").join("nested").join("case.pl");
        assert_eq!(
            CorpusAssetPath::from_host_path(&path)
                .expect("canonical relative path")
                .as_str(),
            "test_corpus/nested/case.pl"
        );
    }
''',
)

readme = Path("crates/perl-corpus/README.md")
replace_once(
    readme,
    "`require_repository_layout()` verifies only the required `test_corpus/` and `crates/perl-corpus/fuzz/` directory chains. It does not recurse, choose members, infer extensions, or replace `CorpusTopology`. Selected-member containment and opening belong to the later capability traversal seam.\n\n### Typed source loading\n",
    "`require_repository_layout()` verifies only the required `test_corpus/` and `crates/perl-corpus/fuzz/` directory chains. It does not recurse, choose members, infer extensions, or replace `CorpusTopology`. Selected-member containment and opening belong to the later capability traversal seam.\n\n"
    "### Portable member identity\n\n"
    "`CorpusAssetPath` is the reusable root-relative member identity. Its canonical serialized form uses `/` between ordered UTF-8 components on every host; a literal backslash is component data, not a portable separator. Portable parsing therefore never delegates to the host path parser. `to_host_path()` materializes components one at a time and fails with `unsupported_on_host` when the host would reinterpret them.\n\n"
    "`CorpusAsset::portable_path()` validates the topology-v1 `id`/`relative_path` pair and declared layer prefix. That proves path shape, not membership. `CorpusTopology::member_path()` additionally requires exact membership before returning the typed identity, and `asset_path()` materializes a host path only after that proof and runtime-root binding. Component-by-component no-follow opening and same-handle byte reads remain the separate #7693 authority.\n\n"
    "### Typed source loading\n",
)
replace_once(
    readme,
    "cargo test -p perl-corpus --test root_path_authority\n"
    "cargo test -p perl-corpus --test distribution_contract\n",
    "cargo test -p perl-corpus --test root_path_authority\n"
    "cargo test -p perl-corpus --test corpus_asset_path\n"
    "cargo test -p perl-corpus --test distribution_contract\n",
)

claude = Path("crates/perl-corpus/CLAUDE.md")
replace_once(
    claude,
    "cargo test -p perl-corpus --test root_path_authority\n"
    "cargo test -p perl-corpus --test distribution_contract\n",
    "cargo test -p perl-corpus --test root_path_authority\n"
    "cargo test -p perl-corpus --test corpus_asset_path\n"
    "cargo test -p perl-corpus --test distribution_contract\n",
)
replace_once(
    claude,
    "- The published package ships APIs and deliberately included crate assets. Repository\n"
    "  corpus data remains an external root.\n\n"
    "## Typed loading authority\n",
    "- The published package ships APIs and deliberately included crate assets. Repository\n"
    "  corpus data remains an external root.\n\n"
    "## Portable member identity\n\n"
    "- `CorpusAssetPath` alone proves one canonical root-relative component sequence. It\n"
    "  does not prove topology membership, existence, containment, opening, or bytes.\n"
    "- `/` is the sole portable serialization separator. A literal backslash is data;\n"
    "  durable parsing must never route through the host `Path` parser.\n"
    "- Host paths enter through actual host components. Host materialization pushes\n"
    "  validated components individually and must round-trip injectively or fail with\n"
    "  `unsupported_on_host`.\n"
    "- `CorpusAsset::portable_path()` validates the v1 duplicated identity fields and\n"
    "  layer prefix. `CorpusTopology::member_path()` adds exact topology membership.\n"
    "- Keep topology schema v1 serialized strings byte-compatible and deterministic. Do\n"
    "  not add a second component-array encoding.\n"
    "- #7693 must consume `CorpusTopology::member_path()` plus the retained `CorpusRoot`;\n"
    "  it must not reconstruct portable identity or reopen the root by pathname.\n\n"
    "## Typed loading authority\n",
)
