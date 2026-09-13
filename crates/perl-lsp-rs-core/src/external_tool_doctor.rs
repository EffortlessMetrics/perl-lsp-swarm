//! Registry-driven, native-first external-tool doctor projection.
//!
//! This module consumes the canonical external-tool role registry (#7209) and
//! projects one typed status entry per registered tool for the doctor surface
//! (#7212). It is policy projection only: it never discovers, probes,
//! installs, selects, or executes an external tool, and it reads no
//! environment or filesystem state, so its output cannot leak private paths
//! or raw environment values.
//!
//! Verdicts derive entirely from the registry rows passed in. The CLI and
//! editor projections share the same serialized status/reason codes, so no
//! presentation layer re-encodes tool policy.

use crate::external_tools::{
    ExternalExecutionSupport, ExternalToolPolicy, ExternalToolRole, InstallHelpScope,
    NativeReplacementDelivery, RuntimeEnablement,
};
use serde::Serialize;

/// Native health is unaffected by this tool; it is optional external tooling.
pub const STATUS_OPTIONAL_EXTERNAL: &str = "optional_external";
/// The tool is a repository/conformance oracle with no product runtime.
pub const STATUS_CONFORMANCE_ONLY: &str = "conformance_only";
/// The tool is an explicitly selected optional peer.
pub const STATUS_OPTIONAL_PEER: &str = "optional_peer";
/// The registry row did not match a known role shape.
pub const STATUS_UNKNOWN: &str = "unknown";

/// The native replacement for this domain ships today.
pub const NATIVE_STATUS_AVAILABLE: &str = "native_available";
/// The native replacement is a reviewed ruling that has not shipped yet.
pub const NATIVE_STATUS_PLANNED: &str = "native_planned";
/// The tool is a peer/oracle with no native replacement claim.
pub const NATIVE_STATUS_NOT_APPLICABLE: &str = "native_not_applicable";

/// Registry row forbids any product runtime/editor enablement.
pub const REASON_RUNTIME_ENABLEMENT_FORBIDDEN: &str = "runtime_enablement_forbidden";
/// An explicit user action is required before an adapter may run.
pub const REASON_EXPLICIT_ADAPTER_ONLY: &str = "explicit_adapter_only";
/// The tool cooperates only as an explicitly selected peer.
pub const REASON_EXPLICIT_OPTIONAL_PEER: &str = "explicit_optional_peer";
/// The registry row did not match a known role shape.
pub const REASON_UNCLASSIFIED: &str = "unclassified";

/// Typed doctor projection for one registry row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalToolDoctorEntry {
    /// Canonical registry identity.
    pub canonical_name: &'static str,
    /// Native product surface owning this domain.
    pub native_host: &'static str,
    /// Exact native products replacing the external implementation.
    pub native_products: &'static [&'static str],
    /// Exact native executable replacing the external implementation, if ruled.
    pub native_replacement_executable: Option<&'static str>,
    /// Delivery posture of the native replacement.
    pub native_delivery: NativeReplacementDelivery,
    /// Typed native readiness verdict (`native_available` and friends).
    pub native_status_code: &'static str,
    /// Typed role verdict (`optional_external` and friends).
    pub status_code: &'static str,
    /// Typed reason for the role verdict.
    pub reason_code: &'static str,
    /// Roles the registry authorizes, in declaration order.
    pub allowed_roles: Vec<&'static str>,
    /// Highest authorized external execution class.
    pub execution_support: &'static str,
    /// Whether any product runtime or editor surface may select this tool.
    pub runtime_enablement: &'static str,
    /// Scope in which install guidance may be offered.
    pub install_help_scope: &'static str,
    /// Familiar configuration files associated with the tool.
    pub config_files: &'static [&'static str],
    /// Native configuration-reader posture (`none`/`planned`/`partial`/`supported`).
    pub config_reader_support: &'static str,
    /// Whether the native product requires this external implementation.
    pub required_for_native: bool,
    /// Whether an absence verdict could ever degrade native health.
    pub degrades_native_health: bool,
    /// Concise registry claim boundary.
    pub claim_boundary: &'static str,
    /// One safe next action, derived from the registry.
    pub safe_next_action: &'static str,
    /// Issue or controller owning current status.
    pub status_owner: &'static str,
}

/// Project every registry row into a typed doctor entry, in declaration order.
///
/// The registry is accepted as a parameter so doctor output is provably
/// registry-driven: a row added or changed in a test fixture changes the
/// projection.
#[must_use]
pub fn external_tool_doctor_entries(
    registry: &[ExternalToolPolicy],
) -> Vec<ExternalToolDoctorEntry> {
    registry.iter().map(external_tool_doctor_entry).collect()
}

/// Project the Perl::Critic compatibility entry from the registry.
///
/// Critic compatibility is identified by its recognized configuration file
/// (`.perlcriticrc`), not by hard-coded policy counts, so the projection
/// follows the registry row that owns critic configuration compatibility.
/// The registry does not enforce unique config-file ownership, so this fails
/// closed: exactly one owning row projects, zero or duplicate owners yield
/// `None` and the caller reports the ambiguity honestly instead of guessing
/// by declaration order.
#[must_use]
pub fn critic_compatibility_entry(
    registry: &[ExternalToolPolicy],
) -> Option<ExternalToolDoctorEntry> {
    let mut matches =
        registry.iter().filter(|policy| policy.config_files.contains(&".perlcriticrc"));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(external_tool_doctor_entry(first))
}

/// Project one registry row into a typed doctor entry.
fn external_tool_doctor_entry(policy: &ExternalToolPolicy) -> ExternalToolDoctorEntry {
    let has_peer = policy.roles.contains(&ExternalToolRole::ExplicitOptionalPeer);
    let has_adapter = policy.roles.contains(&ExternalToolRole::ExplicitExternalAdapter);

    let (status_code, reason_code) = if policy.runtime_enablement == RuntimeEnablement::Forbidden {
        (STATUS_CONFORMANCE_ONLY, REASON_RUNTIME_ENABLEMENT_FORBIDDEN)
    } else if has_peer {
        (STATUS_OPTIONAL_PEER, REASON_EXPLICIT_OPTIONAL_PEER)
    } else if has_adapter {
        (STATUS_OPTIONAL_EXTERNAL, REASON_EXPLICIT_ADAPTER_ONLY)
    } else {
        (STATUS_UNKNOWN, REASON_UNCLASSIFIED)
    };

    let native_status_code = match policy.native_replacement.delivery {
        NativeReplacementDelivery::Shipped => NATIVE_STATUS_AVAILABLE,
        NativeReplacementDelivery::Planned => NATIVE_STATUS_PLANNED,
        NativeReplacementDelivery::NotApplicable => NATIVE_STATUS_NOT_APPLICABLE,
    };

    ExternalToolDoctorEntry {
        canonical_name: policy.canonical_name,
        native_host: policy.native_host,
        native_products: policy.native_replacement.products,
        native_replacement_executable: policy.native_replacement.executable,
        native_delivery: policy.native_replacement.delivery,
        native_status_code,
        status_code,
        reason_code,
        allowed_roles: policy.roles.iter().copied().map(role_label).collect(),
        execution_support: execution_support_label(policy.external_execution_support),
        runtime_enablement: runtime_enablement_label(policy.runtime_enablement),
        install_help_scope: install_help_scope_label(policy.install_help_scope),
        config_files: policy.config_files,
        config_reader_support: config_reader_support_label(policy),
        required_for_native: policy.required_for_native,
        degrades_native_health: policy.required_for_native
            || policy.evidence_promotes_native_readiness,
        claim_boundary: policy.claim_boundary,
        safe_next_action: safe_next_action(policy, status_code),
        status_owner: policy.status_owner,
    }
}

/// One safe next action per registry posture; never an executable or install
/// command, and never a runtime engine switch.
fn safe_next_action(policy: &ExternalToolPolicy, status_code: &'static str) -> &'static str {
    if status_code == STATUS_CONFORMANCE_ONLY {
        return "No product setup exists; familiar configuration is explained process-free and \
            real-tool execution stays in repository conformance.";
    }
    if status_code == STATUS_OPTIONAL_PEER {
        return "Optionally configure an explicit peer session; the native product keeps \
            protocol ownership.";
    }
    if policy.install_help_scope == InstallHelpScope::UserRequestedCompatibility {
        return "Optionally request explicit compatibility setup; guidance is copyable, \
            environment-scoped, and never auto-executed.";
    }
    "No install guidance is offered for this tool."
}

fn role_label(role: ExternalToolRole) -> &'static str {
    match role {
        ExternalToolRole::ConfigurationCompatibility => "configuration_compatibility",
        ExternalToolRole::ExplicitExternalAdapter => "explicit_external_adapter",
        ExternalToolRole::ConformanceOracle => "conformance_oracle",
        ExternalToolRole::ExplicitOptionalPeer => "explicit_optional_peer",
    }
}

fn execution_support_label(support: ExternalExecutionSupport) -> &'static str {
    match support {
        ExternalExecutionSupport::None => "none",
        ExternalExecutionSupport::RepositoryConformanceOnly => "repository_conformance_only",
        ExternalExecutionSupport::ExplicitProductAdapter => "explicit_product_adapter",
    }
}

fn runtime_enablement_label(enablement: RuntimeEnablement) -> &'static str {
    match enablement {
        RuntimeEnablement::Forbidden => "forbidden",
        RuntimeEnablement::ExplicitUserAction => "explicit_user_action",
        RuntimeEnablement::ImplicitOnDiscovery => "implicit_on_discovery",
    }
}

fn install_help_scope_label(scope: InstallHelpScope) -> &'static str {
    match scope {
        InstallHelpScope::None => "none",
        InstallHelpScope::UserRequestedCompatibility => "user_requested_compatibility",
        InstallHelpScope::DeveloperConformance => "developer_conformance",
    }
}

fn config_reader_support_label(policy: &ExternalToolPolicy) -> &'static str {
    match policy.config_reader_support {
        crate::external_tools::ConfigReaderSupport::None => "none",
        crate::external_tools::ConfigReaderSupport::Planned => "planned",
        crate::external_tools::ConfigReaderSupport::Partial => "partial",
        crate::external_tools::ConfigReaderSupport::Supported => "supported",
    }
}

/// Render the native-first external-tooling report.
///
/// Every line derives from the projected entries; the renderer holds no tool
/// policy of its own and hard-codes no role or policy counts.
#[must_use]
pub fn render_external_tool_doctor_text(entries: &[ExternalToolDoctorEntry]) -> String {
    let mut out = String::new();
    out.push_str("perl-lsp doctor — external tooling\n");
    out.push_str("==================================\n\n");
    let degrading: Vec<_> = entries
        .iter()
        .filter(|entry| entry.degrades_native_health)
        .map(|entry| entry.canonical_name)
        .collect();
    if degrading.is_empty() {
        out.push_str("Native health is independent of every tool below; absence of any\n");
        out.push_str("external tool never degrades native readiness.\n\n");
    } else {
        out.push_str(&format!(
            "WARNING: registry rows claim native health dependence for {}; this\n",
            degrading.join(", ")
        ));
        out.push_str(
            "contradicts the reviewed registry contract — report it, do not rely on it.\n\n",
        );
    }

    for entry in entries {
        out.push_str(&format!("{}: {}\n", entry.canonical_name, role_summary(entry)));
        out.push_str(&format!("  Native host: {}\n", entry.native_host));
        out.push_str(&format!("  {}\n", native_summary(entry)));
        out.push_str(&format!("  Allowed roles: {}\n", entry.allowed_roles.join(", ")));
        if !entry.config_files.is_empty() {
            out.push_str(&format!(
                "  Recognized config files: {} (reader posture: {})\n",
                entry.config_files.join(", "),
                entry.config_reader_support
            ));
        }
        out.push_str(&format!("  Execution: {}\n", entry.execution_support));
        out.push_str(&format!("  Runtime enablement: {}\n", entry.runtime_enablement));
        out.push_str(&format!(
            "  Selection: none occurred; discovery is advisory and cannot select (status: {}, \
             reason: {})\n",
            entry.status_code, entry.reason_code
        ));
        out.push_str(&format!("  Next action: {}\n", entry.safe_next_action));
        out.push_str(&format!("  {}\n", entry.claim_boundary));
        out.push('\n');
    }

    out.push_str("Claim boundary:\n");
    out.push_str(
        "  Source-only projection of the reviewed registry. This report does not probe,\n",
    );
    out.push_str("  install, enable, select, or execute any external tool.\n");
    out
}

/// Render the critic configuration compatibility explanation.
#[must_use]
pub fn render_critic_compatibility_text(entry: &ExternalToolDoctorEntry) -> String {
    let mut out = String::new();
    out.push_str("perl-lsp doctor — critic configuration compatibility\n");
    out.push_str("===================================================\n\n");
    out.push_str(&format!(
        "{}: configuration compatibility + repository conformance only; not a runtime engine\n\n",
        entry.canonical_name
    ));
    out.push_str(&format!(
        "  {}: read process-free for its native mapping; reader posture: {} (owner: {})\n",
        entry.config_files.join(", "),
        entry.config_reader_support,
        entry.status_owner
    ));
    out.push_str("  No Perl::Critic installation is required for this explanation.\n");
    out.push_str("  No runtime engine switch exists: there is no `Configure External\n");
    out.push_str("  Perl::Critic` action and none may be added.\n");
    out.push_str("  Real perlcritic execution is repository conformance only.\n");
    out.push_str(&format!("  {}\n", entry.claim_boundary));
    out.push_str("  Native critic readiness is never affected by perlcritic's absence.\n");
    out
}

fn role_summary(entry: &ExternalToolDoctorEntry) -> String {
    match entry.status_code {
        STATUS_OPTIONAL_EXTERNAL => {
            "optional external compatibility adapter/conformance tool".to_string()
        }
        STATUS_CONFORMANCE_ONLY => {
            "configuration compatibility + repository conformance only; not a runtime engine"
                .to_string()
        }
        STATUS_OPTIONAL_PEER => "optional debugger peer".to_string(),
        _ => "registry row did not match a known role shape".to_string(),
    }
}

fn native_summary(entry: &ExternalToolDoctorEntry) -> String {
    match entry.native_delivery {
        NativeReplacementDelivery::Shipped => format!(
            "Native replacement: {} — shipped/available (status: {})",
            non_empty_list(entry.native_products),
            entry.native_status_code
        ),
        NativeReplacementDelivery::Planned => format!(
            "Native replacement: {} — planned ruling, not shipped yet (status: {})",
            non_empty_list(entry.native_products),
            entry.native_status_code
        ),
        NativeReplacementDelivery::NotApplicable => {
            "Native replacement: not applicable — the native product owns the protocol".to_string()
        }
    }
}

fn non_empty_list(values: &[&str]) -> String {
    if values.is_empty() {
        "(no native products named yet)".to_string()
    } else {
        values.join(" + ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_tools::EXTERNAL_TOOL_REGISTRY;
    use perl_test_must::must_with;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn entry_for(name: &str) -> Result<ExternalToolDoctorEntry, Box<dyn std::error::Error>> {
        external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY)
            .into_iter()
            .find(|entry| entry.canonical_name == name)
            .ok_or_else(|| format!("registry row for {name} should project").into())
    }

    #[test]
    fn registry_drives_every_verdict() {
        // Consumption proof: a fixture registry with one added row changes the
        // projection and the rendered output, so the doctor cannot be
        // duplicating registry policy.
        let mut fixture = EXTERNAL_TOOL_REGISTRY.to_vec();
        let mut extra = EXTERNAL_TOOL_REGISTRY[0];
        extra.canonical_name = "Perl::ExtraFixture";
        fixture.push(extra);

        let entries = external_tool_doctor_entries(&fixture);
        assert_eq!(entries.len(), fixture.len());
        assert!(entries.iter().any(|entry| entry.canonical_name == "Perl::ExtraFixture"));

        let rendered = render_external_tool_doctor_text(&entries);
        assert!(rendered.contains("Perl::ExtraFixture"));
        let baseline =
            render_external_tool_doctor_text(&external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY));
        assert_ne!(rendered, baseline);
    }

    #[test]
    fn shipped_native_replacement_yields_native_available_without_probing() -> TestResult {
        let pls = entry_for("Perl::LanguageServer")?;
        assert_eq!(pls.native_status_code, NATIVE_STATUS_AVAILABLE);
        assert!(pls.native_products.contains(&"perllsp"));
        // No probe dependency: the projection reads only registry rows.
        assert!(!pls.required_for_native);
        assert!(!pls.degrades_native_health);
        Ok(())
    }

    #[test]
    fn absent_external_tools_never_degrade_native_health() {
        let entries = external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY);
        assert!(!entries.is_empty());
        for entry in &entries {
            assert!(!entry.required_for_native, "{} must not be required", entry.canonical_name);
            assert!(
                !entry.degrades_native_health,
                "{} must not degrade native health",
                entry.canonical_name
            );
        }

        let rendered = render_external_tool_doctor_text(&entries);
        assert!(rendered.contains("never degrades native readiness"));
        assert!(!rendered.to_lowercase().contains("unhealthy"));
        assert!(!rendered.to_lowercase().contains("missing tool degrades"));
    }

    #[test]
    fn perlcritic_is_never_a_runtime_engine_or_health_warning() -> TestResult {
        let critic = entry_for("Perl::Critic")?;
        assert_eq!(critic.status_code, STATUS_CONFORMANCE_ONLY);
        assert_eq!(critic.reason_code, REASON_RUNTIME_ENABLEMENT_FORBIDDEN);
        assert_eq!(critic.execution_support, "repository_conformance_only");
        assert_eq!(critic.runtime_enablement, "forbidden");

        let rendered = render_external_tool_doctor_text(std::slice::from_ref(&critic));
        assert!(!rendered.contains("Configure External Perl::Critic"));
        assert!(rendered.contains("repository conformance only; not a runtime engine"));
        Ok(())
    }

    #[test]
    fn critic_compatibility_entry_comes_from_the_registry() -> TestResult {
        let entry = critic_compatibility_entry(EXTERNAL_TOOL_REGISTRY)
            .ok_or("registry should own a .perlcriticrc row")?;
        assert_eq!(entry.canonical_name, "Perl::Critic");
        assert!(entry.config_files.contains(&".perlcriticrc"));

        let rendered = render_critic_compatibility_text(&entry);
        assert!(rendered.contains(".perlcriticrc"));
        assert!(rendered.contains("read process-free"));
        assert!(rendered.contains("No runtime engine switch exists"));
        assert!(rendered.contains("No Perl::Critic installation is required"));
        assert!(rendered.contains("Native critic readiness is never affected"));
        assert!(!rendered.contains("Configure External Perl::Critic"));
        Ok(())
    }

    #[test]
    fn pls_is_conformance_only_in_ordinary_projection() -> TestResult {
        let pls = entry_for("Perl::LanguageServer")?;
        assert_eq!(pls.status_code, STATUS_CONFORMANCE_ONLY);
        assert_eq!(pls.allowed_roles, vec!["conformance_oracle"]);
        assert_eq!(pls.execution_support, "repository_conformance_only");
        let rendered = render_external_tool_doctor_text(std::slice::from_ref(&pls));
        assert!(rendered.contains("not a runtime engine"));
        Ok(())
    }

    #[test]
    fn explicit_adapters_and_peers_carry_their_typed_verdicts() -> TestResult {
        let perltidy = entry_for("Perl::Tidy")?;
        assert_eq!(perltidy.status_code, STATUS_OPTIONAL_EXTERNAL);
        assert_eq!(perltidy.reason_code, REASON_EXPLICIT_ADAPTER_ONLY);
        assert_eq!(perltidy.native_status_code, NATIVE_STATUS_PLANNED);
        assert_eq!(perltidy.install_help_scope, "user_requested_compatibility");

        let perlimports = entry_for("App::perlimports")?;
        assert_eq!(perlimports.status_code, STATUS_OPTIONAL_EXTERNAL);

        let ptkdb = entry_for("Devel::ptkdb")?;
        assert_eq!(ptkdb.status_code, STATUS_OPTIONAL_PEER);
        assert_eq!(ptkdb.reason_code, REASON_EXPLICIT_OPTIONAL_PEER);
        assert_eq!(ptkdb.native_delivery, NativeReplacementDelivery::NotApplicable);
        // ptkdb's install_help_scope is user_requested_compatibility, but the
        // peer role must win: a debugger-peer user gets the peer-session next
        // action, not the generic compatibility-setup text.
        assert!(ptkdb.safe_next_action.contains("peer session"));
        assert!(!ptkdb.safe_next_action.contains("compatibility setup"));
        Ok(())
    }

    #[test]
    fn critic_compatibility_fails_closed_on_duplicate_config_owner() {
        // Duplicate .perlcriticrc ownership is not rejected by registry
        // validation; the doctor projection must not guess by declaration
        // order.
        let mut fixture = EXTERNAL_TOOL_REGISTRY.to_vec();
        let mut impostor = EXTERNAL_TOOL_REGISTRY[2];
        impostor.canonical_name = "Perl::Impostor";
        fixture.push(impostor);
        assert!(critic_compatibility_entry(&fixture).is_none());

        // The real registry still projects exactly one owner.
        assert!(critic_compatibility_entry(EXTERNAL_TOOL_REGISTRY).is_some());
    }

    #[test]
    fn renderer_headline_follows_entry_health_claims() {
        // A mutated row that claims native-health dependence must change the
        // aggregate headline, so text and typed JSON cannot disagree.
        let mut fixture = EXTERNAL_TOOL_REGISTRY.to_vec();
        fixture[0].required_for_native = true;

        let entries = external_tool_doctor_entries(&fixture);
        let rendered = render_external_tool_doctor_text(&entries);
        assert!(rendered.contains("WARNING: registry rows claim native health dependence"));
        assert!(!rendered.contains("never degrades native readiness"));
        assert!(entries[0].degrades_native_health);
    }

    #[test]
    fn output_carries_no_private_paths_or_environment_values() {
        let rendered =
            render_external_tool_doctor_text(&external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY));
        // Source-only projection: no drive letters, no home-relative paths,
        // no env-var interpolation appear in the report.
        for line in rendered.lines() {
            let lower = line.to_lowercase();
            assert!(!lower.contains("c:\\") && !lower.contains("/home/"));
            assert!(!line.contains("${") && !line.contains('%'));
        }
    }

    #[test]
    fn entries_are_json_serializable_for_shared_cli_editor_codes() {
        let entries = external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY);
        let json = match serde_json::to_string(&entries) {
            Ok(json) => json,
            Err(error) => format!("serialization failed: {error}"),
        };
        assert!(json.contains("\"status_code\":\"conformance_only\""));
        assert!(json.contains("\"reason_code\":\"runtime_enablement_forbidden\""));
    }

    #[test]
    fn unclassified_registry_row_reports_unknown_honestly() {
        let mut fixture = EXTERNAL_TOOL_REGISTRY.to_vec();
        // A row with no adapter, no peer, but explicit runtime enablement does
        // not match any known shape: it must surface as unknown, never as a
        // silent pass.
        fixture[0].roles = &[ExternalToolRole::ConfigurationCompatibility];
        fixture[0].runtime_enablement = RuntimeEnablement::ExplicitUserAction;

        let entry = must_with(
            external_tool_doctor_entries(&fixture)
                .into_iter()
                .next()
                .ok_or("fixture row should project"),
            "fixture row should project",
        );
        assert_eq!(entry.status_code, STATUS_UNKNOWN);
        assert_eq!(entry.reason_code, REASON_UNCLASSIFIED);
        let rendered = render_external_tool_doctor_text(std::slice::from_ref(&entry));
        assert!(rendered.contains("did not match a known role shape"));
    }
}
