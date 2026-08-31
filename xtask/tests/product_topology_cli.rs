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
