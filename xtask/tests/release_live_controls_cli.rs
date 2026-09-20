//! Regression proof that the release-live-controls observer remains reachable
//! through the xtask binary (#16073).

use anyhow::{Context, Result, ensure};
use assert_cmd::Command;

#[test]
fn release_live_controls_subcommand_is_wired() -> Result<()> {
    let output = Command::cargo_bin("xtask")
        .context("locating the xtask test binary")?
        .args(["release-live-controls", "--help"])
        .output()
        .context("running release-live-controls --help")?;

    ensure!(
        output.status.success(),
        "release-live-controls --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    ensure!(
        help.contains("Observe live GitHub publication controls"),
        "help output did not describe the wired subcommand: {help}"
    );
    Ok(())
}
