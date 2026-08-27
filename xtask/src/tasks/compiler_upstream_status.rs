//! `cargo xtask compiler upstream status ...` — project the exact current
//! upstream-derived compiler conformance state into one deterministic machine
//! packet and one generated human view (#12532).
//!
//! All commands are read-only except the deterministic repository-local
//! generated outputs written by `build`/`docs`. They never execute the oracle
//! or compiler, touch the network, or mutate product/profile state.

use clap::Subcommand;
use color_eyre::eyre::{Result, eyre};
use std::path::PathBuf;
use xtask::compiler_upstream_status as status;

#[derive(Subcommand, Debug)]
pub enum CompilerUpstreamStatusSubcommand {
    /// Project reviewed inputs into one deterministic machine status packet.
    Build {
        /// Directory holding manifest.json and rows/*.json inputs.
        #[arg(long)]
        inputs: PathBuf,
        /// Output path for the generated machine packet.
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate one machine packet and print its stable identity.
    Check {
        /// Path to the machine packet JSON.
        path: PathBuf,
    },
    /// Print rows of one validated packet, optionally filtered.
    Show {
        /// Path to the machine packet JSON.
        path: PathBuf,
        /// Only rows of this maintained Perl series.
        #[arg(long)]
        series: Option<String>,
        /// Only rows whose concept id or family matches exactly.
        #[arg(long)]
        concept: Option<String>,
    },
    /// Deterministically compare two validated packets.
    Diff {
        /// Earlier packet path.
        before: PathBuf,
        /// Later packet path.
        after: PathBuf,
    },
    /// Render the generated Markdown view of one validated packet.
    Docs {
        /// Path to the machine packet JSON.
        #[arg(long)]
        status: PathBuf,
        /// Output path for the generated Markdown view.
        #[arg(long)]
        output: PathBuf,
    },
    /// Check generated Markdown matches its validated packet byte for byte.
    DocsCheck {
        /// Path to the machine packet JSON.
        #[arg(long)]
        status: PathBuf,
        /// Path of the committed generated Markdown view.
        #[arg(long)]
        path: PathBuf,
    },
}

fn map_error(result: anyhow::Result<()>) -> Result<()> {
    result.map_err(|error| eyre!("{error}"))
}

pub fn run(command: CompilerUpstreamStatusSubcommand) -> Result<()> {
    match command {
        CompilerUpstreamStatusSubcommand::Build { inputs, output } => {
            map_error(status::run_build(&inputs, &output))
        }
        CompilerUpstreamStatusSubcommand::Check { path } => map_error(status::run_check(&path)),
        CompilerUpstreamStatusSubcommand::Show {
            path,
            series,
            concept,
        } => map_error(status::run_show(
            &path,
            series.as_deref(),
            concept.as_deref(),
        )),
        CompilerUpstreamStatusSubcommand::Diff { before, after } => {
            map_error(status::run_diff(&before, &after))
        }
        CompilerUpstreamStatusSubcommand::Docs { status: status_path, output } => {
            map_error(status::run_docs(&status_path, &output))
        }
        CompilerUpstreamStatusSubcommand::DocsCheck { status: status_path, path } => {
            map_error(status::run_docs_check(&status_path, &path))
        }
    }
}
