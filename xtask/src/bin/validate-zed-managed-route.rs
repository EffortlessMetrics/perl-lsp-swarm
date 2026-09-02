//! Validate a Zed managed-route receipt against the checked-in contract.
//!
//! Infrastructure authority for #8753: this CLI reuses the journey-test
//! support validator so CI, reviewers, and the successor real-host evidence
//! issue all share one contract and receipt authority.

#![allow(clippy::print_stderr, clippy::print_stdout)]

#[path = "../../tests/support/zed_managed_route.rs"]
mod zed_managed_route;

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
struct Args {
    /// Managed-route contract document (defaults to the checked-in fixture).
    #[arg(long, default_value = ".ci/fixtures/zed-perl-upstream/managed-route.v1.json")]
    contract: PathBuf,
    /// Receipt to validate (defaults to the checked-in not_run template).
    #[arg(
        long,
        default_value = ".ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json"
    )]
    receipt: PathBuf,
}

fn read_json(path: &Path) -> Result<Value> {
    let bytes = fs::read(path).wrap_err_with(|| format!("cannot read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .wrap_err_with(|| format!("cannot parse {} as JSON", path.display()))
}

fn file_digest(path: &Path) -> Result<String> {
    let bytes = fs::read(path).wrap_err_with(|| format!("cannot read {}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("sha256:{hex}"))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let contract = read_json(&args.contract)?;
    let receipt = read_json(&args.receipt)?;

    zed_managed_route::validate_contract(&contract)
        .map_err(|message| eyre!("invalid contract {}: {message}", args.contract.display()))?;

    let recorded = receipt.pointer("/contract/sha256").and_then(Value::as_str).unwrap_or_default();
    if !recorded.is_empty() {
        let actual = file_digest(&args.contract)?;
        if recorded != actual {
            bail!("contract digest mismatch: receipt records `{recorded}`, file is `{actual}`");
        }
    }

    zed_managed_route::validate_receipt(&receipt, &contract)
        .map_err(|message| eyre!("invalid receipt {}: {message}", args.receipt.display()))?;
    println!("managed-route contract and receipt are valid");
    Ok(())
}
