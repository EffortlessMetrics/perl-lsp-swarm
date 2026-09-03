//! Pure operation execution authorization for the project environment model.
//!
//! [`super`] answers *which project inputs are active*. This module answers the
//! separate operation question:
//!
//! ```text
//! May this exact user-/CI-requested operation execute now,
//! with these inputs, under this scope and authority?
//! ```
//!
//! The vocabulary is deliberately transport-neutral and side-effect free. It
//! performs no configuration read, trust transition, filesystem access, process
//! spawn, cancellation, or provider work, and it introduces no LSP, DAP, or
//! editor types. It compiles
//!
//! ```text
//! ExecutionIntent
//! + OperationTrustRequirement
//! + environment/input authority facts
//! + trust/policy evidence
//! → ExecutionAuthorizationDecision
//! ```
//!
//! # What an authorization decision is not
//!
//! A decision states that an operation's *authority* requirements are met. It
//! never asserts that a resulting process plan is safe, that the selected
//! interpreter is benign, or that project code under execution is trustworthy.
//! Those remain separate obligations for the process-plan layer.
//!
//! # Separate facts
//!
//! Workspace source trust, environment input authority, input risk
//! classification, user intent, operation capability requirement, policy
//! denial, and execution authorization are distinct facts. No `bool` and no
//! enum ordinal stands in for the complete decision: [`authorize`] is the only
//! producer of an [`ExecutionAuthorizationDecision`], and the decision's
//! granted capabilities are not publicly constructible or widenable.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};

use super::{
    EnvironmentFingerprint, EnvironmentInputAuthority, WorkspaceTrust, push_field, stable_id,
};
use crate::Digest;

/// Schema version for [`ExecutionAuthorizationDecision`] and its inputs.
///
/// Bump whenever the meaning of an existing field, reason code, capability, or
/// registry entry changes. Adding a reviewed operation profile that no existing
/// caller can observe is additive; changing what a profile requires is not.
pub const EXECUTION_AUTHORIZATION_SCHEMA_VERSION: u32 = 1;

/// Version of the reviewed operation registry in [`OperationTrustRequirement`].
pub const OPERATION_REGISTRY_VERSION: u32 = 1;

const INTENT_ID_DOMAIN: &str = "execution_authorization.intent.v1";
const EVIDENCE_ID_DOMAIN: &str = "execution_authorization.evidence.v1";
const REQUIREMENT_ID_DOMAIN: &str = "execution_authorization.requirement.v1";
const CLASSIFIED_INPUT_ID_DOMAIN: &str = "execution_authorization.input.v1";

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// One authority axis an operation may require and a decision may grant.
///
/// These are the nine authority classes an [`OperationTrustRequirement`] is
/// expressed in. Variant order is not policy; it exists only so capability sets
/// have a deterministic encoding. [`Self::identity_tag`] is the stable form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionCapability {
    /// Read and analyze workspace source without executing anything.
    SourceAnalysis,
    /// Read files outside the workspace for module resolution.
    ExternalRead,
    /// Treat project-controlled configuration as authority.
    ProjectConfiguration,
    /// Invoke an external executable or tool.
    ExecutableTool,
    /// Let environment state influence Perl code loading.
    EnvironmentCodeLoading,
    /// Execute project-controlled Perl code.
    ProjectCodeExecution,
    /// Use a path outside the workspace root or a private/system path.
    OutsideRootPath,
    /// Run repeatedly on a persistent cadence rather than once on request.
    PersistentCadence,
    /// Hold an interactive or long-lived external session.
    InteractiveSession,
}

impl ExecutionCapability {
    /// Every capability in deterministic order.
    pub const ALL: [Self; 9] = [
        Self::SourceAnalysis,
        Self::ExternalRead,
        Self::ProjectConfiguration,
        Self::ExecutableTool,
        Self::EnvironmentCodeLoading,
        Self::ProjectCodeExecution,
        Self::OutsideRootPath,
        Self::PersistentCadence,
        Self::InteractiveSession,
    ];

    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::SourceAnalysis => "source_analysis",
            Self::ExternalRead => "external_read",
            Self::ProjectConfiguration => "project_configuration",
            Self::ExecutableTool => "executable_tool",
            Self::EnvironmentCodeLoading => "environment_code_loading",
            Self::ProjectCodeExecution => "project_code_execution",
            Self::OutsideRootPath => "outside_root_path",
            Self::PersistentCadence => "persistent_cadence",
            Self::InteractiveSession => "interactive_session",
        }
    }

    /// Whether this capability permits running code or an external binary.
    ///
    /// [`Self::SourceAnalysis`] and [`Self::ExternalRead`] are read-only; every
    /// other axis is execution-bearing and is never granted implicitly.
    #[must_use]
    pub const fn is_execution_bearing(self) -> bool {
        !matches!(self, Self::SourceAnalysis | Self::ExternalRead)
    }
}

/// An immutable set of capabilities.
///
/// There is deliberately no insert, extend, or union-in-place operation: a
/// granted set produced by [`authorize`] cannot be widened by a downstream
/// consumer.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<ExecutionCapability>);

impl CapabilitySet {
    /// Build a set from any capability iterator.
    #[must_use]
    pub fn new(capabilities: impl IntoIterator<Item = ExecutionCapability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// The empty set.
    #[must_use]
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    /// Whether the set contains `capability`.
    #[must_use]
    pub fn contains(&self, capability: ExecutionCapability) -> bool {
        self.0.contains(&capability)
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of capabilities in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Capabilities in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = ExecutionCapability> + '_ {
        self.0.iter().copied()
    }

    /// Whether every capability in `other` is present in this set.
    #[must_use]
    pub fn contains_all(&self, other: &Self) -> bool {
        other.0.is_subset(&self.0)
    }

    /// Capabilities present here but absent from `other`.
    #[must_use]
    pub fn difference(&self, other: &Self) -> Self {
        Self(self.0.difference(&other.0).copied().collect())
    }

    /// Stable tags in deterministic order.
    #[must_use]
    pub fn tags(&self) -> Vec<&'static str> {
        self.0.iter().map(|capability| capability.identity_tag()).collect()
    }
}

impl FromIterator<ExecutionCapability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = ExecutionCapability>>(iter: I) -> Self {
        Self::new(iter)
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// Which authority world a scope belongs to.
///
/// CI/hermetic authority is a separate class. It is never synthesized from
/// editor workspace trust, and editor authority is never synthesized from a CI
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustScopeKind {
    /// An interactive editor workspace governed by workspace trust.
    EditorWorkspace,
    /// A hermetic CI or batch context governed by a CI identity.
    CiHermetic,
}

impl TrustScopeKind {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::EditorWorkspace => "editor_workspace",
            Self::CiHermetic => "ci_hermetic",
        }
    }
}

/// The exact scope an intent, evidence record, or decision is bound to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustScope {
    /// Authority world.
    pub kind: TrustScopeKind,
    /// Stable workspace/root-set identity.
    pub workspace_id: String,
    /// Stable root identity within the workspace, when the operation is
    /// root-scoped. Distinct roots may carry distinct trust generations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    /// Stable client/session identity, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl TrustScope {
    /// Construct an editor workspace scope.
    #[must_use]
    pub fn editor_workspace(workspace_id: impl Into<String>) -> Self {
        Self {
            kind: TrustScopeKind::EditorWorkspace,
            workspace_id: workspace_id.into(),
            root_id: None,
            session_id: None,
        }
    }

    /// Construct a CI/hermetic scope.
    #[must_use]
    pub fn ci_hermetic(workspace_id: impl Into<String>) -> Self {
        Self {
            kind: TrustScopeKind::CiHermetic,
            workspace_id: workspace_id.into(),
            root_id: None,
            session_id: None,
        }
    }

    /// Return this scope bound to a specific root.
    #[must_use]
    pub fn with_root(mut self, root_id: impl Into<String>) -> Self {
        self.root_id = Some(root_id.into());
        self
    }

    /// Return this scope bound to a specific session.
    #[must_use]
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    fn push_identity(&self, material: &mut String, prefix: &str) {
        push_field(material, &format!("{prefix}.kind"), self.kind.identity_tag());
        push_field(material, &format!("{prefix}.workspace"), self.workspace_id.as_str());
        push_field(material, &format!("{prefix}.root"), self.root_id.as_deref().unwrap_or(""));
        push_field(
            material,
            &format!("{prefix}.session"),
            self.session_id.as_deref().unwrap_or(""),
        );
    }
}

// ---------------------------------------------------------------------------
// Generations
// ---------------------------------------------------------------------------

/// Every generation an authorization is bound to.
///
/// Any load-bearing movement in these values makes a prior decision stale.
/// [`ExecutionAuthorizationDecision::is_current_for`] is the executable form of
/// that rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundGenerations {
    /// Configuration generation the decision was compiled against.
    pub configuration_generation: u64,
    /// Trust/policy generation the decision was compiled against.
    pub policy_generation: u64,
    /// Source/document generation the decision was compiled against.
    pub source_generation: u64,
    /// Exact environment snapshot identity the decision was compiled against.
    pub environment_fingerprint: EnvironmentFingerprint,
}

impl BoundGenerations {
    /// Construct a generation binding.
    #[must_use]
    pub fn new(
        configuration_generation: u64,
        policy_generation: u64,
        source_generation: u64,
        environment_fingerprint: EnvironmentFingerprint,
    ) -> Self {
        Self {
            configuration_generation,
            policy_generation,
            source_generation,
            environment_fingerprint,
        }
    }

    fn push_identity(&self, material: &mut String, prefix: &str) {
        push_field(
            material,
            &format!("{prefix}.configuration"),
            &self.configuration_generation.to_string(),
        );
        push_field(material, &format!("{prefix}.policy"), &self.policy_generation.to_string());
        push_field(material, &format!("{prefix}.source"), &self.source_generation.to_string());
        push_field(
            material,
            &format!("{prefix}.environment"),
            self.environment_fingerprint.as_str(),
        );
    }
}

// ---------------------------------------------------------------------------
// Operation registry
// ---------------------------------------------------------------------------

/// A reviewed operation profile.
///
/// Callers select a registered profile; they cannot invent a free-form
/// operation name, and they cannot declare a profile that asks for less
/// authority than the registry says it uses. New product domains extend the
/// registry through review, not at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationProfile {
    /// Parse and index workspace source only.
    SourceAnalysisOnly,
    /// Resolve modules, reading roots outside the workspace.
    ModuleResolutionExternalRead,
    /// Run the current saved file.
    RunCurrentSavedFile,
    /// Run the project's test suite.
    RunTests,
    /// Run a project-defined command.
    RunProjectCommand,
    /// Validate a debug configuration before launch.
    DapPrelaunchCheck,
    /// Launch a debuggee or debug helper process.
    DapDebuggeeOrHelper,
    /// Run an external formatter.
    ExternalFormatter,
    /// Run an external linter or critic.
    ExternalLinterOrCritic,
    /// Compile-check the current saved file with a real interpreter.
    PerlCompileCurrentSavedFile,
    /// Compile-check on every save, on a persistent cadence.
    TrustedCompileOnSave,
    /// Probe an interpreter or module for identity facts.
    InterpreterOrModuleProbe,
    /// Run an oracle or corpus harness.
    OracleOrCorpusHarness,
    /// Run a hermetic CI process.
    CiHermeticProcess,
    /// Hold an interactive external session.
    InteractiveExternalSession,
}

impl OperationProfile {
    /// Every reviewed profile in deterministic order.
    pub const ALL: [Self; 15] = [
        Self::SourceAnalysisOnly,
        Self::ModuleResolutionExternalRead,
        Self::RunCurrentSavedFile,
        Self::RunTests,
        Self::RunProjectCommand,
        Self::DapPrelaunchCheck,
        Self::DapDebuggeeOrHelper,
        Self::ExternalFormatter,
        Self::ExternalLinterOrCritic,
        Self::PerlCompileCurrentSavedFile,
        Self::TrustedCompileOnSave,
        Self::InterpreterOrModuleProbe,
        Self::OracleOrCorpusHarness,
        Self::CiHermeticProcess,
        Self::InteractiveExternalSession,
    ];

    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::SourceAnalysisOnly => "source_analysis_only",
            Self::ModuleResolutionExternalRead => "module_resolution_external_read",
            Self::RunCurrentSavedFile => "run_current_saved_file",
            Self::RunTests => "run_tests",
            Self::RunProjectCommand => "run_project_command",
            Self::DapPrelaunchCheck => "dap_prelaunch_check",
            Self::DapDebuggeeOrHelper => "dap_debuggee_or_helper",
            Self::ExternalFormatter => "external_formatter",
            Self::ExternalLinterOrCritic => "external_linter_or_critic",
            Self::PerlCompileCurrentSavedFile => "perl_compile_current_saved_file",
            Self::TrustedCompileOnSave => "trusted_compile_on_save",
            Self::InterpreterOrModuleProbe => "interpreter_or_module_probe",
            Self::OracleOrCorpusHarness => "oracle_or_corpus_harness",
            Self::CiHermeticProcess => "ci_hermetic_process",
            Self::InteractiveExternalSession => "interactive_external_session",
        }
    }
}

/// Which scope kinds a profile may run under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredScope {
    /// Only an editor workspace scope.
    EditorWorkspaceOnly,
    /// Only a CI/hermetic scope.
    CiHermeticOnly,
    /// Either scope kind, evaluated under that scope's own authority class.
    EitherScope,
}

impl RequiredScope {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::EditorWorkspaceOnly => "editor_workspace_only",
            Self::CiHermeticOnly => "ci_hermetic_only",
            Self::EitherScope => "either_scope",
        }
    }

    const fn admits(self, kind: TrustScopeKind) -> bool {
        matches!(
            (self, kind),
            (Self::EitherScope, _)
                | (Self::EditorWorkspaceOnly, TrustScopeKind::EditorWorkspace)
                | (Self::CiHermeticOnly, TrustScopeKind::CiHermetic)
        )
    }
}

/// The reviewed authority a profile requires.
///
/// This is registry data, not caller input: obtain it with
/// [`OperationTrustRequirement::for_profile`]. It states required capabilities
/// and explicit non-claims. It deliberately encodes no domain output semantics
/// and no process budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationTrustRequirement {
    /// Registry version this entry was compiled from.
    pub registry_version: u32,
    /// The profile this requirement describes.
    pub profile: OperationProfile,
    /// Capabilities that must all be granted for the operation to proceed.
    pub required: CapabilitySet,
    /// Scope kinds this profile may run under.
    pub scope: RequiredScope,
    /// Stable codes for what authorizing this profile does not prove.
    pub non_claims: Vec<String>,
}

impl OperationTrustRequirement {
    /// The reviewed requirement for one profile.
    #[must_use]
    pub fn for_profile(profile: OperationProfile) -> Self {
        use ExecutionCapability as Cap;

        let (required, scope): (&[Cap], RequiredScope) = match profile {
            OperationProfile::SourceAnalysisOnly => {
                (&[Cap::SourceAnalysis], RequiredScope::EitherScope)
            }
            OperationProfile::ModuleResolutionExternalRead => {
                (&[Cap::SourceAnalysis, Cap::ExternalRead], RequiredScope::EitherScope)
            }
            OperationProfile::RunCurrentSavedFile => (
                &[Cap::SourceAnalysis, Cap::ExecutableTool, Cap::ProjectCodeExecution],
                RequiredScope::EditorWorkspaceOnly,
            ),
            OperationProfile::RunTests => (
                &[
                    Cap::SourceAnalysis,
                    Cap::ProjectConfiguration,
                    Cap::ExecutableTool,
                    Cap::ProjectCodeExecution,
                ],
                RequiredScope::EitherScope,
            ),
            OperationProfile::RunProjectCommand => (
                &[Cap::ProjectConfiguration, Cap::ExecutableTool, Cap::ProjectCodeExecution],
                RequiredScope::EitherScope,
            ),
            // Prelaunch validation inspects a configuration and the tool it
            // names. It does not itself run project code.
            OperationProfile::DapPrelaunchCheck => (
                &[Cap::SourceAnalysis, Cap::ProjectConfiguration, Cap::ExecutableTool],
                RequiredScope::EditorWorkspaceOnly,
            ),
            OperationProfile::DapDebuggeeOrHelper => (
                &[
                    Cap::ExecutableTool,
                    Cap::EnvironmentCodeLoading,
                    Cap::ProjectCodeExecution,
                    Cap::InteractiveSession,
                ],
                RequiredScope::EditorWorkspaceOnly,
            ),
            OperationProfile::ExternalFormatter | OperationProfile::ExternalLinterOrCritic => {
                (&[Cap::ExecutableTool], RequiredScope::EitherScope)
            }
            // `perl -c` runs BEGIN blocks and loads modules, so a compile check
            // is project-code execution, not a read-only analysis.
            OperationProfile::PerlCompileCurrentSavedFile => (
                &[
                    Cap::SourceAnalysis,
                    Cap::ExecutableTool,
                    Cap::EnvironmentCodeLoading,
                    Cap::ProjectCodeExecution,
                ],
                RequiredScope::EditorWorkspaceOnly,
            ),
            // Identical to a manual compile plus the cadence authority that
            // makes it repeat without a fresh user action.
            OperationProfile::TrustedCompileOnSave => (
                &[
                    Cap::SourceAnalysis,
                    Cap::ExecutableTool,
                    Cap::EnvironmentCodeLoading,
                    Cap::ProjectCodeExecution,
                    Cap::PersistentCadence,
                ],
                RequiredScope::EditorWorkspaceOnly,
            ),
            OperationProfile::InterpreterOrModuleProbe => {
                (&[Cap::ExecutableTool, Cap::EnvironmentCodeLoading], RequiredScope::EitherScope)
            }
            OperationProfile::OracleOrCorpusHarness => (
                &[Cap::ExternalRead, Cap::ExecutableTool, Cap::ProjectCodeExecution],
                RequiredScope::EitherScope,
            ),
            OperationProfile::CiHermeticProcess => (
                &[Cap::ProjectConfiguration, Cap::ExecutableTool, Cap::ProjectCodeExecution],
                RequiredScope::CiHermeticOnly,
            ),
            OperationProfile::InteractiveExternalSession => (
                &[Cap::ExecutableTool, Cap::ProjectCodeExecution, Cap::InteractiveSession],
                RequiredScope::EditorWorkspaceOnly,
            ),
        };

        Self {
            registry_version: OPERATION_REGISTRY_VERSION,
            profile,
            required: CapabilitySet::new(required.iter().copied()),
            scope,
            non_claims: vec![
                NON_CLAIM_PROCESS_PLAN_SAFETY.to_string(),
                NON_CLAIM_PROJECT_CODE_BENIGN.to_string(),
                NON_CLAIM_NO_SANDBOX.to_string(),
            ],
        }
    }

    /// Stable identity of this registry entry.
    #[must_use]
    pub fn identity(&self) -> String {
        stable_id(
            REQUIREMENT_ID_DOMAIN,
            &[
                &self.registry_version.to_string(),
                self.profile.identity_tag(),
                &self.required.tags().join(","),
                self.scope.identity_tag(),
            ],
        )
    }
}

/// Authorizing an operation does not prove its process plan safe.
pub const NON_CLAIM_PROCESS_PLAN_SAFETY: &str = "non_claim.process_plan_safety";
/// Authorizing an operation does not prove project code benign.
pub const NON_CLAIM_PROJECT_CODE_BENIGN: &str = "non_claim.project_code_benign";
/// Authorization is not a sandbox and provides no containment.
pub const NON_CLAIM_NO_SANDBOX: &str = "non_claim.no_sandbox";

// ---------------------------------------------------------------------------
// Input risk vocabulary
// ---------------------------------------------------------------------------

/// Stable class of one execution-bearing input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRiskClass {
    /// A path contained within the workspace root.
    WorkspaceContainedPath,
    /// An absolute, private, or system path outside the workspace.
    ExternalAbsolutePath,
    /// A symlinked or traversal path escaping the workspace root.
    SymlinkOrTraversalPath,
    /// A user- or machine-scoped setting.
    UserScopedSetting,
    /// A workspace- or resource-scoped setting.
    WorkspaceScopedSetting,
    /// Ambient `PATH` or working directory.
    AmbientPathOrCwd,
    /// Ambient Perl environment such as `PERL5LIB`, `PERL5OPT`, `PERLLIB`,
    /// `local::lib`, `perlbrew`, or `plenv`.
    AmbientPerlEnvironment,
    /// A project-controlled executable, command, argv, profile, or config file.
    ProjectExecutableOrCommand,
    /// An explicitly selected and verified tool or interpreter.
    SelectedVerifiedTool,
    /// An environment value that may carry a secret.
    SecretBearingValue,
    /// Provenance is unknown or unavailable.
    UnknownProvenance,
}

impl InputRiskClass {
    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::WorkspaceContainedPath => "workspace_contained_path",
            Self::ExternalAbsolutePath => "external_absolute_path",
            Self::SymlinkOrTraversalPath => "symlink_or_traversal_path",
            Self::UserScopedSetting => "user_scoped_setting",
            Self::WorkspaceScopedSetting => "workspace_scoped_setting",
            Self::AmbientPathOrCwd => "ambient_path_or_cwd",
            Self::AmbientPerlEnvironment => "ambient_perl_environment",
            Self::ProjectExecutableOrCommand => "project_executable_or_command",
            Self::SelectedVerifiedTool => "selected_verified_tool",
            Self::SecretBearingValue => "secret_bearing_value",
            Self::UnknownProvenance => "unknown_provenance",
        }
    }

    /// Whether a value of this class may carry a secret and must never reach a
    /// public explanation even in redacted-value form.
    #[must_use]
    pub const fn is_secret_bearing(self) -> bool {
        matches!(self, Self::SecretBearingValue)
    }
}

/// Disposition of one classified input under review.
///
/// "Suspicious" is not automatically malicious, and an accepted workspace trust
/// does not automatically accept every input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputDisposition {
    /// The input is accepted as authority for its class.
    Accepted,
    /// The input needs a separate, stronger authority before it counts.
    RequiresSeparateAuthority,
    /// The input needs an explicit confirmation before it counts.
    ConfirmationRequired,
    /// The input is accepted with named capabilities withheld.
    AcceptedLimited,
    /// The input is denied.
    Denied,
    /// The input's provenance or value could not be established.
    UnknownNotProven,
}

impl InputDisposition {
    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::RequiresSeparateAuthority => "requires_separate_authority",
            Self::ConfirmationRequired => "confirmation_required",
            Self::AcceptedLimited => "accepted_limited",
            Self::Denied => "denied",
            Self::UnknownNotProven => "unknown_not_proven",
        }
    }

    /// Whether this disposition makes the input active authority.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted | Self::AcceptedLimited)
    }
}

/// Stable identity of one classified input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClassifiedInputId(String);

impl ClassifiedInputId {
    /// String form of the identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClassifiedInputId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One execution-bearing input, classified and dispositioned.
///
/// The raw value never appears here: only a class, an authority, a stable
/// source identifier, and an optional value fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedInput {
    /// Stable identity derived from the classification evidence.
    pub id: ClassifiedInputId,
    /// Logical slot this input supplies, such as `tool.formatter`.
    pub semantic_key: String,
    /// Risk class.
    pub risk_class: InputRiskClass,
    /// Authority class carried over from the environment model.
    pub authority: EnvironmentInputAuthority,
    /// Reviewed disposition.
    pub disposition: InputDisposition,
    /// Fingerprint of the behavior-bearing value, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_fingerprint: Option<Digest>,
    /// Stable explanation code for the disposition.
    pub explanation_code: String,
}

impl ClassifiedInput {
    /// Classify one execution-bearing input.
    #[must_use]
    pub fn new(
        semantic_key: impl Into<String>,
        risk_class: InputRiskClass,
        authority: EnvironmentInputAuthority,
        disposition: InputDisposition,
        value_fingerprint: Option<Digest>,
        explanation_code: impl Into<String>,
    ) -> Self {
        let semantic_key = semantic_key.into();
        let explanation_code = explanation_code.into();
        let fingerprint = value_fingerprint.as_ref().map_or("", Digest::as_str);
        let id = ClassifiedInputId(stable_id(
            CLASSIFIED_INPUT_ID_DOMAIN,
            &[
                semantic_key.as_str(),
                risk_class.identity_tag(),
                authority.identity_tag(),
                disposition.identity_tag(),
                fingerprint,
                explanation_code.as_str(),
            ],
        ));
        Self {
            id,
            semantic_key,
            risk_class,
            authority,
            disposition,
            value_fingerprint,
            explanation_code,
        }
    }

    /// Whether this input is evaluated even when an intent does not name it.
    ///
    /// Some inputs are scope-level facts rather than per-operation choices, so
    /// an intent cannot narrow its declaration to escape their classification:
    ///
    /// - ambient process state is not opt-in. An ambient `PERL5LIB`,
    ///   `PERL5OPT`, `PATH`, or working directory reaches the interpreter
    ///   regardless of what the operation declared it would consume.
    /// - a path that escapes the workspace root is a hazard for the whole
    ///   scope. [`ExecutionCapability::OutsideRootPath`] is blanket authority
    ///   to leave the root, so granting it while a known traversal path sits in
    ///   the evidence would hand a consumer exactly the path that was denied.
    ///
    /// Slot-specific inputs are deliberately *not* inescapable: a denied
    /// formatter is not a reason to refuse an unrelated test run.
    #[must_use]
    pub const fn applies_regardless_of_intent(&self) -> bool {
        matches!(
            self.risk_class,
            InputRiskClass::AmbientPathOrCwd
                | InputRiskClass::AmbientPerlEnvironment
                | InputRiskClass::SymlinkOrTraversalPath
        )
    }

    fn push_identity(&self, material: &mut String) {
        push_field(material, "input.id", self.id.as_str());
        push_field(material, "input.key", self.semantic_key.as_str());
        push_field(material, "input.risk", self.risk_class.identity_tag());
        push_field(material, "input.authority", self.authority.identity_tag());
        push_field(material, "input.disposition", self.disposition.identity_tag());
        push_field(
            material,
            "input.value",
            self.value_fingerprint.as_ref().map_or("", Digest::as_str),
        );
        push_field(material, "input.explanation", self.explanation_code.as_str());
    }
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// Who requested the operation.
///
/// A user action is an input to authorization, never a substitute for it: an
/// explicit action does not override denied executable, environment, or path
/// authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthorizationActor {
    /// No actor: the operation was not explicitly requested by anyone.
    None,
    /// An explicit, attributable user action.
    ExplicitUserAction {
        /// Stable identity of the action.
        action_id: String,
    },
    /// A CI identity in a hermetic scope.
    CiIdentity {
        /// Stable identity of the CI principal.
        identity_id: String,
    },
}

impl AuthorizationActor {
    const fn identity_tag(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ExplicitUserAction { .. } => "explicit_user_action",
            Self::CiIdentity { .. } => "ci_identity",
        }
    }

    fn actor_id(&self) -> &str {
        match self {
            Self::None => "",
            Self::ExplicitUserAction { action_id } => action_id.as_str(),
            Self::CiIdentity { identity_id } => identity_id.as_str(),
        }
    }

    /// Whether this actor is an explicit user action.
    #[must_use]
    pub const fn is_explicit_user_action(&self) -> bool {
        matches!(self, Self::ExplicitUserAction { .. })
    }

    /// Whether this actor is a CI identity.
    #[must_use]
    pub const fn is_ci_identity(&self) -> bool {
        matches!(self, Self::CiIdentity { .. })
    }
}

/// A scoped, expiring capability grant.
///
/// An override is explicit, bound to one scope, bound to the policy generation
/// that issued it, and expirable. It can never defeat a policy denial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOverride {
    /// Stable identity of the grant.
    pub override_id: String,
    /// Exact scope the grant applies to.
    pub scope: TrustScope,
    /// Policy generation at which the grant was issued.
    pub granted_policy_generation: u64,
    /// Last policy generation at which the grant remains valid.
    pub expires_after_policy_generation: u64,
    /// Capabilities the grant supplies.
    pub capabilities: CapabilitySet,
}

impl SessionOverride {
    /// Whether the grant is current for `scope` at `policy_generation`.
    #[must_use]
    pub fn is_current_for(&self, scope: &TrustScope, policy_generation: u64) -> bool {
        self.scope == *scope
            && policy_generation >= self.granted_policy_generation
            && policy_generation <= self.expires_after_policy_generation
    }

    fn push_identity(&self, material: &mut String) {
        push_field(material, "override.id", self.override_id.as_str());
        self.scope.push_identity(material, "override.scope");
        push_field(material, "override.granted", &self.granted_policy_generation.to_string());
        push_field(material, "override.expires", &self.expires_after_policy_generation.to_string());
        push_field(material, "override.capabilities", &self.capabilities.tags().join(","));
    }
}

/// An administrator or policy denial of specific capabilities.
///
/// Policy denial dominates every local grant, including a current session
/// override and an explicit user action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDenial {
    /// Stable identity of the denying policy.
    pub policy_id: String,
    /// Capabilities the policy denies.
    pub denied: CapabilitySet,
    /// Stable explanation code.
    pub reason_code: String,
}

impl PolicyDenial {
    /// Construct a policy denial.
    #[must_use]
    pub fn new(
        policy_id: impl Into<String>,
        denied: CapabilitySet,
        reason_code: impl Into<String>,
    ) -> Self {
        Self { policy_id: policy_id.into(), denied, reason_code: reason_code.into() }
    }

    fn push_identity(&self, material: &mut String) {
        push_field(material, "policy.id", self.policy_id.as_str());
        push_field(material, "policy.denied", &self.denied.tags().join(","));
        push_field(material, "policy.reason", self.reason_code.as_str());
    }
}

/// Stable identity of one evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthorizationEvidenceId(String);

impl AuthorizationEvidenceId {
    /// String form of the identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AuthorizationEvidenceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The authority facts an authorization is evaluated against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationEvidence {
    /// Scope these facts belong to.
    pub scope: TrustScope,
    /// Workspace trust for an editor scope. Not consulted under a CI scope.
    pub trust: WorkspaceTrust,
    /// Who requested the operation.
    pub actor: AuthorizationActor,
    /// Generations these facts were observed at.
    pub generations: BoundGenerations,
    /// Classified execution-bearing inputs.
    pub inputs: Vec<ClassifiedInput>,
    /// A scoped capability grant, when one is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_override: Option<SessionOverride>,
    /// Policy denials in force.
    pub policy_denials: Vec<PolicyDenial>,
    /// Stable codes for what this evidence cannot establish.
    pub limitation_codes: Vec<String>,
}

impl AuthorizationEvidence {
    /// Stable identity of this evidence record.
    ///
    /// Identity is independent of the order inputs, policy denials, and
    /// limitation codes were supplied in: the same facts always yield the same
    /// identity.
    #[must_use]
    pub fn identity(&self) -> AuthorizationEvidenceId {
        let mut material = String::new();
        self.scope.push_identity(&mut material, "scope");
        push_field(&mut material, "trust", self.trust.identity_tag());
        push_field(&mut material, "actor.kind", self.actor.identity_tag());
        push_field(&mut material, "actor.id", self.actor.actor_id());
        self.generations.push_identity(&mut material, "generation");

        let mut inputs: Vec<&ClassifiedInput> = self.inputs.iter().collect();
        inputs.sort_by(|left, right| left.id.cmp(&right.id));
        for input in inputs {
            input.push_identity(&mut material);
        }

        if let Some(session_override) = &self.session_override {
            session_override.push_identity(&mut material);
        }

        let mut denials: Vec<&PolicyDenial> = self.policy_denials.iter().collect();
        denials.sort_by(|left, right| {
            (left.policy_id.as_str(), left.reason_code.as_str())
                .cmp(&(right.policy_id.as_str(), right.reason_code.as_str()))
        });
        for denial in denials {
            denial.push_identity(&mut material);
        }

        let mut codes: Vec<&String> = self.limitation_codes.iter().collect();
        codes.sort();
        for code in codes {
            push_field(&mut material, "limitation", code.as_str());
        }
        AuthorizationEvidenceId(stable_id(EVIDENCE_ID_DOMAIN, &[material.as_str()]))
    }

    /// Check structural invariants that must hold before evaluation.
    ///
    /// A failure here is not a denial: it means the evidence cannot be
    /// evaluated at all. [`authorize`] converts a failure into
    /// [`AuthorizationOutcome::NotProven`] rather than an allow.
    pub fn validate(&self) -> Result<(), AuthorizationError> {
        if self.scope.workspace_id.is_empty() {
            return Err(AuthorizationError::EmptyScopeWorkspaceId);
        }
        match &self.actor {
            AuthorizationActor::None => {}
            AuthorizationActor::ExplicitUserAction { action_id } => {
                if action_id.is_empty() {
                    return Err(AuthorizationError::EmptyActorId);
                }
                if self.scope.kind == TrustScopeKind::CiHermetic {
                    return Err(AuthorizationError::ActorScopeMismatch {
                        actor: "explicit_user_action",
                        scope: TrustScopeKind::CiHermetic,
                    });
                }
            }
            AuthorizationActor::CiIdentity { identity_id } => {
                if identity_id.is_empty() {
                    return Err(AuthorizationError::EmptyActorId);
                }
                if self.scope.kind == TrustScopeKind::EditorWorkspace {
                    return Err(AuthorizationError::ActorScopeMismatch {
                        actor: "ci_identity",
                        scope: TrustScopeKind::EditorWorkspace,
                    });
                }
            }
        }
        let mut seen: BTreeSet<&ClassifiedInputId> = BTreeSet::new();
        for input in &self.inputs {
            if input.semantic_key.is_empty() || input.explanation_code.is_empty() {
                return Err(AuthorizationError::EmptyInputField { input_id: input.id.clone() });
            }
            if !seen.insert(&input.id) {
                return Err(AuthorizationError::DuplicateInputId { input_id: input.id.clone() });
            }
        }
        if let Some(session_override) = &self.session_override {
            if session_override.override_id.is_empty() {
                return Err(AuthorizationError::EmptyOverrideId);
            }
            if session_override.expires_after_policy_generation
                < session_override.granted_policy_generation
            {
                return Err(AuthorizationError::OverrideExpiryBeforeGrant);
            }
        }
        for denial in &self.policy_denials {
            if denial.policy_id.is_empty() || denial.reason_code.is_empty() {
                return Err(AuthorizationError::EmptyPolicyField);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Intent
// ---------------------------------------------------------------------------

/// Why the operation was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionReasonClass {
    /// A direct, explicit user action.
    ExplicitUserAction,
    /// An automatic action after a save in a trusted workspace.
    TrustedPostSave,
    /// Debug-adapter prelaunch validation.
    DapPrelaunch,
    /// A test run.
    TestRun,
    /// An external tool invocation.
    ExternalTool,
    /// A project-defined runner.
    ProjectRunner,
    /// An identity or capability probe.
    Probe,
    /// An oracle or corpus harness job.
    OracleHarness,
    /// A hermetic CI job.
    CiHermetic,
}

impl ExecutionReasonClass {
    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::ExplicitUserAction => "explicit_user_action",
            Self::TrustedPostSave => "trusted_post_save",
            Self::DapPrelaunch => "dap_prelaunch",
            Self::TestRun => "test_run",
            Self::ExternalTool => "external_tool",
            Self::ProjectRunner => "project_runner",
            Self::Probe => "probe",
            Self::OracleHarness => "oracle_harness",
            Self::CiHermetic => "ci_hermetic",
        }
    }
}

/// Stable identity of one execution intent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecutionIntentId(String);

impl ExecutionIntentId {
    /// String form of the identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ExecutionIntentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One exact operation an actor wants to execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIntent {
    /// Reviewed operation profile.
    pub profile: OperationProfile,
    /// Why the operation was requested.
    pub reason_class: ExecutionReasonClass,
    /// Scope the operation runs in.
    pub scope: TrustScope,
    /// Generations the request was formed against.
    pub generations: BoundGenerations,
    /// Capabilities the caller declares it will use.
    ///
    /// This must cover everything the registry requires for the profile; a
    /// caller cannot ask for less authority than the operation uses.
    pub requested: CapabilitySet,
    /// Identities of the execution-bearing inputs this operation will consume.
    pub input_ids: Vec<ClassifiedInputId>,
    /// Stable statement of what this request does and does not cover.
    pub claim_boundary: String,
}

impl ExecutionIntent {
    /// Stable identity of this intent.
    #[must_use]
    pub fn identity(&self) -> ExecutionIntentId {
        let mut material = String::new();
        push_field(&mut material, "profile", self.profile.identity_tag());
        push_field(&mut material, "reason", self.reason_class.identity_tag());
        self.scope.push_identity(&mut material, "scope");
        self.generations.push_identity(&mut material, "generation");
        push_field(&mut material, "requested", &self.requested.tags().join(","));
        let mut input_ids: Vec<&ClassifiedInputId> = self.input_ids.iter().collect();
        input_ids.sort();
        for input_id in input_ids {
            push_field(&mut material, "input", input_id.as_str());
        }
        push_field(&mut material, "claim", self.claim_boundary.as_str());
        ExecutionIntentId(stable_id(INTENT_ID_DOMAIN, &[material.as_str()]))
    }

    /// Check structural invariants that must hold before evaluation.
    pub fn validate(&self) -> Result<(), AuthorizationError> {
        if self.scope.workspace_id.is_empty() {
            return Err(AuthorizationError::EmptyScopeWorkspaceId);
        }
        if self.claim_boundary.is_empty() {
            return Err(AuthorizationError::EmptyClaimBoundary);
        }
        let requirement = OperationTrustRequirement::for_profile(self.profile);
        if !self.requested.contains_all(&requirement.required) {
            return Err(AuthorizationError::UnderDeclaredCapabilities {
                missing: requirement.required.difference(&self.requested),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// The outcome class of an authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationOutcome {
    /// Every required capability is granted.
    Allowed,
    /// Every required capability is granted; named requested extras are not.
    AllowedLimited,
    /// A required capability needs explicit confirmation first.
    ConfirmationRequired,
    /// A required capability is denied.
    Denied,
    /// The operation is not supported in this scope or as declared.
    Unsupported,
    /// The evidence no longer matches the intent's generations.
    Stale,
    /// A required capability could not be established either way.
    NotProven,
}

impl AuthorizationOutcome {
    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::AllowedLimited => "allowed_limited",
            Self::ConfirmationRequired => "confirmation_required",
            Self::Denied => "denied",
            Self::Unsupported => "unsupported",
            Self::Stale => "stale",
            Self::NotProven => "not_proven",
        }
    }

    /// Whether the operation may proceed at all.
    ///
    /// Only [`Self::Allowed`] and [`Self::AllowedLimited`] permit execution,
    /// and [`Self::AllowedLimited`] permits it only within the exact granted
    /// capability set.
    #[must_use]
    pub const fn permits_execution(self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedLimited)
    }
}

/// The authority a user or administrator must change to alter an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionableAuthority {
    /// Workspace trust must be granted.
    WorkspaceTrust,
    /// A user- or machine-scoped setting must select the input explicitly.
    UserConfiguration,
    /// An administrator policy must be changed.
    PolicyAdministrator,
    /// A scoped session grant is required.
    SessionOverride,
    /// A CI identity is required.
    CiIdentity,
    /// The input's provenance must be established.
    InputProvenance,
    /// An explicit user action is required.
    ExplicitUserAction,
    /// Nothing is actionable; the operation is unsupported here.
    NotActionable,
}

impl ActionableAuthority {
    /// Stable machine-readable tag.
    #[must_use]
    pub const fn identity_tag(self) -> &'static str {
        match self {
            Self::WorkspaceTrust => "workspace_trust",
            Self::UserConfiguration => "user_configuration",
            Self::PolicyAdministrator => "policy_administrator",
            Self::SessionOverride => "session_override",
            Self::CiIdentity => "ci_identity",
            Self::InputProvenance => "input_provenance",
            Self::ExplicitUserAction => "explicit_user_action",
            Self::NotActionable => "not_actionable",
        }
    }
}

/// One typed reason contributing to an outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationReason {
    /// Stable reason code.
    pub code: String,
    /// Capability the reason concerns, when it is capability-specific.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<ExecutionCapability>,
    /// Input the reason concerns, when it is input-specific.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_id: Option<ClassifiedInputId>,
    /// What must change for the outcome to change.
    pub actionable_authority: ActionableAuthority,
}

/// What movement invalidates a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevalidationRequirement {
    /// Generations the decision is valid at.
    pub bound: BoundGenerations,
    /// Policy generation after which a supplying session override lapses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_expires_after_policy_generation: Option<u64>,
    /// Stable codes naming what must be re-derived on movement.
    pub revalidate_on: Vec<String>,
}

/// Stable identity of one decision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthorizationFingerprint(Digest);

impl AuthorizationFingerprint {
    /// Fingerprint string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for AuthorizationFingerprint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The compiled authorization decision for one intent.
///
/// Fields are private and there is no public constructor: [`authorize`] is the
/// only producer. A downstream consumer can read the granted set but cannot
/// widen it, and cannot assemble a decision from a client-supplied boolean.
///
/// The fingerprint detects staleness and transport corruption. It is not an
/// unforgeable token: authority comes from the evaluator that produced the
/// decision, not from the fingerprint alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAuthorizationDecision {
    schema_version: u32,
    outcome: AuthorizationOutcome,
    granted: CapabilitySet,
    omitted: CapabilitySet,
    reasons: Vec<AuthorizationReason>,
    intent_id: ExecutionIntentId,
    requirement_id: String,
    evidence_id: AuthorizationEvidenceId,
    scope: TrustScope,
    revalidation: RevalidationRequirement,
    non_claims: Vec<String>,
    fingerprint: AuthorizationFingerprint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionAuthorizationDecisionWire {
    schema_version: u32,
    outcome: AuthorizationOutcome,
    granted: CapabilitySet,
    omitted: CapabilitySet,
    reasons: Vec<AuthorizationReason>,
    intent_id: ExecutionIntentId,
    requirement_id: String,
    evidence_id: AuthorizationEvidenceId,
    scope: TrustScope,
    revalidation: RevalidationRequirement,
    non_claims: Vec<String>,
    fingerprint: AuthorizationFingerprint,
}

impl<'de> Deserialize<'de> for ExecutionAuthorizationDecision {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ExecutionAuthorizationDecisionWire::deserialize(deserializer)?;
        let decision = Self {
            schema_version: wire.schema_version,
            outcome: wire.outcome,
            granted: wire.granted,
            omitted: wire.omitted,
            reasons: wire.reasons,
            intent_id: wire.intent_id,
            requirement_id: wire.requirement_id,
            evidence_id: wire.evidence_id,
            scope: wire.scope,
            revalidation: wire.revalidation,
            non_claims: wire.non_claims,
            fingerprint: wire.fingerprint,
        };
        decision.validate().map_err(serde::de::Error::custom)?;
        Ok(decision)
    }
}

impl ExecutionAuthorizationDecision {
    /// Schema version of this decision.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Outcome class.
    #[must_use]
    pub const fn outcome(&self) -> AuthorizationOutcome {
        self.outcome
    }

    /// Exactly the capabilities granted.
    ///
    /// Empty for every outcome that does not permit execution.
    #[must_use]
    pub const fn granted(&self) -> &CapabilitySet {
        &self.granted
    }

    /// Requested capabilities deliberately withheld under
    /// [`AuthorizationOutcome::AllowedLimited`].
    #[must_use]
    pub const fn omitted(&self) -> &CapabilitySet {
        &self.omitted
    }

    /// Typed reasons contributing to the outcome.
    #[must_use]
    pub fn reasons(&self) -> &[AuthorizationReason] {
        &self.reasons
    }

    /// Identity of the evaluated intent.
    #[must_use]
    pub const fn intent_id(&self) -> &ExecutionIntentId {
        &self.intent_id
    }

    /// Identity of the registry requirement applied.
    #[must_use]
    pub fn requirement_id(&self) -> &str {
        &self.requirement_id
    }

    /// Identity of the evidence evaluated.
    #[must_use]
    pub const fn evidence_id(&self) -> &AuthorizationEvidenceId {
        &self.evidence_id
    }

    /// Scope this decision is bound to.
    #[must_use]
    pub const fn scope(&self) -> &TrustScope {
        &self.scope
    }

    /// What movement invalidates this decision.
    #[must_use]
    pub const fn revalidation(&self) -> &RevalidationRequirement {
        &self.revalidation
    }

    /// What this decision does not prove.
    #[must_use]
    pub fn non_claims(&self) -> &[String] {
        &self.non_claims
    }

    /// Deterministic identity of this decision.
    #[must_use]
    pub const fn fingerprint(&self) -> &AuthorizationFingerprint {
        &self.fingerprint
    }

    /// Whether this decision still applies at `scope` and `generations`.
    ///
    /// Any load-bearing movement makes a prior decision stale.
    #[must_use]
    pub fn is_current_for(&self, scope: &TrustScope, generations: &BoundGenerations) -> bool {
        self.scope == *scope && self.revalidation.bound == *generations
    }

    /// Whether the operation may use `capability` under this decision.
    #[must_use]
    pub fn permits(&self, capability: ExecutionCapability) -> bool {
        self.outcome.permits_execution() && self.granted.contains(capability)
    }

    /// Re-check invariants for a deserialized or reconstructed decision.
    ///
    /// Treat a failure as non-authoritative: do not consult [`Self::granted`]
    /// on an invalid value.
    pub fn validate(&self) -> Result<(), AuthorizationError> {
        if self.schema_version != EXECUTION_AUTHORIZATION_SCHEMA_VERSION {
            return Err(AuthorizationError::UnsupportedSchemaVersion {
                schema_version: self.schema_version,
            });
        }
        if !self.outcome.permits_execution() && !self.granted.is_empty() {
            return Err(AuthorizationError::GrantWithoutPermittingOutcome {
                outcome: self.outcome,
            });
        }
        if self.outcome != AuthorizationOutcome::AllowedLimited && !self.omitted.is_empty() {
            return Err(AuthorizationError::OmittedWithoutLimitedOutcome { outcome: self.outcome });
        }
        if self.outcome == AuthorizationOutcome::AllowedLimited && self.omitted.is_empty() {
            return Err(AuthorizationError::LimitedWithoutOmittedCapabilities);
        }
        let expected = compute_decision_fingerprint(
            self.outcome,
            &self.granted,
            &self.omitted,
            &self.reasons,
            &self.intent_id,
            self.requirement_id.as_str(),
            &self.evidence_id,
            &self.scope,
            &self.revalidation,
            &self.non_claims,
        );
        if self.fingerprint != expected {
            return Err(AuthorizationError::StaleFingerprint);
        }
        Ok(())
    }

    /// Redacted public explanation.
    ///
    /// Carries stable classes, digests, generations, and reason codes only. It
    /// never carries an executable path, an environment value, a secret,
    /// source text, configuration prose, or `Debug` output.
    #[must_use]
    pub fn public_explanation(&self) -> PublicAuthorizationExplanation {
        let blocked: Vec<&'static str> = self
            .reasons
            .iter()
            .filter_map(|reason| reason.capability)
            .filter(|&capability| !self.granted.contains(capability))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(ExecutionCapability::identity_tag)
            .collect();
        let actionable: Vec<&'static str> = self
            .reasons
            .iter()
            .map(|reason| reason.actionable_authority)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(ActionableAuthority::identity_tag)
            .collect();
        PublicAuthorizationExplanation {
            schema_version: self.schema_version,
            outcome: self.outcome,
            scope_kind: self.scope.kind,
            workspace_id: self.scope.workspace_id.clone(),
            granted: self.granted.tags().into_iter().map(str::to_string).collect(),
            blocked_capabilities: blocked.into_iter().map(str::to_string).collect(),
            omitted_capabilities: self.omitted.tags().into_iter().map(str::to_string).collect(),
            reason_codes: self
                .reasons
                .iter()
                .map(|reason| reason.code.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            actionable_authorities: actionable.into_iter().map(str::to_string).collect(),
            configuration_generation: self.revalidation.bound.configuration_generation,
            policy_generation: self.revalidation.bound.policy_generation,
            source_generation: self.revalidation.bound.source_generation,
            environment_fingerprint: self.revalidation.bound.environment_fingerprint.clone(),
            non_claims: self.non_claims.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

/// Redacted public authorization explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicAuthorizationExplanation {
    /// Schema version.
    pub schema_version: u32,
    /// Outcome class.
    pub outcome: AuthorizationOutcome,
    /// Scope kind.
    pub scope_kind: TrustScopeKind,
    /// Stable workspace identity.
    pub workspace_id: String,
    /// Granted capability tags.
    pub granted: Vec<String>,
    /// Capability tags named by a reason but not granted.
    pub blocked_capabilities: Vec<String>,
    /// Capability tags deliberately withheld under a limited allow.
    pub omitted_capabilities: Vec<String>,
    /// Stable reason codes in deterministic order.
    pub reason_codes: Vec<String>,
    /// Authorities a user or administrator can act on.
    pub actionable_authorities: Vec<String>,
    /// Configuration generation.
    pub configuration_generation: u64,
    /// Policy generation.
    pub policy_generation: u64,
    /// Source generation.
    pub source_generation: u64,
    /// Environment snapshot identity.
    pub environment_fingerprint: EnvironmentFingerprint,
    /// What the decision does not prove.
    pub non_claims: Vec<String>,
    /// Decision identity.
    pub fingerprint: AuthorizationFingerprint,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error raised while validating authorization inputs or a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationError {
    /// A scope carries an empty workspace identity.
    EmptyScopeWorkspaceId,
    /// An intent carries an empty claim boundary.
    EmptyClaimBoundary,
    /// An actor identity is empty.
    EmptyActorId,
    /// An actor class cannot appear in this scope kind.
    ActorScopeMismatch {
        /// Actor tag.
        actor: &'static str,
        /// Scope kind that rejected it.
        scope: TrustScopeKind,
    },
    /// A classified input has an empty required field.
    EmptyInputField {
        /// Input identity.
        input_id: ClassifiedInputId,
    },
    /// Two classified inputs share one identity.
    DuplicateInputId {
        /// Repeated identity.
        input_id: ClassifiedInputId,
    },
    /// A session override identity is empty.
    EmptyOverrideId,
    /// A session override expires before it is granted.
    OverrideExpiryBeforeGrant,
    /// A policy denial has an empty required field.
    EmptyPolicyField,
    /// The intent declares less authority than its profile requires.
    UnderDeclaredCapabilities {
        /// Required capabilities the intent failed to declare.
        missing: CapabilitySet,
    },
    /// A decision schema version is not supported by this crate.
    UnsupportedSchemaVersion {
        /// Observed schema version.
        schema_version: u32,
    },
    /// A non-permitting outcome carries granted capabilities.
    GrantWithoutPermittingOutcome {
        /// Observed outcome.
        outcome: AuthorizationOutcome,
    },
    /// Omitted capabilities appear on an outcome that is not a limited allow.
    OmittedWithoutLimitedOutcome {
        /// Observed outcome.
        outcome: AuthorizationOutcome,
    },
    /// A limited allow names no omitted capability.
    LimitedWithoutOmittedCapabilities,
    /// Advertised fingerprint does not match decision fields.
    StaleFingerprint,
}

impl std::fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyScopeWorkspaceId => formatter.write_str("trust scope workspace ID is empty"),
            Self::EmptyClaimBoundary => {
                formatter.write_str("execution intent claim boundary is empty")
            }
            Self::EmptyActorId => formatter.write_str("authorization actor identity is empty"),
            Self::ActorScopeMismatch { actor, scope } => {
                write!(
                    formatter,
                    "actor {actor} cannot supply authority in scope {}",
                    scope.identity_tag()
                )
            }
            Self::EmptyInputField { input_id } => {
                write!(formatter, "classified input {input_id} has an empty required field")
            }
            Self::DuplicateInputId { input_id } => {
                write!(formatter, "classified input {input_id} appears more than once")
            }
            Self::EmptyOverrideId => formatter.write_str("session override identity is empty"),
            Self::OverrideExpiryBeforeGrant => formatter
                .write_str("session override expires before the generation that granted it"),
            Self::EmptyPolicyField => {
                formatter.write_str("policy denial has an empty required field")
            }
            Self::UnderDeclaredCapabilities { missing } => {
                write!(
                    formatter,
                    "execution intent omits required capabilities: {}",
                    missing.tags().join(",")
                )
            }
            Self::UnsupportedSchemaVersion { schema_version } => {
                write!(
                    formatter,
                    "unsupported execution authorization schema version {schema_version}; expected {EXECUTION_AUTHORIZATION_SCHEMA_VERSION}"
                )
            }
            Self::GrantWithoutPermittingOutcome { outcome } => {
                write!(
                    formatter,
                    "outcome {} must not carry granted capabilities",
                    outcome.identity_tag()
                )
            }
            Self::OmittedWithoutLimitedOutcome { outcome } => {
                write!(
                    formatter,
                    "outcome {} must not carry omitted capabilities",
                    outcome.identity_tag()
                )
            }
            Self::LimitedWithoutOmittedCapabilities => {
                formatter.write_str("allowed_limited must name at least one omitted capability")
            }
            Self::StaleFingerprint => {
                formatter.write_str("authorization fingerprint does not match decision fields")
            }
        }
    }
}

impl std::error::Error for AuthorizationError {}

// ---------------------------------------------------------------------------
// Reason codes
// ---------------------------------------------------------------------------

/// Reason: the intent and evidence disagree about scope.
pub const REASON_SCOPE_MISMATCH: &str = "scope_mismatch";
/// Reason: evidence generations no longer match the intent's.
pub const REASON_GENERATION_MOVED: &str = "generation_moved";
/// Reason: the profile cannot run under this scope kind.
pub const REASON_SCOPE_NOT_ADMITTED: &str = "scope_not_admitted";
/// Reason: CI authority cannot be synthesized from editor workspace trust.
pub const REASON_CI_AUTHORITY_NOT_SYNTHESIZABLE: &str = "ci_authority_not_synthesizable";
/// Reason: the intent declares less authority than the profile requires.
pub const REASON_UNDER_DECLARED_CAPABILITIES: &str = "under_declared_capabilities";
/// Reason: the supplied intent or evidence failed structural validation.
pub const REASON_INVALID_REQUEST: &str = "invalid_request";
/// Reason: an administrator policy denies the capability.
pub const REASON_POLICY_DENIED: &str = "policy_denied";
/// Reason: workspace trust has not been granted.
pub const REASON_WORKSPACE_UNTRUSTED: &str = "workspace_untrusted";
/// Reason: workspace trust has not been decided.
pub const REASON_WORKSPACE_TRUST_UNKNOWN: &str = "workspace_trust_unknown";
/// Reason: the operation needs an explicit actor and has none.
pub const REASON_NO_EXPLICIT_ACTOR: &str = "no_explicit_actor";
/// Reason: no input supplies a verified executable or interpreter.
pub const REASON_NO_VERIFIED_TOOL: &str = "no_verified_tool";
/// Reason: the only tool-bearing input is project-controlled.
pub const REASON_PROJECT_SUPPLIED_EXECUTABLE: &str = "project_supplied_executable";
/// Reason: the only tool-bearing input comes from ambient `PATH` or cwd.
pub const REASON_AMBIENT_TOOL_SELECTION: &str = "ambient_tool_selection";
/// Reason: ambient Perl environment cannot supply code-loading authority.
pub const REASON_AMBIENT_ENVIRONMENT_DENIED: &str = "ambient_environment_denied";
/// Reason: a symlink or traversal path escapes the workspace root.
pub const REASON_PATH_ESCAPES_ROOT: &str = "path_escapes_root";
/// Reason: an external absolute path needs explicit confirmation.
pub const REASON_EXTERNAL_PATH_UNCONFIRMED: &str = "external_path_unconfirmed";
/// Reason: a workspace-scoped setting cannot supply user or machine authority.
pub const REASON_WORKSPACE_SETTING_CANNOT_GRANT_USER_AUTHORITY: &str =
    "workspace_setting_cannot_grant_user_authority";
/// Reason: a persistent cadence needs its own explicit user opt-in.
pub const REASON_CADENCE_NOT_AUTHORIZED: &str = "cadence_not_authorized";
/// Reason: an interactive session needs an explicit user action.
pub const REASON_INTERACTIVE_SESSION_NOT_AUTHORIZED: &str = "interactive_session_not_authorized";
/// Reason: an input's provenance could not be established.
pub const REASON_UNKNOWN_PROVENANCE: &str = "unknown_provenance";
/// Reason: a scoped session override supplied the capability.
pub const REASON_GRANTED_BY_SESSION_OVERRIDE: &str = "granted_by_session_override";
/// Reason: a session override exists but has lapsed or does not match scope.
pub const REASON_SESSION_OVERRIDE_NOT_CURRENT: &str = "session_override_not_current";
/// Reason: a requested capability beyond the profile requirement was withheld.
pub const REASON_REQUESTED_CAPABILITY_WITHHELD: &str = "requested_capability_withheld";

/// Revalidate when configuration generation moves.
pub const REVALIDATE_ON_CONFIGURATION_GENERATION: &str = "configuration_generation";
/// Revalidate when policy or trust generation moves.
pub const REVALIDATE_ON_POLICY_GENERATION: &str = "policy_generation";
/// Revalidate when source generation moves.
pub const REVALIDATE_ON_SOURCE_GENERATION: &str = "source_generation";
/// Revalidate when the environment snapshot fingerprint moves.
pub const REVALIDATE_ON_ENVIRONMENT_FINGERPRINT: &str = "environment_fingerprint";
/// Revalidate when the supplying session override expires.
pub const REVALIDATE_ON_OVERRIDE_EXPIRY: &str = "session_override_expiry";

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

/// Per-capability finding produced while evaluating one requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityFinding {
    Granted,
    Denied,
    ConfirmationRequired,
    NotProven,
}

/// Compile an [`ExecutionAuthorizationDecision`] from an intent and evidence.
///
/// This is the only producer of a decision. It is pure: it reads no
/// configuration, touches no filesystem, and spawns no process.
///
/// Evaluation fails closed. Structural problems, scope mismatches, generation
/// movement, unknown provenance, and missing evidence never produce an allow.
#[must_use]
pub fn authorize(
    intent: &ExecutionIntent,
    evidence: &AuthorizationEvidence,
) -> ExecutionAuthorizationDecision {
    let requirement = OperationTrustRequirement::for_profile(intent.profile);
    let intent_id = intent.identity();
    let evidence_id = evidence.identity();

    // Structural validation first: an unevaluable request is never an allow.
    if let Err(error) = intent.validate() {
        let code = if matches!(error, AuthorizationError::UnderDeclaredCapabilities { .. }) {
            REASON_UNDER_DECLARED_CAPABILITIES
        } else {
            REASON_INVALID_REQUEST
        };
        let outcome = if matches!(error, AuthorizationError::UnderDeclaredCapabilities { .. }) {
            AuthorizationOutcome::Unsupported
        } else {
            AuthorizationOutcome::NotProven
        };
        return finish(
            outcome,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            vec![reason(code, None, None, ActionableAuthority::NotActionable)],
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }
    if evidence.validate().is_err() {
        return finish(
            AuthorizationOutcome::NotProven,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            vec![reason(REASON_INVALID_REQUEST, None, None, ActionableAuthority::NotActionable)],
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }

    // The intent and its evidence must describe the same subject.
    if intent.scope != evidence.scope {
        return finish(
            AuthorizationOutcome::NotProven,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            vec![reason(REASON_SCOPE_MISMATCH, None, None, ActionableAuthority::NotActionable)],
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }
    if intent.generations != evidence.generations {
        return finish(
            AuthorizationOutcome::Stale,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            vec![reason(REASON_GENERATION_MOVED, None, None, ActionableAuthority::NotActionable)],
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }

    // CI/hermetic authority is a separate class from editor workspace trust.
    if !requirement.scope.admits(intent.scope.kind) {
        let code = if requirement.scope == RequiredScope::CiHermeticOnly {
            REASON_CI_AUTHORITY_NOT_SYNTHESIZABLE
        } else {
            REASON_SCOPE_NOT_ADMITTED
        };
        let actionable = if requirement.scope == RequiredScope::CiHermeticOnly {
            ActionableAuthority::CiIdentity
        } else {
            ActionableAuthority::NotActionable
        };
        return finish(
            AuthorizationOutcome::Denied,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            vec![reason(code, None, None, actionable)],
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }

    let inputs = relevant_inputs(intent, evidence);
    let mut reasons: Vec<AuthorizationReason> = Vec::new();
    let mut granted: Vec<ExecutionCapability> = Vec::new();
    let mut denied = false;
    let mut not_proven = false;
    let mut confirmation = false;

    for capability in requirement.required.iter() {
        let finding = evaluate_capability(capability, evidence, &inputs, &mut reasons);
        match finding {
            CapabilityFinding::Granted => granted.push(capability),
            CapabilityFinding::Denied => denied = true,
            CapabilityFinding::NotProven => not_proven = true,
            CapabilityFinding::ConfirmationRequired => confirmation = true,
        }
    }

    // Outcome precedence: a denial dominates every weaker signal, and an
    // unproven capability never degrades into a confirmation prompt.
    if denied {
        return finish(
            AuthorizationOutcome::Denied,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            reasons,
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }
    if not_proven {
        return finish(
            AuthorizationOutcome::NotProven,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            reasons,
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }
    if confirmation {
        return finish(
            AuthorizationOutcome::ConfirmationRequired,
            CapabilitySet::empty(),
            CapabilitySet::empty(),
            reasons,
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        );
    }

    // Every required capability is granted. Requested extras beyond the
    // registry requirement are evaluated too, and named when withheld.
    let extras = intent.requested.difference(&requirement.required);
    let mut omitted: Vec<ExecutionCapability> = Vec::new();
    for capability in extras.iter() {
        match evaluate_capability(capability, evidence, &inputs, &mut reasons) {
            CapabilityFinding::Granted => granted.push(capability),
            _ => {
                omitted.push(capability);
                reasons.push(reason(
                    REASON_REQUESTED_CAPABILITY_WITHHELD,
                    Some(capability),
                    None,
                    ActionableAuthority::UserConfiguration,
                ));
            }
        }
    }

    if omitted.is_empty() {
        finish(
            AuthorizationOutcome::Allowed,
            CapabilitySet::new(granted),
            CapabilitySet::empty(),
            reasons,
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        )
    } else {
        finish(
            AuthorizationOutcome::AllowedLimited,
            CapabilitySet::new(granted),
            CapabilitySet::new(omitted),
            reasons,
            &intent_id,
            &requirement,
            &evidence_id,
            intent,
            evidence,
        )
    }
}

/// The classified inputs an operation is evaluated against, in deterministic
/// order.
///
/// This is the intent's declared inputs *plus* every ambient input in the
/// evidence. Ambient state is process-wide: an ambient `PERL5LIB` or an ambient
/// `PATH` reaches the interpreter whether or not the intent named it, so
/// leaving one out of the declaration must never buy more authority than
/// declaring it would. See [`ClassifiedInput::applies_regardless_of_intent`].
///
/// Sorting here makes evaluation independent of the order a producer happened
/// to collect inputs in, so the same facts always cite the same input.
fn relevant_inputs<'a>(
    intent: &ExecutionIntent,
    evidence: &'a AuthorizationEvidence,
) -> Vec<&'a ClassifiedInput> {
    let wanted: BTreeSet<&ClassifiedInputId> = intent.input_ids.iter().collect();
    let mut inputs: Vec<&'a ClassifiedInput> = evidence
        .inputs
        .iter()
        .filter(|&input| wanted.contains(&input.id) || input.applies_regardless_of_intent())
        .collect();
    inputs.sort_by(|left, right| left.id.cmp(&right.id));
    inputs
}

fn evaluate_capability(
    capability: ExecutionCapability,
    evidence: &AuthorizationEvidence,
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    // Policy denial dominates every local grant, including a current override.
    for denial in &evidence.policy_denials {
        if denial.denied.contains(capability) {
            reasons.push(reason(
                REASON_POLICY_DENIED,
                Some(capability),
                None,
                ActionableAuthority::PolicyAdministrator,
            ));
            return CapabilityFinding::Denied;
        }
    }

    let base = evaluate_capability_from_facts(capability, evidence, inputs, reasons);
    if base == CapabilityFinding::Granted {
        return base;
    }

    // A scoped, unexpired override may supply what the base facts withheld.
    // It can never supply a policy-denied capability: that returned above.
    if let Some(session_override) = &evidence.session_override
        && session_override.capabilities.contains(capability)
    {
        if session_override.is_current_for(&evidence.scope, evidence.generations.policy_generation)
        {
            reasons.push(reason(
                REASON_GRANTED_BY_SESSION_OVERRIDE,
                Some(capability),
                None,
                ActionableAuthority::SessionOverride,
            ));
            return CapabilityFinding::Granted;
        }
        reasons.push(reason(
            REASON_SESSION_OVERRIDE_NOT_CURRENT,
            Some(capability),
            None,
            ActionableAuthority::SessionOverride,
        ));
    }

    base
}

fn evaluate_capability_from_facts(
    capability: ExecutionCapability,
    evidence: &AuthorizationEvidence,
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    // An input whose provenance is unknown blocks whatever it would support.
    if let Some(input) =
        inputs.iter().copied().find(|&input| input.risk_class == InputRiskClass::UnknownProvenance)
        && capability.is_execution_bearing()
    {
        reasons.push(reason(
            REASON_UNKNOWN_PROVENANCE,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::InputProvenance,
        ));
        return CapabilityFinding::NotProven;
    }

    match capability {
        // Source analysis never needs execution authority. A restricted
        // workspace can still be parsed and indexed.
        ExecutionCapability::SourceAnalysis => CapabilityFinding::Granted,

        ExecutionCapability::ExternalRead => match execution_authority(evidence) {
            ExecutionAuthority::Established => CapabilityFinding::Granted,
            ExecutionAuthority::Absent => {
                // Reading is weaker than executing: an explicit action is
                // enough even without full project-execution authority.
                if evidence.actor.is_explicit_user_action() {
                    CapabilityFinding::Granted
                } else {
                    reasons.push(reason(
                        REASON_WORKSPACE_UNTRUSTED,
                        Some(capability),
                        None,
                        ActionableAuthority::WorkspaceTrust,
                    ));
                    CapabilityFinding::Denied
                }
            }
            ExecutionAuthority::Undecided => {
                reasons.push(reason(
                    REASON_WORKSPACE_TRUST_UNKNOWN,
                    Some(capability),
                    None,
                    ActionableAuthority::WorkspaceTrust,
                ));
                CapabilityFinding::NotProven
            }
        },

        // Treating project-controlled configuration as authority requires
        // workspace trust, or a CI identity in a hermetic scope.
        ExecutionCapability::ProjectConfiguration => match execution_authority(evidence) {
            ExecutionAuthority::Established => CapabilityFinding::Granted,
            ExecutionAuthority::Absent => {
                reasons.push(reason(
                    REASON_WORKSPACE_UNTRUSTED,
                    Some(capability),
                    None,
                    ActionableAuthority::WorkspaceTrust,
                ));
                CapabilityFinding::Denied
            }
            ExecutionAuthority::Undecided => {
                reasons.push(reason(
                    REASON_WORKSPACE_TRUST_UNKNOWN,
                    Some(capability),
                    None,
                    ActionableAuthority::WorkspaceTrust,
                ));
                CapabilityFinding::NotProven
            }
        },

        ExecutionCapability::ExecutableTool => evaluate_executable_tool(inputs, reasons),

        ExecutionCapability::EnvironmentCodeLoading => {
            evaluate_environment_code_loading(evidence, inputs, reasons)
        }

        // Trusted source configuration is weaker than trusted project
        // execution: trust alone is not enough, an explicit actor is required.
        ExecutionCapability::ProjectCodeExecution => match execution_authority(evidence) {
            ExecutionAuthority::Established => {
                if matches!(evidence.actor, AuthorizationActor::None) {
                    reasons.push(reason(
                        REASON_NO_EXPLICIT_ACTOR,
                        Some(capability),
                        None,
                        ActionableAuthority::ExplicitUserAction,
                    ));
                    CapabilityFinding::ConfirmationRequired
                } else {
                    CapabilityFinding::Granted
                }
            }
            ExecutionAuthority::Absent => {
                reasons.push(reason(
                    REASON_WORKSPACE_UNTRUSTED,
                    Some(capability),
                    None,
                    ActionableAuthority::WorkspaceTrust,
                ));
                CapabilityFinding::Denied
            }
            ExecutionAuthority::Undecided => {
                reasons.push(reason(
                    REASON_WORKSPACE_TRUST_UNKNOWN,
                    Some(capability),
                    None,
                    ActionableAuthority::WorkspaceTrust,
                ));
                CapabilityFinding::NotProven
            }
        },

        ExecutionCapability::OutsideRootPath => evaluate_outside_root_path(inputs, reasons),

        ExecutionCapability::PersistentCadence => evaluate_persistent_cadence(inputs, reasons),

        ExecutionCapability::InteractiveSession => {
            if evidence.actor.is_explicit_user_action()
                && execution_authority(evidence) == ExecutionAuthority::Established
            {
                CapabilityFinding::Granted
            } else {
                reasons.push(reason(
                    REASON_INTERACTIVE_SESSION_NOT_AUTHORIZED,
                    Some(capability),
                    None,
                    ActionableAuthority::ExplicitUserAction,
                ));
                CapabilityFinding::ConfirmationRequired
            }
        }
    }
}

/// Whether the scope's own authority class is established.
///
/// Editor scopes read workspace trust; hermetic scopes read the CI identity.
/// Neither is ever synthesized from the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionAuthority {
    Established,
    Absent,
    Undecided,
}

fn execution_authority(evidence: &AuthorizationEvidence) -> ExecutionAuthority {
    match evidence.scope.kind {
        TrustScopeKind::EditorWorkspace => match evidence.trust {
            WorkspaceTrust::Trusted => ExecutionAuthority::Established,
            WorkspaceTrust::Untrusted => ExecutionAuthority::Absent,
            WorkspaceTrust::Unknown => ExecutionAuthority::Undecided,
        },
        TrustScopeKind::CiHermetic => {
            if evidence.actor.is_ci_identity() {
                ExecutionAuthority::Established
            } else {
                ExecutionAuthority::Absent
            }
        }
    }
}

fn evaluate_executable_tool(
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    let capability = ExecutionCapability::ExecutableTool;

    if inputs.iter().any(|&input| {
        input.risk_class == InputRiskClass::SelectedVerifiedTool && input.disposition.is_accepted()
    }) {
        return CapabilityFinding::Granted;
    }

    // A project-supplied executable is project-controlled authority. An
    // explicit user action does not upgrade it.
    if let Some(input) = inputs
        .iter()
        .copied()
        .find(|&input| input.risk_class == InputRiskClass::ProjectExecutableOrCommand)
    {
        reasons.push(reason(
            REASON_PROJECT_SUPPLIED_EXECUTABLE,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::UserConfiguration,
        ));
        return CapabilityFinding::Denied;
    }

    if let Some(input) =
        inputs.iter().copied().find(|&input| input.risk_class == InputRiskClass::AmbientPathOrCwd)
    {
        reasons.push(reason(
            REASON_AMBIENT_TOOL_SELECTION,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::UserConfiguration,
        ));
        return CapabilityFinding::ConfirmationRequired;
    }

    reasons.push(reason(
        REASON_NO_VERIFIED_TOOL,
        Some(capability),
        None,
        ActionableAuthority::UserConfiguration,
    ));
    CapabilityFinding::NotProven
}

fn evaluate_environment_code_loading(
    evidence: &AuthorizationEvidence,
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    let capability = ExecutionCapability::EnvironmentCodeLoading;

    // Ambient PERL5LIB/PERL5OPT and friends never supply code-loading
    // authority. An explicitly reviewed activation is a different input.
    if let Some(input) = inputs.iter().copied().find(|&input| {
        input.risk_class == InputRiskClass::AmbientPerlEnvironment
            && input.authority == EnvironmentInputAuthority::Ambient
    }) {
        reasons.push(reason(
            REASON_AMBIENT_ENVIRONMENT_DENIED,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::UserConfiguration,
        ));
        return CapabilityFinding::Denied;
    }

    if inputs.iter().any(|&input| {
        input.risk_class == InputRiskClass::AmbientPerlEnvironment
            && input.authority == EnvironmentInputAuthority::ExplicitEnvironment
            && input.disposition.is_accepted()
    }) {
        return CapabilityFinding::Granted;
    }

    // No environment input at all: loading uses the verified interpreter's own
    // defaults, which the scope's execution authority already covers.
    match execution_authority(evidence) {
        ExecutionAuthority::Established => CapabilityFinding::Granted,
        ExecutionAuthority::Absent => {
            reasons.push(reason(
                REASON_WORKSPACE_UNTRUSTED,
                Some(capability),
                None,
                ActionableAuthority::WorkspaceTrust,
            ));
            CapabilityFinding::Denied
        }
        ExecutionAuthority::Undecided => {
            reasons.push(reason(
                REASON_WORKSPACE_TRUST_UNKNOWN,
                Some(capability),
                None,
                ActionableAuthority::WorkspaceTrust,
            ));
            CapabilityFinding::NotProven
        }
    }
}

fn evaluate_outside_root_path(
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    let capability = ExecutionCapability::OutsideRootPath;

    if let Some(input) = inputs
        .iter()
        .copied()
        .find(|&input| input.risk_class == InputRiskClass::SymlinkOrTraversalPath)
    {
        reasons.push(reason(
            REASON_PATH_ESCAPES_ROOT,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::NotActionable,
        ));
        return CapabilityFinding::Denied;
    }

    let external: Vec<&ClassifiedInput> = inputs
        .iter()
        .copied()
        .filter(|&input| input.risk_class == InputRiskClass::ExternalAbsolutePath)
        .collect();
    if external.is_empty() {
        // Nothing outside the root is in play.
        return CapabilityFinding::Granted;
    }
    if external.iter().all(|&input| {
        input.authority == EnvironmentInputAuthority::UserConfiguration
            && input.disposition.is_accepted()
    }) {
        return CapabilityFinding::Granted;
    }

    let blocking = external
        .iter()
        .find(|&input| {
            input.authority != EnvironmentInputAuthority::UserConfiguration
                || !input.disposition.is_accepted()
        })
        .map(|input| input.id.clone());
    reasons.push(reason(
        REASON_EXTERNAL_PATH_UNCONFIRMED,
        Some(capability),
        blocking,
        ActionableAuthority::UserConfiguration,
    ));
    CapabilityFinding::ConfirmationRequired
}

fn evaluate_persistent_cadence(
    inputs: &[&ClassifiedInput],
    reasons: &mut Vec<AuthorizationReason>,
) -> CapabilityFinding {
    let capability = ExecutionCapability::PersistentCadence;

    if inputs.iter().any(|&input| {
        input.risk_class == InputRiskClass::UserScopedSetting
            && input.authority == EnvironmentInputAuthority::UserConfiguration
            && input.disposition.is_accepted()
    }) {
        return CapabilityFinding::Granted;
    }

    // A workspace- or resource-scoped setting cannot manufacture user or
    // machine authority merely by claiming provenance.
    if let Some(input) = inputs
        .iter()
        .copied()
        .find(|&input| input.risk_class == InputRiskClass::WorkspaceScopedSetting)
    {
        reasons.push(reason(
            REASON_WORKSPACE_SETTING_CANNOT_GRANT_USER_AUTHORITY,
            Some(capability),
            Some(input.id.clone()),
            ActionableAuthority::UserConfiguration,
        ));
        return CapabilityFinding::Denied;
    }

    reasons.push(reason(
        REASON_CADENCE_NOT_AUTHORIZED,
        Some(capability),
        None,
        ActionableAuthority::UserConfiguration,
    ));
    CapabilityFinding::ConfirmationRequired
}

fn reason(
    code: &str,
    capability: Option<ExecutionCapability>,
    input_id: Option<ClassifiedInputId>,
    actionable_authority: ActionableAuthority,
) -> AuthorizationReason {
    AuthorizationReason { code: code.to_string(), capability, input_id, actionable_authority }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    outcome: AuthorizationOutcome,
    granted: CapabilitySet,
    omitted: CapabilitySet,
    mut reasons: Vec<AuthorizationReason>,
    intent_id: &ExecutionIntentId,
    requirement: &OperationTrustRequirement,
    evidence_id: &AuthorizationEvidenceId,
    intent: &ExecutionIntent,
    evidence: &AuthorizationEvidence,
) -> ExecutionAuthorizationDecision {
    reasons.sort_by(|left, right| {
        (left.code.as_str(), left.capability, left.input_id.as_ref()).cmp(&(
            right.code.as_str(),
            right.capability,
            right.input_id.as_ref(),
        ))
    });
    reasons.dedup();

    let override_expiry =
        evidence.session_override.as_ref().map(|item| item.expires_after_policy_generation);
    let mut revalidate_on = vec![
        REVALIDATE_ON_CONFIGURATION_GENERATION.to_string(),
        REVALIDATE_ON_POLICY_GENERATION.to_string(),
        REVALIDATE_ON_SOURCE_GENERATION.to_string(),
        REVALIDATE_ON_ENVIRONMENT_FINGERPRINT.to_string(),
    ];
    if override_expiry.is_some() {
        revalidate_on.push(REVALIDATE_ON_OVERRIDE_EXPIRY.to_string());
    }

    let revalidation = RevalidationRequirement {
        bound: intent.generations.clone(),
        override_expires_after_policy_generation: override_expiry,
        revalidate_on,
    };
    let requirement_id = requirement.identity();
    let non_claims = requirement.non_claims.clone();
    let fingerprint = compute_decision_fingerprint(
        outcome,
        &granted,
        &omitted,
        &reasons,
        intent_id,
        requirement_id.as_str(),
        evidence_id,
        &intent.scope,
        &revalidation,
        &non_claims,
    );

    ExecutionAuthorizationDecision {
        schema_version: EXECUTION_AUTHORIZATION_SCHEMA_VERSION,
        outcome,
        granted,
        omitted,
        reasons,
        intent_id: intent_id.clone(),
        requirement_id,
        evidence_id: evidence_id.clone(),
        scope: intent.scope.clone(),
        revalidation,
        non_claims,
        fingerprint,
    }
}

#[allow(clippy::too_many_arguments)]
fn compute_decision_fingerprint(
    outcome: AuthorizationOutcome,
    granted: &CapabilitySet,
    omitted: &CapabilitySet,
    reasons: &[AuthorizationReason],
    intent_id: &ExecutionIntentId,
    requirement_id: &str,
    evidence_id: &AuthorizationEvidenceId,
    scope: &TrustScope,
    revalidation: &RevalidationRequirement,
    non_claims: &[String],
) -> AuthorizationFingerprint {
    let mut material = String::new();
    push_field(&mut material, "schema", &EXECUTION_AUTHORIZATION_SCHEMA_VERSION.to_string());
    push_field(&mut material, "outcome", outcome.identity_tag());
    push_field(&mut material, "granted", &granted.tags().join(","));
    push_field(&mut material, "omitted", &omitted.tags().join(","));
    for item in reasons {
        push_field(&mut material, "reason.code", item.code.as_str());
        push_field(
            &mut material,
            "reason.capability",
            item.capability.map_or("", ExecutionCapability::identity_tag),
        );
        push_field(
            &mut material,
            "reason.input",
            item.input_id.as_ref().map_or("", ClassifiedInputId::as_str),
        );
        push_field(&mut material, "reason.actionable", item.actionable_authority.identity_tag());
    }
    push_field(&mut material, "intent", intent_id.as_str());
    push_field(&mut material, "requirement", requirement_id);
    push_field(&mut material, "evidence", evidence_id.as_str());
    scope.push_identity(&mut material, "scope");
    revalidation.bound.push_identity(&mut material, "revalidation.bound");
    push_field(
        &mut material,
        "revalidation.override_expiry",
        &revalidation
            .override_expires_after_policy_generation
            .map_or_else(String::new, |value| value.to_string()),
    );
    for code in &revalidation.revalidate_on {
        push_field(&mut material, "revalidation.on", code.as_str());
    }
    for claim in non_claims {
        push_field(&mut material, "non_claim", claim.as_str());
    }
    AuthorizationFingerprint(Digest::of(&material))
}

/// The reviewed registry as a deterministic map, for contract tests and
/// generated documentation.
#[must_use]
pub fn operation_registry() -> BTreeMap<&'static str, OperationTrustRequirement> {
    OperationProfile::ALL
        .iter()
        .map(|profile| (profile.identity_tag(), OperationTrustRequirement::for_profile(*profile)))
        .collect()
}
