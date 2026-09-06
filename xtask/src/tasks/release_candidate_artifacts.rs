//! Freeze and verify `release_candidate_artifacts.v1` (#9092).
//!
//! This module owns the no-publish candidate-artifact handoff:
//! build-once bytes are hashed after packaging, bound to one release-repo SHA
//! and topology digest, and later retrieved/verified without rebuilding.
//! Transport identity (`artifact_set_id`) is not semantic authority. Public
//! channels are never mutated here; `publish_authorized` is always false.

use crate::utils::project_root;
use chrono::{DateTime, Utc};
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const SCHEMA_PATH: &str = "schemas/release_candidate_artifacts.v1.schema.json";
const SCHEMA_VERSION: &str = "release_candidate_artifacts.v1";
const TOPOLOGY_FIXTURE: &str = "fixtures/release_candidate_artifacts/topology.json";
const PACKET_DIGEST_DOMAIN: &[u8] = b"perl_lsp.release_candidate_artifacts.v1.packet\n";
const CHECKSUMS_NAME: &str = "SHA256SUMS";

/// CLI configuration for `cargo xtask release freeze-candidate-artifacts`.
pub struct FreezeConfig {
    pub staging: PathBuf,
    pub topology: PathBuf,
    pub output: PathBuf,
    pub candidate_id: String,
    pub producer_workflow: String,
    pub producer_run_id: String,
    pub producer_attempt: u32,
    pub artifact_set_id: String,
    pub cargo_lock: PathBuf,
    pub npm_lock: PathBuf,
    pub toolchains: BTreeMap<String, String>,
    pub transport_kind: TransportKind,
    pub available_until: Option<String>,
}

/// CLI configuration for `cargo xtask release verify-candidate-artifacts`.
pub struct VerifyConfig {
    pub packet: PathBuf,
    pub staging: PathBuf,
    pub receipt: Option<PathBuf>,
    pub artifact_set_id: Option<String>,
    pub producer_run_id: Option<String>,
    pub now: Option<DateTime<Utc>>,
    pub rebuild_attempt: bool,
    /// Optional topology document. When supplied, its bytes must match the
    /// frozen `release_topology_digest`.
    pub topology: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    StagingDirectory,
    GithubActionsArtifact,
}

impl std::str::FromStr for TransportKind {
    type Err = color_eyre::eyre::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "staging_directory" => Ok(Self::StagingDirectory),
            "github_actions_artifact" => Ok(Self::GithubActionsArtifact),
            other => bail!("unsupported transport kind {other:?}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    MissingTopologyArchive,
    ExtraPublishableArtifact,
    DigestMismatch,
    CrossRunSubstitution,
    TopologyDigestMismatch,
    VersionMetadataMismatch,
    TransportUnavailable,
    TransportExpired,
    TransportIncomplete,
    RebuildForbidden,
    PacketDigestMismatch,
    DuplicateArtifactName,
    PublishAuthorized,
    PublishedChannelsNonEmpty,
    SchemaViolation,
    MalformedDocument,
}

impl ReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingTopologyArchive => "missing_topology_archive",
            Self::ExtraPublishableArtifact => "extra_publishable_artifact",
            Self::DigestMismatch => "digest_mismatch",
            Self::CrossRunSubstitution => "cross_run_substitution",
            Self::TopologyDigestMismatch => "topology_digest_mismatch",
            Self::VersionMetadataMismatch => "version_metadata_mismatch",
            Self::TransportUnavailable => "transport_unavailable",
            Self::TransportExpired => "transport_expired",
            Self::TransportIncomplete => "transport_incomplete",
            Self::RebuildForbidden => "rebuild_forbidden",
            Self::PacketDigestMismatch => "packet_digest_mismatch",
            Self::DuplicateArtifactName => "duplicate_artifact_name",
            Self::PublishAuthorized => "publish_authorized",
            Self::PublishedChannelsNonEmpty => "published_channels_non_empty",
            Self::SchemaViolation => "schema_violation",
            Self::MalformedDocument => "malformed_document",
        }
    }
}

#[derive(Debug)]
struct HandoffError {
    code: ReasonCode,
    message: String,
}

impl HandoffError {
    fn new(code: ReasonCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for HandoffError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProducerRun {
    workflow: String,
    run_id: String,
    attempt: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PacketInputs {
    cargo_lock: String,
    npm_lock: String,
    toolchains: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenArtifact {
    role: ArtifactRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    name: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactRole {
    ReleaseArchive,
    Vsix,
    Checksums,
    Sbom,
}

impl ArtifactRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReleaseArchive => "release_archive",
            Self::Vsix => "vsix",
            Self::Checksums => "checksums",
            Self::Sbom => "sbom",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrateSubject {
    name: String,
    version: String,
    package_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    kind: TransportKind,
    artifact_set_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    available_until: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateArtifactPacket {
    schema_version: String,
    release: String,
    candidate_id: String,
    release_repo_sha: String,
    release_topology_digest: String,
    producer_run: ProducerRun,
    inputs: PacketInputs,
    artifacts: Vec<FrozenArtifact>,
    crate_subjects: Vec<CrateSubject>,
    attestation_subjects: Vec<String>,
    transport: Transport,
    published_channels: Vec<String>,
    publish_authorized: bool,
    packet_digest: String,
}

#[derive(Clone, Debug)]
struct TopologyMembership {
    release: String,
    frozen_product_sha: String,
    digest: String,
    archives: Vec<(String, String)>,
    vsix_asset_name: String,
    crate_subjects: Vec<CrateSubject>,
}

#[derive(Clone, Debug, Serialize)]
struct VerificationReceipt {
    schema_version: &'static str,
    result: &'static str,
    packet_digest: String,
    artifact_set_id: String,
    release_repo_sha: String,
    release_topology_digest: String,
    members: Vec<VerifiedMember>,
    published_channels: Vec<String>,
    publish_authorized: bool,
    rebuild: bool,
}

#[derive(Clone, Debug, Serialize)]
struct VerifiedMember {
    role: String,
    name: String,
    path: String,
    sha256: String,
    size: u64,
}

pub fn freeze(config: FreezeConfig) -> Result<()> {
    let packet = freeze_packet(&config)?;
    if let Some(parent) = config.output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&packet)
        .context("serializing release_candidate_artifacts.v1 packet")?;
    fs::write(&config.output, encoded)
        .with_context(|| format!("writing {}", config.output.display()))?;
    println!(
        "froze {} artifacts to {} (packet_digest={}, artifact_set_id={}, publish_authorized=false)",
        packet.artifacts.len(),
        config.output.display(),
        packet.packet_digest,
        packet.transport.artifact_set_id
    );
    Ok(())
}

pub fn verify(config: VerifyConfig) -> Result<()> {
    let receipt = verify_packet(&config)?;
    if let Some(path) = &config.receipt {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let encoded =
            serde_json::to_vec_pretty(&receipt).context("serializing verification receipt")?;
        fs::write(path, encoded).with_context(|| format!("writing {}", path.display()))?;
    }
    println!(
        "verified {} members from packet_digest={} artifact_set_id={} (rebuild=false, published_channels=[])",
        receipt.members.len(),
        receipt.packet_digest,
        receipt.artifact_set_id
    );
    Ok(())
}

/// Run the closed freeze/verify contract against the committed topology fixture.
pub fn check() -> Result<()> {
    let root = project_root()?;
    validate_schema_file(&root)?;
    let topology = root.join(TOPOLOGY_FIXTURE);
    if !topology.is_file() {
        bail!("missing topology fixture {}", topology.display());
    }

    let mut failures: Vec<String> = Vec::new();
    push_failure(&mut failures, "happy_path", happy_path_check(&topology));
    push_failure(&mut failures, "missing_archive", missing_archive_check(&topology));
    push_failure(&mut failures, "extra_vsix", extra_vsix_check(&topology));
    push_failure(&mut failures, "digest_mismatch", digest_mismatch_check(&topology));
    push_failure(&mut failures, "cross_run", cross_run_check(&topology));
    push_failure(&mut failures, "topology_digest", topology_digest_check(&topology));
    push_failure(&mut failures, "version_metadata", version_metadata_check(&topology));
    push_failure(&mut failures, "transport_missing", transport_missing_check(&topology));
    push_failure(&mut failures, "transport_incomplete", transport_incomplete_check(&topology));
    push_failure(&mut failures, "transport_expired", transport_expired_check(&topology));
    push_failure(&mut failures, "rebuild_forbidden", rebuild_forbidden_check(&topology));
    push_failure(&mut failures, "packet_field_edit", packet_field_edit_check(&topology));
    push_failure(&mut failures, "determinism", determinism_check(&topology));

    if failures.is_empty() {
        println!(
            "release_candidate_artifacts.v1: freeze, retrieval, verification, and negative controls all valid (publish_authorized=false)"
        );
        Ok(())
    } else {
        bail!("release_candidate_artifacts.v1 check failed:\n{}", failures.join("\n"))
    }
}

fn freeze_packet(config: &FreezeConfig) -> Result<CandidateArtifactPacket> {
    if config.producer_attempt < 1 {
        return Err(HandoffError::new(
            ReasonCode::MalformedDocument,
            "producer_run.attempt must be >= 1",
        )
        .into());
    }
    if config.toolchains.is_empty() {
        return Err(HandoffError::new(
            ReasonCode::MalformedDocument,
            "inputs.toolchains must name at least one toolchain",
        )
        .into());
    }
    let topology = load_topology(&config.topology)?;
    let staging_files = scan_staging(&config.staging)?;
    let artifacts = bind_artifacts(&topology, &staging_files, &config.staging)?;
    let packet = CandidateArtifactPacket {
        schema_version: SCHEMA_VERSION.to_string(),
        release: topology.release.clone(),
        candidate_id: config.candidate_id.clone(),
        release_repo_sha: topology.frozen_product_sha.clone(),
        release_topology_digest: topology.digest.clone(),
        producer_run: ProducerRun {
            workflow: config.producer_workflow.clone(),
            run_id: config.producer_run_id.clone(),
            attempt: config.producer_attempt,
        },
        inputs: PacketInputs {
            cargo_lock: sha256_file(&config.cargo_lock)?,
            npm_lock: sha256_file(&config.npm_lock)?,
            toolchains: config.toolchains.clone(),
        },
        attestation_subjects: artifacts.iter().map(|artifact| artifact.name.clone()).collect(),
        artifacts,
        crate_subjects: topology.crate_subjects,
        transport: Transport {
            kind: config.transport_kind,
            artifact_set_id: config.artifact_set_id.clone(),
            available_until: config.available_until.clone(),
        },
        published_channels: Vec::new(),
        publish_authorized: false,
        packet_digest: String::new(),
    };
    let mut packet = packet;
    packet.packet_digest = compute_packet_digest(&packet)?;
    validate_packet_schema(&packet)?;
    Ok(packet)
}

fn verify_packet(config: &VerifyConfig) -> Result<VerificationReceipt> {
    if config.rebuild_attempt {
        return Err(HandoffError::new(
            ReasonCode::RebuildForbidden,
            "publisher input cannot be satisfied by rebuilding locally; retrieve the frozen set or regenerate a new candidate",
        )
        .into());
    }
    if !config.staging.is_dir() {
        return Err(HandoffError::new(
            ReasonCode::TransportUnavailable,
            format!(
                "frozen artifact set is unavailable at {}; regenerate and re-approve the candidate rather than rebuilding",
                config.staging.display()
            ),
        )
        .into());
    }
    let bytes = fs::read(&config.packet)
        .with_context(|| format!("reading packet {}", config.packet.display()))?;
    let packet: CandidateArtifactPacket = serde_json::from_slice(&bytes)
        .map_err(|error| HandoffError::new(ReasonCode::MalformedDocument, error.to_string()))?;
    validate_packet_schema(&packet)?;
    let expected_digest = compute_packet_digest(&packet)?;
    if packet.packet_digest != expected_digest {
        return Err(HandoffError::new(
            ReasonCode::PacketDigestMismatch,
            "packet fields were edited without regenerating packet_digest",
        )
        .into());
    }
    if let Some(topology_path) = &config.topology {
        let topology = load_topology(topology_path)?;
        if topology.digest != packet.release_topology_digest {
            return Err(HandoffError::new(
                ReasonCode::TopologyDigestMismatch,
                "supplied topology bytes do not match the frozen release_topology_digest; version identity is not sufficient",
            )
            .into());
        }
        if topology.frozen_product_sha != packet.release_repo_sha {
            return Err(HandoffError::new(
                ReasonCode::TopologyDigestMismatch,
                "supplied topology frozen_product_sha does not match the frozen release-repo SHA",
            )
            .into());
        }
    }
    if packet.publish_authorized {
        return Err(HandoffError::new(
            ReasonCode::PublishAuthorized,
            "release_candidate_artifacts.v1 never authorizes publication",
        )
        .into());
    }
    if !packet.published_channels.is_empty() {
        return Err(HandoffError::new(
            ReasonCode::PublishedChannelsNonEmpty,
            "published_channels must remain empty on a no-publish packet",
        )
        .into());
    }
    if let Some(expected) = &config.artifact_set_id
        && expected != &packet.transport.artifact_set_id
    {
        return Err(HandoffError::new(
            ReasonCode::CrossRunSubstitution,
            format!(
                "artifact_set_id {expected} does not match frozen set {}",
                packet.transport.artifact_set_id
            ),
        )
        .into());
    }
    if let Some(expected) = &config.producer_run_id
        && expected != &packet.producer_run.run_id
    {
        return Err(HandoffError::new(
            ReasonCode::CrossRunSubstitution,
            format!("producer run {expected} is not frozen run {}", packet.producer_run.run_id),
        )
        .into());
    }
    if let Some(until) = &packet.transport.available_until {
        let expiry = DateTime::parse_from_rfc3339(until)
            .map_err(|error| {
                HandoffError::new(
                    ReasonCode::MalformedDocument,
                    format!("transport.available_until is not RFC3339: {error}"),
                )
            })?
            .with_timezone(&Utc);
        let now = config.now.unwrap_or_else(Utc::now);
        if now >= expiry {
            return Err(HandoffError::new(
                ReasonCode::TransportExpired,
                "transport expired; regenerate a new candidate packet and obtain new review rather than rebuilding under the old approval",
            )
            .into());
        }
    }

    let staging_files = scan_staging(&config.staging)?;
    let mut members = Vec::new();
    let mut seen = BTreeSet::new();
    for artifact in &packet.artifacts {
        if !seen.insert(artifact.name.as_str()) {
            return Err(HandoffError::new(
                ReasonCode::DuplicateArtifactName,
                format!("duplicate frozen name {}", artifact.name),
            )
            .into());
        }
        let Some(path) = staging_files.get(&artifact.name) else {
            return Err(HandoffError::new(
                ReasonCode::TransportIncomplete,
                format!(
                    "frozen member {} is missing from retrieved set; regenerate the candidate rather than rebuilding",
                    artifact.name
                ),
            )
            .into());
        };
        let (digest, size) = digest_and_size(path)?;
        if digest != artifact.sha256 || size != artifact.size {
            return Err(HandoffError::new(
                ReasonCode::DigestMismatch,
                format!(
                    "{} name matches but retrieved bytes differ (declared {}/{} observed {digest}/{size})",
                    artifact.name, artifact.sha256, artifact.size
                ),
            )
            .into());
        }
        members.push(VerifiedMember {
            role: artifact.role.as_str().to_string(),
            name: artifact.name.clone(),
            path: artifact.name.clone(),
            sha256: digest,
            size,
        });
    }
    for name in staging_files.keys() {
        if classify_publishable(name).is_some() && !seen.contains(name.as_str()) {
            return Err(HandoffError::new(
                ReasonCode::ExtraPublishableArtifact,
                format!("{name} is present in retrieval but was not frozen"),
            )
            .into());
        }
    }
    Ok(VerificationReceipt {
        schema_version: "release_candidate_artifacts_verification.v1",
        result: "verified",
        packet_digest: packet.packet_digest,
        artifact_set_id: packet.transport.artifact_set_id,
        release_repo_sha: packet.release_repo_sha,
        release_topology_digest: packet.release_topology_digest,
        members,
        published_channels: Vec::new(),
        publish_authorized: false,
        rebuild: false,
    })
}

fn load_topology(path: &Path) -> Result<TopologyMembership> {
    let bytes = fs::read(path).with_context(|| format!("reading topology {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing topology {}", path.display()))?;
    let release = required_string(&value, "release")?;
    let frozen_product_sha = required_string(&value, "frozen_product_sha")?;
    if !is_git_sha(&frozen_product_sha) {
        return Err(HandoffError::new(
            ReasonCode::MalformedDocument,
            "frozen_product_sha must be a 40-character lowercase git SHA",
        )
        .into());
    }
    let vsix = value.get("vsix").ok_or_else(|| {
        HandoffError::new(ReasonCode::MalformedDocument, "topology vsix is required")
    })?;
    let vsix_asset_name = required_string(vsix, "asset_name")?;
    let mut archives = Vec::new();
    let targets = value.get("binary_targets").and_then(Value::as_array).ok_or_else(|| {
        HandoffError::new(ReasonCode::MalformedDocument, "topology binary_targets is required")
    })?;
    for (index, target) in targets.iter().enumerate() {
        let triple = required_string(target, "target")?;
        let archive_name = required_string(target, "archive_name")?;
        if archives.iter().any(|(name, _)| name == &archive_name) {
            return Err(HandoffError::new(
                ReasonCode::DuplicateArtifactName,
                format!("topology archive name {archive_name} is duplicated"),
            )
            .into());
        }
        if !archive_name.contains(&release) {
            return Err(HandoffError::new(
                ReasonCode::VersionMetadataMismatch,
                format!("binary_targets[{index}].archive_name does not contain release {release}"),
            )
            .into());
        }
        archives.push((archive_name, triple));
    }
    if archives.is_empty() {
        return Err(HandoffError::new(
            ReasonCode::MalformedDocument,
            "topology declares no binary_targets",
        )
        .into());
    }
    if !vsix_asset_name.contains(&release) {
        return Err(HandoffError::new(
            ReasonCode::VersionMetadataMismatch,
            format!("vsix asset_name does not contain release {release}"),
        )
        .into());
    }
    let crate_subjects = match value.get("published_crates") {
        Some(Value::Array(entries)) => {
            let mut subjects = Vec::new();
            for entry in entries {
                subjects.push(CrateSubject {
                    name: required_string(entry, "name")?,
                    version: required_string(entry, "version")?,
                    package_path: required_string(entry, "package_path")?,
                });
            }
            subjects
        }
        None => Vec::new(),
        Some(_) => {
            return Err(HandoffError::new(
                ReasonCode::MalformedDocument,
                "published_crates must be an array",
            )
            .into());
        }
    };
    Ok(TopologyMembership {
        release,
        frozen_product_sha,
        digest: sha256_bytes(&bytes),
        archives,
        vsix_asset_name,
        crate_subjects,
    })
}

fn scan_staging(staging: &Path) -> Result<BTreeMap<String, PathBuf>> {
    if !staging.is_dir() {
        return Err(HandoffError::new(
            ReasonCode::TransportUnavailable,
            format!("staging directory does not exist: {}", staging.display()),
        )
        .into());
    }
    let mut files = BTreeMap::new();
    let entries =
        fs::read_dir(staging).with_context(|| format!("reading staging {}", staging.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", staging.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if files.insert(name.clone(), path).is_some() {
            return Err(HandoffError::new(
                ReasonCode::DuplicateArtifactName,
                format!("duplicate staging name {name}"),
            )
            .into());
        }
    }
    Ok(files)
}

fn bind_artifacts(
    topology: &TopologyMembership,
    staging: &BTreeMap<String, PathBuf>,
    staging_dir: &Path,
) -> Result<Vec<FrozenArtifact>> {
    let mut artifacts = Vec::new();
    let mut claimed = BTreeSet::new();

    for (name, target) in &topology.archives {
        let path = require_file(staging, name, ReasonCode::MissingTopologyArchive)?;
        let (digest, size) = digest_and_size(path)?;
        reject_version_mismatch(name, &topology.release, Some(target))?;
        artifacts.push(FrozenArtifact {
            role: ArtifactRole::ReleaseArchive,
            target: Some(target.clone()),
            name: name.clone(),
            sha256: digest,
            size,
        });
        claimed.insert(name.clone());
    }

    let vsix_path =
        require_file(staging, &topology.vsix_asset_name, ReasonCode::MissingTopologyArchive)?;
    reject_version_mismatch(&topology.vsix_asset_name, &topology.release, None)?;
    let (digest, size) = digest_and_size(vsix_path)?;
    artifacts.push(FrozenArtifact {
        role: ArtifactRole::Vsix,
        target: None,
        name: topology.vsix_asset_name.clone(),
        sha256: digest,
        size,
    });
    claimed.insert(topology.vsix_asset_name.clone());

    let checksums = require_file(staging, CHECKSUMS_NAME, ReasonCode::TransportIncomplete)?;
    let (digest, size) = digest_and_size(checksums)?;
    artifacts.push(FrozenArtifact {
        role: ArtifactRole::Checksums,
        target: None,
        name: CHECKSUMS_NAME.to_string(),
        sha256: digest,
        size,
    });
    claimed.insert(CHECKSUMS_NAME.to_string());

    let sbom_name =
        staging.keys().find(|name| classify_publishable(name) == Some(ArtifactRole::Sbom)).cloned();
    if let Some(name) = sbom_name {
        let path = require_file(staging, &name, ReasonCode::TransportIncomplete)?;
        let (digest, size) = digest_and_size(path)?;
        artifacts.push(FrozenArtifact {
            role: ArtifactRole::Sbom,
            target: None,
            name: name.clone(),
            sha256: digest,
            size,
        });
        claimed.insert(name);
    } else {
        return Err(HandoffError::new(
            ReasonCode::TransportIncomplete,
            format!("SBOM is missing from {}", staging_dir.display()),
        )
        .into());
    }

    for name in staging.keys() {
        if claimed.contains(name) {
            continue;
        }
        if classify_publishable(name).is_some() {
            return Err(HandoffError::new(
                ReasonCode::ExtraPublishableArtifact,
                format!("{name} is not a topology-declared publication subject"),
            )
            .into());
        }
    }
    Ok(artifacts)
}

fn require_file<'a>(
    staging: &'a BTreeMap<String, PathBuf>,
    name: &str,
    code: ReasonCode,
) -> Result<&'a PathBuf> {
    staging.get(name).ok_or_else(|| {
        HandoffError::new(code, format!("{name} is missing from the frozen artifact set")).into()
    })
}

fn classify_publishable(name: &str) -> Option<ArtifactRole> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".zip") {
        Some(ArtifactRole::ReleaseArchive)
    } else if lower.ends_with(".vsix") {
        Some(ArtifactRole::Vsix)
    } else if name == CHECKSUMS_NAME {
        Some(ArtifactRole::Checksums)
    } else if lower.contains("sbom")
        || lower.ends_with(".spdx.json")
        || lower.ends_with(".cdx.json")
    {
        Some(ArtifactRole::Sbom)
    } else {
        None
    }
}

fn reject_version_mismatch(name: &str, release: &str, target: Option<&str>) -> Result<()> {
    if !name.contains(release) {
        return Err(HandoffError::new(
            ReasonCode::VersionMetadataMismatch,
            format!("{name} does not report release {release}"),
        )
        .into());
    }
    if let Some(target) = target
        && !name.contains(target)
    {
        return Err(HandoffError::new(
            ReasonCode::VersionMetadataMismatch,
            format!("{name} does not report target {target}"),
        )
        .into());
    }
    Ok(())
}

fn compute_packet_digest(packet: &CandidateArtifactPacket) -> Result<String> {
    let mut value = serde_json::to_value(packet).context("canonicalizing packet")?;
    let Value::Object(ref mut map) = value else {
        return Err(HandoffError::new(
            ReasonCode::MalformedDocument,
            "packet must serialize to an object",
        )
        .into());
    };
    map.remove("packet_digest");
    let canonical = canonical_json(&value)?;
    Ok(domain_digest(PACKET_DIGEST_DOMAIN, canonical.as_bytes()))
}

fn validate_packet_schema(packet: &CandidateArtifactPacket) -> Result<()> {
    let root = project_root()?;
    let schema = load_schema(&root)?;
    let value = serde_json::to_value(packet).context("encoding packet for schema validation")?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| HandoffError::new(ReasonCode::SchemaViolation, error.to_string()))?;
    let violations: Vec<String> =
        validator.iter_errors(&value).map(|error| error.to_string()).collect();
    if !violations.is_empty() {
        return Err(HandoffError::new(ReasonCode::SchemaViolation, violations.join("; ")).into());
    }
    Ok(())
}

fn validate_schema_file(root: &Path) -> Result<()> {
    let schema = load_schema(root)?;
    jsonschema::validator_for(&schema)
        .map_err(|error| eyre!("{SCHEMA_PATH}: invalid schema: {error}"))?;
    Ok(())
}

fn load_schema(root: &Path) -> Result<Value> {
    let path = root.join(SCHEMA_PATH);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn canonical_json(value: &Value) -> Result<String> {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut members = Vec::with_capacity(keys.len());
            for key in keys {
                let encoded_key = serde_json::to_string(key).context("encoding object key")?;
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
        other => serde_json::to_string(other).context("encoding canonical JSON atom"),
    }
}

fn domain_digest(domain: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(payload);
    hex_lower(&hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String> {
    Ok(digest_and_size(path)?.0)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn digest_and_size(path: &Path) -> Result<(String, u64)> {
    let mut file =
        File::open(path).with_context(|| format!("opening {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buf).with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok((hex_lower(&hasher.finalize()), size))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
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

fn required_string(value: &Value, key: &str) -> Result<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| {
        HandoffError::new(ReasonCode::MalformedDocument, format!("{key} must be a string")).into()
    })
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn push_failure(failures: &mut Vec<String>, name: &str, result: Result<()>) {
    if let Err(error) = result {
        failures.push(format!("{name}: {error:#}"));
    }
}

fn reason_of(error: &color_eyre::eyre::Report) -> Option<ReasonCode> {
    error.downcast_ref::<HandoffError>().map(|error| error.code)
}

fn expect_reason(result: Result<CandidateArtifactPacket>, code: ReasonCode) -> Result<()> {
    match result {
        Ok(_) => bail!("expected failure {}", code.as_str()),
        Err(error) => {
            if reason_of(&error) == Some(code) {
                Ok(())
            } else {
                bail!("expected {}, got {error:#}", code.as_str())
            }
        }
    }
}

fn expect_verify_reason(result: Result<VerificationReceipt>, code: ReasonCode) -> Result<()> {
    match result {
        Ok(_) => bail!("expected verification failure {}", code.as_str()),
        Err(error) => {
            if reason_of(&error) == Some(code) {
                Ok(())
            } else {
                bail!("expected {}, got {error:#}", code.as_str())
            }
        }
    }
}

struct ScenarioDirs {
    _root: tempfile::TempDir,
    staging: PathBuf,
    topology: PathBuf,
    cargo_lock: PathBuf,
    npm_lock: PathBuf,
    packet: PathBuf,
}

fn scenario_dirs(topology_src: &Path) -> Result<ScenarioDirs> {
    let root = tempfile::TempDir::new().context("creating scenario tempdir")?;
    let staging = root.path().join("staging");
    fs::create_dir_all(&staging)?;
    let topology = root.path().join("topology.json");
    fs::copy(topology_src, &topology)?;
    let cargo_lock = root.path().join("Cargo.lock");
    let npm_lock = root.path().join("package-lock.json");
    fs::write(&cargo_lock, b"cargo-lock-bytes\n")?;
    fs::write(&npm_lock, b"npm-lock-bytes\n")?;
    write_complete_set(&staging, topology_src)?;
    let packet = root.path().join("packet.json");
    Ok(ScenarioDirs { _root: root, staging, topology, cargo_lock, npm_lock, packet })
}

fn write_complete_set(staging: &Path, topology_src: &Path) -> Result<()> {
    let topology = load_topology(topology_src)?;
    for (name, _) in &topology.archives {
        fs::write(staging.join(name), format!("archive:{name}\n").into_bytes())?;
    }
    fs::write(
        staging.join(&topology.vsix_asset_name),
        format!("vsix:{}\n", topology.vsix_asset_name).into_bytes(),
    )?;
    fs::write(staging.join(CHECKSUMS_NAME), b"checksums\n")?;
    fs::write(staging.join("sbom.cdx.json"), b"{\"bomFormat\":\"CycloneDX\"}\n")?;
    Ok(())
}

fn freeze_cfg(dirs: &ScenarioDirs, run_id: &str, set_id: &str) -> FreezeConfig {
    FreezeConfig {
        staging: dirs.staging.clone(),
        topology: dirs.topology.clone(),
        output: dirs.packet.clone(),
        candidate_id: "rc1".to_string(),
        producer_workflow: "no-publish-candidate.yml".to_string(),
        producer_run_id: run_id.to_string(),
        producer_attempt: 1,
        artifact_set_id: set_id.to_string(),
        cargo_lock: dirs.cargo_lock.clone(),
        npm_lock: dirs.npm_lock.clone(),
        toolchains: BTreeMap::from([("rustc".to_string(), "1.95.0".to_string())]),
        transport_kind: TransportKind::StagingDirectory,
        available_until: None,
    }
}

fn verify_cfg(dirs: &ScenarioDirs) -> VerifyConfig {
    VerifyConfig {
        packet: dirs.packet.clone(),
        staging: dirs.staging.clone(),
        receipt: None,
        artifact_set_id: Some("set-rc1".to_string()),
        producer_run_id: Some("run-1".to_string()),
        now: None,
        rebuild_attempt: false,
        topology: Some(dirs.topology.clone()),
    }
}

fn happy_path_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    if packet.publish_authorized || !packet.published_channels.is_empty() {
        bail!("happy path packet mutated a public channel");
    }
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let receipt = verify_packet(&verify_cfg(&dirs))?;
    if receipt.members.len() != packet.artifacts.len() {
        bail!("verification did not expose every frozen member");
    }
    Ok(())
}

fn missing_archive_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    fs::remove_file(dirs.staging.join("perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz"))?;
    expect_reason(
        freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1")),
        ReasonCode::MissingTopologyArchive,
    )
}

fn extra_vsix_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    fs::write(dirs.staging.join("perl-lsp-rs-extra.vsix"), b"extra\n")?;
    expect_reason(
        freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1")),
        ReasonCode::ExtraPublishableArtifact,
    )
}

fn digest_mismatch_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    fs::write(dirs.staging.join("perl-lsp-rs-0.18.0.vsix"), b"tampered-vsix\n")?;
    expect_verify_reason(verify_packet(&verify_cfg(&dirs)), ReasonCode::DigestMismatch)
}

fn cross_run_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let mut cfg = verify_cfg(&dirs);
    cfg.artifact_set_id = Some("set-other-run".to_string());
    expect_verify_reason(verify_packet(&cfg), ReasonCode::CrossRunSubstitution)
}

fn topology_digest_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let original_digest = packet.release_topology_digest.clone();
    let mut alt = serde_json::from_slice::<Value>(&fs::read(&dirs.topology)?)?;
    if let Some(targets) = alt.get_mut("binary_targets").and_then(Value::as_array_mut)
        && let Some(first) = targets.get_mut(0)
    {
        first["archive_name"] =
            Value::String("perllsp-0.18.0-aarch64-unknown-linux-gnu.tar.gz".to_string());
        first["target"] = Value::String("aarch64-unknown-linux-gnu".to_string());
    }
    fs::write(&dirs.topology, serde_json::to_vec_pretty(&alt)?)?;
    expect_verify_reason(verify_packet(&verify_cfg(&dirs)), ReasonCode::TopologyDigestMismatch)?;
    fs::write(
        dirs.staging.join("perllsp-0.18.0-aarch64-unknown-linux-gnu.tar.gz"),
        b"alt-archive\n",
    )?;
    let _ = fs::remove_file(dirs.staging.join("perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz"));
    let alt_packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    if alt_packet.release_topology_digest == original_digest {
        bail!("same-version topology edit did not change topology digest");
    }
    if alt_packet.release != packet.release {
        bail!("topology digest control changed the release version");
    }
    Ok(())
}

fn version_metadata_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    fs::rename(
        dirs.staging.join("perl-lsp-rs-0.18.0.vsix"),
        dirs.staging.join("perl-lsp-rs-0.19.0.vsix"),
    )?;
    let mut topo: Value = serde_json::from_slice(&fs::read(&dirs.topology)?)?;
    if let Some(vsix) = topo.get_mut("vsix") {
        vsix["asset_name"] = Value::String("perl-lsp-rs-0.19.0.vsix".to_string());
    }
    fs::write(&dirs.topology, serde_json::to_vec_pretty(&topo)?)?;
    expect_reason(
        freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1")),
        ReasonCode::VersionMetadataMismatch,
    )
}

fn transport_missing_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let mut cfg = verify_cfg(&dirs);
    cfg.staging = dirs.staging.join("does-not-exist");
    expect_verify_reason(verify_packet(&cfg), ReasonCode::TransportUnavailable)
}

fn transport_incomplete_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    fs::remove_file(dirs.staging.join(CHECKSUMS_NAME))?;
    expect_verify_reason(verify_packet(&verify_cfg(&dirs)), ReasonCode::TransportIncomplete)
}

fn transport_expired_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let mut cfg = freeze_cfg(&dirs, "run-1", "set-rc1");
    cfg.available_until = Some("2020-01-01T00:00:00Z".to_string());
    let packet = freeze_packet(&cfg)?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    expect_verify_reason(verify_packet(&verify_cfg(&dirs)), ReasonCode::TransportExpired)
}

fn rebuild_forbidden_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let mut cfg = verify_cfg(&dirs);
    cfg.rebuild_attempt = true;
    expect_verify_reason(verify_packet(&cfg), ReasonCode::RebuildForbidden)
}

fn packet_field_edit_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let packet = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &packet)?;
    let mut value: Value = serde_json::from_slice(&fs::read(&dirs.packet)?)?;
    value["candidate_id"] = Value::String("rc-tampered".to_string());
    fs::write(&dirs.packet, serde_json::to_vec_pretty(&value)?)?;
    expect_verify_reason(verify_packet(&verify_cfg(&dirs)), ReasonCode::PacketDigestMismatch)
}

fn determinism_check(topology: &Path) -> Result<()> {
    let dirs = scenario_dirs(topology)?;
    let first = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    let second = freeze_packet(&freeze_cfg(&dirs, "run-1", "set-rc1"))?;
    if first.packet_digest != second.packet_digest {
        bail!("freeze of identical bytes produced different packet digests");
    }
    serde_json::to_writer_pretty(File::create(&dirs.packet)?, &first)?;
    verify_packet(&verify_cfg(&dirs)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topology() -> Result<PathBuf> {
        Ok(project_root()?.join(TOPOLOGY_FIXTURE))
    }

    #[test]
    fn schema_compiles() -> Result<()> {
        validate_schema_file(&project_root()?)
    }

    #[test]
    fn freeze_verify_round_trip_does_not_publish() -> Result<()> {
        happy_path_check(&topology()?)
    }

    #[test]
    fn missing_topology_archive_fails() -> Result<()> {
        missing_archive_check(&topology()?)
    }

    #[test]
    fn extra_vsix_fails() -> Result<()> {
        extra_vsix_check(&topology()?)
    }

    #[test]
    fn name_match_digest_mismatch_fails() -> Result<()> {
        digest_mismatch_check(&topology()?)
    }

    #[test]
    fn cross_run_substitution_fails() -> Result<()> {
        cross_run_check(&topology()?)
    }

    #[test]
    fn same_version_topology_edit_changes_digest() -> Result<()> {
        topology_digest_check(&topology()?)
    }

    #[test]
    fn vsix_version_metadata_mismatch_fails() -> Result<()> {
        version_metadata_check(&topology()?)
    }

    #[test]
    fn missing_transport_fails_closed() -> Result<()> {
        transport_missing_check(&topology()?)
    }

    #[test]
    fn incomplete_transport_fails_closed() -> Result<()> {
        transport_incomplete_check(&topology()?)
    }

    #[test]
    fn expired_transport_fails_closed() -> Result<()> {
        transport_expired_check(&topology()?)
    }

    #[test]
    fn local_rebuild_is_rejected() -> Result<()> {
        rebuild_forbidden_check(&topology()?)
    }

    #[test]
    fn edited_packet_without_new_digest_fails() -> Result<()> {
        packet_field_edit_check(&topology()?)
    }

    #[test]
    fn freeze_is_deterministic_for_fixed_inputs() -> Result<()> {
        determinism_check(&topology()?)
    }

    #[test]
    fn canonical_json_sorts_object_keys() -> Result<()> {
        let shuffled = serde_json::json!({"b": 1, "a": 2});
        let ordered = serde_json::json!({"a": 2, "b": 1});
        assert_eq!(canonical_json(&shuffled)?, canonical_json(&ordered)?);
        Ok(())
    }
}
