//! Deterministic, transport-neutral Perl project environment identity.
//!
//! This module defines the pure data model and precedence compiler used by
//! higher layers to describe which project, interpreter, include-root, build,
//! and tool inputs are active. It performs no discovery, probing, filesystem
//! access, process execution, or provider work.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Digest, fnv1a};

/// Schema version for [`ProjectEnvironmentSnapshot`].
pub const PROJECT_ENVIRONMENT_SCHEMA_VERSION: u32 = 1;

const ENVIRONMENT_INPUT_ID_DOMAIN: &str = "project_environment.input.v1";
const INCLUDE_ENTRY_ID_DOMAIN: &str = "project_environment.include.v1";
const PROJECT_ROOT_ID_DOMAIN: &str = "project_environment.root.v1";
const BUILD_SYSTEM_ID_DOMAIN: &str = "project_environment.build.v1";
const TOOL_CANDIDATE_ID_DOMAIN: &str = "project_environment.tool.v1";

/// Workspace trust disposition carried by an environment snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTrust {
    /// The workspace is explicitly trusted for the supplied project inputs.
    Trusted,
    /// The workspace is explicitly untrusted.
    Untrusted,
    /// Trust has not yet been decided.
    Unknown,
}

impl WorkspaceTrust {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::Unknown => "unknown",
        }
    }
}

/// Authority class for one candidate environment input.
///
/// Variant order is not used as policy. [`Self::precedence_rank`] is the
/// executable precedence contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentInputAuthority {
    /// Explicit user or client configuration.
    UserConfiguration,
    /// Trusted project configuration.
    TrustedProjectConfiguration,
    /// Facts supplied by the selected interpreter probe.
    InterpreterEvidence,
    /// Workspace layout or another reviewed project convention.
    WorkspaceConvention,
    /// Bounded non-executing build metadata.
    BuildMetadata,
    /// Explicitly enabled environment activation.
    ExplicitEnvironment,
    /// Ambient process or host state that has not been authorized.
    Ambient,
}

impl EnvironmentInputAuthority {
    /// Stable precedence rank; lower values have stronger authority.
    #[must_use]
    pub const fn precedence_rank(self) -> u8 {
        match self {
            Self::UserConfiguration => 0,
            Self::TrustedProjectConfiguration => 1,
            Self::InterpreterEvidence => 2,
            Self::WorkspaceConvention => 3,
            Self::BuildMetadata => 4,
            Self::ExplicitEnvironment => 5,
            Self::Ambient => 6,
        }
    }

    const fn identity_tag(self) -> &'static str {
        match self {
            Self::UserConfiguration => "user_configuration",
            Self::TrustedProjectConfiguration => "trusted_project_configuration",
            Self::InterpreterEvidence => "interpreter_evidence",
            Self::WorkspaceConvention => "workspace_convention",
            Self::BuildMetadata => "build_metadata",
            Self::ExplicitEnvironment => "explicit_environment",
            Self::Ambient => "ambient",
        }
    }
}

/// Resolved disposition of an environment input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentInputState {
    /// The input is active project-environment authority.
    Accepted,
    /// The input was denied by trust or policy.
    Denied,
    /// The input was observed only as ambient/advisory state.
    Ambient,
    /// The input could not be obtained.
    Unavailable,
    /// Equally authoritative candidates disagree.
    Conflicting,
    /// A stronger or deterministic equivalent candidate won.
    Superseded,
}

impl EnvironmentInputState {
    /// Whether this input is active authority.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Accepted)
    }

    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Denied => "denied",
            Self::Ambient => "ambient",
            Self::Unavailable => "unavailable",
            Self::Conflicting => "conflicting",
            Self::Superseded => "superseded",
        }
    }

    const fn sort_rank(self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Denied => 1,
            Self::Ambient => 2,
            Self::Unavailable => 3,
            Self::Conflicting => 4,
            Self::Superseded => 5,
        }
    }
}

/// Stable identity of one environment input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnvironmentInputId(String);

impl EnvironmentInputId {
    /// String form of the identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EnvironmentInputId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One candidate input before and after deterministic precedence resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentInput {
    /// Stable identity derived from the candidate's semantic evidence.
    pub id: EnvironmentInputId,
    /// Logical slot governed by precedence, such as `interpreter.selected`.
    pub semantic_key: String,
    /// Authority class.
    pub authority: EnvironmentInputAuthority,
    /// Resolved state.
    pub state: EnvironmentInputState,
    /// Stable source identifier supplied by the producer.
    pub source_id: String,
    /// Fingerprint of the behavior-bearing value, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_fingerprint: Option<Digest>,
    /// Stable explanation code for the initial policy decision.
    pub explanation_code: String,
}

impl EnvironmentInput {
    /// Create one candidate input.
    ///
    /// The builder may change `Accepted` candidates to `Superseded` or
    /// `Conflicting` while preserving this identity.
    #[must_use]
    pub fn new(
        semantic_key: impl Into<String>,
        authority: EnvironmentInputAuthority,
        state: EnvironmentInputState,
        source_id: impl Into<String>,
        value_fingerprint: Option<Digest>,
        explanation_code: impl Into<String>,
    ) -> Self {
        let semantic_key = semantic_key.into();
        let source_id = source_id.into();
        let explanation_code = explanation_code.into();
        let fingerprint = value_fingerprint.as_ref().map_or("", Digest::as_str);
        let id = EnvironmentInputId(stable_id(
            ENVIRONMENT_INPUT_ID_DOMAIN,
            &[
                semantic_key.as_str(),
                authority.identity_tag(),
                state.identity_tag(),
                source_id.as_str(),
                fingerprint,
                explanation_code.as_str(),
            ],
        ));
        Self { id, semantic_key, authority, state, source_id, value_fingerprint, explanation_code }
    }
}

/// Internal normalized path plus a safe public identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentPathRef {
    /// Normalized path used by trusted internal consumers.
    pub normalized: String,
    /// Stable redacted identifier used by public receipts.
    pub public_id: String,
}

impl EnvironmentPathRef {
    /// Construct a path reference from caller-normalized material.
    #[must_use]
    pub fn new(normalized: impl Into<String>, public_id: impl Into<String>) -> Self {
        Self { normalized: normalized.into(), public_id: public_id.into() }
    }
}

/// Logical role of an include-root candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludeEntryRole {
    /// Default workspace root.
    WorkspaceDefault,
    /// Explicit workspace-configured root.
    WorkspaceConfigured,
    /// Source-ordered `use lib` root.
    LexicalUseLib,
    /// Root derived from a reviewed FindBin expression.
    FindBinDerived,
    /// Explicitly enabled `PERL5LIB` root.
    Perl5Lib,
    /// Selected interpreter startup `@INC` root.
    InterpreterStartup,
    /// local::lib root.
    LocalLib,
    /// Generated `blib/lib` root.
    BlibLib,
    /// Generated `blib/arch` root.
    BlibArch,
    /// Vendor root.
    Vendor,
    /// Other generated root.
    Generated,
    /// Other reviewed root role.
    Other,
}

impl IncludeEntryRole {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::WorkspaceDefault => "workspace_default",
            Self::WorkspaceConfigured => "workspace_configured",
            Self::LexicalUseLib => "lexical_use_lib",
            Self::FindBinDerived => "findbin_derived",
            Self::Perl5Lib => "perl5lib",
            Self::InterpreterStartup => "interpreter_startup",
            Self::LocalLib => "local_lib",
            Self::BlibLib => "blib_lib",
            Self::BlibArch => "blib_arch",
            Self::Vendor => "vendor",
            Self::Generated => "generated",
            Self::Other => "other",
        }
    }
}

/// One ordered include-root candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncludeEntry {
    /// Stable include-entry identity.
    pub id: String,
    /// Logical include-root role.
    pub role: IncludeEntryRole,
    /// Internal and public path identities.
    pub path: EnvironmentPathRef,
    /// Input that authorized or rejected this entry.
    pub input_id: EnvironmentInputId,
    /// Producer-local stable order for multiple roots from one input.
    pub source_order: u32,
}

impl IncludeEntry {
    /// Create an include-root candidate.
    #[must_use]
    pub fn new(
        role: IncludeEntryRole,
        path: EnvironmentPathRef,
        input_id: EnvironmentInputId,
        source_order: u32,
    ) -> Self {
        let id = stable_id(
            INCLUDE_ENTRY_ID_DOMAIN,
            &[
                role.identity_tag(),
                path.normalized.as_str(),
                input_id.as_str(),
                &source_order.to_string(),
            ],
        );
        Self { id, role, path, input_id, source_order }
    }
}

/// Logical role of a project root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRootRole {
    /// Workspace ownership root.
    Workspace,
    /// Ordinary project source root.
    Source,
    /// Test-only source root.
    Test,
    /// Generated or staged source root.
    Generated,
    /// Installed/build output root.
    Installed,
    /// Vendor source root.
    Vendor,
    /// Local dependency root.
    Local,
    /// Build-system working/output root.
    Build,
    /// Other reviewed root role.
    Other,
}

impl ProjectRootRole {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Source => "source",
            Self::Test => "test",
            Self::Generated => "generated",
            Self::Installed => "installed",
            Self::Vendor => "vendor",
            Self::Local => "local",
            Self::Build => "build",
            Self::Other => "other",
        }
    }
}

/// One project root candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRoot {
    /// Stable project-root identity.
    pub id: String,
    /// Root role.
    pub role: ProjectRootRole,
    /// Internal and public path identities.
    pub path: EnvironmentPathRef,
    /// Input that authorized or rejected this root.
    pub input_id: EnvironmentInputId,
}

impl ProjectRoot {
    /// Create a project-root candidate.
    #[must_use]
    pub fn new(
        role: ProjectRootRole,
        path: EnvironmentPathRef,
        input_id: EnvironmentInputId,
    ) -> Self {
        let id = stable_id(
            PROJECT_ROOT_ID_DOMAIN,
            &[role.identity_tag(), path.normalized.as_str(), input_id.as_str()],
        );
        Self { id, role, path, input_id }
    }
}

/// Reference to selected interpreter evidence produced elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpreterIdentityRef {
    /// Logical interpreter selection identity.
    pub logical_id: String,
    /// Reference to the normalized executable location.
    pub executable: EnvironmentPathRef,
    /// Fingerprint of bounded probe evidence.
    pub evidence_fingerprint: Digest,
    /// Input that selected this interpreter.
    pub input_id: EnvironmentInputId,
}

/// Reviewed build-system family.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildSystemKind {
    /// ExtUtils::MakeMaker.
    ExtUtilsMakeMaker,
    /// Module::Build.
    ModuleBuild,
    /// Dist::Zilla.
    DistZilla,
    /// Carton/cpanfile project management.
    Carton,
    /// Other reviewed build system.
    Other(String),
}

impl BuildSystemKind {
    fn identity_key(&self) -> String {
        match self {
            Self::ExtUtilsMakeMaker => "extutils_makemaker".to_string(),
            Self::ModuleBuild => "module_build".to_string(),
            Self::DistZilla => "dist_zilla".to_string(),
            Self::Carton => "carton".to_string(),
            Self::Other(value) => format!("other:{value}"),
        }
    }
}

/// Reference to non-executing project/build metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSystemFactRef {
    /// Stable build-system fact identity.
    pub id: String,
    /// Build-system family.
    pub kind: BuildSystemKind,
    /// Fingerprint of the bounded metadata evidence.
    pub fact_fingerprint: Digest,
    /// Input that supplied this fact.
    pub input_id: EnvironmentInputId,
}

impl BuildSystemFactRef {
    /// Create a build-system fact reference.
    #[must_use]
    pub fn new(
        kind: BuildSystemKind,
        fact_fingerprint: Digest,
        input_id: EnvironmentInputId,
    ) -> Self {
        let kind_key = kind.identity_key();
        let id = stable_id(
            BUILD_SYSTEM_ID_DOMAIN,
            &[kind_key.as_str(), fact_fingerprint.as_str(), input_id.as_str()],
        );
        Self { id, kind, fact_fingerprint, input_id }
    }
}

/// Logical role of a candidate executable/tool.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCandidateRole {
    /// Perl interpreter candidate.
    Perl,
    /// Test runner candidate.
    TestRunner,
    /// Debugger candidate.
    Debugger,
    /// Formatter candidate.
    Formatter,
    /// Critic/linter candidate.
    Critic,
    /// Build tool candidate.
    BuildTool,
    /// Other reviewed tool.
    Other(String),
}

impl ToolCandidateRole {
    fn identity_key(&self) -> String {
        match self {
            Self::Perl => "perl".to_string(),
            Self::TestRunner => "test_runner".to_string(),
            Self::Debugger => "debugger".to_string(),
            Self::Formatter => "formatter".to_string(),
            Self::Critic => "critic".to_string(),
            Self::BuildTool => "build_tool".to_string(),
            Self::Other(value) => format!("other:{value}"),
        }
    }
}

/// One executable/tool candidate; discovery alone does not select or execute it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCandidate {
    /// Stable tool-candidate identity.
    pub id: String,
    /// Tool role.
    pub role: ToolCandidateRole,
    /// Logical tool name.
    pub logical_name: String,
    /// Internal and public executable location identities.
    pub executable: EnvironmentPathRef,
    /// Input that supplied this candidate.
    pub input_id: EnvironmentInputId,
}

impl ToolCandidate {
    /// Create a tool candidate.
    #[must_use]
    pub fn new(
        role: ToolCandidateRole,
        logical_name: impl Into<String>,
        executable: EnvironmentPathRef,
        input_id: EnvironmentInputId,
    ) -> Self {
        let logical_name = logical_name.into();
        let role_key = role.identity_key();
        let id = stable_id(
            TOOL_CANDIDATE_ID_DOMAIN,
            &[
                role_key.as_str(),
                logical_name.as_str(),
                executable.normalized.as_str(),
                input_id.as_str(),
            ],
        );
        Self { id, role, logical_name, executable, input_id }
    }
}

/// One explicit environment limitation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentLimitation {
    /// Stable machine-readable limitation code.
    pub code: String,
    /// Internal bounded explanation.
    pub detail: String,
    /// Related input, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_id: Option<EnvironmentInputId>,
}

/// Deterministic identity of a complete environment snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnvironmentFingerprint(Digest);

impl EnvironmentFingerprint {
    /// Fingerprint string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for EnvironmentFingerprint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Immutable, versioned authority for project environment inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEnvironmentSnapshot {
    /// Environment schema version.
    pub schema_version: u32,
    /// Stable workspace/root-set identity supplied by the caller.
    pub workspace_id: String,
    /// Configuration generation used to build this snapshot.
    pub configuration_generation: u64,
    /// Workspace trust state.
    pub trust: WorkspaceTrust,
    /// Resolved input decisions in deterministic order.
    pub inputs: Vec<EnvironmentInput>,
    /// Selected interpreter evidence, when an accepted input selects one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_interpreter: Option<InterpreterIdentityRef>,
    /// Include-root candidates, including inactive explainable candidates.
    pub include_entries: Vec<IncludeEntry>,
    /// Project roots, including inactive explainable candidates.
    pub project_roots: Vec<ProjectRoot>,
    /// Build-system fact references.
    pub build_systems: Vec<BuildSystemFactRef>,
    /// Tool candidates; presence does not authorize execution.
    pub tool_candidates: Vec<ToolCandidate>,
    /// Explicit limitations.
    pub limitations: Vec<EnvironmentLimitation>,
    /// Deterministic fingerprint over all behavior-bearing fields.
    pub fingerprint: EnvironmentFingerprint,
}

impl ProjectEnvironmentSnapshot {
    /// State of a referenced input.
    #[must_use]
    pub fn input_state(&self, input_id: &EnvironmentInputId) -> Option<EnvironmentInputState> {
        self.inputs.iter().find(|input| input.id == *input_id).map(|input| input.state)
    }

    /// Active include entries in deterministic precedence order.
    pub fn active_include_entries(&self) -> impl Iterator<Item = &IncludeEntry> {
        self.include_entries.iter().filter(|entry| {
            self.input_state(&entry.input_id).is_some_and(EnvironmentInputState::is_active)
        })
    }

    /// Active project roots in deterministic precedence order.
    pub fn active_project_roots(&self) -> impl Iterator<Item = &ProjectRoot> {
        self.project_roots.iter().filter(|root| {
            self.input_state(&root.input_id).is_some_and(EnvironmentInputState::is_active)
        })
    }

    /// Active tool candidates in deterministic precedence order.
    pub fn active_tool_candidates(&self) -> impl Iterator<Item = &ToolCandidate> {
        self.tool_candidates.iter().filter(|tool| {
            self.input_state(&tool.input_id).is_some_and(EnvironmentInputState::is_active)
        })
    }

    /// Redacted public receipt projection.
    #[must_use]
    pub fn public_receipt(&self) -> PublicProjectEnvironmentReceipt {
        PublicProjectEnvironmentReceipt {
            schema_version: self.schema_version,
            workspace_id: self.workspace_id.clone(),
            configuration_generation: self.configuration_generation,
            trust: self.trust,
            inputs: self
                .inputs
                .iter()
                .map(|input| PublicEnvironmentInput {
                    id: input.id.clone(),
                    semantic_key: input.semantic_key.clone(),
                    authority: input.authority,
                    state: input.state,
                    value_fingerprint: input.value_fingerprint.clone(),
                    explanation_code: input.explanation_code.clone(),
                })
                .collect(),
            selected_interpreter: self.selected_interpreter.as_ref().map(|interpreter| {
                PublicInterpreterIdentityRef {
                    logical_id: interpreter.logical_id.clone(),
                    executable_public_id: interpreter.executable.public_id.clone(),
                    evidence_fingerprint: interpreter.evidence_fingerprint.clone(),
                    input_id: interpreter.input_id.clone(),
                }
            }),
            include_entries: self
                .include_entries
                .iter()
                .map(|entry| PublicPathEntry {
                    id: entry.id.clone(),
                    role: entry.role.identity_tag().to_string(),
                    public_id: entry.path.public_id.clone(),
                    input_id: entry.input_id.clone(),
                })
                .collect(),
            project_roots: self
                .project_roots
                .iter()
                .map(|root| PublicPathEntry {
                    id: root.id.clone(),
                    role: root.role.identity_tag().to_string(),
                    public_id: root.path.public_id.clone(),
                    input_id: root.input_id.clone(),
                })
                .collect(),
            build_systems: self
                .build_systems
                .iter()
                .map(|build| PublicBuildSystemFactRef {
                    id: build.id.clone(),
                    kind: build.kind.identity_key(),
                    fact_fingerprint: build.fact_fingerprint.clone(),
                    input_id: build.input_id.clone(),
                })
                .collect(),
            tool_candidates: self
                .tool_candidates
                .iter()
                .map(|tool| PublicToolCandidate {
                    id: tool.id.clone(),
                    role: tool.role.identity_key(),
                    logical_name: tool.logical_name.clone(),
                    executable_public_id: tool.executable.public_id.clone(),
                    input_id: tool.input_id.clone(),
                })
                .collect(),
            limitation_codes: self.limitations.iter().map(|item| item.code.clone()).collect(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

/// Redacted public environment receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProjectEnvironmentReceipt {
    /// Environment schema version.
    pub schema_version: u32,
    /// Stable workspace identity.
    pub workspace_id: String,
    /// Configuration generation.
    pub configuration_generation: u64,
    /// Trust state.
    pub trust: WorkspaceTrust,
    /// Redacted input decisions.
    pub inputs: Vec<PublicEnvironmentInput>,
    /// Redacted interpreter reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_interpreter: Option<PublicInterpreterIdentityRef>,
    /// Redacted include entries.
    pub include_entries: Vec<PublicPathEntry>,
    /// Redacted project roots.
    pub project_roots: Vec<PublicPathEntry>,
    /// Redacted build-system facts.
    pub build_systems: Vec<PublicBuildSystemFactRef>,
    /// Redacted tool candidates.
    pub tool_candidates: Vec<PublicToolCandidate>,
    /// Limitation codes without internal detail.
    pub limitation_codes: Vec<String>,
    /// Exact snapshot fingerprint.
    pub fingerprint: EnvironmentFingerprint,
}

/// Redacted input decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicEnvironmentInput {
    /// Input identity.
    pub id: EnvironmentInputId,
    /// Logical precedence key.
    pub semantic_key: String,
    /// Input authority.
    pub authority: EnvironmentInputAuthority,
    /// Resolved state.
    pub state: EnvironmentInputState,
    /// Value fingerprint, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_fingerprint: Option<Digest>,
    /// Stable policy explanation code.
    pub explanation_code: String,
}

/// Redacted interpreter evidence reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicInterpreterIdentityRef {
    /// Logical interpreter identity.
    pub logical_id: String,
    /// Redacted executable identity.
    pub executable_public_id: String,
    /// Bounded evidence fingerprint.
    pub evidence_fingerprint: Digest,
    /// Selecting input.
    pub input_id: EnvironmentInputId,
}

/// Redacted path entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicPathEntry {
    /// Entry identity.
    pub id: String,
    /// Stable role tag.
    pub role: String,
    /// Redacted path identity.
    pub public_id: String,
    /// Governing input.
    pub input_id: EnvironmentInputId,
}

/// Redacted build-system fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicBuildSystemFactRef {
    /// Fact identity.
    pub id: String,
    /// Stable build-system kind.
    pub kind: String,
    /// Metadata evidence fingerprint.
    pub fact_fingerprint: Digest,
    /// Governing input.
    pub input_id: EnvironmentInputId,
}

/// Redacted tool candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicToolCandidate {
    /// Candidate identity.
    pub id: String,
    /// Stable tool role.
    pub role: String,
    /// Logical tool name.
    pub logical_name: String,
    /// Redacted executable identity.
    pub executable_public_id: String,
    /// Governing input.
    pub input_id: EnvironmentInputId,
}

/// Error returned while compiling an environment snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentBuildError {
    /// Workspace identity is empty.
    EmptyWorkspaceId,
    /// An input has an empty required field.
    EmptyInputField {
        /// Input identity.
        input_id: EnvironmentInputId,
        /// Empty field name.
        field: &'static str,
    },
    /// A typed candidate references an unknown input.
    MissingInputReference {
        /// Candidate class.
        owner: &'static str,
        /// Missing input.
        input_id: EnvironmentInputId,
    },
    /// The selected interpreter is not backed by an accepted input.
    InactiveSelectedInterpreter {
        /// Selecting input.
        input_id: EnvironmentInputId,
        /// Resolved state, or `None` if the input is missing.
        state: Option<EnvironmentInputState>,
    },
    /// Ambient authority was incorrectly marked active.
    AmbientInputAccepted {
        /// Input identity.
        input_id: EnvironmentInputId,
    },
    /// A path reference is missing internal or public identity.
    EmptyPathField {
        /// Candidate class.
        owner: &'static str,
        /// Empty field name.
        field: &'static str,
    },
}

impl std::fmt::Display for EnvironmentBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWorkspaceId => formatter.write_str("workspace environment ID is empty"),
            Self::EmptyInputField { input_id, field } => {
                write!(formatter, "environment input {input_id} has empty field {field}")
            }
            Self::MissingInputReference { owner, input_id } => {
                write!(formatter, "{owner} references missing environment input {input_id}")
            }
            Self::InactiveSelectedInterpreter { input_id, state } => {
                write!(formatter, "selected interpreter input {input_id} is not active: {state:?}")
            }
            Self::AmbientInputAccepted { input_id } => {
                write!(
                    formatter,
                    "ambient input {input_id} cannot be accepted; use explicit_environment authority"
                )
            }
            Self::EmptyPathField { owner, field } => {
                write!(formatter, "{owner} has empty path field {field}")
            }
        }
    }
}

impl std::error::Error for EnvironmentBuildError {}

/// Pure builder and precedence compiler for [`ProjectEnvironmentSnapshot`].
#[derive(Debug, Clone)]
pub struct ProjectEnvironmentSnapshotBuilder {
    workspace_id: String,
    configuration_generation: u64,
    trust: WorkspaceTrust,
    inputs: Vec<EnvironmentInput>,
    selected_interpreter: Option<InterpreterIdentityRef>,
    include_entries: Vec<IncludeEntry>,
    project_roots: Vec<ProjectRoot>,
    build_systems: Vec<BuildSystemFactRef>,
    tool_candidates: Vec<ToolCandidate>,
    limitations: Vec<EnvironmentLimitation>,
}

impl ProjectEnvironmentSnapshotBuilder {
    /// Start a pure environment snapshot builder.
    #[must_use]
    pub fn new(
        workspace_id: impl Into<String>,
        configuration_generation: u64,
        trust: WorkspaceTrust,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            configuration_generation,
            trust,
            inputs: Vec::new(),
            selected_interpreter: None,
            include_entries: Vec::new(),
            project_roots: Vec::new(),
            build_systems: Vec::new(),
            tool_candidates: Vec::new(),
            limitations: Vec::new(),
        }
    }

    /// Add an environment input candidate.
    #[must_use]
    pub fn with_input(mut self, input: EnvironmentInput) -> Self {
        self.inputs.push(input);
        self
    }

    /// Select interpreter evidence supplied by an accepted input.
    #[must_use]
    pub fn with_selected_interpreter(mut self, interpreter: InterpreterIdentityRef) -> Self {
        self.selected_interpreter = Some(interpreter);
        self
    }

    /// Add an include-root candidate.
    #[must_use]
    pub fn with_include_entry(mut self, entry: IncludeEntry) -> Self {
        self.include_entries.push(entry);
        self
    }

    /// Add a project-root candidate.
    #[must_use]
    pub fn with_project_root(mut self, root: ProjectRoot) -> Self {
        self.project_roots.push(root);
        self
    }

    /// Add a build-system fact reference.
    #[must_use]
    pub fn with_build_system(mut self, build_system: BuildSystemFactRef) -> Self {
        self.build_systems.push(build_system);
        self
    }

    /// Add an executable/tool candidate.
    #[must_use]
    pub fn with_tool_candidate(mut self, tool: ToolCandidate) -> Self {
        self.tool_candidates.push(tool);
        self
    }

    /// Add an explicit limitation.
    #[must_use]
    pub fn with_limitation(mut self, limitation: EnvironmentLimitation) -> Self {
        self.limitations.push(limitation);
        self
    }

    /// Resolve precedence, validate references, and build the immutable snapshot.
    pub fn build(mut self) -> Result<ProjectEnvironmentSnapshot, EnvironmentBuildError> {
        if self.workspace_id.is_empty() {
            return Err(EnvironmentBuildError::EmptyWorkspaceId);
        }

        self.inputs.sort_by(input_sort_key);
        self.inputs.dedup();
        validate_inputs(&self.inputs)?;
        resolve_input_precedence(&mut self.inputs);
        self.inputs.sort_by(input_sort_key);

        let input_decisions: BTreeMap<
            EnvironmentInputId,
            (EnvironmentInputState, EnvironmentInputAuthority),
        > = self
            .inputs
            .iter()
            .map(|input| (input.id.clone(), (input.state, input.authority)))
            .collect();

        validate_paths("include_entry", self.include_entries.iter().map(|item| &item.path))?;
        validate_paths("project_root", self.project_roots.iter().map(|item| &item.path))?;
        validate_paths("tool_candidate", self.tool_candidates.iter().map(|item| &item.executable))?;

        validate_references(
            &input_decisions,
            "include_entry",
            self.include_entries.iter().map(|item| &item.input_id),
        )?;
        validate_references(
            &input_decisions,
            "project_root",
            self.project_roots.iter().map(|item| &item.input_id),
        )?;
        validate_references(
            &input_decisions,
            "build_system",
            self.build_systems.iter().map(|item| &item.input_id),
        )?;
        validate_references(
            &input_decisions,
            "tool_candidate",
            self.tool_candidates.iter().map(|item| &item.input_id),
        )?;

        if let Some(interpreter) = &self.selected_interpreter {
            let state = input_decisions.get(&interpreter.input_id).map(|decision| decision.0);
            if !state.is_some_and(EnvironmentInputState::is_active) {
                return Err(EnvironmentBuildError::InactiveSelectedInterpreter {
                    input_id: interpreter.input_id.clone(),
                    state,
                });
            }
            validate_paths("selected_interpreter", std::iter::once(&interpreter.executable))?;
        }

        self.include_entries.sort_by(|left, right| {
            candidate_precedence(&input_decisions, &left.input_id)
                .cmp(&candidate_precedence(&input_decisions, &right.input_id))
                .then(left.role.cmp(&right.role))
                .then(left.source_order.cmp(&right.source_order))
                .then(left.id.cmp(&right.id))
        });
        self.include_entries.dedup_by(|left, right| left.id == right.id);

        self.project_roots.sort_by(|left, right| {
            candidate_precedence(&input_decisions, &left.input_id)
                .cmp(&candidate_precedence(&input_decisions, &right.input_id))
                .then(left.role.cmp(&right.role))
                .then(left.id.cmp(&right.id))
        });
        self.project_roots.dedup_by(|left, right| left.id == right.id);

        self.build_systems.sort_by(|left, right| {
            candidate_precedence(&input_decisions, &left.input_id)
                .cmp(&candidate_precedence(&input_decisions, &right.input_id))
                .then(left.kind.cmp(&right.kind))
                .then(left.id.cmp(&right.id))
        });
        self.build_systems.dedup_by(|left, right| left.id == right.id);

        self.tool_candidates.sort_by(|left, right| {
            candidate_precedence(&input_decisions, &left.input_id)
                .cmp(&candidate_precedence(&input_decisions, &right.input_id))
                .then(left.role.cmp(&right.role))
                .then(left.logical_name.cmp(&right.logical_name))
                .then(left.id.cmp(&right.id))
        });
        self.tool_candidates.dedup_by(|left, right| left.id == right.id);

        self.limitations.sort();
        self.limitations.dedup();

        let fingerprint = compute_fingerprint(
            self.workspace_id.as_str(),
            self.configuration_generation,
            self.trust,
            &self.inputs,
            self.selected_interpreter.as_ref(),
            &self.include_entries,
            &self.project_roots,
            &self.build_systems,
            &self.tool_candidates,
            &self.limitations,
        );

        Ok(ProjectEnvironmentSnapshot {
            schema_version: PROJECT_ENVIRONMENT_SCHEMA_VERSION,
            workspace_id: self.workspace_id,
            configuration_generation: self.configuration_generation,
            trust: self.trust,
            inputs: self.inputs,
            selected_interpreter: self.selected_interpreter,
            include_entries: self.include_entries,
            project_roots: self.project_roots,
            build_systems: self.build_systems,
            tool_candidates: self.tool_candidates,
            limitations: self.limitations,
            fingerprint,
        })
    }
}

fn input_sort_key(left: &EnvironmentInput, right: &EnvironmentInput) -> std::cmp::Ordering {
    left.semantic_key
        .cmp(&right.semantic_key)
        .then(left.authority.precedence_rank().cmp(&right.authority.precedence_rank()))
        .then(left.source_id.cmp(&right.source_id))
        .then(
            left.value_fingerprint
                .as_ref()
                .map(Digest::as_str)
                .cmp(&right.value_fingerprint.as_ref().map(Digest::as_str)),
        )
        .then(left.id.cmp(&right.id))
        .then(left.state.cmp(&right.state))
}

fn validate_inputs(inputs: &[EnvironmentInput]) -> Result<(), EnvironmentBuildError> {
    for input in inputs {
        if input.authority == EnvironmentInputAuthority::Ambient
            && input.state == EnvironmentInputState::Accepted
        {
            return Err(EnvironmentBuildError::AmbientInputAccepted { input_id: input.id.clone() });
        }
        for (field, value) in [
            ("semantic_key", input.semantic_key.as_str()),
            ("source_id", input.source_id.as_str()),
            ("explanation_code", input.explanation_code.as_str()),
        ] {
            if value.is_empty() {
                return Err(EnvironmentBuildError::EmptyInputField {
                    input_id: input.id.clone(),
                    field,
                });
            }
        }
    }
    Ok(())
}

fn resolve_input_precedence(inputs: &mut [EnvironmentInput]) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        groups.entry(input.semantic_key.clone()).or_default().push(index);
    }

    for indices in groups.values() {
        let accepted: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|index| inputs[*index].state == EnvironmentInputState::Accepted)
            .collect();
        let Some(best_rank) =
            accepted.iter().map(|index| inputs[*index].authority.precedence_rank()).min()
        else {
            continue;
        };

        let winners: Vec<usize> = accepted
            .iter()
            .copied()
            .filter(|index| inputs[*index].authority.precedence_rank() == best_rank)
            .collect();
        let distinct_values: BTreeSet<String> = winners
            .iter()
            .map(|index| {
                inputs[*index].value_fingerprint.as_ref().map_or_else(
                    || format!("unfingerprinted:{}", inputs[*index].source_id),
                    |digest| format!("fingerprinted:{}", digest.as_str()),
                )
            })
            .collect();

        if distinct_values.len() > 1 {
            for index in winners {
                inputs[index].state = EnvironmentInputState::Conflicting;
            }
        } else if let Some((&winner, duplicates)) = winners.split_first() {
            inputs[winner].state = EnvironmentInputState::Accepted;
            for index in duplicates {
                inputs[*index].state = EnvironmentInputState::Superseded;
            }
        }

        for index in accepted {
            if inputs[index].authority.precedence_rank() > best_rank {
                inputs[index].state = EnvironmentInputState::Superseded;
            }
        }
    }
}

fn validate_references<'a>(
    input_decisions: &BTreeMap<
        EnvironmentInputId,
        (EnvironmentInputState, EnvironmentInputAuthority),
    >,
    owner: &'static str,
    input_ids: impl Iterator<Item = &'a EnvironmentInputId>,
) -> Result<(), EnvironmentBuildError> {
    for input_id in input_ids {
        if !input_decisions.contains_key(input_id) {
            return Err(EnvironmentBuildError::MissingInputReference {
                owner,
                input_id: input_id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_paths<'a>(
    owner: &'static str,
    paths: impl Iterator<Item = &'a EnvironmentPathRef>,
) -> Result<(), EnvironmentBuildError> {
    for path in paths {
        if path.normalized.is_empty() {
            return Err(EnvironmentBuildError::EmptyPathField { owner, field: "normalized" });
        }
        if path.public_id.is_empty() {
            return Err(EnvironmentBuildError::EmptyPathField { owner, field: "public_id" });
        }
    }
    Ok(())
}

fn candidate_precedence<'a>(
    input_decisions: &BTreeMap<
        EnvironmentInputId,
        (EnvironmentInputState, EnvironmentInputAuthority),
    >,
    input_id: &'a EnvironmentInputId,
) -> (u8, u8, u8, &'a str) {
    let decision = input_decisions.get(input_id).copied();
    let state = decision.map(|value| value.0);
    let authority = decision.map(|value| value.1);
    let active_rank = if state.is_some_and(EnvironmentInputState::is_active) { 0 } else { 1 };
    let authority_rank = authority.map_or(u8::MAX, EnvironmentInputAuthority::precedence_rank);
    let state_rank = state.map_or(u8::MAX, EnvironmentInputState::sort_rank);
    (active_rank, authority_rank, state_rank, input_id.as_str())
}

#[allow(clippy::too_many_arguments)]
fn compute_fingerprint(
    workspace_id: &str,
    configuration_generation: u64,
    trust: WorkspaceTrust,
    inputs: &[EnvironmentInput],
    selected_interpreter: Option<&InterpreterIdentityRef>,
    include_entries: &[IncludeEntry],
    project_roots: &[ProjectRoot],
    build_systems: &[BuildSystemFactRef],
    tool_candidates: &[ToolCandidate],
    limitations: &[EnvironmentLimitation],
) -> EnvironmentFingerprint {
    let mut material = String::new();
    push_field(&mut material, "schema", &PROJECT_ENVIRONMENT_SCHEMA_VERSION.to_string());
    push_field(&mut material, "workspace", workspace_id);
    push_field(&mut material, "generation", &configuration_generation.to_string());
    push_field(&mut material, "trust", trust.identity_tag());

    for input in inputs {
        push_field(&mut material, "input.id", input.id.as_str());
        push_field(&mut material, "input.key", input.semantic_key.as_str());
        push_field(&mut material, "input.authority", input.authority.identity_tag());
        push_field(&mut material, "input.state", input.state.identity_tag());
        push_field(&mut material, "input.source", input.source_id.as_str());
        push_field(
            &mut material,
            "input.value",
            input.value_fingerprint.as_ref().map_or("", Digest::as_str),
        );
        push_field(&mut material, "input.explanation", input.explanation_code.as_str());
    }

    if let Some(interpreter) = selected_interpreter {
        push_field(&mut material, "interpreter.logical", interpreter.logical_id.as_str());
        push_field(&mut material, "interpreter.path", interpreter.executable.normalized.as_str());
        push_field(
            &mut material,
            "interpreter.evidence",
            interpreter.evidence_fingerprint.as_str(),
        );
        push_field(&mut material, "interpreter.input", interpreter.input_id.as_str());
    }

    for entry in include_entries {
        push_field(&mut material, "include.id", entry.id.as_str());
        push_field(&mut material, "include.role", entry.role.identity_tag());
        push_field(&mut material, "include.path", entry.path.normalized.as_str());
        push_field(&mut material, "include.input", entry.input_id.as_str());
        push_field(&mut material, "include.order", &entry.source_order.to_string());
    }

    for root in project_roots {
        push_field(&mut material, "root.id", root.id.as_str());
        push_field(&mut material, "root.role", root.role.identity_tag());
        push_field(&mut material, "root.path", root.path.normalized.as_str());
        push_field(&mut material, "root.input", root.input_id.as_str());
    }

    for build in build_systems {
        push_field(&mut material, "build.id", build.id.as_str());
        let build_kind = build.kind.identity_key();
        push_field(&mut material, "build.kind", build_kind.as_str());
        push_field(&mut material, "build.fact", build.fact_fingerprint.as_str());
        push_field(&mut material, "build.input", build.input_id.as_str());
    }

    for tool in tool_candidates {
        push_field(&mut material, "tool.id", tool.id.as_str());
        let tool_role = tool.role.identity_key();
        push_field(&mut material, "tool.role", tool_role.as_str());
        push_field(&mut material, "tool.name", tool.logical_name.as_str());
        push_field(&mut material, "tool.path", tool.executable.normalized.as_str());
        push_field(&mut material, "tool.input", tool.input_id.as_str());
    }

    for limitation in limitations {
        push_field(&mut material, "limitation.code", limitation.code.as_str());
        push_field(&mut material, "limitation.detail", limitation.detail.as_str());
        push_field(
            &mut material,
            "limitation.input",
            limitation.input_id.as_ref().map_or("", EnvironmentInputId::as_str),
        );
    }

    EnvironmentFingerprint(Digest::of(&material))
}

fn push_field(output: &mut String, tag: &str, value: &str) {
    output.push_str(tag.len().to_string().as_str());
    output.push(':');
    output.push_str(tag);
    output.push(':');
    output.push_str(value.len().to_string().as_str());
    output.push(':');
    output.push_str(value);
}

fn stable_id(domain: &str, fields: &[&str]) -> String {
    let mut material = String::new();
    push_field(&mut material, "domain", domain);
    for field in fields {
        push_field(&mut material, "field", field);
    }
    format!("{domain}:fnv64:{:016x}", fnv1a(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        key: &str,
        authority: EnvironmentInputAuthority,
        value: &str,
        source: &str,
    ) -> EnvironmentInput {
        EnvironmentInput::new(
            key,
            authority,
            EnvironmentInputState::Accepted,
            source,
            Some(Digest::of(value)),
            "fixture",
        )
    }

    #[test]
    fn precedence_and_output_are_input_order_independent() -> Result<(), EnvironmentBuildError> {
        let user = input(
            "include.primary",
            EnvironmentInputAuthority::UserConfiguration,
            "user",
            "client",
        );
        let project = input(
            "include.primary",
            EnvironmentInputAuthority::TrustedProjectConfiguration,
            "project",
            "project_file",
        );

        let left =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 7, WorkspaceTrust::Trusted)
                .with_input(project.clone())
                .with_input(user.clone())
                .build()?;
        let right =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 7, WorkspaceTrust::Trusted)
                .with_input(user)
                .with_input(project)
                .build()?;

        assert_eq!(left, right);
        assert_eq!(left.inputs.iter().filter(|item| item.state.is_active()).count(), 1);
        assert_eq!(
            left.inputs.iter().find(|item| item.state.is_active()).map(|item| item.authority),
            Some(EnvironmentInputAuthority::UserConfiguration)
        );
        Ok(())
    }

    #[test]
    fn equally_authoritative_disagreement_remains_conflicting() -> Result<(), EnvironmentBuildError>
    {
        let left = input(
            "interpreter.selected",
            EnvironmentInputAuthority::UserConfiguration,
            "perl-a",
            "settings-a",
        );
        let right = input(
            "interpreter.selected",
            EnvironmentInputAuthority::UserConfiguration,
            "perl-b",
            "settings-b",
        );

        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_input(left)
                .with_input(right)
                .build()?;

        assert_eq!(
            snapshot
                .inputs
                .iter()
                .filter(|item| item.state == EnvironmentInputState::Conflicting)
                .count(),
            2
        );
        assert!(!snapshot.inputs.iter().any(|item| item.state.is_active()));
        Ok(())
    }

    #[test]
    fn denied_ambient_input_is_visible_but_inactive() -> Result<(), EnvironmentBuildError> {
        let ambient = EnvironmentInput::new(
            "env.PERL5OPT",
            EnvironmentInputAuthority::Ambient,
            EnvironmentInputState::Denied,
            "process_environment",
            Some(Digest::of("-Mlocal::lib")),
            "ambient_code_loading_denied",
        );
        let input_id = ambient.id.clone();
        let root = IncludeEntry::new(
            IncludeEntryRole::Other,
            EnvironmentPathRef::new("/private/ambient", "path:ambient"),
            input_id,
            0,
        );

        let snapshot = ProjectEnvironmentSnapshotBuilder::new(
            "workspace:fixture",
            1,
            WorkspaceTrust::Untrusted,
        )
        .with_input(ambient)
        .with_include_entry(root)
        .build()?;

        assert_eq!(snapshot.active_include_entries().count(), 0);
        assert_eq!(snapshot.include_entries.len(), 1);
        assert_eq!(snapshot.inputs[0].state, EnvironmentInputState::Denied);
        Ok(())
    }

    #[test]
    fn same_path_with_different_root_roles_remains_distinct() -> Result<(), EnvironmentBuildError> {
        let source_input = input(
            "root.source",
            EnvironmentInputAuthority::WorkspaceConvention,
            "lib",
            "workspace",
        );
        let generated_input = input(
            "root.generated",
            EnvironmentInputAuthority::BuildMetadata,
            "lib",
            "build_metadata",
        );
        let source = ProjectRoot::new(
            ProjectRootRole::Source,
            EnvironmentPathRef::new("/repo/lib", "root:lib"),
            source_input.id.clone(),
        );
        let generated = ProjectRoot::new(
            ProjectRootRole::Generated,
            EnvironmentPathRef::new("/repo/lib", "root:lib"),
            generated_input.id.clone(),
        );

        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_input(source_input)
                .with_input(generated_input)
                .with_project_root(source)
                .with_project_root(generated)
                .build()?;

        assert_eq!(snapshot.project_roots.len(), 2);
        assert_ne!(snapshot.project_roots[0].id, snapshot.project_roots[1].id);
        Ok(())
    }

    #[test]
    fn inactive_interpreter_cannot_be_selected() {
        let candidate = EnvironmentInput::new(
            "interpreter.selected",
            EnvironmentInputAuthority::Ambient,
            EnvironmentInputState::Unavailable,
            "path_search",
            None,
            "interpreter_unavailable",
        );
        let interpreter = InterpreterIdentityRef {
            logical_id: "perl:missing".to_string(),
            executable: EnvironmentPathRef::new("/missing/perl", "tool:perl"),
            evidence_fingerprint: Digest::of("missing"),
            input_id: candidate.id.clone(),
        };

        let result =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Unknown)
                .with_input(candidate)
                .with_selected_interpreter(interpreter)
                .build();

        assert!(matches!(
            result,
            Err(EnvironmentBuildError::InactiveSelectedInterpreter {
                state: Some(EnvironmentInputState::Unavailable),
                ..
            })
        ));
    }

    #[test]
    fn public_receipt_redacts_internal_paths() -> Result<(), Box<dyn std::error::Error>> {
        let configured = input(
            "include.configured",
            EnvironmentInputAuthority::UserConfiguration,
            "/home/steven/private/lib",
            "client",
        );
        let root = IncludeEntry::new(
            IncludeEntryRole::WorkspaceConfigured,
            EnvironmentPathRef::new("/home/steven/private/lib", "path:configured-lib"),
            configured.id.clone(),
            0,
        );

        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_input(configured)
                .with_include_entry(root)
                .build()?;
        let json = serde_json::to_string(&snapshot.public_receipt())?;

        assert!(!json.contains("/home/steven/private/lib"));
        assert!(json.contains("path:configured-lib"));
        Ok(())
    }

    #[test]
    fn behavior_bearing_changes_move_the_fingerprint() -> Result<(), EnvironmentBuildError> {
        let first =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_input(input(
                    "include.configured",
                    EnvironmentInputAuthority::UserConfiguration,
                    "lib",
                    "client",
                ))
                .build()?;
        let second =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 2, WorkspaceTrust::Trusted)
                .with_input(input(
                    "include.configured",
                    EnvironmentInputAuthority::UserConfiguration,
                    "lib",
                    "client",
                ))
                .build()?;
        assert_ne!(first.fingerprint, second.fingerprint);
        Ok(())
    }

    #[test]
    fn ambient_authority_cannot_be_marked_active() {
        let ambient = EnvironmentInput::new(
            "env.PERL5LIB",
            EnvironmentInputAuthority::Ambient,
            EnvironmentInputState::Accepted,
            "process_environment",
            Some(Digest::of("/ambient/lib")),
            "ambient_observed",
        );
        let input_id = ambient.id.clone();

        let result =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Unknown)
                .with_input(ambient)
                .build();

        assert!(matches!(
            result,
            Err(EnvironmentBuildError::AmbientInputAccepted { input_id: actual })
                if actual == input_id
        ));
    }

    #[test]
    fn missing_referenced_input_fails_closed() {
        let missing = EnvironmentInputId("project_environment.input.v1:fnv64:missing".to_string());
        let root = ProjectRoot::new(
            ProjectRootRole::Source,
            EnvironmentPathRef::new("/repo/lib", "root:lib"),
            missing.clone(),
        );
        let result =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_project_root(root)
                .build();

        assert!(matches!(
            result,
            Err(EnvironmentBuildError::MissingInputReference {
                owner: "project_root",
                input_id,
            }) if input_id == missing
        ));
    }
}
