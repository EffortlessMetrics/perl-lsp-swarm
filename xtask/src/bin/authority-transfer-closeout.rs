//! CLI for exact-head leaf-closeout verification and canonical handoff
//! emission in the authority-transfer programme (issue #11703).
//!
//! Exit contract: 0 = LEAF_READY with an emitted canonical handoff,
//! 2 = deterministic non-green result, 3 = instrument failure.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::{Parser, ValueEnum};
use color_eyre::eyre::{Context, eyre};
use std::path::{Path, PathBuf};
use xtask::authority_transfer_closeout::{
    self as closeout, CloseoutOutcome, CloseoutResult, GitFacts,
};

#[derive(Debug, Parser)]
#[command(name = "authority-transfer-closeout")]
#[command(
    about = "Verify one bounded leaf candidate against its exact packets and emit the canonical handoff"
)]
struct Args {
    /// Closeout request document (authority_transfer_leaf_closeout_request.v1).
    #[arg(long, required_unless_present = "fixture")]
    request: Option<PathBuf>,

    /// Repository root used for git observation.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Ref resolved as the repository current-main subject.
    #[arg(long, default_value = "origin/main")]
    main_ref: String,

    /// Immutable offline regression fixture; runs without a repository.
    #[arg(long, conflicts_with = "request")]
    fixture: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let outcome = if let Some(fixture) = args.fixture.as_deref() {
        run_fixture(fixture)?
    } else {
        let request = args
            .request
            .as_deref()
            .ok_or_else(|| eyre!("a closeout request document is required"))?;
        closeout::evaluate_request_file(&args.repo, request, &args.main_ref)
            .map_err(|error| eyre!(error))?
    };
    match args.format {
        OutputFormat::Human => println!("{}", closeout::render_outcome_human(&outcome)),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            if let Some(handoff) = outcome.handoff.as_ref() {
                println!("{}", closeout::render_handoff_json(handoff)?);
            }
        }
    }
    std::process::exit(outcome.result.exit_code());
}

fn run_fixture(path: &Path) -> color_eyre::Result<CloseoutOutcome> {
    let raw = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("fixture {} is readable", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw).wrap_err("fixture parses as JSON")?;
    let expected: CloseoutResult = serde_json::from_value(value["expected_result"].clone())
        .wrap_err("fixture expectation deserializes into the closed vocabulary")?;
    let request: closeout::CloseoutRequest = serde_json::from_value(value["request"].clone())
        .wrap_err("fixture parses as a closeout request")?;
    let facts: GitFacts =
        serde_json::from_value(value["git_facts"].clone()).wrap_err("fixture git facts parse")?;
    let outcome = closeout::evaluate(&request, &facts);
    if outcome.result != expected {
        return Err(eyre!(
            "fixture {} expected {}, observed {}",
            path.display(),
            expected.as_str(),
            outcome.result.as_str()
        ));
    }
    Ok(outcome)
}
