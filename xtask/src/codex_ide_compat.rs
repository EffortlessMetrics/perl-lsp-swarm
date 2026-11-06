//! Codex IDE extension compatibility substrate for #7826.
//!
//! Defines the journey cell vocabulary and blocked-receipt boundaries consumed by
//! contract tests and optional host smoke runs. This module does not claim that
//! any IDE host row passes; it pins the exact matrix from issue #7826 and the
//! revised receipt model from Steven's 2026-08-20 research comment.

/// Governing issues that must land before semantic `agent_client_compat.v1` rows
/// can execute instead of remaining blocked.
pub const BLOCKED_ON_ISSUES: &[u64] = &[7825, 6958, 7230];

/// Standalone #7825 skill route (`codex_ide` + `cli_skill`).
pub const CLI_SKILL_JOURNEY_CELL_IDS: &[&str] = &[
    "skill.discovery",
    "skill.invoke_canonical_fixture",
    "skill.select_narrow_command",
    "skill.host_edit_refresh",
    "skill.partial_or_unreadable",
    "skill.source_identity",
    "skill.removal_refresh",
];

/// Shared/direct `perllsp mcp --stdio` route (`codex_ide` + `native_local_mcp`).
pub const NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS: &[&str] = &[
    "mcp.cli_configured_route",
    "mcp.ide_configured_route",
    "mcp.shared_config_observed",
    "mcp.tool_inventory",
    "mcp.semantic_calls",
    "mcp.post_edit_freshness",
    "mcp.partial_or_failure_path",
    "mcp.preview_non_mutation",
    "mcp.restart_cleanup",
];

/// Companion host observation; not an `agent_client_compat.v1` integration mode.
pub const COMPANION_PLUGIN_INSTALLABILITY_CELL: &str = "host.plugin_installability";

/// Every semantic integration mode row expected for one exact IDE host version.
pub const SEMANTIC_INTEGRATION_MODES: &[&str] = &["cli_skill", "native_local_mcp"];

pub fn all_journey_cell_ids() -> Vec<&'static str> {
    let mut ids = Vec::with_capacity(
        CLI_SKILL_JOURNEY_CELL_IDS.len() + NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS.len() + 1,
    );
    ids.extend_from_slice(CLI_SKILL_JOURNEY_CELL_IDS);
    ids.extend_from_slice(NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS);
    ids.push(COMPANION_PLUGIN_INSTALLABILITY_CELL);
    ids
}

pub fn journey_catalog_is_complete() -> bool {
    let ids = all_journey_cell_ids();
    ids.len() == ids.iter().collect::<std::collections::BTreeSet<_>>().len()
}

pub fn blocked_claim_limitation(mode: &str) -> String {
    format!(
        "Blocked on exact product subjects (#7825 standalone skill, #6958/#7230 native MCP); \
         {mode} IDE row remains not_proven until host smoke lands under #7826."
    )
}
