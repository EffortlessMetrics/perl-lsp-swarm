//! `cargo xtask compiler-lexical-cutline` — list, validate, and explain the
//! compiler lexical cut-line cases manifest (#12156).

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::compiler_lexical_cutline as cutline;

#[derive(clap::Subcommand, Debug)]
pub enum CompilerLexicalCutlineSubcommand {
    /// List the stable case IDs in manifest order.
    List,
    /// Validate the manifest, its schema, and its deterministic canonical bytes.
    Validate,
    /// Print one case row as pretty JSON.
    Explain {
        /// Stable case ID (for example `LX-POS-001`).
        case_id: String,
    },
}

pub fn run(command: CompilerLexicalCutlineSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        CompilerLexicalCutlineSubcommand::List => {
            let manifest = cutline::load_manifest(&root).map_err(|error| eyre!("{error}"))?;
            for case_id in cutline::list_case_ids(&manifest) {
                println!("{case_id}");
            }
        }
        CompilerLexicalCutlineSubcommand::Validate => {
            let stats = cutline::validate_manifest_file(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "compiler lexical cut-line manifest check passed: {} fixtures, {} cases, {} mutations, {} work invariants",
                stats.fixtures, stats.cases, stats.mutations, stats.work_invariants
            );
        }
        CompilerLexicalCutlineSubcommand::Explain { case_id } => {
            let manifest = cutline::load_manifest(&root).map_err(|error| eyre!("{error}"))?;
            let explained = cutline::explain_case(&manifest, &case_id)
                .ok_or_else(|| eyre!("unknown case id `{case_id}`"))?;
            println!("{explained}");
        }
    }
    Ok(())
}
