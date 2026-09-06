//! Diagnostic entry point for the release-candidate security contract.
//!
//! Validates one `release_candidate_security.v1` document against the closed
//! schema and fail-closed rail inventory (#9427). It runs no scanner.

#![allow(clippy::print_stdout)]

#[path = "../tasks/candidate_security_contract.rs"]
mod candidate_security_contract;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    candidate_security_contract::run_cli()
}
