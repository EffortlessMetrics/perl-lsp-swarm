//! Checked contract for immutable standalone candidates and current selection (#11179).
//!
//! This validator owns the pure, runtime-neutral model only:
//!
//! ```text
//! verified route-specific stage DAG
//! → ImmutableStandaloneCandidate (manifest)
//! → StandaloneCurrentSelection
//! → previous-known-good relationship
//! → install | repair | update | rollback transition record
//! ```
//!
//! It recomputes candidate identity from canonical bytes instead of trusting
//! producer declarations, rejects mixed/partial/path-inferred/cross-root/
//! cross-subject state, and keeps product-transition, cleanup,
//! process/startup, and PATH outcomes as four independent dimensions. It
//! touches no filesystem state: no lock, directory, rename, pointer, PATH
//! entry, promotion, rollback action, or cleanup is performed or implied.
//! POSIX and PowerShell adapters consume the identical semantic records
//! without gaining a new runtime dependency (#11099 boundary).

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

const PACKET_SCHEMA_VERSION: &str = "standalone_candidate_selection_vector.v1";
const CANDIDATE_SCHEMA_VERSION: &str = "standalone_candidate.v1";
const SELECTION_SCHEMA_VERSION: &str = "standalone_current_selection.v1";
const TRANSITION_SCHEMA_VERSION: &str = "standalone_install_transition.v1";

/// Domain separation for derived candidate identity. Candidate IDs are
/// field-sensitive over canonical bytes; a display version, directory name,
/// or single binary digest can never merge two candidates.
const MANIFEST_DIGEST_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-candidate-manifest.v1\0";
const CANDIDATE_ID_DOMAIN: &[u8] = b"perl-lsp-swarm:standalone-candidate-id.v1\0";

const MAX_ID_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 512;

macro_rules! closed_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        $(#[$meta])*
        enum $name {
            $(#[serde(rename = $text)] $variant),+
        }
    };
}

closed_enum!(RouteMode {
    FirstPartyPosix => "first_party_posix",
    FirstPartyPowershell => "first_party_powershell"
});
closed_enum!(SourceMode {
    ReleaseArchive => "release_archive",
    ExactRegistrySource => "exact_registry_source",
    ExplicitLocalDevelopment => "explicit_local_development"
});
closed_enum!(SelectionOperation {
    Install => "install",
    Repair => "repair",
    Update => "update",
    Rollback => "rollback"
});
closed_enum!(ProductUnitDisposition {
    ArchivePairRequired => "archive_pair_required",
    HistoricalServerOnly => "historical_server_only",
    AdvancedSourceServerOnly => "advanced_source_server_only"
});
closed_enum!(MemberRole {
    PerllspServer => "perllsp_server",
    PerlDapAdapter => "perl_dap_adapter"
});

impl MemberRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PerllspServer => "perllsp_server",
            Self::PerlDapAdapter => "perl_dap_adapter",
        }
    }
}
closed_enum!(LibcDisposition {
    Gnu => "gnu",
    Musl => "musl",
    Msvc => "msvc",
    NoneLibc => "none"
});
closed_enum!(InstallRootRole {
    UserLocal => "user_local",
    SystemShared => "system_shared"
});
closed_enum!(CommitResult {
    Committed => "committed",
    PreservedPriorState => "preserved_prior_state"
});
closed_enum!(TransitionDisposition {
    CandidateVerified => "candidate_verified",
    CandidatePublishedUnselected => "candidate_published_unselected",
    SelectionCommitted => "selection_committed",
    SelectionUnchanged => "selection_unchanged",
    RollbackCommitted => "rollback_committed",
    FailedPreservedCurrent => "failed_preserved_current",
    CancelledPreservedCurrent => "cancelled_preserved_current",
    NotProvenPreservedCurrent => "not_proven_preserved_current"
});
closed_enum!(ProductTransitionOutcome {
    Installed => "installed",
    Repaired => "repaired",
    Updated => "updated",
    RolledBack => "rolled_back",
    Unchanged => "unchanged",
    PreservedPrior => "preserved_prior",
    NotApplicable => "not_applicable"
});
closed_enum!(CleanupOutcome {
    Completed => "completed",
    Deferred => "deferred",
    FailedPreserved => "failed_preserved",
    NotProven => "not_proven",
    NotApplicable => "not_applicable"
});
closed_enum!(ProcessStartupOutcome {
    Verified => "verified",
    Unproven => "unproven",
    Failed => "failed",
    NotApplicable => "not_applicable"
});
closed_enum!(PathOutcome {
    Persisted => "persisted",
    Unchanged => "unchanged",
    Failed => "failed",
    NotApplicable => "not_applicable"
});

/// Closed accept/reject vocabulary for authored expectations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Accept,
    Reject,
}

impl Verdict {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
        }
    }
}

/// Closed fail-closed reason vocabulary. Every rejection names exactly one
/// code so fixture expectations and adapter projections cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReasonCode {
    MalformedDocument,
    UnknownSchemaVersion,
    MixedPairMembers,
    SourceModeAsPair,
    LocalDevelopmentNonAuthoritative,
    CandidateIdentityDrift,
    DuplicateCandidateIdentity,
    IncompletePairMembers,
    MemberSubjectMismatch,
    DuplicateMemberRole,
    MalformedCurrentRecord,
    CurrentNamesMissingCandidate,
    ManifestDigestMismatch,
    GenerationRegression,
    WrongInstallRoot,
    WrongTargetIdentity,
    PreviousCurrentAlias,
    RevertWithoutRollback,
    RollbackTargetNotGoverned,
    RollbackWithoutPriorState,
    CrossAttemptTransition,
    PrivateOutputLeakage,
    TransitionDispositionConflict,
    PreservedCurrentViolated,
}

impl ReasonCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedDocument => "malformed_document",
            Self::UnknownSchemaVersion => "unknown_schema_version",
            Self::MixedPairMembers => "mixed_pair_members",
            Self::SourceModeAsPair => "source_mode_as_pair",
            Self::LocalDevelopmentNonAuthoritative => "local_development_non_authoritative",
            Self::CandidateIdentityDrift => "candidate_identity_drift",
            Self::DuplicateCandidateIdentity => "duplicate_candidate_identity",
            Self::IncompletePairMembers => "incomplete_pair_members",
            Self::MemberSubjectMismatch => "member_subject_mismatch",
            Self::DuplicateMemberRole => "duplicate_member_role",
            Self::MalformedCurrentRecord => "malformed_current_record",
            Self::CurrentNamesMissingCandidate => "current_names_missing_candidate",
            Self::ManifestDigestMismatch => "manifest_digest_mismatch",
            Self::GenerationRegression => "generation_regression",
            Self::WrongInstallRoot => "wrong_install_root",
            Self::WrongTargetIdentity => "wrong_target_identity",
            Self::PreviousCurrentAlias => "previous_current_alias",
            Self::RevertWithoutRollback => "revert_without_rollback",
            Self::RollbackTargetNotGoverned => "rollback_target_not_governed",
            Self::RollbackWithoutPriorState => "rollback_without_prior_state",
            Self::CrossAttemptTransition => "cross_attempt_transition",
            Self::PrivateOutputLeakage => "private_output_leakage",
            Self::TransitionDispositionConflict => "transition_disposition_conflict",
            Self::PreservedCurrentViolated => "preserved_current_violated",
        }
    }
}

#[derive(Debug)]
struct ContractError {
    code: ReasonCode,
    detail: String,
}

impl ContractError {
    fn new(code: ReasonCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

impl Display for ContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.detail)
    }
}

impl std::error::Error for ContractError {}

type ContractResult<T> = std::result::Result<T, ContractError>;

fn err<T>(code: ReasonCode, detail: impl Into<String>) -> ContractResult<T> {
    Err(ContractError::new(code, detail))
}

/// Display prefix for an opaque identity: bounded and char-boundary safe so
/// diagnostics can never panic on malformed input.
fn head(value: &str) -> &str {
    let mut end = value.len().min(16);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn parse_contract<T: for<'de> Deserialize<'de>>(text: &str) -> ContractResult<T> {
    serde_json::from_str(text).map_err(|error| {
        ContractError::new(ReasonCode::MalformedDocument, format!("parse error: {error}"))
    })
}

// ---------------------------------------------------------------------------
// Bounded scalar validation
// ---------------------------------------------------------------------------

fn hex_sha256(value: &str, field: &str) -> ContractResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return err(
            ReasonCode::MalformedDocument,
            format!("{field} must be exactly 64 hexadecimal characters"),
        );
    }
    Ok(())
}

fn bounded_id(value: &str, field: &str) -> ContractResult<()> {
    let valid = !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'));
    if valid {
        Ok(())
    } else {
        err(
            ReasonCode::MalformedDocument,
            format!("{field} must be a bounded path-safe identity (1..={MAX_ID_CHARS})"),
        )
    }
}

fn bounded_text(value: &str, field: &str, max: usize) -> ContractResult<()> {
    if value.trim().is_empty() || value.len() > max {
        return err(
            ReasonCode::MalformedDocument,
            format!("{field} must be non-empty and at most {max} characters"),
        );
    }
    if value.chars().any(char::is_control) {
        return err(
            ReasonCode::MalformedDocument,
            format!("{field} must not contain control characters"),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Canonical serialization and domain-separated digests
// ---------------------------------------------------------------------------

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let members: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(item)
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Domain-separated digest: the prefix binds the purpose so a digest computed
/// for one role can never be replayed as another identity.
fn domain_digest(domain: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(payload);
    sha256_bytes(&hasher.finalize())
}

// ---------------------------------------------------------------------------
// Privacy boundary
// ---------------------------------------------------------------------------

/// Durable records carry roles, digests, and bounded identities — never
/// private filesystem locations, full environments, or credentials. Any
/// string value matching these shapes fails closed.
fn value_leaks_private_output(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => privacy_finding(text),
        Value::Array(items) => items.iter().find_map(value_leaks_private_output),
        Value::Object(map) => map.values().find_map(value_leaks_private_output),
        _ => None,
    }
}

fn privacy_finding(text: &str) -> Option<String> {
    if text.contains('\\') {
        return Some(format!("backslash path separator in {text:?}"));
    }
    if text.starts_with('/') && text.len() > 1 {
        return Some(format!("rooted absolute path in {text:?}"));
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        return Some(format!("drive-letter absolute path in {text:?}"));
    }
    let lowered = text.to_ascii_lowercase();
    for needle in
        ["path=", "${", "$home", "%home", "bearer ", "private key", "password", "secret", "token"]
    {
        if lowered.contains(needle) {
            return Some(format!("credential/environment marker {needle:?} in {text:?}"));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Immutable standalone candidate manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExecutableMember {
    role: MemberRole,
    artifact_sha256: String,
    observation_packet_sha256: String,
    /// Every member of one candidate binds the exact #11099 resolved-subject
    /// digest; members resolved from different subjects never compose.
    subject_binding: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateManifest {
    schema_version: String,
    policy_version: String,
    route_mode: RouteMode,
    source_mode: SourceMode,
    transaction_id: String,
    attempt_id: String,
    created_by_operation: SelectionOperation,
    resolved_subject_digest: String,
    stage_dag_digest: String,
    release_display_version: String,
    release_topology_digest: String,
    target_platform: String,
    target_triple: String,
    target_libc: LibcDisposition,
    product_unit_disposition: ProductUnitDisposition,
    dap_preview_maturity: bool,
    candidate_generation: u64,
    members: Vec<ExecutableMember>,
    limitations: Vec<String>,
    /// Declared derived identities. The verifier always recomputes both.
    candidate_id: String,
    manifest_sha256: String,
}

impl CandidateManifest {
    fn target_identity(&self) -> (&str, &str, LibcDisposition) {
        (&self.target_platform, &self.target_triple, self.target_libc)
    }

    fn has_role(&self, role: MemberRole) -> bool {
        self.members.iter().any(|member| member.role == role)
    }
}

/// Recompute both derived identities over canonical content bytes. The
/// declared `candidate_id` and `manifest_sha256` fields are excluded from the
/// hashed content, breaking any circularity.
fn recompute_candidate_identity(manifest: &CandidateManifest) -> ContractResult<(String, String)> {
    let mut value = serde_json::to_value(manifest).map_err(|error| {
        ContractError::new(
            ReasonCode::MalformedDocument,
            format!("candidate serialization failed: {error}"),
        )
    })?;
    let Value::Object(ref mut map) = value else {
        return err(
            ReasonCode::MalformedDocument,
            "candidate manifest must serialize to an object",
        );
    };
    map.remove("candidate_id");
    map.remove("manifest_sha256");
    let manifest_sha256 = domain_digest(MANIFEST_DIGEST_DOMAIN, canonical_json(&value).as_bytes());
    let candidate_id = domain_digest(CANDIDATE_ID_DOMAIN, manifest_sha256.as_bytes());
    Ok((candidate_id, manifest_sha256))
}

fn validate_member(
    member: &ExecutableMember,
    index: usize,
    manifest: &CandidateManifest,
) -> ContractResult<()> {
    hex_sha256(&member.artifact_sha256, &format!("members[{index}].artifact_sha256"))?;
    hex_sha256(
        &member.observation_packet_sha256,
        &format!("members[{index}].observation_packet_sha256"),
    )?;
    hex_sha256(&member.subject_binding, &format!("members[{index}].subject_binding"))?;
    if member.subject_binding != manifest.resolved_subject_digest {
        return err(
            ReasonCode::MemberSubjectMismatch,
            format!(
                "members[{index}] ({}) was resolved under subject {} but the candidate names subject {}",
                member.role.as_str(),
                &member.subject_binding[..12],
                &manifest.resolved_subject_digest[..12]
            ),
        );
    }
    Ok(())
}

fn validate_candidate(manifest: &CandidateManifest) -> ContractResult<(String, String)> {
    if manifest.schema_version != CANDIDATE_SCHEMA_VERSION {
        return err(
            ReasonCode::UnknownSchemaVersion,
            format!(
                "schema_version must be {CANDIDATE_SCHEMA_VERSION}, got {}",
                manifest.schema_version
            ),
        );
    }
    bounded_text(&manifest.policy_version, "policy_version", 64)?;
    bounded_id(&manifest.transaction_id, "transaction_id")?;
    bounded_id(&manifest.attempt_id, "attempt_id")?;
    bounded_text(&manifest.release_display_version, "release_display_version", 64)?;
    hex_sha256(&manifest.resolved_subject_digest, "resolved_subject_digest")?;
    hex_sha256(&manifest.stage_dag_digest, "stage_dag_digest")?;
    hex_sha256(&manifest.release_topology_digest, "release_topology_digest")?;
    bounded_text(&manifest.target_platform, "target_platform", 32)?;
    bounded_text(&manifest.target_triple, "target_triple", 64)?;
    if manifest.candidate_generation == 0 {
        return err(ReasonCode::MalformedDocument, "candidate_generation starts at 1");
    }
    if manifest.limitations.len() > 8 {
        return err(ReasonCode::MalformedDocument, "at most 8 limitations entries");
    }
    for (index, limitation) in manifest.limitations.iter().enumerate() {
        bounded_text(limitation, &format!("limitations[{index}]"), MAX_TEXT_CHARS)?;
    }
    if manifest.members.is_empty() || manifest.members.len() > 2 {
        return err(
            ReasonCode::MalformedDocument,
            "a candidate carries one or two executable members",
        );
    }
    for (index, member) in manifest.members.iter().enumerate() {
        validate_member(member, index, manifest)?;
    }
    let mut seen_roles = BTreeMap::new();
    for member in &manifest.members {
        if seen_roles.insert(member.role.as_str(), ()).is_some() {
            return err(
                ReasonCode::DuplicateMemberRole,
                format!("duplicate member role {}", member.role.as_str()),
            );
        }
    }

    // Route-mode / disposition matrix. A source-mode build can never wear an
    // archive-pair or historical-archive label, and local development is
    // explicitly non-authoritative as durable candidate bytes.
    match (manifest.source_mode, manifest.product_unit_disposition) {
        (SourceMode::ExplicitLocalDevelopment, _) => {
            return err(
                ReasonCode::LocalDevelopmentNonAuthoritative,
                "explicit local-development builds are non-authoritative and cannot become durable candidates",
            );
        }
        (SourceMode::ReleaseArchive, ProductUnitDisposition::AdvancedSourceServerOnly) => {
            return err(
                ReasonCode::SourceModeAsPair,
                "historical archive route cannot declare advanced source server-only product units",
            );
        }
        (SourceMode::ExactRegistrySource, ProductUnitDisposition::ArchivePairRequired)
        | (SourceMode::ExactRegistrySource, ProductUnitDisposition::HistoricalServerOnly) => {
            return err(
                ReasonCode::SourceModeAsPair,
                "source-mode candidates cannot be labeled as archive products",
            );
        }
        (SourceMode::ReleaseArchive, ProductUnitDisposition::ArchivePairRequired)
        | (SourceMode::ReleaseArchive, ProductUnitDisposition::HistoricalServerOnly)
        | (SourceMode::ExactRegistrySource, ProductUnitDisposition::AdvancedSourceServerOnly) => {}
    }

    // Required member completeness per product-unit disposition. DAP preview
    // maturity never weakens archive_pair_required integrity: both executable
    // roles remain mandatory with full observation packets.
    let pair_complete = manifest.has_role(MemberRole::PerllspServer)
        && manifest.has_role(MemberRole::PerlDapAdapter);
    let server_only = manifest.has_role(MemberRole::PerllspServer) && manifest.members.len() == 1;
    match manifest.product_unit_disposition {
        ProductUnitDisposition::ArchivePairRequired if !pair_complete => {
            return err(
                ReasonCode::IncompletePairMembers,
                "archive_pair_required demands both perllsp_server and perl_dap_adapter members with observation packets",
            );
        }
        ProductUnitDisposition::HistoricalServerOnly
        | ProductUnitDisposition::AdvancedSourceServerOnly
            if !server_only =>
        {
            return err(
                ReasonCode::IncompletePairMembers,
                "server-only dispositions demand exactly one perllsp_server member",
            );
        }
        ProductUnitDisposition::ArchivePairRequired
        | ProductUnitDisposition::HistoricalServerOnly
        | ProductUnitDisposition::AdvancedSourceServerOnly => {}
    }

    let (candidate_id, manifest_sha256) = recompute_candidate_identity(manifest)?;
    if candidate_id != manifest.candidate_id || manifest_sha256 != manifest.manifest_sha256 {
        return err(
            ReasonCode::CandidateIdentityDrift,
            format!(
                "declared identity drifts from canonical recomputation: declared ({}, {}), computed ({candidate_id}, {manifest_sha256})",
                head(&manifest.candidate_id),
                head(&manifest.manifest_sha256)
            ),
        );
    }
    Ok((candidate_id, manifest_sha256))
}

// ---------------------------------------------------------------------------
// Current selection record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallRootIdentity {
    root_role: InstallRootRole,
    /// Logical install-root identity digest; platform implementations bind it
    /// to concrete locations locally. The durable record itself never stores
    /// a private path.
    root_identity_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreviousKnownGoodRef {
    candidate_id: String,
    manifest_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentSelection {
    schema_version: String,
    selection_policy_version: String,
    install_root: InstallRootIdentity,
    /// Monotonic within one logical install root. Generations identify the
    /// selection state: committed changes strictly increase it; preserved
    /// prior-state records keep it.
    selection_generation: u64,
    selected_candidate_id: String,
    selected_manifest_sha256: String,
    previous_known_good: Option<PreviousKnownGoodRef>,
    operation: SelectionOperation,
    transaction_id: String,
    attempt_id: String,
    commit_result: CommitResult,
    bounded_reason: String,
}

fn validate_root(root: &InstallRootIdentity) -> ContractResult<()> {
    hex_sha256(&root.root_identity_sha256, "install_root.root_identity_sha256")
}

/// Validate one selection record against the catalog. Completeness is proven
/// by membership and recomputed digests, never by a producer flag.
fn validate_selection<'a>(
    selection: &CurrentSelection,
    catalog: &'a Catalog<'a>,
) -> ContractResult<&'a CandidateManifest> {
    if selection.schema_version != SELECTION_SCHEMA_VERSION {
        return err(
            ReasonCode::UnknownSchemaVersion,
            format!(
                "schema_version must be {SELECTION_SCHEMA_VERSION}, got {}",
                selection.schema_version
            ),
        );
    }
    bounded_text(&selection.selection_policy_version, "selection_policy_version", 64)?;
    validate_root(&selection.install_root)?;
    if selection.selection_generation == 0 {
        return err(ReasonCode::MalformedCurrentRecord, "selection_generation starts at 1");
    }
    bounded_id(&selection.transaction_id, "transaction_id")?;
    bounded_id(&selection.attempt_id, "attempt_id")?;
    bounded_text(&selection.bounded_reason, "bounded_reason", MAX_TEXT_CHARS)?;
    hex_sha256(&selection.selected_manifest_sha256, "selected_manifest_sha256")?;

    let selected =
        catalog.resolve(&selection.selected_candidate_id, &selection.selected_manifest_sha256)?;
    if let Some(previous) = &selection.previous_known_good {
        hex_sha256(&previous.manifest_sha256, "previous_known_good.manifest_sha256")?;
        if previous.candidate_id == selection.selected_candidate_id {
            return err(
                ReasonCode::PreviousCurrentAlias,
                "previous_known_good and selected candidate must name different candidates",
            );
        }
        catalog.resolve(&previous.candidate_id, &previous.manifest_sha256)?;
    }
    Ok(selected)
}

// ---------------------------------------------------------------------------
// Transition record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransitionOutcomeDimensions {
    product_units: ProductTransitionOutcome,
    cleanup: CleanupOutcome,
    process_startup: ProcessStartupOutcome,
    path_persistence: PathOutcome,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallTransition {
    schema_version: String,
    route_mode: RouteMode,
    operation: SelectionOperation,
    transaction_id: String,
    attempt_id: String,
    disposition: TransitionDisposition,
    candidate_id: Option<String>,
    prior_current_candidate_id: Option<String>,
    outcome_dimensions: TransitionOutcomeDimensions,
    bounded_reason: String,
}

// ---------------------------------------------------------------------------
// Vector packet envelope (fixture harness vocabulary, not a durable record)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    verdict: Verdict,
    /// Required when rejecting; names one closed fail-closed reason code.
    reason_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VectorPacket {
    packet_schema: String,
    expectation: Expectation,
    candidates: Vec<CandidateManifest>,
    prior_selection: Option<CurrentSelection>,
    next_selection: Option<CurrentSelection>,
    transition: Option<InstallTransition>,
}

struct Catalog<'a> {
    by_id: BTreeMap<&'a str, &'a CandidateManifest>,
}

impl<'a> Catalog<'a> {
    fn build(candidates: &'a [CandidateManifest]) -> ContractResult<Self> {
        let mut by_id = BTreeMap::new();
        // Generation uniqueness is scoped to one target lineage
        // (route/platform/triple/libc): a multi-platform catalog legitimately
        // restarts each target's sequence at 1, while two candidates claiming
        // the same generation within one lineage is drift.
        let mut generations = BTreeMap::new();
        for candidate in candidates {
            validate_candidate(candidate)?;
            if by_id.insert(candidate.candidate_id.as_str(), candidate).is_some() {
                return err(
                    ReasonCode::DuplicateCandidateIdentity,
                    format!("two candidates claim identity {}", head(&candidate.candidate_id)),
                );
            }
            let lineage = (
                candidate.route_mode,
                candidate.target_platform.as_str(),
                candidate.target_triple.as_str(),
                candidate.target_libc,
                candidate.candidate_generation,
            );
            if generations.insert(lineage, candidate.candidate_id.as_str()).is_some() {
                return err(
                    ReasonCode::DuplicateCandidateIdentity,
                    format!(
                        "candidate_generation {} claimed twice within target lineage {:?}",
                        candidate.candidate_generation,
                        (candidate.target_platform.as_str(), candidate.target_triple.as_str())
                    ),
                );
            }
        }
        Ok(Self { by_id })
    }

    fn resolve(
        &self,
        candidate_id: &str,
        manifest_sha256: &str,
    ) -> ContractResult<&'a CandidateManifest> {
        let candidate = self.by_id.get(candidate_id).ok_or_else(|| {
            ContractError::new(
                ReasonCode::CurrentNamesMissingCandidate,
                format!(
                    "record names candidate {} which is absent from the verified catalog",
                    head(candidate_id)
                ),
            )
        })?;
        if candidate.manifest_sha256 != manifest_sha256 {
            return err(
                ReasonCode::ManifestDigestMismatch,
                format!(
                    "record binds manifest {} but candidate {} carries {}",
                    head(manifest_sha256),
                    head(candidate_id),
                    head(&candidate.manifest_sha256)
                ),
            );
        }
        Ok(candidate)
    }
}

// ---------------------------------------------------------------------------
// Packet verification
// ---------------------------------------------------------------------------

fn verify_packet(packet: &VectorPacket) -> ContractResult<()> {
    if packet.packet_schema != PACKET_SCHEMA_VERSION {
        return err(
            ReasonCode::UnknownSchemaVersion,
            format!("packet_schema must be {PACKET_SCHEMA_VERSION}, got {}", packet.packet_schema),
        );
    }
    match (&packet.expectation.verdict, &packet.expectation.reason_code) {
        (Verdict::Accept, None) => {}
        (Verdict::Reject, Some(reason_code)) => {
            if parse_reason_code(reason_code).is_none() {
                return err(
                    ReasonCode::MalformedDocument,
                    format!(
                        "expectation reason_code {reason_code:?} is outside the closed vocabulary"
                    ),
                );
            }
        }
        (Verdict::Accept, Some(unexpected)) => {
            return err(
                ReasonCode::MalformedDocument,
                format!("accepting expectation must not carry reason_code {unexpected:?}"),
            );
        }
        (Verdict::Reject, None) => {
            return err(
                ReasonCode::MalformedDocument,
                "rejecting expectation must name its reason_code",
            );
        }
    }

    let catalog = Catalog::build(&packet.candidates)?;

    let prior = packet
        .prior_selection
        .as_ref()
        .map(|selection| validate_selection(selection, &catalog))
        .transpose()?;
    let next = packet
        .next_selection
        .as_ref()
        .map(|selection| validate_selection(selection, &catalog))
        .transpose()?;

    verify_selection_transition(packet, prior, next)?;
    if let Some(transition) = &packet.transition {
        verify_transition_record(packet, prior, next, transition)?;
    }
    Ok(())
}

fn same_install_root(a: &InstallRootIdentity, b: &InstallRootIdentity) -> bool {
    a.root_role == b.root_role && a.root_identity_sha256 == b.root_identity_sha256
}

fn verify_selection_transition(
    packet: &VectorPacket,
    prior: Option<&CandidateManifest>,
    next: Option<&CandidateManifest>,
) -> ContractResult<()> {
    let (Some(prior_selection), Some(next_selection)) =
        (&packet.prior_selection, &packet.next_selection)
    else {
        return Ok(());
    };
    let Some(prior_candidate) = prior else {
        return err(ReasonCode::MalformedCurrentRecord, "prior selection lost its candidate");
    };
    let Some(next_candidate) = next else {
        return err(ReasonCode::MalformedCurrentRecord, "next selection lost its candidate");
    };

    if !same_install_root(&prior_selection.install_root, &next_selection.install_root) {
        return err(
            ReasonCode::WrongInstallRoot,
            "selection changed logical install root or role; cross-root selection is invalid",
        );
    }

    match next_selection.commit_result {
        CommitResult::Committed => {
            if next_selection.selection_generation <= prior_selection.selection_generation {
                return err(
                    ReasonCode::GenerationRegression,
                    format!(
                        "committed selection generation {} does not advance prior generation {}",
                        next_selection.selection_generation, prior_selection.selection_generation
                    ),
                );
            }
        }
        CommitResult::PreservedPriorState => {
            let unchanged_state = next_selection.selection_generation
                == prior_selection.selection_generation
                && next_selection.selected_candidate_id == prior_selection.selected_candidate_id
                && next_selection.selected_manifest_sha256
                    == prior_selection.selected_manifest_sha256
                && match (&next_selection.previous_known_good, &prior_selection.previous_known_good)
                {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        a.candidate_id == b.candidate_id && a.manifest_sha256 == b.manifest_sha256
                    }
                    _ => false,
                };
            if !unchanged_state {
                return err(
                    ReasonCode::PreservedCurrentViolated,
                    "a preserved prior-state record must keep the current generation, candidate, and previous reference unchanged",
                );
            }
        }
    }

    let reverting_to_previous = prior_selection
        .previous_known_good
        .as_ref()
        .is_some_and(|previous| previous.candidate_id == next_selection.selected_candidate_id);
    if reverting_to_previous && next_selection.operation != SelectionOperation::Rollback {
        return err(
            ReasonCode::RevertWithoutRollback,
            "returning to the previous-known-good candidate requires an explicit rollback operation",
        );
    }
    if next_selection.operation == SelectionOperation::Rollback
        && next_selection.commit_result == CommitResult::Committed
    {
        let governed_target = prior_selection
            .previous_known_good
            .as_ref()
            .is_some_and(|previous| previous.candidate_id == next_selection.selected_candidate_id);
        let governed_previous = next_selection
            .previous_known_good
            .as_ref()
            .map(|previous| previous.candidate_id == prior_selection.selected_candidate_id)
            .unwrap_or(false);
        if !governed_target || !governed_previous {
            return err(
                ReasonCode::RollbackTargetNotGoverned,
                "rollback must select the governed previous-known-good candidate and demote current to previous",
            );
        }
    }

    if next_selection.commit_result == CommitResult::Committed
        && next_selection.selected_candidate_id != prior_selection.selected_candidate_id
        && next_candidate.target_identity() != prior_candidate.target_identity()
    {
        return err(
            ReasonCode::WrongTargetIdentity,
            "candidates selected within one logical install root must share platform/target/libc identity",
        );
    }
    Ok(())
}

fn verify_transition_record(
    packet: &VectorPacket,
    prior: Option<&CandidateManifest>,
    next: Option<&CandidateManifest>,
    transition: &InstallTransition,
) -> ContractResult<()> {
    if transition.schema_version != TRANSITION_SCHEMA_VERSION {
        return err(
            ReasonCode::UnknownSchemaVersion,
            format!(
                "schema_version must be {TRANSITION_SCHEMA_VERSION}, got {}",
                transition.schema_version
            ),
        );
    }
    bounded_id(&transition.transaction_id, "transaction_id")?;
    bounded_id(&transition.attempt_id, "attempt_id")?;
    bounded_text(&transition.bounded_reason, "bounded_reason", MAX_TEXT_CHARS)?;
    if let Some(candidate_id) = &transition.candidate_id {
        hex_sha256(candidate_id, "candidate_id")?;
    }
    if let Some(candidate_id) = &transition.prior_current_candidate_id {
        hex_sha256(candidate_id, "prior_current_candidate_id")?;
    }

    // Route binding covers every candidate the transition names or
    // transitions between: the prior/next selections plus the candidates the
    // transition record itself names. Every named identity must resolve in
    // the verified catalog — a well-formed digest the catalog does not carry
    // is rejected fail-closed instead of skipping the route check — so a
    // publish/verify transition without selection records still binds its
    // subject's route and can never verify against an unknown identity.
    let mut route_witnesses: Vec<&CandidateManifest> =
        [prior, next].into_iter().flatten().collect();
    for candidate_id in
        [&transition.candidate_id, &transition.prior_current_candidate_id].into_iter().flatten()
    {
        let candidate = packet
            .candidates
            .iter()
            .find(|candidate| &candidate.candidate_id == candidate_id)
            .ok_or_else(|| {
                ContractError::new(
                    ReasonCode::CurrentNamesMissingCandidate,
                    format!(
                        "transition names candidate {} which is absent from the verified catalog",
                        head(candidate_id)
                    ),
                )
            })?;
        route_witnesses.push(candidate);
    }
    let route_binding =
        route_witnesses.iter().all(|candidate| candidate.route_mode == transition.route_mode);
    if !route_binding {
        return err(
            ReasonCode::TransitionDispositionConflict,
            "transition route mode disagrees with the candidates it transitions between",
        );
    }

    let next_committed = packet
        .next_selection
        .as_ref()
        .is_some_and(|selection| selection.commit_result == CommitResult::Committed);

    match transition.disposition {
        TransitionDisposition::RollbackCommitted => {
            if transition.operation != SelectionOperation::Rollback || !next_committed {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "rollback_committed requires a rollback operation with a committed selection change",
                );
            }
            let Some(prior_selection) = &packet.prior_selection else {
                return err(
                    ReasonCode::RollbackWithoutPriorState,
                    "a committed rollback must prove the prior state it reverts; packets without prior_selection cannot document rollback_committed",
                );
            };
            if transition.prior_current_candidate_id.as_deref()
                != Some(prior_selection.selected_candidate_id.as_str())
            {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "rollback_committed must name the demoted candidate as prior_current_candidate_id",
                );
            }
            if !matches!(
                transition.outcome_dimensions.product_units,
                ProductTransitionOutcome::RolledBack
            ) {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "a committed rollback must report rolled_back product units",
                );
            }
        }
        TransitionDisposition::SelectionCommitted => {
            if transition.operation == SelectionOperation::Rollback || !next_committed {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "selection_committed requires a non-rollback operation with a committed selection change",
                );
            }
            let product_ok = matches!(
                transition.outcome_dimensions.product_units,
                ProductTransitionOutcome::Installed
                    | ProductTransitionOutcome::Repaired
                    | ProductTransitionOutcome::Updated
            );
            if !product_ok {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "a committed selection must move product units to installed/repaired/updated",
                );
            }
        }
        TransitionDisposition::SelectionUnchanged => {
            let same_candidate = match (&packet.prior_selection, &packet.next_selection) {
                (Some(prior), Some(next)) => {
                    next.selected_candidate_id == prior.selected_candidate_id
                }
                _ => false,
            };
            if !same_candidate || !next_committed {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "selection_unchanged requires re-committing the currently selected candidate",
                );
            }
        }
        TransitionDisposition::CandidateVerified
        | TransitionDisposition::CandidatePublishedUnselected => {
            if next_committed {
                return err(
                    ReasonCode::TransitionDispositionConflict,
                    "verifying or publishing a candidate never selects it",
                );
            }
            if transition.disposition == TransitionDisposition::CandidatePublishedUnselected {
                let published_is_new =
                    transition.candidate_id.as_deref().is_some_and(|published| {
                        packet
                            .candidates
                            .iter()
                            .any(|candidate| candidate.candidate_id == published)
                            && packet
                                .prior_selection
                                .as_ref()
                                .is_none_or(|prior| prior.selected_candidate_id != published)
                    });
                if !published_is_new {
                    return err(
                        ReasonCode::TransitionDispositionConflict,
                        "candidate_published_unselected must name a cataloged candidate that is not current",
                    );
                }
            }
        }
        TransitionDisposition::FailedPreservedCurrent
        | TransitionDisposition::CancelledPreservedCurrent
        | TransitionDisposition::NotProvenPreservedCurrent => {
            if next_committed {
                return err(
                    ReasonCode::PreservedCurrentViolated,
                    "a failed/cancelled/not-proven attempt cannot commit a new selection",
                );
            }
            let prior_preserved = transition
                .prior_current_candidate_id
                .as_deref()
                .map(|named| {
                    packet
                        .prior_selection
                        .as_ref()
                        .is_some_and(|prior| prior.selected_candidate_id == named)
                })
                .unwrap_or(true);
            if !prior_preserved {
                return err(
                    ReasonCode::PreservedCurrentViolated,
                    "the preserved current candidate must equal the prior selection's candidate",
                );
            }
        }
    }

    // The durable transition record must name exactly the candidate the
    // committed selection selects: an absent, prior, or unrelated identity
    // would make the record dishonest about what changed.
    if let Some(next_selection) = &packet.next_selection {
        let documents_selection = matches!(
            transition.disposition,
            TransitionDisposition::RollbackCommitted
                | TransitionDisposition::SelectionCommitted
                | TransitionDisposition::SelectionUnchanged
        );
        if documents_selection
            && transition.candidate_id.as_deref()
                != Some(next_selection.selected_candidate_id.as_str())
        {
            return err(
                ReasonCode::TransitionDispositionConflict,
                "transition candidate_id must name the candidate the committed selection selects",
            );
        }
    }

    // Attempt freshness: a transition documenting this attempt's effect must
    // not describe a selection written by a different attempt.
    if let Some(next_selection) = &packet.next_selection {
        let documents_next = matches!(
            transition.disposition,
            TransitionDisposition::RollbackCommitted
                | TransitionDisposition::SelectionCommitted
                | TransitionDisposition::SelectionUnchanged
                | TransitionDisposition::FailedPreservedCurrent
                | TransitionDisposition::CancelledPreservedCurrent
                | TransitionDisposition::NotProvenPreservedCurrent
        );
        if documents_next
            && (next_selection.attempt_id != transition.attempt_id
                || next_selection.transaction_id != transition.transaction_id)
        {
            return err(
                ReasonCode::CrossAttemptTransition,
                "transition names a different attempt/transaction than the selection record it documents",
            );
        }
    }
    Ok(())
}

fn parse_reason_code(text: &str) -> Option<ReasonCode> {
    const NAMES: &[(&str, ReasonCode)] = &[
        ("malformed_document", ReasonCode::MalformedDocument),
        ("unknown_schema_version", ReasonCode::UnknownSchemaVersion),
        ("mixed_pair_members", ReasonCode::MixedPairMembers),
        ("source_mode_as_pair", ReasonCode::SourceModeAsPair),
        ("local_development_non_authoritative", ReasonCode::LocalDevelopmentNonAuthoritative),
        ("candidate_identity_drift", ReasonCode::CandidateIdentityDrift),
        ("duplicate_candidate_identity", ReasonCode::DuplicateCandidateIdentity),
        ("incomplete_pair_members", ReasonCode::IncompletePairMembers),
        ("member_subject_mismatch", ReasonCode::MemberSubjectMismatch),
        ("duplicate_member_role", ReasonCode::DuplicateMemberRole),
        ("malformed_current_record", ReasonCode::MalformedCurrentRecord),
        ("current_names_missing_candidate", ReasonCode::CurrentNamesMissingCandidate),
        ("manifest_digest_mismatch", ReasonCode::ManifestDigestMismatch),
        ("generation_regression", ReasonCode::GenerationRegression),
        ("wrong_install_root", ReasonCode::WrongInstallRoot),
        ("wrong_target_identity", ReasonCode::WrongTargetIdentity),
        ("previous_current_alias", ReasonCode::PreviousCurrentAlias),
        ("revert_without_rollback", ReasonCode::RevertWithoutRollback),
        ("rollback_target_not_governed", ReasonCode::RollbackTargetNotGoverned),
        ("rollback_without_prior_state", ReasonCode::RollbackWithoutPriorState),
        ("cross_attempt_transition", ReasonCode::CrossAttemptTransition),
        ("private_output_leakage", ReasonCode::PrivateOutputLeakage),
        ("transition_disposition_conflict", ReasonCode::TransitionDispositionConflict),
        ("preserved_current_violated", ReasonCode::PreservedCurrentViolated),
    ];
    NAMES.iter().find(|(name, _)| *name == text).map(|(_, code)| *code)
}

/// Verify one raw packet document end to end: structural parse, privacy
/// boundary, catalog recomputation, selection composition rules, and
/// transition consistency.
fn verify_document(text: &str) -> ContractResult<VectorPacket> {
    let packet: VectorPacket = parse_contract(text)?;
    if let Some(finding) =
        value_leaks_private_output(&serde_json::to_value(packet.clone()).map_err(|error| {
            ContractError::new(
                ReasonCode::MalformedDocument,
                format!("packet serialization failed: {error}"),
            )
        })?)
    {
        return err(ReasonCode::PrivateOutputLeakage, finding);
    }
    verify_packet(&packet)?;
    Ok(packet)
}

fn check_expectation(packet: &VectorPacket, verification: ContractResult<()>) -> Result<Verdict> {
    match (verification, packet.expectation.verdict) {
        (Ok(()), Verdict::Accept) => Ok(Verdict::Accept),
        (Ok(()), Verdict::Reject) => bail!(
            "fixture expected reject({}) but the packet verified cleanly",
            packet.expectation.reason_code.as_deref().unwrap_or("?")
        ),
        (Err(error), Verdict::Accept) => {
            bail!("fixture expected accept but verification rejected: {error}")
        }
        (Err(error), Verdict::Reject) => {
            let expected = packet.expectation.reason_code.as_deref().unwrap_or("?");
            if error.code.as_str() == expected {
                Ok(Verdict::Reject)
            } else {
                bail!("fixture expected reject({expected}) but verification returned {}", error)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "standalone-candidate-selection")]
#[command(
    about = "Validate the pure standalone candidate/current-selection/transition contract (#11179); verifies documents only and mutates nothing"
)]
struct Args {
    /// One vector packet JSON to verify against its authored expectation.
    #[arg(long)]
    packet: Option<PathBuf>,
    /// Verify every vector packet under a fixtures directory.
    #[arg(long)]
    verify_fixtures: Option<PathBuf>,
    /// Print recomputed derived identities (candidate_id, manifest_sha256)
    /// for every candidate in a packet; a diagnostic for contract authors.
    #[arg(long)]
    print_derived: Option<PathBuf>,
    /// Print the canonical key-sorted serialization of a packet after
    /// verification succeeds.
    #[arg(long)]
    print_canonical: bool,
}

fn load_document(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn verify_packet_file(path: &Path) -> Result<Verdict> {
    let text = load_document(path)?;
    let packet: VectorPacket = parse_contract(&text)
        .map_err(|error| color_eyre::eyre::eyre!("parsing {}: {error}", path.display()))?;
    let verification = verify_document(&text).map(|_| ());
    let verdict = check_expectation(&packet, verification)?;
    println!("{}: {}", path.display(), verdict.as_str());
    Ok(verdict)
}

fn print_derived(path: &Path) -> Result<()> {
    let text = load_document(path)?;
    let packet: VectorPacket = parse_contract(&text)
        .map_err(|error| color_eyre::eyre::eyre!("parsing {}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for (index, candidate) in packet.candidates.iter().enumerate() {
        let (candidate_id, manifest_sha256) = recompute_candidate_identity(candidate)
            .map_err(|error| color_eyre::eyre::eyre!("{}[{}]: {error}", path.display(), index))?;
        rows.push(serde_json::json!({
            "file": path.display().to_string(),
            "index": index,
            "candidate_id": candidate_id,
            "manifest_sha256": manifest_sha256,
        }));
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    if args.packet.is_none() && args.verify_fixtures.is_none() && args.print_derived.is_none() {
        bail!("nothing to do: pass --packet, --verify-fixtures, or --print-derived");
    }

    if let Some(path) = &args.packet {
        let verdict = verify_packet_file(path)?;
        if verdict == Verdict::Reject {
            bail!("packet {} rejected as its expectation requires", path.display());
        }
    }
    if let Some(directory) = &args.verify_fixtures {
        let mut accepted = 0u32;
        let mut rejected = 0u32;
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)
            .with_context(|| format!("reading fixtures directory {}", directory.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("walking fixtures directory {}", directory.display()))?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect();
        entries.sort();
        if entries.is_empty() {
            bail!("no vector packets found under {}", directory.display());
        }
        for path in entries {
            if verify_packet_file(&path)? == Verdict::Accept {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        println!(
            "standalone-candidate-selection: {accepted} accepted, {rejected} rejected as expected"
        );
    }
    if let Some(path) = &args.print_derived {
        print_derived(path)?;
    }
    if args.print_canonical {
        let path = args
            .packet
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("--print-canonical requires --packet"))?;
        let value: Value = serde_json::from_str(&load_document(path)?)?;
        println!("{}", canonical_json(&value));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CANDIDATE_ID_DOMAIN, CandidateManifest, ContractResult, LibcDisposition,
        MANIFEST_DIGEST_DOMAIN, canonical_json, check_expectation, domain_digest, privacy_finding,
        recompute_candidate_identity, verify_document,
    };
    use color_eyre::eyre::{Result, bail, ensure};
    use serde_json::Value;
    use std::fs;

    const FIXTURE_DIR: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/experience/standalone_candidate_selection"
    );

    fn fixture_names() -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(FIXTURE_DIR)? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "json") {
                names.push(
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
            }
        }
        ensure!(!names.is_empty(), "no fixtures found under {FIXTURE_DIR}");
        names.sort();
        Ok(names)
    }

    fn fixture_text(name: &str) -> Result<String> {
        fs::read_to_string(format!("{FIXTURE_DIR}/{name}"))
            .map_err(|error| color_eyre::eyre::eyre!("fixture {name} unreadable: {error}"))
    }

    #[track_caller]
    fn expect_rejected(
        name: &str,
        outcome: ContractResult<&'static str>,
        needle: &str,
    ) -> Result<()> {
        match outcome {
            Ok(code) => bail!("fixture {name} unexpectedly verified ({code})"),
            Err(error) => {
                let rendered = format!("{error}");
                ensure!(
                    rendered.contains(needle),
                    "expected rejection mentioning {needle:?}, got: {rendered}"
                );
                Ok(())
            }
        }
    }

    /// Every fixture must agree with its independently authored expectation.
    #[test]
    fn all_fixtures_agree_with_expectations() -> Result<()> {
        for name in fixture_names()? {
            let text = fixture_text(&name)?;
            let packet: super::VectorPacket = serde_json::from_str(&text)
                .map_err(|error| color_eyre::eyre::eyre!("parsing {name}: {error}"))?;
            let verification = verify_document(&text).map(|_| ());
            let verdict = check_expectation(&packet, verification)?;
            let expected = match packet.expectation.verdict {
                super::Verdict::Accept => "accept",
                super::Verdict::Reject => "reject",
            };
            ensure!(
                verdict.as_str() == expected,
                "fixture {name} produced {} but expects {expected}",
                verdict.as_str()
            );
        }
        Ok(())
    }

    /// Serialize a JSON document with every object's key order reversed at
    /// the text level, so canonicalization can be challenged with a real
    /// permutation of the same document.
    fn reversed_order_json(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let members: Vec<String> = map
                    .iter()
                    .rev()
                    .map(|(key, item)| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap_or_default(),
                            reversed_order_json(item)
                        )
                    })
                    .collect();
                format!("{{{}}}", members.join(","))
            }
            Value::Array(items) => {
                let items: Vec<String> = items.iter().map(reversed_order_json).collect();
                format!("[{}]", items.join(","))
            }
            other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
        }
    }

    #[test]
    fn canonical_serialization_is_deterministic() -> Result<()> {
        for name in ["01_complete_archive_pair.json", "07_ab_selection_committed.json"] {
            let first: Value = serde_json::from_str(&fixture_text(name)?)?;
            let second: Value = serde_json::from_str(&canonical_json(&first))?;
            ensure!(
                canonical_json(&first) == canonical_json(&second),
                "canonical serialization of {name} is not byte-stable"
            );
            // Permutation discrimination: the same document with every
            // object's key order reversed must canonicalize to identical
            // bytes, so an insertion-ordered implementation fails here
            // instead of passing by comparing like-ordered inputs.
            let permuted: Value = serde_json::from_str(&reversed_order_json(&first))?;
            ensure!(
                canonical_json(&first) == canonical_json(&permuted),
                "canonical serialization of {name} depends on input key order"
            );
        }
        Ok(())
    }

    fn candidate_value(name: &str, index: usize) -> Result<Value> {
        let text = fixture_text(name)?;
        let packet: Value = serde_json::from_str(&text)?;
        packet["candidates"]
            .get(index)
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("{name} has no candidate {index}"))
    }

    fn recompute_from_value(candidate: &Value) -> Result<(String, String)> {
        let manifest: CandidateManifest = serde_json::from_value(candidate.clone())
            .map_err(|error| color_eyre::eyre::eyre!("typed parse failed: {error}"))?;
        Ok(recompute_candidate_identity(&manifest)?)
    }

    /// Candidate identity is field-sensitive over canonical bytes: changing
    /// any load-bearing field changes the derived candidate id. A matching
    /// display version, directory-style label, or one binary digest can never
    /// merge two candidates.
    #[test]
    fn candidate_identity_is_field_sensitive() -> Result<()> {
        let base = candidate_value("01_complete_archive_pair.json", 0)?;
        let (base_id, _) = recompute_from_value(&base)?;

        let mutations: Vec<(&str, Value)> = vec![
            ("transaction_id", Value::String("tx-11179-other".into())),
            ("attempt_id", Value::String("at-11179-other".into())),
            ("created_by_operation", Value::String("repair".into())),
            ("resolved_subject_digest", Value::String("e5".repeat(32))),
            ("stage_dag_digest", Value::String("f7".repeat(32))),
            ("release_display_version", Value::String("0.19.0-clone".into())),
            ("release_topology_digest", Value::String("c8".repeat(32))),
            ("target_platform", Value::String("windows".into())),
            ("target_triple", Value::String("aarch64-pc-windows-msvc".into())),
            ("target_libc", Value::String("msvc".into())),
            ("product_unit_disposition", Value::String("historical_server_only".into())),
            ("dap_preview_maturity", Value::Bool(true)),
            ("candidate_generation", Value::from(99u64)),
            ("policy_version", Value::String("v2".into())),
        ];
        for (field, replacement) in &mutations {
            let mut mutated = base.clone();
            mutated[field] = replacement.clone();
            let (id, _) = recompute_from_value(&mutated)?;
            ensure!(id != base_id, "candidate id failed to change when {field} was substituted");
        }
        for pointer in [
            "/members/0/artifact_sha256",
            "/members/0/observation_packet_sha256",
            "/members/0/subject_binding",
            "/members/1/artifact_sha256",
            "/limitations/0",
        ] {
            let mut mutated = base.clone();
            let node = mutated
                .pointer_mut(pointer)
                .ok_or_else(|| color_eyre::eyre::eyre!("pointer {pointer} absent"))?;
            *node = Value::String("ff".repeat(32));
            let (id, _) = recompute_from_value(&mutated)?;
            ensure!(id != base_id, "candidate id failed to change when {pointer} changed");
        }
        Ok(())
    }

    /// Domain separation binds each digest to its purpose.
    #[test]
    fn digests_are_domain_separated() -> Result<()> {
        let payload = b"identical payload bytes";
        ensure!(
            domain_digest(MANIFEST_DIGEST_DOMAIN, payload)
                != domain_digest(CANDIDATE_ID_DOMAIN, payload),
            "two domains produced identical digests over the same payload"
        );
        let candidate = candidate_value("01_complete_archive_pair.json", 0)?;
        let (id, digest) = recompute_from_value(&candidate)?;
        ensure!(id != digest, "candidate id equals its manifest digest");
        Ok(())
    }

    /// Producer-declared completeness is never authority: a selection binding
    /// the wrong manifest digest fails even though the candidate exists.
    #[test]
    fn selection_manifest_binding_is_enforced() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("07_ab_selection_committed.json")?)?;
        value["next_selection"]["selected_manifest_sha256"] = Value::String("9".repeat(64));
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "07(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "manifest_digest_mismatch",
        )?;
        Ok(())
    }

    /// Publishing a candidate does not select it; claiming otherwise conflicts.
    #[test]
    fn published_as_selected_is_rejected() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("05_published_unselected.json")?)?;
        value["transition"]["disposition"] = Value::String("selection_committed".into());
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "05(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;
        Ok(())
    }

    /// A failed new candidate cannot commit or demote the prior current.
    #[test]
    fn failed_attempt_cannot_demote_current() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("06_failed_b_preserves_a.json")?)?;
        value["next_selection"]["commit_result"] = Value::String("committed".into());
        value["next_selection"]["selection_generation"] = Value::from(12u64);
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "06(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "preserved_current_violated",
        )?;
        Ok(())
    }

    #[test]
    fn privacy_boundary_shapes_fail_closed() -> Result<()> {
        for leaking in [
            "/usr/local/bin/perllsp",
            "C:\\Users\\x\\perllsp",
            "PATH=/usr/bin",
            "${HOME}",
            "$HOME/x",
            "%HOME%",
            "Bearer abc123",
            "-----BEGIN PRIVATE KEY-----",
            "api_token",
        ] {
            ensure!(privacy_finding(leaking).is_some(), "privacy scan missed {leaking:?}");
        }
        for benign in
            ["0.19.0", "first_party_posix", "archive_pair_required", "tx-11179-build-a", "a1a1a1a1"]
        {
            ensure!(
                privacy_finding(benign).is_none(),
                "privacy scan false-positived on {benign:?}"
            );
        }
        Ok(())
    }

    /// Re-verifying an already verified document twice in one process yields
    /// identical canonical bytes (second generation produces no diff).
    #[test]
    fn second_generation_is_byte_identical() -> Result<()> {
        let text = fixture_text("02_historical_server_only.json")?;
        let packet = verify_document(&text)?;
        let serialized = serde_json::to_value(&packet)?;
        let first = canonical_json(&serialized);
        let reparsed: Value = serde_json::from_str(&first)?;
        ensure!(first == canonical_json(&reparsed), "canonical bytes drifted on regeneration");
        // Same permutation discrimination as the determinism test: reversed
        // key order must not change the canonical bytes of the envelope.
        let permuted: Value = serde_json::from_str(&reversed_order_json(&serialized))?;
        ensure!(
            first == canonical_json(&permuted),
            "canonical bytes of the verified envelope depend on key order"
        );
        Ok(())
    }

    /// The declared literals are the wire vocabulary, not the snake_case of
    /// the Rust identifiers: `NoneLibc => "none"` must round-trip as "none".
    #[test]
    fn closed_enum_literals_are_the_wire_vocabulary() -> Result<()> {
        ensure!(
            serde_json::to_string(&LibcDisposition::NoneLibc)? == "\"none\"",
            "NoneLibc must serialize to its declared literal"
        );
        ensure!(
            serde_json::from_str::<LibcDisposition>("\"none\"").is_ok(),
            "\"none\" must deserialize as NoneLibc"
        );
        ensure!(
            serde_json::from_str::<LibcDisposition>("\"none_libc\"").is_err(),
            "the identifier's snake_case spelling is not the declared wire vocabulary"
        );
        for (text, value) in [
            ("\"gnu\"", LibcDisposition::Gnu),
            ("\"musl\"", LibcDisposition::Musl),
            ("\"msvc\"", LibcDisposition::Msvc),
        ] {
            ensure!(serde_json::from_str::<LibcDisposition>(text)? == value);
            ensure!(serde_json::to_string(&value)? == text);
        }
        Ok(())
    }

    /// A publish transition without selection records still binds its
    /// subject's route: the candidate named by the transition must agree with
    /// the transition's route_mode even when prior/next are absent.
    #[test]
    fn publish_transition_route_binds_the_named_candidate() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("05_published_unselected.json")?)?;
        let Some(object) = value.as_object_mut() else {
            bail!("05 is not a JSON object");
        };
        object.remove("prior_selection");
        value["transition"]["route_mode"] = Value::String("first_party_powershell".into());
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "05(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;
        Ok(())
    }

    /// Generation uniqueness is scoped per target lineage: a multi-platform
    /// catalog may restart each target's sequence at 1, while two candidates
    /// claiming one generation within a single lineage is still drift.
    #[test]
    fn generation_uniqueness_is_scoped_per_target_lineage() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("01_complete_archive_pair.json")?)?;
        let mut cross_target = candidate_value("01_complete_archive_pair.json", 0)?;
        cross_target["target_platform"] = Value::String("windows".into());
        cross_target["target_triple"] = Value::String("x86_64-pc-windows-msvc".into());
        cross_target["target_libc"] = Value::String("msvc".into());
        let (id, digest) = recompute_from_value(&cross_target)?;
        cross_target["candidate_id"] = Value::String(id);
        cross_target["manifest_sha256"] = Value::String(digest);
        let Some(candidates) = value["candidates"].as_array_mut() else {
            bail!("01 candidates missing");
        };
        candidates.push(cross_target);
        let text = serde_json::to_string(&value)?;
        verify_document(&text)?;

        let mut value: Value =
            serde_json::from_str(&fixture_text("01_complete_archive_pair.json")?)?;
        let mut clone = candidate_value("01_complete_archive_pair.json", 0)?;
        clone["transaction_id"] = Value::String("tx-11179-clone".into());
        let (id, digest) = recompute_from_value(&clone)?;
        clone["candidate_id"] = Value::String(id);
        clone["manifest_sha256"] = Value::String(digest);
        let Some(candidates) = value["candidates"].as_array_mut() else {
            bail!("01 candidates missing");
        };
        candidates.push(clone);
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "01(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "duplicate_candidate_identity",
        )?;
        Ok(())
    }

    /// A committed rollback must prove the prior state it reverts; a packet
    /// with only a committed rollback next-selection is partial evidence.
    #[test]
    fn rollback_requires_proven_prior_state() -> Result<()> {
        let mut value: Value = serde_json::from_str(&fixture_text("08_rollback_b_to_a.json")?)?;
        let Some(object) = value.as_object_mut() else {
            bail!("08 is not a JSON object");
        };
        object.remove("prior_selection");
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "08(mutated)",
            verify_document(&tampered).map(|_| "accept"),
            "rollback_without_prior_state",
        )?;
        Ok(())
    }

    /// The transition record must name the candidate the committed selection
    /// demotes (rollback) and selects (commit): a prior or absent identity is
    /// dishonest about what changed.
    #[test]
    fn rollback_names_the_demoted_current() -> Result<()> {
        let mut value: Value = serde_json::from_str(&fixture_text("08_rollback_b_to_a.json")?)?;
        value["transition"]["prior_current_candidate_id"] =
            value["candidates"][0]["candidate_id"].clone();
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "08(mutated-demoted)",
            verify_document(&tampered).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;
        Ok(())
    }

    #[test]
    fn committed_transition_names_the_selected_candidate() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("07_ab_selection_committed.json")?)?;
        value["transition"]["candidate_id"] =
            value["prior_selection"]["selected_candidate_id"].clone();
        let mismatched = serde_json::to_string(&value)?;
        expect_rejected(
            "07(mutated-mismatch)",
            verify_document(&mismatched).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;

        let mut value: Value =
            serde_json::from_str(&fixture_text("07_ab_selection_committed.json")?)?;
        let Some(transition) = value["transition"].as_object_mut() else {
            bail!("07 transition missing");
        };
        transition.remove("candidate_id");
        let absent = serde_json::to_string(&value)?;
        expect_rejected(
            "07(mutated-absent)",
            verify_document(&absent).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;
        Ok(())
    }

    /// A committed rollback whose independent product-unit outcome says
    /// installed/updated contradicts itself.
    #[test]
    fn rollback_outcome_must_report_rolled_back() -> Result<()> {
        let mut value: Value = serde_json::from_str(&fixture_text("08_rollback_b_to_a.json")?)?;
        value["transition"]["outcome_dimensions"]["product_units"] =
            Value::String("installed".into());
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "08(mutated-outcome)",
            verify_document(&tampered).map(|_| "accept"),
            "transition_disposition_conflict",
        )?;
        Ok(())
    }

    /// Every candidate identity a transition names must resolve in the
    /// verified catalog: a well-formed 64-hex digest the catalog does not
    /// carry is rejected fail-closed instead of verifying against an
    /// unknown identity (candidate_verified has no other membership check).
    #[test]
    fn transition_names_must_resolve_in_the_verified_catalog() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(&fixture_text("05_published_unselected.json")?)?;
        value["transition"]["disposition"] = Value::String("candidate_verified".into());
        value["transition"]["candidate_id"] = Value::String("e".repeat(64));
        let tampered = serde_json::to_string(&value)?;
        expect_rejected(
            "05(mutated-absent)",
            verify_document(&tampered).map(|_| "accept"),
            "current_names_missing_candidate",
        )?;
        Ok(())
    }
}
