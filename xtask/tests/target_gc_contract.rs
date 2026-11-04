//! Contract tests for scripts/target-gc.sh (#12791).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn target_gc_is_dry_run_default_and_apply_gated() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let script = fs::read_to_string(root.join("scripts/target-gc.sh"))?;

    assert!(
        script.contains("DRY-RUN BY DEFAULT: nothing is deleted unless --apply is passed"),
        "target-gc must document the dry-run default"
    );
    assert!(
        script.contains("if [ \"$apply\" -ne 1 ]; then"),
        "deletion must be gated on an explicit --apply"
    );
    assert!(
        script.contains("refuse_if_build_lock_held"),
        "target-gc must refuse while the devplane build flock is held"
    );
    assert!(
        !script.contains("rm -rf -- \"$root\"") && script.contains("rm -rf -- \"$candidate\""),
        "deletion must target vetted candidates only"
    );
    // The candidate shape guard must reject anything that is not a target/
    // dir at the repo root, directly under .worktrees, or under the real
    // agent-worktree home .claude/worktrees (the home clean-worktrees.sh
    // reconciled agents to, #3573). Each allowed alternative is asserted
    // separately so extending the allowlist does not invalidate the other
    // alternatives' needles, and the last alternative is asserted with the
    // case-pattern terminator so the allowlist still ends closed.
    assert!(
        script.contains("\"$root\"/target|"),
        "the candidate shape guard must allow the repo root target/ dir"
    );
    assert!(
        script.contains("\"$root\"/.worktrees/*/target"),
        "the candidate shape guard must allow .worktrees/*/target dirs"
    );
    assert!(
        script.contains("\"$root\"/.claude/worktrees/*/target)"),
        "the candidate shape guard must allow .claude/worktrees/*/target dirs and terminate the allowlist there"
    );

    let justfile = fs::read_to_string(root.join("justfile"))?;
    assert!(
        justfile.contains("target-gc *args:")
            && justfile.contains("./scripts/target-gc.sh {{args}}"),
        "just must expose target-gc as a passthrough recipe"
    );

    Ok(())
}

#[test]
fn target_gc_self_test_discriminates_stale_from_fresh() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    // The script's own discrimination test: builds fresh/stale/decoy fixtures
    // in a temp dir, asserts only the stale target/ is selected, that --apply
    // preserves the fresh tree and registry/lockfile decoys, and that a held
    // devplane flock refuses the run.
    // MSYS bash strips backslashes from absolute Windows paths passed as
    // argv, so forward-slash the script path before handing it to bash on
    // Windows (#15435 / #15423 family C8).
    let script = root.join("scripts/target-gc.sh");
    let script_arg = script.to_string_lossy().replace('\\', "/");
    let output = Command::new("bash").arg(script_arg).arg("--self-test").output()?;
    assert!(
        output.status.success(),
        "target-gc --self-test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("target-gc self-test: OK"),
        "self-test must report its OK marker"
    );
    Ok(())
}

#[test]
fn target_gc_self_test_plumbing_requires_an_injected_root() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let script = root.join("scripts/target-gc.sh");
    // MSYS bash strips backslashes from absolute Windows paths (#15435 / #15423
    // family C8); forward-slash the script path before handing it to bash.
    let script_arg = script.to_string_lossy().replace('\\', "/");
    let output = Command::new("bash")
        .arg(script_arg)
        .arg("--self-test-apply")
        .env_remove("TARGET_GC_SELFTEST_ROOT")
        .env_remove("TARGET_GC_SELFTEST_DRY_RUN")
        .output()?;

    assert!(!output.status.success(), "self-test apply must not fall back to the real repository");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires its injected fixture root"),
        "missing fixture-root refusal must be explicit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
