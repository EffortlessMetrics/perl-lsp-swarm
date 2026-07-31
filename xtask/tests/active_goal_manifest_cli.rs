use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn check_active_goal_manifest_reports_retired_compatibility_command() -> Result<()> {
    let output = cargo_bin_cmd!("xtask")
        .arg("check-active-goal-manifest")
        .output()?;

    assert!(
        output.status.success(),
        "retired compatibility command should succeed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("retired"), "expected retired receipt; got: {stdout}");
    assert!(stdout.contains("GitHub"), "expected GitHub authority; got: {stdout}");
    Ok(())
}
