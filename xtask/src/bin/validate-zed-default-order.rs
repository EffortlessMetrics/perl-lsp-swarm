#[path = "../../tests/support/zed_default_order.rs"]
mod zed_default_order;

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::from("sha256:");
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let contract_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing contract path"))?;
    let receipt_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing receipt path"))?;
    if arguments.next().is_some() {
        return Err(io::Error::other("expected contract and receipt paths").into());
    }
    let contract_bytes = fs::read(contract_path)?;
    let receipt_bytes = fs::read(receipt_path)?;
    let contract: Value = serde_json::from_slice(&contract_bytes)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    zed_default_order::validate_contract(&contract).map_err(io::Error::other)?;
    if receipt.get("result").and_then(Value::as_str) == Some("pass") {
        let bound = receipt
            .pointer("/contract/sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("passing receipt lacks contract digest"))?;
        if bound != sha256(&contract_bytes) {
            return Err(io::Error::other("receipt contract digest mismatch").into());
        }
    }
    zed_default_order::validate_receipt(&receipt, &contract).map_err(io::Error::other)?;
    writeln!(std::io::stdout(), "Zed default-order receipt checks passed.")?;
    Ok(())
}
