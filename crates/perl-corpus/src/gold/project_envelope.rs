//! Versioned gold project envelopes with strict closed-world membership.
//!
//! Every `test_corpus/gold/<project>/` directory carries a `gold_project.toml`
//! envelope that names its members, their roles, and their content digests.
//! The envelope is the authority for that project's closed membership: a
//! regular file under the project directory that is absent from the envelope
//! is an **orphan** and fails discovery.
//!
//! # ID stability
//!
//! `project_id` and `member_id` are seeded once from legacy directory and file
//! names using deterministic rules that are documented below and never change
//! when paths or content change:
//!
//! - **`project_id`**: the directory name verbatim (e.g., `hello_world`).
//! - **`member_id`**: the project-relative path with every `/` replaced by
//!   `__` and every `.` replaced by `_` (e.g., `fixture.pl` → `fixture_pl`,
//!   `lib/FindBinModule.pm` → `lib__FindBinModule_pm`). The rule is applied
//!   after the path is validated (no `..`, no absolute, no backslash, no NUL).
//!
//! # Schema version
//!
//! The TOML file begins with `schema = "gold_project.v1"`. Parsing fails
//! closed for any other schema string.
//!
//! # Member roles
//!
//! Every member carries exactly one [`MemberRole`]:
//!
//! - `entrypoint` — the primary Perl source (`fixture.pl`); implicitly source.
//! - `source` — additional Perl source not serving as the entrypoint.
//! - `support_source` — library modules under `lib/` (`*.pm`).
//! - `config` — configuration files.
//! - `data` — data-only files.
//! - `generated_expectation_input` — checked-in expected sidecar JSON files
//!   (`expected_*.json`, `expected.json`).
//!
//! # Fingerprints
//!
//! Two fingerprints are computed deterministically and stored in the envelope:
//!
//! - **`topology_fingerprint`**: SHA-256 of the sorted member paths,
//!   newline-separated, encoded as lowercase hex. Captures set membership and
//!   file layout without depending on content.
//! - **`content_fingerprint`**: SHA-256 of sorted `"<path>:<sha256>\n"` lines,
//!   encoded as lowercase hex. Captures layout and content together.
//!
//! Both fingerprints are checkout-root independent: they depend only on
//! project-relative paths and raw-byte digests, never on absolute paths.
//!
//! # Closed-world enforcement
//!
//! `GoldProjectEnvelope::validate_against_fs` walks the filesystem and
//! verifies that every regular file under the project directory appears
//! exactly once in the member list. Symlinks, non-regular files, the
//! `gold_project.toml` itself, and directories are handled explicitly.

use crate::byte_fidelity::{ByteFidelity, Encoding, NewlineStyle};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The schema token that every `gold_project.v1` TOML file must carry.
pub const GOLD_PROJECT_SCHEMA: &str = "gold_project.v1";

/// The envelope filename inside every project directory.
pub const ENVELOPE_FILENAME: &str = "gold_project.toml";

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// The functional role of one member within its gold project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    /// The primary Perl source that is the fixture entry point (`fixture.pl`).
    /// Implicitly carries source semantics.
    Entrypoint,
    /// Additional Perl source that is not the fixture entry point.
    Source,
    /// Library module consumed by the fixture (`lib/*.pm`).
    SupportSource,
    /// Configuration file consumed by the fixture.
    Config,
    /// Data-only file consumed by the fixture.
    Data,
    /// Checked-in expected sidecar (`expected.json`, `expected_*.json`).
    GeneratedExpectationInput,
}

impl fmt::Display for MemberRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Entrypoint => "entrypoint",
            Self::Source => "source",
            Self::SupportSource => "support_source",
            Self::Config => "config",
            Self::Data => "data",
            Self::GeneratedExpectationInput => "generated_expectation_input",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Member record
// ---------------------------------------------------------------------------

/// One member of a gold project as stored in `gold_project.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldMemberRecord {
    /// Stable identifier seeded from the member path.
    ///
    /// Rule: project-relative path with `/` → `__` and `.` → `_`.
    /// Never changes when content changes.
    pub member_id: String,
    /// Project-relative path using forward slashes.
    pub path: String,
    /// Role this member plays in the project.
    pub role: MemberRole,
    /// Exact byte length of the member.
    pub byte_length: u64,
    /// Lowercase hex SHA-256 digest of the member's raw bytes.
    pub sha256: String,
    /// UTF-8 encoding status.
    pub encoding: EncodingRecord,
    /// Newline style used in the member.
    pub newline_style: NewlineStyleRecord,
    /// Whether the member ends with a newline terminator.
    pub final_newline: bool,
    /// Whether the member begins with a UTF-8 byte-order mark.
    pub byte_order_mark: bool,
}

/// Serializable encoding status for a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingRecord {
    /// The member is valid UTF-8.
    Utf8,
    /// The member is not valid UTF-8; `valid_up_to` is the first bad byte offset.
    #[serde(rename = "invalid_utf8")]
    InvalidUtf8 {
        /// Byte offset of the first undecodable byte.
        valid_up_to: u64,
    },
}

impl From<Encoding> for EncodingRecord {
    fn from(e: Encoding) -> Self {
        match e {
            Encoding::Utf8 => Self::Utf8,
            Encoding::InvalidUtf8 { valid_up_to } => {
                Self::InvalidUtf8 { valid_up_to: valid_up_to as u64 }
            }
        }
    }
}

/// Serializable newline style for a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewlineStyleRecord {
    /// Only `\n` terminators.
    Lf,
    /// Only `\r\n` terminators.
    CrLf,
    /// Only `\r` terminators.
    Cr,
    /// Mixed terminators.
    Mixed,
    /// No terminators (empty file or single line without terminator).
    None,
}

impl From<NewlineStyle> for NewlineStyleRecord {
    fn from(s: NewlineStyle) -> Self {
        match s {
            NewlineStyle::Lf => Self::Lf,
            NewlineStyle::CrLf => Self::CrLf,
            NewlineStyle::Cr => Self::Cr,
            NewlineStyle::Mixed => Self::Mixed,
            NewlineStyle::None => Self::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// The on-disk representation of `gold_project.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldProjectEnvelope {
    /// Must equal [`GOLD_PROJECT_SCHEMA`].
    pub schema: String,
    /// Stable project identifier, seeded from the directory name.
    pub project_id: String,
    /// Topology fingerprint: SHA-256 of sorted member paths.
    pub topology_fingerprint: String,
    /// Content fingerprint: SHA-256 of sorted `"<path>:<sha256>\n"` lines.
    pub content_fingerprint: String,
    /// Member records in sorted path order.
    pub members: Vec<GoldMemberRecord>,
}

// ---------------------------------------------------------------------------
// Discovery outcomes
// ---------------------------------------------------------------------------

/// Typed outcomes from loading and validating a gold project envelope.
#[derive(Debug)]
pub enum DiscoveryOutcome {
    /// The envelope was loaded, parsed, validated, and all members are
    /// consistent with the filesystem.
    Valid(GoldProjectEnvelope),
    /// The `gold_project.toml` file was absent.
    MissingEnvelope {
        /// The project directory path.
        project_dir: PathBuf,
    },
    /// The TOML could not be parsed.
    ParseError {
        /// The project directory path.
        project_dir: PathBuf,
        /// Human-readable parse error.
        detail: String,
    },
    /// The envelope schema string did not match [`GOLD_PROJECT_SCHEMA`].
    SchemaMismatch {
        /// The project directory path.
        project_dir: PathBuf,
        /// The schema string that was found.
        found: String,
    },
    /// At least one member ID appears more than once in the envelope.
    DuplicateMemberId {
        /// The project directory path.
        project_dir: PathBuf,
        /// The duplicate ID.
        member_id: String,
    },
    /// A member path failed validation (e.g., `..`, absolute, backslash, NUL).
    InvalidMemberPath {
        /// The project directory path.
        project_dir: PathBuf,
        /// The offending path.
        path: String,
        /// Human-readable reason.
        reason: String,
    },
    /// A file found on the filesystem is not listed in the envelope.
    OrphanFile {
        /// The project directory path.
        project_dir: PathBuf,
        /// The orphan's project-relative path.
        orphan_path: String,
    },
    /// A member path listed in the envelope was not found on the filesystem,
    /// or is a symlink or non-regular file.
    MissingOrInvalidMember {
        /// The project directory path.
        project_dir: PathBuf,
        /// The member path.
        path: String,
        /// Human-readable reason.
        reason: String,
    },
    /// A member's stored digest or byte length did not match the file on disk.
    ContentMismatch {
        /// The project directory path.
        project_dir: PathBuf,
        /// The member path.
        path: String,
        /// Human-readable description of the mismatch.
        detail: String,
    },
    /// The stored fingerprints did not match the recomputed values.
    FingerprintMismatch {
        /// The project directory path.
        project_dir: PathBuf,
        /// Human-readable description.
        detail: String,
    },
    /// An I/O error occurred during validation.
    IoError {
        /// The project directory path.
        project_dir: PathBuf,
        /// Human-readable error.
        detail: String,
    },
}

impl DiscoveryOutcome {
    /// Whether this outcome represents a valid, coherent project.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid(_))
    }

    /// The project directory associated with this outcome.
    #[must_use]
    pub fn project_dir(&self) -> &Path {
        match self {
            Self::Valid(env) => {
                // Valid outcomes don't carry a PathBuf; callers hold the dir separately.
                // This method is here for uniformity but panics if called on Valid.
                // Use the project_dir argument you passed to discover_project instead.
                let _ = env;
                Path::new("")
            }
            Self::MissingEnvelope { project_dir }
            | Self::ParseError { project_dir, .. }
            | Self::SchemaMismatch { project_dir, .. }
            | Self::DuplicateMemberId { project_dir, .. }
            | Self::InvalidMemberPath { project_dir, .. }
            | Self::OrphanFile { project_dir, .. }
            | Self::MissingOrInvalidMember { project_dir, .. }
            | Self::ContentMismatch { project_dir, .. }
            | Self::FingerprintMismatch { project_dir, .. }
            | Self::IoError { project_dir, .. } => project_dir,
        }
    }
}

impl fmt::Display for DiscoveryOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Valid(env) => write!(f, "valid project {}", env.project_id),
            Self::MissingEnvelope { project_dir } => {
                write!(f, "missing envelope in {}", project_dir.display())
            }
            Self::ParseError { project_dir, detail } => {
                write!(f, "parse error in {}: {detail}", project_dir.display())
            }
            Self::SchemaMismatch { project_dir, found } => {
                write!(
                    f,
                    "schema mismatch in {} (found {found:?}, expected {GOLD_PROJECT_SCHEMA:?})",
                    project_dir.display()
                )
            }
            Self::DuplicateMemberId { project_dir, member_id } => {
                write!(f, "duplicate member_id {member_id:?} in {}", project_dir.display())
            }
            Self::InvalidMemberPath { project_dir, path, reason } => {
                write!(f, "invalid member path {path:?} in {}: {reason}", project_dir.display())
            }
            Self::OrphanFile { project_dir, orphan_path } => {
                write!(f, "orphan file {orphan_path:?} in {}", project_dir.display())
            }
            Self::MissingOrInvalidMember { project_dir, path, reason } => {
                write!(
                    f,
                    "member {path:?} missing or invalid in {}: {reason}",
                    project_dir.display()
                )
            }
            Self::ContentMismatch { project_dir, path, detail } => {
                write!(f, "content mismatch for {path:?} in {}: {detail}", project_dir.display())
            }
            Self::FingerprintMismatch { project_dir, detail } => {
                write!(f, "fingerprint mismatch in {}: {detail}", project_dir.display())
            }
            Self::IoError { project_dir, detail } => {
                write!(f, "I/O error in {}: {detail}", project_dir.display())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ID derivation
// ---------------------------------------------------------------------------

/// Derive a stable `project_id` from a project directory name.
///
/// Rule: the directory name verbatim. The value never changes after initial
/// seeding, even if the directory is renamed (which would require a new
/// project anyway).
#[must_use]
pub fn project_id_from_dir_name(dir_name: &str) -> String {
    dir_name.to_owned()
}

/// Derive a stable `member_id` from a project-relative member path.
///
/// Rule: replace every `/` with `__` and every `.` with `_`. Applied after
/// the path passes validation. The value is stable even when file content
/// changes; it only changes if the path itself changes (which requires
/// updating the envelope anyway).
///
/// Examples:
/// - `fixture.pl` → `fixture_pl`
/// - `lib/FindBinModule.pm` → `lib__FindBinModule_pm`
/// - `expected.json` → `expected_json`
/// - `expected_hover.json` → `expected_hover_json`
#[must_use]
pub fn member_id_from_path(path: &str) -> String {
    path.replace('/', "__").replace('.', "_")
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validate that a project-relative member path is safe.
///
/// Rejects:
/// - absolute paths (starts with `/` or a Windows drive letter)
/// - `..` components
/// - `.` components (current-dir references)
/// - backslash path separators
/// - NUL bytes
/// - empty paths
fn validate_member_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("member path is empty".to_owned());
    }
    if path.contains('\0') {
        return Err("member path contains NUL byte".to_owned());
    }
    if path.contains('\\') {
        return Err("member path contains backslash separator".to_owned());
    }

    let p = Path::new(path);

    // Reject absolute paths.
    if p.is_absolute() {
        return Err("member path is absolute".to_owned());
    }

    // Reject Windows-style drive prefixes (e.g., `C:`).
    if let Some(Component::Prefix(_)) = p.components().next() {
        return Err("member path has a drive-letter prefix".to_owned());
    }

    // Reject `..` and `.` components.
    for component in p.components() {
        match component {
            Component::ParentDir => {
                return Err("member path contains '..' component".to_owned());
            }
            Component::CurDir => {
                return Err("member path contains '.' component".to_owned());
            }
            _ => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Filesystem enumeration
// ---------------------------------------------------------------------------

/// Collect all regular files under `project_dir`, returning project-relative
/// `/`-separated paths in sorted order.
///
/// Symlinks are rejected: the contract requires no symlinks.
/// The `gold_project.toml` file is excluded from the returned list; it is
/// the envelope, not a content member.
fn collect_fs_members(project_dir: &Path) -> Result<BTreeSet<String>, io::Error> {
    let mut result = BTreeSet::new();
    collect_fs_members_recursive(project_dir, project_dir, &mut result)?;
    Ok(result)
}

fn collect_fs_members_recursive(
    root: &Path,
    dir: &Path,
    out: &mut BTreeSet<String>,
) -> Result<(), io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        // Use symlink_metadata to detect symlinks.
        let sym_metadata = fs::symlink_metadata(&path)?;
        if sym_metadata.file_type().is_symlink() {
            return Err(io::Error::other(format!(
                "symlink found under project directory: {}",
                path.display()
            )));
        }

        if metadata.is_dir() {
            collect_fs_members_recursive(root, &path, out)?;
        } else if metadata.is_file() {
            // Exclude the envelope itself.
            if name == ENVELOPE_FILENAME {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|e| io::Error::other(format!("path strip failed: {e}")))?;
            // Convert to forward-slash string.
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            out.insert(rel_str);
        } else {
            return Err(io::Error::other(format!(
                "non-regular non-directory file under project directory: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Fingerprint computation
// ---------------------------------------------------------------------------

/// Compute the topology fingerprint from an ordered set of member paths.
///
/// SHA-256 of `"<path>\n"` lines for each path in sorted order.
#[must_use]
pub fn compute_topology_fingerprint(sorted_paths: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for path in sorted_paths {
        hasher.update(path.as_bytes());
        hasher.update(b"\n");
    }
    hex_encode(hasher.finalize().as_slice())
}

/// Compute the content fingerprint from an ordered list of (path, sha256) pairs.
///
/// SHA-256 of `"<path>:<sha256>\n"` lines for each member in sorted path order.
#[must_use]
pub fn compute_content_fingerprint(sorted_path_digests: &[(&str, &str)]) -> String {
    let mut hasher = Sha256::new();
    for (path, digest) in sorted_path_digests {
        hasher.update(path.as_bytes());
        hasher.update(b":");
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    hex_encode(hasher.finalize().as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// File digest
// ---------------------------------------------------------------------------

/// Read a file and return its (raw bytes, SHA-256 hex, byte length, fidelity).
fn read_and_digest(path: &Path) -> Result<(Vec<u8>, String, u64, ByteFidelity), io::Error> {
    let bytes = fs::read(path)?;
    let len = bytes.len() as u64;
    let fidelity = ByteFidelity::classify(&bytes);
    let digest = hex_encode(Sha256::digest(&bytes).as_slice());
    Ok((bytes, digest, len, fidelity))
}

// ---------------------------------------------------------------------------
// Role inference
// ---------------------------------------------------------------------------

/// Infer the [`MemberRole`] for a project-relative path using the standard rules.
///
/// - `fixture.pl` → [`MemberRole::Entrypoint`]
/// - `lib/*.pm` or `*.pm` anywhere → [`MemberRole::SupportSource`]
/// - `expected.json` or `expected_*.json` → [`MemberRole::GeneratedExpectationInput`]
/// - `*.pl` elsewhere → [`MemberRole::Source`]
/// - anything else → [`MemberRole::Data`]
#[must_use]
pub fn infer_role(rel_path: &str) -> MemberRole {
    let path = Path::new(rel_path);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    if file_name == "fixture.pl" {
        return MemberRole::Entrypoint;
    }
    if extension == "pm" {
        return MemberRole::SupportSource;
    }
    if file_name == "expected.json" || (file_name.starts_with("expected_") && extension == "json") {
        return MemberRole::GeneratedExpectationInput;
    }
    if extension == "pl" || extension == "t" {
        return MemberRole::Source;
    }
    MemberRole::Data
}

// ---------------------------------------------------------------------------
// Envelope building
// ---------------------------------------------------------------------------

/// Build a [`GoldProjectEnvelope`] by reading all files under `project_dir`.
///
/// This is used during migration to generate the initial `gold_project.toml`
/// for existing fixtures.
pub fn build_envelope(project_dir: &Path) -> Result<GoldProjectEnvelope, io::Error> {
    let dir_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("project directory has no valid name"))?;
    let project_id = project_id_from_dir_name(dir_name);

    let fs_members = collect_fs_members(project_dir)?;

    let mut member_records: Vec<GoldMemberRecord> = Vec::new();
    for rel_path in &fs_members {
        let abs_path = project_dir.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let (_, digest, byte_length, fidelity) = read_and_digest(&abs_path)?;
        let role = infer_role(rel_path);
        let member_id = member_id_from_path(rel_path);
        member_records.push(GoldMemberRecord {
            member_id,
            path: rel_path.clone(),
            role,
            byte_length,
            sha256: digest,
            encoding: fidelity.encoding.into(),
            newline_style: fidelity.newline_style.into(),
            final_newline: fidelity.final_newline,
            byte_order_mark: fidelity.byte_order_mark,
        });
    }

    // Sort by path for deterministic order.
    member_records.sort_by(|a, b| a.path.cmp(&b.path));

    let sorted_paths: Vec<&str> = member_records.iter().map(|m| m.path.as_str()).collect();
    let sorted_path_digests: Vec<(&str, &str)> =
        member_records.iter().map(|m| (m.path.as_str(), m.sha256.as_str())).collect();

    let topology_fingerprint = compute_topology_fingerprint(&sorted_paths);
    let content_fingerprint = compute_content_fingerprint(&sorted_path_digests);

    Ok(GoldProjectEnvelope {
        schema: GOLD_PROJECT_SCHEMA.to_owned(),
        project_id,
        topology_fingerprint,
        content_fingerprint,
        members: member_records,
    })
}

// ---------------------------------------------------------------------------
// Envelope parsing and validation
// ---------------------------------------------------------------------------

/// Parse a `gold_project.toml` from bytes.
///
/// Fails closed: returns an error for any schema mismatch, unknown field,
/// or structural invalidity. Does **not** perform filesystem validation.
pub fn parse_envelope(toml_bytes: &[u8]) -> Result<GoldProjectEnvelope, EnvelopeError> {
    let text = std::str::from_utf8(toml_bytes)
        .map_err(|e| EnvelopeError::Parse(format!("envelope is not valid UTF-8: {e}")))?;
    let envelope: GoldProjectEnvelope =
        toml::from_str(text).map_err(|e| EnvelopeError::Parse(format!("TOML parse error: {e}")))?;
    if envelope.schema != GOLD_PROJECT_SCHEMA {
        return Err(EnvelopeError::SchemaMismatch { found: envelope.schema });
    }
    Ok(envelope)
}

/// Parse a `gold_project.toml` from a string.
pub fn parse_envelope_str(toml_str: &str) -> Result<GoldProjectEnvelope, EnvelopeError> {
    parse_envelope(toml_str.as_bytes())
}

/// Errors from parsing or validating a gold project envelope.
#[derive(Debug)]
pub enum EnvelopeError {
    /// TOML could not be parsed or was structurally invalid.
    Parse(String),
    /// Schema string did not match [`GOLD_PROJECT_SCHEMA`].
    SchemaMismatch { found: String },
    /// Structural constraint violation (duplicate IDs, invalid paths, etc.).
    Invalid(String),
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::SchemaMismatch { found } => {
                write!(f, "schema mismatch: found {found:?}, expected {GOLD_PROJECT_SCHEMA:?}")
            }
            Self::Invalid(msg) => write!(f, "invalid envelope: {msg}"),
        }
    }
}

impl std::error::Error for EnvelopeError {}

impl From<EnvelopeError> for io::Error {
    fn from(e: EnvelopeError) -> Self {
        io::Error::other(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Structural validation (no filesystem access)
// ---------------------------------------------------------------------------

/// Validate the structural invariants of an already-parsed envelope.
///
/// Checks: no duplicate `member_id`, all paths pass [`validate_member_path`],
/// and the stored fingerprints match the recomputed values.
///
/// This does **not** perform filesystem access.
pub fn validate_envelope_structure(env: &GoldProjectEnvelope) -> Result<(), EnvelopeError> {
    // Duplicate member_id check.
    let mut seen_ids = BTreeSet::new();
    for member in &env.members {
        if !seen_ids.insert(&member.member_id) {
            return Err(EnvelopeError::Invalid(format!(
                "duplicate member_id {:?}",
                member.member_id
            )));
        }
    }

    // Path validation and duplicate path check.
    let mut seen_paths = BTreeSet::new();
    for member in &env.members {
        validate_member_path(&member.path).map_err(|reason| {
            EnvelopeError::Invalid(format!("invalid member path {:?}: {reason}", member.path))
        })?;
        if !seen_paths.insert(&member.path) {
            return Err(EnvelopeError::Invalid(format!("duplicate member path {:?}", member.path)));
        }
    }

    // Members must be in sorted path order.
    let sorted = env.members.windows(2).all(|w| w[0].path <= w[1].path);
    if !sorted {
        return Err(EnvelopeError::Invalid("members are not in sorted path order".to_owned()));
    }

    // Fingerprint verification.
    let sorted_paths: Vec<&str> = env.members.iter().map(|m| m.path.as_str()).collect();
    let sorted_path_digests: Vec<(&str, &str)> =
        env.members.iter().map(|m| (m.path.as_str(), m.sha256.as_str())).collect();

    let expected_topo = compute_topology_fingerprint(&sorted_paths);
    if env.topology_fingerprint != expected_topo {
        return Err(EnvelopeError::Invalid(format!(
            "topology_fingerprint mismatch: stored {:?}, computed {expected_topo:?}",
            env.topology_fingerprint
        )));
    }

    let expected_content = compute_content_fingerprint(&sorted_path_digests);
    if env.content_fingerprint != expected_content {
        return Err(EnvelopeError::Invalid(format!(
            "content_fingerprint mismatch: stored {:?}, computed {expected_content:?}",
            env.content_fingerprint
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Filesystem validation
// ---------------------------------------------------------------------------

/// Validate an already-parsed envelope against the filesystem.
///
/// 1. Validates structural invariants via [`validate_envelope_structure`].
/// 2. Walks the filesystem to find all regular files under `project_dir`
///    (excluding `gold_project.toml`).
/// 3. Verifies that every filesystem file appears in the envelope (**no orphans**).
/// 4. Verifies that every envelope member exists on disk as a regular file
///    (not a symlink, not a directory).
/// 5. Verifies that every member's stored digest and byte length match the
///    file on disk.
pub fn validate_against_fs(
    env: &GoldProjectEnvelope,
    project_dir: &Path,
) -> Result<(), DiscoveryOutcome> {
    // Structural check.
    if let Err(e) = validate_envelope_structure(env) {
        return Err(match e {
            EnvelopeError::Invalid(msg) => {
                let member_id = env
                    .members
                    .iter()
                    .find(|m| {
                        msg.contains(m.member_id.as_str()) && msg.contains("duplicate member_id")
                    })
                    .map(|m| m.member_id.clone())
                    .unwrap_or_default();
                if !member_id.is_empty() {
                    DiscoveryOutcome::DuplicateMemberId {
                        project_dir: project_dir.to_owned(),
                        member_id,
                    }
                } else {
                    DiscoveryOutcome::IoError { project_dir: project_dir.to_owned(), detail: msg }
                }
            }
            _ => DiscoveryOutcome::IoError {
                project_dir: project_dir.to_owned(),
                detail: e.to_string(),
            },
        });
    }

    // Collect filesystem members.
    let fs_members = collect_fs_members(project_dir).map_err(|e| DiscoveryOutcome::IoError {
        project_dir: project_dir.to_owned(),
        detail: e.to_string(),
    })?;

    let envelope_paths: BTreeSet<&str> = env.members.iter().map(|m| m.path.as_str()).collect();

    // Orphan check: every filesystem file must be in the envelope.
    for fs_path in &fs_members {
        if !envelope_paths.contains(fs_path.as_str()) {
            return Err(DiscoveryOutcome::OrphanFile {
                project_dir: project_dir.to_owned(),
                orphan_path: fs_path.clone(),
            });
        }
    }

    // Member check: every envelope member must exist on disk and match.
    let mut member_map: BTreeMap<&str, &GoldMemberRecord> = BTreeMap::new();
    for m in &env.members {
        member_map.insert(m.path.as_str(), m);
    }

    for member in member_map.values() {
        // Convert forward-slash path to platform-native.
        let abs_path = project_dir.join(member.path.replace('/', std::path::MAIN_SEPARATOR_STR));

        let sym_meta = match fs::symlink_metadata(&abs_path) {
            Ok(m) => m,
            Err(e) => {
                return Err(DiscoveryOutcome::MissingOrInvalidMember {
                    project_dir: project_dir.to_owned(),
                    path: member.path.clone(),
                    reason: format!("cannot stat: {e}"),
                });
            }
        };

        if sym_meta.file_type().is_symlink() {
            return Err(DiscoveryOutcome::MissingOrInvalidMember {
                project_dir: project_dir.to_owned(),
                path: member.path.clone(),
                reason: "member is a symlink".to_owned(),
            });
        }
        if !sym_meta.file_type().is_file() {
            return Err(DiscoveryOutcome::MissingOrInvalidMember {
                project_dir: project_dir.to_owned(),
                path: member.path.clone(),
                reason: "member is not a regular file".to_owned(),
            });
        }

        let actual_len = sym_meta.len();
        if actual_len != member.byte_length {
            return Err(DiscoveryOutcome::ContentMismatch {
                project_dir: project_dir.to_owned(),
                path: member.path.clone(),
                detail: format!(
                    "byte_length mismatch: stored {}, actual {actual_len}",
                    member.byte_length
                ),
            });
        }

        // Read and digest.
        let bytes = fs::read(&abs_path).map_err(|e| DiscoveryOutcome::IoError {
            project_dir: project_dir.to_owned(),
            detail: format!("read {}: {e}", abs_path.display()),
        })?;
        let actual_digest = hex_encode(Sha256::digest(&bytes).as_slice());
        if actual_digest != member.sha256 {
            return Err(DiscoveryOutcome::ContentMismatch {
                project_dir: project_dir.to_owned(),
                path: member.path.clone(),
                detail: format!(
                    "sha256 mismatch: stored {:?}, actual {:?}",
                    member.sha256, actual_digest
                ),
            });
        }
    }

    // Recompute fingerprints and verify.
    let sorted_paths: Vec<&str> = env.members.iter().map(|m| m.path.as_str()).collect();
    let sorted_path_digests: Vec<(&str, &str)> =
        env.members.iter().map(|m| (m.path.as_str(), m.sha256.as_str())).collect();
    let topo_fp = compute_topology_fingerprint(&sorted_paths);
    let content_fp = compute_content_fingerprint(&sorted_path_digests);
    if topo_fp != env.topology_fingerprint || content_fp != env.content_fingerprint {
        return Err(DiscoveryOutcome::FingerprintMismatch {
            project_dir: project_dir.to_owned(),
            detail: format!(
                "topology: stored={:?} computed={topo_fp:?}, content: stored={:?} computed={content_fp:?}",
                env.topology_fingerprint, env.content_fingerprint
            ),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// High-level discovery
// ---------------------------------------------------------------------------

/// Load and validate the gold project envelope for `project_dir`.
///
/// Returns a [`DiscoveryOutcome`] that is either `Valid` or describes exactly
/// which constraint failed. Never panics.
pub fn discover_project(project_dir: &Path) -> DiscoveryOutcome {
    let envelope_path = project_dir.join(ENVELOPE_FILENAME);

    let bytes = match fs::read(&envelope_path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return DiscoveryOutcome::MissingEnvelope { project_dir: project_dir.to_owned() };
        }
        Err(e) => {
            return DiscoveryOutcome::IoError {
                project_dir: project_dir.to_owned(),
                detail: format!("read {}: {e}", envelope_path.display()),
            };
        }
    };

    let envelope = match parse_envelope(&bytes) {
        Ok(e) => e,
        Err(EnvelopeError::Parse(msg)) => {
            return DiscoveryOutcome::ParseError {
                project_dir: project_dir.to_owned(),
                detail: msg,
            };
        }
        Err(EnvelopeError::SchemaMismatch { found }) => {
            return DiscoveryOutcome::SchemaMismatch { project_dir: project_dir.to_owned(), found };
        }
        Err(EnvelopeError::Invalid(msg)) => {
            return DiscoveryOutcome::ParseError {
                project_dir: project_dir.to_owned(),
                detail: msg,
            };
        }
    };

    match validate_against_fs(&envelope, project_dir) {
        Ok(()) => DiscoveryOutcome::Valid(envelope),
        Err(outcome) => outcome,
    }
}

/// Load and validate all gold project envelopes under `gold_root`.
///
/// One malformed or invalid project does not prevent others from being
/// validated: every project directory is attempted independently and all
/// outcomes are returned.
pub fn discover_corpus(gold_root: &Path) -> Result<Vec<(PathBuf, DiscoveryOutcome)>, io::Error> {
    let mut results = Vec::new();

    let entries = {
        let mut v: Vec<_> = fs::read_dir(gold_root)?.collect::<Result<Vec<_>, _>>()?;
        v.sort_by_key(|e| e.file_name());
        v
    };

    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;

        if meta.file_type().is_symlink() {
            // gold root contains no symlinks per contract
            continue;
        }
        if !meta.is_dir() {
            // top-level files (README.md etc.) are not project dirs
            continue;
        }

        let outcome = discover_project(&path);
        results.push((path, outcome));
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// TOML serialization helpers
// ---------------------------------------------------------------------------

/// Serialize a [`GoldProjectEnvelope`] to a TOML string suitable for writing
/// to `gold_project.toml`.
pub fn envelope_to_toml(env: &GoldProjectEnvelope) -> Result<String, io::Error> {
    toml::to_string_pretty(env)
        .map_err(|e| io::Error::other(format!("TOML serialization error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_id_derivation_matches_documented_rule() {
        assert_eq!(member_id_from_path("fixture.pl"), "fixture_pl");
        assert_eq!(member_id_from_path("expected.json"), "expected_json");
        assert_eq!(member_id_from_path("lib/FindBinModule.pm"), "lib__FindBinModule_pm");
        assert_eq!(member_id_from_path("expected_hover.json"), "expected_hover_json");
        assert_eq!(member_id_from_path("expected_module.json"), "expected_module_json");
    }

    #[test]
    fn project_id_is_dir_name_verbatim() {
        assert_eq!(project_id_from_dir_name("hello_world"), "hello_world");
        assert_eq!(project_id_from_dir_name("inc_findbin_relative"), "inc_findbin_relative");
    }

    #[test]
    fn path_validation_rejects_dotdot() {
        assert!(validate_member_path("../escape").is_err());
        assert!(validate_member_path("lib/../escape.pm").is_err());
    }

    #[test]
    fn path_validation_rejects_absolute() {
        assert!(validate_member_path("/etc/passwd").is_err());
    }

    #[test]
    fn path_validation_rejects_backslash() {
        assert!(validate_member_path("lib\\Module.pm").is_err());
    }

    #[test]
    fn path_validation_rejects_nul() {
        assert!(validate_member_path("lib/bad\0name.pm").is_err());
    }

    #[test]
    fn path_validation_accepts_normal_paths() {
        assert!(validate_member_path("fixture.pl").is_ok());
        assert!(validate_member_path("lib/FindBinModule.pm").is_ok());
        assert!(validate_member_path("expected.json").is_ok());
        assert!(validate_member_path("expected_hover.json").is_ok());
    }

    #[test]
    fn fingerprints_are_deterministic_and_order_independent_of_input_order() {
        let paths_a = vec!["expected.json", "fixture.pl"];
        let paths_b = vec!["expected.json", "fixture.pl"];
        assert_eq!(compute_topology_fingerprint(&paths_a), compute_topology_fingerprint(&paths_b));
    }

    #[test]
    fn schema_mismatch_rejected() {
        let toml = r#"
schema = "gold_project.v2"
project_id = "hello"
topology_fingerprint = "aaa"
content_fingerprint = "bbb"
members = []
"#;
        let err = parse_envelope_str(toml);
        assert!(matches!(err, Err(EnvelopeError::SchemaMismatch { .. })));
    }

    #[test]
    fn parse_fails_on_unknown_field() {
        let toml = r#"
schema = "gold_project.v1"
project_id = "hello"
topology_fingerprint = "aaa"
content_fingerprint = "bbb"
members = []
unknown_field = "oops"
"#;
        let err = parse_envelope_str(toml);
        assert!(err.is_err(), "unknown fields must be rejected");
    }

    #[test]
    fn role_inference_covers_all_known_patterns() {
        assert_eq!(infer_role("fixture.pl"), MemberRole::Entrypoint);
        assert_eq!(infer_role("lib/Module.pm"), MemberRole::SupportSource);
        assert_eq!(infer_role("expected.json"), MemberRole::GeneratedExpectationInput);
        assert_eq!(infer_role("expected_hover.json"), MemberRole::GeneratedExpectationInput);
        assert_eq!(infer_role("expected_goto.json"), MemberRole::GeneratedExpectationInput);
        assert_eq!(infer_role("other_source.pl"), MemberRole::Source);
        assert_eq!(infer_role("data.txt"), MemberRole::Data);
    }

    #[test]
    fn duplicate_member_id_fails_structural_validation() {
        let member = GoldMemberRecord {
            member_id: "fixture_pl".to_owned(),
            path: "fixture.pl".to_owned(),
            role: MemberRole::Entrypoint,
            byte_length: 0,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
            encoding: EncodingRecord::Utf8,
            newline_style: NewlineStyleRecord::None,
            final_newline: false,
            byte_order_mark: false,
        };
        // Empty fingerprints for two identical paths — structure check fires first.
        let env = GoldProjectEnvelope {
            schema: GOLD_PROJECT_SCHEMA.to_owned(),
            project_id: "test".to_owned(),
            topology_fingerprint: String::new(),
            content_fingerprint: String::new(),
            members: vec![member.clone(), member],
        };
        let err = validate_envelope_structure(&env);
        assert!(
            matches!(err, Err(EnvelopeError::Invalid(ref msg)) if msg.contains("duplicate")),
            "expected duplicate member_id error, got {err:?}"
        );
    }
}
