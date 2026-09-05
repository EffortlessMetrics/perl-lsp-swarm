//! `cargo xtask critic-rule-proof` — versioned native critic rule-proof checker (#6973).

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::critic_rule_proof as proof;

#[derive(clap::Subcommand, Debug)]
pub enum CriticRuleProofSubcommand {
    /// Validate schema, authorities, fixture digests, live critic cases, and generated status.
    Check,
    /// Same as `check`, then rewrite `docs/project/status/critic_rule_proof.md`.
    WriteStatus,
    /// List stable case IDs in manifest order.
    List,
}

pub fn run(command: CriticRuleProofSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        CriticRuleProofSubcommand::Check => {
            let manifest = proof::check(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "critic rule-proof check passed: {} pilot rule(s), {} case(s), {} fixture(s)",
                manifest.rules.len(),
                manifest.cases.len(),
                manifest.fixtures.len()
            );
        }
        CriticRuleProofSubcommand::WriteStatus => {
            let manifest =
                proof::check_and_write_status(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "wrote {} ({} pilot rule(s), {} case(s))",
                proof::STATUS_PATH,
                manifest.rules.len(),
                manifest.cases.len()
            );
        }
        CriticRuleProofSubcommand::List => {
            let manifest = proof::load_and_validate(&root).map_err(|error| eyre!("{error}"))?;
            for case_id in proof::list_case_ids(&manifest) {
                println!("{case_id}");
            }
        }
    }
    Ok(())
}
