//! Fail-closed validator and deterministic status generator for #10112.
//!
//! The Kubernetes DAP workspace-profile contract admits only `project_image`
//! and `injected_tool` subjects and mechanically rejects every unsupported
//! network/service/sidecar/operator topology before any cluster execution
//! work begins.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p xtask --bin kubernetes-dap-profiles -- --check
//! cargo run -p xtask --bin kubernetes-dap-profiles -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const CONTRACT_SCHEMA_VERSION: &str = "kubernetes_dap_workspace_profile.v1";
const FIXTURE_SCHEMA_VERSION: &str = "kubernetes_dap_workspace_profile_fixture.v1";
const DEFAULT_CONTRACT: &str = "contracts/dap/kubernetes_dap_workspace_profile.v1.toml";
const DEFAULT_FIXTURES_DIR: &str = "contracts/dap/fixtures";
const DEFAULT_STATUS: &str = "docs/project/status/kubernetes_dap_workspace_profiles.md";
const DEFAULT_PROFILE_SCHEMA: &str = "schemas/kubernetes_dap_workspace_profile.v1.schema.json";
const DEFAULT_FIXTURE_SCHEMA: &str =
    "schemas/kubernetes_dap_workspace_profile_fixture.v1.schema.json";

/// Rejection classes that must stay registered as topology rows because the
/// controlling issue (#10112) names them as unsupported topologies.
const REQUIRED_TOPOLOGY_CODES: &[RejectionReason] = &[
    RejectionReason::StandaloneDeploymentForbidden,
    RejectionReason::ServiceExposureForbidden,
    RejectionReason::NetworkListenerForbidden,
    RejectionReason::SharedMultiTenantAdapterForbidden,
    RejectionReason::SidecarEnvironmentMismatch,
    RejectionReason::EditorPathTranslationForbidden,
    RejectionReason::AttachInjectionUnsupported,
    RejectionReason::AdapterOwnedClusterAccessForbidden,
    RejectionReason::KubernetesApiDependencyForbidden,
    RejectionReason::OperatorControllerForbidden,
    RejectionReason::ImageIdentityNotExact,
];

/// Minimum number of security requirements; #10112 lists eight ownership and
/// disposition facts that must stay explicit.
const MINIMUM_SECURITY_REQUIREMENTS: usize = 8;

/// Number of DAP cells in the initial admitted family; #10112 enumerates this
/// family explicitly and the projection may not shrink silently.
const INITIAL_ADMITTED_FAMILY_SIZE: usize = 8;

#[derive(Debug, Parser)]
#[command(name = "kubernetes-dap-profiles")]
#[command(about = "Validate the Kubernetes DAP workspace-profile contract and its fixtures")]
struct Cli {
    /// Machine-readable profile contract.
    #[arg(long, default_value = DEFAULT_CONTRACT)]
    contract: PathBuf,

    /// Directory of deterministic positive/negative profile fixtures.
    #[arg(long, default_value = DEFAULT_FIXTURES_DIR)]
    fixtures_dir: PathBuf,

    /// Generated Markdown status path.
    #[arg(long, default_value = DEFAULT_STATUS)]
    status: PathBuf,

    /// Published profile JSON schema path.
    #[arg(long, default_value = DEFAULT_PROFILE_SCHEMA)]
    profile_schema: PathBuf,

    /// Published fixture JSON schema path.
    #[arg(long, default_value = DEFAULT_FIXTURE_SCHEMA)]
    fixture_schema: PathBuf,

    /// Validate everything and fail when the committed status is stale.
    #[arg(long)]
    check: bool,

    /// Validate everything and rewrite the generated status.
    #[arg(long)]
    write_status: bool,
}

// ---------------------------------------------------------------------------
// Typed rejection reasons
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RejectionReason {
    StandaloneDeploymentForbidden,
    SharedMultiTenantAdapterForbidden,
    SidecarEnvironmentMismatch,
    ServiceExposureForbidden,
    NetworkListenerForbidden,
    AttachInjectionUnsupported,
    AdapterOwnedClusterAccessForbidden,
    KubernetesApiDependencyForbidden,
    OperatorControllerForbidden,
    EditorPathTranslationForbidden,
    LspProfileProjectionForbidden,
    CapabilityCatalogInheritanceForbidden,
    TransportBoundaryViolation,
    SourceNamespaceMismatch,
    LoaderContractMismatch,
    InitImagePerlSubstitutionForbidden,
    BaselinePerlSubstitutionForbidden,
    ProjectPerlIdentityMismatch,
    ImageIdentityNotExact,
    InjectionSourceUnbound,
    ArtifactDigestUnverified,
    ExecutableModeInvalid,
    ToolMountNotReadOnly,
    ResourceProfileMissing,
    CleanupOwnershipMissing,
    SecurityContextMissing,
    ServiceAccountTokenForbidden,
    DapCellEvidenceMissing,
    InstallModeIdentityConflict,
    AdapterIdentityIncomplete,
}

impl RejectionReason {
    const ALL: &'static [RejectionReason] = &[
        Self::StandaloneDeploymentForbidden,
        Self::SharedMultiTenantAdapterForbidden,
        Self::SidecarEnvironmentMismatch,
        Self::ServiceExposureForbidden,
        Self::NetworkListenerForbidden,
        Self::AttachInjectionUnsupported,
        Self::AdapterOwnedClusterAccessForbidden,
        Self::KubernetesApiDependencyForbidden,
        Self::OperatorControllerForbidden,
        Self::EditorPathTranslationForbidden,
        Self::LspProfileProjectionForbidden,
        Self::CapabilityCatalogInheritanceForbidden,
        Self::TransportBoundaryViolation,
        Self::SourceNamespaceMismatch,
        Self::LoaderContractMismatch,
        Self::InitImagePerlSubstitutionForbidden,
        Self::BaselinePerlSubstitutionForbidden,
        Self::ProjectPerlIdentityMismatch,
        Self::ImageIdentityNotExact,
        Self::InjectionSourceUnbound,
        Self::ArtifactDigestUnverified,
        Self::ExecutableModeInvalid,
        Self::ToolMountNotReadOnly,
        Self::ResourceProfileMissing,
        Self::CleanupOwnershipMissing,
        Self::SecurityContextMissing,
        Self::ServiceAccountTokenForbidden,
        Self::DapCellEvidenceMissing,
        Self::InstallModeIdentityConflict,
        Self::AdapterIdentityIncomplete,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::StandaloneDeploymentForbidden => "standalone_deployment_forbidden",
            Self::SharedMultiTenantAdapterForbidden => "shared_multi_tenant_adapter_forbidden",
            Self::SidecarEnvironmentMismatch => "sidecar_environment_mismatch",
            Self::ServiceExposureForbidden => "service_exposure_forbidden",
            Self::NetworkListenerForbidden => "network_listener_forbidden",
            Self::AttachInjectionUnsupported => "attach_injection_unsupported",
            Self::AdapterOwnedClusterAccessForbidden => "adapter_owned_cluster_access_forbidden",
            Self::KubernetesApiDependencyForbidden => "kubernetes_api_dependency_forbidden",
            Self::OperatorControllerForbidden => "operator_controller_forbidden",
            Self::EditorPathTranslationForbidden => "editor_path_translation_forbidden",
            Self::LspProfileProjectionForbidden => "lsp_profile_projection_forbidden",
            Self::CapabilityCatalogInheritanceForbidden => {
                "capability_catalog_inheritance_forbidden"
            }
            Self::TransportBoundaryViolation => "transport_boundary_violation",
            Self::SourceNamespaceMismatch => "source_namespace_mismatch",
            Self::LoaderContractMismatch => "loader_contract_mismatch",
            Self::InitImagePerlSubstitutionForbidden => "init_image_perl_substitution_forbidden",
            Self::BaselinePerlSubstitutionForbidden => "baseline_perl_substitution_forbidden",
            Self::ProjectPerlIdentityMismatch => "project_perl_identity_mismatch",
            Self::ImageIdentityNotExact => "image_identity_not_exact",
            Self::InjectionSourceUnbound => "injection_source_unbound",
            Self::ArtifactDigestUnverified => "artifact_digest_unverified",
            Self::ExecutableModeInvalid => "executable_mode_invalid",
            Self::ToolMountNotReadOnly => "tool_mount_not_read_only",
            Self::ResourceProfileMissing => "resource_profile_missing",
            Self::CleanupOwnershipMissing => "cleanup_ownership_missing",
            Self::SecurityContextMissing => "security_context_missing",
            Self::ServiceAccountTokenForbidden => "service_account_token_forbidden",
            Self::DapCellEvidenceMissing => "dap_cell_evidence_missing",
            Self::InstallModeIdentityConflict => "install_mode_identity_conflict",
            Self::AdapterIdentityIncomplete => "adapter_identity_incomplete",
        }
    }
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A typed, fail-closed rejection of one candidate profile.
#[derive(Debug, Clone)]
struct Rejection {
    reason: RejectionReason,
    detail: String,
}

impl Rejection {
    fn new(reason: RejectionReason, detail: impl Into<String>) -> Self {
        Self { reason, detail: detail.into() }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reason.as_str(), self.detail)
    }
}

impl std::error::Error for Rejection {}

fn rejection(reason: RejectionReason, detail: impl Into<String>) -> Result<(), Rejection> {
    Err(Rejection::new(reason, detail))
}

// ---------------------------------------------------------------------------
// Contract document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProfileContract {
    schema_version: String,
    contract_id: String,
    controller_issue: String,
    parent_train: String,
    environment_subject_authority: String,
    path_namespace_authority: String,
    dap_core_authority: String,
    launch_environment_authority: String,
    support_registry_authority: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    transport_boundary: TransportBoundary,
    source_namespace: SourceNamespaceRule,
    admitted_profiles: Vec<AdmittedProfile>,
    rejected_topologies: Vec<RejectedTopology>,
    dap_cells: Vec<DapCellRow>,
    security_requirements: Vec<SecurityRequirementRow>,
    limitations: Vec<LimitationRow>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct TransportBoundary {
    transport: TransportKind,
    stream_owner: StreamOwner,
    tty: bool,
    shell: bool,
    network_listener: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransportKind {
    Stdio,
    Tcp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StreamOwner {
    Parent,
    Adapter,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceNamespaceRule {
    equality: String,
    canonicalization_authority: String,
    rewrite_authority: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AdmittedProfile {
    profile_id: String,
    install_mode: InstallMode,
    summary: String,
    required_facts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum InstallMode {
    ProjectImage,
    InjectedTool,
}

impl InstallMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProjectImage => "project_image",
            Self::InjectedTool => "injected_tool",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RejectedTopology {
    topology_id: String,
    reason_code: RejectionReason,
    boundary: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum CellFamily {
    InitialAdmitted,
    Optional,
}

impl CellFamily {
    fn as_str(self) -> &'static str {
        match self {
            Self::InitialAdmitted => "initial_admitted",
            Self::Optional => "optional",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum CellState {
    EvidenceBacked,
    NotProven,
    Unsupported,
}

impl CellState {
    fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceBacked => "evidence_backed",
            Self::NotProven => "not_proven",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DapCellRow {
    cell_id: String,
    family: CellFamily,
    state: CellState,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SecurityRequirementRow {
    requirement_id: String,
    statement: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LimitationRow {
    statement: String,
}

fn is_non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn validate_issue_ref(name: &str, value: &str) -> Result<()> {
    let Some(digits) = value.strip_prefix('#') else {
        bail!("{name} must be a GitHub issue reference like #10112; got {value:?}");
    };
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{name} must be a GitHub issue reference like #10112; got {value:?}");
    }
    Ok(())
}

/// Every required fact an admitted-profile row may declare, bound to the typed
/// rejection this validator actually emits when the fact is missing or false.
///
/// The contract's own header claims each row is "mechanically enforced by
/// `--check`". Before this table that claim was prose: `required_facts` was
/// checked for name shape and duplicates only, so a declared fact could inflate
/// the generated coverage story without any admission consequence — which is
/// how `adapter_binary_path_version_hash_target` sat unenforced. A contract
/// that declares a fact absent from this table is now rejected, so the
/// advertised coverage cannot outrun the gate again.
const FACT_ENFORCEMENT: &[(&str, RejectionReason)] = &[
    ("image_digest_and_build_identity", RejectionReason::ImageIdentityNotExact),
    ("image_libc_identity", RejectionReason::LoaderContractMismatch),
    ("adapter_binary_path_version_hash_target", RejectionReason::AdapterIdentityIncomplete),
    ("project_perl_path_version_environment", RejectionReason::ProjectPerlIdentityMismatch),
    ("exact_workspace_root_and_source_paths", RejectionReason::SourceNamespaceMismatch),
    ("non_root_security_resource_cleanup", RejectionReason::SecurityContextMissing),
    ("no_network_listener", RejectionReason::NetworkListenerForbidden),
    (
        "injection_source_artifact_digest_and_build_revision",
        RejectionReason::InjectionSourceUnbound,
    ),
    ("injected_artifact_libc_identity", RejectionReason::LoaderContractMismatch),
    ("copy_and_post_copy_digest_verification", RejectionReason::ArtifactDigestUnverified),
    ("executable_mode", RejectionReason::ExecutableModeInvalid),
    (
        "host_container_os_libc_architecture_loader_compatibility",
        RejectionReason::LoaderContractMismatch,
    ),
    ("tool_volume_ownership_and_read_only_mount", RejectionReason::ToolMountNotReadOnly),
    (
        "project_container_perl_authority_not_init_image_perl",
        RejectionReason::InitImagePerlSubstitutionForbidden,
    ),
];

fn enforcement_for_fact(key: &str) -> Option<RejectionReason> {
    FACT_ENFORCEMENT.iter().find(|(fact, _)| *fact == key).map(|(_, reason)| *reason)
}

fn validate_fact_key(profile_id: &str, key: &str) -> Result<()> {
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !key.starts_with('_')
        && !key.ends_with('_');
    if !valid {
        bail!("admitted profile {profile_id} has malformed required fact key {key:?}");
    }
    if enforcement_for_fact(key).is_none() {
        bail!(
            "admitted profile {profile_id} declares required fact {key:?} with no typed \
             enforcement; add it to FACT_ENFORCEMENT with the rejection the validator emits, \
             or remove the claim"
        );
    }
    Ok(())
}

impl ProfileContract {
    fn from_str(source: &str) -> Result<Self> {
        toml::from_str(source).context("parse kubernetes DAP workspace-profile contract")
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != CONTRACT_SCHEMA_VERSION {
            bail!(
                "unsupported profile contract schema {:?}; expected {:?}",
                self.schema_version,
                CONTRACT_SCHEMA_VERSION
            );
        }
        for (name, value) in [
            ("contract_id", self.contract_id.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
            ("environment_subject_authority", self.environment_subject_authority.as_str()),
            ("path_namespace_authority", self.path_namespace_authority.as_str()),
            ("dap_core_authority", self.dap_core_authority.as_str()),
            ("launch_environment_authority", self.launch_environment_authority.as_str()),
            ("support_registry_authority", self.support_registry_authority.as_str()),
        ] {
            if !is_non_empty(value) {
                bail!("profile contract field {name} must not be empty");
            }
        }
        for (name, value) in [
            ("controller_issue", self.controller_issue.as_str()),
            ("parent_train", self.parent_train.as_str()),
        ] {
            validate_issue_ref(name, value)?;
        }
        if self.complete {
            bail!(
                "this seed contract is intentionally incomplete; flipping complete requires a successor contract"
            );
        }

        let boundary = self.transport_boundary;
        if boundary.transport != TransportKind::Stdio
            || boundary.stream_owner != StreamOwner::Parent
            || boundary.tty
            || boundary.shell
            || boundary.network_listener
        {
            bail!(
                "contract transport boundary must remain parent-owned stdio with no shell, no TTY, and no listener"
            );
        }

        if self.source_namespace.equality != "client == adapter == debuggee" {
            bail!("source namespace equality must bind client, adapter, and debuggee paths");
        }
        if !is_non_empty(&self.source_namespace.canonicalization_authority)
            || self.source_namespace.rewrite_authority != "none"
        {
            bail!(
                "source namespace rule must delegate canonicalization to its owning authority and admit no rewrite authority"
            );
        }

        let expected: [(&str, InstallMode); 2] = [
            ("project_image", InstallMode::ProjectImage),
            ("injected_tool", InstallMode::InjectedTool),
        ];
        if self.admitted_profiles.len() != expected.len() {
            bail!(
                "contract must admit exactly {} profiles (project_image, injected_tool); found {}",
                expected.len(),
                self.admitted_profiles.len()
            );
        }
        for (row, (profile_id, install_mode)) in self.admitted_profiles.iter().zip(expected) {
            if row.profile_id != profile_id || row.install_mode != install_mode {
                bail!(
                    "admitted profile row {:?}/{} does not match the required pair {:?}/{}",
                    row.profile_id,
                    row.install_mode.as_str(),
                    profile_id,
                    install_mode.as_str()
                );
            }
            if !is_non_empty(&row.summary) {
                bail!("admitted profile {profile_id} needs a non-empty summary");
            }
            if row.required_facts.is_empty() {
                bail!("admitted profile {profile_id} must declare required facts");
            }
            let mut seen = BTreeMap::new();
            for fact in &row.required_facts {
                validate_fact_key(&row.profile_id, fact)?;
                if seen.insert(fact.as_str(), ()).is_some() {
                    bail!("admitted profile {profile_id} repeats required fact {fact:?}");
                }
            }
        }

        let mut registered_codes = BTreeMap::new();
        for topology in &self.rejected_topologies {
            if !is_non_empty(&topology.topology_id) || !is_non_empty(&topology.boundary) {
                bail!(
                    "rejected topology row {:?} needs a non-empty id and boundary statement",
                    topology.topology_id
                );
            }
            if registered_codes
                .insert(topology.reason_code.as_str(), topology.topology_id.as_str())
                .is_some()
            {
                bail!(
                    "reason code {} is registered by more than one rejected topology row",
                    topology.reason_code.as_str()
                );
            }
        }
        for required in REQUIRED_TOPOLOGY_CODES {
            if !registered_codes.contains_key(required.as_str()) {
                bail!(
                    "rejected topology registry is missing a row for required code {}",
                    required.as_str()
                );
            }
        }

        let mut cell_ids = BTreeMap::new();
        let mut initial_admitted = 0usize;
        for cell in &self.dap_cells {
            if !is_non_empty(&cell.cell_id) {
                bail!("dap cell rows need non-empty cell ids");
            }
            if cell_ids.insert(cell.cell_id.as_str(), ()).is_some() {
                bail!("duplicate dap cell row {:?}", cell.cell_id);
            }
            match cell.family {
                CellFamily::InitialAdmitted => {
                    initial_admitted += 1;
                    if cell.state != CellState::EvidenceBacked {
                        bail!("initial admitted cell {:?} must be evidence backed", cell.cell_id);
                    }
                    if cell.evidence.is_empty() || cell.evidence.iter().any(String::is_empty) {
                        bail!(
                            "initial admitted cell {:?} must reference current exact evidence",
                            cell.cell_id
                        );
                    }
                }
                CellFamily::Optional => {
                    if cell.state == CellState::EvidenceBacked {
                        bail!(
                            "optional cell {:?} has no profile-specific row and cannot be evidence backed",
                            cell.cell_id
                        );
                    }
                    if !cell.evidence.is_empty() {
                        bail!(
                            "optional cell {:?} carries no evidence while it remains not_proven or unsupported",
                            cell.cell_id
                        );
                    }
                }
            }
        }
        if initial_admitted < INITIAL_ADMITTED_FAMILY_SIZE {
            bail!(
                "initial admitted family shrank to {initial_admitted} cells; the #10112 family has {INITIAL_ADMITTED_FAMILY_SIZE}"
            );
        }
        let has_optional_ceiling =
            self.dap_cells.iter().any(|cell| cell.family == CellFamily::Optional);
        if !has_optional_ceiling {
            bail!("projection must keep an explicit not_proven/unsupported optional ceiling");
        }

        if self.security_requirements.len() < MINIMUM_SECURITY_REQUIREMENTS {
            bail!(
                "security requirements fell below the {MINIMUM_SECURITY_REQUIREMENTS} explicit ownership facts required by #10112"
            );
        }
        let mut requirement_ids = BTreeMap::new();
        for requirement in &self.security_requirements {
            if !is_non_empty(&requirement.requirement_id) || !is_non_empty(&requirement.statement) {
                bail!("security requirements need non-empty ids and statements");
            }
            if requirement_ids.insert(requirement.requirement_id.as_str(), ()).is_some() {
                bail!("duplicate security requirement {:?}", requirement.requirement_id);
            }
        }

        if self.limitations.is_empty()
            || self.limitations.iter().any(|limitation| limitation.statement.trim().is_empty())
        {
            bail!("the contract must state its limitations explicitly");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Candidate profile documents (fixtures embed these)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DeploymentMode {
    WorkspaceContainerProcess,
    StandaloneDeployment,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SharingMode {
    DedicatedWorkspace,
    SharedMultiTenant,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PerlAuthority {
    ProjectContainer,
    InitImage,
    BaselineImage,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ServiceAccountToken {
    Absent,
    MountedUnused,
    MountedUsed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ServiceKind {
    Service,
    Ingress,
    LoadBalancer,
    NodePort,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ToolMount {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProfileTransportKind {
    Stdio,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProfileTransport {
    transport: ProfileTransportKind,
    stream_owner: StreamOwner,
    tty: bool,
    shell: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WorkspacePaths {
    root: String,
    source_paths: SourcePaths,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourcePaths {
    client: String,
    adapter: String,
    debuggee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LoaderContract {
    os: String,
    libc: String,
    architecture: String,
    container_matches: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PerlEnvironment {
    authority: PerlAuthority,
    interpreter_path: String,
    version: String,
    include_roots: Vec<String>,
    dependency_authority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LaunchPlan {
    authority: String,
    perl_identity: String,
}

/// Adapter (perl-dap) binary identity. The contract declares
/// `adapter_binary_path_version_hash_target` as a required fact for the
/// `project_image` profile, so the profile must name the exact binary it
/// intends to execute rather than relying on the image digest alone: an exact
/// image can still contain no identified debugger, or a different one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AdapterIdentity {
    binary_path: String,
    version: String,
    hash: String,
    target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ImageIdentity {
    repository: String,
    digest: String,
    build_revision: String,
    architecture: String,
    libc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InjectedArtifact {
    source_identity: String,
    artifact_digest: String,
    copied_digest: String,
    post_copy_verified: bool,
    exec_mode: String,
    tool_mount: ToolMount,
    target_arch: String,
    libc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SecurityDisposition {
    run_as_non_root: Option<bool>,
    uid: Option<u32>,
    gid: Option<u32>,
    service_account_token: Option<ServiceAccountToken>,
    kubernetes_credentials: Option<bool>,
    network_listener: Option<bool>,
    writable_paths_declared: Option<bool>,
    secrets_redacted_from_receipts: Option<bool>,
    adapter_selection_isolation: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DapCellClaim {
    cell_id: String,
    claimed_state: CellState,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ServiceExposure {
    kind: ServiceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ListenerConfig {
    bind_address: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SidecarProjection {
    shares_filesystem: bool,
    perl_authority: PerlAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AttachPolicy {
    process_id_targeting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ClusterAccess {
    owner: String,
    kubectl_exec: bool,
    port_forward: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct KubernetesApiDependency {
    rbac_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OperatorRole {
    role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PathRewrite {
    table: Vec<PathRewriteEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PathRewriteEntry {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LspProfileBinding {
    inherits_lsp_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CapabilityCatalog {
    static_catalog: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProfileDocument {
    profile_id: String,
    install_mode: InstallMode,
    deployment_mode: DeploymentMode,
    sharing_mode: SharingMode,
    transport: ProfileTransport,
    workspace: WorkspacePaths,
    loader: LoaderContract,
    perl: PerlEnvironment,
    launch_plan: LaunchPlan,
    image: Option<ImageIdentity>,
    artifact: Option<InjectedArtifact>,
    adapter: Option<AdapterIdentity>,
    resource_profile: Option<String>,
    cleanup_owner: Option<String>,
    security: SecurityDisposition,
    dap_claims: Vec<DapCellClaim>,
    service_exposure: Option<ServiceExposure>,
    listener: Option<ListenerConfig>,
    sidecar: Option<SidecarProjection>,
    attach: Option<AttachPolicy>,
    cluster_access: Option<ClusterAccess>,
    kubernetes_api: Option<KubernetesApiDependency>,
    operator: Option<OperatorRole>,
    path_rewrite: Option<PathRewrite>,
    lsp_profile_binding: Option<LspProfileBinding>,
    capability_catalog: Option<CapabilityCatalog>,
}

fn is_exact_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// An injected-tool source identity is `<source>@<exact digest>`. The digest
/// alone names bytes but not their provenance, so a value whose prefix is empty
/// or blank (`"@sha256:..."`) is unbound: nothing records where the artifact
/// came from.
fn is_bound_source_identity(value: &str) -> bool {
    match value.rsplit_once('@') {
        Some((source, digest)) => !source.trim().is_empty() && is_exact_digest(digest),
        None => false,
    }
}

fn source_identity_digest(value: &str) -> Option<&str> {
    value.rsplit_once('@').map(|(_, digest)| digest)
}

fn is_normalized_workspace_path(value: &str) -> bool {
    !value.trim().is_empty()
        && value.starts_with('/')
        && (value == "/" || !value.ends_with('/'))
        && !value.contains("//")
        && value.split('/').skip(1).all(|component| component != "." && component != "..")
}

fn is_contained_path(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.starts_with(&format!("{root}/"))
}

impl ProfileDocument {
    fn derived_perl_identity(&self) -> String {
        format!("{}@{}", self.perl.interpreter_path, self.perl.version)
    }

    /// Fail-closed admission decision. Every unsupported topology class and
    /// every missing admission fact yields a typed rejection; nothing is
    /// defaulted to a pass.
    fn evaluate(&self, contract: &ProfileContract) -> Result<(), Rejection> {
        let why = |detail: String| format!("profile {:?}: {detail}", self.profile_id);

        if self.deployment_mode == DeploymentMode::StandaloneDeployment {
            return rejection(
                RejectionReason::StandaloneDeploymentForbidden,
                why("perl-dap runs as a standalone Deployment outside the project workspace container".into()),
            );
        }
        if self.sharing_mode == SharingMode::SharedMultiTenant {
            return rejection(
                RejectionReason::SharedMultiTenantAdapterForbidden,
                why("one shared multi-tenant adapter serves several workspaces".into()),
            );
        }
        if self.sidecar.is_some() {
            return rejection(
                RejectionReason::SidecarEnvironmentMismatch,
                why("an adapter sidecar sharing only a pod cannot prove identical source and project-Perl parity".into()),
            );
        }
        if self.service_exposure.is_some() {
            return rejection(
                RejectionReason::ServiceExposureForbidden,
                why("service/ingress/load-balancer/node-port exposure enters the profile".into()),
            );
        }
        if self.listener.is_some() {
            return rejection(
                RejectionReason::NetworkListenerForbidden,
                why("the profile declares an ambient TCP listener for DAP".into()),
            );
        }
        if self.attach.is_some() {
            return rejection(
                RejectionReason::AttachInjectionUnsupported,
                why("processId attach into an arbitrary pod process is not an admitted cell".into()),
            );
        }
        if self.cluster_access.is_some() {
            return rejection(
                RejectionReason::AdapterOwnedClusterAccessForbidden,
                why("the adapter owns kubectl/exec/port-forward authority".into()),
            );
        }
        if self.kubernetes_api.is_some() {
            return rejection(
                RejectionReason::KubernetesApiDependencyForbidden,
                why("perl-dap depends on the Kubernetes API or RBAC".into()),
            );
        }
        if self.operator.is_some() {
            return rejection(
                RejectionReason::OperatorControllerForbidden,
                why("an operator/CRD/controller reconciles debugger subjects".into()),
            );
        }
        if self.path_rewrite.is_some() {
            return rejection(
                RejectionReason::EditorPathTranslationForbidden,
                why("a path-rewrite table translates editor paths into workspace paths".into()),
            );
        }
        if self.lsp_profile_binding.is_some() {
            return rejection(
                RejectionReason::LspProfileProjectionForbidden,
                why("a DAP profile may not be inferred from an LSP workspace profile".into()),
            );
        }
        if self.capability_catalog.is_some() {
            return rejection(
                RejectionReason::CapabilityCatalogInheritanceForbidden,
                why("DAP capabilities inherited from a static catalog are not evidence".into()),
            );
        }

        if self.transport.transport != ProfileTransportKind::Stdio
            || self.transport.stream_owner != StreamOwner::Parent
            || self.transport.tty
            || self.transport.shell
        {
            return rejection(
                RejectionReason::TransportBoundaryViolation,
                why("DAP transport must be parent-owned stdio with no shell and no TTY".into()),
            );
        }
        if self.security.network_listener == Some(true) {
            return rejection(
                RejectionReason::NetworkListenerForbidden,
                why("the security disposition creates a network listener for DAP".into()),
            );
        }

        let paths = &self.workspace.source_paths;
        if !is_normalized_workspace_path(&self.workspace.root)
            || !is_normalized_workspace_path(&paths.client)
            || !is_normalized_workspace_path(&paths.adapter)
            || !is_normalized_workspace_path(&paths.debuggee)
        {
            return rejection(
                RejectionReason::SourceNamespaceMismatch,
                why("workspace root and source paths must be absolute normalized paths".into()),
            );
        }
        if !is_contained_path(&self.workspace.root, &paths.client)
            || !is_contained_path(&self.workspace.root, &paths.adapter)
            || !is_contained_path(&self.workspace.root, &paths.debuggee)
        {
            return rejection(
                RejectionReason::SourceNamespaceMismatch,
                why(format!(
                    "source paths must be contained by workspace root {:?}",
                    self.workspace.root
                )),
            );
        }
        if paths.client != paths.adapter || paths.adapter != paths.debuggee {
            return rejection(
                RejectionReason::SourceNamespaceMismatch,
                why(format!(
                    "client ({:?}), adapter ({:?}), and debuggee ({:?}) source namespaces differ",
                    paths.client, paths.adapter, paths.debuggee
                )),
            );
        }

        match self.perl.authority {
            PerlAuthority::InitImage => {
                return rejection(
                    RejectionReason::InitImagePerlSubstitutionForbidden,
                    why("the init-image Perl cannot satisfy the project-container Perl identity"
                        .into()),
                );
            }
            PerlAuthority::BaselineImage => {
                return rejection(
                    RejectionReason::BaselinePerlSubstitutionForbidden,
                    why("a baseline image Perl cannot silently satisfy a project-environment row"
                        .into()),
                );
            }
            PerlAuthority::ProjectContainer => {}
        }
        if self.perl.interpreter_path.trim().is_empty()
            || self.perl.version.trim().is_empty()
            || self.perl.include_roots.is_empty()
            || self.perl.dependency_authority.trim().is_empty()
            || self.launch_plan.authority.trim().is_empty()
        {
            return rejection(
                RejectionReason::ProjectPerlIdentityMismatch,
                why("project Perl/environment or launch-plan authority facts are incomplete".into()),
            );
        }
        if self.launch_plan.perl_identity != self.derived_perl_identity() {
            return rejection(
                RejectionReason::ProjectPerlIdentityMismatch,
                why(format!(
                    "launch plan binds perl identity {:?} but the profile derives {:?}",
                    self.launch_plan.perl_identity,
                    self.derived_perl_identity()
                )),
            );
        }

        match self.install_mode {
            InstallMode::ProjectImage => {
                let Some(image) = &self.image else {
                    return rejection(
                        RejectionReason::ImageIdentityNotExact,
                        why("project_image requires an exact project image identity".into()),
                    );
                };
                if image.repository.trim().is_empty()
                    || !is_exact_digest(&image.digest)
                    || image.build_revision.trim().is_empty()
                    || image.architecture.trim().is_empty()
                    || image.libc.trim().is_empty()
                {
                    return rejection(
                        RejectionReason::ImageIdentityNotExact,
                        why("project image needs repository, sha256 digest, build/source identity, architecture, and libc".into()),
                    );
                }
                if self.artifact.is_some() {
                    return rejection(
                        RejectionReason::InstallModeIdentityConflict,
                        why("project_image cannot include injected-artifact identity".into()),
                    );
                }
            }
            InstallMode::InjectedTool => {
                let Some(artifact) = &self.artifact else {
                    return rejection(
                        RejectionReason::InjectionSourceUnbound,
                        why("injected_tool requires an exact injected artifact description".into()),
                    );
                };
                if !is_bound_source_identity(&artifact.source_identity) {
                    return rejection(
                        RejectionReason::InjectionSourceUnbound,
                        why(format!(
                            "injection source {:?} is not bound to an exact digest and revision",
                            artifact.source_identity
                        )),
                    );
                }
                if artifact.libc.trim().is_empty() {
                    return rejection(
                        RejectionReason::LoaderContractMismatch,
                        why("injected artifact libc identity is empty".into()),
                    );
                }
                if !is_exact_digest(&artifact.artifact_digest)
                    || !is_exact_digest(&artifact.copied_digest)
                    || artifact.artifact_digest != artifact.copied_digest
                    || !artifact.post_copy_verified
                {
                    return rejection(
                        RejectionReason::ArtifactDigestUnverified,
                        why(
                            "the injected artifact is not digest-identical after the verified copy"
                                .into(),
                        ),
                    );
                }
                let source_digest = match source_identity_digest(&artifact.source_identity) {
                    Some(digest) => digest,
                    None => "",
                };
                if source_digest != artifact.artifact_digest.as_str()
                    || source_digest != artifact.copied_digest.as_str()
                {
                    return rejection(
                        RejectionReason::ArtifactDigestUnverified,
                        why(format!(
                            "injection source digest {:?} does not match artifact digest {:?} and copied digest {:?}",
                            source_digest, artifact.artifact_digest, artifact.copied_digest
                        )),
                    );
                }
                if artifact.exec_mode != "0555" {
                    return rejection(
                        RejectionReason::ExecutableModeInvalid,
                        why(format!(
                            "injected adapter executable mode {:?} is wrong or missing",
                            artifact.exec_mode
                        )),
                    );
                }
                if artifact.tool_mount != ToolMount::ReadOnly {
                    return rejection(
                        RejectionReason::ToolMountNotReadOnly,
                        why("the shared tool volume must mount read-only into the workspace container".into()),
                    );
                }
                if self.image.is_some() {
                    return rejection(
                        RejectionReason::InstallModeIdentityConflict,
                        why("injected_tool cannot include image identity".into()),
                    );
                }
            }
        }

        // The contract declares the adapter binary identity as a required fact
        // for the admitted profiles. An exact image or artifact digest does not
        // establish which debugger, if any, the profile intends to execute, so
        // the identity is validated here rather than inferred from the subject.
        let Some(adapter) = &self.adapter else {
            return rejection(
                RejectionReason::AdapterIdentityIncomplete,
                why("no adapter binary identity is declared".into()),
            );
        };
        if !is_normalized_workspace_path(&adapter.binary_path) {
            return rejection(
                RejectionReason::AdapterIdentityIncomplete,
                why(format!(
                    "adapter binary path {:?} is not an absolute normalized path",
                    adapter.binary_path
                )),
            );
        }
        if !is_exact_digest(&adapter.hash) {
            return rejection(
                RejectionReason::AdapterIdentityIncomplete,
                why(format!(
                    "adapter binary hash {:?} is not an exact sha256 digest",
                    adapter.hash
                )),
            );
        }
        if adapter.version.trim().is_empty() || adapter.target.trim().is_empty() {
            return rejection(
                RejectionReason::AdapterIdentityIncomplete,
                why("adapter binary version and target must both be named".into()),
            );
        }

        let (subject_architecture, subject_libc) = match self.install_mode {
            InstallMode::ProjectImage => {
                let image = self.image.as_ref().ok_or_else(|| {
                    Rejection::new(
                        RejectionReason::ImageIdentityNotExact,
                        why("project_image requires an exact project image identity".into()),
                    )
                })?;
                (image.architecture.as_str(), image.libc.as_str())
            }
            InstallMode::InjectedTool => {
                let artifact = self.artifact.as_ref().ok_or_else(|| {
                    Rejection::new(
                        RejectionReason::InjectionSourceUnbound,
                        why("injected_tool requires an exact injected artifact description".into()),
                    )
                })?;
                (artifact.target_arch.as_str(), artifact.libc.as_str())
            }
        };
        if subject_architecture != self.loader.architecture {
            return rejection(
                RejectionReason::LoaderContractMismatch,
                why(format!(
                    "loader contract architecture {:?} does not match subject architecture {:?}",
                    self.loader.architecture, subject_architecture
                )),
            );
        }
        if subject_libc != self.loader.libc {
            return rejection(
                RejectionReason::LoaderContractMismatch,
                why(format!(
                    "loader contract libc {:?} does not match subject libc {:?}",
                    self.loader.libc, subject_libc
                )),
            );
        }
        if !self.loader.container_matches {
            return rejection(
                RejectionReason::LoaderContractMismatch,
                why("loader contract does not match the subject container".into()),
            );
        }

        match self.resource_profile.as_deref() {
            None => {
                return rejection(
                    RejectionReason::ResourceProfileMissing,
                    why("no named resource profile is declared".into()),
                );
            }
            Some(value) if value.trim().is_empty() || value == "unspecified" => {
                return rejection(
                    RejectionReason::ResourceProfileMissing,
                    why("resource profile is unspecified".into()),
                );
            }
            Some(_) => {}
        }

        match self.cleanup_owner.as_deref() {
            None => {
                return rejection(
                    RejectionReason::CleanupOwnershipMissing,
                    why("no process-tree/pod cleanup owner is declared".into()),
                );
            }
            Some(owner) if owner.trim().is_empty() => {
                return rejection(
                    RejectionReason::CleanupOwnershipMissing,
                    why("cleanup owner is empty".into()),
                );
            }
            Some(_) => {}
        }

        let security = &self.security;
        let missing_security_fact = security.run_as_non_root.is_none()
            || security.uid.is_none()
            || security.gid.is_none()
            || security.service_account_token.is_none()
            || security.kubernetes_credentials.is_none()
            || security.network_listener.is_none()
            || security.writable_paths_declared.is_none()
            || security.secrets_redacted_from_receipts.is_none()
            || security.adapter_selection_isolation.is_none();
        if missing_security_fact {
            return rejection(
                RejectionReason::SecurityContextMissing,
                why("security disposition facts are missing and are never defaulted to a pass"
                    .into()),
            );
        }
        if security.run_as_non_root != Some(true)
            || security.uid == Some(0)
            || security.gid == Some(0)
        {
            return rejection(
                RejectionReason::SecurityContextMissing,
                why("the subject must run as an explicit non-root UID/GID".into()),
            );
        }
        for (name, value) in [
            ("writable_paths_declared", security.writable_paths_declared),
            ("secrets_redacted_from_receipts", security.secrets_redacted_from_receipts),
            ("adapter_selection_isolation", security.adapter_selection_isolation),
        ] {
            if value == Some(false) {
                return rejection(
                    RejectionReason::SecurityContextMissing,
                    why(format!("security disposition {name} is explicitly false")),
                );
            }
        }
        if security.service_account_token == Some(ServiceAccountToken::MountedUsed) {
            return rejection(
                RejectionReason::ServiceAccountTokenForbidden,
                why("the ServiceAccount token is mounted and used".into()),
            );
        }
        if security.kubernetes_credentials == Some(true) {
            return rejection(
                RejectionReason::KubernetesApiDependencyForbidden,
                why("Kubernetes credentials reach the adapter/debuggee".into()),
            );
        }

        let mut claimed_ids = BTreeMap::new();
        for claim in &self.dap_claims {
            if claimed_ids.insert(claim.cell_id.as_str(), ()).is_some() {
                return rejection(
                    RejectionReason::DapCellEvidenceMissing,
                    why(format!("cell {:?} is claimed more than once", claim.cell_id)),
                );
            }
            let Some(row) = contract.dap_cells.iter().find(|row| row.cell_id == claim.cell_id)
            else {
                return rejection(
                    RejectionReason::DapCellEvidenceMissing,
                    why(format!("claim references unknown DAP cell {:?}", claim.cell_id)),
                );
            };
            let mut claimed_evidence = BTreeMap::new();
            for item in &claim.evidence {
                if claimed_evidence.insert(item.as_str(), ()).is_some()
                    || item.trim().is_empty()
                    || !row.evidence.iter().any(|expected| expected == item)
                {
                    return rejection(
                        RejectionReason::DapCellEvidenceMissing,
                        why(format!(
                            "cell {:?} cites unsupported, empty, or duplicate evidence {:?}",
                            claim.cell_id, item
                        )),
                    );
                }
            }
            if claim.claimed_state == CellState::EvidenceBacked
                && (row.family != CellFamily::InitialAdmitted
                    || row.state != CellState::EvidenceBacked
                    || claim.evidence.is_empty()
                    || claim.evidence.iter().any(|item| item.trim().is_empty()))
            {
                return rejection(
                    RejectionReason::DapCellEvidenceMissing,
                    why(format!(
                        "cell {:?} claims evidence-backed support outside the admitted family or without profile-specific evidence",
                        claim.cell_id
                    )),
                );
            }
        }
        for row in contract.dap_cells.iter().filter(|row| {
            row.family == CellFamily::InitialAdmitted && row.state == CellState::EvidenceBacked
        }) {
            let Some(claim) = self.dap_claims.iter().find(|claim| claim.cell_id == row.cell_id)
            else {
                return rejection(
                    RejectionReason::DapCellEvidenceMissing,
                    why(format!("required DAP cell {:?} is not claimed", row.cell_id)),
                );
            };
            if claim.claimed_state != CellState::EvidenceBacked
                || claim.evidence.is_empty()
                || claim.evidence.iter().any(|item| item.trim().is_empty())
            {
                return rejection(
                    RejectionReason::DapCellEvidenceMissing,
                    why(format!(
                        "required DAP cell {:?} must claim evidence_backed with non-empty evidence",
                        row.cell_id
                    )),
                );
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Fixture documents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Expectation {
    Admit,
    Reject,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FixtureDocument {
    schema_version: String,
    fixture_id: String,
    expectation: Expectation,
    expected_rejection: Option<RejectionReason>,
    profile: ProfileDocument,
}

impl FixtureDocument {
    fn from_str(source: &str) -> Result<Self> {
        toml::from_str(source).context("parse profile fixture document")
    }

    fn validate_shape(&self) -> Result<()> {
        if self.schema_version != FIXTURE_SCHEMA_VERSION {
            bail!(
                "fixture {:?} has schema {:?}; expected {:?}",
                self.fixture_id,
                self.schema_version,
                FIXTURE_SCHEMA_VERSION
            );
        }
        if !is_non_empty(&self.fixture_id) {
            bail!("fixtures need non-empty fixture ids");
        }
        match (self.expectation, self.expected_rejection) {
            (Expectation::Reject, Some(_)) => {}
            (Expectation::Reject, None) => bail!(
                "negative fixture {:?} must name its expected typed rejection reason",
                self.fixture_id
            ),
            (Expectation::Admit, Some(reason)) => bail!(
                "positive fixture {:?} cannot expect rejection {}",
                self.fixture_id,
                reason.as_str()
            ),
            (Expectation::Admit, None) => {}
        }
        Ok(())
    }

    /// Evaluate against the contract and enforce the declared expectation.
    fn verify_against(&self, contract: &ProfileContract) -> Result<()> {
        self.validate_shape()?;
        match (self.expectation, self.profile.evaluate(contract)) {
            (Expectation::Admit, Ok(())) => Ok(()),
            (Expectation::Admit, Err(rejection)) => {
                bail!("positive fixture {:?} was rejected: {rejection}", self.fixture_id)
            }
            (Expectation::Reject, Ok(())) => bail!(
                "negative fixture {:?} was admitted although it expects rejection",
                self.fixture_id
            ),
            (Expectation::Reject, Err(rejection)) => {
                let expected = self.expected_rejection.ok_or_else(|| {
                    anyhow!("negative fixture {:?} lacks an expected reason", self.fixture_id)
                })?;
                if rejection.reason != expected {
                    bail!(
                        "negative fixture {:?} rejected with {} instead of expected {}: {rejection}",
                        self.fixture_id,
                        rejection.reason.as_str(),
                        expected.as_str()
                    );
                }
                Ok(())
            }
        }
    }
}

fn load_fixtures(
    fixtures_dir: &Path,
    fixture_schema: &jsonschema::Validator,
) -> Result<Vec<FixtureDocument>> {
    let entries =
        fs::read_dir(fixtures_dir).with_context(|| format!("read {}", fixtures_dir.display()))?;
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        // An unreadable directory entry must fail the gate rather than silently
        // shrink the fixture set: a partial matrix would still satisfy every
        // coverage check below.
        let entry = entry.with_context(|| format!("read entry in {}", fixtures_dir.display()))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        bail!("fixture directory {} contains no .toml fixtures", fixtures_dir.display());
    }
    let mut fixtures = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path)
            .with_context(|| format!("read fixture {}", path.display()))?;
        first_schema_error(
            fixture_schema,
            &toml_as_json(&source, "fixture")?,
            &format!("fixture {}", path.display()),
        )?;
        let fixture = FixtureDocument::from_str(&source)
            .with_context(|| format!("load fixture {}", path.display()))?;
        // The generated status names fixtures by `fixture_id`; binding it to the
        // file stem keeps those rows pointing at their source files after a
        // rename.
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("fixture path {} has no readable file stem", path.display()))?;
        if fixture.fixture_id != stem {
            bail!(
                "fixture {} declares id {:?}, which does not match its file stem {:?}",
                path.display(),
                fixture.fixture_id,
                stem
            );
        }
        fixtures.push(fixture);
    }
    let mut ids = BTreeMap::new();
    for fixture in &fixtures {
        if ids.insert(fixture.fixture_id.as_str(), ()).is_some() {
            bail!("duplicate fixture id {:?}", fixture.fixture_id);
        }
    }
    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Status rendering
// ---------------------------------------------------------------------------

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render kubernetes DAP profile status"))
}

fn outcome_label(
    fixture: &FixtureDocument,
    exercised: &mut BTreeMap<&'static str, usize>,
) -> Result<String> {
    fixture.validate_shape()?;
    Ok(match (fixture.expectation, fixture.expected_rejection) {
        (Expectation::Admit, None) => "admit".to_string(),
        (Expectation::Reject, Some(reason)) => {
            *exercised.entry(reason.as_str()).or_default() += 1;
            format!("reject `{}`", reason.as_str())
        }
        _ => bail!("fixture {:?} has an invalid expectation shape", fixture.fixture_id),
    })
}

fn render_status(contract: &ProfileContract, fixtures: &[FixtureDocument]) -> Result<String> {
    contract.validate()?;
    let mut output = String::new();

    line(&mut output, "# Kubernetes DAP Workspace Profiles")?;
    line(&mut output, "")?;
    line(
        &mut output,
        "> Generated by `cargo run -p xtask --bin kubernetes-dap-profiles -- --write-status`.",
    )?;
    line(
        &mut output,
        "> Check with `cargo run -p xtask --bin kubernetes-dap-profiles -- --check`.",
    )?;
    line(&mut output, "")?;
    line(&mut output, &contract.coverage_scope)?;
    line(&mut output, "")?;
    line(&mut output, &format!("- Schema: `{}`", contract.schema_version))?;
    line(&mut output, &format!("- Contract: `{}`", contract.contract_id))?;
    line(&mut output, &format!("- Controller: {}", contract.controller_issue))?;
    line(&mut output, &format!("- Parent train: {}", contract.parent_train))?;
    line(
        &mut output,
        &format!("- Environment subject authority: {}", contract.environment_subject_authority),
    )?;
    line(
        &mut output,
        &format!("- Support registry authority: {}", contract.support_registry_authority),
    )?;
    line(&mut output, "")?;
    line(&mut output, &format!("**Claim boundary:** {}", contract.claim_boundary))?;
    line(&mut output, "")?;

    line(&mut output, "## Admitted profiles")?;
    line(&mut output, "")?;
    line(&mut output, "| Profile | Install mode | Required facts |")?;
    line(&mut output, "| --- | --- | ---: |")?;
    for profile in &contract.admitted_profiles {
        line(
            &mut output,
            &format!(
                "| `{}` | `{}` | {} |",
                profile.profile_id,
                profile.install_mode.as_str(),
                profile.required_facts.len()
            ),
        )?;
    }
    line(&mut output, "")?;

    line(&mut output, "## Rejected topologies")?;
    line(&mut output, "")?;
    line(&mut output, "| Topology | Typed rejection |")?;
    line(&mut output, "| --- | --- |")?;
    for topology in &contract.rejected_topologies {
        line(
            &mut output,
            &format!("| `{}` | `{}` |", topology.topology_id, topology.reason_code.as_str()),
        )?;
    }
    line(&mut output, "")?;

    line(&mut output, "## DAP claim projection")?;
    line(&mut output, "")?;
    line(&mut output, "| Cell | Family | State | Evidence |")?;
    line(&mut output, "| --- | --- | --- | --- |")?;
    for cell in &contract.dap_cells {
        let evidence = if cell.evidence.is_empty() {
            "—".to_string()
        } else {
            cell.evidence.iter().map(|item| format!("`{item}`")).collect::<Vec<_>>().join(", ")
        };
        line(
            &mut output,
            &format!(
                "| `{}` | `{}` | `{}` | {evidence} |",
                cell.cell_id,
                cell.family.as_str(),
                cell.state.as_str()
            ),
        )?;
    }
    line(&mut output, "")?;

    line(&mut output, "## Security and ownership requirements")?;
    line(&mut output, "")?;
    for requirement in &contract.security_requirements {
        line(
            &mut output,
            &format!("- **`{}`:** {}", requirement.requirement_id, requirement.statement),
        )?;
    }
    line(&mut output, "")?;

    line(&mut output, "## Limitations")?;
    line(&mut output, "")?;
    for limitation in &contract.limitations {
        line(&mut output, &format!("- {}", limitation.statement))?;
    }
    line(&mut output, "")?;

    line(&mut output, "## Fixture matrix")?;
    line(&mut output, "")?;
    let positives =
        fixtures.iter().filter(|fixture| fixture.expectation == Expectation::Admit).count();
    let negatives = fixtures.len() - positives;
    line(
        &mut output,
        &format!("{positives} positive and {negatives} negative deterministic fixtures."),
    )?;
    line(&mut output, "")?;
    line(&mut output, "| Fixture | Expectation | Typed outcome |")?;
    line(&mut output, "| --- | --- | --- |")?;
    let mut exercised = BTreeMap::<&'static str, usize>::new();
    for fixture in fixtures {
        let outcome = outcome_label(fixture, &mut exercised)?;
        let expectation =
            if fixture.expectation == Expectation::Admit { "`admit`" } else { "`reject`" };
        line(&mut output, &format!("| `{}` | {expectation} | {outcome} |", fixture.fixture_id))?;
    }
    line(&mut output, "")?;
    let uncovered: Vec<&'static str> = RejectionReason::ALL
        .iter()
        .map(|reason| reason.as_str())
        .filter(|code| !exercised.contains_key(code))
        .collect();
    if !uncovered.is_empty() {
        bail!("typed rejection reasons without a negative fixture: {uncovered:?}");
    }
    line(
        &mut output,
        &format!(
            "All {} typed rejection reasons are exercised by at least one negative fixture.",
            RejectionReason::ALL.len()
        ),
    )?;
    line(&mut output, "")?;

    Ok(output.trim_end_matches('\n').to_owned() + "\n")
}

fn validate_published_schema_values(
    profile_schema: &serde_json::Value,
    fixture_schema: &serde_json::Value,
) -> Result<()> {
    let expected_reasons = RejectionReason::ALL
        .iter()
        .map(|reason| serde_json::Value::String(reason.as_str().to_string()))
        .collect::<Vec<_>>();
    let published_reasons = profile_schema
        .pointer("/$defs/reason_code/enum")
        .and_then(serde_json::Value::as_array)
        .context("profile schema must publish the reason_code enum")?;
    if *published_reasons != expected_reasons {
        bail!("profile schema reason_code enum diverges from typed rejection reasons");
    }
    if profile_schema
        .pointer("/properties/schema_version/const")
        .and_then(serde_json::Value::as_str)
        != Some(CONTRACT_SCHEMA_VERSION)
    {
        bail!("profile schema schema_version const is incorrect");
    }
    if profile_schema
        .pointer("/$defs/source_namespace/properties/equality/const")
        .and_then(serde_json::Value::as_str)
        != Some("client == adapter == debuggee")
    {
        bail!("profile schema source namespace equality must be an exact const");
    }
    for definition in ["image_identity", "injected_artifact"] {
        let required = fixture_schema
            .pointer(&format!("/$defs/{definition}/required"))
            .and_then(serde_json::Value::as_array)
            .context("fixture schema identity definition must list required fields")?;
        if !required.iter().any(|value| value.as_str() == Some("libc")) {
            bail!("fixture schema {definition} must require libc");
        }
    }
    if fixture_schema
        .pointer("/properties/schema_version/const")
        .and_then(serde_json::Value::as_str)
        != Some(FIXTURE_SCHEMA_VERSION)
    {
        bail!("fixture schema schema_version const is incorrect");
    }
    let expected_ref = fixture_schema
        .pointer("/properties/expected_rejection/$ref")
        .and_then(serde_json::Value::as_str)
        .context("fixture schema expected_rejection must reference reason_code")?;
    if expected_ref != "kubernetes_dap_workspace_profile.v1.schema.json#/$defs/reason_code"
        || profile_schema.pointer("/$defs/reason_code").is_none()
    {
        bail!("fixture schema expected_rejection must resolve to profile reason_code");
    }
    let expected_conditional = serde_json::json!({
        "allOf": [
            {
                "if": {"properties": {"expectation": {"const": "reject"}}},
                "then": {"required": ["expected_rejection"]}
            },
            {
                "if": {"properties": {"expectation": {"const": "admit"}}},
                "then": {"not": {"required": ["expected_rejection"]}}
            }
        ]
    });
    if fixture_schema.get("allOf") != expected_conditional.get("allOf") {
        bail!("fixture schema must publish the expectation/expected_rejection conditional");
    }
    Ok(())
}

/// The published schemas, compiled so that documents are validated against them
/// rather than only inspected field by field.
///
/// `validate_published_schema_values` ratchets the parts of the schemas that no
/// document can prove — enum parity with `RejectionReason::ALL`, the schema
/// version consts, the exact source-equality const, the expectation
/// conditional. Everything else is proven the honest way: by validating the
/// committed contract and every committed fixture against the shapes we publish
/// to external consumers, so ordinary schema drift fails the gate instead of
/// passing it.
struct PublishedSchemas {
    contract: jsonschema::Validator,
    fixture: jsonschema::Validator,
}

/// Render the first schema error only. `jsonschema` reports every failure, but
/// a gate message that names one exact instance path and reason is what a
/// contributor acts on.
fn first_schema_error(
    validator: &jsonschema::Validator,
    instance: &serde_json::Value,
    subject: &str,
) -> Result<()> {
    if let Some(error) = validator.iter_errors(instance).next() {
        bail!(
            "{subject} does not satisfy its published schema at `{}`: {error}",
            error.instance_path()
        );
    }
    Ok(())
}

fn toml_as_json(source: &str, subject: &str) -> Result<serde_json::Value> {
    toml::from_str::<serde_json::Value>(source)
        .with_context(|| format!("read {subject} as a JSON document for schema validation"))
}

fn validate_published_schemas(
    profile_path: &Path,
    fixture_path: &Path,
) -> Result<PublishedSchemas> {
    let profile_source = fs::read_to_string(profile_path)
        .with_context(|| format!("read profile schema {}", profile_path.display()))?;
    let fixture_source = fs::read_to_string(fixture_path)
        .with_context(|| format!("read fixture schema {}", fixture_path.display()))?;
    let profile_schema: serde_json::Value = serde_json::from_str(&profile_source)
        .with_context(|| format!("parse profile schema {}", profile_path.display()))?;
    let fixture_schema: serde_json::Value = serde_json::from_str(&fixture_source)
        .with_context(|| format!("parse fixture schema {}", fixture_path.display()))?;
    validate_published_schema_values(&profile_schema, &fixture_schema)?;

    // The fixture schema resolves `expected_rejection` through a relative
    // reference to the profile schema, so the profile schema is registered
    // under its own `$id` rather than fetched.
    let profile_id = profile_schema
        .get("$id")
        .and_then(serde_json::Value::as_str)
        .context("profile schema must declare $id so relative references resolve")?
        .to_string();
    let contract = jsonschema::validator_for(&profile_schema)
        .map_err(|error| anyhow!("compile profile schema {}: {error}", profile_path.display()))?;
    let registry = jsonschema::Registry::new()
        .add(profile_id, profile_schema)
        .and_then(jsonschema::RegistryBuilder::prepare)
        .map_err(|error| {
            anyhow!("register the profile schema for reference resolution: {error}")
        })?;
    let fixture = jsonschema::options()
        .with_registry(&registry)
        .build(&fixture_schema)
        .map_err(|error| anyhow!("compile fixture schema {}: {error}", fixture_path.display()))?;
    Ok(PublishedSchemas { contract, fixture })
}

/// Every admitted profile row must keep exactly one admitting fixture.
///
/// The fixture matrix already refuses to lose a typed rejection reason, but
/// nothing required the positives: deleting both and regenerating the status
/// left the required gate green with no proof that anything is admissible at
/// all. A contract that rejects everything is not the claim this contract makes.
fn verify_admission_proof(contract: &ProfileContract, fixtures: &[FixtureDocument]) -> Result<()> {
    for row in &contract.admitted_profiles {
        let admitting: Vec<&str> = fixtures
            .iter()
            .filter(|fixture| {
                fixture.expectation == Expectation::Admit
                    && fixture.profile.install_mode == row.install_mode
            })
            .map(|fixture| fixture.fixture_id.as_str())
            .collect();
        match admitting.len() {
            1 => {}
            0 => bail!(
                "admitted profile {:?} ({}) has no admitting fixture; the gate would pass \
                 without proving anything is admissible",
                row.profile_id,
                row.install_mode.as_str()
            ),
            _ => bail!(
                "admitted profile {:?} ({}) has {} admitting fixtures {:?}; exactly one \
                 positive keeps the admitted shape single-authored",
                row.profile_id,
                row.install_mode.as_str(),
                admitting.len(),
                admitting
            ),
        }
    }
    Ok(())
}

fn run(cli: &Cli) -> Result<()> {
    let contract_source = fs::read_to_string(&cli.contract)
        .with_context(|| format!("read contract {}", cli.contract.display()))?;
    let contract = ProfileContract::from_str(&contract_source)?;
    contract.validate()?;
    let schemas = validate_published_schemas(&cli.profile_schema, &cli.fixture_schema)?;
    first_schema_error(
        &schemas.contract,
        &toml_as_json(&contract_source, "contract")?,
        &format!("contract {}", cli.contract.display()),
    )?;
    let fixtures = load_fixtures(&cli.fixtures_dir, &schemas.fixture)?;
    for fixture in &fixtures {
        fixture.verify_against(&contract)?;
    }
    verify_admission_proof(&contract, &fixtures)?;

    let rendered = render_status(&contract, &fixtures)?;

    if cli.write_status {
        if let Some(parent) = cli.status.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create status directory {}", parent.display()))?;
        }
        fs::write(&cli.status, &rendered)
            .with_context(|| format!("write status {}", cli.status.display()))?;
        println!(
            "kubernetes-dap-profiles: wrote status {} ({} fixtures)",
            cli.status.display(),
            fixtures.len()
        );
        return Ok(());
    }

    if cli.check {
        let committed = fs::read_to_string(&cli.status)
            .with_context(|| format!("read committed status {}", cli.status.display()))?;
        if committed != rendered {
            bail!("committed status {} is stale; rerun with --write-status", cli.status.display());
        }
    }

    println!(
        "kubernetes-dap-profiles: contract `{}` valid; {} fixtures agree ({} positive, {} negative)",
        contract.contract_id,
        fixtures.len(),
        fixtures.iter().filter(|fixture| fixture.expectation == Expectation::Admit).count(),
        fixtures.iter().filter(|fixture| fixture.expectation == Expectation::Reject).count(),
    );
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(&cli)
}

// ---------------------------------------------------------------------------
// Falsifiers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED_CONTRACT: &str =
        include_str!("../../../contracts/dap/kubernetes_dap_workspace_profile.v1.toml");
    const COMMITTED_STATUS: &str =
        include_str!("../../../docs/project/status/kubernetes_dap_workspace_profiles.md");
    const COMMITTED_SCHEMA: &str =
        include_str!("../../../schemas/kubernetes_dap_workspace_profile.v1.schema.json");
    const COMMITTED_FIXTURE_SCHEMA: &str =
        include_str!("../../../schemas/kubernetes_dap_workspace_profile_fixture.v1.schema.json");

    const POSITIVE_PROJECT_IMAGE: &str = "positive-project-image";
    const POSITIVE_INJECTED_TOOL: &str = "positive-injected-tool";

    fn committed_contract() -> Result<ProfileContract> {
        let contract = ProfileContract::from_str(COMMITTED_CONTRACT)?;
        contract.validate()?;
        Ok(contract)
    }

    fn repository_root() -> Result<&'static Path> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| anyhow!("cannot derive repository root from the xtask manifest"))
    }

    fn committed_schemas() -> Result<PublishedSchemas> {
        let root = repository_root()?;
        validate_published_schemas(
            &root.join(DEFAULT_PROFILE_SCHEMA),
            &root.join(DEFAULT_FIXTURE_SCHEMA),
        )
    }

    fn committed_fixtures() -> Result<Vec<FixtureDocument>> {
        let fixtures_dir = repository_root()?.join(DEFAULT_FIXTURES_DIR);
        load_fixtures(&fixtures_dir, &committed_schemas()?.fixture)
    }

    fn find_fixture<'a>(fixtures: &'a [FixtureDocument], id: &str) -> Result<&'a FixtureDocument> {
        fixtures
            .iter()
            .find(|fixture| fixture.fixture_id == id)
            .ok_or_else(|| anyhow!("missing fixture {id}"))
    }

    fn evaluate(profile: &ProfileDocument) -> Result<(), anyhow::Error> {
        let contract = committed_contract()?;
        profile.evaluate(&contract).map_err(Into::into)
    }

    fn assert_rejected_with(profile: &ProfileDocument, expected: RejectionReason) -> Result<()> {
        match evaluate(profile) {
            Err(error) => match error.downcast::<Rejection>() {
                Ok(rejection) => {
                    assert_eq!(
                        rejection.reason,
                        expected,
                        "expected rejection {}, got: {rejection}",
                        expected.as_str()
                    );
                    Ok(())
                }
                Err(other) => bail!("expected a typed rejection, got: {other}"),
            },
            Ok(()) => bail!("expected rejection {} but the profile was admitted", expected),
        }
    }

    #[test]
    fn committed_contract_validates_and_status_is_current() -> Result<()> {
        let contract = committed_contract()?;
        let fixtures = committed_fixtures()?;
        assert_eq!(render_status(&contract, &fixtures)?, COMMITTED_STATUS);
        Ok(())
    }

    #[test]
    fn committed_fixtures_all_agree_with_expectations() -> Result<()> {
        let contract = committed_contract()?;
        let fixtures = committed_fixtures()?;
        assert!(fixtures.len() >= 30, "fixture matrix unexpectedly small");
        for fixture in &fixtures {
            fixture.verify_against(&contract)?;
        }
        Ok(())
    }

    #[test]
    fn every_required_topology_code_is_registered_once() -> Result<()> {
        let contract = committed_contract()?;
        let mut seen = std::collections::BTreeSet::new();
        for topology in &contract.rejected_topologies {
            assert!(seen.insert(topology.reason_code.as_str()));
        }
        for required in REQUIRED_TOPOLOGY_CODES {
            assert!(
                seen.contains(required.as_str()),
                "registry lost required code {}",
                required.as_str()
            );
        }
        Ok(())
    }

    #[test]
    fn every_typed_rejection_reason_has_a_negative_fixture() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let mut covered = std::collections::BTreeSet::new();
        for fixture in &fixtures {
            if let Some(reason) = fixture.expected_rejection {
                covered.insert(reason.as_str());
            }
        }
        for reason in RejectionReason::ALL {
            assert!(
                covered.contains(reason.as_str()),
                "no negative fixture exercises {}",
                reason.as_str()
            );
        }
        Ok(())
    }

    // Negative control 10: one profile satisfies the other by inference.
    #[test]
    fn positive_profiles_are_independent() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let project_image = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let injected_tool = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        assert!(evaluate(&project_image.profile).is_ok());
        assert!(evaluate(&injected_tool.profile).is_ok());
        let mut broken_project_image = project_image.profile.clone();
        if let Some(image) = broken_project_image.image.as_mut() {
            image.digest = "sha256:00".into();
        }
        assert!(evaluate(&broken_project_image).is_err());
        assert!(
            evaluate(&injected_tool.profile).is_ok(),
            "mutating the project_image profile must not affect the injected_tool verdict"
        );
        Ok(())
    }

    // Negative control 1: LSP profile success never creates a DAP profile pass.
    #[test]
    fn lsp_profile_reference_never_admits() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        let mut inheriting = base.profile.clone();
        inheriting.lsp_profile_binding = Some(LspProfileBinding {
            inherits_lsp_profile: "kubernetes-lsp-workspace-profile-v1".into(),
        });
        assert_rejected_with(&inheriting, RejectionReason::LspProfileProjectionForbidden)?;
        inheriting.image = None;
        inheriting.security.service_account_token = None;
        assert!(
            evaluate(&inheriting).is_err(),
            "an LSP binding plus missing facts still never admits"
        );
        Ok(())
    }

    // Negative control 2: a generic sidecar is not project-environment parity.
    #[test]
    fn pod_sharing_sidecar_is_not_environment_parity() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        let mut sided = base.profile.clone();
        sided.sidecar = Some(SidecarProjection {
            shares_filesystem: true,
            perl_authority: PerlAuthority::ProjectContainer,
        });
        assert_rejected_with(&sided, RejectionReason::SidecarEnvironmentMismatch)
    }

    // Negative control 3: a rewrite cannot hide a source namespace difference.
    #[test]
    fn rewrite_cannot_hide_namespace_difference() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, "negative-source-other-absolute-path")?;
        let mut hidden = base.profile.clone();
        hidden.path_rewrite = Some(PathRewrite {
            table: vec![PathRewriteEntry {
                from: "/home/dev/project".into(),
                to: "/workspace".into(),
            }],
        });
        assert_rejected_with(&hidden, RejectionReason::EditorPathTranslationForbidden)?;
        assert!(evaluate(&base.profile).is_err(), "the unhidden mismatch stays rejected");
        Ok(())
    }

    // Negative control 4: identities must be exact.
    #[test]
    fn tag_or_short_digest_image_identity_fails() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mut floating = base.profile.clone();
        if let Some(image) = floating.image.as_mut() {
            image.digest = "registry.example/acme/dev:latest".into();
        }
        assert_rejected_with(&floating, RejectionReason::ImageIdentityNotExact)?;
        let mut truncated = base.profile.clone();
        if let Some(image) = truncated.image.as_mut() {
            image.digest = "sha256:abcd".into();
        }
        assert_rejected_with(&truncated, RejectionReason::ImageIdentityNotExact)
    }

    // Negative control 5: the copy must be verified.
    #[test]
    fn unverified_copy_rejects_even_with_matching_digests() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        let mut unverified = base.profile.clone();
        if let Some(artifact) = unverified.artifact.as_mut() {
            artifact.post_copy_verified = false;
        }
        assert_rejected_with(&unverified, RejectionReason::ArtifactDigestUnverified)
    }

    // Negative control 6: init-image Perl never satisfies the project row.
    #[test]
    fn init_image_perl_cannot_satisfy_project_row() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        let mut substituted = base.profile.clone();
        substituted.perl.authority = PerlAuthority::InitImage;
        assert_rejected_with(&substituted, RejectionReason::InitImagePerlSubstitutionForbidden)
    }

    // Negative control 7: one Service field rejects an otherwise clean profile.
    #[test]
    fn single_service_field_rejects_clean_profile() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mut exposed = base.profile.clone();
        exposed.service_exposure = Some(ServiceExposure { kind: ServiceKind::LoadBalancer });
        assert_rejected_with(&exposed, RejectionReason::ServiceExposureForbidden)
    }

    // Negative control 8: optional capabilities never inherit static catalogs.
    #[test]
    fn catalog_backed_capability_claim_is_rejected() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mut catalogued = base.profile.clone();
        catalogued.capability_catalog =
            Some(CapabilityCatalog { static_catalog: "dap-capability-catalog-2026-08".into() });
        assert_rejected_with(&catalogued, RejectionReason::CapabilityCatalogInheritanceForbidden)?;

        let mut optional_claim = base.profile.clone();
        optional_claim.dap_claims.push(DapCellClaim {
            cell_id: "evaluate_repl".into(),
            claimed_state: CellState::EvidenceBacked,
            evidence: vec!["static catalog row".into()],
        });
        assert_rejected_with(&optional_claim, RejectionReason::DapCellEvidenceMissing)?;

        let mut unknown_cell = base.profile.clone();
        unknown_cell.dap_claims.push(DapCellClaim {
            cell_id: "attach_arbitrary_pod".into(),
            claimed_state: CellState::EvidenceBacked,
            evidence: vec!["somewhere".into()],
        });
        assert_rejected_with(&unknown_cell, RejectionReason::DapCellEvidenceMissing)
    }

    // Negative control 9: missing facts are never defaulted to a pass.
    #[test]
    fn missing_cleanup_resource_and_security_facts_are_not_defaulted() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;

        let mut no_cleanup = base.profile.clone();
        no_cleanup.cleanup_owner = None;
        assert_rejected_with(&no_cleanup, RejectionReason::CleanupOwnershipMissing)?;

        let mut no_resources = base.profile.clone();
        no_resources.resource_profile = Some("unspecified".into());
        assert_rejected_with(&no_resources, RejectionReason::ResourceProfileMissing)?;

        let mut sparse_security = base.profile.clone();
        sparse_security.security.writable_paths_declared = None;
        assert_rejected_with(&sparse_security, RejectionReason::SecurityContextMissing)
    }

    #[test]
    fn unknown_profile_field_fails_closed_at_parse() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let source = toml::to_string(&base.profile)?;
        assert!(!source.is_empty(), "round-trip serialization must work");
        let mutated = format!("unknown_carrier = true\n\n{source}");
        let parsed = toml::from_str::<ProfileDocument>(&mutated);
        assert!(parsed.is_err(), "unknown topology carriers must fail closed");
        Ok(())
    }

    #[test]
    fn published_schema_matches_typed_reason_codes() -> Result<()> {
        let schema: serde_json::Value =
            serde_json::from_str(COMMITTED_SCHEMA).context("parse committed schema")?;
        let fixture_schema: serde_json::Value = serde_json::from_str(COMMITTED_FIXTURE_SCHEMA)
            .context("parse committed fixture schema")?;
        validate_published_schema_values(&schema, &fixture_schema)?;
        Ok(())
    }

    #[test]
    fn published_schema_missing_reason_is_rejected() -> Result<()> {
        let mut schema: serde_json::Value = serde_json::from_str(COMMITTED_SCHEMA)?;
        schema["$defs"]["reason_code"]["enum"]
            .as_array_mut()
            .context("reason enum must be an array")?
            .pop();
        let fixture_schema: serde_json::Value = serde_json::from_str(COMMITTED_FIXTURE_SCHEMA)?;
        assert!(validate_published_schema_values(&schema, &fixture_schema).is_err());
        Ok(())
    }

    #[test]
    fn published_fixture_schema_missing_conditional_is_rejected() -> Result<()> {
        let profile_schema: serde_json::Value = serde_json::from_str(COMMITTED_SCHEMA)?;
        let mut fixture_schema: serde_json::Value = serde_json::from_str(COMMITTED_FIXTURE_SCHEMA)?;
        fixture_schema.as_object_mut().context("fixture schema must be an object")?.remove("allOf");
        assert!(validate_published_schema_values(&profile_schema, &fixture_schema).is_err());
        Ok(())
    }

    #[test]
    fn fabricated_dap_evidence_is_rejected() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mut fabricated = base.profile.clone();
        fabricated.dap_claims[0].evidence = vec!["fabricated capability evidence".into()];
        assert_rejected_with(&fabricated, RejectionReason::DapCellEvidenceMissing)
    }

    #[test]
    fn contract_header_invariants_are_load_bearing() -> Result<()> {
        let mut contract = committed_contract()?;
        contract.transport_boundary.tty = true;
        assert!(contract.validate().is_err());
        contract.transport_boundary.tty = false;
        contract.source_namespace.rewrite_authority = "#7667-rewrite-table".into();
        assert!(contract.validate().is_err());
        contract.source_namespace.rewrite_authority = "none".into();
        contract.complete = true;
        assert!(contract.validate().is_err());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Adapter binary identity. The contract declared
    // `adapter_binary_path_version_hash_target` as a required fact for the
    // project_image profile and nothing enforced it, so an exact image with no
    // identified debugger was admissible.
    // -----------------------------------------------------------------------

    #[test]
    fn adapter_identity_is_required_for_admission() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mut unnamed = base.profile.clone();
        unnamed.adapter = None;
        assert_rejected_with(&unnamed, RejectionReason::AdapterIdentityIncomplete)
    }

    #[test]
    fn adapter_identity_components_are_each_load_bearing() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?;
        let mutations: Vec<Box<dyn Fn(&mut AdapterIdentity)>> = vec![
            Box::new(|adapter| adapter.binary_path = "perl-dap".into()),
            Box::new(|adapter| adapter.binary_path = String::new()),
            Box::new(|adapter| adapter.hash = "v0.17.0".into()),
            Box::new(|adapter| adapter.hash = "sha256:short".into()),
            Box::new(|adapter| adapter.version = "  ".into()),
            Box::new(|adapter| adapter.target = String::new()),
        ];
        for mutate in mutations {
            let mut profile = base.profile.clone();
            let adapter =
                profile.adapter.as_mut().context("positive fixture must name its adapter")?;
            mutate(adapter);
            assert_rejected_with(&profile, RejectionReason::AdapterIdentityIncomplete)?;
        }
        Ok(())
    }

    /// A digest names bytes; it does not record where they came from.
    #[test]
    fn digest_only_injection_source_is_unbound() -> Result<()> {
        let fixtures = committed_fixtures()?;
        let base = find_fixture(&fixtures, POSITIVE_INJECTED_TOOL)?;
        for prefix in ["", "   "] {
            let mut profile = base.profile.clone();
            let artifact =
                profile.artifact.as_mut().context("injected fixture must name its artifact")?;
            let digest = source_identity_digest(&artifact.source_identity)
                .context("injected fixture source identity must carry a digest")?
                .to_owned();
            artifact.source_identity = format!("{prefix}@{digest}");
            assert_rejected_with(&profile, RejectionReason::InjectionSourceUnbound)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The contract's advertised coverage cannot outrun the gate.
    // -----------------------------------------------------------------------

    #[test]
    fn every_declared_required_fact_has_typed_enforcement() -> Result<()> {
        let contract = committed_contract()?;
        let fixtures = committed_fixtures()?;
        let covered: std::collections::BTreeSet<&str> = fixtures
            .iter()
            .filter_map(|fixture| fixture.expected_rejection)
            .map(RejectionReason::as_str)
            .collect();
        for row in &contract.admitted_profiles {
            for fact in &row.required_facts {
                let reason = enforcement_for_fact(fact)
                    .with_context(|| format!("required fact {fact:?} has no typed enforcement"))?;
                assert!(
                    covered.contains(reason.as_str()),
                    "required fact {fact:?} maps to {} with no negative fixture",
                    reason.as_str()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_required_fact_without_enforcement_is_rejected() -> Result<()> {
        let mut contract = committed_contract()?;
        contract.admitted_profiles[0].required_facts.push("unenforced_new_fact".into());
        assert!(
            contract.validate().is_err(),
            "a declared fact with no admission consequence must fail the contract"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admission proof cannot silently disappear.
    // -----------------------------------------------------------------------

    #[test]
    fn removing_either_positive_fixture_fails_the_gate() -> Result<()> {
        let contract = committed_contract()?;
        let fixtures = committed_fixtures()?;
        verify_admission_proof(&contract, &fixtures)?;
        for removed in [POSITIVE_PROJECT_IMAGE, POSITIVE_INJECTED_TOOL] {
            let remaining: Vec<FixtureDocument> =
                fixtures.iter().filter(|fixture| fixture.fixture_id != removed).cloned().collect();
            assert!(
                verify_admission_proof(&contract, &remaining).is_err(),
                "removing {removed} left the gate green with no admission proof"
            );
        }
        Ok(())
    }

    #[test]
    fn a_second_positive_for_one_install_mode_is_rejected() -> Result<()> {
        let contract = committed_contract()?;
        let mut fixtures = committed_fixtures()?;
        let mut duplicate = find_fixture(&fixtures, POSITIVE_PROJECT_IMAGE)?.clone();
        duplicate.fixture_id = "positive-project-image-alternate".into();
        fixtures.push(duplicate);
        assert!(verify_admission_proof(&contract, &fixtures).is_err());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The published schemas are applied to documents, not only inspected.
    // -----------------------------------------------------------------------

    /// The fixture schema describes *candidate* documents. Negative fixtures
    /// must be able to carry an empty path or an omitted security fact, or the
    /// validator's typed rejections would be unreachable through the published
    /// shape. Re-tightening the shape to describe admitted documents therefore
    /// has to fail against the committed matrix.
    #[test]
    fn candidate_shape_admits_the_committed_negative_fixtures() -> Result<()> {
        let schemas = committed_schemas()?;
        let root = repository_root()?;
        for id in ["negative-empty-workspace-paths", "negative-omitted-security-fact"] {
            let source =
                fs::read_to_string(root.join(DEFAULT_FIXTURES_DIR).join(format!("{id}.toml")))?;
            first_schema_error(&schemas.fixture, &toml_as_json(&source, "fixture")?, id)?;
        }
        Ok(())
    }

    #[test]
    fn published_fixture_schema_rejects_an_ill_typed_document() -> Result<()> {
        let schemas = committed_schemas()?;
        let root = repository_root()?;
        let source = fs::read_to_string(
            root.join(DEFAULT_FIXTURES_DIR).join(format!("{POSITIVE_PROJECT_IMAGE}.toml")),
        )?;
        let mut document = toml_as_json(&source, "fixture")?;
        first_schema_error(&schemas.fixture, &document, POSITIVE_PROJECT_IMAGE)?;
        document
            .pointer_mut("/profile")
            .and_then(serde_json::Value::as_object_mut)
            .context("fixture must carry a profile")?
            .insert("install_mode".into(), serde_json::json!("chart_release"));
        assert!(
            first_schema_error(&schemas.fixture, &document, POSITIVE_PROJECT_IMAGE).is_err(),
            "the published fixture schema must reject an unlisted install mode"
        );
        Ok(())
    }

    #[test]
    fn committed_contract_satisfies_its_published_schema() -> Result<()> {
        let schemas = committed_schemas()?;
        let source = fs::read_to_string(repository_root()?.join(DEFAULT_CONTRACT))?;
        first_schema_error(&schemas.contract, &toml_as_json(&source, "contract")?, "contract")
    }

    // -----------------------------------------------------------------------
    // Generated status rows keep naming their source files.
    // -----------------------------------------------------------------------

    #[test]
    fn a_renamed_fixture_file_fails_the_gate() -> Result<()> {
        let schemas = committed_schemas()?;
        let root = repository_root()?;
        let staging = tempfile::tempdir()?;
        for entry in fs::read_dir(root.join(DEFAULT_FIXTURES_DIR))? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                let name = path.file_name().context("fixture path must have a file name")?;
                fs::copy(&path, staging.path().join(name))?;
            }
        }
        load_fixtures(staging.path(), &schemas.fixture)?;
        fs::rename(
            staging.path().join("negative-ambient-listener.toml"),
            staging.path().join("renamed-fixture.toml"),
        )?;
        assert!(
            load_fixtures(staging.path(), &schemas.fixture).is_err(),
            "a fixture id that no longer matches its file stem must fail the gate"
        );
        Ok(())
    }
}
