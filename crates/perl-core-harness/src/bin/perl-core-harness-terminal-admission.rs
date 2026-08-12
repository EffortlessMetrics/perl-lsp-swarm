#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Fail-closed admission for legacy upstream runner status observations.
//!
//! This is deliberately narrower than the final typed process contract in
//! #6884. It prevents the selected-evidence workflow from treating an absent
//! or nonzero legacy `harness_status` as authoritative merely because file and
//! assertion counts look green.

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{RUN_REPORT_SCHEMA_VERSION, RunReport};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "perl_core_harness.legacy_terminal_admission.v1";

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut options = Options::parse(std::env::args().skip(1))?;
    let reports = options
        .repeated("--report")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if reports.is_empty() {
        bail!("at least one --report is required");
    }
    let output = PathBuf::from(options.required("--output")?);
    options.finish()?;

    let receipt = build_receipt(&reports)?;
    write_receipt(&output, &receipt)?;
    if receipt.verdict != AdmissionVerdict::AdmittedLegacyZero {
        bail!(
            "runner terminal evidence is not proven; receipt written to {}",
            output.display()
        );
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Options {
    values: BTreeMap<String, VecDeque<String>>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut values = BTreeMap::<String, VecDeque<String>>::new();
        let mut args = args.peekable();
        while let Some(flag) = args.next() {
            if !flag.starts_with("--") {
                bail!("expected an option beginning with --, found {flag}");
            }
            let value = args
                .next()
                .ok_or_else(|| color_eyre::eyre::eyre!("missing value for {flag}"))?;
            if value.starts_with("--") {
                bail!("missing value for {flag}; found option {value}");
            }
            values.entry(flag).or_default().push_back(value);
        }
        Ok(Self { values })
    }

    fn required(&mut self, flag: &str) -> Result<String> {
        let value = self
            .values
            .get_mut(flag)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| color_eyre::eyre::eyre!("required option {flag} was not supplied"))?;
        if self.values.get(flag).is_some_and(|values| !values.is_empty()) {
            bail!("option {flag} may be supplied only once");
        }
        self.values.remove(flag);
        Ok(value)
    }

    fn repeated(&mut self, flag: &str) -> Vec<String> {
        self.values
            .remove(flag)
            .map(|values| values.into_iter().collect())
            .unwrap_or_default()
    }

    fn finish(self) -> Result<()> {
        if self.values.is_empty() {
            return Ok(());
        }
        bail!(
            "unrecognized option(s): {}",
            self.values.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AdmissionVerdict {
    AdmittedLegacyZero,
    NotProven,
}

#[derive(Debug, Serialize)]
struct AdmissionReceipt {
    schema_version: &'static str,
    verdict: AdmissionVerdict,
    reports: Vec<ReportAdmission>,
}

#[derive(Debug, Serialize)]
struct ReportAdmission {
    path: String,
    sha256: String,
    run_report_schema: String,
    runner: String,
    mode: String,
    profile: String,
    observed_legacy_status: Option<i32>,
    admitted: bool,
    reason: &'static str,
}

fn build_receipt(paths: &[PathBuf]) -> Result<AdmissionReceipt> {
    let mut reports = Vec::with_capacity(paths.len());
    for path in paths {
        reports.push(read_report_admission(path)?);
    }
    reports.sort_by(|left, right| left.path.cmp(&right.path));
    let admitted = reports.iter().all(|report| report.admitted);
    Ok(AdmissionReceipt {
        schema_version: SCHEMA_VERSION,
        verdict: if admitted {
            AdmissionVerdict::AdmittedLegacyZero
        } else {
            AdmissionVerdict::NotProven
        },
        reports,
    })
}

fn read_report_admission(path: &Path) -> Result<ReportAdmission> {
    let bytes = fs::read(path).with_context(|| format!("reading run report {}", path.display()))?;
    let report: RunReport = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding run report {}", path.display()))?;
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported run-report schema {}",
            path.display(),
            report.schema_version
        );
    }

    let (admitted, reason) = match report.harness_status {
        Some(0) => (
            true,
            "legacy zero exit observed; admitted only by the bounded interim policy",
        ),
        Some(_) => (
            false,
            "nonzero legacy status has no reviewed runner/mode meaning; counts cannot override it",
        ),
        None => (
            false,
            "legacy report has no terminal status; process completion is not proven",
        ),
    };

    Ok(ReportAdmission {
        path: path.to_string_lossy().replace('\\', "/"),
        sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        run_report_schema: report.schema_version,
        runner: report.runner.to_string(),
        mode: report.mode.to_string(),
        profile: report.profile.to_string(),
        observed_legacy_status: report.harness_status,
        admitted,
        reason,
    })
}

fn write_receipt(path: &Path, receipt: &AdmissionReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let encoded = serde_json::to_string_pretty(receipt).context("serializing terminal receipt")?;
    fs::write(path, format!("{encoded}\n"))
        .with_context(|| format!("writing terminal receipt {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_core_harness_types::{
        HarnessMode, HarnessProfile, HarnessRunner, RunSummary,
    };

    type TestResult = Result<()>;

    #[test]
    fn zero_status_is_the_only_interim_admitted_state() -> TestResult {
        let temp = tempfile::tempdir()?;
        let zero = write_report(temp.path(), "zero.json", Some(0))?;
        let receipt = build_receipt(&[zero])?;
        assert_eq!(receipt.verdict, AdmissionVerdict::AdmittedLegacyZero);
        assert!(receipt.reports[0].admitted);
        Ok(())
    }

    #[test]
    fn nonzero_all_pass_report_is_not_proven() -> TestResult {
        let temp = tempfile::tempdir()?;
        let status_255 = write_report(temp.path(), "status-255.json", Some(255))?;
        let receipt = build_receipt(&[status_255])?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert!(!receipt.reports[0].admitted);
        assert!(receipt.reports[0].reason.contains("counts cannot override"));
        Ok(())
    }

    #[test]
    fn missing_status_is_not_proven() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = write_report(temp.path(), "missing.json", None)?;
        let receipt = build_receipt(&[missing])?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert!(!receipt.reports[0].admitted);
        Ok(())
    }

    #[test]
    fn one_invalid_report_blocks_the_combined_receipt() -> TestResult {
        let temp = tempfile::tempdir()?;
        let zero = write_report(temp.path(), "parse.json", Some(0))?;
        let nonzero = write_report(temp.path(), "compile.json", Some(7))?;
        let receipt = build_receipt(&[zero, nonzero])?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert_eq!(receipt.reports.len(), 2);
        Ok(())
    }

    fn write_report(directory: &Path, name: &str, status: Option<i32>) -> Result<PathBuf> {
        let path = directory.join(name);
        let report = RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.to_string(),
            commit: "a".repeat(40),
            timestamp: "2026-08-12T00:00:00Z".to_string(),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree: "<prepared-tree>".to_string(),
            run_tree: "<run-tree>".to_string(),
            host_perl: "perl".to_string(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            harness_status: status,
            summary: RunSummary {
                files_total: 1,
                files_passed: 1,
                files_failed: 0,
                tap_assertions_total: 1,
                tap_assertions_passed: 1,
            },
            buckets: BTreeMap::new(),
            file_results: Vec::new(),
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        };
        fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&report)?))?;
        Ok(path)
    }
}
