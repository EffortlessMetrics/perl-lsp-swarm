//! Product-independent Cargo package-name resolver.
//!
//! This binary intentionally lives in `perl-ci-hygiene` rather than `xtask`
//! so hooks and other compiler-light control-plane callers do not select the
//! parser/LSP product graph merely to map a crate directory to its Cargo
//! package identity.

use clap::Parser;
use color_eyre::eyre::{Context, Result};

#[derive(Debug, Parser)]
#[command(
    name = "resolve-package-name",
    about = "Resolve a workspace crate directory to its Cargo package name"
)]
struct Args {
    /// Workspace-relative crate directory, for example `crates/perl-lsp-rs`.
    #[arg(value_name = "CRATE_DIR")]
    crate_dir: String,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = std::env::current_dir().context("failed to read the current workspace root")?;
    let name = perl_ci_hygiene::package_resolver::resolve_single_package_name(
        &root,
        &args.crate_dir,
    )?;
    println!("{name}");
    Ok(())
}
