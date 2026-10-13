//! `cargo xtask release-trust-invariants` — versioned trust-invariant registry (#9392).

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::release_trust_invariants as registry;

#[derive(clap::Subcommand, Debug)]
pub enum ReleaseTrustInvariantsSubcommand {
    /// Validate schema, owner/producer authority, mandatory rows, and generated status.
    Check,
    /// Same as `check`, then rewrite `docs/project/status/release_trust_invariants.md`.
    WriteStatus,
    /// List stable invariant IDs in registry order.
    List,
}

pub fn run(command: ReleaseTrustInvariantsSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        ReleaseTrustInvariantsSubcommand::Check => {
            let document = registry::check(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "release trust-invariants check passed: {} invariant(s), {} producer(s)",
                document.invariants.len(),
                document.producer_authorities.len()
            );
        }
        ReleaseTrustInvariantsSubcommand::WriteStatus => {
            let document =
                registry::check_and_write_status(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "wrote {} ({} invariant(s))",
                registry::STATUS_PATH,
                document.invariants.len()
            );
        }
        ReleaseTrustInvariantsSubcommand::List => {
            let document = registry::load_and_validate(&root).map_err(|error| eyre!("{error}"))?;
            for invariant_id in registry::list_invariant_ids(&document) {
                println!("{invariant_id}");
            }
        }
    }
    Ok(())
}
