//! CLI smoke for `cargo xtask release check-candidate-artifacts` (#9092).

use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn release_check_candidate_artifacts_proves_handoff_and_negative_controls() -> Result<()> {
    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd.args(["release", "check-candidate-artifacts"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "check-candidate-artifacts failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("publish_authorized=false"),
        "check output must record the no-publish boundary\n{stdout}"
    );
    Ok(())
}

#[test]
fn release_freeze_candidate_artifacts_help_names_no_publish_boundary() -> Result<()> {
    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd.args(["release", "freeze-candidate-artifacts", "--help"]).output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("does not rebuild or publish"), "{stdout}");
    Ok(())
}
