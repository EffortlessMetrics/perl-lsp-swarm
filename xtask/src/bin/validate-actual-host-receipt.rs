use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::PathBuf;
use xtask::actual_host_receipt::validate_receipt;

fn main() -> Result<()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let Some(path) = args.next() else {
        bail!("usage: validate-actual-host-receipt <receipt.json>");
    };
    if args.next().is_some() {
        bail!("usage: validate-actual-host-receipt <receipt.json>");
    }

    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read actual-host receipt {}", path.display()))?;
    let receipt: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse actual-host receipt {}", path.display()))?;
    validate_receipt(&receipt)
        .with_context(|| format!("validate actual-host receipt {}", path.display()))?;
    println!("actual-host receipt valid: {}", path.display());
    Ok(())
}
