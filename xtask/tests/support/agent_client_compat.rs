use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub const SCHEMA_VERSION: &str = "agent_client_compat.v1";

pub const CANONICAL_EXPECTATION_IDS: &[&str] = &[
    "code_action_preview.syntax",
    "definition.widget_new",
    "diagnostic.syntax",
    "document_symbols.widget",
    "edit_requery.widget_greet",
    "hover.widget_name",
    "lifecycle.shutdown",
    "references.widget_greet",
    "rename_preview.greet",
    "unicode.utf16",
    "workspace.partial_not_ready",
    "workspace_symbols.widget",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStage {
    StaticPackage,
    ExactSourceLocal,
    PublicArtifact,
    OfficialDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostProduct {
    ClaudeCode,
    CodexCli,
    CodexDesktop,
    CodexIde,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    NativeLspPlugin,
    CliSkill,
    NativeLocalMcp,
    ExternalBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Lsp,
    Mcp,
    Cli,
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
    HostPlugin,
    Fixture,
    Protocol,
    Instrument,
    Environment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformIdentity {
    pub os: String,
    pub os_version: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostIdentity {
    pub product: HostProduct,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationIdentity {
    pub mode: IntegrationMode,
    pub plugin_name: String,
    pub plugin_version: String,
    pub marketplace_source: String,
    pub marketplace_ref: String,
    pub package_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerIdentity {
    pub executable: String,
    pub version: String,
    pub build_revision: String,
    pub artifact_sha256: String,
    pub protocol: Protocol,
    pub protocol_or_schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFixtureIdentity {
    pub id: String,
    pub digest: String,
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
    pub id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentClientCompatReceipt {
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
    pub journey: Vec<JourneyCell>,
    pub result: ObservationResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<EvidenceArtifact>,
    pub claim_boundary: String,
}

impl AgentClientCompatReceipt {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == SCHEMA_VERSION, "unexpected schema_version");
        chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .context("observed_at must be an RFC 3339 date-time")?;
        ensure!(!self.repository.trim().is_empty(), "repository is required");
        ensure!(
            is_lower_hex(&self.candidate_sha, 40),
            "candidate_sha must be 40 lowercase hex chars"
        );
        ensure!(!self.platform.os.trim().is_empty(), "platform.os is required");
        ensure!(!self.platform.os_version.trim().is_empty(), "platform.os_version is required");
        ensure!(!self.platform.arch.trim().is_empty(), "platform.arch is required");
        ensure!(!self.host.version.trim().is_empty(), "host.version is required");
        if let Some(model) = self.host.instrument_model.as_deref() {
            ensure!(!model.trim().is_empty(), "instrument_model cannot be empty");
        }

        ensure!(!self.integration.plugin_name.trim().is_empty(), "plugin_name is required");
        ensure!(!self.integration.plugin_version.trim().is_empty(), "plugin_version is required");
        validate_safe_identity(&self.integration.marketplace_source, "marketplace_source")?;
        validate_safe_identity(&self.integration.marketplace_ref, "marketplace_ref")?;
        validate_sha256(&self.integration.package_sha256, "package_sha256")?;

        ensure!(!self.server.executable.trim().is_empty(), "server executable is required");
        ensure!(!self.server.version.trim().is_empty(), "server version is required");
        ensure!(!self.server.build_revision.trim().is_empty(), "server build_revision is required");
        validate_sha256(&self.server.artifact_sha256, "server artifact_sha256")?;
        ensure!(
            !self.server.protocol_or_schema_version.trim().is_empty(),
            "protocol_or_schema_version is required"
        );

        match self.integration.mode {
            IntegrationMode::NativeLspPlugin => {
                ensure!(self.server.protocol == Protocol::Lsp, "native_lsp_plugin must use LSP");
            }
            IntegrationMode::NativeLocalMcp => {
                ensure!(self.server.protocol == Protocol::Mcp, "native_local_mcp must use MCP");
            }
            IntegrationMode::CliSkill => {
                ensure!(self.server.protocol == Protocol::Cli, "cli_skill must use CLI protocol");
            }
            IntegrationMode::ExternalBridge => {}
        }

        ensure!(!self.workspace_fixture.id.trim().is_empty(), "workspace fixture id is required");
        validate_sha256(&self.workspace_fixture.digest, "workspace fixture digest")?;
        ensure!(!self.journey.is_empty(), "at least one journey cell is required");

        let mut seen_cells = BTreeSet::new();
        for cell in &self.journey {
            ensure!(
                is_reason_token(&cell.id),
                "journey cell id must be a stable reason token: {}",
                cell.id
            );
            ensure!(seen_cells.insert(cell.id.as_str()), "duplicate journey cell id: {}", cell.id);
            for evidence in &cell.evidence {
                validate_safe_identity(evidence, "journey evidence")?;
            }
            if matches!(cell.result, ObservationResult::Partial | ObservationResult::NotProven) {
                ensure!(
                    cell.limitation.as_deref().is_some_and(|value| !value.trim().is_empty()),
                    "partial/not_proven cell {} requires a limitation",
                    cell.id
                );
            }
        }

        if self.result == ObservationResult::Pass {
            ensure!(self.failure_class.is_none(), "passing receipt cannot carry a failure_class");
            ensure!(
                self.journey.iter().all(|cell| matches!(
                    cell.result,
                    ObservationResult::Pass | ObservationResult::Unsupported
                )),
                "passing receipt cannot contain fail/partial/not_proven journey cells"
            );
        }
        if matches!(self.result, ObservationResult::Fail | ObservationResult::NotProven) {
            ensure!(self.failure_class.is_some(), "fail/not_proven receipt requires failure_class");
        }
        if matches!(self.result, ObservationResult::Partial | ObservationResult::Unsupported) {
            ensure!(
                self.limitations.iter().any(|value| !value.trim().is_empty()),
                "partial/unsupported receipt requires a limitation"
            );
        }

        for limitation in &self.limitations {
            ensure!(!limitation.trim().is_empty(), "limitations cannot contain empty strings");
        }
        for artifact in &self.artifacts {
            validate_safe_identity(&artifact.id, "artifact id")?;
            validate_sha256(&artifact.sha256, "artifact sha256")?;
        }
        ensure!(!self.claim_boundary.trim().is_empty(), "claim_boundary is required");
        Ok(())
    }

    pub fn subject_invalidations_against(&self, current: &Self) -> BTreeSet<&'static str> {
        let mut changed = BTreeSet::new();
        if self.stage != current.stage {
            changed.insert("evidence_stage");
        }
        if self.repository != current.repository {
            changed.insert("repository");
        }
        if self.platform != current.platform {
            changed.insert("platform");
        }
        if self.host != current.host {
            changed.insert("host");
        }
        if self.integration.mode != current.integration.mode
            || self.integration.plugin_name != current.integration.plugin_name
            || self.integration.plugin_version != current.integration.plugin_version
            || self.integration.package_sha256 != current.integration.package_sha256
        {
            changed.insert("plugin");
        }
        if self.integration.marketplace_source != current.integration.marketplace_source
            || self.integration.marketplace_ref != current.integration.marketplace_ref
        {
            changed.insert("marketplace");
        }
        if self.server.executable != current.server.executable
            || self.server.version != current.server.version
            || self.server.build_revision != current.server.build_revision
            || self.server.artifact_sha256 != current.server.artifact_sha256
        {
            changed.insert("server");
        }
        if self.server.protocol != current.server.protocol
            || self.server.protocol_or_schema_version != current.server.protocol_or_schema_version
        {
            changed.insert("protocol");
        }
        if self.workspace_fixture != current.workspace_fixture {
            changed.insert("fixture");
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

pub fn fixture_digest(root: &Path) -> Result<String> {
    ensure!(root.is_dir(), "fixture root is not a directory: {}", root.display());
    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry.with_context(|| format!("walking fixture root {}", root.display()))?;
        if entry.file_type().is_symlink() {
            bail!("fixture must not contain symlink: {}", entry.path().display());
        }
        if entry.file_type().is_file() {
            let relative_path =
                entry.path().strip_prefix(root).with_context(|| "fixture path escaped root")?;
            let mut components = Vec::new();
            for component in relative_path.components() {
                let component = component.as_os_str().to_str().with_context(|| {
                    format!("fixture path is not valid UTF-8: {}", entry.path().display())
                })?;
                ensure!(
                    !component.contains('\\'),
                    "fixture path component must not contain a backslash"
                );
                components.push(component);
            }
            let relative = components.join("/");
            files.push((relative, entry.path().to_path_buf()));
        }
    }
    ensure!(!files.is_empty(), "fixture root contains no files: {}", root.display());
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (relative, path) in files {
        let bytes =
            fs::read(&path).with_context(|| format!("reading fixture file {}", path.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{field} must use sha256:<64 lowercase hex> identity");
    };
    ensure!(is_lower_hex(hex, 64), "{field} must use sha256:<64 lowercase hex> identity");
    Ok(())
}

fn validate_safe_identity(value: &str, field: &str) -> Result<()> {
    let value = value.trim();
    ensure!(!value.is_empty(), "{field} cannot be empty");
    ensure!(!value.starts_with('/'), "{field} must not expose an absolute path");
    ensure!(!value.starts_with('~'), "{field} must not expose a home-relative path");
    ensure!(!value.contains('\\'), "{field} must use normalized separators");
    ensure!(!value.contains("://"), "{field} must not expose a URI-qualified path");
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
    value.len() == len && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
}
