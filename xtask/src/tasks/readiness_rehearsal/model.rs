//! Typed `readiness_rehearsal.v1` document and deterministic projections (#16785).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const SCHEMA_PATH: &str = "schemas/readiness_rehearsal.v1.schema.json";
pub(crate) const SCHEMA_VERSION: &str = "readiness_rehearsal.v1";
pub(crate) const FIXTURE_DIR: &str = "fixtures/readiness_rehearsal";
pub(crate) const DIGEST_DOMAIN: &[u8] = b"perl_lsp.readiness_rehearsal.v1.receipt\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    WrongBinaryPathRole,
    MixedSourceSha,
    MixedVersion,
    MissingArchiveMember,
    DuplicateArchiveMember,
    CollidingArchiveMember,
    ChecksumInvalidMember,
    VsixServerMismatch,
    UnavailableInstrumentPass,
    NonTerminalStagePass,
    NonzeroExitPass,
    UncleanTreePass,
    InstalledMemberMismatch,
    FailedCleanupHidden,
    MutationAttempted,
    PrivacyLeak,
    CompensatingStage,
    PublishedChannelsNonEmpty,
    ReleaseCutTrue,
    StatusMismatch,
    IdentityKindMismatch,
    ReceiptDigestMismatch,
    SchemaViolation,
    MalformedDocument,
}

impl ReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WrongBinaryPathRole => "wrong_binary_path_role",
            Self::MixedSourceSha => "mixed_source_sha",
            Self::MixedVersion => "mixed_version",
            Self::MissingArchiveMember => "missing_archive_member",
            Self::DuplicateArchiveMember => "duplicate_archive_member",
            Self::CollidingArchiveMember => "colliding_archive_member",
            Self::ChecksumInvalidMember => "checksum_invalid_member",
            Self::VsixServerMismatch => "vsix_server_mismatch",
            Self::UnavailableInstrumentPass => "unavailable_instrument_pass",
            Self::NonTerminalStagePass => "non_terminal_stage_pass",
            Self::NonzeroExitPass => "nonzero_exit_pass",
            Self::UncleanTreePass => "unclean_tree_pass",
            Self::InstalledMemberMismatch => "installed_member_mismatch",
            Self::FailedCleanupHidden => "failed_cleanup_hidden",
            Self::MutationAttempted => "mutation_attempted",
            Self::PrivacyLeak => "privacy_leak",
            Self::CompensatingStage => "compensating_stage",
            Self::PublishedChannelsNonEmpty => "published_channels_non_empty",
            Self::ReleaseCutTrue => "release_cut_true",
            Self::StatusMismatch => "status_mismatch",
            Self::IdentityKindMismatch => "identity_kind_mismatch",
            Self::ReceiptDigestMismatch => "receipt_digest_mismatch",
            Self::SchemaViolation => "schema_violation",
            Self::MalformedDocument => "malformed_document",
        }
    }
}

#[derive(Debug)]
pub struct RehearsalError {
    pub code: ReasonCode,
    pub message: String,
}

impl RehearsalError {
    pub(crate) fn new(code: ReasonCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for RehearsalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for RehearsalError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Pass,
    Limited,
    Failed,
    NotProven,
}

impl Status {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Limited => "limited",
            Self::Failed => "failed",
            Self::NotProven => "not_proven",
        }
    }

    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Limited => 1,
            Self::NotProven => 2,
            Self::Failed => 3,
        }
    }

    pub(crate) fn worse(self, other: Self) -> Self {
        if other.rank() > self.rank() { other } else { self }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TreeStatus {
    Clean,
    Dirty,
    NotProven,
}

impl TreeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Availability {
    Available,
    Unavailable,
    NotProven,
}

impl Availability {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Completed,
    Cancelled,
    TimedOut,
    Skipped,
    Malformed,
    Stale,
}

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
            Self::Malformed => "malformed",
            Self::Stale => "stale",
        }
    }

    pub(crate) fn is_terminal_success_shape(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PathRole {
    StagedInstall,
    VsixBundledServer,
    ArchiveExtracted,
    WorkspaceTarget,
    Unmanaged,
}

impl PathRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::StagedInstall => "staged_install",
            Self::VsixBundledServer => "vsix_bundled_server",
            Self::ArchiveExtracted => "archive_extracted",
            Self::WorkspaceTarget => "workspace_target",
            Self::Unmanaged => "unmanaged",
        }
    }

    pub(crate) fn is_rehearsal_install(self) -> bool {
        matches!(self, Self::StagedInstall | Self::VsixBundledServer | Self::ArchiveExtracted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CleanupDisposition {
    Cleaned,
    Failed,
    Skipped,
    NotProven,
}

impl CleanupDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cleaned => "cleaned",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MutationKind {
    Tag,
    Release,
    Registry,
    Marketplace,
    Container,
    Secret,
    RemoteBranch,
}

impl MutationKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Release => "release",
            Self::Registry => "registry",
            Self::Marketplace => "marketplace",
            Self::Container => "container",
            Self::Secret => "secret",
            Self::RemoteBranch => "remote_branch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceKind {
    SourceRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactKind {
    ArtifactBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstalledKind {
    InstalledSubject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstrumentKind {
    Instrument,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lockfiles {
    pub(crate) cargo_lock: String,
    pub(crate) npm_lock: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Repository {
    pub(crate) head_sha: String,
    pub(crate) kind: SourceKind,
    pub(crate) tree_status: TreeStatus,
    pub(crate) lockfiles: Lockfiles,
    pub(crate) toolchains: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Instrument {
    pub(crate) role: String,
    pub(crate) identity: String,
    pub(crate) kind: InstrumentKind,
    pub(crate) availability: Availability,
    pub(crate) outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exit_code: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveMember {
    pub(crate) name: String,
    pub(crate) digest: String,
    pub(crate) size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checksum: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) kind: ArtifactKind,
    pub(crate) digest: String,
    pub(crate) size: u64,
    pub(crate) candidate_id: String,
    pub(crate) source_sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) members: Option<Vec<ArchiveMember>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) declared_server_identity: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledSubject {
    pub(crate) role: String,
    pub(crate) kind: InstalledKind,
    pub(crate) identity: String,
    pub(crate) digest: String,
    pub(crate) path_role: PathRole,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stage {
    pub(crate) id: String,
    pub(crate) mandatory: bool,
    pub(crate) status: Status,
    pub(crate) instrument: Instrument,
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) installed: Vec<InstalledSubject>,
    pub(crate) limitations: Vec<String>,
    pub(crate) owning_issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cleanup {
    pub(crate) disposition: CleanupDisposition,
    pub(crate) status: Status,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mutation {
    pub(crate) kind: MutationKind,
    pub(crate) attempted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RehearsalReceipt {
    pub(crate) schema_version: String,
    pub(crate) repository: Repository,
    pub(crate) candidate_id: String,
    pub(crate) package_version: String,
    pub(crate) extension_version: String,
    pub(crate) stages: Vec<Stage>,
    pub(crate) cleanup: Cleanup,
    pub(crate) mutations: Vec<Mutation>,
    pub(crate) limitations: Vec<String>,
    pub(crate) owning_issues: Vec<String>,
    pub(crate) published_channels: Vec<String>,
    pub(crate) release_cut: bool,
    pub(crate) status: Status,
    pub(crate) receipt_digest: String,
}

impl RehearsalReceipt {
    pub(crate) fn seal(mut self) -> Result<Self, RehearsalError> {
        self.receipt_digest = String::new();
        self.receipt_digest = compute_receipt_digest(&self)?;
        Ok(self)
    }
}

pub(crate) fn compute_receipt_digest(receipt: &RehearsalReceipt) -> Result<String, RehearsalError> {
    let mut value = serde_json::to_value(receipt)
        .map_err(|error| RehearsalError::new(ReasonCode::MalformedDocument, error.to_string()))?;
    let Value::Object(ref mut map) = value else {
        return Err(RehearsalError::new(
            ReasonCode::MalformedDocument,
            "receipt must serialize to an object",
        ));
    };
    map.remove("receipt_digest");
    let canonical = canonical_json(&value)?;
    Ok(domain_digest(DIGEST_DOMAIN, canonical.as_bytes()))
}

pub(crate) fn canonical_json(value: &Value) -> Result<String, RehearsalError> {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut members = Vec::with_capacity(keys.len());
            for key in keys {
                let encoded_key = serde_json::to_string(key).map_err(|error| {
                    RehearsalError::new(ReasonCode::MalformedDocument, error.to_string())
                })?;
                let Some(item) = map.get(key) else {
                    continue;
                };
                members.push(format!("{encoded_key}:{}", canonical_json(item)?));
            }
            Ok(format!("{{{}}}", members.join(",")))
        }
        Value::Array(items) => {
            let mut encoded = Vec::with_capacity(items.len());
            for item in items {
                encoded.push(canonical_json(item)?);
            }
            Ok(format!("[{}]", encoded.join(",")))
        }
        other => serde_json::to_string(other)
            .map_err(|error| RehearsalError::new(ReasonCode::MalformedDocument, error.to_string())),
    }
}

fn domain_digest(domain: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(payload);
    hex_lower(&hasher.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let high = usize::from(byte >> 4);
        let low = usize::from(byte & 0x0f);
        if let (Some(high), Some(low)) = (HEX.get(high), HEX.get(low)) {
            out.push(char::from(*high));
            out.push(char::from(*low));
        }
    }
    out
}

pub(crate) fn project_json(receipt: &RehearsalReceipt) -> Result<String, RehearsalError> {
    serde_json::to_string_pretty(receipt)
        .map_err(|error| RehearsalError::new(ReasonCode::MalformedDocument, error.to_string()))
}

pub(crate) fn project_human(receipt: &RehearsalReceipt) -> String {
    let mut out = String::new();
    out.push_str("# readiness_rehearsal.v1\n\n");
    out.push_str(&format!("- status: {}\n", receipt.status.as_str()));
    out.push_str(&format!("- candidate_id: {}\n", receipt.candidate_id));
    out.push_str(&format!("- head_sha: {}\n", receipt.repository.head_sha));
    out.push_str(&format!("- tree_status: {}\n", receipt.repository.tree_status.as_str()));
    out.push_str(&format!("- package_version: {}\n", receipt.package_version));
    out.push_str(&format!("- extension_version: {}\n", receipt.extension_version));
    out.push_str(&format!("- cargo_lock: {}\n", receipt.repository.lockfiles.cargo_lock));
    out.push_str(&format!("- npm_lock: {}\n", receipt.repository.lockfiles.npm_lock));
    let tools: Vec<String> = receipt
        .repository
        .toolchains
        .iter()
        .map(|(name, version)| format!("{name}={version}"))
        .collect();
    out.push_str(&format!("- toolchains: {}\n", tools.join(", ")));
    out.push_str("- published_channels: (none)\n");
    out.push_str("- release_cut: false\n");
    out.push_str(&format!(
        "- cleanup: {} / {}\n",
        receipt.cleanup.disposition.as_str(),
        receipt.cleanup.status.as_str()
    ));
    if receipt.mutations.is_empty() {
        out.push_str("- mutations: (none)\n");
    } else {
        let kinds: Vec<&str> = receipt.mutations.iter().map(|item| item.kind.as_str()).collect();
        out.push_str(&format!("- mutations: {}\n", kinds.join(", ")));
    }
    out.push_str(&format!("- receipt_digest: {}\n", receipt.receipt_digest));
    if receipt.limitations.is_empty() {
        out.push_str("- limitations: (none)\n");
    } else {
        out.push_str("- limitations:\n");
        for note in &receipt.limitations {
            out.push_str(&format!("  - {note}\n"));
        }
    }
    out.push_str(&format!("- owning_issues: {}\n", receipt.owning_issues.join(", ")));
    out.push_str("\n## Stages\n\n");
    for stage in &receipt.stages {
        let mandatory = if stage.mandatory { "mandatory" } else { "optional" };
        out.push_str(&format!(
            "- {}: {} ({mandatory}; instrument={} kind=instrument availability={} outcome={})\n",
            stage.id,
            stage.status.as_str(),
            stage.instrument.identity,
            stage.instrument.availability.as_str(),
            stage.instrument.outcome.as_str()
        ));
        for artifact in &stage.artifacts {
            out.push_str(&format!(
                "  - artifact {} role={} kind=artifact_bytes digest={} size={} source_sha={}\n",
                artifact.name, artifact.role, artifact.digest, artifact.size, artifact.source_sha
            ));
            if let Some(members) = &artifact.members {
                for member in members {
                    out.push_str(&format!(
                        "    - member {} digest={} size={}\n",
                        member.name, member.digest, member.size
                    ));
                }
            }
            if let Some(declared) = &artifact.declared_server_identity {
                out.push_str(&format!("    - declared_server_identity: {declared}\n"));
            }
        }
        for installed in &stage.installed {
            out.push_str(&format!(
                "  - installed {} kind=installed_subject identity={} digest={} path_role={}\n",
                installed.role,
                installed.identity,
                installed.digest,
                installed.path_role.as_str()
            ));
        }
    }
    out
}
