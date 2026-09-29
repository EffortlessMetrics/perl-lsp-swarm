//! Current-tree reachability proof for the staged product-topology checker.

use assert_cmd::Command;
use std::error::Error;
use std::path::{Path, PathBuf};

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest has no repository parent".into())
}

#[test]
fn current_tree_cli_accepts_the_absent_stage() -> Result<(), Box<dyn Error>> {
    let output =
        Command::cargo_bin("product-topology")?.current_dir(repo_root()?).arg("check").output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "current-tree product-topology check failed: stdout={stdout:?} stderr={stderr:?}"
        )
        .into());
    }
    for field in [
        "product-topology: accepted",
        "stage=absent",
        "product=perllsp",
        "dap=perl-dap",
        "mcp_package=absent",
    ] {
        if !stdout.contains(field) {
            return Err(format!(
                "current-tree product-topology output omitted {field:?}: {stdout:?}"
            )
            .into());
        }
    }
    if !stderr.is_empty() {
        return Err(format!(
            "current-tree product-topology check wrote unexpected stderr: {stderr:?}"
        )
        .into());
    }
    Ok(())
}

/// Build a dependency-free workspace the checker can run `cargo metadata
/// --locked` against, so the instrument-failure seam can be driven without
/// the real repository tree. The hand-written lockfile keeps `--locked`
/// satisfiable offline.
fn fixture_workspace(dir: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"topology-fixture\"\nversion = \"0.0.0\"\n\
         edition = \"2021\"\n\n[workspace.metadata.publish]\nallow = [\"topology-fixture\"]\n",
    )?;
    std::fs::write(
        dir.join("Cargo.lock"),
        "version = 3\n\n[[package]]\nname = \"topology-fixture\"\nversion = \"0.0.0\"\n",
    )?;
    std::fs::write(dir.join("src/lib.rs"), "")?;
    Ok(())
}

fn run_in(dir: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::cargo_bin("product-topology")?.current_dir(dir).arg("check").output()?)
}

/// A policy file that is not readable is an instrument failure (exit 2), never
/// a silent acceptance. Without this the only proven exit code is 0, so a
/// checker that swallowed its own load errors would still look green.
#[test]
fn missing_policy_file_is_an_instrument_failure() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fixture_workspace(dir.path())?;

    let output = run_in(dir.path())?;

    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("instrument failure"), "stderr={stderr:?}");
    assert!(stderr.contains("product-topology.toml"), "stderr={stderr:?}");
    Ok(())
}

/// A policy file that exists but does not parse is an instrument failure, not
/// a rejection and not an acceptance — the checker must not fall back to a
/// default topology when its own contract is unreadable.
#[test]
fn unparseable_policy_is_an_instrument_failure() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fixture_workspace(dir.path())?;
    std::fs::create_dir_all(dir.path().join("policy"))?;
    std::fs::write(dir.path().join("policy/product-topology.toml"), "this is not = = toml\n")?;

    let output = run_in(dir.path())?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr={stderr:?}");
    // Name the parse seam: `cargo metadata` failing in the fixture would also
    // exit 2, and would leave this test green for the wrong reason.
    assert!(stderr.contains("instrument failure"), "stderr={stderr:?}");
    assert!(stderr.contains("parse"), "stderr={stderr:?}");
    assert!(stderr.contains("product-topology.toml"), "stderr={stderr:?}");
    Ok(())
}

/// A syntactically valid policy naming an unknown stage must fail closed
/// rather than defaulting to `absent`, which is the permissive state.
#[test]
fn unknown_mcp_stage_is_an_instrument_failure() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fixture_workspace(dir.path())?;
    std::fs::create_dir_all(dir.path().join("policy"))?;
    let real = std::fs::read_to_string(repo_root()?.join("policy/product-topology.toml"))?;
    let mutated = real.replace("mcp_stage = \"absent\"", "mcp_stage = \"eventually\"");
    assert_ne!(real, mutated, "fixture must actually mutate the stage");
    std::fs::write(dir.path().join("policy/product-topology.toml"), mutated)?;

    let output = run_in(dir.path())?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr={stderr:?}");
    assert!(stderr.contains("parse"), "stderr={stderr:?}");
    assert!(stderr.contains("product-topology.toml"), "stderr={stderr:?}");
    Ok(())
}

/// Any argument shape other than the single `check` command fails closed.
#[test]
fn unexpected_arguments_fail_closed() -> Result<(), Box<dyn Error>> {
    let output =
        Command::cargo_bin("product-topology")?.current_dir(repo_root()?).arg("checkk").output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected exactly one command"));
    Ok(())
}

/// The rejection exit (1) is distinct from the instrument-failure exit (2):
/// a readable policy evaluated against a tree that violates it is a finding,
/// not a broken instrument. Proving only 0 and 2 would leave the exit code
/// the merge surface actually gates on unbound.
#[test]
fn violating_tree_is_rejected_not_an_instrument_failure() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fixture_workspace(dir.path())?;
    std::fs::create_dir_all(dir.path().join("policy"))?;
    // The real policy, unmodified — the fixture workspace simply does not
    // contain the packages it governs.
    std::fs::copy(
        repo_root()?.join("policy/product-topology.toml"),
        dir.path().join("policy/product-topology.toml"),
    )?;

    let output = run_in(dir.path())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert!(stderr.contains("product-topology: rejected"), "stderr={stderr:?}");
    assert!(!stderr.contains("instrument failure"), "stderr={stderr:?}");
    assert!(stdout.is_empty(), "a rejected tree must not print an acceptance: {stdout:?}");
    Ok(())
}
