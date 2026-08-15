//! CLI for the read-only contributor development/publication topology projection.

use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;

mod contributor_topology;

use contributor_topology::{Projection, build_projection, render_human, validate_projection};

#[derive(Parser, Debug)]
#[command(about = "Project the canonical contributor topology without network access")]
struct Args {
    /// Repository root containing policy/product-identity.toml.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Captured read-only live observation JSON. Omit to retain NOT_PROVEN.
    #[arg(long)]
    observation: Option<PathBuf>,
    /// Write the canonical projection to this path.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Validate the existing --output file against current source authority.
    #[arg(long)]
    check: bool,
    /// Print canonical JSON rather than the human projection.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.check {
        let output = args.output.as_ref().context("--check requires --output")?;
        let raw = fs::read_to_string(output)
            .with_context(|| format!("reading {}", output.display()))?;
        let projection: Projection = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", output.display()))?;
        validate_projection(&args.root, &projection)?;
        println!("contributor-topology: OK");
        return Ok(());
    }

    let projection = build_projection(&args.root, args.observation.as_deref())?;
    if let Some(output) = &args.output {
        if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(output, serde_json::to_string_pretty(&projection)? + "\n")
            .with_context(|| format!("writing {}", output.display()))?;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&projection)?);
    } else {
        println!("{}", render_human(&projection));
    }
    Ok(())
}
