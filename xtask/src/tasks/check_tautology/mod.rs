//! Conservative checker for provably tautological Rust assertions (#14061).
//!
//! Inventory is the existing `crates`/`xtask`/`examples`/`tests` source tree.
//! Detection is a syn AST walk of `assert!`/`assert_eq!` (and debug variants).
//! False negatives are accepted; false positives are not.

mod detect;
mod disposition;
mod inventory;
mod scan;

use crate::utils::project_root;
use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Context, Result, bail};
use disposition::DispositionLedger;
use inventory::collect_rust_files;
use scan::{Finding, scan_file};
use serde::Serialize;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub struct CheckTautologyArgs {
    pub check: bool,
    pub root: Option<PathBuf>,
    pub policy: Option<PathBuf>,
    pub receipt: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
struct Receipt {
    schema_version: &'static str,
    checker: &'static str,
    files_scanned: usize,
    findings: Vec<String>,
    instrument_errors: Vec<String>,
}

#[derive(Debug, Default)]
struct ScanReport {
    files_scanned: usize,
    findings: Vec<Finding>,
    errors: Vec<String>,
}

pub fn run(args: CheckTautologyArgs) -> Result<()> {
    let root = match args.root {
        Some(root) => root,
        None => project_root()?,
    };
    let as_of = Utc::now().date_naive();
    let report = scan_root(&root, args.policy.as_deref(), as_of)?;
    print_report(&report);

    if let Some(receipt_path) = args.receipt.as_deref() {
        write_receipt(receipt_path, &report)?;
    }

    if !report.errors.is_empty() {
        bail!(
            "tautology checker instrument failure: {} error(s); this is not a zero-finding result",
            report.errors.len()
        );
    }

    if args.check && !report.findings.is_empty() {
        bail!("tautology checker found {} tautological assertion(s)", report.findings.len());
    }

    if report.findings.is_empty() {
        println!("tautology checker: 0 findings in {} governed Rust file(s)", report.files_scanned);
    }
    Ok(())
}

fn scan_root(root: &Path, policy: Option<&Path>, as_of: NaiveDate) -> Result<ScanReport> {
    let ledger = load_ledger(root, policy, as_of)?;
    let files = collect_rust_files(root)?;
    let mut report = ScanReport { files_scanned: files.len(), ..ScanReport::default() };

    for path in files {
        let relative =
            path.strip_prefix(root).unwrap_or(path.as_path()).to_string_lossy().replace('\\', "/");
        match read_governed_source(&path) {
            Ok(source) => match scan_file(&relative, &source) {
                Ok(findings) => report.findings.extend(findings),
                Err(error) => {
                    report.errors.push(format!("{relative}: unparsable governed input: {error}"))
                }
            },
            Err(error) => {
                report.errors.push(format!("{relative}: unreadable governed input: {error}"))
            }
        }
    }

    report.findings.sort();
    let unused = ledger.unused_for(&report.findings);
    if !unused.is_empty() {
        report.errors.push(format!("unused tautology disposition(s): {}", unused.join(", ")));
    }
    report.findings.retain(|finding| !ledger.suppress(finding));
    Ok(report)
}

fn load_ledger(root: &Path, policy: Option<&Path>, as_of: NaiveDate) -> Result<DispositionLedger> {
    let default_path = root.join("policy/tautology-dispositions.toml");
    let path = match policy {
        Some(path) => path.to_path_buf(),
        None if default_path.is_file() => default_path,
        None => return Ok(DispositionLedger::empty()),
    };
    DispositionLedger::load(&path, as_of)
}

fn read_governed_source(path: &Path) -> Result<String, String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| format!("not UTF-8 ({error})")),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(format!("file disappeared during scan: {error}"))
        }
        Err(error) => Err(error.to_string()),
    }
}

fn print_report(report: &ScanReport) {
    for error in &report.errors {
        eprintln!("tautology-error: {error}");
    }
    for finding in &report.findings {
        eprintln!("{}", finding.render());
    }
}

fn write_receipt(path: &Path, report: &ScanReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create receipt dir {}", parent.display()))?;
    }
    let receipt = Receipt {
        schema_version: "tautology-check.v1",
        checker: "check-tautology",
        files_scanned: report.files_scanned,
        findings: report.findings.iter().map(Finding::render).collect(),
        instrument_errors: report.errors.clone(),
    };
    let json = serde_json::to_string_pretty(&receipt).context("serialize tautology receipt")?;
    fs::write(path, json).with_context(|| format!("failed to write receipt {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::detect::RuleId;
    use super::{scan_file, scan_root};
    use chrono::NaiveDate;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const PATH_SECURITY_HIT: &str = r#"
        fn sanitize_completion_path_input(_path: &str) -> Option<String> { None }
        #[test]
        fn test_traversal_encoded_dot_segments_completion() {
            assert!(
                sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_some()
                    || sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_none()
            );
            assert!(sanitize_completion_path_input("../foo").is_none());
        }
    "#;

    fn as_of() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 30).expect("date")
    }

    fn write_rs(root: &std::path::Path, relative: &str, source: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, source).expect("write rust");
    }

    #[test]
    fn path_security_hit_is_red_before_repair() {
        let findings =
            scan_file("crates/perl-parser-core/src/syntax/path_security.rs", PATH_SECURITY_HIT)
                .expect("parse");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleId::OptionSomeOrNone);
        assert!(findings[0].line >= 1);
    }

    #[test]
    fn repaired_path_security_boundary_stays_green_and_reinsertion_is_red() {
        let repaired = r#"
            fn sanitize_completion_path_input(path: &str) -> Option<String> {
                Some(path.to_string())
            }
            #[test]
            fn test_traversal_encoded_dot_segments_completion() {
                assert_eq!(
                    sanitize_completion_path_input("..%2f..%2fetc%2fpasswd"),
                    Some("..%2f..%2fetc%2fpasswd".to_string())
                );
                assert!(sanitize_completion_path_input("../foo").is_none());
            }
        "#;
        let findings = scan_file("path_security.rs", repaired).expect("parse repaired");
        assert!(findings.is_empty(), "{findings:?}");

        let reinserted = repaired.replace(
            r#"assert_eq!(
                    sanitize_completion_path_input("..%2f..%2fetc%2fpasswd"),
                    Some("..%2f..%2fetc%2fpasswd".to_string())
                );"#,
            r#"assert!(
                    sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_some()
                        || sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_none()
                );"#,
        );
        let findings = scan_file("path_security.rs", &reinserted).expect("parse reinsertion");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleId::OptionSomeOrNone);
    }

    #[test]
    fn opposite_direction_controls_stay_green() {
        let source = r#"
            fn probe(result: Result<(), Expected>, item: Item, ready: bool) {
                assert!(result.is_ok() || matches!(result, Err(Expected::Deferred)));
                assert!(item.code.is_some() || item.data.is_none());
                let _ = ready || !ready;
                tick();
                assert!(tick() || !tick());
            }
            enum Expected { Deferred }
            struct Item { code: Option<u8>, data: Option<u8> }
            fn tick() -> bool { true }
            // Historical example: assert!(value.is_some() || value.is_none());
        "#;
        let findings = scan_file("controls.rs", source).expect("parse");
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn compile_fail_fixture_trees_are_outside_the_executable_denominator() {
        let tmp = TempDir::new().expect("tempdir");
        write_rs(
            tmp.path(),
            "crates/demo/src/lib.rs",
            "fn probe(value: Option<u8>) { assert!(value.is_some()); }\n",
        );
        write_rs(
            tmp.path(),
            "crates/demo/tests/fixtures/hist.rs",
            "use Scalar::Util qw(looks_like_number);\nfn f(v: Option<u8>) { assert!(v.is_some() || v.is_none()); }\n",
        );
        let report = scan_root(tmp.path(), None, as_of()).expect("scan");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn fixture_skip_does_not_hide_governed_src_findings() {
        let tmp = TempDir::new().expect("tempdir");
        write_rs(
            tmp.path(),
            "crates/demo/src/lib.rs",
            "fn probe(value: Option<u8>) { assert!(value.is_some() || value.is_none()); }\n",
        );
        write_rs(
            tmp.path(),
            "crates/demo/tests/fixtures/hist.rs",
            "use Scalar::Util qw(looks_like_number);\nfn f(v: Option<u8>) { assert!(v.is_some() || v.is_none()); }\n",
        );
        let report = scan_root(tmp.path(), None, as_of()).expect("scan");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        assert_eq!(report.findings[0].path, "crates/demo/src/lib.rs");
        assert_eq!(report.findings[0].rule, RuleId::OptionSomeOrNone);
    }

    #[test]
    fn unparsable_governed_file_is_instrument_failure() {
        let tmp = TempDir::new().expect("tempdir");
        write_rs(tmp.path(), "crates/demo/src/lib.rs", "fn broken( {");
        let report = scan_root(tmp.path(), None, as_of()).expect("scan");
        assert!(report.findings.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("unparsable"), "{:?}", report.errors);
    }

    #[test]
    fn missing_governed_file_is_unreadable() {
        let error =
            super::read_governed_source(Path::new("/definitely/missing/tautology-probe.rs"))
                .expect_err("missing file");
        assert!(!error.is_empty(), "{error}");
    }

    #[test]
    fn expired_disposition_fails_the_instrument() {
        let tmp = TempDir::new().expect("tempdir");
        write_rs(
            tmp.path(),
            "crates/demo/src/lib.rs",
            "fn probe(value: Option<u8>) { assert!(value.is_some() || value.is_none()); }\n",
        );
        fs::create_dir_all(tmp.path().join("policy")).expect("policy dir");
        fs::write(
            tmp.path().join("policy/ledger.toml"),
            r##"
schema_version = 1
policy = "tautology-dispositions"
[[disposition]]
id = "tautology-demo"
rule = "option-is-some-or-none"
path = "crates/demo/src/lib.rs"
owner = "parser-core"
issue = "#14061"
reason = "expired on purpose"
created = "2026-01-01"
expires = "2026-01-02"
"##,
        )
        .expect("ledger");
        let error = scan_root(tmp.path(), Some(&tmp.path().join("policy/ledger.toml")), as_of())
            .expect_err("expired ledger");
        let display = format!("{error:#}");
        assert!(display.contains("expired"), "{display}");
    }

    #[test]
    fn ownerless_disposition_fails_the_instrument() {
        let tmp = TempDir::new().expect("tempdir");
        write_rs(tmp.path(), "crates/demo/src/lib.rs", "fn probe() {}\n");
        fs::create_dir_all(tmp.path().join("policy")).expect("policy dir");
        fs::write(
            tmp.path().join("policy/ledger.toml"),
            r##"
schema_version = 1
policy = "tautology-dispositions"
[[disposition]]
id = "tautology-demo"
rule = "option-is-some-or-none"
path = "crates/demo/src/lib.rs"
owner = ""
issue = "#14061"
reason = "missing owner"
created = "2026-08-30"
expires = "2026-11-30"
"##,
        )
        .expect("ledger");
        let error = scan_root(tmp.path(), Some(&tmp.path().join("policy/ledger.toml")), as_of())
            .expect_err("ownerless ledger");
        let display = format!("{error:#}");
        assert!(display.contains("ownerless"), "{display}");
    }
}
