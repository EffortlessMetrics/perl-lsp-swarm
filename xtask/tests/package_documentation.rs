//! Integration tests for package documentation projections (#14122).

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
fn current_tree_package_documentation_roundtrip() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    Command::cargo_bin("xtask")?
        .current_dir(&root)
        .args(["package-documentation"])
        .assert()
        .success();
    Command::cargo_bin("xtask")?
        .current_dir(&root)
        .args(["package-documentation", "--check"])
        .assert()
        .success();
    Ok(())
}
