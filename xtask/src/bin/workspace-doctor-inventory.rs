//! CLI for the read-only workspace doctor/readiness authority inventory.

use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;

mod workspace_doctor_inventory;

use workspace_doctor_inventory::{Inventory, build_inventory, render_human, validate_inventory};

#[derive(Parser, Debug)]
#[command(about = "Inventory workspace doctor checks without changing local state")]
struct Args {
    /// Repository root containing the current doctor and its authority sources.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Write the deterministic inventory to this path.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Validate the existing --output file against current sources.
    #[arg(long)]
    check: bool,
    /// Print canonical JSON rather than the human debt report.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.check {
        let output = args.output.as_ref().context("--check requires --output")?;
        let raw =
            fs::read_to_string(output).with_context(|| format!("reading {}", output.display()))?;
        let inventory: Inventory =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", output.display()))?;
        validate_inventory(&args.root, &inventory)?;
        println!("workspace-doctor-inventory: OK");
        return Ok(());
    }

    let inventory = build_inventory(&args.root)?;
    if let Some(output) = &args.output {
        if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(output, serde_json::to_string_pretty(&inventory)? + "\n")
            .with_context(|| format!("writing {}", output.display()))?;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&inventory)?);
    } else {
        println!("{}", render_human(&inventory));
    }
    Ok(())
}
