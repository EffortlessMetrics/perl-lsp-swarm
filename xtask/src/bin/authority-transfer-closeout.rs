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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Absolute path of a repository-root-relative closeout fixture document.
    fn fixture(relative: &str) -> color_eyre::Result<PathBuf> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root =
            manifest_dir.parent().ok_or_else(|| eyre!("xtask manifest has a repository parent"))?;
        Ok(root.join(closeout::FIXTURE_DIR).join(relative))
    }

    /// The mismatch branch must fail closed and reveal both sides of the
    /// comparison — the declared expectation and the observed result — so a
    /// drifted fixture is diagnosed, not just rejected.
    #[test]
    fn expectation_mismatch_reveals_expected_and_observed() -> color_eyre::Result<()> {
        let valid = fixture("valid/leaf_ready_offline.v1.json")?;
        let raw = std::fs::read_to_string(&valid)
            .wrap_err_with(|| format!("fixture {} is readable", valid.display()))?;
        let mut document: serde_json::Value =
            serde_json::from_str(&raw).wrap_err("fixture parses as JSON")?;
        document["expected_result"] = serde_json::Value::String("WRONG_SUBJECT".to_string());

        let workspace = tempfile::tempdir().wrap_err("temp workspace creates")?;
        let path = workspace.path().join("mismatched-expectation.v1.json");
        std::fs::write(&path, serde_json::to_string(&document).wrap_err("document serializes")?)
            .wrap_err("mismatched fixture writes")?;

        let error = match run_fixture(&path) {
            Ok(outcome) => {
                return Err(eyre!(
                    "mismatched expectation must fail, observed {}",
                    outcome.result.as_str()
                ));
            }
            Err(error) => error,
        };
        let rendered = format!("{error}");
        assert!(
            rendered.contains("expected WRONG_SUBJECT, observed LEAF_READY"),
            "mismatch must reveal both sides: {rendered}"
        );
        assert!(
            rendered.contains(&path.display().to_string()),
            "reveal names the fixture: {rendered}"
        );
        Ok(())
    }

    /// The same document with the true expectation passes: the mismatch test
    /// above fails on the comparison itself, not on parsing or evaluation.
    #[test]
    fn matching_expectation_is_accepted() -> color_eyre::Result<()> {
        let valid = fixture("valid/leaf_ready_offline.v1.json")?;
        let outcome =
            run_fixture(&valid).map_err(|error| eyre!("valid fixture passes: {error}"))?;
        assert_eq!(outcome.result, closeout::CloseoutResult::LeafReady);
        Ok(())
    }
}
