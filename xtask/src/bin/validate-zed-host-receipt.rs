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
    let mut arguments = env::args_os().skip(1);
    let receipt = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("usage: validate-zed-host-receipt <receipt.json>"))?;
    if arguments.next().is_some() {
        return Err(io::Error::other("usage: validate-zed-host-receipt <receipt.json>").into());
    }

    let receipt_bytes = fs::read(&receipt)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    zed_host_compat::validate_pass(&receipt, None).map_err(io::Error::other)?;
    writeln!(io::stdout(), "Zed exact-source host receipt checks passed.")?;
    Ok(())
}
