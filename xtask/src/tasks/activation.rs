//! `cargo xtask activation` — classified inventory (#9204) and fail-closed
//! class-contract checking (#9205).
//!
//! `generate` / `validate` / `list` own classification. `check` / `report` /
//! `explain` evaluate each row against its class connection requirements.
//! Direct Cargo dependency membership is topology evidence only.

use crate::utils::project_root;
use color_eyre::eyre::{Result, bail, eyre};
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
    /// Fail closed when any row misses its class connection contract.
    Check,
    /// Print the class-contract evaluation for every row.
    Report {
        /// Emit JSON instead of the reviewer-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Print the class-contract evaluation for one surface id.
    Explain {
        /// Surface id from the inventory (for example `feature:lsp.hover`).
        id: String,
    },
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
        ActivationSubcommand::Check => {
            let report = activation::check(&root).map_err(|error| eyre!("{error}"))?;
            print!("{}", activation::render_report(&report));
            if !report.is_clean() {
                bail!(
                    "activation check failed: {} row(s) missed their class contract",
                    report.failed
                );
            }
        }
        ActivationSubcommand::Report { json } => {
            let report = activation::check(&root).map_err(|error| eyre!("{error}"))?;
            if json {
                println!(
                    "{}",
                    activation::report_to_json(&report).map_err(|error| eyre!("{error}"))?
                );
            } else {
                print!("{}", activation::render_report(&report));
            }
            if !report.is_clean() {
                bail!(
                    "activation check failed: {} row(s) missed their class contract",
                    report.failed
                );
            }
        }
        ActivationSubcommand::Explain { id } => {
            let finding = activation::explain(&root, &id).map_err(|error| eyre!("{error}"))?;
            println!("{}", serde_json::to_string_pretty(&finding)?);
            if finding.is_fail() {
                bail!("activation surface `{id}` failed its class contract");
            }
        }
    }
    Ok(())
}
