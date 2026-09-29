//! CLI over the checked independent coordinate corpus (#8172).

use clap::Subcommand;
use color_eyre::eyre::{Result, bail, eyre};
use perl_position_fixtures::{PROJECTION_PATH, load};
use std::fs;

/// Fixture operation.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fail closed on raw bytes, literal facts, and projection freshness.
    Check,
    /// Write the deterministic Markdown view, or check that it is current.
    Project {
        /// Only check; never rewrite.
        #[arg(long)]
        check: bool,
    },
    /// List stable case IDs, optionally filtered by tag.
    List {
        /// Exact tag or case ID.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Explain one case and all its literal coordinate facts.
    Explain {
        /// Stable case ID.
        id: String,
    },
}

/// Run an operation after mandatory integrity validation.
pub fn run(command: Command) -> Result<()> {
    let root = crate::utils::project_root()?;
    let manifest = load(&root).map_err(|message| eyre!(message))?;
    match command {
        Command::Check | Command::Project { check: true } => {
            let path = root.join(PROJECTION_PATH);
            let actual = fs::read_to_string(&path)?;
            if actual != manifest.markdown() {
                bail!("{} is stale; run cargo xtask position-fixtures project", path.display());
            }
            println!(
                "position fixtures: {} validated cases; projection current",
                manifest.cases().len()
            );
        }
        Command::Project { check: false } => {
            let path = root.join(PROJECTION_PATH);
            fs::write(&path, manifest.markdown())?;
            println!("wrote {}", path.display());
        }
        Command::List { filter } => {
            let selected = filter
                .as_deref()
                .map_or_else(|| manifest.cases().iter().collect(), |tag| manifest.select(tag));
            for case in selected {
                println!("{}\t{}", case.id, case.tags.join(","));
            }
        }
        Command::Explain { id } => {
            let detail = manifest.explain(&id).ok_or_else(|| eyre!("unknown case {id}"))?;
            print!("{detail}");
        }
    }
    Ok(())
}
