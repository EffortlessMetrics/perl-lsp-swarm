#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Fail-closed admission for legacy upstream runner status observations.
//!
//! This is deliberately narrower than the final typed process contract in
//! #6884. It prevents the selected-evidence workflow from treating an absent
//! or nonzero legacy `harness_status` as authoritative merely because file and
//! assertion counts look green, and it creates verified admitted copies so
//! later consumers cannot silently reopen different report bytes.

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{HarnessMode, RUN_REPORT_SCHEMA_VERSION, RunReport};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "perl_core_harness.legacy_terminal_admission.v2";

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut options = Options::parse(std::env::args().skip(1))?;

    if let Some(receipt_path) = options.optional("--check-receipt")? {
        let expected = ExpectedIdentity::from_options(&mut options)?;
        options.finish()?;
        let receipt = read_receipt(Path::new(&receipt_path))?;
        verify_admitted_receipt(&receipt, &expected)?;
        tracing::info!("terminal admission receipt remains bound to its admitted report bytes");
        return Ok(());
    }

    let reports = options.repeated("--report").into_iter().map(PathBuf::from).collect::<Vec<_>>();
    let output = PathBuf::from(options.required("--output")?);
    let admitted_dir = PathBuf::from(options.required("--admitted-dir")?);
    let expected = ExpectedIdentity::from_options(&mut options)?;
    options.finish()?;

    if admitted_dir.exists() {
        bail!(
            "admitted report directory must be a fresh path and will not be replaced: {}",
            admitted_dir.display()
        );
    }

    let receipt = build_receipt(&reports, &expected, &admitted_dir)?;
    write_receipt(&output, &receipt)?;
    if receipt.verdict != AdmissionVerdict::AdmittedLegacyZero {
        bail!("runner terminal evidence is not proven; receipt written to {}", output.display());
    }
    verify_admitted_receipt(&receipt, &expected)?;
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
            let value =
                args.next().ok_or_else(|| color_eyre::eyre::eyre!("missing value for {flag}"))?;
            if value.starts_with("--") {
                bail!("missing value for {flag}; found option {value}");
            }
            values.entry(flag).or_default().push_back(value);
        }
        Ok(Self { values })
    }

    fn required(&mut self, flag: &str) -> Result<String> {
        self.optional(flag)?
            .ok_or_else(|| color_eyre::eyre::eyre!("required option {flag} was not supplied"))
    }

    fn optional(&mut self, flag: &str) -> Result<Option<String>> {
        let Some(mut values) = self.values.remove(flag) else {
            return Ok(None);
        };
        let value = values.pop_front();
        if !values.is_empty() {
            bail!("option {flag} may be supplied only once");
        }
        Ok(value)
    }

    fn repeated(&mut self, flag: &str) -> Vec<String> {
        self.values.remove(flag).map(|values| values.into_iter().collect()).unwrap_or_default()
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct ExpectedIdentity {
    runner: String,
    profile: String,
    commit: String,
    perl_ref: String,
}

impl ExpectedIdentity {
    fn from_options(options: &mut Options) -> Result<Self> {
        Ok(Self {
            runner: options.required("--expected-runner")?,
            profile: options.required("--expected-profile")?,
            commit: options.required("--expected-commit")?,
            perl_ref: options.required("--expected-perl-ref")?,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AdmissionVerdict {
    AdmittedLegacyZero,
    NotProven,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdmissionReceipt {
    schema_version: String,
    verdict: AdmissionVerdict,
    expected: ExpectedIdentity,
    identity_valid: bool,
    identity_errors: Vec<String>,
    reports: Vec<ReportAdmission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReportAdmission {
    source_label: String,
    admitted_path: Option<String>,
    sha256: String,
    run_report_schema: String,
    commit: String,
    perl_ref: String,
    prepared_tree: String,
    host_perl: String,
    runner: String,
    mode: String,
    profile: String,
    observed_legacy_status: Option<i32>,
    terminal_admitted: bool,
    reason: String,
}

struct ReportEvidence {
    admission: ReportAdmission,
    bytes: Vec<u8>,
}

fn build_receipt(
    paths: &[PathBuf],
    expected: &ExpectedIdentity,
    admitted_dir: &Path,
) -> Result<AdmissionReceipt> {
    let mut evidence = Vec::with_capacity(paths.len());
    for path in paths {
        evidence.push(read_report_evidence(path)?);
    }
    evidence.sort_by(|left, right| left.admission.mode.cmp(&right.admission.mode));

    let identity_errors = validate_report_identity(&evidence, expected);
    let identity_valid = identity_errors.is_empty();
    let terminal_valid = evidence.iter().all(|report| report.admission.terminal_admitted);
    let verdict = if identity_valid && terminal_valid {
        AdmissionVerdict::AdmittedLegacyZero
    } else {
        AdmissionVerdict::NotProven
    };

    if verdict == AdmissionVerdict::AdmittedLegacyZero {
        write_admitted_copies(&mut evidence, admitted_dir)?;
    }

    Ok(AdmissionReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        verdict,
        expected: expected.clone(),
        identity_valid,
        identity_errors,
        reports: evidence.into_iter().map(|report| report.admission).collect(),
    })
}

fn read_report_evidence(path: &Path) -> Result<ReportEvidence> {
    let bytes = fs::read(path).with_context(|| format!("reading run report {}", path.display()))?;
    let report: RunReport = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding run report {}", path.display()))?;
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!("{} uses unsupported run-report schema {}", path.display(), report.schema_version);
    }

    let (terminal_admitted, reason) = match report.harness_status {
        Some(0) => (
            true,
            "legacy zero exit observed; admitted only by the bounded interim policy".to_string(),
        ),
        Some(_) => (
            false,
            "nonzero legacy status has no reviewed runner/mode meaning; counts cannot override it"
                .to_string(),
        ),
        None => (
            false,
            "legacy report has no terminal status; process completion is not proven".to_string(),
        ),
    };

    Ok(ReportEvidence {
        admission: ReportAdmission {
            source_label: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<non-utf8-report>")
                .to_string(),
            admitted_path: None,
            sha256: sha256(&bytes),
            run_report_schema: report.schema_version,
            commit: report.commit,
            perl_ref: report.perl_ref,
            prepared_tree: report.prepared_tree,
            host_perl: report.host_perl,
            runner: report.runner.to_string(),
            mode: report.mode.to_string(),
            profile: report.profile.to_string(),
            observed_legacy_status: report.harness_status,
            terminal_admitted,
            reason,
        },
        bytes,
    })
}

fn validate_report_identity(
    reports: &[ReportEvidence],
    expected: &ExpectedIdentity,
) -> Vec<String> {
    let mut errors = Vec::new();
    if reports.len() != 2 {
        errors.push(format!(
            "selected evidence requires exactly two reports (parse and compile), found {}",
            reports.len()
        ));
    }

    let modes =
        reports.iter().map(|report| report.admission.mode.as_str()).collect::<BTreeSet<_>>();
    let expected_modes = [HarnessMode::Parse.to_string(), HarnessMode::Compile.to_string()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let actual_modes = modes.into_iter().map(ToString::to_string).collect::<BTreeSet<_>>();
    if actual_modes != expected_modes {
        errors.push(format!(
            "selected evidence requires one parse and one compile report, found {:?}",
            actual_modes
        ));
    }

    for report in reports {
        let admission = &report.admission;
        if admission.runner != expected.runner {
            errors.push(format!(
                "{} runner {} does not match expected {}",
                admission.mode, admission.runner, expected.runner
            ));
        }
        if admission.profile != expected.profile {
            errors.push(format!(
                "{} profile {} does not match expected {}",
                admission.mode, admission.profile, expected.profile
            ));
        }
        if admission.commit != expected.commit {
            errors.push(format!(
                "{} commit does not match the measured repository commit",
                admission.mode
            ));
        }
        if admission.perl_ref != expected.perl_ref {
            errors.push(format!(
                "{} Perl ref {} does not match expected {}",
                admission.mode, admission.perl_ref, expected.perl_ref
            ));
        }
    }

    if let Some(first) = reports.first() {
        for report in reports.iter().skip(1) {
            if report.admission.prepared_tree != first.admission.prepared_tree {
                errors.push("parse and compile reports use different prepared trees".to_string());
            }
            if report.admission.host_perl != first.admission.host_perl {
                errors.push(
                    "parse and compile reports use different host Perl identities".to_string(),
                );
            }
        }
    }

    errors.sort();
    errors.dedup();
    errors
}

fn write_admitted_copies(evidence: &mut [ReportEvidence], admitted_dir: &Path) -> Result<()> {
    let parent = admitted_dir.parent().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "admitted report directory has no parent: {}",
            admitted_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating admitted report parent {}", parent.display()))?;
    fs::create_dir(admitted_dir).with_context(|| {
        format!("creating fresh admitted report directory {}", admitted_dir.display())
    })?;

    for report in evidence {
        let destination = admitted_dir.join(format!("{}.json", report.admission.mode));
        fs::write(&destination, &report.bytes)
            .with_context(|| format!("writing admitted report {}", destination.display()))?;
        let mut permissions = fs::metadata(&destination)
            .with_context(|| format!("reading admitted report {}", destination.display()))?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&destination, permissions).with_context(|| {
            format!("making admitted report read-only {}", destination.display())
        })?;
        report.admission.admitted_path = Some(destination.to_string_lossy().replace('\\', "/"));
    }
    Ok(())
}

fn verify_admitted_receipt(receipt: &AdmissionReceipt, expected: &ExpectedIdentity) -> Result<()> {
    if receipt.schema_version != SCHEMA_VERSION {
        bail!("unsupported terminal-admission schema {}", receipt.schema_version);
    }
    if &receipt.expected != expected {
        bail!("terminal-admission receipt expected identity does not match the verifier input");
    }

    let mut recomputed = Vec::with_capacity(receipt.reports.len());
    let mut recorded_by_mode = BTreeMap::new();
    let mut admitted_paths = BTreeSet::new();
    for recorded in &receipt.reports {
        if recorded_by_mode.insert(recorded.mode.clone(), recorded).is_some() {
            bail!("terminal-admission receipt contains duplicate mode {}", recorded.mode);
        }
        let path = recorded
            .admitted_path
            .as_deref()
            .ok_or_else(|| color_eyre::eyre::eyre!("admitted report has no admitted path"))?;
        if !admitted_paths.insert(path.to_string()) {
            bail!("terminal-admission receipt reuses admitted path {path}");
        }
        let metadata = fs::metadata(path)
            .with_context(|| format!("reading admitted report metadata {path}"))?;
        if !metadata.is_file() {
            bail!("admitted report is not a regular file: {path}");
        }
        if !metadata.permissions().readonly() {
            bail!("admitted report is writable: {path}");
        }
        let mut evidence = read_report_evidence(Path::new(path))?;
        evidence.admission.admitted_path = Some(path.to_string());
        recomputed.push(evidence);
    }
    recomputed.sort_by(|left, right| left.admission.mode.cmp(&right.admission.mode));

    let identity_errors = validate_report_identity(&recomputed, expected);
    if !identity_errors.is_empty() {
        bail!("admitted terminal receipt identity is invalid:\n{}", identity_errors.join("\n"));
    }
    if recomputed.iter().any(|report| !report.admission.terminal_admitted) {
        bail!("admitted terminal receipt contains a non-admitted terminal outcome");
    }

    for evidence in &recomputed {
        let recorded = recorded_by_mode.get(&evidence.admission.mode).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "receipt has no recorded admission for mode {}",
                evidence.admission.mode
            )
        })?;
        if evidence.admission.sha256 != recorded.sha256
            || evidence.admission.run_report_schema != recorded.run_report_schema
            || evidence.admission.commit != recorded.commit
            || evidence.admission.perl_ref != recorded.perl_ref
            || evidence.admission.prepared_tree != recorded.prepared_tree
            || evidence.admission.host_perl != recorded.host_perl
            || evidence.admission.runner != recorded.runner
            || evidence.admission.mode != recorded.mode
            || evidence.admission.profile != recorded.profile
            || evidence.admission.observed_legacy_status != recorded.observed_legacy_status
            || evidence.admission.terminal_admitted != recorded.terminal_admitted
        {
            bail!(
                "admitted report differs from the recorded receipt for mode {}",
                evidence.admission.mode
            );
        }
    }

    if receipt.verdict != AdmissionVerdict::AdmittedLegacyZero
        || !receipt.identity_valid
        || !receipt.identity_errors.is_empty()
    {
        bail!(
            "terminal-admission receipt metadata does not describe the recomputed admitted state"
        );
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    // `digest`'s array type does not implement `LowerHex`; render by hand with
    // the same idiom as the crate's internal `hex_lower` helper.
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

fn read_receipt(path: &Path) -> Result<AdmissionReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading terminal receipt {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding terminal receipt {}", path.display()))
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
    use perl_core_harness_types::{HarnessProfile, HarnessRunner, RunSummary};

    type TestResult = Result<()>;

    #[test]
    fn exact_parse_compile_pair_is_copied_and_verifiable() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let expected = expected_identity();
        let receipt = build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        assert_eq!(receipt.verdict, AdmissionVerdict::AdmittedLegacyZero);
        assert!(receipt.identity_valid);
        verify_admitted_receipt(&receipt, &expected)?;
        Ok(())
    }

    #[test]
    fn zero_status_with_product_failure_remains_terminally_admissible() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), true)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), true)?;
        let expected = expected_identity();
        let receipt = build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        assert_eq!(receipt.verdict, AdmissionVerdict::AdmittedLegacyZero);
        verify_admitted_receipt(&receipt, &expected)?;
        Ok(())
    }

    #[test]
    fn nonzero_all_pass_report_is_not_proven() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let status_255 =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(255), false)?;
        let receipt = build_receipt(
            &[parse, status_255],
            &expected_identity(),
            &temp.path().join("admitted"),
        )?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert!(receipt.reports.iter().any(|report| {
            !report.terminal_admitted && report.reason.contains("counts cannot override")
        }));
        assert!(!temp.path().join("admitted").exists());
        Ok(())
    }

    #[test]
    fn duplicate_compile_reports_do_not_satisfy_identity() -> TestResult {
        let temp = tempfile::tempdir()?;
        let first = write_report(temp.path(), "one.json", HarnessMode::Compile, Some(0), false)?;
        let second = write_report(temp.path(), "two.json", HarnessMode::Compile, Some(0), false)?;
        let receipt =
            build_receipt(&[first, second], &expected_identity(), &temp.path().join("admitted"))?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert!(!receipt.identity_valid);
        assert!(receipt.identity_errors.iter().any(|error| error.contains("one parse")));
        Ok(())
    }

    #[test]
    fn unexpected_profile_or_runner_blocks_admission() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let mut expected = expected_identity();
        expected.profile = "comp".to_string();
        expected.runner = "harness".to_string();
        let receipt = build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        assert_eq!(receipt.verdict, AdmissionVerdict::NotProven);
        assert!(receipt.identity_errors.iter().any(|error| error.contains("runner")));
        assert!(receipt.identity_errors.iter().any(|error| error.contains("profile")));
        Ok(())
    }

    #[test]
    fn existing_admitted_directory_is_never_replaced() -> TestResult {
        let temp = tempfile::tempdir()?;
        let admitted = temp.path().join("admitted");
        fs::create_dir(&admitted)?;
        fs::write(admitted.join("sentinel"), "keep")?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let mut evidence = vec![read_report_evidence(&parse)?, read_report_evidence(&compile)?];
        assert!(write_admitted_copies(&mut evidence, &admitted).is_err());
        assert_eq!(fs::read_to_string(admitted.join("sentinel"))?, "keep");
        Ok(())
    }

    #[test]
    fn replacing_an_admitted_copy_breaks_verification() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let expected = expected_identity();
        let receipt = build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        let compile_path = receipt
            .reports
            .iter()
            .find(|report| report.mode == HarnessMode::Compile.to_string())
            .and_then(|report| report.admitted_path.as_deref())
            .ok_or_else(|| color_eyre::eyre::eyre!("compile admitted path missing"))?;
        make_writable(Path::new(compile_path))?;
        fs::write(compile_path, b"{}\n")?;
        assert!(verify_admitted_receipt(&receipt, &expected).is_err());
        Ok(())
    }

    #[test]
    fn recycled_valid_report_with_inflated_counts_breaks_verification() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let expected = expected_identity();
        let receipt = build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        let compile_path = receipt
            .reports
            .iter()
            .find(|report| report.mode == HarnessMode::Compile.to_string())
            .and_then(|report| report.admitted_path.as_deref())
            .ok_or_else(|| color_eyre::eyre::eyre!("compile admitted path missing"))?;

        // Swap in a valid, schema-conformant report carrying the same identity
        // but inflated counts, as if recycled from another run of the same
        // subject. Read-only is restored so verification passes every gate up
        // to the recorded-vs-recomputed comparison.
        let mut recycled: RunReport = serde_json::from_str(&fs::read_to_string(compile_path)?)?;
        recycled.summary.files_total = 7;
        recycled.summary.files_passed = 7;
        recycled.summary.tap_assertions_total = 7;
        recycled.summary.tap_assertions_passed = 7;
        make_writable(Path::new(compile_path))?;
        fs::write(compile_path, format!("{}\n", serde_json::to_string_pretty(&recycled)?))?;
        make_readonly(Path::new(compile_path))?;

        let error = match verify_admitted_receipt(&receipt, &expected) {
            Ok(()) => {
                return Err(color_eyre::eyre::eyre!(
                    "a recycled admitted copy with different counts was accepted"
                ));
            }
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("admitted report differs from the recorded receipt"),
            "verification must fail at the recorded-vs-recomputed comparison: {error}"
        );
        Ok(())
    }

    #[test]
    fn serialized_identity_flags_cannot_override_recomputed_identity() -> TestResult {
        let temp = tempfile::tempdir()?;
        let parse = write_report(temp.path(), "parse.json", HarnessMode::Parse, Some(0), false)?;
        let compile =
            write_report(temp.path(), "compile.json", HarnessMode::Compile, Some(0), false)?;
        let expected = expected_identity();
        let mut receipt =
            build_receipt(&[parse, compile], &expected, &temp.path().join("admitted"))?;
        receipt.expected.profile = "comp".to_string();
        receipt.identity_valid = true;
        receipt.identity_errors.clear();
        assert!(verify_admitted_receipt(&receipt, &expected).is_err());
        Ok(())
    }

    #[cfg(unix)]
    fn make_writable(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn make_writable(path: &Path) -> Result<()> {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }

    fn make_readonly(path: &Path) -> Result<()> {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }

    fn expected_identity() -> ExpectedIdentity {
        ExpectedIdentity {
            runner: HarnessRunner::Test.to_string(),
            profile: HarnessProfile::Base.to_string(),
            commit: "a".repeat(40),
            perl_ref: "perl-5.42.2".to_string(),
        }
    }

    fn write_report(
        directory: &Path,
        name: &str,
        mode: HarnessMode,
        status: Option<i32>,
        product_failure: bool,
    ) -> Result<PathBuf> {
        let path = directory.join(name);
        let (files_passed, files_failed) = if product_failure { (0, 1) } else { (1, 0) };
        let report = RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.to_string(),
            commit: "a".repeat(40),
            timestamp: "2026-08-12T00:00:00Z".to_string(),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree: "<prepared-tree>".to_string(),
            run_tree: format!("<run-tree-{mode}>"),
            host_perl: "perl".to_string(),
            runner: HarnessRunner::Test,
            mode,
            profile: HarnessProfile::Base,
            harness_status: status,
            summary: RunSummary {
                files_total: 1,
                files_passed,
                files_failed,
                tap_assertions_total: 1,
                tap_assertions_passed: files_passed,
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
