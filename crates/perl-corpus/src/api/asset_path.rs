//! Portable, root-relative identity for one corpus asset.
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
        let has_drive_prefix =
            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
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
                    let value =
                        value.to_str().ok_or(CorpusAssetPathError::NonUtf8Component { index })?;
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
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator + '_ {
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
