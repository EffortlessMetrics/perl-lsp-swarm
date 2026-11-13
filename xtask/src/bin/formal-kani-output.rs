//! Inspect a pinned Kani structured export without claiming authenticated proof.
#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "Inspection JSON and diagnostics are CLI output"
)]

#[path = "../formal_kani/mod.rs"]
mod formal_kani;

use anyhow::{Context, Result, bail, ensure};
use formal_kani::{MAX_INPUT_BYTES, ObservationKind};
use std::{env, fs::File, io::Read, path::Path};

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        std::fs::symlink_metadata(path)?.is_file(),
        "input is not a regular non-symlink file"
    );
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "input is not a regular file");
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_INPUT_BYTES, "input exceeds size limit");
    Ok(bytes)
}

fn run() -> Result<bool> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("usage: formal-kani-output <expected.json> <kani-export.json>\nInspects untrusted artifacts; never emits an accepted proof receipt.");
        return Ok(true);
    }
    if args.len() != 2 {
        bail!("usage: formal-kani-output <expected.json> <kani-export.json>");
    }
    let expected = formal_kani::parse_expected(&read_bounded(Path::new(&args[0]))?)?;
    let report = formal_kani::inspect(&read_bounded(Path::new(&args[1]))?, &expected)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(report.kind == ObservationKind::PositiveReport)
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("formal-kani-output NOT_PROVEN: {error:#}");
            std::process::exit(2);
        }
    }
}
