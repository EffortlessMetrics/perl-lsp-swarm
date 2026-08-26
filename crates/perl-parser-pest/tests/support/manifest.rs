use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::FixtureError;
use super::digest::sha256_digest;
use super::path::{require_under_fixtures, resolve_crate_relative};

/// Supported catalog schema identifier.
pub const MANIFEST_SCHEMA: &str = "perl-parser-pest.fixture_manifest.v1";

/// Package-relative default catalog path.
pub const DEFAULT_MANIFEST_RELATIVE: &str = "tests/fixtures/manifest.toml";

/// Stable classification for a seed row. This is not a parse verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
    /// Source is intended as currently valid Perl in the train set.
    Valid,
    /// Source is intended as malformed input.
    Malformed,
    /// Source is a candidate the experimental parser may not support.
    UnsupportedCandidate,
}

/// How later isolation/extraction subjects should execute the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    /// Embedded workspace crate tests.
    Embedded,
    /// Unpacked published package tests.
    Packaged,
    /// Extracted-repository tests.
    Extracted,
    /// External comparison subject.
    External,
}

/// Whether a current parse return may be treated as final acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    /// Observation only; not a support claim.
    ProvisionalObservation,
    /// Final expected outcome. Requires `expected_outcome_owner`.
    FinalAcceptance,
}

/// Declared newline convention for a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NewlineVariant {
    /// Unix LF.
    Lf,
    /// Windows CRLF.
    Crlf,
    /// Bare CR.
    Cr,
    /// Mixed conventions in one file.
    Mixed,
}

/// Origin of the resolved fixture bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// Package-relative file under `tests/fixtures/`.
    File {
        /// Path relative to the package root.
        relative: PathBuf,
    },
    /// Inline UTF-8 source recorded in the manifest.
    Inline,
}

/// One resolved catalog row with exact bytes and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFixture {
    /// Stable fixture id.
    pub id: String,
    /// Optional shared source identity used to detect byte drift across files.
    pub identity: Option<String>,
    /// Train family.
    pub family: String,
    /// Valid / malformed / unsupported-candidate.
    pub classification: Classification,
    /// Subjects that must be able to run this row.
    pub execution_modes: Vec<ExecutionMode>,
    /// Issue that owns the current observation.
    pub observation_owner: String,
    /// Issue that will own the final expected outcome.
    pub expected_outcome_owner: Option<String>,
    /// Declared newline variant, when relevant.
    pub newline: Option<NewlineVariant>,
    /// Declared encoding, when relevant.
    pub encoding: Option<String>,
    /// Provisional vs final-acceptance.
    pub disposition: Disposition,
    /// Free-form limitation token.
    pub notes: Option<String>,
    /// File or inline origin.
    pub source_kind: SourceKind,
    /// Exact fixture bytes.
    pub bytes: Vec<u8>,
    /// Digest of `bytes`.
    pub source_digest: String,
}

/// Loaded catalog in manifest insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedManifest {
    /// Schema identifier.
    pub schema: String,
    /// Rows in insertion order.
    pub fixtures: Vec<ResolvedFixture>,
}

/// Selection over the catalog. An empty match is an error, not success.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Exact fixture id.
    pub id: Option<String>,
    /// Exact family.
    pub family: Option<String>,
}

impl Selection {
    /// Every row in insertion order.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// One fixture id.
    #[must_use]
    pub fn id(id: impl Into<String>) -> Self {
        Self { id: Some(id.into()), family: None }
    }

    /// Every row of one family, preserving insertion order.
    #[must_use]
    pub fn family(family: impl Into<String>) -> Self {
        Self { id: None, family: Some(family.into()) }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    schema: String,
    fixtures: Vec<ManifestRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRow {
    id: Option<String>,
    identity: Option<String>,
    family: Option<String>,
    source: Option<PathBuf>,
    inline_source: Option<String>,
    classification: Classification,
    execution_modes: Vec<ExecutionMode>,
    observation_owner: String,
    expected_outcome_owner: Option<String>,
    newline: Option<NewlineVariant>,
    encoding: Option<String>,
    disposition: Disposition,
    notes: Option<String>,
    source_digest: Option<String>,
}

/// Package root used when the caller does not supply one.
#[must_use]
pub fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Load `tests/fixtures/manifest.toml` from `package_root`.
pub fn load_manifest(package_root: &Path) -> Result<LoadedManifest, FixtureError> {
    load_manifest_at(package_root, Path::new(DEFAULT_MANIFEST_RELATIVE))
}

/// Load a crate-local manifest relative to `package_root`.
pub fn load_manifest_at(
    package_root: &Path,
    manifest_relative: &Path,
) -> Result<LoadedManifest, FixtureError> {
    let manifest_path = resolve_crate_relative(package_root, manifest_relative)?;
    require_under_fixtures(manifest_relative)?;
    reject_symlink_components("manifest", package_root, manifest_relative)?;
    let bytes = read_regular_file("manifest", manifest_relative, &manifest_path)?;
    let text = String::from_utf8(bytes).map_err(|error| FixtureError::Unreadable {
        id: "manifest".to_string(),
        path: manifest_relative.display().to_string(),
        detail: error.to_string(),
    })?;
    let parsed: ManifestFile =
        toml::from_str(&text).map_err(|error| FixtureError::InvalidToml {
            path: manifest_relative.display().to_string(),
            detail: error.to_string(),
        })?;
    if parsed.schema != MANIFEST_SCHEMA {
        return Err(FixtureError::InvalidSchema(parsed.schema));
    }
    if parsed.fixtures.is_empty() {
        return Err(FixtureError::EmptyManifest);
    }

    let mut fixtures = Vec::with_capacity(parsed.fixtures.len());
    let mut seen_ids = HashSet::new();
    for row in parsed.fixtures {
        let resolved = resolve_row(package_root, row)?;
        if !seen_ids.insert(resolved.id.clone()) {
            return Err(FixtureError::DuplicateId(resolved.id));
        }
        fixtures.push(resolved);
    }
    reject_shared_identity_with_distinct_bytes(&fixtures)?;
    Ok(LoadedManifest { schema: parsed.schema, fixtures })
}

impl LoadedManifest {
    /// Select rows in insertion order. An empty match fails closed.
    pub fn select(&self, selection: &Selection) -> Result<Vec<&ResolvedFixture>, FixtureError> {
        self.select_with_mode(selection, None)
    }

    /// Select rows, optionally requiring an execution mode.
    pub fn select_with_mode(
        &self,
        selection: &Selection,
        mode: Option<ExecutionMode>,
    ) -> Result<Vec<&ResolvedFixture>, FixtureError> {
        let selected: Vec<&ResolvedFixture> = self
            .fixtures
            .iter()
            .filter(|fixture| {
                selection.id.as_ref().is_none_or(|id| fixture.id == *id)
                    && selection.family.as_ref().is_none_or(|family| fixture.family == *family)
                    && mode.is_none_or(|required| fixture.execution_modes.contains(&required))
            })
            .collect();
        if selected.is_empty() {
            return Err(FixtureError::EmptySelection {
                id: selection.id.clone(),
                family: selection.family.clone(),
            });
        }
        Ok(selected)
    }
}

fn resolve_row(package_root: &Path, row: ManifestRow) -> Result<ResolvedFixture, FixtureError> {
    let id = row.id.filter(|value| !value.is_empty()).ok_or(FixtureError::MissingId)?;
    let family = row
        .family
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FixtureError::MissingFamily { id: id.clone() })?;
    if row.execution_modes.is_empty() {
        return Err(FixtureError::MissingExecutionModes { id: id.clone() });
    }
    if row.disposition == Disposition::FinalAcceptance
        && row.expected_outcome_owner.as_ref().is_none_or(|owner| owner.is_empty())
    {
        return Err(FixtureError::FinalAcceptanceWithoutOwner { id: id.clone() });
    }

    let (source_kind, bytes) = match (row.source, row.inline_source) {
        (Some(relative), None) => {
            let absolute = resolve_crate_relative(package_root, &relative)?;
            require_under_fixtures(&relative)?;
            reject_symlink_components(&id, package_root, &relative)?;
            let bytes = read_regular_file(&id, &relative, &absolute)?;
            (SourceKind::File { relative }, bytes)
        }
        (None, Some(inline_source)) => (SourceKind::Inline, inline_source.into_bytes()),
        _ => return Err(FixtureError::AmbiguousSource { id }),
    };
    let source_digest = sha256_digest(&bytes);
    if let Some(declared) = row.source_digest.as_ref()
        && declared != &source_digest
    {
        return Err(FixtureError::DigestMismatch {
            id: id.clone(),
            declared: declared.clone(),
            actual: source_digest,
        });
    }

    Ok(ResolvedFixture {
        id,
        identity: row.identity,
        family,
        classification: row.classification,
        execution_modes: row.execution_modes,
        observation_owner: row.observation_owner,
        expected_outcome_owner: row.expected_outcome_owner,
        newline: row.newline,
        encoding: row.encoding,
        disposition: row.disposition,
        notes: row.notes,
        source_kind,
        bytes,
        source_digest,
    })
}

fn reject_symlink_components(
    id: &str,
    package_root: &Path,
    relative: &Path,
) -> Result<(), FixtureError> {
    let mut current = package_root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                current.push(name);
                let metadata = fs::symlink_metadata(&current).map_err(|error| {
                    if error.kind() == ErrorKind::NotFound {
                        FixtureError::MissingSource {
                            id: id.to_string(),
                            path: relative.display().to_string(),
                        }
                    } else {
                        FixtureError::Unreadable {
                            id: id.to_string(),
                            path: relative.display().to_string(),
                            detail: error.to_string(),
                        }
                    }
                })?;
                if metadata.file_type().is_symlink() {
                    return Err(FixtureError::SymlinkSource {
                        id: id.to_string(),
                        path: relative.display().to_string(),
                    });
                }
            }
            _ => return Err(FixtureError::PathEscape(relative.display().to_string())),
        }
    }
    Ok(())
}

fn read_regular_file(id: &str, relative: &Path, absolute: &Path) -> Result<Vec<u8>, FixtureError> {
    let metadata = fs::symlink_metadata(absolute).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            FixtureError::MissingSource { id: id.to_string(), path: relative.display().to_string() }
        } else {
            FixtureError::Unreadable {
                id: id.to_string(),
                path: relative.display().to_string(),
                detail: error.to_string(),
            }
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(FixtureError::SymlinkSource {
            id: id.to_string(),
            path: relative.display().to_string(),
        });
    }
    if !metadata.is_file() {
        return Err(FixtureError::Unreadable {
            id: id.to_string(),
            path: relative.display().to_string(),
            detail: "not a regular file".to_string(),
        });
    }
    fs::read(absolute).map_err(|error| FixtureError::Unreadable {
        id: id.to_string(),
        path: relative.display().to_string(),
        detail: error.to_string(),
    })
}

fn reject_shared_identity_with_distinct_bytes(
    fixtures: &[ResolvedFixture],
) -> Result<(), FixtureError> {
    let mut by_identity: HashMap<&str, &ResolvedFixture> = HashMap::new();
    for fixture in fixtures {
        let Some(identity) = fixture.identity.as_deref() else {
            continue;
        };
        if identity.is_empty() {
            continue;
        }
        if let Some(previous) = by_identity.insert(identity, fixture)
            && previous.bytes != fixture.bytes
        {
            return Err(FixtureError::IdentityByteMismatch {
                identity: identity.to_string(),
                left: source_identity(previous),
                right: source_identity(fixture),
            });
        }
    }
    Ok(())
}

fn source_identity(fixture: &ResolvedFixture) -> String {
    match &fixture.source_kind {
        SourceKind::File { relative } => relative.display().to_string(),
        SourceKind::Inline => format!("inline:{}", fixture.id),
    }
}
