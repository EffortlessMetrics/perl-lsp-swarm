//! Validate external GitHub Action references against the checked-in provenance ledger.
//!
//! Ordinary CI is network-free. Immutable SHAs remain execution authority; the ledger
//! records the separately reviewed human-readable release or branch projection.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const RECEIPT_SCHEMA: &str = "action_pin_provenance.v2";
const WORKFLOW_ROOTS: &[&str] = &[".github/workflows", ".github/actions"];
const DEFAULT_LEDGER: &str = ".ci/policies/action-pin-provenance.toml";

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    base: Option<String>,
    #[arg(long)]
    receipt: Option<PathBuf>,
    #[arg(long)]
    strict_all: bool,
    #[arg(long, default_value = DEFAULT_LEDGER)]
    ledger: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
struct Ledger {
    pin: Vec<LedgerPin>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
struct LedgerPin {
    action: String,
    sha: String,
    kind: ProjectionKind,
    value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProjectionKind {
    ReleaseTag,
    BranchCommit,
    LegacyDebt,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReferenceKind {
    ImmutableSha,
    Mutable,
    Docker,
    Malformed,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Occurrence {
    path: String,
    line: usize,
    action: String,
    reference: String,
    comment: Option<String>,
    reference_kind: ReferenceKind,
    projection_kind: Option<ProjectionKind>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Identity {
    path: String,
    action: String,
    reference: String,
    comment: Option<String>,
}
impl From<&Occurrence> for Identity {
    fn from(v: &Occurrence) -> Self {
        Self {
            path: v.path.clone(),
            action: v.action.clone(),
            reference: v.reference.clone(),
            comment: v.comment.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Evaluated {
    #[serde(flatten)]
    occurrence: Occurrence,
    new_or_changed: bool,
}
#[derive(Clone, Debug, Serialize)]
struct Issue {
    level: &'static str,
    code: &'static str,
    path: String,
    line: usize,
    message: String,
}
#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: &'static str,
    receipt_kind: &'static str,
    base: Option<String>,
    base_compared: bool,
    strict_all: bool,
    passed: bool,
    occurrence_count: usize,
    new_or_changed_count: usize,
    error_count: usize,
    warning_count: usize,
    occurrences: Vec<Evaluated>,
    issues: Vec<Issue>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = args.root.unwrap_or_else(default_root);
    let ledger_path = if args.ledger.is_absolute() { args.ledger } else { root.join(args.ledger) };
    let ledger = load_ledger(&ledger_path)?;
    let pattern = uses_pattern()?;
    let current = scan_worktree(&root, &pattern)?;
    let (base, compared) = match args.base.as_deref().filter(|v| !v.trim().is_empty()) {
        Some(base) => (scan_git_ref(&root, base, &pattern)?, true),
        None => (Vec::new(), false),
    };
    let mut receipt = validate(current, base, compared, args.strict_all, &ledger);
    receipt.base = args.base;
    for issue in &receipt.issues {
        eprintln!(
            "::{} file={},line={}::[{}] {}",
            issue.level, issue.path, issue.line, issue.code, issue.message
        );
    }
    if let Some(path) = args.receipt {
        write_receipt(&path, &receipt)?;
    }
    if !receipt.passed {
        bail!(
            "action-pin provenance failed with {} error(s) and {} warning(s)",
            receipt.error_count,
            receipt.warning_count
        );
    }
    println!(
        "Action-pin provenance passed ({} external use(s), {} new/changed, {} warning(s))",
        receipt.occurrence_count, receipt.new_or_changed_count, receipt.warning_count
    );
    Ok(())
}

fn default_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
fn uses_pattern() -> Result<Regex> {
    Regex::new(r#"^\s*(?:-\s*)?uses:\s*(?:\"([^\"]+)\"|'([^']+)'|([^\s#]+))\s*(?:#\s*(.*?)\s*)?$"#)
        .context("compiling uses pattern")
}
fn release_pattern() -> Result<Regex> {
    Regex::new(r"^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
        .context("compiling release pattern")
}
fn branch_pattern() -> Result<Regex> {
    Regex::new(r"^[A-Za-z0-9._/-]+ \([A-Za-z0-9._/-]+\)$").context("compiling branch pattern")
}

fn load_ledger(path: &Path) -> Result<Ledger> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn scan_worktree(root: &Path, pattern: &Regex) -> Result<Vec<Occurrence>> {
    let mut out = Vec::new();
    for relative_root in WORKFLOW_ROOTS {
        let directory = root.join(relative_root);
        if !directory.exists() {
            continue;
        }
        for entry in WalkDir::new(&directory).sort_by_file_name() {
            let entry = entry.with_context(|| format!("walking {}", directory.display()))?;
            if !entry.file_type().is_file() || !is_yaml(entry.path()) {
                continue;
            }
            let path = relative_path(root, entry.path())?;
            let text = fs::read_to_string(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            out.extend(scan_text(&path, &text, pattern)?);
        }
    }
    out.sort();
    Ok(out)
}

fn scan_git_ref(root: &Path, base: &str, pattern: &Regex) -> Result<Vec<Occurrence>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-tree", "-r", "--name-only", base, "--", ".github/workflows", ".github/actions"])
        .output()?;
    if !output.status.success() {
        return Err(eyre!(
            "git ls-tree failed for {base}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut paths: Vec<_> = String::from_utf8(output.stdout)?
        .lines()
        .filter(|p| is_yaml(Path::new(p)))
        .map(str::to_owned)
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let object = format!("{base}:{path}");
        let output = Command::new("git").current_dir(root).args(["show", &object]).output()?;
        if !output.status.success() {
            return Err(eyre!("git show failed for {object}"));
        }
        out.extend(scan_text(&path, &String::from_utf8(output.stdout)?, pattern)?);
    }
    out.sort();
    Ok(out)
}

fn scan_text(path: &str, text: &str, pattern: &Regex) -> Result<Vec<Occurrence>> {
    let release = release_pattern()?;
    let branch = branch_pattern()?;
    Ok(text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let captures = pattern.captures(line)?;
            let scalar = (1..=3).find_map(|i| captures.get(i).map(|m| m.as_str()))?;
            if scalar.starts_with("./") {
                return None;
            }
            let comment =
                captures.get(4).map(|m| m.as_str().trim().to_owned()).filter(|v| !v.is_empty());
            let (action, reference, reference_kind) =
                if let Some(image) = scalar.strip_prefix("docker://") {
                    (image.to_owned(), scalar.to_owned(), ReferenceKind::Docker)
                } else if let Some((action, reference)) = scalar.rsplit_once('@') {
                    let kind = if Regex::new(r"^[0-9a-fA-F]{40}$").ok()?.is_match(reference) {
                        ReferenceKind::ImmutableSha
                    } else {
                        ReferenceKind::Mutable
                    };
                    (action.to_owned(), reference.to_ascii_lowercase(), kind)
                } else {
                    (scalar.to_owned(), scalar.to_owned(), ReferenceKind::Malformed)
                };
            let projection_kind =
                Some(comment.as_deref().map_or(ProjectionKind::LegacyDebt, |v| {
                    if release.is_match(v) {
                        ProjectionKind::ReleaseTag
                    } else if branch.is_match(v) {
                        ProjectionKind::BranchCommit
                    } else {
                        ProjectionKind::LegacyDebt
                    }
                }));
            Some(Occurrence {
                path: path.to_owned(),
                line: index + 1,
                action,
                reference,
                comment,
                reference_kind,
                projection_kind,
            })
        })
        .collect())
}

fn validate(
    current: Vec<Occurrence>,
    base: Vec<Occurrence>,
    compared: bool,
    strict: bool,
    ledger: &Ledger,
) -> Receipt {
    let mut counts = BTreeMap::new();
    for item in &base {
        *counts.entry(Identity::from(item)).or_insert(0usize) += 1;
    }
    let evaluated: Vec<_> = current
        .into_iter()
        .map(|occurrence| {
            let unchanged = counts.get_mut(&Identity::from(&occurrence)).is_some_and(|count| {
                if *count > 0 {
                    *count -= 1;
                    true
                } else {
                    false
                }
            });
            Evaluated { occurrence, new_or_changed: compared && !unchanged }
        })
        .collect();
    let mut issues = Vec::new();
    validate_ledger(ledger, &mut issues);
    for item in &evaluated {
        validate_occurrence(item, strict, ledger, &mut issues);
    }
    issues.sort_by(|a, b| {
        (&a.level, &a.path, a.line, &a.code, &a.message)
            .cmp(&(&b.level, &b.path, b.line, &b.code, &b.message))
    });
    let error_count = issues.iter().filter(|i| i.level == "error").count();
    let warning_count = issues.len() - error_count;
    Receipt {
        schema_version: RECEIPT_SCHEMA,
        receipt_kind: "action_pin_provenance",
        base: None,
        base_compared: compared,
        strict_all: strict,
        passed: error_count == 0,
        occurrence_count: evaluated.len(),
        new_or_changed_count: evaluated.iter().filter(|v| v.new_or_changed).count(),
        error_count,
        warning_count,
        occurrences: evaluated,
        issues,
    }
}

fn validate_ledger(ledger: &Ledger, issues: &mut Vec<Issue>) {
    let mut values: BTreeMap<(&str, &str), BTreeSet<(&ProjectionKind, &str)>> = BTreeMap::new();
    for pin in &ledger.pin {
        values.entry((&pin.action, &pin.sha)).or_default().insert((&pin.kind, &pin.value));
    }
    for ((action, sha), projections) in values {
        let authoritative: BTreeSet<_> =
            projections.iter().filter(|(kind, _)| **kind != ProjectionKind::LegacyDebt).collect();
        if authoritative.len() > 1 {
            issues.push(Issue {
                level: "error",
                code: "CONTRADICTORY_LEDGER_MAPPING",
                path: DEFAULT_LEDGER.into(),
                line: 1,
                message: format!("{action}@{sha} has contradictory reviewed mappings"),
            });
        }
    }
}

fn validate_occurrence(item: &Evaluated, strict: bool, ledger: &Ledger, issues: &mut Vec<Issue>) {
    let pin = &item.occurrence;
    if pin.reference_kind != ReferenceKind::ImmutableSha {
        issues.push(Issue {
            level: "error",
            code: "MUTABLE_OR_UNSUPPORTED_ACTION_REF",
            path: pin.path.clone(),
            line: pin.line,
            message: format!(
                "external use {} is {:?}; require an exact 40-hex commit SHA",
                pin.reference, pin.reference_kind
            ),
        });
        return;
    }
    let matched = ledger.pin.iter().find(|entry| {
        entry.action == pin.action
            && entry.sha == pin.reference
            && (entry.kind == ProjectionKind::LegacyDebt || Some(entry.kind) == pin.projection_kind)
            && pin.comment.as_deref().unwrap_or("") == entry.value
    });
    match matched {
        Some(entry) if entry.kind != ProjectionKind::LegacyDebt => {}
        Some(_) if !item.new_or_changed && !strict => issues.push(Issue {
            level: "warning",
            code: "RECORDED_LEGACY_PROVENANCE_DEBT",
            path: pin.path.clone(),
            line: pin.line,
            message: format!(
                "{}@{} retains explicitly recorded legacy provenance debt",
                pin.action, pin.reference
            ),
        }),
        Some(_) => issues.push(Issue {
            level: "error",
            code: "LEGACY_DEBT_NOT_ALLOWED_FOR_CHANGED_PIN",
            path: pin.path.clone(),
            line: pin.line,
            message: format!(
                "{}@{} cannot introduce or retain legacy debt in strict mode",
                pin.action, pin.reference
            ),
        }),
        None => issues.push(Issue {
            level: "error",
            code: "ACTION_PROVENANCE_NOT_PROVEN",
            path: pin.path.clone(),
            line: pin.line,
            message: format!(
                "{}@{} and its projection are absent from the reviewed ledger",
                pin.action, pin.reference
            ),
        }),
    }
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(receipt)?))
        .with_context(|| format!("writing {}", path.display()))
}
fn relative_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"))
}
fn is_yaml(path: &Path) -> bool {
    path.extension().and_then(|v| v.to_str()).is_some_and(|v| v == "yml" || v == "yaml")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan(source: &str) -> Result<Vec<Occurrence>> {
        scan_text(".github/workflows/test.yml", source, &uses_pattern()?)
    }
    fn ledger(pins: Vec<LedgerPin>) -> Ledger {
        Ledger { pin: pins }
    }
    fn row(action: &str, sha: &str, kind: ProjectionKind, value: &str) -> LedgerPin {
        LedgerPin { action: action.into(), sha: sha.into(), kind, value: value.into() }
    }
    const SHA: &str = "1111111111111111111111111111111111111111";
    #[test]
    fn inventories_mutable_and_quoted_sha() -> Result<()> {
        let got = scan(&format!(
            "- uses: actions/checkout@v4\n- uses: \"actions/checkout@{SHA}\" # v4.1.0\n"
        ))?;
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].reference_kind, ReferenceKind::Mutable);
        assert_eq!(got[1].reference_kind, ReferenceKind::ImmutableSha);
        Ok(())
    }
    #[test]
    fn mutable_ref_is_blocking() -> Result<()> {
        let receipt = validate(
            scan("- uses: actions/checkout@main\n")?,
            vec![],
            true,
            false,
            &ledger(vec![]),
        );
        assert!(!receipt.passed);
        assert!(receipt.issues.iter().any(|i| i.code == "MUTABLE_OR_UNSUPPORTED_ACTION_REF"));
        Ok(())
    }
    #[test]
    fn stale_exact_tag_on_unrelated_sha_fails() -> Result<()> {
        let got = scan(&format!("- uses: actions/checkout@{SHA} # v7.0.0\n"))?;
        let known = row(
            "actions/checkout",
            "2222222222222222222222222222222222222222",
            ProjectionKind::ReleaseTag,
            "v7.0.0",
        );
        let receipt = validate(got, vec![], true, false, &ledger(vec![known]));
        assert!(receipt.issues.iter().any(|i| i.code == "ACTION_PROVENANCE_NOT_PROVEN"));
        Ok(())
    }
    #[test]
    fn mapped_release_and_branch_pass() -> Result<()> {
        let source = format!(
            "- uses: actions/checkout@{SHA} # v7.0.0\n- uses: dtolnay/rust-toolchain@2222222222222222222222222222222222222222 # stable (master)\n"
        );
        let map = ledger(vec![
            row("actions/checkout", SHA, ProjectionKind::ReleaseTag, "v7.0.0"),
            row(
                "dtolnay/rust-toolchain",
                "2222222222222222222222222222222222222222",
                ProjectionKind::BranchCommit,
                "stable (master)",
            ),
        ]);
        assert!(validate(scan(&source)?, vec![], true, false, &map).passed);
        Ok(())
    }
    #[test]
    fn contradictory_authoritative_mappings_fail() -> Result<()> {
        let map = ledger(vec![
            row("actions/checkout", SHA, ProjectionKind::ReleaseTag, "v7.0.0"),
            row("actions/checkout", SHA, ProjectionKind::ReleaseTag, "v7.0.1"),
        ]);
        let receipt = validate(vec![], vec![], false, false, &map);
        assert!(receipt.issues.iter().any(|i| i.code == "CONTRADICTORY_LEDGER_MAPPING"));
        Ok(())
    }
    #[test]
    fn explicitly_recorded_unchanged_debt_only_warns() -> Result<()> {
        let source = format!("- uses: actions/checkout@{SHA} # v7\n");
        let got = scan(&source)?;
        let receipt = validate(
            got.clone(),
            got,
            true,
            false,
            &ledger(vec![row("actions/checkout", SHA, ProjectionKind::LegacyDebt, "v7")]),
        );
        assert!(receipt.passed);
        assert_eq!(receipt.warning_count, 1);
        Ok(())
    }
}
