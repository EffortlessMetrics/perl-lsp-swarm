//! CA02B source binding check. This does not grant configuration authority.
#[path = "../high_risk_configuration_bindings.rs"]
mod bindings;

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os().nth(1).map(PathBuf::from).ok_or(
        "usage: high-risk-configuration-bindings <repository-root> <typescript-package-root>",
    )?;
    let typescript = std::env::args_os().nth(2).map(PathBuf::from);
    bindings::check(&root, typescript.as_deref())?;
    Ok(())
}
