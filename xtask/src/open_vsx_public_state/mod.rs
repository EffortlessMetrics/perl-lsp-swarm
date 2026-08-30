//! Read-only Open VSX public-state probe and receipt (`#9923`, incident `#9129`).
//!
//! This tool answers one question and refuses to answer any other: what is the
//! *current* public state of one exact Open VSX extension identity? It consumes
//! a bounded observation of six independent registry surfaces and emits a
//! durable `open_vsx_public_state.v1` receipt classifying that identity.
//!
//! Two properties are deliberate.
//!
//! **Nothing here can mutate a registry.** The sanctioned request set lives in
//! [`plan`]: `GET`-only, single-origin, credential-free. Classification refuses
//! any observation addressing a URL the plan did not derive, so the receipt and
//! the probe describe the same requests. No publisher credential is read,
//! accepted, or representable.
//!
//! **A failure to observe is never proof of absence.** Transport errors, budget
//! overruns, rate limits, schema drift and contradictory answers all resolve to
//! `provider_not_proven`. Reaching `extension_missing` requires three
//! independent affirmative `404`s while the namespace still resolves.
//!
//! The receipt is the domain truth. The process exit status is an operational
//! convenience for callers that want a non-zero signal when the identity is not
//! provably intact; it is not the classification.

mod classify;
mod model;
mod plan;

#[cfg(test)]
mod plan_binding_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use crate::receipt_output::{ensure_safe_output, prepare_output_parent, write_receipt};
use clap::Parser;
use classify::classify;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use model::{Observation, PublicState};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Label used in receipt-output diagnostics for this probe.
const SUBJECT: &str = "open vsx public state";

/// How far ahead of this machine's clock an observation may be dated.
const CLOCK_SKEW_ALLOWANCE_MINUTES: i64 = 5;

#[derive(Debug, Parser)]
#[command(about = "Classify a read-only Open VSX public-state observation")]
struct Args {
    /// Bounded read-only observation JSON.
    #[arg(long)]
    input: PathBuf,

    /// Receipt JSON, written for every classified state including blocking ones.
    #[arg(long, default_value = "target/receipts/open-vsx-public-state.json")]
    out: PathBuf,
}

pub fn run_from_env() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run_with_paths(args.input, args.out)
}

pub fn run_with_paths(input: PathBuf, out: PathBuf) -> Result<()> {
    let observation = load_observation(&input)?;
    prepare_output_parent(SUBJECT, &out)?;
    ensure_safe_output(SUBJECT, &out, &[input.as_path()])?;

    let receipt = classify(observation);
    // The receipt is persisted before any exit decision: a durable artifact must
    // exist for every state, and a write failure must never leave a caller with
    // a green summary and no evidence.
    write_receipt(SUBJECT, &out, &receipt)?;

    // Exit status answers "did this tool produce a trustworthy classification?",
    // not "is the extension healthy?" — the receipt is the domain truth, and
    // #9138 asks for the two to stay separate. Only `invalid` is a process
    // failure: there the observation itself could not be trusted, so there is no
    // classification to act on. Every other state is a real answer about the
    // registry, including the ones an operator will not enjoy reading.
    let identity = &receipt.identity.extension_id;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(
        handle,
        "open-vsx-public-state: {identity} classified {} ({} blocker(s)); see {}",
        receipt.state.key(),
        receipt.blockers.len(),
        out.display()
    )?;

    if receipt.state == PublicState::Invalid {
        bail!(
            "open-vsx-public-state: the observation for {identity} could not be trusted; see {}",
            out.display()
        );
    }
    Ok(())
}

/// The published input contract, applied to the document before classification.
const OBSERVATION_SCHEMA: &str =
    include_str!("../../../schemas/open_vsx_public_state_observation.v1.schema.json");

fn load_observation(path: &Path) -> Result<Observation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading Open VSX observation {}", path.display()))?;
    let document: serde_json::Value = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing Open VSX observation {}", path.display()))?;

    // Raised in review: deserialization alone enforces only what the Rust types
    // happen to encode, so every published constraint the classifier does not
    // restate — string lengths, status ranges, identity patterns — went
    // unchecked. Validating against the schema keeps one authority for the
    // input shape instead of two that can drift.
    let schema: serde_json::Value = serde_json::from_str(OBSERVATION_SCHEMA)
        .wrap_err("parsing the Open VSX observation contract")?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| eyre!("compiling the Open VSX observation contract: {error}"))?;
    let violations: Vec<String> =
        validator.iter_errors(&document).map(|error| format!("{error}")).collect();
    if !violations.is_empty() {
        bail!(
            "Open VSX observation {} does not conform to {}: {}",
            path.display(),
            "open_vsx_public_state_observation.v1",
            violations.join("; ")
        );
    }

    let observation: Observation = serde_json::from_value(document)
        .wrap_err_with(|| format!("parsing Open VSX observation {}", path.display()))?;

    // Temporal plausibility lives here, not in `classify`: reading a clock would
    // make classification non-deterministic, and determinism is a property the
    // receipt contract depends on. An instant in the future cannot describe a
    // probe that already ran.
    if let Ok(observed) = chrono::DateTime::parse_from_rfc3339(&observation.observed_at) {
        let skew = chrono::Utc::now() + chrono::Duration::minutes(CLOCK_SKEW_ALLOWANCE_MINUTES);
        if observed.with_timezone(&chrono::Utc) > skew {
            bail!(
                "Open VSX observation {} is dated {}, beyond the {CLOCK_SKEW_ALLOWANCE_MINUTES}-minute \
                 clock-skew allowance; it cannot describe a completed probe",
                path.display(),
                observation.observed_at
            );
        }
    }

    Ok(observation)
}
