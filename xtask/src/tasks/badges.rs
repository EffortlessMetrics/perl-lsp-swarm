//! Deprecated compatibility entry point for public RIPR badge endpoints.
//!
//! Badge semantics are owned by `scripts/generate-badges.py`. This module must
//! remain a delegate and must not parse RIPR output or map badge fields.

use std::process::Command;

use color_eyre::eyre::{Context, Result, bail};

use crate::utils::project_root;

pub fn run(check: bool) -> Result<()> {
    let workspace_root = project_root()?;
    eprintln!("cargo xtask badges is deprecated; invoking python3 scripts/generate-badges.py");
    let mut command = Command::new("python3");
    command.arg("scripts/generate-badges.py").current_dir(&workspace_root);
    if check {
        command.arg("--check");
    }
    let status = command.status().wrap_err("running the Python badge endpoint owner")?;
    if !status.success() {
        bail!("Python badge endpoint generator failed with {status}");
    }
    Ok(())
}
