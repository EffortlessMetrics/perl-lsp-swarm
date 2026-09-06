use anyhow::{Result, ensure};
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, binary_available};
use std::time::Duration;

#[test]
fn explicit_shutdown_waits_for_response_then_zero_exit() -> Result<()> {
    ensure!(
        binary_available(),
        "perl-lsp binary is unavailable; build it with `cargo build -p perl-lsp-rs`"
    );

    let harness =
        UxHarness::new(ScenarioConfig { timeout: Duration::from_secs(10), ..Default::default() })?;

    let evidence = harness.client.shutdown_and_exit(Duration::from_secs(10))?;
    ensure!(
        evidence.status.success(),
        "explicit completion returned non-zero evidence: {}",
        evidence.status
    );

    let duplicate = harness.client.shutdown_and_exit(Duration::from_secs(1));
    ensure!(duplicate.is_err(), "a completed client must not emit a second shutdown/exit sequence");
    Ok(())
}
