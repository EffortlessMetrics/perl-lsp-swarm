#[path = "../../tests/support/zed_settings_behavior.rs"]
mod zed_settings_behavior;

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

fn content_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity("sha256:".len() + 64);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let contract_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing contract path"))?;
    let schema_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing schema path"))?;
    let receipt_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing receipt path"))?;
    if arguments.next().is_some() {
        return Err(io::Error::other("expected exactly three paths").into());
    }

    let contract_bytes = fs::read(&contract_path)?;
    let schema_bytes = fs::read(&schema_path)?;
    let receipt_bytes = fs::read(&receipt_path)?;
    let contract: Value = serde_json::from_slice(&contract_bytes)?;
    let schema: Value = serde_json::from_slice(&schema_bytes)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;

    zed_settings_behavior::validate_contract(&contract, &schema).map_err(io::Error::other)?;
    if receipt.get("result").and_then(Value::as_str) == Some("pass") {
        let bound = receipt
            .pointer("/contract/sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("passing receipt lacks contract.sha256"))?;
        if bound != content_sha256(&contract_bytes) {
            return Err(
                io::Error::other("receipt contract digest does not match contract bytes").into()
            );
        }
    }
    zed_settings_behavior::validate_receipt(&receipt, &contract).map_err(io::Error::other)?;
    println!("Zed settings behavior receipt checks passed.");
    Ok(())
}
