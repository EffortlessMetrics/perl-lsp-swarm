#[path = "../../tests/support/zed_host_compat.rs"]
mod zed_host_compat;

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;

fn main() -> Result<(), Box<dyn Error>> {
    let usage = "usage: validate-zed-host-receipt [--schema-only] <receipt.json>";
    let mut schema_only = false;
    let mut receipt = None;
    for argument in env::args_os().skip(1) {
        if argument == "--schema-only" {
            schema_only = true;
        } else if receipt.is_none() {
            receipt = Some(PathBuf::from(argument));
        } else {
            return Err(io::Error::other(usage).into());
        }
    }
    let receipt = receipt.ok_or_else(|| io::Error::other(usage))?;

    let receipt_bytes = fs::read(&receipt)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    zed_host_compat::validate_schema(&receipt).map_err(io::Error::other)?;
    if !schema_only {
        zed_host_compat::validate_pass(&receipt, None).map_err(io::Error::other)?;
    }
    writeln!(io::stdout(), "Zed exact-source host receipt checks passed.")?;
    Ok(())
}
