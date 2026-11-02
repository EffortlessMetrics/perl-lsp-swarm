//! Regression and wiring proof for #13411 cargo lock coherence.

use assert_cmd::Command;
use std::error::Error;
use std::fs;
use std::path::Path;

fn write_workspace(dir: &Path, manifest: &str, lock: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("Cargo.toml"), manifest)?;
    fs::write(dir.join("Cargo.lock"), lock)?;
    fs::write(dir.join("src/lib.rs"), "")?;
    Ok(())
}

#[test]
fn current_repository_passes_cargo_lock_coherence_cli() -> Result<(), Box<dyn Error>> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("repo root");
    let output = Command::cargo_bin("xtask")?
        .current_dir(repo_root)
        .args(["cargo-lock-coherence"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected current tree COHERENT: stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(stdout.contains("COHERENT"), "stdout={stdout:?}");
    Ok(())
}

#[test]
fn fixture_missing_path_dep_lock_edge_fails() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("dep/src"))?;
    fs::write(
        dir.path().join("dep/Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(dir.path().join("dep/src/lib.rs"), "")?;
    write_workspace(
        dir.path(),
        "[package]\nname = \"root-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dev-dependencies]\ndep = { path = \"dep\" }\n",
        "version = 3\n\n[[package]]\nname = \"root-fixture\"\nversion = \"0.0.0\"\n",
    )?;

    let output = std::process::Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(dir.path())
        .output()?;
    assert!(!output.status.success(), "missing lock edge must fail locked metadata");
    Ok(())
}

#[test]
fn gate_policy_wires_pr_fast_and_commit_checks() -> Result<(), Box<dyn Error>> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("repo root");
    let policy = fs::read_to_string(repo_root.join(".ci/gate-policy.yaml"))?;
    assert!(policy.contains("name: cargo_lock_coherence\n"), "pr_fast gate missing");
    assert!(
        policy.contains("command: cargo xtask cargo-lock-coherence"),
        "pr_fast command missing"
    );
    assert!(policy.contains("name: cargo_lock_coherence_staged\n"), "commit-tier gate missing");
    assert!(
        policy.contains("command: cargo xtask commit-check cargo_lock_coherence_staged"),
        "commit-tier command missing"
    );
    Ok(())
}
