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

// Windows process lookup may choose System32's WSL shim before PATH. Allow
// the fixture runner to select its installed Git Bash without starting WSL.
fn bash_command() -> Command {
    Command::new(std::env::var_os("TARGET_GC_TEST_BASH").unwrap_or_else(|| "bash".into()))
}

#[test]
fn target_gc_is_advisory_and_destructive_options_refuse() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let script = fs::read_to_string(root.join("scripts/target-gc.sh"))?;
    assert!(script.contains("DELETION RETIRED"), "retired deletion must be documented");
    assert!(
        !script.contains("rm -rf -- \"$candidate\"") && !script.contains("rm -rf -- \"$root\""),
        "inspection must not contain candidate/root deletion"
    );
    let script_arg = root.join("scripts/target-gc.sh").to_string_lossy().replace('\\', "/");
    for flag in ["--apply", "--self-test-apply"] {
        let output = bash_command().arg(&script_arg).arg(flag).output()?;
        assert_eq!(output.status.code(), Some(78), "{flag} must refuse");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("deletion is retired"),
            "{flag} must explain retirement"
        );
    }

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
    // in a temp dir, asserts only stale target/ paths are reported, destructive
    // options refuse, and all fresh/stale contents and registry/lockfile evidence
    // survive. Age classification does not establish owner or cleanup authority.
    // MSYS bash strips backslashes from absolute Windows paths passed as
    // argv, so forward-slash the script path before handing it to bash on
    // Windows (#15435 / #15423 family C8).
    let script = root.join("scripts/target-gc.sh");
    let script_arg = script.to_string_lossy().replace('\\', "/");
    let output = bash_command().arg(script_arg).arg("--self-test").output()?;
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
    let output = bash_command()
        .arg(script_arg)
        .arg("--self-test-dry-run")
        .env_remove("TARGET_GC_SELFTEST_ROOT")
        .env_remove("TARGET_GC_SELFTEST_DRY_RUN")
        .output()?;

    assert!(
        !output.status.success(),
        "self-test inspection must not fall back to the real repository"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires its injected fixture root"),
        "missing fixture-root refusal must be explicit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
