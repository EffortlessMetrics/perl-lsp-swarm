//! Validate managed-route observations and their exact upstream evidence.

#![allow(clippy::print_stderr, clippy::print_stdout)]

#[path = "../../tests/support/zed_managed_bindings.rs"]
mod zed_managed_bindings;
#[path = "../../tests/support/zed_managed_route.rs"]
mod zed_managed_route;

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use zed_managed_bindings::content_sha256;

#[derive(Debug, Parser)]
struct Args {
    /// Managed-route contract document.
    #[arg(long, default_value = ".ci/fixtures/zed-perl-upstream/managed-route.v1.json")]
    contract: PathBuf,
    /// Managed-route receipt; the default template claims no observation.
    #[arg(
        long,
        default_value = ".ci/fixtures/zed-perl-upstream/receipts/managed-route-template.json"
    )]
    receipt: PathBuf,
    /// Existing public-asset receipt, required for pass.
    #[arg(long)]
    asset_receipt: Option<PathBuf>,
    /// Existing exact-source host receipt, required for pass.
    #[arg(long)]
    host_receipt: Option<PathBuf>,
    /// Checked public-asset contract used by the existing Python authority.
    #[arg(long, default_value = ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json")]
    asset_contract: PathBuf,
    /// Python 3.11+ interpreter; used only for passing candidates.
    #[arg(long, default_value = if cfg!(windows) { "python" } else { "python3" })]
    python: PathBuf,
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).wrap_err_with(|| format!("cannot read {}", path.display()))
}

fn validate_asset_authority(python: &Path, asset: &[u8], contract: &[u8]) -> Result<()> {
    let captured = tempfile::tempdir().wrap_err("cannot create captured asset inputs")?;
    let asset_path = captured.path().join("asset-receipt.json");
    let contract_path = captured.path().join("asset-contract.json");
    fs::write(&asset_path, asset)?;
    fs::write(&contract_path, contract)?;
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/zed_public_asset_receipts.py");
    // Reuse validation only: direct argv, no shell, producer or downloaded code.
    let output = Command::new(python)
        .arg("-B")
        .arg(script)
        .arg("validate-receipt")
        .arg("--receipt")
        .arg(&asset_path)
        .arg("--contract")
        .arg(&contract_path)
        .output()
        .wrap_err("cannot run existing Python asset receipt validator")?;
    if !output.status.success() {
        bail!(
            "asset receipt authority rejected input: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let contract_bytes = read(&args.contract)?;
    let contract: Value =
        serde_json::from_slice(&contract_bytes).wrap_err("invalid contract JSON")?;
    let receipt: Value =
        serde_json::from_slice(&read(&args.receipt)?).wrap_err("invalid receipt JSON")?;
    zed_managed_route::validate_receipt(&receipt, &contract).map_err(|message| eyre!(message))?;
    if let Some(recorded) = receipt.pointer("/contract/sha256").and_then(Value::as_str)
        && recorded != content_sha256(&contract_bytes)
    {
        bail!("contract digest mismatch");
    }
    if receipt.get("result").and_then(Value::as_str) == Some("pass") {
        let asset_path =
            args.asset_receipt.as_deref().ok_or_else(|| eyre!("pass requires --asset-receipt"))?;
        let host_path =
            args.host_receipt.as_deref().ok_or_else(|| eyre!("pass requires --host-receipt"))?;
        let asset_bytes = read(asset_path)?;
        let host_bytes = read(host_path)?;
        let asset_contract_bytes = read(&args.asset_contract)?;
        zed_managed_bindings::validate_bound_pass(
            &receipt,
            &asset_bytes,
            &host_bytes,
            &asset_contract_bytes,
        )
        .map_err(|message| eyre!(message))?;
        validate_asset_authority(&args.python, &asset_bytes, &asset_contract_bytes)?;
    }
    println!("managed-route contract and receipt are valid");
    Ok(())
}
