//! `cargo xtask sync-divergence check` — fail closed on unclassified target commits.

use color_eyre::eyre::{Context, Report, Result, eyre};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLASSIFICATIONS: [&str; 5] = [
    "port_to_swarm",
    "already_equivalent_in_swarm",
    "superseded_by_newer_architecture",
    "deliberately_abandoned",
    "release_lineage_only",
];

/// Arguments for the sync-divergence preflight.
pub struct CheckConfig {
    /// Common source/target base used for the git cherry comparison.
    pub base: String,
    /// Active swarm source ref.
    pub source: String,
    /// Release-repo target ref, normally the first parent of the sync merge.
    pub target: String,
    /// Machine-readable reconciliation ledger.
    pub ledger: PathBuf,
    /// Output source-sync receipt JSON.
    pub receipt: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Ledger {
    schema_version: u32,
    base: String,
    source: String,
    target: String,
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Deserialize)]
struct LedgerEntry {
    commit: String,
    subject: String,
    classification: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    base: String,
    source: String,
    target: String,
    ledger: String,
    target_unique_commits: Vec<ReceiptCommit>,
    excluded_merge_commits: Vec<String>,
    excluded_release_lineage_commits: Vec<String>,
    accepted_commits: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReceiptCommit {
    commit: String,
    subject: String,
    classification: String,
}

#[derive(Debug)]
struct CherryCommit {
    commit: String,
    subject: String,
    is_merge: bool,
}

/// Run the preflight and write a receipt even when validation fails.
pub fn check(config: CheckConfig) -> Result<()> {
    let ledger = match load_ledger(&config.ledger) {
        Ok(ledger) => ledger,
        Err(error) => return fail_with_receipt(&config, error),
    };
    if let Err(error) = validate_ledger_identity(&ledger, &config) {
        return fail_with_receipt(&config, error);
    }
    if let Err(error) = validate_source_ref(&config.source) {
        return fail_with_receipt(&config, error);
    }
    let target_unique = match target_unique_commits(&config.base, &config.target) {
        Ok(commits) => commits,
        Err(error) => return fail_with_receipt(&config, error),
    };

    let mut errors = Vec::new();
    let mut entries = BTreeMap::new();
    for entry in &ledger.entries {
        if entries.contains_key(entry.commit.as_str()) {
            errors.push(format!("commit {} appears more than once", entry.commit));
        } else {
            entries.insert(entry.commit.as_str(), entry);
        }
    }
    let mut seen = BTreeSet::new();
    let mut receipt_commits = Vec::new();
    let mut excluded_merge_commits = Vec::new();
    let mut excluded_release_lineage_commits = Vec::new();
    let mut accepted_commits = Vec::new();

    for commit in &target_unique {
        if commit.is_merge {
            excluded_merge_commits.push(commit.commit.clone());
            continue;
        }

        let Some(entry) = entries.get(commit.commit.as_str()) else {
            errors.push(format!(
                "target-unique commit {} is missing from the reconciliation ledger",
                commit.commit
            ));
            continue;
        };

        seen.insert(commit.commit.as_str());
        if normalize_subject(&entry.subject) != normalize_subject(&commit.subject) {
            errors.push(format!(
                "ledger subject for {} does not match Git: ledger=`{}` Git=`{}`",
                commit.commit, entry.subject, commit.subject
            ));
        }
        receipt_commits.push(ReceiptCommit {
            commit: commit.commit.clone(),
            subject: commit.subject.clone(),
            classification: entry.classification.clone(),
        });

        if !CLASSIFICATIONS.contains(&entry.classification.as_str()) {
            errors.push(format!(
                "commit {} has invalid classification `{}`",
                commit.commit, entry.classification
            ));
            continue;
        }

        if entry.classification == "release_lineage_only" {
            excluded_release_lineage_commits.push(commit.commit.clone());
        } else {
            accepted_commits.push(commit.commit.clone());
        }
    }

    for entry in &ledger.entries {
        if entry.evidence.is_empty() {
            errors.push(format!("commit {} has no evidence", entry.commit));
        }
        if !seen.contains(entry.commit.as_str()) {
            errors.push(format!(
                "ledger commit {} is not a non-merge target-unique commit",
                entry.commit
            ));
        }
    }

    let receipt = Receipt {
        schema_version: 1,
        base: config.base.clone(),
        source: config.source.clone(),
        target: config.target.clone(),
        ledger: config.ledger.display().to_string(),
        target_unique_commits: receipt_commits,
        excluded_merge_commits,
        excluded_release_lineage_commits,
        accepted_commits,
        errors: errors.clone(),
    };
    write_receipt(&config.receipt, &receipt)?;

    if errors.is_empty() {
        println!(
            "sync-divergence: checked {} target-unique non-merge commit(s)",
            receipt.target_unique_commits.len()
        );
        Ok(())
    } else {
        Err(eyre!(
            "sync-divergence preflight failed with {} error(s); see {}",
            errors.len(),
            config.receipt.display()
        ))
    }
}

fn load_ledger(path: &Path) -> Result<Ledger> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading reconciliation ledger {}", path.display()))?;
    let ledger = serde_json::from_str(&content)
        .with_context(|| format!("parsing reconciliation ledger {}", path.display()))?;
    Ok(ledger)
}

fn validate_ledger_identity(ledger: &Ledger, config: &CheckConfig) -> Result<()> {
    if ledger.schema_version != 1 {
        return Err(eyre!(
            "unsupported reconciliation ledger schema version {}",
            ledger.schema_version
        ));
    }
    if ledger.base != config.base
        || ledger.source != config.source
        || ledger.target != config.target
    {
        return Err(eyre!(
            "reconciliation ledger refs do not match --base, --source, and --target"
        ));
    }
    Ok(())
}

fn validate_source_ref(source: &str) -> Result<()> {
    let commit_ref = format!("{source}^{{commit}}");
    git_output(["rev-parse", "--verify", "--quiet", &commit_ref])
        .with_context(|| format!("validating --source ref `{source}`"))?;
    Ok(())
}

fn target_unique_commits(base: &str, target: &str) -> Result<Vec<CherryCommit>> {
    let output = git_output(["cherry", base, target])?;
    let mut commits = Vec::new();
    for commit in parse_cherry_plus_lines(&output) {
        let commit = commit.to_string();
        let subject = git_output(["show", "-s", "--format=%s", &commit])?.trim().to_string();
        let parents =
            git_output(["rev-list", "--parents", "-n", "1", &commit])?.split_whitespace().count();
        commits.push(CherryCommit { commit, subject, is_merge: parents > 2 });
    }
    Ok(commits)
}

fn parse_cherry_plus_lines(output: &str) -> Vec<&str> {
    output.lines().filter_map(|line| line.strip_prefix("+ ").map(str::trim)).collect()
}

fn normalize_subject(subject: &str) -> String {
    subject.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fail_with_receipt(config: &CheckConfig, error: Report) -> Result<()> {
    let message = error.to_string();
    let receipt = Receipt {
        schema_version: 1,
        base: config.base.clone(),
        source: config.source.clone(),
        target: config.target.clone(),
        ledger: config.ledger.display().to_string(),
        target_unique_commits: Vec::new(),
        excluded_merge_commits: Vec::new(),
        excluded_release_lineage_commits: Vec::new(),
        accepted_commits: Vec::new(),
        errors: vec![message.clone()],
    };
    write_receipt(&config.receipt, &receipt)?;
    Err(eyre!(
        "sync-divergence preflight failed before comparison: {message}; see {}",
        config.receipt.display()
    ))
}

fn git_output<const N: usize>(args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("running git for sync-divergence preflight")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("git command failed: {stderr}"));
    }
    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(receipt).context("serializing sync receipt")?;
    fs::write(path, format!("{content}\n"))
        .with_context(|| format!("writing sync receipt {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plus_lines_are_target_unique() -> Result<()> {
        let output = "+ abc\n- def\n  ghi\n";
        assert_eq!(parse_cherry_plus_lines(output), vec!["abc"]);
        Ok(())
    }

    #[test]
    fn early_failures_write_a_receipt() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let ledger = directory.path().join("missing.json");
        let receipt = directory.path().join("receipt.json");
        let error = check(CheckConfig {
            base: "HEAD".to_string(),
            source: "HEAD".to_string(),
            target: "HEAD".to_string(),
            ledger,
            receipt: receipt.clone(),
        });
        assert!(error.is_err());

        let content = fs::read_to_string(receipt)?;
        let receipt: serde_json::Value = serde_json::from_str(&content)?;
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn valid_refs_write_a_success_receipt() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let ledger = directory.path().join("ledger.json");
        let receipt = directory.path().join("receipt.json");
        fs::write(
            &ledger,
            r#"{
  "schema_version": 1,
  "base": "HEAD",
  "source": "HEAD",
  "target": "HEAD",
  "entries": []
}"#,
        )?;

        check(CheckConfig {
            base: "HEAD".to_string(),
            source: "HEAD".to_string(),
            target: "HEAD".to_string(),
            ledger,
            receipt: receipt.clone(),
        })?;

        let content = fs::read_to_string(receipt)?;
        let receipt: serde_json::Value = serde_json::from_str(&content)?;
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn source_ref_must_resolve() -> Result<()> {
        let error = validate_source_ref("refs/heads/does-not-exist-for-sync-divergence");
        assert!(error.is_err());
        Ok(())
    }

    #[test]
    fn subject_comparison_normalizes_whitespace() -> Result<()> {
        assert_eq!(
            normalize_subject("fix:   preserve  the subject"),
            normalize_subject("fix: preserve the subject")
        );
        Ok(())
    }

    #[test]
    fn classifications_are_explicit() -> Result<()> {
        assert!(CLASSIFICATIONS.contains(&"port_to_swarm"));
        assert!(CLASSIFICATIONS.contains(&"release_lineage_only"));
        assert!(!CLASSIFICATIONS.contains(&"unclassified"));
        Ok(())
    }
}
