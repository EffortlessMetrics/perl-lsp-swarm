use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn check_active_goal_manifest_passes_for_current_manifest() -> Result<()> {
    let mut cmd = cargo_bin_cmd!("xtask");
    let output = cmd.arg("check-active-goal-manifest").output()?;

    assert!(
        output.status.success(),
        "active goal manifest check should pass; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("active goal manifest check passed:"),
        "expected success receipt in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains("actionable"),
        "expected actionable work count in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains("current: receiver-real-workspace-quality-receipt"),
        "expected current work item in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains(
            "current work item plan: plans/editor-trust-ux-closeout/implementation-plan.md#work-item-receiver-real-workspace-quality-receipt"
        ),
        "expected current work item plan in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains(
            "current work item pointer: docs/project/status/receiver_facts.md#next-implementation-steps"
        ),
        "expected current work item pointer in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains(
            "current work item status: Ready next because receiver facts have a narrow source-backed pilot"
        ),
        "expected current work item status in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains("current work item claim boundary: Receipt-only receiver quality proof"),
        "expected current work item claim boundary in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains("current work item proof commands:"),
        "expected current work item proof command header in stdout; got: {stdout}"
    );
    assert!(
        stdout.contains("  - rtk cargo test -p perl-lsp-ux-tests --test ux_scenario_28_mojolicious_completion_ranking"),
        "expected current work item proof commands in stdout; got: {stdout}"
    );

    Ok(())
}
