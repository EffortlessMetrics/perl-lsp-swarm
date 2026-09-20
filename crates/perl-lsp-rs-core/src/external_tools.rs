//! Canonical roles and native replacements for external Perl tooling.
//!
//! This registry answers product-policy questions only. It does not discover,
//! install, execute, or parse tool-specific configuration. Domain registries
//! remain authoritative for Perl::Tidy options, Perl::Critic policies,
//! import-cleanup semantics, and debugger-peer capabilities.
//!
//! The registry deliberately distinguishes domains. Perl::Tidy and
//! App::perlimports own explicitly selected product adapters; Perl::Critic and
//! Perl::LanguageServer do not, and cannot be configured into one.

use serde::Serialize;
use std::collections::BTreeSet;

/// Stable identity for a reviewed external Perl tool or integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolId {
    /// `Perl::LanguageServer`, retained only as an external conformance oracle.
    PerlLanguageServer,
    /// Perl::Tidy / the `perltidy` executable.
    PerlTidy,
    /// Perl::Critic / the `perlcritic` executable.
    PerlCritic,
    /// `App::perlimports` / the `perlimports` executable.
    Perlimports,
    /// `Devel::ptkdb`, an explicit optional debugger peer.
    Ptkdb,
}

/// A bounded role an external tool may hold around the native product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolRole {
    /// Read familiar configuration and explain its native mapping, process-free.
    ConfigurationCompatibility,
    /// Execute an explicitly selected external implementation as a product adapter.
    ExplicitExternalAdapter,
    /// Observe external behavior in comparison infrastructure.
    ///
    /// [`ExternalToolPolicy::external_execution_support`] states whether that
    /// comparison is repository-only or shares an authorized product adapter.
    ConformanceOracle,
    /// Cooperate as an explicitly selected peer while the native product keeps protocol ownership.
    ExplicitOptionalPeer,
}

/// Support level for reading an external tool's configuration without running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigReaderSupport {
    /// No configuration reader is authorized.
    None,
    /// A reader is planned but not yet behavior-backed.
    Planned,
    /// A bounded subset is behavior-backed and unsupported input remains visible.
    Partial,
    /// The reviewed supported grammar is behavior-backed.
    Supported,
}

/// Highest class of external execution the product authorizes for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalExecutionSupport {
    /// The product never runs this executable.
    None,
    /// Execution is limited to repository and developer conformance entrypoints.
    ///
    /// No shipped product surface may run the tool, and no user setting can
    /// enable one.
    RepositoryConformanceOnly,
    /// A shipped adapter may run the tool after an explicit user selection.
    ExplicitProductAdapter,
}

/// Whether a product runtime or editor surface may ever select this tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEnablement {
    /// No product runtime or editor enablement exists, and none may be added.
    Forbidden,
    /// A user action or configuration decision is required before use.
    ExplicitUserAction,
    /// Enabled merely by discovery. Never valid; retained so validation rejects it.
    ImplicitOnDiscovery,
}

/// Source-state precondition an external adapter must satisfy before running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRequirement {
    /// No source-state precondition.
    None,
    /// The adapter runs only against a saved file on disk.
    SavedFile,
}

/// Scope in which installation guidance may be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallHelpScope {
    /// No normal installation guidance is offered.
    None,
    /// Guidance may be shown after an explicit compatibility-mode request.
    UserRequestedCompatibility,
    /// Guidance is limited to repository development/conformance setup.
    DeveloperConformance,
}

/// Security and trust owner for interaction with an external implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolTrustClass {
    /// Repository-only test infrastructure with pinned identity and bounded receipts.
    RepositoryConformance,
    /// User-authorized external process execution through the process supervisor.
    ExplicitExternalProcess,
    /// Explicit authenticated debugger-peer session.
    ExplicitDebuggerPeer,
}

/// Delivery state of the native identity that replaces an external tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeReplacementDelivery {
    /// The tool is a peer or oracle with no native implementation replacing it.
    NotApplicable,
    /// The canonical identity is a reviewed ruling that has not shipped yet.
    Planned,
    /// The canonical identity ships today.
    Shipped,
}

/// Exact native identity replacing an external implementation.
///
/// Prose such as "native formatter" is not sufficient: consumers need the exact
/// product, package, library, and executable names so documentation, doctor,
/// packaging, and settings cannot drift into separate vocabularies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeReplacement {
    /// Exact native product/binary identities replacing the tool.
    pub products: &'static [&'static str],
    /// Exact native package (crate) identity, when one is ruled.
    pub package: Option<&'static str>,
    /// Exact native library identity, when one is ruled.
    pub library: Option<&'static str>,
    /// Exact native executable identity, when one is ruled.
    pub executable: Option<&'static str>,
    /// Native LSP surface consuming the replacement.
    pub lsp_consumer: Option<&'static str>,
    /// Whether the canonical identity ships today.
    pub delivery: NativeReplacementDelivery,
    /// Identity currently implementing the behavior when the ruling has not shipped.
    pub current_implementation: Option<&'static str>,
    /// Issue owning delivery and status of the native replacement.
    pub owner: &'static str,
}

/// Evidence requirements for a conformance-oracle role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceRequirements {
    /// Whether comparison must bind an exact pinned external tool version.
    pub pinned_version_required: bool,
    /// Whether comparison must emit bounded receipts binding tool, fixture, and candidate identity.
    pub receipt_required: bool,
    /// Issue owning the conformance evidence.
    pub owner: &'static str,
}

/// Policy and capability boundary for one external tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalToolPolicy {
    /// Stable registry identity.
    pub tool_id: ExternalToolId,
    /// Canonical user-facing name.
    pub canonical_name: &'static str,
    /// Exact aliases used for identity resolution and advisory detection.
    ///
    /// Aliases are not package payload patterns. Package inspection must use
    /// artifact-specific exact paths or basenames rather than substring scans
    /// over this vocabulary.
    pub aliases: &'static [&'static str],
    /// Product domain in which the tool is relevant.
    pub owned_domain: &'static str,
    /// Native product surface owning this domain.
    ///
    /// This is the host surface, not a replacement claim. See
    /// [`ExternalToolPolicy::native_replacement`] for the replacing identity.
    pub native_host: &'static str,
    /// Exact native identity replacing the external implementation.
    pub native_replacement: NativeReplacement,
    /// Authorized roles for this tool.
    pub roles: &'static [ExternalToolRole],
    /// Whether any external implementation payload is bundled in product artifacts.
    pub bundled: bool,
    /// Whether the native product requires this external implementation.
    pub required_for_native: bool,
    /// Whether the published native package depends on the external implementation.
    pub native_package_requires_external: bool,
    /// Whether ordinary startup may record an advisory candidate without executing it.
    pub may_auto_detect: bool,
    /// Whether discovery or PATH presence may select this tool automatically.
    pub may_auto_select: bool,
    /// Whether ordinary workspace opening may execute the tool.
    pub may_execute_on_workspace_open: bool,
    /// Whether a product runtime or editor surface may select this tool.
    pub runtime_enablement: RuntimeEnablement,
    /// Source-state precondition for an authorized external adapter.
    pub source_requirement: SourceRequirement,
    /// Familiar configuration files associated with the tool.
    pub config_files: &'static [&'static str],
    /// Current native configuration-reader posture.
    pub config_reader_support: ConfigReaderSupport,
    /// Issue owning the configuration reader, when one is authorized.
    pub config_reader_owner: Option<&'static str>,
    /// Whether the presence of a configuration file may authorize execution.
    ///
    /// Always false. Retained as an explicit field so the invariant is
    /// mechanically checked rather than merely documented.
    pub config_presence_authorizes_execution: bool,
    /// Highest authorized external execution class.
    pub external_execution_support: ExternalExecutionSupport,
    /// Issue owning environment, process identity, and execution.
    pub external_execution_owner: Option<&'static str>,
    /// Conformance evidence requirements, when the oracle role is authorized.
    pub conformance: Option<ConformanceRequirements>,
    /// Whether external evidence may promote native product readiness.
    ///
    /// Always false. External comparison never changes native health.
    pub evidence_promotes_native_readiness: bool,
    /// Issue owning validation of adapter output before it becomes an edit.
    pub candidate_output_validation_owner: Option<&'static str>,
    /// Scope in which install guidance may be offered.
    pub install_help_scope: InstallHelpScope,
    /// Security/trust class governing interaction.
    pub trust_class: ExternalToolTrustClass,
    /// Concise claim boundary for docs, doctor, and status projections.
    pub claim_boundary: &'static str,
    /// Issue or controller owning current evidence/status.
    pub status_owner: &'static str,
}

const PLS_ROLES: &[ExternalToolRole] = &[ExternalToolRole::ConformanceOracle];
const PERLTIDY_ROLES: &[ExternalToolRole] = &[
    ExternalToolRole::ConfigurationCompatibility,
    ExternalToolRole::ExplicitExternalAdapter,
    ExternalToolRole::ConformanceOracle,
];
const PERLCRITIC_ROLES: &[ExternalToolRole] =
    &[ExternalToolRole::ConfigurationCompatibility, ExternalToolRole::ConformanceOracle];
const PERLIMPORTS_ROLES: &[ExternalToolRole] =
    &[ExternalToolRole::ExplicitExternalAdapter, ExternalToolRole::ConformanceOracle];
const PTKDB_ROLES: &[ExternalToolRole] = &[ExternalToolRole::ExplicitOptionalPeer];

const PLS_ALIASES: &[&str] = &["Perl-LanguageServer", "pls"];
const PERLTIDY_ALIASES: &[&str] = &["perltidy"];
const PERLCRITIC_ALIASES: &[&str] = &["perlcritic"];
const PERLIMPORTS_ALIASES: &[&str] = &["perlimports"];
const PTKDB_ALIASES: &[&str] = &["ptkdb"];
const PERLTIDY_CONFIG: &[&str] = &[".perltidyrc"];
const PERLCRITIC_CONFIG: &[&str] = &[".perlcriticrc"];
const NO_CONFIG_FILES: &[&str] = &[];
const NO_NATIVE_PRODUCTS: &[&str] = &[];
const PLS_NATIVE_PRODUCTS: &[&str] = &["perllsp", "perl-dap"];
const PERLTIDY_NATIVE_PRODUCTS: &[&str] = &["perl-tidy"];

/// Reviewed external-tool policy registry.
pub const EXTERNAL_TOOL_REGISTRY: &[ExternalToolPolicy] = &[
    ExternalToolPolicy {
        tool_id: ExternalToolId::PerlLanguageServer,
        canonical_name: "Perl::LanguageServer",
        aliases: PLS_ALIASES,
        owned_domain: "language intelligence and historical DAP comparison",
        native_host: "perllsp + perl-dap",
        native_replacement: NativeReplacement {
            products: PLS_NATIVE_PRODUCTS,
            package: None,
            library: None,
            executable: Some("perllsp"),
            lsp_consumer: Some("perllsp"),
            delivery: NativeReplacementDelivery::Shipped,
            current_implementation: None,
            owner: "#6956 / #7210",
        },
        roles: PLS_ROLES,
        bundled: false,
        required_for_native: false,
        native_package_requires_external: false,
        may_auto_detect: false,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        runtime_enablement: RuntimeEnablement::Forbidden,
        source_requirement: SourceRequirement::None,
        config_files: NO_CONFIG_FILES,
        config_reader_support: ConfigReaderSupport::None,
        config_reader_owner: None,
        config_presence_authorizes_execution: false,
        external_execution_support: ExternalExecutionSupport::RepositoryConformanceOnly,
        external_execution_owner: Some("#7210"),
        conformance: Some(ConformanceRequirements {
            pinned_version_required: true,
            receipt_required: true,
            owner: "#7210",
        }),
        evidence_promotes_native_readiness: false,
        candidate_output_validation_owner: None,
        install_help_scope: InstallHelpScope::DeveloperConformance,
        trust_class: ExternalToolTrustClass::RepositoryConformance,
        claim_boundary: "repository-only conformance oracle; never a runtime backend",
        status_owner: "#6956 / #7210",
    },
    ExternalToolPolicy {
        tool_id: ExternalToolId::PerlTidy,
        canonical_name: "Perl::Tidy",
        aliases: PERLTIDY_ALIASES,
        owned_domain: "formatting",
        native_host: "perllsp",
        native_replacement: NativeReplacement {
            products: PERLTIDY_NATIVE_PRODUCTS,
            package: Some("perl-tidy"),
            library: Some("perl_tidy"),
            executable: Some("perl-tidy"),
            lsp_consumer: Some("perllsp"),
            delivery: NativeReplacementDelivery::Planned,
            current_implementation: Some("perl-lsp-perltidy"),
            owner: "#7411 / #8653 / #7143",
        },
        roles: PERLTIDY_ROLES,
        bundled: false,
        required_for_native: false,
        native_package_requires_external: false,
        may_auto_detect: true,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        runtime_enablement: RuntimeEnablement::ExplicitUserAction,
        source_requirement: SourceRequirement::None,
        config_files: PERLTIDY_CONFIG,
        config_reader_support: ConfigReaderSupport::Partial,
        config_reader_owner: Some("#8509"),
        config_presence_authorizes_execution: false,
        external_execution_support: ExternalExecutionSupport::ExplicitProductAdapter,
        external_execution_owner: Some("#7134"),
        conformance: Some(ConformanceRequirements {
            pinned_version_required: true,
            receipt_required: true,
            owner: "#7135",
        }),
        evidence_promotes_native_readiness: false,
        candidate_output_validation_owner: Some("#7056"),
        install_help_scope: InstallHelpScope::UserRequestedCompatibility,
        trust_class: ExternalToolTrustClass::ExplicitExternalProcess,
        claim_boundary: "native formatting is default; external execution is explicit compatibility only",
        status_owner: "#7056 / #7134 / #7135",
    },
    ExternalToolPolicy {
        tool_id: ExternalToolId::PerlCritic,
        canonical_name: "Perl::Critic",
        aliases: PERLCRITIC_ALIASES,
        owned_domain: "critic diagnostics",
        native_host: "perllsp",
        native_replacement: NativeReplacement {
            products: NO_NATIVE_PRODUCTS,
            package: None,
            library: None,
            executable: None,
            lsp_consumer: Some("perllsp"),
            delivery: NativeReplacementDelivery::Planned,
            current_implementation: None,
            owner: "#8253 / #9062 / #9068",
        },
        roles: PERLCRITIC_ROLES,
        bundled: false,
        required_for_native: false,
        native_package_requires_external: false,
        may_auto_detect: false,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        runtime_enablement: RuntimeEnablement::Forbidden,
        source_requirement: SourceRequirement::None,
        config_files: PERLCRITIC_CONFIG,
        config_reader_support: ConfigReaderSupport::Planned,
        config_reader_owner: Some("#7211"),
        config_presence_authorizes_execution: false,
        external_execution_support: ExternalExecutionSupport::RepositoryConformanceOnly,
        external_execution_owner: Some("#6987 / #7210"),
        conformance: Some(ConformanceRequirements {
            pinned_version_required: true,
            receipt_required: true,
            owner: "#6984 / #8225",
        }),
        evidence_promotes_native_readiness: false,
        candidate_output_validation_owner: None,
        install_help_scope: InstallHelpScope::DeveloperConformance,
        trust_class: ExternalToolTrustClass::RepositoryConformance,
        claim_boundary: "process-free .perlcriticrc compatibility plus repository-only oracle; no runtime, editor, or CLI adapter",
        status_owner: "#6997 / #7211 / #8253",
    },
    ExternalToolPolicy {
        tool_id: ExternalToolId::Perlimports,
        canonical_name: "App::perlimports",
        aliases: PERLIMPORTS_ALIASES,
        owned_domain: "import cleanup",
        native_host: "perllsp",
        native_replacement: NativeReplacement {
            products: NO_NATIVE_PRODUCTS,
            package: None,
            library: None,
            executable: None,
            lsp_consumer: Some("perllsp"),
            delivery: NativeReplacementDelivery::Planned,
            current_implementation: None,
            owner: "#8277",
        },
        roles: PERLIMPORTS_ROLES,
        bundled: false,
        required_for_native: false,
        native_package_requires_external: false,
        may_auto_detect: true,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        runtime_enablement: RuntimeEnablement::ExplicitUserAction,
        source_requirement: SourceRequirement::SavedFile,
        config_files: NO_CONFIG_FILES,
        config_reader_support: ConfigReaderSupport::None,
        config_reader_owner: None,
        config_presence_authorizes_execution: false,
        external_execution_support: ExternalExecutionSupport::ExplicitProductAdapter,
        external_execution_owner: Some("#8277"),
        conformance: Some(ConformanceRequirements {
            pinned_version_required: true,
            receipt_required: true,
            owner: "#8277",
        }),
        evidence_promotes_native_readiness: false,
        candidate_output_validation_owner: Some("#8277"),
        install_help_scope: InstallHelpScope::UserRequestedCompatibility,
        trust_class: ExternalToolTrustClass::ExplicitExternalProcess,
        claim_boundary: "explicit saved-file import-cleanup adapter; output is candidate evidence validated by the native plan owner",
        status_owner: "#8277",
    },
    ExternalToolPolicy {
        tool_id: ExternalToolId::Ptkdb,
        canonical_name: "Devel::ptkdb",
        aliases: PTKDB_ALIASES,
        owned_domain: "debugger engine and GUI peer",
        native_host: "perl-dap",
        native_replacement: NativeReplacement {
            products: NO_NATIVE_PRODUCTS,
            package: None,
            library: None,
            executable: None,
            lsp_consumer: None,
            delivery: NativeReplacementDelivery::NotApplicable,
            current_implementation: None,
            owner: "#4786 / #7276",
        },
        roles: PTKDB_ROLES,
        bundled: false,
        required_for_native: false,
        native_package_requires_external: false,
        may_auto_detect: false,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        runtime_enablement: RuntimeEnablement::ExplicitUserAction,
        source_requirement: SourceRequirement::None,
        config_files: NO_CONFIG_FILES,
        config_reader_support: ConfigReaderSupport::None,
        config_reader_owner: None,
        config_presence_authorizes_execution: false,
        external_execution_support: ExternalExecutionSupport::None,
        external_execution_owner: None,
        conformance: None,
        evidence_promotes_native_readiness: false,
        candidate_output_validation_owner: None,
        install_help_scope: InstallHelpScope::UserRequestedCompatibility,
        trust_class: ExternalToolTrustClass::ExplicitDebuggerPeer,
        claim_boundary: "explicit optional peer; perl-dap remains the DAP server",
        status_owner: "#4786 / #7276",
    },
];

/// Registry validation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExternalToolRegistryError {
    /// An external tool was marked as bundled.
    #[error("external tool {tool} must not be bundled")]
    BundledExternalTool {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An external implementation was marked as required for native behavior.
    #[error("external tool {tool} must not be required for the native product")]
    RequiredForNative {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// The published native package was made to depend on an external implementation.
    #[error("native package must not require external tool {tool}")]
    NativePackageRequiresExternal {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A compatibility tool could be selected automatically.
    #[error("external tool {tool} must not be selected automatically")]
    AutomaticSelection {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An external tool could execute merely because a workspace opened.
    #[error("external tool {tool} must not execute on workspace open")]
    WorkspaceOpenExecution {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A tool was enabled merely by being discovered.
    #[error("external tool {tool} must not be enabled implicitly on discovery")]
    ImplicitEnablement {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A role requiring user authorization was not marked explicit.
    #[error("external tool {tool} requires explicit user enablement")]
    MissingExplicitEnablement {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// External execution support and the explicit-adapter role disagree.
    #[error("external adapter role/support mismatch for {tool}")]
    ExternalExecutionRoleMismatch {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An external execution role lacks the process/trust owner.
    #[error("external adapter {tool} must use the explicit external-process trust class")]
    InvalidExternalExecutionTrust {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An authorized execution class has no environment/process owner.
    #[error("external execution for {tool} requires an environment/process owner")]
    ExternalExecutionWithoutOwner {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// Conformance requirements and the conformance-oracle role disagree.
    #[error("conformance role/requirement mismatch for {tool}")]
    ConformanceRoleMismatch {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A conformance oracle does not pin an exact version or require receipts.
    #[error("conformance oracle {tool} requires pinned version and bounded receipts")]
    ConformanceWithoutEvidenceRequirements {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// External comparison evidence was allowed to promote native readiness.
    #[error("external evidence for {tool} must not promote native readiness")]
    EvidencePromotesNativeReadiness {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A configuration reader exists without the compatibility role, or vice versa.
    #[error("configuration-reader role/support mismatch for {tool}")]
    ConfigReaderRoleMismatch {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An authorized configuration reader has no domain owner.
    #[error("configuration reader for {tool} requires a domain owner")]
    ConfigReaderWithoutOwner {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// Configuration-file presence was allowed to authorize process execution.
    #[error("configuration presence for {tool} must not authorize execution")]
    ConfigPresenceAuthorizesExecution {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An adapter whose output can become an edit has no validation owner.
    #[error("external adapter {tool} requires a candidate-output validation owner")]
    MissingCandidateOutputValidationOwner {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// An optional debugger peer lacks the peer trust owner.
    #[error("external debugger peer {tool} must use the explicit debugger-peer trust class")]
    InvalidPeerTrust {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A tool with a permanently bounded role was exposed as product runtime.
    #[error("external tool {tool} is not authorized as a product runtime")]
    RuntimeForbidden {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A native-only tool was given a shipped product adapter or runtime enablement.
    #[error("external tool {tool} must remain native-only with no product adapter")]
    NativeOnlyToolHasProductAdapter {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A native-only tool was made ordinarily auto-detectable.
    #[error("native-only external tool {tool} must not be auto-detected in ordinary startup")]
    NativeOnlyToolAutoDetected {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A shipped native replacement names no exact identity.
    #[error("native replacement for {tool} must name an exact identity")]
    NativeReplacementWithoutIdentity {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// The native replacement identity is the external tool itself.
    #[error("native replacement for {tool} must not be the external tool")]
    NativeReplacementIsExternalTool {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A native replacement has no delivery owner.
    #[error("native replacement for {tool} requires a delivery owner")]
    NativeReplacementWithoutOwner {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A canonical name or alias is duplicated across registry entries.
    #[error("duplicate external-tool identity: {identity}")]
    DuplicateIdentity {
        /// Conflicting canonical name or alias.
        identity: String,
    },
    /// A canonical name or alias is empty after trimming.
    #[error("external-tool identity must not be empty")]
    EmptyIdentity,
}

/// Tools that may never gain a product runtime, editor, or CLI adapter.
///
/// Perl::LanguageServer is retired as a runtime backend. Perl::Critic is fully
/// native in the product: `.perlcriticrc` is read process-free, and a real
/// `perlcritic` runs only from repository conformance entrypoints.
const NATIVE_ONLY_TOOLS: &[ExternalToolId] =
    &[ExternalToolId::PerlLanguageServer, ExternalToolId::PerlCritic];

/// Validate a registry against native-product and role-consistency invariants.
///
/// # Errors
///
/// Returns the first deterministic policy violation.
pub fn validate_external_tool_registry(
    registry: &[ExternalToolPolicy],
) -> Result<(), ExternalToolRegistryError> {
    let mut identities = BTreeSet::new();

    for policy in registry {
        let tool = policy.canonical_name;

        if policy.bundled {
            return Err(ExternalToolRegistryError::BundledExternalTool { tool });
        }
        if policy.required_for_native {
            return Err(ExternalToolRegistryError::RequiredForNative { tool });
        }
        if policy.native_package_requires_external {
            return Err(ExternalToolRegistryError::NativePackageRequiresExternal { tool });
        }
        if policy.may_auto_select {
            return Err(ExternalToolRegistryError::AutomaticSelection { tool });
        }
        if policy.may_execute_on_workspace_open {
            return Err(ExternalToolRegistryError::WorkspaceOpenExecution { tool });
        }
        if policy.runtime_enablement == RuntimeEnablement::ImplicitOnDiscovery {
            return Err(ExternalToolRegistryError::ImplicitEnablement { tool });
        }
        if policy.config_presence_authorizes_execution {
            return Err(ExternalToolRegistryError::ConfigPresenceAuthorizesExecution { tool });
        }
        if policy.evidence_promotes_native_readiness {
            return Err(ExternalToolRegistryError::EvidencePromotesNativeReadiness { tool });
        }

        let has_external_adapter =
            policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter);
        let has_conformance = policy.roles.contains(&ExternalToolRole::ConformanceOracle);
        let has_config_reader =
            policy.roles.contains(&ExternalToolRole::ConfigurationCompatibility);
        let has_peer = policy.roles.contains(&ExternalToolRole::ExplicitOptionalPeer);
        let is_product_adapter =
            policy.external_execution_support == ExternalExecutionSupport::ExplicitProductAdapter;

        if has_external_adapter != is_product_adapter {
            return Err(ExternalToolRegistryError::ExternalExecutionRoleMismatch { tool });
        }
        if is_product_adapter
            && policy.trust_class != ExternalToolTrustClass::ExplicitExternalProcess
        {
            return Err(ExternalToolRegistryError::InvalidExternalExecutionTrust { tool });
        }
        if policy.external_execution_support != ExternalExecutionSupport::None
            && policy.external_execution_owner.is_none()
        {
            return Err(ExternalToolRegistryError::ExternalExecutionWithoutOwner { tool });
        }
        if is_product_adapter && policy.candidate_output_validation_owner.is_none() {
            return Err(ExternalToolRegistryError::MissingCandidateOutputValidationOwner { tool });
        }

        if has_conformance != policy.conformance.is_some() {
            return Err(ExternalToolRegistryError::ConformanceRoleMismatch { tool });
        }
        if let Some(requirements) = policy.conformance
            && (!requirements.pinned_version_required
                || !requirements.receipt_required
                || requirements.owner.trim().is_empty())
        {
            return Err(ExternalToolRegistryError::ConformanceWithoutEvidenceRequirements { tool });
        }

        if has_config_reader != (policy.config_reader_support != ConfigReaderSupport::None) {
            return Err(ExternalToolRegistryError::ConfigReaderRoleMismatch { tool });
        }
        if has_config_reader && policy.config_reader_owner.is_none() {
            return Err(ExternalToolRegistryError::ConfigReaderWithoutOwner { tool });
        }

        if has_peer && policy.trust_class != ExternalToolTrustClass::ExplicitDebuggerPeer {
            return Err(ExternalToolRegistryError::InvalidPeerTrust { tool });
        }
        if (has_external_adapter || has_peer)
            && policy.runtime_enablement != RuntimeEnablement::ExplicitUserAction
        {
            return Err(ExternalToolRegistryError::MissingExplicitEnablement { tool });
        }

        if policy.tool_id == ExternalToolId::PerlLanguageServer
            && (has_external_adapter || has_peer || is_product_adapter)
        {
            return Err(ExternalToolRegistryError::RuntimeForbidden { tool });
        }
        if NATIVE_ONLY_TOOLS.contains(&policy.tool_id) {
            if has_external_adapter
                || is_product_adapter
                || policy.runtime_enablement != RuntimeEnablement::Forbidden
                || policy.install_help_scope != InstallHelpScope::DeveloperConformance
            {
                return Err(ExternalToolRegistryError::NativeOnlyToolHasProductAdapter { tool });
            }
            if policy.may_auto_detect {
                return Err(ExternalToolRegistryError::NativeOnlyToolAutoDetected { tool });
            }
        }

        validate_native_replacement(policy)?;

        for identity in std::iter::once(policy.canonical_name).chain(policy.aliases.iter().copied())
        {
            let trimmed = identity.trim();
            if trimmed.is_empty() {
                return Err(ExternalToolRegistryError::EmptyIdentity);
            }
            let normalized = trimmed.to_ascii_lowercase();
            if !identities.insert(normalized.clone()) {
                return Err(ExternalToolRegistryError::DuplicateIdentity { identity: normalized });
            }
        }
    }

    Ok(())
}

/// Validate that a native replacement names an exact, non-external identity.
fn validate_native_replacement(
    policy: &ExternalToolPolicy,
) -> Result<(), ExternalToolRegistryError> {
    let tool = policy.canonical_name;
    let replacement = policy.native_replacement;

    if replacement.owner.trim().is_empty() {
        return Err(ExternalToolRegistryError::NativeReplacementWithoutOwner { tool });
    }

    let named_identities = replacement
        .products
        .iter()
        .copied()
        .chain(replacement.package)
        .chain(replacement.library)
        .chain(replacement.executable);

    match replacement.delivery {
        NativeReplacementDelivery::Shipped => {
            if replacement.products.is_empty() {
                return Err(ExternalToolRegistryError::NativeReplacementWithoutIdentity { tool });
            }
        }
        NativeReplacementDelivery::NotApplicable => {
            if !replacement.products.is_empty() {
                return Err(ExternalToolRegistryError::NativeReplacementWithoutIdentity { tool });
            }
        }
        NativeReplacementDelivery::Planned => {}
    }

    for identity in named_identities {
        let normalized = identity.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(ExternalToolRegistryError::NativeReplacementWithoutIdentity { tool });
        }
        let collides = policy.canonical_name.eq_ignore_ascii_case(&normalized)
            || policy.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(&normalized));
        if collides {
            return Err(ExternalToolRegistryError::NativeReplacementIsExternalTool { tool });
        }
    }

    Ok(())
}

/// Find a policy by stable tool ID.
#[must_use]
pub fn external_tool_policy(tool_id: ExternalToolId) -> Option<&'static ExternalToolPolicy> {
    EXTERNAL_TOOL_REGISTRY.iter().find(|policy| policy.tool_id == tool_id)
}

/// Resolve a policy by an exact canonical name or alias, case-insensitively.
///
/// This is exact identity matching. It deliberately does not perform substring
/// matching, so an alias such as `pls` cannot classify `my-pls-wrapper`.
#[must_use]
pub fn external_tool_policy_by_identity(identity: &str) -> Option<&'static ExternalToolPolicy> {
    let normalized = identity.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    EXTERNAL_TOOL_REGISTRY.iter().find(|policy| {
        policy.canonical_name.eq_ignore_ascii_case(&normalized)
            || policy.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(&normalized))
    })
}

/// Serialize the reviewed registry in deterministic declaration order.
///
/// # Errors
///
/// Returns a serialization error only if the static contract ceases to be JSON serializable.
pub fn external_tool_registry_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(EXTERNAL_TOOL_REGISTRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Index of the Perl::Critic entry, used to build mutation fixtures.
    const CRITIC_INDEX: usize = 2;
    /// Index of the App::perlimports entry, used to build mutation fixtures.
    const PERLIMPORTS_INDEX: usize = 3;
    /// Index of the Devel::ptkdb entry, used to build mutation fixtures.
    const PTKDB_INDEX: usize = 4;

    #[test]
    fn reviewed_registry_is_valid() -> Result<(), ExternalToolRegistryError> {
        validate_external_tool_registry(EXTERNAL_TOOL_REGISTRY)
    }

    #[test]
    fn all_external_tools_are_optional_and_unbundled() {
        for policy in EXTERNAL_TOOL_REGISTRY {
            assert!(!policy.bundled, "{} must remain unbundled", policy.canonical_name);
            assert!(
                !policy.required_for_native,
                "{} must not be required for native behavior",
                policy.canonical_name
            );
            assert!(
                !policy.native_package_requires_external,
                "{} must not be a native package dependency",
                policy.canonical_name
            );
            assert!(!policy.may_auto_select, "{} must not auto-select", policy.canonical_name);
            assert!(
                !policy.evidence_promotes_native_readiness,
                "{} evidence must not promote native readiness",
                policy.canonical_name
            );
            assert_ne!(
                policy.runtime_enablement,
                RuntimeEnablement::ImplicitOnDiscovery,
                "{} must never be implicitly enabled",
                policy.canonical_name
            );
        }
    }

    #[test]
    fn registry_covers_every_reviewed_tool() {
        let ids: Vec<ExternalToolId> =
            EXTERNAL_TOOL_REGISTRY.iter().map(|policy| policy.tool_id).collect();
        assert_eq!(
            ids,
            vec![
                ExternalToolId::PerlLanguageServer,
                ExternalToolId::PerlTidy,
                ExternalToolId::PerlCritic,
                ExternalToolId::Perlimports,
                ExternalToolId::Ptkdb,
            ]
        );
    }

    #[test]
    fn exact_identity_resolution_does_not_turn_aliases_into_substring_patterns()
    -> Result<(), Box<dyn std::error::Error>> {
        let pls = external_tool_policy_by_identity("PLS").ok_or("PLS alias should resolve")?;
        assert_eq!(pls.tool_id, ExternalToolId::PerlLanguageServer);
        assert!(external_tool_policy_by_identity("my-pls-wrapper").is_none());
        assert!(external_tool_policy_by_identity("  ").is_none());

        let imports = external_tool_policy_by_identity("perlimports")
            .ok_or("perlimports alias should resolve")?;
        assert_eq!(imports.tool_id, ExternalToolId::Perlimports);
        assert!(external_tool_policy_by_identity("perlimports-wrapper").is_none());
        Ok(())
    }

    #[test]
    fn pls_is_conformance_only() -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::PerlLanguageServer)
            .ok_or("missing Perl::LanguageServer policy")?;
        assert_eq!(policy.roles, &[ExternalToolRole::ConformanceOracle]);
        assert_eq!(
            policy.external_execution_support,
            ExternalExecutionSupport::RepositoryConformanceOnly
        );
        assert_eq!(policy.runtime_enablement, RuntimeEnablement::Forbidden);
        assert_eq!(policy.install_help_scope, InstallHelpScope::DeveloperConformance);
        Ok(())
    }

    #[test]
    fn perl_tidy_names_its_exact_native_replacement_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy =
            external_tool_policy(ExternalToolId::PerlTidy).ok_or("missing Perl::Tidy policy")?;
        let native = policy.native_replacement;
        assert_eq!(native.products, &["perl-tidy"]);
        assert_eq!(native.package, Some("perl-tidy"));
        assert_eq!(native.library, Some("perl_tidy"));
        assert_eq!(native.executable, Some("perl-tidy"));
        assert_eq!(native.lsp_consumer, Some("perllsp"));

        // The ruled identity has not shipped; the registry must say so rather
        // than imply `perl-tidy` is already the crate consumers can depend on.
        assert_eq!(native.delivery, NativeReplacementDelivery::Planned);
        assert_eq!(native.current_implementation, Some("perl-lsp-perltidy"));
        Ok(())
    }

    #[test]
    fn perl_tidy_keeps_its_explicit_external_adapter() -> Result<(), Box<dyn std::error::Error>> {
        let policy =
            external_tool_policy(ExternalToolId::PerlTidy).ok_or("missing Perl::Tidy policy")?;
        assert!(policy.roles.contains(&ExternalToolRole::ConfigurationCompatibility));
        assert!(policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter));
        assert_eq!(
            policy.external_execution_support,
            ExternalExecutionSupport::ExplicitProductAdapter
        );
        assert_eq!(policy.trust_class, ExternalToolTrustClass::ExplicitExternalProcess);
        assert_eq!(policy.runtime_enablement, RuntimeEnablement::ExplicitUserAction);
        assert_eq!(policy.candidate_output_validation_owner, Some("#7056"));
        Ok(())
    }

    #[test]
    fn perl_critic_is_config_compatibility_plus_repository_oracle_only()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::PerlCritic)
            .ok_or("missing Perl::Critic policy")?;
        assert_eq!(
            policy.roles,
            &[ExternalToolRole::ConfigurationCompatibility, ExternalToolRole::ConformanceOracle,]
        );
        assert!(!policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter));
        assert_eq!(
            policy.external_execution_support,
            ExternalExecutionSupport::RepositoryConformanceOnly
        );
        assert_eq!(policy.runtime_enablement, RuntimeEnablement::Forbidden);
        assert!(!policy.may_auto_detect, "ordinary startup must not probe for perlcritic");
        assert_eq!(policy.trust_class, ExternalToolTrustClass::RepositoryConformance);
        assert_eq!(policy.install_help_scope, InstallHelpScope::DeveloperConformance);
        assert!(!policy.config_presence_authorizes_execution);
        assert_eq!(policy.config_reader_support, ConfigReaderSupport::Planned);
        assert_eq!(policy.config_reader_owner, Some("#7211"));
        Ok(())
    }

    #[test]
    fn perlimports_is_an_explicit_saved_file_adapter_bound_to_a_validation_owner()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::Perlimports)
            .ok_or("missing App::perlimports policy")?;
        assert_eq!(
            policy.roles,
            &[ExternalToolRole::ExplicitExternalAdapter, ExternalToolRole::ConformanceOracle]
        );
        assert_eq!(policy.source_requirement, SourceRequirement::SavedFile);
        assert_eq!(policy.runtime_enablement, RuntimeEnablement::ExplicitUserAction);
        assert!(policy.may_auto_detect, "advisory detection is authorized");
        assert!(!policy.may_auto_select);
        assert_eq!(policy.candidate_output_validation_owner, Some("#8277"));
        assert_eq!(policy.trust_class, ExternalToolTrustClass::ExplicitExternalProcess);
        Ok(())
    }

    #[test]
    fn ptkdb_is_an_optional_peer_not_an_external_dap_server()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::Ptkdb).ok_or("missing ptkdb policy")?;
        assert_eq!(policy.roles, &[ExternalToolRole::ExplicitOptionalPeer]);
        assert_eq!(policy.native_host, "perl-dap");
        assert_eq!(policy.native_replacement.delivery, NativeReplacementDelivery::NotApplicable);
        assert!(policy.claim_boundary.contains("perl-dap remains the DAP server"));
        Ok(())
    }

    #[test]
    fn registry_json_is_deterministic_and_machine_readable()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = external_tool_registry_json()?;
        let second = external_tool_registry_json()?;
        assert_eq!(first, second);
        let parsed: serde_json::Value = serde_json::from_str(&first)?;
        let entries = parsed.as_array().ok_or("registry JSON must be an array")?;
        assert_eq!(entries.len(), EXTERNAL_TOOL_REGISTRY.len());
        assert_eq!(entries[0]["toolId"], "perl_language_server");
        assert_eq!(entries[1]["toolId"], "perl_tidy");
        assert_eq!(entries[2]["toolId"], "perl_critic");
        assert_eq!(entries[3]["toolId"], "perlimports");
        assert_eq!(entries[4]["toolId"], "ptkdb");

        // Exact native identity must survive serialization for docs/doctor consumers.
        assert_eq!(entries[1]["nativeReplacement"]["package"], "perl-tidy");
        assert_eq!(entries[1]["nativeReplacement"]["library"], "perl_tidy");
        assert_eq!(entries[2]["externalExecutionSupport"], "repository_conformance_only");
        Ok(())
    }

    #[test]
    fn validator_rejects_forbidden_policy_combinations() {
        let bundled = [ExternalToolPolicy { bundled: true, ..EXTERNAL_TOOL_REGISTRY[0] }];
        assert!(matches!(
            validate_external_tool_registry(&bundled),
            Err(ExternalToolRegistryError::BundledExternalTool { .. })
        ));

        let auto_selected =
            [ExternalToolPolicy { may_auto_select: true, ..EXTERNAL_TOOL_REGISTRY[1] }];
        assert!(matches!(
            validate_external_tool_registry(&auto_selected),
            Err(ExternalToolRegistryError::AutomaticSelection { .. })
        ));

        let native_dependency = [ExternalToolPolicy {
            native_package_requires_external: true,
            ..EXTERNAL_TOOL_REGISTRY[1]
        }];
        assert!(matches!(
            validate_external_tool_registry(&native_dependency),
            Err(ExternalToolRegistryError::NativePackageRequiresExternal { .. })
        ));

        let missing_execution_role = [ExternalToolPolicy {
            external_execution_support: ExternalExecutionSupport::RepositoryConformanceOnly,
            ..EXTERNAL_TOOL_REGISTRY[1]
        }];
        assert!(matches!(
            validate_external_tool_registry(&missing_execution_role),
            Err(ExternalToolRegistryError::ExternalExecutionRoleMismatch { .. })
        ));

        let unowned_execution =
            [ExternalToolPolicy { external_execution_owner: None, ..EXTERNAL_TOOL_REGISTRY[1] }];
        assert!(matches!(
            validate_external_tool_registry(&unowned_execution),
            Err(ExternalToolRegistryError::ExternalExecutionWithoutOwner { .. })
        ));

        let implicit_peer = [ExternalToolPolicy {
            runtime_enablement: RuntimeEnablement::Forbidden,
            ..EXTERNAL_TOOL_REGISTRY[PTKDB_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&implicit_peer),
            Err(ExternalToolRegistryError::MissingExplicitEnablement { .. })
        ));

        let discovered_peer = [ExternalToolPolicy {
            runtime_enablement: RuntimeEnablement::ImplicitOnDiscovery,
            ..EXTERNAL_TOOL_REGISTRY[PTKDB_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&discovered_peer),
            Err(ExternalToolRegistryError::ImplicitEnablement { .. })
        ));
    }

    #[test]
    fn validator_rejects_perl_critic_becoming_a_product_engine() {
        // A runtime/editor adapter is the exact regression #7209 exists to prevent.
        let runtime_adapter = [ExternalToolPolicy {
            roles: PERLTIDY_ROLES,
            external_execution_support: ExternalExecutionSupport::ExplicitProductAdapter,
            trust_class: ExternalToolTrustClass::ExplicitExternalProcess,
            runtime_enablement: RuntimeEnablement::ExplicitUserAction,
            candidate_output_validation_owner: Some("#0000"),
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&runtime_adapter),
            Err(ExternalToolRegistryError::NativeOnlyToolHasProductAdapter { .. })
        ));

        let runtime_enabled = [ExternalToolPolicy {
            runtime_enablement: RuntimeEnablement::ExplicitUserAction,
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&runtime_enabled),
            Err(ExternalToolRegistryError::NativeOnlyToolHasProductAdapter { .. })
        ));

        let user_install_help = [ExternalToolPolicy {
            install_help_scope: InstallHelpScope::UserRequestedCompatibility,
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&user_install_help),
            Err(ExternalToolRegistryError::NativeOnlyToolHasProductAdapter { .. })
        ));

        let probed_on_startup =
            [ExternalToolPolicy { may_auto_detect: true, ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX] }];
        assert!(matches!(
            validate_external_tool_registry(&probed_on_startup),
            Err(ExternalToolRegistryError::NativeOnlyToolAutoDetected { .. })
        ));
    }

    #[test]
    fn validator_rejects_config_presence_authorizing_execution() {
        let critic_config_executes = [ExternalToolPolicy {
            config_presence_authorizes_execution: true,
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&critic_config_executes),
            Err(ExternalToolRegistryError::ConfigPresenceAuthorizesExecution { .. })
        ));
    }

    #[test]
    fn validator_rejects_oracle_evidence_promoting_native_readiness() {
        let promoting = [ExternalToolPolicy {
            evidence_promotes_native_readiness: true,
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&promoting),
            Err(ExternalToolRegistryError::EvidencePromotesNativeReadiness { .. })
        ));
    }

    #[test]
    fn validator_rejects_conformance_without_pinned_evidence() {
        let unpinned = [ExternalToolPolicy {
            conformance: Some(ConformanceRequirements {
                pinned_version_required: false,
                receipt_required: true,
                owner: "#7210",
            }),
            ..EXTERNAL_TOOL_REGISTRY[0]
        }];
        assert!(matches!(
            validate_external_tool_registry(&unpinned),
            Err(ExternalToolRegistryError::ConformanceWithoutEvidenceRequirements { .. })
        ));

        let receiptless = [ExternalToolPolicy {
            conformance: Some(ConformanceRequirements {
                pinned_version_required: true,
                receipt_required: false,
                owner: "#7210",
            }),
            ..EXTERNAL_TOOL_REGISTRY[0]
        }];
        assert!(matches!(
            validate_external_tool_registry(&receiptless),
            Err(ExternalToolRegistryError::ConformanceWithoutEvidenceRequirements { .. })
        ));

        let unowned = [ExternalToolPolicy { conformance: None, ..EXTERNAL_TOOL_REGISTRY[0] }];
        assert!(matches!(
            validate_external_tool_registry(&unowned),
            Err(ExternalToolRegistryError::ConformanceRoleMismatch { .. })
        ));
    }

    #[test]
    fn validator_rejects_adapter_output_becoming_edit_authority() {
        let unvalidated = [ExternalToolPolicy {
            candidate_output_validation_owner: None,
            ..EXTERNAL_TOOL_REGISTRY[PERLIMPORTS_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&unvalidated),
            Err(ExternalToolRegistryError::MissingCandidateOutputValidationOwner { .. })
        ));
    }

    #[test]
    fn validator_rejects_naming_the_external_tool_as_its_own_native_replacement() {
        // "Perl::Tidy is the canonical native formatter" must not be expressible.
        let self_replacing = [ExternalToolPolicy {
            native_replacement: NativeReplacement {
                products: PERLTIDY_ALIASES,
                package: None,
                library: None,
                executable: Some("perltidy"),
                lsp_consumer: Some("perllsp"),
                delivery: NativeReplacementDelivery::Shipped,
                current_implementation: None,
                owner: "#7411",
            },
            ..EXTERNAL_TOOL_REGISTRY[1]
        }];
        assert!(matches!(
            validate_external_tool_registry(&self_replacing),
            Err(ExternalToolRegistryError::NativeReplacementIsExternalTool { .. })
        ));
    }

    #[test]
    fn validator_rejects_shipped_native_replacement_without_identity() {
        let shipped_without_identity = [ExternalToolPolicy {
            native_replacement: NativeReplacement {
                delivery: NativeReplacementDelivery::Shipped,
                ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX].native_replacement
            },
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&shipped_without_identity),
            Err(ExternalToolRegistryError::NativeReplacementWithoutIdentity { .. })
        ));

        let unowned = [ExternalToolPolicy {
            native_replacement: NativeReplacement {
                owner: "  ",
                ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX].native_replacement
            },
            ..EXTERNAL_TOOL_REGISTRY[CRITIC_INDEX]
        }];
        assert!(matches!(
            validate_external_tool_registry(&unowned),
            Err(ExternalToolRegistryError::NativeReplacementWithoutOwner { .. })
        ));
    }

    #[test]
    fn validator_rejects_duplicate_identities() {
        let duplicate_alias = [
            EXTERNAL_TOOL_REGISTRY[0],
            ExternalToolPolicy { aliases: &["Perl-LanguageServer"], ..EXTERNAL_TOOL_REGISTRY[1] },
        ];
        assert!(matches!(
            validate_external_tool_registry(&duplicate_alias),
            Err(ExternalToolRegistryError::DuplicateIdentity { .. })
        ));
    }
}
