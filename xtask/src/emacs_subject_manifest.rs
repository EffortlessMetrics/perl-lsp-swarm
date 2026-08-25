//! Checked immutable Emacs client-subject manifest, resolver, and identity
//! cache (#11744, SUBJ_CORE).
//!
//! This module owns the subject-manifest mechanics the Emacs train's
//! subject lane consumes: the checked manifest at
//! `.ci/editor-clients/emacs-subjects.v1.json` declares one immutable row
//! per exact client subject, the resolver validates every declared digest
//! against a bounded input location *before* anything is loadable, and the
//! cache is keyed by the complete subject identity — never a version string
//! alone.
//!
//! Two invariants dominate the design:
//!
//! - *Manifest identity is intended input, never runtime proof.* The
//!   resolver output and cache records carry no observation of what a host
//!   actually used; that stays proven only by the actual host-run receipt's
//!   runtime attestation (`emacs_host_run`).
//! - *Exact identity is non-substitutable.* A visible version string is a
//!   naming hint; the binding facts are the declared source digest, the
//!   exact Emacs release tag/commit, the executable digest observed at
//!   resolution, and the resolved library form.
//!
//! Claim boundary (#11744): subject identity machinery only. No journey,
//! capability-profile, project/root, support, or public-artifact claim is
//! earned here. External Eglot and lsp-mode subject rows arrive with
//! #11745/#11746 and extend this manifest without changing resolver
//! semantics.

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::editor_client_compat::ClientSourceState;
use crate::emacs_host_run::EmacsClientSubject;

/// Schema identity of the checked subject manifest.
pub const MANIFEST_SCHEMA_VERSION: &str = "emacs_subject_manifest.v1";

/// Repository-relative location of the checked manifest.
pub const MANIFEST_RELATIVE_PATH: &str = ".ci/editor-clients/emacs-subjects.v1.json";

/// Schema identity of one immutable cache entry record.
pub const CACHE_SCHEMA_VERSION: &str = "emacs_subject_input_cache.v1";

/// The single file an immutable cache entry directory may contain.
pub const CACHE_ENTRY_FILE: &str = "subject-input.json";

/// Path-segment markers of package/native-comp installation layouts. A
/// client file reached through one of these segments is ambient package
/// state, not the bundled library of the exact Emacs build; run-time
/// ambient isolation (hermetic HOME, package-user-dir, native-comp env) is
/// the landed runner's job (#7778/#8734), this is the input-location seam.
const AMBIENT_PACKAGE_LAYOUT_MARKERS: [&str; 4] = ["elpa", "site-lisp", "eln-cache", "native-lisp"];

/// Refs that name a moving target rather than exact bytes.
const FLOATING_REFS: [&str; 5] = ["main", "master", "head", "trunk", "latest"];

/// The boundary text every cache record carries verbatim.
const INTENDED_INPUT_BOUNDARY: &str = "manifest identity is intended input, never runtime \
                                       proof; the client actually used at runtime is proven \
                                       only by the actual host-run receipt";

// ---------------------------------------------------------------------------
// Manifest schema
// ---------------------------------------------------------------------------

/// Client family of a subject row. Mirrors the runner's client kinds; the
/// bundled family is bound by this revision, the external families arrive
/// with #11745/#11746 rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectClientKind {
    BundledEglot,
    ExternalEglot,
    LspMode,
}

impl SubjectClientKind {
    /// The landed runner's client kind for this family.
    pub fn runner_kind(self) -> crate::emacs_host_run::emacs_host_runner::EmacsClientKind {
        use crate::emacs_host_run::emacs_host_runner::EmacsClientKind;
        match self {
            Self::BundledEglot => EmacsClientKind::BundledEglot,
            Self::ExternalEglot => EmacsClientKind::ExternalEglot,
            Self::LspMode => EmacsClientKind::LspMode,
        }
    }

    fn is_bundled(self) -> bool {
        matches!(self, Self::BundledEglot)
    }
}

/// How a row's client material is acquired. Bundled generations resolve
/// inside the exact Emacs installation root; external packages require an
/// explicit exact input (their rows and this resolver extension arrive with
/// #11745/#11746).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MaterializationMethod {
    InstallationRootResolution,
    ExplicitInput,
}

/// Implementation-time upstream audit backing a row's declared digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestAudit {
    /// Official GNU release tarball the audited file was extracted from.
    pub gnu_tarball_url: String,
    /// sha256 of that tarball, so the audit source is itself pinned.
    pub gnu_tarball_sha256: String,
    /// `;; Version:` header observed in the audited file. Must equal the
    /// row's version hint: the hint is the audited header, never an
    /// independent claim.
    pub observed_client_version_header: String,
}

/// One immutable subject row. New client releases and different host
/// builds are new rows, never silent edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectRow {
    /// Stable subject id, identical to the runner registry id.
    pub subject_id: String,
    pub client_kind: SubjectClientKind,
    pub source_state: ClientSourceState,
    /// Exact Emacs release tag (`emacs-30.1`) or 40-hex commit pin. Never a
    /// floating ref or mutable alias.
    pub emacs_release_tag: String,
    /// Token the probed `emacs --version` line must contain.
    pub emacs_version_token: String,
    /// Audited `;; Version:` header of the client file. Naming hint only:
    /// identity binds digests, not version strings.
    pub client_version_hint: String,
    /// Upstream-relative location of the client source inside the Emacs
    /// release tree.
    pub client_source_relative_path: String,
    /// Declared sha256 (of the decompressed client source) every
    /// materialization must validate before load.
    pub client_source_sha256: String,
    pub materialization: MaterializationMethod,
    /// File names (in deterministic preference order) one exact build can
    /// ship for the client library.
    pub client_library_forms: Vec<String>,
    pub digest_audit: DigestAudit,
}

/// The checked manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectManifest {
    pub schema_version: String,
    pub subjects: Vec<SubjectRow>,
}

impl SubjectManifest {
    /// Load and validate the checked manifest under a repository root.
    /// A missing or malformed manifest is a fail-closed instrument error;
    /// a content violation embeds the typed rejection in the error.
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(MANIFEST_RELATIVE_PATH);
        let bytes = fs::read(&path).with_context(|| {
            format!("checked subject manifest {} is missing or unreadable", path.display())
        })?;
        let manifest: Self = serde_json::from_slice(&bytes).with_context(|| {
            format!("checked subject manifest {} is not valid JSON", path.display())
        })?;
        manifest.validate().map_err(|rejection| {
            anyhow::anyhow!("invalid subject manifest {}: {rejection}", path.display())
        })?;
        Ok(manifest)
    }

    /// Structural and identity-rule validation. Every violation is one
    /// typed reason.
    pub fn validate(&self) -> Result<(), SubjectRejection> {
        let invalid = |reason: String| SubjectRejection::InvalidManifest { reason };
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(invalid(format!(
                "schema_version must be {MANIFEST_SCHEMA_VERSION}, found {}",
                self.schema_version
            )));
        }
        if self.subjects.is_empty() {
            return Err(invalid("the subject manifest must declare at least one row".to_string()));
        }
        let mut seen_ids = std::collections::BTreeSet::new();
        let mut seen_identities = std::collections::BTreeSet::new();
        for row in &self.subjects {
            let violation = if !is_subject_id_token(&row.subject_id) {
                Some(format!("subject_id {:?} is not a stable subject id token", row.subject_id))
            } else if !seen_ids.insert(row.subject_id.clone()) {
                Some(format!("duplicate subject_id {}", row.subject_id))
            } else if !seen_identities.insert((row.client_kind, row.emacs_release_tag.clone())) {
                Some(format!(
                    "duplicate subject identity {:?} for release tag {}",
                    row.client_kind, row.emacs_release_tag
                ))
            } else {
                validate_row(row).err()
            };
            if let Some(reason) = violation {
                return Err(invalid(reason));
            }
        }
        Ok(())
    }

    /// The row for an exact subject id.
    pub fn row_for(&self, subject_id: &str) -> Result<&SubjectRow> {
        self.subjects.iter().find(|row| row.subject_id == subject_id).with_context(|| {
            format!(
                "unknown subject {subject_id}: known subjects are {}",
                self.subjects
                    .iter()
                    .map(|row| row.subject_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }

    /// Ids of every declared subject, in manifest order.
    pub fn subject_ids(&self) -> Vec<String> {
        self.subjects.iter().map(|row| row.subject_id.clone()).collect()
    }
}

fn validate_row(row: &SubjectRow) -> Result<(), String> {
    if row.client_kind.is_bundled() != (row.source_state == ClientSourceState::Bundled) {
        return Err(format!(
            "subject {}: bundled client kinds must use bundled source state and external \
             kinds must not",
            row.subject_id
        ));
    }
    if row.client_kind.is_bundled()
        != (row.materialization == MaterializationMethod::InstallationRootResolution)
    {
        return Err(format!(
            "subject {}: bundled rows resolve inside the installation root, external rows \
             require an explicit input",
            row.subject_id
        ));
    }
    if is_floating_ref(&row.emacs_release_tag) || !is_exact_ref(&row.emacs_release_tag) {
        return Err(format!(
            "subject {}: emacs_release_tag {:?} is a floating or mutable ref; only exact \
             emacs-x.y release tags or 40-hex commit pins are accepted",
            row.subject_id, row.emacs_release_tag
        ));
    }
    if let Some(version) = release_tag_version(&row.emacs_release_tag)
        && row.emacs_version_token != version
    {
        return Err(format!(
            "subject {}: emacs_version_token {} disagrees with the release tag {}",
            row.subject_id, row.emacs_version_token, row.emacs_release_tag
        ));
    }
    if !is_safe_identity_token(&row.emacs_version_token) {
        return Err(format!(
            "subject {}: emacs_version_token {:?} is not a safe identity token",
            row.subject_id, row.emacs_version_token
        ));
    }
    if !is_safe_identity_token(&row.client_version_hint) {
        return Err(format!(
            "subject {}: client_version_hint {:?} is not a safe identity token",
            row.subject_id, row.client_version_hint
        ));
    }
    if !is_sha256_digest(&row.client_source_sha256) {
        return Err(format!(
            "subject {}: client_source_sha256 must be sha256:<64 lowercase hex>",
            row.subject_id
        ));
    }
    if !is_relative_upstream_path(&row.client_source_relative_path) {
        return Err(format!(
            "subject {}: client_source_relative_path {:?} must be a relative forward-slash \
             path without parent traversal",
            row.subject_id, row.client_source_relative_path
        ));
    }
    if row.client_library_forms.is_empty()
        || row.client_library_forms.iter().any(|form| !is_plain_file_name(form))
    {
        return Err(format!(
            "subject {}: client_library_forms must be non-empty plain file names",
            row.subject_id
        ));
    }
    if !is_sha256_digest(&row.digest_audit.gnu_tarball_sha256)
        || !row.digest_audit.gnu_tarball_url.starts_with("https://")
    {
        return Err(format!(
            "subject {}: digest_audit must pin an https GNU tarball and its sha256",
            row.subject_id
        ));
    }
    if row.digest_audit.observed_client_version_header != row.client_version_hint {
        return Err(format!(
            "subject {}: client_version_hint {} does not match the audited version header {}",
            row.subject_id,
            row.client_version_hint,
            row.digest_audit.observed_client_version_header
        ));
    }
    Ok(())
}

fn is_floating_ref(tag: &str) -> bool {
    FLOATING_REFS.iter().any(|floating| tag.eq_ignore_ascii_case(floating))
}

fn is_exact_ref(tag: &str) -> bool {
    release_tag_version(tag).is_some() || is_commit_pin(tag)
}

fn is_commit_pin(tag: &str) -> bool {
    tag.len() == 40 && tag.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The `x.y` of an `emacs-x.y` release tag, when the tag is one.
fn release_tag_version(tag: &str) -> Option<&str> {
    let rest = tag.strip_prefix("emacs-")?;
    let mut dots = 0;
    let well_formed = rest.chars().all(|character| {
        if character == '.' {
            dots += 1;
            true
        } else {
            character.is_ascii_digit()
        }
    });
    if well_formed && dots == 1 && !rest.starts_with('.') && !rest.ends_with('.') {
        Some(rest)
    } else {
        None
    }
}

fn is_subject_id_token(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'_'
                || byte == b'-'
                || byte == b'.'
        })
        && !id.starts_with('.')
}

fn is_safe_identity_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b';' && byte != b'"' && byte != b'\\')
        && !token.contains('/')
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_relative_upstream_path(path: &str) -> bool {
    !path.starts_with('/')
        && !path.contains('\\')
        && !path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && is_relative_upstream_path(name) && !name.contains('/')
}

// ---------------------------------------------------------------------------
// Typed dispositions
// ---------------------------------------------------------------------------

/// Typed reasons the resolver refuses to produce a subject input. A refusal
/// is never a pass and never a fallback load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectRejection {
    /// The requested id is not a checked subject.
    UnknownSubject { requested: String, known_subjects: Vec<String> },
    /// The declared material is missing or cannot be validated in the
    /// bounded input location.
    UnavailableSubject { subject_id: String, reason: String },
    /// The material does not bind this subject's declared identity.
    IdentityMismatch { subject_id: String, reason: String },
    /// The probed host does not satisfy the row's compatibility pin.
    IncompatibleSubject { subject_id: String, reason: String },
    /// Ambient ELPA/package/native-comp state cannot satisfy the subject.
    AmbientStateRejected { subject_id: String, reason: String },
    /// The cache holds this subject under a different complete identity.
    StaleCacheEntry { subject_id: String, stale_cache_key: String, reason: String },
    /// The manifest violates a schema or identity rule.
    InvalidManifest { reason: String },
}

impl fmt::Display for SubjectRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSubject { requested, known_subjects } => write!(
                formatter,
                "unknown subject {requested}: known subjects are {}",
                known_subjects.join(", ")
            ),
            Self::UnavailableSubject { subject_id, reason } => {
                write!(formatter, "subject {subject_id} is unavailable: {reason}")
            }
            Self::IdentityMismatch { subject_id, reason } => {
                write!(formatter, "subject {subject_id} identity mismatch: {reason}")
            }
            Self::IncompatibleSubject { subject_id, reason } => {
                write!(formatter, "subject {subject_id} is incompatible: {reason}")
            }
            Self::AmbientStateRejected { subject_id, reason } => {
                write!(formatter, "subject {subject_id} rejects ambient state: {reason}")
            }
            Self::StaleCacheEntry { subject_id, stale_cache_key, reason } => write!(
                formatter,
                "subject {subject_id} has a stale cache entry {stale_cache_key}: {reason}"
            ),
            Self::InvalidManifest { reason } => {
                write!(formatter, "invalid subject manifest: {reason}")
            }
        }
    }
}

/// The resolver's fail-closed failure surface: either a typed rejection or
/// an instrument failure (I/O, digest instrument, corrupt cache bytes). An
/// instrument failure is never a pass and never a skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveFailure {
    Rejected(SubjectRejection),
    Instrument(String),
}

impl fmt::Display for ResolveFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(rejection) => write!(formatter, "{rejection}"),
            Self::Instrument(message) => {
                write!(formatter, "subject resolution instrument failure: {message}")
            }
        }
    }
}

impl std::error::Error for ResolveFailure {}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Bounded, immutable input locations for one resolution.
pub struct ResolveRequest<'a> {
    /// Absolute path of the exact Emacs executable.
    pub emacs_executable: &'a Path,
    /// Explicit exact client file. For bundled subjects it must live inside
    /// the exact Emacs installation; omitting it resolves the bundled
    /// library inside the installation.
    pub client_source: Option<&'a Path>,
    /// Bounded immutable cache location. One location serves one complete
    /// identity per subject; a changed identity requires a fresh location.
    pub cache_root: &'a Path,
    /// Probed `emacs --version` first line, when the caller probed it.
    pub probed_emacs_version: Option<&'a str>,
}

/// The complete subject identity a cache key derives from. Version strings
/// appear only alongside every digest-and-tag fact that actually binds the
/// identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteSubjectIdentity {
    pub cache_schema: String,
    pub manifest_schema_version: String,
    pub subject_id: String,
    pub client_kind: SubjectClientKind,
    pub source_state: ClientSourceState,
    pub emacs_release_tag: String,
    pub emacs_version_token: String,
    pub client_version_hint: String,
    pub declared_client_source_sha256: String,
    pub emacs_build_sha256: String,
    pub resolved_library_form: String,
    pub resolved_client_sha256: String,
}

/// One immutable cache entry record. Carries intended input only: there is
/// deliberately no field that could claim what a runtime actually used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectInputRecord {
    pub cache_schema: String,
    pub cache_key: String,
    pub identity: CompleteSubjectIdentity,
    pub emacs_executable: String,
    pub client_source: String,
    pub emacs_version_token_verified: bool,
    pub intended_input_boundary: String,
}

/// A resolved subject: the exact pinned identity, the runner-facing client
/// subject, and the immutable cache entry backing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSubject {
    pub subject_id: String,
    /// Runner-facing client identity built from the row and the validated
    /// digests (`emacs_host_runner::ClientSubject`).
    pub client: crate::emacs_host_run::emacs_host_runner::ClientSubject,
    /// The canonical exact Emacs executable the identity was bound to.
    pub emacs_executable: PathBuf,
    /// The exact resolved client file.
    pub client_source: PathBuf,
    /// Digest of the exact Emacs executable, bound at resolution.
    pub emacs_build_sha256: String,
    pub cache_key: String,
    pub cache_entry: PathBuf,
    pub reused_cache: bool,
}

impl ResolvedSubject {
    /// The landed runner's host-run inputs for this resolved subject.
    /// Bundled subjects never carry a package identity.
    pub fn host_run_inputs(
        &self,
        candidate_executable: &Path,
        out_root: &Path,
        timeout_ms: u64,
    ) -> crate::emacs_host_run::EmacsHostRunInputs {
        crate::emacs_host_run::EmacsHostRunInputs {
            emacs_executable: self.emacs_executable.clone(),
            candidate_executable: candidate_executable.to_path_buf(),
            client_source: self.client_source.clone(),
            client_package: None,
            out_root: out_root.to_path_buf(),
            timeout_ms,
        }
    }
}

/// Resolve one checked subject against bounded immutable locations.
///
/// Every declared digest is validated before anything is treated as
/// loadable; ambient package state, floating pairing, and stale cache
/// identities are typed rejections; instrument failures fail closed.
pub fn resolve(
    manifest: &SubjectManifest,
    subject_id: &str,
    request: &ResolveRequest<'_>,
) -> Result<ResolvedSubject, ResolveFailure> {
    if let Err(rejection) = manifest.validate() {
        return Err(ResolveFailure::Rejected(rejection));
    }
    let row = match manifest.row_for(subject_id) {
        Ok(row) => row,
        Err(_) => {
            return Err(ResolveFailure::Rejected(SubjectRejection::UnknownSubject {
                requested: subject_id.to_string(),
                known_subjects: manifest.subject_ids(),
            }));
        }
    };

    // The runner registry must be able to dispatch this row's mechanics;
    // otherwise the manifest and registry have drifted.
    if EmacsClientSubject::from_id(subject_id).is_err() {
        return Err(ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
            subject_id: subject_id.to_string(),
            reason: "the runner registry cannot dispatch this subject id; manifest and \
                     registry have drifted"
                .to_string(),
        }));
    }

    if row.materialization != MaterializationMethod::InstallationRootResolution {
        return Err(ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
            subject_id: subject_id.to_string(),
            reason: "explicit-input materialization for external subjects arrives with \
                     #11745/#11746"
                .to_string(),
        }));
    }

    let executable = request.emacs_executable;
    if !executable.is_absolute() || !executable.is_file() {
        return Err(ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
            subject_id: subject_id.to_string(),
            reason: format!(
                "the exact Emacs executable is not a present absolute file: {}",
                executable.display()
            ),
        }));
    }
    let canonical_executable = fs::canonicalize(executable).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "canonicalizing the exact Emacs executable {}: {error}",
            executable.display()
        ))
    })?;
    let installation_root =
        canonical_executable.parent().and_then(Path::parent).map(Path::to_path_buf).ok_or_else(
            || {
                ResolveFailure::Instrument(
                    "the exact Emacs executable has no installation root".to_string(),
                )
            },
        )?;

    // Resolve the client file inside the bounded input location.
    let client_source = match request.client_source {
        Some(explicit) => {
            if !explicit.is_absolute() || !explicit.is_file() {
                return Err(ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
                    subject_id: subject_id.to_string(),
                    reason: format!(
                        "the explicit client source is not a present absolute file: {}",
                        explicit.display()
                    ),
                }));
            }
            let canonical = fs::canonicalize(explicit).map_err(|error| {
                ResolveFailure::Instrument(format!(
                    "canonicalizing the explicit client source {}: {error}",
                    explicit.display()
                ))
            })?;
            if canonical.strip_prefix(&installation_root).is_err() {
                return Err(ResolveFailure::Rejected(SubjectRejection::AmbientStateRejected {
                    subject_id: subject_id.to_string(),
                    reason: format!(
                        "the explicit client source {} is outside the exact Emacs \
                         installation {}; an ambient user package cannot satisfy a bundled \
                         subject",
                        canonical.display(),
                        installation_root.display()
                    ),
                }));
            }
            canonical
        }
        None => crate::emacs_host_run::resolve_bundled_client_source(&canonical_executable)
            .map_err(|error| classify_installation_resolution_error(subject_id, &error))?,
    };
    if let Some(marker) = ambient_layout_marker(&client_source, &installation_root) {
        return Err(ResolveFailure::Rejected(SubjectRejection::AmbientStateRejected {
            subject_id: subject_id.to_string(),
            reason: format!(
                "the resolved client file {} is reached through the {} package layout; \
                 ambient package state cannot satisfy a bundled subject",
                client_source.display(),
                marker
            ),
        }));
    }

    // Validate every declared digest before the file is loadable.
    let form = client_source
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_default();
    if !row.client_library_forms.contains(&form) {
        return Err(ResolveFailure::Rejected(SubjectRejection::IdentityMismatch {
            subject_id: subject_id.to_string(),
            reason: format!(
                "the resolved client form {form:?} is not a declared library form of this \
                 subject (declared: {})",
                row.client_library_forms.join(", ")
            ),
        }));
    }
    let resolved_digest = match form.as_str() {
        "eglot.el" => file_digest(&client_source)?,
        "eglot.el.gz" => decompressed_digest(&client_source)?,
        "eglot.elc" => {
            return Err(ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
                subject_id: subject_id.to_string(),
                reason: "a compiled-only installation cannot validate the declared upstream \
                         source digest; supply the exact release eglot.el or eglot.el.gz"
                    .to_string(),
            }));
        }
        other => {
            return Err(ResolveFailure::Rejected(SubjectRejection::IdentityMismatch {
                subject_id: subject_id.to_string(),
                reason: format!("unexpected client library form {other:?}"),
            }));
        }
    };
    if resolved_digest != row.client_source_sha256 {
        return Err(ResolveFailure::Rejected(SubjectRejection::IdentityMismatch {
            subject_id: subject_id.to_string(),
            reason: format!(
                "the resolved bundled client digest {resolved_digest} does not match the \
                 pinned subject digest {} ({});{}",
                row.client_source_sha256,
                row.emacs_release_tag,
                generation_hint(manifest, subject_id, &resolved_digest)
            ),
        }));
    }

    let emacs_build_sha256 = file_digest(&canonical_executable)?;
    if let Some(version_line) = request.probed_emacs_version
        && !version_line.contains(&row.emacs_version_token)
    {
        return Err(ResolveFailure::Rejected(SubjectRejection::IncompatibleSubject {
            subject_id: subject_id.to_string(),
            reason: format!(
                "the probed host version line {version_line:?} does not contain the pinned \
                 token {}",
                row.emacs_version_token
            ),
        }));
    }

    let identity = CompleteSubjectIdentity {
        cache_schema: CACHE_SCHEMA_VERSION.to_string(),
        manifest_schema_version: manifest.schema_version.clone(),
        subject_id: row.subject_id.clone(),
        client_kind: row.client_kind,
        source_state: row.source_state,
        emacs_release_tag: row.emacs_release_tag.clone(),
        emacs_version_token: row.emacs_version_token.clone(),
        client_version_hint: row.client_version_hint.clone(),
        declared_client_source_sha256: row.client_source_sha256.clone(),
        emacs_build_sha256: emacs_build_sha256.clone(),
        resolved_library_form: form,
        resolved_client_sha256: resolved_digest.clone(),
    };
    let cache_key = identity_cache_key(&identity)?;
    let cache_entry = request.cache_root.join(&cache_key);

    // A cache location is immutable per identity: an existing entry is
    // reused only under the exact recomputed identity, and any other entry
    // for this subject is a typed stale rejection.
    let known_entries = scan_cache_entries(request.cache_root)?;
    if let Some(stale) = known_entries
        .iter()
        .find(|entry| entry.identity.subject_id == subject_id && entry.cache_key != cache_key)
    {
        return Err(ResolveFailure::Rejected(SubjectRejection::StaleCacheEntry {
            subject_id: subject_id.to_string(),
            stale_cache_key: stale.cache_key.clone(),
            reason: format!(
                "{}; the immutable cache never mutates in place, so a changed subject \
                 identity requires a fresh bounded cache location",
                identity_difference(&stale.identity, &identity)
            ),
        }));
    }
    let record_path = cache_entry.join(CACHE_ENTRY_FILE);
    if record_path.is_file() {
        let record_bytes = fs::read(&record_path).map_err(|error| {
            ResolveFailure::Instrument(format!("reading {}: {error}", record_path.display()))
        })?;
        let record: SubjectInputRecord =
            serde_json::from_slice(&record_bytes).map_err(|error| {
                ResolveFailure::Instrument(format!(
                    "the cache entry record {} is corrupt and refuses to parse: {error}",
                    record_path.display()
                ))
            })?;
        if record.cache_schema != CACHE_SCHEMA_VERSION
            || record.cache_key != cache_key
            || record.identity != identity
        {
            return Err(ResolveFailure::Rejected(SubjectRejection::StaleCacheEntry {
                subject_id: subject_id.to_string(),
                stale_cache_key: cache_key,
                reason: "the entry record does not match the recomputed complete identity"
                    .to_string(),
            }));
        }
        let unexpected = unexpected_entry_files(&cache_entry)?;
        if !unexpected.is_empty() {
            return Err(ResolveFailure::Rejected(SubjectRejection::AmbientStateRejected {
                subject_id: subject_id.to_string(),
                reason: format!(
                    "the immutable cache entry contains unexpected files: {}",
                    unexpected.join(", ")
                ),
            }));
        }
        return Ok(ResolvedSubject {
            subject_id: subject_id.to_string(),
            client: runner_client_subject(row, resolved_digest),
            emacs_executable: canonical_executable,
            client_source,
            emacs_build_sha256,
            cache_key,
            cache_entry,
            reused_cache: true,
        });
    }

    let record = SubjectInputRecord {
        cache_schema: CACHE_SCHEMA_VERSION.to_string(),
        cache_key: cache_key.clone(),
        identity,
        emacs_executable: canonical_executable.to_string_lossy().into_owned(),
        client_source: client_source.to_string_lossy().into_owned(),
        emacs_version_token_verified: request.probed_emacs_version.is_some(),
        intended_input_boundary: INTENDED_INPUT_BOUNDARY.to_string(),
    };
    fs::create_dir_all(&cache_entry).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "creating the cache entry {}: {error}",
            cache_entry.display()
        ))
    })?;
    let serialized = serde_json::to_vec_pretty(&record).map_err(|error| {
        ResolveFailure::Instrument(format!("serializing the cache entry record: {error}"))
    })?;
    fs::write(&record_path, serialized).map_err(|error| {
        ResolveFailure::Instrument(format!("writing {}: {error}", record_path.display()))
    })?;
    Ok(ResolvedSubject {
        subject_id: subject_id.to_string(),
        client: runner_client_subject(row, resolved_digest),
        emacs_executable: canonical_executable,
        client_source,
        emacs_build_sha256,
        cache_key,
        cache_entry,
        reused_cache: false,
    })
}

/// Build the runner-facing client subject for a row whose declared digest
/// has just been validated.
pub fn runner_client_subject(
    row: &SubjectRow,
    resolved_source_sha256: String,
) -> crate::emacs_host_run::emacs_host_runner::ClientSubject {
    crate::emacs_host_run::emacs_host_runner::ClientSubject {
        client_id: row.subject_id.clone(),
        kind: row.client_kind.runner_kind(),
        version: row.client_version_hint.clone(),
        source_state: row.source_state,
        source_ref: row.emacs_release_tag.clone(),
        source_sha256: resolved_source_sha256,
        // Bundled subjects explicitly have no independent package/archive
        // identity; external rows (with package identities) arrive with
        // #11745/#11746 and extend this constructor's use.
        package_sha256: None,
    }
}

fn classify_installation_resolution_error(
    subject_id: &str,
    error: &anyhow::Error,
) -> ResolveFailure {
    let message = format!("{error:#}");
    if message.contains("no bundled Eglot library") {
        ResolveFailure::Rejected(SubjectRejection::UnavailableSubject {
            subject_id: subject_id.to_string(),
            reason: "no bundled client library of any declared form was found inside the \
                     exact Emacs installation"
                .to_string(),
        })
    } else if message.contains("ambiguous") {
        ResolveFailure::Rejected(SubjectRejection::IdentityMismatch {
            subject_id: subject_id.to_string(),
            reason: message,
        })
    } else {
        ResolveFailure::Instrument(format!("resolving the bundled client source: {message}"))
    }
}

/// If the mismatching digest belongs to another declared row, say so: the
/// file is another generation's material, and pairing it with this subject
/// is exactly the cross-generation substitution the resolver exists to
/// refuse.
fn generation_hint(manifest: &SubjectManifest, subject_id: &str, observed_digest: &str) -> String {
    match manifest
        .subjects
        .iter()
        .find(|row| row.subject_id != subject_id && row.client_source_sha256 == observed_digest)
    {
        Some(other) => format!(
            " the observed digest is the declared digest of subject {} ({})",
            other.subject_id, other.emacs_release_tag
        ),
        None => String::new(),
    }
}

fn identity_difference(
    stale: &CompleteSubjectIdentity,
    current: &CompleteSubjectIdentity,
) -> String {
    if stale.emacs_build_sha256 != current.emacs_build_sha256 {
        return "the cache holds this subject bound to a different Emacs build digest".to_string();
    }
    if stale.declared_client_source_sha256 != current.declared_client_source_sha256
        || stale.resolved_client_sha256 != current.resolved_client_sha256
    {
        return "the cache holds this subject bound to a different client digest".to_string();
    }
    if stale.resolved_library_form != current.resolved_library_form {
        return "the cache holds this subject bound to a different library form".to_string();
    }
    "the cache holds this subject bound to a different complete identity".to_string()
}

fn ambient_layout_marker(client_source: &Path, installation_root: &Path) -> Option<String> {
    let relative = client_source.strip_prefix(installation_root).ok()?;
    relative.components().find_map(|component| {
        let segment = component.as_os_str().to_str()?;
        AMBIENT_PACKAGE_LAYOUT_MARKERS
            .iter()
            .find(|marker| segment.eq_ignore_ascii_case(marker))
            .map(|marker| marker.to_string())
    })
}

fn unexpected_entry_files(cache_entry: &Path) -> Result<Vec<String>, ResolveFailure> {
    let mut unexpected = Vec::new();
    let entries = fs::read_dir(cache_entry).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "listing the cache entry {}: {error}",
            cache_entry.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ResolveFailure::Instrument(format!(
                "listing the cache entry {}: {error}",
                cache_entry.display()
            ))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != CACHE_ENTRY_FILE {
            unexpected.push(name);
        }
    }
    unexpected.sort();
    Ok(unexpected)
}

fn scan_cache_entries(cache_root: &Path) -> Result<Vec<SubjectInputRecord>, ResolveFailure> {
    if !cache_root.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(cache_root).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "listing the bounded cache root {}: {error}",
            cache_root.display()
        ))
    })?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ResolveFailure::Instrument(format!(
                "listing the bounded cache root {}: {error}",
                cache_root.display()
            ))
        })?;
        let record_path = entry.path().join(CACHE_ENTRY_FILE);
        if !record_path.is_file() {
            continue;
        }
        let bytes = fs::read(&record_path).map_err(|error| {
            ResolveFailure::Instrument(format!("reading {}: {error}", record_path.display()))
        })?;
        let record: SubjectInputRecord = serde_json::from_slice(&bytes).map_err(|error| {
            ResolveFailure::Instrument(format!(
                "the sibling cache record {} is corrupt and refuses to parse: {error}",
                record_path.display()
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

fn file_digest(path: &Path) -> Result<String, ResolveFailure> {
    let bytes = fs::read(path).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "reading {} to bind its digest: {error}",
            path.display()
        ))
    })?;
    Ok(prefixed_sha256(&bytes))
}

fn decompressed_digest(path: &Path) -> Result<String, ResolveFailure> {
    let file = fs::File::open(path).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "opening the gzip-wrapped client file {}: {error}",
            path.display()
        ))
    })?;
    let mut decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).map_err(|error| {
        ResolveFailure::Instrument(format!(
            "decompressing the gzip-wrapped client file {}: {error}",
            path.display()
        ))
    })?;
    Ok(prefixed_sha256(&bytes))
}

/// `sha256:<64 hex>`, the manifest's declared-digest form.
fn prefixed_sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut identity = String::with_capacity(64);
    for byte in hasher.finalize() {
        identity.push_str(&format!("{byte:02x}"));
    }
    identity
}

/// Deterministic cache key: sha256 over the canonical JSON serialization of
/// the complete subject identity. Struct field order is fixed and maps are
/// absent, so the serialization is deterministic by construction. The key
/// is bare lowercase hex so it is a safe directory name on every platform.
fn identity_cache_key(identity: &CompleteSubjectIdentity) -> Result<String, ResolveFailure> {
    let bytes = serde_json::to_vec(identity).map_err(|error| {
        ResolveFailure::Instrument(format!("serializing the complete subject identity: {error}"))
    })?;
    Ok(sha256_hex(&bytes))
}
