//! Optional exact-host smoke for the Codex IDE extension (#7826).
//!
//! Run locally when `CODEX_IDE_SMOKE=1` and the extension host is installed:
//!
//! ```bash
//! CODEX_IDE_SMOKE=1 cargo test -p xtask codex_ide_host_smoke -- --ignored --test-threads=1
//! ```

#[path = "support/agent_client_compat.rs"]
mod agent_client_compat;

use agent_client_compat::{HostProduct, IntegrationMode, Protocol, SCHEMA_VERSION};
use anyhow::{Result, bail, ensure};
use xtask::codex_ide_compat::{CLI_SKILL_JOURNEY_CELL_IDS, NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS};

#[test]
#[ignore = "requires a locally installed Codex IDE extension host and exact #7825/#6958 subjects"]
fn codex_ide_host_smoke_records_cli_skill_and_native_mcp_rows() -> Result<()> {
    if std::env::var_os("CODEX_IDE_SMOKE").is_none() {
        bail!("set CODEX_IDE_SMOKE=1 to opt into the exact-host smoke");
    }
    ensure!(CLI_SKILL_JOURNEY_CELL_IDS.len() >= 5);
    ensure!(NATIVE_LOCAL_MCP_JOURNEY_CELL_IDS.len() >= 5);
    ensure!(SCHEMA_VERSION == "agent_client_compat.v1");
    let _ = (HostProduct::CodexIde, IntegrationMode::CliSkill, Protocol::Cli);
    let _ = (IntegrationMode::NativeLocalMcp, Protocol::Mcp);
    bail!("host smoke harness not wired yet; blocked on #7825 and #6958/#7230 product subjects")
}
