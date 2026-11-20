#[path = "support/agent_client_compat.rs"]
mod agent_client_compat;

use agent_client_compat::{
    AgentClientCompatReceipt, EvidenceStage, FailureClass, HostIdentity, HostProduct,
    IntegrationIdentity, IntegrationMode, JourneyCell, ObservationResult, Protocol, SCHEMA_VERSION,
    ServerIdentity, WorkspaceFixtureIdentity, fixture_digest,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use xtask::client_compat_fixture::is_reason_token;
use xtask::codex_ide_compat::{
    CLI_SKILL_JOURNEY_CELL_IDS, COMPANION_PLUGIN_INSTALLABILITY_CELL,
    NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS, SEMANTIC_INTEGRATION_MODES, all_journey_cell_ids,
    blocked_claim_limitation, journey_catalog_is_complete,
};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn fixture_root() -> Result<PathBuf> {
    Ok(repository_root()?.join("crates/perl-lsp-ux-tests/fixtures/agent-client-compat"))
}

fn sha256(fill: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    value.extend(std::iter::repeat_n(fill, 64));
    value
}

fn blocked_receipt(mode: IntegrationMode, protocol: Protocol) -> Result<AgentClientCompatReceipt> {
    let mode_name = match mode {
        IntegrationMode::CliSkill => "cli_skill",
        IntegrationMode::NativeLocalMcp => "native_local_mcp",
        IntegrationMode::NativeLspPlugin | IntegrationMode::ExternalBridge => {
            anyhow::bail!("unexpected integration mode for Codex IDE substrate")
        }
    };
    let journey_ids = match mode {
        IntegrationMode::CliSkill => CLI_SKILL_JOURNEY_CELL_IDS,
        IntegrationMode::NativeLocalMcp => NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS,
        IntegrationMode::NativeLspPlugin | IntegrationMode::ExternalBridge => &[],
    };
    let limitation = blocked_claim_limitation(mode_name);
    let journey = journey_ids
        .iter()
        .map(|id| JourneyCell {
            id: (*id).to_string(),
            result: ObservationResult::NotProven,
            evidence: Vec::new(),
            limitation: Some(limitation.clone()),
        })
        .collect();

    Ok(AgentClientCompatReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        observed_at: "2026-11-02T08:00:00Z".to_string(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        candidate_sha: "0".repeat(40),
        platform: agent_client_compat::PlatformIdentity {
            os: "linux".to_string(),
            os_version: "ubuntu-24.04".to_string(),
            arch: "x86_64".to_string(),
        },
        host: HostIdentity {
            product: HostProduct::CodexIde,
            version: "not-exercised".to_string(),
            instrument_model: None,
        },
        integration: IntegrationIdentity {
            mode,
            plugin_name: "perl-code-intelligence".to_string(),
            plugin_version: "not-available".to_string(),
            marketplace_source: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            marketplace_ref: "refs/heads/main".to_string(),
            package_sha256: sha256('a'),
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: "not-exercised".to_string(),
            build_revision: "0".repeat(40),
            artifact_sha256: sha256('b'),
            protocol,
            protocol_or_schema_version: "not-exercised".to_string(),
        },
        workspace_fixture: WorkspaceFixtureIdentity {
            id: "perl-agent-client-v1".to_string(),
            digest: fixture_digest(&fixture_root()?)?,
        },
        journey,
        result: ObservationResult::NotProven,
        failure_class: Some(FailureClass::Environment),
        limitations: vec![limitation],
        artifacts: Vec::new(),
        claim_boundary: format!(
            "Exact-source Codex IDE {mode_name} cells only; CLI/desktop receipts cannot substitute."
        ),
    })
}

#[test]
fn journey_catalog_matches_issue_7826_matrix() -> Result<()> {
    ensure!(journey_catalog_is_complete(), "journey catalog contains duplicate ids");
    for id in all_journey_cell_ids() {
        ensure!(is_reason_token(id), "journey cell id must be a reason token: {id}");
    }
    ensure!(
        SEMANTIC_INTEGRATION_MODES == &["cli_skill", "native_local_mcp"],
        "semantic integration modes drifted"
    );
    ensure!(
        !all_journey_cell_ids().contains(&COMPANION_PLUGIN_INSTALLABILITY_CELL)
            || COMPANION_PLUGIN_INSTALLABILITY_CELL == "host.plugin_installability",
        "companion plugin cell id drifted"
    );
    Ok(())
}

#[test]
fn blocked_codex_ide_receipts_validate_for_each_semantic_mode() -> Result<()> {
    let cli = blocked_receipt(IntegrationMode::CliSkill, Protocol::Cli)?;
    cli.validate()?;
    ensure!(cli.host.product == HostProduct::CodexIde);

    let mcp = blocked_receipt(IntegrationMode::NativeLocalMcp, Protocol::Mcp)?;
    mcp.validate()?;
    ensure!(mcp.integration.mode == IntegrationMode::NativeLocalMcp);

    let cli_cells = cli.journey.iter().map(|cell| cell.id.as_str()).collect::<BTreeSet<_>>();
    ensure!(
        cli_cells == CLI_SKILL_JOURNEY_CELL_IDS.iter().copied().collect::<BTreeSet<_>>(),
        "cli_skill journey cells drifted from catalog"
    );
    Ok(())
}

#[test]
fn codex_ide_host_product_serializes_for_schema() -> Result<()> {
    let receipt = blocked_receipt(IntegrationMode::CliSkill, Protocol::Cli)?;
    let value = serde_json::to_value(&receipt.host)?;
    ensure!(value["product"] == "codex_ide");
    Ok(())
}

#[test]
fn plugin_installability_is_companion_observation_not_integration_mode() -> Result<()> {
    ensure!(
        COMPANION_PLUGIN_INSTALLABILITY_CELL == "host.plugin_installability",
        "companion cell id drifted"
    );
    ensure!(
        !SEMANTIC_INTEGRATION_MODES.contains(&"plugin_installation"),
        "plugin_installation must not appear as an agent_client_compat integration mode"
    );
    Ok(())
}
