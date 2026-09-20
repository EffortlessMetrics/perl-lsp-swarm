//! Fail-closed host-path validation for public Perl core harness receipts.
//!
//! The harness receipt directory is uploaded as a workflow artifact on every
//! run, so anything it contains is published. This gate scans every public JSON
//! and JSONL receipt with the shared structural classifier and fails when a
//! host path is embedded anywhere in the payload.
//!
//! `--quarantine` additionally deletes the rejected files, so an always-on
//! upload step cannot publish a payload this gate has already rejected.

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::public_evidence::{Finding, PublicStringClass, scan_public_value};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    color_eyre::install()?;
    let options = Options::parse(std::env::args().skip(1))?;
    let report = validate_public_evidence(&options.evidence_dir)?;

    if report.findings.is_empty() {
        tracing::info!(
            "public evidence host-path validation passed for {} ({} file(s) scanned)",
            options.evidence_dir.display(),
            report.scanned
        );
        return Ok(());
    }

    if options.quarantine {
        for path in &report.rejected_files {
            fs::remove_file(path)
                .with_context(|| format!("quarantining rejected receipt {}", path.display()))?;
        }
        tracing::warn!(
            "quarantined {} rejected receipt file(s) before publication",
            report.rejected_files.len()
        );
    }

    let rendered = report.findings.iter().map(Finding::render).collect::<Vec<_>>().join("\n");
    bail!(
        "public evidence contains {} embedded host-path finding(s); private values were not echoed:\n{}",
        report.findings.len(),
        rendered
    );
}

struct Options {
    evidence_dir: PathBuf,
    quarantine: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        const USAGE: &str =
            "usage: perl-core-harness-public-evidence --evidence-dir <directory> [--quarantine]";
        let mut evidence_dir = None;
        let mut quarantine = false;
        let mut args = args.peekable();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--evidence-dir" => {
                    let value = args.next().ok_or_else(|| {
                        color_eyre::eyre::eyre!("missing value for --evidence-dir")
                    })?;
                    evidence_dir = Some(PathBuf::from(value));
                }
                "--quarantine" => quarantine = true,
                other => bail!("unexpected argument {other}\n{USAGE}"),
            }
        }
        let evidence_dir = evidence_dir.ok_or_else(|| color_eyre::eyre::eyre!("{USAGE}"))?;
        Ok(Self { evidence_dir, quarantine })
    }
}

#[derive(Default)]
struct Report {
    findings: Vec<Finding>,
    rejected_files: Vec<PathBuf>,
    scanned: usize,
}

fn validate_public_evidence(evidence_dir: &Path) -> Result<Report> {
    // An absent receipt directory means an earlier step produced nothing to
    // publish. There is no payload to leak, so this is a pass rather than a
    // failure attributed to this gate.
    if !evidence_dir.exists() {
        return Ok(Report::default());
    }
    let root = fs::canonicalize(evidence_dir)
        .with_context(|| format!("canonicalizing evidence directory {}", evidence_dir.display()))?;
    if !root.is_dir() {
        bail!("evidence path is not a directory: {}", root.display());
    }

    let mut files = Vec::new();
    collect_public_files(&root, &mut files)?;
    files.sort();
    files.dedup();

    let mut report = Report::default();
    for path in files {
        report.scanned += 1;
        let before = report.findings.len();
        scan_public_file(&root, &path, &mut report.findings)?;
        if report.findings.len() > before {
            report.rejected_files.push(path);
        }
    }
    report.findings.sort();
    report.findings.dedup();
    Ok(report)
}

fn collect_public_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("reading evidence directory {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_public_files(&path, files)?;
        } else if file_type.is_file()
            && matches!(path.extension().and_then(|value| value.to_str()), Some("json" | "jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn scan_public_file(root: &Path, path: &Path, findings: &mut Vec<Finding>) -> Result<()> {
    let logical_file = path
        .strip_prefix(root)
        .with_context(|| format!("normalizing evidence path {}", path.display()))?
        .to_string_lossy()
        .replace('\\', "/");
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading public evidence {}", path.display()))?;

    if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
        for (line_index, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).with_context(|| {
                format!("decoding JSONL line {} in {}", line_index + 1, path.display())
            })?;
            scan_public_value(
                &value,
                &logical_file,
                &format!("/line/{}", line_index + 1),
                PublicStringClass::Ordinary,
                findings,
            );
        }
    } else {
        let value: Value = serde_json::from_str(&raw)
            .with_context(|| format!("decoding public JSON evidence {}", path.display()))?;
        scan_public_value(&value, &logical_file, "", PublicStringClass::Ordinary, findings);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<()>;

    fn write(path: &Path, contents: &str) -> TestResult {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    #[test]
    fn accepts_receipts_that_carry_only_repository_relative_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("perl-core");
        write(
            &root.join("smoke/base/smoke.json"),
            r#"{"prepared_tree":"target/perl-core/smoke/base/perl5","host_perl":"perl"}"#,
        )?;

        let report = validate_public_evidence(&root)?;
        assert!(report.findings.is_empty(), "unexpected findings: {:?}", report.findings);
        assert_eq!(report.scanned, 1);
        Ok(())
    }

    #[test]
    fn rejects_embedded_host_paths_in_json_and_jsonl_without_echoing_them() -> TestResult {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("perl-core");
        write(&root.join("smoke/base/smoke.json"), r#"{"path":"crates/perl-parser/src/lib.rs"}"#)?;
        write(
            &root.join("smoke/base/records.jsonl"),
            "{\"argument\":\"--root=/tmp/private/build\"}\n",
        )?;

        let report = validate_public_evidence(&root)?;
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].pointer, "/line/1/argument");
        assert_eq!(report.findings[0].logical_file, "smoke/base/records.jsonl");
        assert!(!report.findings[0].render().contains("/tmp/private/build"));
        assert_eq!(report.rejected_files.len(), 1);
        Ok(())
    }

    /// The upload step is `if: always()`, so failing the job is not enough:
    /// the rejected payload must be gone before the artifact is collected.
    #[test]
    fn quarantine_removes_only_the_rejected_receipt() -> TestResult {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("perl-core");
        let clean = root.join("smoke/base/smoke.json");
        let leaking = root.join("smoke/comp/smoke.json");
        write(&clean, r#"{"path":"crates/perl-parser/src/lib.rs"}"#)?;
        write(&leaking, r#"{"cwd":"/home/runner/work/private"}"#)?;

        let report = validate_public_evidence(&root)?;
        assert_eq!(report.rejected_files.len(), 1);
        for path in &report.rejected_files {
            fs::remove_file(path)?;
        }

        assert!(clean.exists(), "clean receipt must survive quarantine");
        assert!(!leaking.exists(), "rejected receipt must not reach the upload");
        Ok(())
    }

    /// An earlier failed step can leave no receipts at all. Nothing is
    /// published, so this gate must not attribute a failure to itself.
    #[test]
    fn absent_receipt_directory_is_not_a_finding() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = validate_public_evidence(&temp.path().join("never-written"))?;
        assert!(report.findings.is_empty());
        assert_eq!(report.scanned, 0);
        Ok(())
    }

    #[test]
    fn options_require_an_evidence_directory() -> TestResult {
        assert!(Options::parse(Vec::new().into_iter()).is_err());
        assert!(Options::parse(["--quarantine".to_string()].into_iter()).is_err());
        let parsed = Options::parse(
            ["--evidence-dir".to_string(), "target/perl-core".to_string()].into_iter(),
        )?;
        assert!(!parsed.quarantine);
        Ok(())
    }
}
