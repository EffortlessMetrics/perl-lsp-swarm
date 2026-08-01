use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

/// The exact retirement receipt `check-active-goal-manifest` must print.
///
/// Duplicated from `xtask::tasks::active_goal_manifest` on purpose: this is a
/// black-box CLI test, and pinning the literal is what makes a partial
/// regression (any re-enabled selection, validation, or mutation) fail here
/// instead of sliding past a loose substring check.
const EXPECTED_RECEIPT: &str = "check-active-goal-manifest: retired: selected_work=none, validation_performed=false, mutation_performed=false; use current GitHub issues/PRs and deliver-goal";

#[test]
fn check_active_goal_manifest_emits_only_the_inert_retirement_receipt() -> Result<()> {
    let output = cargo_bin_cmd!("xtask").arg("check-active-goal-manifest").output()?;

    assert!(
        output.status.success(),
        "retired compatibility command should succeed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0), "retired compatibility command must always exit 0");

    let stdout = String::from_utf8(output.stdout)?;
    // Compare exact bytes, including the single trailing newline. Trimming
    // trailing newlines would let an extra `println!()` slip past a pin whose
    // whole purpose is to enforce "exactly one inert line".
    assert_eq!(
        stdout,
        format!("{EXPECTED_RECEIPT}\n"),
        "retirement receipt drifted; the command must emit exactly one inert line"
    );

    // A partial regression that re-enabled selection, validation, or mutation
    // while still printing the word "retired" must fail: the receipt states
    // each of those as an explicit negative, and nothing may be selected.
    assert!(stdout.contains("selected_work=none"), "receipt must prove no work was selected");
    assert!(
        stdout.contains("validation_performed=false"),
        "receipt must prove no manifest validation ran"
    );
    assert!(stdout.contains("mutation_performed=false"), "receipt must prove nothing was mutated");
    assert!(
        !stdout.contains("lane cap"),
        "receipt must not report lane-cap validation; got: {stdout}"
    );
    assert!(
        !stdout.contains("work item"),
        "receipt must not report work-item selection; got: {stdout}"
    );

    Ok(())
}

#[test]
fn check_active_goal_manifest_reads_no_goal_manifest_from_the_working_directory() -> Result<()> {
    // The retired command must not depend on repository state at all. Running
    // it from a directory with no `.perl-lsp/goals/` tree must be byte-identical
    // to running it from the workspace root.
    let from_repo = cargo_bin_cmd!("xtask").arg("check-active-goal-manifest").output()?;
    let from_tmp = cargo_bin_cmd!("xtask")
        .arg("check-active-goal-manifest")
        .current_dir(std::env::temp_dir())
        .output()?;

    assert!(from_tmp.status.success(), "retired command must succeed outside the repository");
    assert_eq!(
        String::from_utf8(from_repo.stdout)?,
        String::from_utf8(from_tmp.stdout)?,
        "retired command must not read repository goal state"
    );

    Ok(())
}
