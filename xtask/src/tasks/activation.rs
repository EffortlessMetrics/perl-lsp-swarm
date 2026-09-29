//! `cargo xtask activation` — versioned activation inventory (#9204).
//!
//! Generates and validates a deterministic classified catalog of product,
//! preview, compatibility-shim, test-api, lab, oracle, benchmark, and gate
//! surfaces. Does not implement activation checking (`check`/`report`/
//! `explain` belong to #9205) and does not change runtime behavior.

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::activation;

#[derive(clap::Subcommand, Debug)]
pub enum ActivationSubcommand {
    /// Regenerate the inventory in memory and fail on drift vs the
    /// committed `policy/activation-inventory.v1.json`. With `--write`,
    /// rewrite the committed artifact instead of failing on drift.
    Generate {
        /// Rewrite `policy/activation-inventory.v1.json` instead of failing on drift.
        #[arg(long)]
        write: bool,
    },
    /// Validate schema, row consistency, and the override ledger against
    /// the committed artifact.
    Validate,
    /// Deterministic reviewer-readable rendering to stdout.
    List,
}

pub fn run(command: ActivationSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        ActivationSubcommand::Generate { write: true } => {
            let inventory = activation::write(&root).map_err(|error| eyre!("{error}"))?;
            println!("wrote {} ({} row(s))", activation::INVENTORY_PATH, inventory.rows.len());
        }
        ActivationSubcommand::Generate { write: false } => {
            let inventory = activation::check_drift(&root).map_err(|error| eyre!("{error}"))?;
            println!("activation inventory is current: {} row(s), no drift", inventory.rows.len());
        }
        ActivationSubcommand::Validate => {
            let inventory = activation::validate(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "activation inventory valid: {} row(s), {} derivation rule(s)",
                inventory.rows.len(),
                inventory.derivation.len()
            );
        }
        ActivationSubcommand::List => {
            // Full validation, not just the artifact's own shape: rendering a
            // clean listing while the override ledger is invalid would present
            // rows the ledger cannot justify as if they were settled.
            let inventory = activation::validate(&root).map_err(|error| eyre!("{error}"))?;
            print!("{}", activation::render_list(&inventory));
        }
    }
    Ok(())
}
