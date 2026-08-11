//! Thin CLI for the transition classify command surface.
//!
//! This slice only settles argument parsing for:
//! `perl-core-harness-transition classify --accepted-baseline <path> --compile <path> --output <path>`
//!
//! Loading JSON, validating observations, calling in-lib `classify_transition`,
//! and writing classification receipts remain follow-up slices.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;

include!("perl-core-harness-transition/cli.rs");

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-transition classify --accepted-baseline <path> --compile <path> --output <path>"
        )
    })?;
    let options = Options::parse(args)?;
    match command.as_str() {
        "classify" => {
            let config = ClassifyConfig::from_options(options)?;
            let receipt = ParsedClassifyArgs {
                schema_version: PARSED_CLASSIFY_ARGS_SCHEMA_VERSION,
                command: "classify",
                accepted_baseline: config.accepted_baseline.display().to_string(),
                compile: config.compile.display().to_string(),
                output: config.output.display().to_string(),
                claim_boundary: "parses classify CLI arguments only; does not load evidence, classify transitions, or write receipts",
            };
            let encoded = serde_json::to_string(&receipt)?;
            let mut out = io::stdout().lock();
            out.write_all(encoded.as_bytes())?;
            out.write_all(b"\n")?;
            Ok(())
        }
        _ => bail!("unknown perl-core-harness-transition command: {command}"),
    }
}

const PARSED_CLASSIFY_ARGS_SCHEMA_VERSION: &str = "perl_core_harness.transition_classify_args.v1";

#[derive(Debug, Serialize)]
struct ParsedClassifyArgs {
    schema_version: &'static str,
    command: &'static str,
    accepted_baseline: String,
    compile: String,
    output: String,
    claim_boundary: &'static str,
}
