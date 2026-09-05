//! Canonical Clippy repair-falsifier corpus (`ClippyRepairFalsifierCorpusV1`).
//!
//! The corpus freezes one reusable denominator of known dishonest-repair and
//! verifier-weakening mutations (#11649) as checked-in fixtures plus a
//! fail-closed validator. The validator reads repository files only; it never
//! executes Cargo, Clippy, or any other instrument, never generates expected
//! results from an implementation under test, and never owns suppression,
//! subject, finding-admission, or suggestion policy. Expected rejections name
//! existing landed authorities; cases whose rejecting authority is not current
//! carry an explicit pending-owner record instead of a fabricated binding.

mod model;
mod validate;

#[cfg(test)]
mod tests;

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Validate the canonical Clippy repair-falsifier corpus invariants")]
struct Args {
    /// Repository root containing fixtures/, schemas/, and the lint authorities.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
}

pub fn run_from_env() -> Result<()> {
    let args = Args::parse();
    let report = validate::validate_corpus(&args.repo_root)?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(
        handle,
        "clippy-repair-corpus: {} cases validated ({} bound, {} pending-owner); all invariants hold",
        report.case_count, report.bound_count, report.pending_count
    )?;
    Ok(())
}

/// Load and parse one JSON document with a stable error prefix.
pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> Result<T> {
    serde_json::from_str(raw).wrap_err_with(|| format!("parsing {what}"))
}
