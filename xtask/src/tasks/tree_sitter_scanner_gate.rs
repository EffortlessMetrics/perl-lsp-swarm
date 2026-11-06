//! `cargo xtask tree-sitter-scanner` — authority/projection gate (#7261).

use color_eyre::eyre::{Result, bail, eyre};
use std::process::Command;

use crate::utils::project_root;
use xtask::tree_sitter_scanner as scanner_authority;

pub fn run_sync() -> Result<()> {
    let root = project_root()?;
    let outcome =
        scanner_authority::sync(&root).map_err(|violation| eyre!(violation.display_line()))?;
    println!("scanner authority `{}` sha256={}", outcome.authority_path, outcome.authority_sha256);
    if outcome.updated_projections.is_empty() {
        println!("active projections already matched authority");
    } else {
        for path in &outcome.updated_projections {
            println!("updated projection {path}");
        }
    }
    Ok(())
}

pub fn run_check() -> Result<()> {
    let root = project_root()?;
    let violations = scanner_authority::check(&root);
    if violations.is_empty() {
        println!("tree-sitter scanner authority gate: OK");
        return Ok(());
    }
    for violation in &violations {
        eprintln!("{}", violation.display_line());
    }
    let codes: Vec<_> = violations.iter().map(|v| format!("{:?}", v.code)).collect();
    bail!("tree-sitter scanner authority gate failed ({})", codes.join(", "));
}

pub fn run_package_smoke() -> Result<()> {
    run_check()?;
    let root = project_root()?;
    let output = Command::new("cargo")
        .args(["check", "--locked", "-p", "tree-sitter-perl-c"])
        .current_dir(&root)
        .output()
        .map_err(|error| color_eyre::eyre::eyre!("spawning cargo check: {error}"))?;
    if !output.status.success() {
        bail!(
            "tree-sitter-perl-c check failed after scanner gate:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
