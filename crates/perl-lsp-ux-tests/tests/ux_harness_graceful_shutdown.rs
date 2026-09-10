use anyhow::{Result, anyhow, ensure};
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, binary_available};
use std::time::Duration;

#[test]
fn explicit_shutdown_waits_for_response_then_zero_exit() -> Result<()> {
    ensure!(
        binary_available(),
        "perl-lsp binary is unavailable; build it with `cargo build -p perllsp`"
    );

    let harness =
        UxHarness::new(ScenarioConfig { timeout: Duration::from_secs(10), ..Default::default() })?;

    let evidence = harness.client.shutdown_and_exit(Duration::from_secs(10))?;
    ensure!(
        evidence.status.success(),
        "explicit completion returned non-zero evidence: {}",
        evidence.status
    );

    let duplicate =
        harness.client.shutdown_and_exit(Duration::from_secs(1)).err().ok_or_else(|| {
            anyhow!("a completed client must not emit a second shutdown/exit sequence")
        })?;
    ensure!(
        duplicate.to_string().contains("explicit LSP shutdown already started or completed"),
        "a duplicate call must fail at the lifecycle guard, not the closed transport: {duplicate:#}"
    );
    Ok(())
}
