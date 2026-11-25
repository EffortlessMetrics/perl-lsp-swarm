//! `cargo xtask framework-attribute-generation-conformance` — validate #2373 truth rows.

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::framework_attribute_generation_conformance as fag;

#[derive(clap::Subcommand, Debug)]
pub enum FrameworkAttributeGenerationConformanceSubcommand {
    /// List stable case IDs in manifest order.
    List,
    /// Validate manifest identity, digests, matrices, and failure probes.
    Validate,
}

pub fn run(command: FrameworkAttributeGenerationConformanceSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        FrameworkAttributeGenerationConformanceSubcommand::List => {
            let manifest = fag::load_manifest(&root).map_err(|error| eyre!("{error}"))?;
            for case_id in fag::list_case_ids(&manifest) {
                println!("{case_id}");
            }
        }
        FrameworkAttributeGenerationConformanceSubcommand::Validate => {
            let stats = fag::validate_manifest_file(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "framework attribute-generation conformance check passed: {} fixtures, {} cases, {} failure probes",
                stats.fixtures, stats.cases, stats.failure_probes
            );
        }
    }
    Ok(())
}
