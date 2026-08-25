//! Thin argv shim over [`perl_core_harness::artifacts`].
//!
//! The producer and its proof live in the library so the workspace `--lib` gate
//! executes them; this binary only selects a subcommand.

use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-artifacts <capture-discovery|check-discovery|derive-runner-records|check-runner-records> [options]"
        )
    })?;
    perl_core_harness::artifacts::run(&command, args)
}
