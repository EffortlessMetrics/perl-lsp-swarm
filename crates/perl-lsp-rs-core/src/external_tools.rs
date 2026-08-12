//! Canonical roles and native replacements for external Perl tooling.
//!
//! This registry answers product-policy questions only. It does not discover,
//! install, execute, or parse tool-specific configuration. Domain registries
//! remain authoritative for Perl::Tidy options, Perl::Critic policies, and
//! debugger-peer capabilities.

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
    /// `Devel::ptkdb`, an explicit optional debugger peer.
    Ptkdb,
}

/// A bounded role an external tool may hold around the native product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolRole {
    /// Read familiar configuration and explain its native mapping.
    ConfigurationCompatibility,
    /// Execute an explicitly selected external implementation.
    ExplicitExternalAdapter,
    /// Observe external behavior in repository-only comparison infrastructure.
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
    /// Native product surface replacing or hosting the external implementation.
    pub native_product_surface: &'static str,
    /// Authorized roles for this tool.
    pub roles: &'static [ExternalToolRole],
    /// Whether any external implementation payload is bundled in product artifacts.
    pub bundled: bool,
    /// Whether the native product requires this external implementation.
    pub required_for_native: bool,
    /// Whether ordinary startup may record an advisory candidate without executing it.
    pub may_auto_detect: bool,
    /// Whether discovery or PATH presence may select this tool automatically.
    pub may_auto_select: bool,
    /// Whether ordinary workspace opening may execute the tool.
    pub may_execute_on_workspace_open: bool,
    /// Whether a user action or configuration decision is required before use.
    pub explicit_enablement_required: bool,
    /// Familiar configuration files associated with the tool.
    pub config_files: &'static [&'static str],
    /// Current native configuration-reader posture.
    pub config_reader_support: ConfigReaderSupport,
    /// Whether an explicit external execution adapter is authorized.
    pub external_execution_support: bool,
    /// Whether repository-only conformance comparison is authorized.
    pub conformance_support: bool,
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
const PERLCRITIC_ROLES: &[ExternalToolRole] = &[
    ExternalToolRole::ConfigurationCompatibility,
    ExternalToolRole::ExplicitExternalAdapter,
    ExternalToolRole::ConformanceOracle,
];
const PTKDB_ROLES: &[ExternalToolRole] = &[ExternalToolRole::ExplicitOptionalPeer];

const PLS_ALIASES: &[&str] = &["Perl-LanguageServer", "pls"];
const PERLTIDY_ALIASES: &[&str] = &["perltidy"];
const PERLCRITIC_ALIASES: &[&str] = &["perlcritic"];
const PTKDB_ALIASES: &[&str] = &["ptkdb"];
const PERLTIDY_CONFIG: &[&str] = &[".perltidyrc"];
const PERLCRITIC_CONFIG: &[&str] = &[".perlcriticrc"];
const NO_CONFIG_FILES: &[&str] = &[];

/// Reviewed external-tool policy registry.
pub const EXTERNAL_TOOL_REGISTRY: &[ExternalToolPolicy] = &[
    ExternalToolPolicy {
        tool_id: ExternalToolId::PerlLanguageServer,
        canonical_name: "Perl::LanguageServer",
        aliases: PLS_ALIASES,
        owned_domain: "language intelligence and historical DAP comparison",
        native_product_surface: "perllsp + perl-dap",
        roles: PLS_ROLES,
        bundled: false,
        required_for_native: false,
        may_auto_detect: false,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        explicit_enablement_required: true,
        config_files: NO_CONFIG_FILES,
        config_reader_support: ConfigReaderSupport::None,
        external_execution_support: false,
        conformance_support: true,
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
        native_product_surface: "native formatter",
        roles: PERLTIDY_ROLES,
        bundled: false,
        required_for_native: false,
        may_auto_detect: true,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        explicit_enablement_required: true,
        config_files: PERLTIDY_CONFIG,
        config_reader_support: ConfigReaderSupport::Partial,
        external_execution_support: true,
        conformance_support: true,
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
        native_product_surface: "native critic",
        roles: PERLCRITIC_ROLES,
        bundled: false,
        required_for_native: false,
        may_auto_detect: true,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        explicit_enablement_required: true,
        config_files: PERLCRITIC_CONFIG,
        config_reader_support: ConfigReaderSupport::Planned,
        external_execution_support: true,
        conformance_support: true,
        install_help_scope: InstallHelpScope::UserRequestedCompatibility,
        trust_class: ExternalToolTrustClass::ExplicitExternalProcess,
        claim_boundary: "native critic is default; external execution is explicit compatibility only",
        status_owner: "#6997 / #6987 / #7211",
    },
    ExternalToolPolicy {
        tool_id: ExternalToolId::Ptkdb,
        canonical_name: "Devel::ptkdb",
        aliases: PTKDB_ALIASES,
        owned_domain: "debugger engine and GUI peer",
        native_product_surface: "perl-dap",
        roles: PTKDB_ROLES,
        bundled: false,
        required_for_native: false,
        may_auto_detect: false,
        may_auto_select: false,
        may_execute_on_workspace_open: false,
        explicit_enablement_required: true,
        config_files: NO_CONFIG_FILES,
        config_reader_support: ConfigReaderSupport::None,
        external_execution_support: false,
        conformance_support: false,
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
    /// A role requiring user authorization was not marked explicit.
    #[error("external tool {tool} requires explicit enablement")]
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
    /// Conformance support and the conformance-oracle role disagree.
    #[error("conformance role/support mismatch for {tool}")]
    ConformanceRoleMismatch {
        /// Canonical tool name.
        tool: &'static str,
    },
    /// A configuration reader exists without the compatibility role, or vice versa.
    #[error("configuration-reader role/support mismatch for {tool}")]
    ConfigReaderRoleMismatch {
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
        if policy.bundled {
            return Err(ExternalToolRegistryError::BundledExternalTool {
                tool: policy.canonical_name,
            });
        }
        if policy.required_for_native {
            return Err(ExternalToolRegistryError::RequiredForNative {
                tool: policy.canonical_name,
            });
        }
        if policy.may_auto_select {
            return Err(ExternalToolRegistryError::AutomaticSelection {
                tool: policy.canonical_name,
            });
        }
        if policy.may_execute_on_workspace_open {
            return Err(ExternalToolRegistryError::WorkspaceOpenExecution {
                tool: policy.canonical_name,
            });
        }

        let has_external_adapter =
            policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter);
        let has_conformance = policy.roles.contains(&ExternalToolRole::ConformanceOracle);
        let has_config_reader =
            policy.roles.contains(&ExternalToolRole::ConfigurationCompatibility);
        let has_peer = policy.roles.contains(&ExternalToolRole::ExplicitOptionalPeer);

        if has_external_adapter != policy.external_execution_support {
            return Err(ExternalToolRegistryError::ExternalExecutionRoleMismatch {
                tool: policy.canonical_name,
            });
        }
        if has_external_adapter
            && policy.trust_class != ExternalToolTrustClass::ExplicitExternalProcess
        {
            return Err(ExternalToolRegistryError::InvalidExternalExecutionTrust {
                tool: policy.canonical_name,
            });
        }
        if has_conformance != policy.conformance_support {
            return Err(ExternalToolRegistryError::ConformanceRoleMismatch {
                tool: policy.canonical_name,
            });
        }
        if has_config_reader != (policy.config_reader_support != ConfigReaderSupport::None) {
            return Err(ExternalToolRegistryError::ConfigReaderRoleMismatch {
                tool: policy.canonical_name,
            });
        }
        if has_peer && policy.trust_class != ExternalToolTrustClass::ExplicitDebuggerPeer {
            return Err(ExternalToolRegistryError::InvalidPeerTrust {
                tool: policy.canonical_name,
            });
        }
        if (has_external_adapter || has_peer) && !policy.explicit_enablement_required {
            return Err(ExternalToolRegistryError::MissingExplicitEnablement {
                tool: policy.canonical_name,
            });
        }
        if policy.tool_id == ExternalToolId::PerlLanguageServer
            && (has_external_adapter || has_peer || policy.external_execution_support)
        {
            return Err(ExternalToolRegistryError::RuntimeForbidden {
                tool: policy.canonical_name,
            });
        }

        for identity in std::iter::once(policy.canonical_name).chain(policy.aliases.iter().copied()) {
            let trimmed = identity.trim();
            if trimmed.is_empty() {
                return Err(ExternalToolRegistryError::EmptyIdentity);
            }
            let normalized = trimmed.to_ascii_lowercase();
            if !identities.insert(normalized.clone()) {
                return Err(ExternalToolRegistryError::DuplicateIdentity {
                    identity: normalized,
                });
            }
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
            assert!(!policy.may_auto_select, "{} must not auto-select", policy.canonical_name);
            assert!(policy.explicit_enablement_required);
        }
    }

    #[test]
    fn exact_identity_resolution_does_not_turn_aliases_into_substring_patterns()
    -> Result<(), Box<dyn std::error::Error>> {
        let pls = external_tool_policy_by_identity("PLS").ok_or("PLS alias should resolve")?;
        assert_eq!(pls.tool_id, ExternalToolId::PerlLanguageServer);
        assert!(external_tool_policy_by_identity("my-pls-wrapper").is_none());
        assert!(external_tool_policy_by_identity("  ").is_none());
        Ok(())
    }

    #[test]
    fn pls_is_conformance_only() -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::PerlLanguageServer)
            .ok_or("missing Perl::LanguageServer policy")?;
        assert_eq!(policy.roles, &[ExternalToolRole::ConformanceOracle]);
        assert!(!policy.external_execution_support);
        assert_eq!(policy.install_help_scope, InstallHelpScope::DeveloperConformance);
        Ok(())
    }

    #[test]
    fn formatter_and_critic_are_explicit_compatibility_adapters()
    -> Result<(), Box<dyn std::error::Error>> {
        for id in [ExternalToolId::PerlTidy, ExternalToolId::PerlCritic] {
            let policy = external_tool_policy(id).ok_or("missing compatibility policy")?;
            assert!(policy.roles.contains(&ExternalToolRole::ConfigurationCompatibility));
            assert!(policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter));
            assert!(policy.roles.contains(&ExternalToolRole::ConformanceOracle));
            assert!(policy.external_execution_support);
            assert_eq!(policy.trust_class, ExternalToolTrustClass::ExplicitExternalProcess);
        }
        Ok(())
    }

    #[test]
    fn ptkdb_is_an_optional_peer_not_an_external_dap_server()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = external_tool_policy(ExternalToolId::Ptkdb).ok_or("missing ptkdb policy")?;
        assert_eq!(policy.roles, &[ExternalToolRole::ExplicitOptionalPeer]);
        assert_eq!(policy.native_product_surface, "perl-dap");
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
        assert_eq!(entries[3]["toolId"], "ptkdb");
        Ok(())
    }

    #[test]
    fn validator_rejects_forbidden_policy_combinations() {
        let bundled = [ExternalToolPolicy {
            bundled: true,
            ..EXTERNAL_TOOL_REGISTRY[0]
        }];
        assert!(matches!(
            validate_external_tool_registry(&bundled),
            Err(ExternalToolRegistryError::BundledExternalTool { .. })
        ));

        let auto_selected = [ExternalToolPolicy {
            may_auto_select: true,
            ..EXTERNAL_TOOL_REGISTRY[1]
        }];
        assert!(matches!(
            validate_external_tool_registry(&auto_selected),
            Err(ExternalToolRegistryError::AutomaticSelection { .. })
        ));

        let runtime_pls = [ExternalToolPolicy {
            roles: PERLTIDY_ROLES,
            external_execution_support: true,
            trust_class: ExternalToolTrustClass::ExplicitExternalProcess,
            ..EXTERNAL_TOOL_REGISTRY[0]
        }];
        assert!(matches!(
            validate_external_tool_registry(&runtime_pls),
            Err(ExternalToolRegistryError::ConfigReaderRoleMismatch { .. })
                | Err(ExternalToolRegistryError::RuntimeForbidden { .. })
        ));

        let missing_execution_role = [ExternalToolPolicy {
            external_execution_support: false,
            ..EXTERNAL_TOOL_REGISTRY[1]
        }];
        assert!(matches!(
            validate_external_tool_registry(&missing_execution_role),
            Err(ExternalToolRegistryError::ExternalExecutionRoleMismatch { .. })
        ));

        let implicit_peer = [ExternalToolPolicy {
            explicit_enablement_required: false,
            ..EXTERNAL_TOOL_REGISTRY[3]
        }];
        assert!(matches!(
            validate_external_tool_registry(&implicit_peer),
            Err(ExternalToolRegistryError::MissingExplicitEnablement { .. })
        ));
    }

    #[test]
    fn validator_rejects_duplicate_identities() {
        let duplicate_alias = [
            EXTERNAL_TOOL_REGISTRY[0],
            ExternalToolPolicy {
                aliases: &["Perl-LanguageServer"],
                ..EXTERNAL_TOOL_REGISTRY[1]
            },
        ];
        assert!(matches!(
            validate_external_tool_registry(&duplicate_alias),
            Err(ExternalToolRegistryError::DuplicateIdentity { .. })
        ));
    }
}
