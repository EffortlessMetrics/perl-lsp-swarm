//! `cargo xtask metrics sweep-stats` — summarize a parser corpus sweep receipt.
//!
//! Reads the JSON produced by `cargo xtask parser-corpus-sweep --receipt`
//! (integer envelope version — receipts may simply omit the phase-timings
//! and slowest-file sections) and prints the same human-readable report
//! that the sweep itself emits at the end of a live run.
//!
//! Useful for analyzing historical receipts, comparing sweeps across
//! commits, or inspecting slowest-file and median-error-density data
//! without re-running the full sweep.

use crate::tasks::parser_corpus_sweep::{
    SCHEMA_VERSION, SweepReport, print_summary, validate_schema_version,
};
use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

/// Default receipt path for the system-Perl corpus sweep.
const DEFAULT_RECEIPT: &str = "target/receipts/system-corpus-sweep.json";

/// Reject `SweepReport` envelopes whose `schema_version` is not the shared
/// integer envelope version.
///
/// Ruling on #15990: the envelope version is a plain integer, so legacy
/// string versions (`"1.3.0"` and older) no longer deserialize at all —
/// they are refused by the typed envelope before this check runs. Any
/// future integer bump must be audited before this consumer accepts it.
fn assert_supported_sweep_version(report: &SweepReport) -> Result<()> {
    if validate_schema_version(report.schema_version).is_ok() {
        return Ok(());
    }
    bail!(
        "unsupported sweep-report schema_version: expected {SCHEMA_VERSION}, found {}. \
         Re-run `cargo xtask parser-corpus-sweep --receipt` to produce a compatible \
         receipt, or audit the new producer shape in \
         xtask/src/tasks/parser_corpus_sweep.rs before accepting it here.",
        report.schema_version,
    )
}

/// Entry point for `cargo xtask metrics sweep-stats`.
pub fn run(input: Option<PathBuf>) -> Result<()> {
    let path = match input {
        Some(p) => p,
        None => project_root()?.join(DEFAULT_RECEIPT),
    };

    if !path.exists() {
        return Err(color_eyre::eyre::eyre!(
            "sweep receipt not found: {}\n  Run `cargo xtask parser-corpus-sweep --receipt` first, \
             or pass `--input <path>` to point at a different receipt.",
            path.display()
        ));
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("reading sweep receipt: {}", path.display()))?;
    let report: SweepReport = serde_json::from_str(&raw)
        .with_context(|| format!("parsing sweep receipt: {}", path.display()))?;
    assert_supported_sweep_version(&report)?;

    println!("Receipt: {}", path.display());
    println!("Commit:  {}", report.commit);
    println!("Profile: {}", report.corpus_profile);
    println!("Perl:    {}", report.perl_version);

    print_summary(&report);

    // Schema-compatibility note: receipts without the phase-timings section
    // deserialize with None phase timings and empty slowest_files. Tell the
    // user explicitly so they know the missing sections are expected, not a
    // bug.
    if report.phase_timings.is_none() {
        println!(
            "\n(Note: receipt predates the phase-timings section — phase timings and \
             slowest-file list are not available. Re-run \
             `cargo xtask parser-corpus-sweep --receipt` to produce a \
             current-schema receipt.)",
        );
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_run_errors_on_missing_input() {
        let tmp = TempDir::new().expect("tempdir");
        let missing = tmp.path().join("nonexistent.json");
        let err = run(Some(missing)).unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "missing-receipt error must mention 'not found', got: {err}"
        );
    }

    #[test]
    fn test_run_reads_and_parses_receipt() -> Result<()> {
        // Minimal but valid current-schema receipt.
        let json = r#"{
            "schema_version": 1,
            "commit": "abcdef1",
            "timestamp": "2026-04-15T00:00:00Z",
            "corpus_profile": "system",
            "corpus_roots": ["/usr/share/perl"],
            "resolved_roots_count": 1,
            "perl_version": "5.38",
            "total_files": 3,
            "files_unreadable": 0,
            "clean_files": 2,
            "files_with_errors": 1,
            "total_error_nodes": 1,
            "first_error_buckets": {"unclosed_brace": 1},
            "elapsed_secs": 0.05,
            "phase_timings": {
                "discovery_ms": 10,
                "file_io_ms": 5,
                "parse_ms": 20,
                "total_ms": 50
            },
            "median_error_density_per_1k_loc": 1.23,
            "slowest_files": [
                {"path": "slow.pm", "parse_duration_ms": 7, "line_count": 300}
            ]
        }"#;
        let tmp = TempDir::new().expect("tempdir");
        let receipt = tmp.path().join("receipt.json");
        fs::write(&receipt, json)?;

        // If parsing or printing failed this would surface as Err here.
        run(Some(receipt))?;
        Ok(())
    }

    #[test]
    fn test_run_tolerates_receipt_without_optional_sections() -> Result<()> {
        // Current-schema receipt: no phase_timings / slowest_files / density.
        // The optional sections deserialize as None / empty defaults.
        let json = r#"{
            "schema_version": 1,
            "commit": "old",
            "timestamp": "2026-04-01T00:00:00Z",
            "corpus_profile": "system",
            "corpus_roots": [],
            "resolved_roots_count": 0,
            "perl_version": "5.38",
            "total_files": 0,
            "files_unreadable": 0,
            "clean_files": 0,
            "files_with_errors": 0,
            "total_error_nodes": 0,
            "first_error_buckets": {},
            "elapsed_secs": 0.0
        }"#;
        let tmp = TempDir::new().expect("tempdir");
        let receipt = tmp.path().join("old.json");
        fs::write(&receipt, json)?;
        run(Some(receipt))?;
        Ok(())
    }

    #[test]
    fn test_run_refuses_legacy_string_schema_version() {
        // Legacy string envelope ("1.0.0"): must not deserialize into the
        // integer envelope version — the consumer fails closed (#15361)
        // instead of silently summarizing a pre-ruling receipt.
        let json = r#"{
            "schema_version": "1.0.0",
            "commit": "ancient",
            "timestamp": "2026-01-01T00:00:00Z",
            "corpus_roots": ["/usr/share/perl"],
            "total_files": 1,
            "files_unreadable": 0,
            "clean_files": 1,
            "files_with_errors": 0,
            "total_error_nodes": 0,
            "first_error_buckets": {},
            "elapsed_secs": 0.0
        }"#;
        let tmp = TempDir::new().expect("tempdir");
        let receipt = tmp.path().join("legacy.json");
        fs::write(&receipt, json).expect("write legacy receipt");

        let err = run(Some(receipt)).expect_err("legacy string version must be refused");
        let msg = err.to_string();
        assert!(
            msg.contains("parsing sweep receipt"),
            "legacy string envelope must fail at envelope parsing, got: {msg}"
        );
    }

    #[test]
    fn test_run_rejects_unsupported_schema_version() {
        // Version 9 receipt: structurally valid SweepReport, but the integer
        // version is not the audited envelope version. The consumer must bail
        // before print_summary rather than silently summarize a future-evolved
        // envelope.
        let json = r#"{
            "schema_version": 9,
            "commit": "future",
            "timestamp": "2030-01-01T00:00:00Z",
            "corpus_profile": "system",
            "corpus_roots": [],
            "resolved_roots_count": 0,
            "perl_version": "5.38",
            "total_files": 0,
            "files_unreadable": 0,
            "clean_files": 0,
            "files_with_errors": 0,
            "total_error_nodes": 0,
            "first_error_buckets": {},
            "elapsed_secs": 0.0
        }"#;
        let tmp = TempDir::new().expect("tempdir");
        let receipt = tmp.path().join("future.json");
        fs::write(&receipt, json).expect("write future receipt");

        let err = run(Some(receipt)).expect_err("unsupported version must bail");
        let msg = err.to_string();
        assert!(
            msg.contains("unsupported sweep-report schema_version"),
            "error must call out the unsupported version, got: {msg}"
        );
        assert!(
            msg.contains("expected 1"),
            "error must name the expected version so the user knows what to do, got: {msg}"
        );
        assert!(msg.contains("found 9"), "error must include the offending version, got: {msg}");
    }
}
