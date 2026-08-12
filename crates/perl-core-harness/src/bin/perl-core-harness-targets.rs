#![allow(clippy::print_stdout)]

//! Validate the versioned upstream Perl target matrix without executing Perl.

#[path = "../target_contracts/model.rs"]
mod model;
#[path = "../target_contracts/contract.rs"]
mod contract;
#[path = "../target_contracts/matrix.rs"]
mod matrix;
#[path = "../target_contracts/io.rs"]
mod io;

use color_eyre::eyre::{ContextCompat, Result, bail};
use io::{read_drift, read_matrix};
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        bail!(
            "usage: perl-core-harness-targets check <pinned-matrix> [drift.json] [observed-matrix]"
        );
    };
    if command.as_os_str() != OsStr::new("check") {
        bail!("unsupported command {:?}; expected check", command);
    }
    let matrix_path = args
        .next()
        .map(PathBuf::from)
        .context("check requires a pinned target matrix path")?;
    let drift_path = args.next().map(PathBuf::from);
    let observed_matrix_path = args.next().map(PathBuf::from);
    if let Some(extra) = args.next() {
        bail!("unexpected extra argument {:?}", extra);
    }

    let matrix = read_matrix(&matrix_path)?;
    let fingerprint = matrix
        .fingerprint()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    if let Some(path) = drift_path.as_deref() {
        let drift = read_drift(path)?;
        let observed = observed_matrix_path
            .as_deref()
            .map(read_matrix)
            .transpose()?;
        drift
            .validate_against(&matrix, &fingerprint, observed.as_ref())
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
    } else if observed_matrix_path.is_some() {
        bail!("an observed matrix requires a drift receipt");
    }
    println!("target matrix valid: {fingerprint}");
    Ok(())
}

#[cfg(test)]
#[path = "../target_contracts/tests.rs"]
mod tests;
