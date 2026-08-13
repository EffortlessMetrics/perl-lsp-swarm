use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[path = "client_compat_fixture.rs"]
mod client_compat_fixture;

pub use client_compat_fixture::{
    CANONICAL_EXPECTATION_IDS, CANONICAL_EXPECTATION_SET_ID, canonical_expectation_set_digest,
    fixture_digest,
};

pub const SCHEMA_VERSION: &str = "editor_client_compat.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStage {
    ExactSourceLocal,
    ReleaseCandidate,
    PublicArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientSourceState {
    Bundled,
    Released,
    UpstreamSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    GenericLsp,
    NativeEditorExtension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationState {
    Manual,
    UpstreamSource,
    AcceptedUnreleased,
    BuiltinReleased,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionEncodingBasis {
    Offered,
    ProtocolDefault,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticMode {
    Push,
    Pull,
    Both,
    None,
    Malformed,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationResult {
    Pass,
    Fail,
    Partial,
    NotProven,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Product,
    HostClient,
    Fixture,
    Protocol,
    Instrument,
    Environment,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupResult {
    Pass,
    Fail,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ClientLog,
    ServerStderr,
    DriverOutput,
    CapabilitySnapshot,
    ProcessLedger,
    FailureDiagnostics,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformIdentity {
    pub os: String,
    pub os_version: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostIdentity {
    pub client_id: String,
    pub product: String,
    pub version: String,
    pub source_state: ClientSourceState,
    pub source_ref: String,
    pub executable_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationIdentity {
    pub mode: IntegrationMode,
    pub registration_state: RegistrationState,
    pub configuration_sha256: String,
    pub driver_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerIdentity {
    pub executable: String,
    pub version: String,
    pub build_revision: String,
    pub artifact_sha256: String,
    pub protocol_version: String,
    pub launch_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFixtureIdentity {
    pub id: String,
    pub digest: String,
    pub expectation_set_id: String,
    pub expectation_set_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityIdentity {
    pub initialize_snapshot_sha256: String,
    #[serde(default)]
    pub position_encodings_offered: Vec<String>,
    pub position_encoding_basis: PositionEncodingBasis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_encoding_selected: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsIdentity {
    pub advertised_mode: DiagnosticMode,
    #[serde(default)]
    pub observed_messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyCell {
    pub id: String,
    pub result: ObservationResult,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limitation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceArtifact {
    pub kind: ArtifactKind,
    pub id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorClientCompatReceipt {
    pub schema_version: String,
    pub observed_at: String,
    pub stage: EvidenceStage,
    pub repository: String,
    pub candidate_sha: String,
    pub platform: PlatformIdentity,
    pub host: HostIdentity,
    pub integration: IntegrationIdentity,
    pub server: ServerIdentity,
    pub workspace_fixture: WorkspaceFixtureIdentity,
    pub capabilities: CapabilityIdentity,
    pub diagnostics: DiagnosticsIdentity,
    pub journey: Vec<JourneyCell>,
    pub process_cleanup: CleanupResult,
    pub result: ObservationResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<EvidenceArtifact>,
    pub claim_boundary: String,
}

impl EditorClientCompatReceipt {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unexpected schema_version"
        );
        chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .context("observed_at must be an RFC 3339 date-time")?;
        validate_safe_identity(&self.repository, "repository")?;
        ensure!(
            is_lower_hex(&self.candidate_sha, 40),
            "candidate_sha must be 40 lowercase hex chars"
        );

        validate_safe_identity(&self.platform.os, "platform.os")?;
        validate_safe_identity(&self.platform.os_version, "platform.os_version")?;
        validate_safe_identity(&self.platform.arch, "platform.arch")?;

        ensure!(
            is_reason_token(&self.host.client_id),
            "host.client_id must be a stable reason token"
        );
        validate_safe_identity(&self.host.product, "host.product")?;
        validate_safe_identity(&self.host.version, "host.version")?;
        validate_safe_identity(&self.host.source_ref, "host.source_ref")?;
        validate_sha256(
            &self.host.executable_sha256,
            "host.executable_sha256",
        )?;

        validate_sha256(
            &self.integration.configuration_sha256,
            "integration.configuration_sha256",
        )?;
        validate_sha256(
            &self.integration.driver_sha256,
            "integration.driver_sha256",
        )?;

        ensure!(
            self.server.executable == "perllsp",
            "actual editor receipt must bind canonical perllsp executable"
        );
        validate_safe_identity(&self.server.version, "server.version")?;
        ensure!(
            self.server.build_revision == "not_proven"
                || is_lower_hex(&self.server.build_revision, 40),
            "server.build_revision must be 40 lowercase hex chars or not_proven"
        );
        validate_sha256(&self.server.artifact_sha256, "server.artifact_sha256")?;
        validate_safe_identity(
            &self.server.protocol_version,
            "server.protocol_version",
        )?;
        ensure!(
            matches!(
                self.server.launch_command.as_slice(),
                [program, argument] if program == "perllsp" && argument == "--stdio"
            ),
            "actual editor launch command must be exactly perllsp --stdio"
        );

        ensure!(
            is_reason_token(&self.workspace_fixture.id),
            "workspace_fixture.id must be a stable reason token"
        );
        validate_sha256(
            &self.workspace_fixture.digest,
            "workspace_fixture.digest",
        )?;
        ensure!(
            is_reason_token(&self.workspace_fixture.expectation_set_id),
            "workspace_fixture.expectation_set_id must be a stable reason token"
        );
        validate_sha256(
            &self.workspace_fixture.expectation_set_digest,
            "workspace_fixture.expectation_set_digest",
        )?;

        validate_sha256(
            &self.capabilities.initialize_snapshot_sha256,
            "capabilities.initialize_snapshot_sha256",
        )?;
        let mut offered = BTreeSet::new();
        for encoding in &self.capabilities.position_encodings_offered {
            ensure!(
                is_reason_token(encoding),
                "position encoding must be a stable reason token: {encoding}"
            );
            ensure!(
                offered.insert(encoding.as_str()),
                "duplicate offered position encoding: {encoding}"
            );
        }
        match self.capabilities.position_encoding_basis {
            PositionEncodingBasis::Offered => {
                let selected = self
                    .capabilities
                    .position_encoding_selected
                    .as_deref()
                    .context("offered position encoding basis requires a selected value")?;
                ensure!(
                    offered.contains(selected),
                    "selected position encoding was not offered by the client"
                );
            }
            PositionEncodingBasis::ProtocolDefault => {
                ensure!(
                    offered.is_empty(),
                    "protocol-default position encoding requires an absent client offer"
                );
                ensure!(
                    self.capabilities.position_encoding_selected.as_deref()
                        == Some("utf-16"),
                    "protocol-default position encoding must select utf-16"
                );
            }
            PositionEncodingBasis::NotProven => {
                ensure!(
                    self.capabilities.position_encoding_selected.is_none(),
                    "not-proven position encoding cannot carry a selected value"
                );
            }
        }

        let mut observed_messages = BTreeSet::new();
        for message in &self.diagnostics.observed_messages {
            ensure!(
                is_reason_token(message),
                "diagnostic observation must be a stable reason token: {message}"
            );
            ensure!(
                observed_messages.insert(message.as_str()),
                "duplicate diagnostic observation: {message}"
            );
        }

        ensure!(
            !self.journey.is_empty(),
            "at least one journey cell is required"
        );
        let mut seen_cells = BTreeSet::new();
        for cell in &self.journey {
            ensure!(
                is_reason_token(&cell.id),
                "journey cell id must be a stable reason token: {}",
                cell.id
            );
            ensure!(
                seen_cells.insert(cell.id.as_str()),
                "duplicate journey cell id: {}",
                cell.id
            );
            for evidence in &cell.evidence {
                validate_safe_identity(evidence, "journey evidence")?;
            }
            if matches!(
                cell.result,
                ObservationResult::Partial
                    | ObservationResult::NotProven
                    | ObservationResult::Unsupported
            ) {
                ensure!(
                    cell.limitation
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty()),
                    "partial/not_proven/unsupported cell {} requires a limitation",
                    cell.id
                );
            }
        }

        let mut artifact_keys = BTreeSet::new();
        let mut artifact_kinds = BTreeSet::new();
        for artifact in &self.artifacts {
            validate_safe_identity(&artifact.id, "artifact id")?;
            validate_sha256(&artifact.sha256, "artifact sha256")?;
            ensure!(
                artifact_keys.insert((artifact.kind, artifact.id.as_str())),
                "duplicate artifact identity: {}",
                artifact.id
            );
            artifact_kinds.insert(artifact.kind);
        }

        if self.result == ObservationResult::Pass {
            ensure!(
                self.failure_class.is_none(),
                "passing receipt cannot carry a failure_class"
            );
            ensure!(
                self.process_cleanup == CleanupResult::Pass,
                "passing receipt requires proven process cleanup"
            );
            ensure!(
                self.capabilities.position_encoding_basis
                    != PositionEncodingBasis::NotProven,
                "passing receipt requires a proven position encoding basis"
            );
            ensure!(
                is_lower_hex(&self.server.build_revision, 40),
                "passing receipt requires an exact server build revision"
            );
            ensure!(
                self.diagnostics.advertised_mode != DiagnosticMode::NotProven,
                "passing receipt cannot leave diagnostic mode not_proven"
            );
            if self.diagnostics.advertised_mode != DiagnosticMode::None {
                ensure!(
                    !self.diagnostics.observed_messages.is_empty(),
                    "passing receipt must observe the selected diagnostic path"
                );
            }
            ensure!(
                self.journey.iter().all(|cell| matches!(
                    cell.result,
                    ObservationResult::Pass | ObservationResult::Unsupported
                )),
                "passing receipt cannot contain fail/partial/not_proven journey cells"
            );
            for required in [
                ArtifactKind::ClientLog,
                ArtifactKind::ServerStderr,
                ArtifactKind::CapabilitySnapshot,
                ArtifactKind::ProcessLedger,
            ] {
                ensure!(
                    artifact_kinds.contains(&required),
                    "passing actual-host receipt omitted required artifact kind {required:?}"
                );
            }
        }

        if matches!(
            self.result,
            ObservationResult::Fail | ObservationResult::NotProven
        ) {
            ensure!(
                self.failure_class.is_some(),
                "fail/not_proven receipt requires failure_class"
            );
        }
        if self.result != ObservationResult::Pass {
            ensure!(
                self.limitations
                    .iter()
                    .any(|value| !value.trim().is_empty()),
                "non-passing receipt requires at least one limitation"
            );
        }
        if self.process_cleanup == CleanupResult::Fail {
            ensure!(
                self.result != ObservationResult::Pass,
                "cleanup failure cannot produce a passing receipt"
            );
        }

        for limitation in &self.limitations {
            ensure!(
                !limitation.trim().is_empty(),
                "limitations cannot contain empty strings"
            );
        }
        ensure!(
            !self.claim_boundary.trim().is_empty(),
            "claim_boundary is required"
        );
        Ok(())
    }

    pub fn subject_invalidations_against(
        &self,
        current: &Self,
    ) -> BTreeSet<&'static str> {
        let mut changed = BTreeSet::new();
        if self.stage != current.stage {
            changed.insert("evidence_stage");
        }
        if self.repository != current.repository {
            changed.insert("repository");
        }
        if self.candidate_sha != current.candidate_sha {
            changed.insert("candidate");
        }
        if self.platform != current.platform {
            changed.insert("platform");
        }
        if self.host != current.host {
            changed.insert("host");
        }
        if self.integration.mode != current.integration.mode {
            changed.insert("integration_mode");
        }
        if self.integration.registration_state
            != current.integration.registration_state
        {
            changed.insert("registration");
        }
        if self.integration.configuration_sha256
            != current.integration.configuration_sha256
        {
            changed.insert("configuration");
        }
        if self.integration.driver_sha256 != current.integration.driver_sha256 {
            changed.insert("driver");
        }
        if self.server.executable != current.server.executable
            || self.server.version != current.server.version
            || self.server.build_revision != current.server.build_revision
            || self.server.artifact_sha256 != current.server.artifact_sha256
        {
            changed.insert("server");
        }
        if self.server.protocol_version != current.server.protocol_version {
            changed.insert("protocol");
        }
        if self.server.launch_command != current.server.launch_command {
            changed.insert("launch");
        }
        if self.workspace_fixture != current.workspace_fixture {
            changed.insert("fixture");
        }
        if self.capabilities != current.capabilities {
            changed.insert("capabilities");
        }
        if self.diagnostics != current.diagnostics {
            changed.insert("diagnostics");
        }
        if self.artifacts != current.artifacts {
            changed.insert("artifacts");
        }
        if self.claim_boundary != current.claim_boundary {
            changed.insert("claim_boundary");
        }
        changed
    }
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{field} must use sha256:<64 lowercase hex> identity");
    };
    ensure!(
        is_lower_hex(hex, 64),
        "{field} must use sha256:<64 lowercase hex> identity"
    );
    Ok(())
}

fn validate_safe_identity(value: &str, field: &str) -> Result<()> {
    let value = value.trim();
    ensure!(!value.is_empty(), "{field} cannot be empty");
    ensure!(
        !value.starts_with('/'),
        "{field} must not expose an absolute path"
    );
    ensure!(
        !value.starts_with('~'),
        "{field} must not expose a home-relative path"
    );
    ensure!(
        !value.contains('\\'),
        "{field} must use normalized separators"
    );
    ensure!(
        !value.contains("://"),
        "{field} must not expose a URI-qualified path"
    );
    ensure!(
        !(value.len() >= 3
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && value.as_bytes()[2] == b'/'),
        "{field} must not expose a drive-qualified path"
    );
    ensure!(
        !value.split('/').any(|component| component == ".."),
        "{field} must not contain parent traversal"
    );
    Ok(())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b'-')
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
}
