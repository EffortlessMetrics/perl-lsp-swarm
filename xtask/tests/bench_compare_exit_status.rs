//! Comparator refusals retain their child status without Rust diagnostic frames.

use anyhow::Result;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn bench_compare_propagates_child_verdict_without_backtrace() -> Result<()> {
    let temp = TempDir::new()?;
    let root = temp.path();
    fs::create_dir_all(root.join("xtask"))?;
    fs::create_dir_all(root.join("benchmarks/scripts"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace.package]\nrepository = \"https://github.com/EffortlessMetrics/perl-lsp\"\n",
    )?;
    fs::write(root.join("xtask/Cargo.toml"), "[package]\nname = \"xtask\"\n")?;
    // A distinct nonzero status detects wrappers that turn every child failure
    // into exit 1. The real JSON refusal is covered by the Python CLI fixtures.
    fs::write(
        root.join("benchmarks/scripts/compare.sh"),
        "#!/bin/bash\necho 'latest.json: partial input' >&2\nexit 7\n",
    )?;
    let mut git = Command::new("git");
    let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            git.env_remove(&key);
            xtask.env_remove(key);
        }
    }
    let initialized = git.args(["init", "-q"]).current_dir(root).output()?;
    assert!(initialized.status.success(), "{}", String::from_utf8_lossy(&initialized.stderr));
    let output = xtask
        .arg("--worktree-root")
        .arg(root)
        .arg("bench-compare")
        .env("RUST_BACKTRACE", "1")
        .env("RUST_LIB_BACKTRACE", "1")
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(7), "{stderr}");
    assert!(stderr.contains("latest.json: partial input"), "{stderr}");
    assert!(!stderr.to_lowercase().contains("backtrace"), "{stderr}");
    assert!(!stderr.contains("benchmark script failed"), "{stderr}");
    Ok(())
}
