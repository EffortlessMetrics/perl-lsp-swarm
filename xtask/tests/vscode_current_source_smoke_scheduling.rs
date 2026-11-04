//! Host-side proof for the VS Code current-source smoke scheduling observer (#6887).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};

fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest.parent().context("CARGO_MANIFEST_DIR has no parent")?.to_path_buf())
}

#[test]
fn scheduling_observer_unittest_passes() -> Result<()> {
    let root = repo_root()?;
    let status = Command::new("python3")
        .args(["-m", "unittest", "scripts.ci.test_vscode_current_source_smoke_scheduling"])
        .current_dir(&root)
        .env("PYTHONPATH", &root)
        .status()
        .context("failed to spawn scheduling observer unittest")?;
    ensure!(
        status.success(),
        "scripts.ci.test_vscode_current_source_smoke_scheduling must pass (exit {status})"
    );
    Ok(())
}
