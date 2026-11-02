//! CLI round trips for `compiler-ast-impact` (#5238 slice 1).

use anyhow::{Context, Result};
use assert_cmd::cargo::cargo_bin_cmd;
use std::path::PathBuf;

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.to_path_buf())
        .context("xtask manifest dir should have a parent")
}

#[test]
fn check_passes_on_committed_packet() -> Result<()> {
    let root = workspace_root()?;
    cargo_bin_cmd!("compiler-ast-impact")
        .args([
            "check",
            "--packet",
            root.join("contracts/compiler/ast_change_impact.v1.toml")
                .to_str()
                .context("packet path")?,
            "--status",
            root.join("docs/project/status/ast_change_impact.md")
                .to_str()
                .context("status path")?,
            "--schema",
            root.join("schemas/ast_change_impact.v1.schema.json")
                .to_str()
                .context("schema path")?,
        ])
        .assert()
        .success();
    Ok(())
}

#[test]
fn explain_chained_comparison() -> Result<()> {
    let root = workspace_root()?;
    cargo_bin_cmd!("compiler-ast-impact")
        .args([
            "explain",
            "ChainedComparison",
            "--packet",
            root.join("contracts/compiler/ast_change_impact.v1.toml")
                .to_str()
                .context("packet path")?,
        ])
        .assert()
        .success();
    Ok(())
}
