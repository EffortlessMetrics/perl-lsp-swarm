//! CLI smoke for `cargo xtask release` candidate-artifact handoff (#9092).

use anyhow::{Context, Result, anyhow};
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::PathBuf;
use std::process::Output;
use std::time::{SystemTime, UNIX_EPOCH};

fn project_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("xtask must be in a subdirectory"))
}

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn create() -> Result<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).context("clock")?.as_nanos();
        let path = std::env::temp_dir()
            .join(format!("release-candidate-artifacts-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run(args: &[&str]) -> Result<Output> {
    let output = cargo_bin_cmd!("xtask").args(args).output()?;
    Ok(output)
}

#[test]
fn release_check_candidate_artifacts_proves_handoff_and_negative_controls() -> Result<()> {
    let output = run(&["release", "check-candidate-artifacts"])?;
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
    let output = run(&["release", "freeze-candidate-artifacts", "--help"])?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("does not rebuild or publish"), "{stdout}");
    Ok(())
}

#[test]
fn release_verify_requires_topology_and_artifact_set_id() -> Result<()> {
    let output = run(&["release", "verify-candidate-artifacts", "--help"])?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--topology"), "{stdout}");
    assert!(stdout.contains("--artifact-set-id"), "{stdout}");
    Ok(())
}

#[test]
fn release_freeze_then_verify_cli_round_trip() -> Result<()> {
    let root = project_root()?;
    let tmp = TempTree::create()?;
    let staging = tmp.path.join("staging");
    fs::create_dir_all(&staging)?;
    let topology_src = root.join("fixtures/release_candidate_artifacts/topology.json");
    let topology = tmp.path.join("topology.json");
    fs::copy(&topology_src, &topology)?;
    fs::write(staging.join("perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz"), b"linux-archive\n")?;
    fs::write(staging.join("perllsp-0.18.0-x86_64-pc-windows-msvc.zip"), b"windows-archive\n")?;
    fs::write(staging.join("perl-lsp-rs-0.18.0.vsix"), b"vsix\n")?;
    fs::write(staging.join("SHA256SUMS"), b"sums\n")?;
    fs::write(staging.join("sbom.cdx.json"), b"{\"bomFormat\":\"CycloneDX\"}\n")?;
    let cargo_lock = tmp.path.join("Cargo.lock");
    let npm_lock = tmp.path.join("package-lock.json");
    fs::write(&cargo_lock, b"lock\n")?;
    fs::write(&npm_lock, b"npm\n")?;
    let packet = tmp.path.join("packet.json");
    let receipt = tmp.path.join("receipt.json");

    let freeze = run(&[
        "release",
        "freeze-candidate-artifacts",
        "--staging",
        staging.to_str().ok_or_else(|| anyhow!("staging utf8"))?,
        "--topology",
        topology.to_str().ok_or_else(|| anyhow!("topology utf8"))?,
        "--output",
        packet.to_str().ok_or_else(|| anyhow!("packet utf8"))?,
        "--candidate-id",
        "rc1",
        "--producer-workflow",
        "no-publish-candidate.yml",
        "--producer-run-id",
        "run-1",
        "--artifact-set-id",
        "set-rc1",
        "--cargo-lock",
        cargo_lock.to_str().ok_or_else(|| anyhow!("cargo utf8"))?,
        "--npm-lock",
        npm_lock.to_str().ok_or_else(|| anyhow!("npm utf8"))?,
        "--toolchain",
        "rustc=1.95.0",
    ])?;
    assert!(
        freeze.status.success(),
        "freeze failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&freeze.stdout),
        String::from_utf8_lossy(&freeze.stderr)
    );

    let verify = run(&[
        "release",
        "verify-candidate-artifacts",
        "--packet",
        packet.to_str().ok_or_else(|| anyhow!("packet utf8"))?,
        "--staging",
        staging.to_str().ok_or_else(|| anyhow!("staging utf8"))?,
        "--topology",
        topology.to_str().ok_or_else(|| anyhow!("topology utf8"))?,
        "--artifact-set-id",
        "set-rc1",
        "--producer-run-id",
        "run-1",
        "--receipt",
        receipt.to_str().ok_or_else(|| anyhow!("receipt utf8"))?,
    ])?;
    assert!(
        verify.status.success(),
        "verify failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
    let receipt_text = fs::read_to_string(&receipt)?;
    assert!(receipt_text.contains("\"publish_authorized\": false"), "{receipt_text}");
    assert!(receipt_text.contains("\"rebuild\": false"), "{receipt_text}");

    let rebuild = run(&[
        "release",
        "verify-candidate-artifacts",
        "--packet",
        packet.to_str().ok_or_else(|| anyhow!("packet utf8"))?,
        "--staging",
        staging.to_str().ok_or_else(|| anyhow!("staging utf8"))?,
        "--topology",
        topology.to_str().ok_or_else(|| anyhow!("topology utf8"))?,
        "--artifact-set-id",
        "set-rc1",
        "--rebuild-attempt",
    ])?;
    assert!(!rebuild.status.success(), "rebuild attempt must fail closed");
    let rebuild_err = format!(
        "{}{}",
        String::from_utf8_lossy(&rebuild.stdout),
        String::from_utf8_lossy(&rebuild.stderr)
    );
    assert!(rebuild_err.contains("rebuild_forbidden"), "{rebuild_err}");
    Ok(())
}
