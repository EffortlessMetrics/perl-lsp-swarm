// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anyhow::Result;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, binary_available};
use std::time::Duration;

#[test]
fn explicit_shutdown_waits_for_response_then_zero_exit() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP explicit_shutdown: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(ScenarioConfig {
        timeout: Duration::from_secs(10),
        ..Default::default()
    })?;

    let evidence = harness.client.shutdown_and_exit(Duration::from_secs(10))?;
    assert!(
        evidence.status.success(),
        "explicit completion returned non-zero evidence: {}",
        evidence.status
    );

    let duplicate = harness.client.shutdown_and_exit(Duration::from_secs(1));
    assert!(
        duplicate.is_err(),
        "a completed client must not emit a second shutdown/exit sequence"
    );
    Ok(())
}
