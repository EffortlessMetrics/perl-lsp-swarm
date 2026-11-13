//! Compute/compare declared formal-input identity without accepting native proof.
//! Runbook: docs/how-to/FORMAL_CLOSURE.md.
#![expect(clippy::print_stdout, clippy::print_stderr, reason = "CLI JSON and diagnostics")]

#[path = "../formal_closure/mod.rs"]
mod formal_closure;
#[path = "../tasks/formal_receipt.rs"]
mod formal_receipt;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use formal_closure::{Freshness, snapshot::Snapshot};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(about = "Declared input freshness only; never authenticates a native proof")]
struct Args {
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    Compute {
        #[arg(long)]
        spec: String,
    },
    Compare {
        #[arg(long)]
        spec: String,
        #[arg(long)]
        receipt: PathBuf,
    },
}

fn document(path: &Path) -> Result<Vec<u8>> {
    ensure!(fs::symlink_metadata(path)?.file_type().is_file(), "document must be a regular file");
    let file = fs::File::open(path)?;
    ensure!(file.metadata()?.file_type().is_file(), "opened document is not regular");
    let mut bytes = Vec::new();
    file.take(formal_closure::MAX_DOCUMENT as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= formal_closure::MAX_DOCUMENT, "document exceeds ceiling");
    Ok(bytes)
}

fn current(root: &Path, path: &str) -> Result<formal_closure::Computed> {
    let snapshot = Snapshot::open(root)?;
    let blob = snapshot.read(path).context("read committed closure spec")?;
    let spec = formal_closure::parse_spec(&blob.bytes)?;
    formal_closure::compute(&snapshot, &spec)
}

fn run() -> Result<i32> {
    let args = Args::parse();
    match args.command {
        Action::Compute { spec } => {
            let computed = current(&args.repo_root, &spec)?;
            println!("{}", serde_json::to_string_pretty(&computed)?);
            Ok(if computed.unknown_inputs.is_empty() { 0 } else { 2 })
        }
        Action::Compare { spec, receipt } => {
            let receipt = formal_closure::parse_receipt(&document(&receipt)?)?;
            // A well-formed historical terminal record remains readable even if
            // its old source no longer exists. Failure to compute active inputs
            // is UNKNOWN, never CURRENT or inferred retirement.
            let computed = current(&args.repo_root, &spec);
            if let Err(error) = &computed {
                let detail: String = format!("{error:#}").chars().take(2048).collect();
                eprintln!("formal-closure current inputs unavailable: {detail}");
            }
            let comparison = formal_closure::compare(&receipt, computed.as_ref().ok());
            println!("{}", serde_json::to_string_pretty(&comparison)?);
            Ok(match comparison.freshness {
                Freshness::Current => 0,
                Freshness::Stale | Freshness::Retired | Freshness::Superseded => 1,
                Freshness::Unknown => 2,
            })
        }
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let detail: String = format!("{error:#}").chars().take(2048).collect();
            eprintln!("formal-closure NOT_PROVEN: {detail}");
            std::process::exit(2);
        }
    }
}
