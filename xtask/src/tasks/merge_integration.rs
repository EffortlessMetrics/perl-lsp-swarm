//! Typed synthetic squash-integration evidence for #4556.
//!
//! This module records the three identities involved in a synthetic
//! integration proof separately. It does not read GitHub, mutate branches, or
//! authorize a merge.

use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::tempdir;

/// Inputs for constructing a synthetic squash result in an isolated worktree.
///
/// All identities are full commit IDs. The PR base is used only to derive the
/// PR's net patch; neither the PR branch nor the integration base is mutated.
#[derive(Debug, Clone, Copy)]
pub struct SyntheticSquashRequest<'a> {
    pub repository: &'a Path,
    pub pr_base: &'a str,
    pub pr_head: &'a str,
    pub integration_basis: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticCleanup {
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticSquashConstruction {
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub cleanup: SyntheticCleanup,
}

/// Apply the PR's net patch to an isolated integration basis and return the
/// resulting tree identity.
///
/// This is deliberately a construction primitive, not an integration policy:
/// it does not read GitHub, select proof packs, run checks, update contributor
/// branches, or authorize a merge. Callers can feed the returned identities
/// into [`evaluate_synthetic_squash`] and run their selected proof separately.
pub fn construct_synthetic_squash(
    request: SyntheticSquashRequest<'_>,
) -> Result<SyntheticSquashConstruction> {
    for (label, identity) in [
        ("PR base", request.pr_base),
        ("PR head", request.pr_head),
        ("integration basis", request.integration_basis),
    ] {
        validate_full_commit_id(label, identity)?;
        run_git(request.repository, &["cat-file", "-e", &format!("{identity}^{{commit}}")])
            .with_context(|| format!("validating {label} commit {identity}"))?;
    }

    let patch = run_git_bytes(
        request.repository,
        &["diff", "--binary", "--full-index", request.pr_base, request.pr_head],
    )
    .context("deriving the PR net patch")?;

    let scratch = tempdir().context("creating synthetic integration scratch directory")?;
    let worktree = scratch.path().join("integration");
    run_git(
        request.repository,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_string_lossy().as_ref(),
            request.integration_basis,
        ],
    )
    .context("creating isolated synthetic integration worktree")?;

    let construction = (|| {
        apply_patch(&worktree, &patch).context("applying the PR net patch")?;
        let synthetic_tree =
            run_git(&worktree, &["write-tree"]).context("writing synthetic tree")?;
        Ok(SyntheticSquashConstruction {
            pr_head: request.pr_head.to_owned(),
            integration_basis: request.integration_basis.to_owned(),
            synthetic_tree: synthetic_tree.trim().to_owned(),
            cleanup: SyntheticCleanup::Complete,
        })
    })();

    let cleanup = run_git(
        request.repository,
        &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
    );

    match (construction, cleanup) {
        (Ok(result), Ok(_)) => Ok(result),
        (Ok(_), Err(cleanup_error)) => {
            Err(eyre!("synthetic integration succeeded but cleanup failed: {cleanup_error}"))
        }
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(cleanup_error)) => Err(eyre!(
            "synthetic integration failed: {error}; cleanup also failed: {cleanup_error}"
        )),
    }
}

fn validate_full_commit_id(label: &str, identity: &str) -> Result<()> {
    if !is_git_object_id(identity) {
        return Err(eyre!("{label} identity must be a 40-character hexadecimal Git commit ID"));
    }
    Ok(())
}

fn run_git(directory: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(run_git_bytes(directory, args)?).context("git output was not UTF-8")
}

fn run_git_bytes(directory: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output =
        Command::new("git").args(args).current_dir(directory).output().context("spawning git")?;
    if !output.status.success() {
        return Err(eyre!(
            "git command failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn apply_patch(worktree: &Path, patch: &[u8]) -> Result<()> {
    let mut child = Command::new("git")
        .args(["apply", "--index", "--whitespace=nowarn"])
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning git apply")?;
    child
        .stdin
        .take()
        .ok_or_else(|| eyre!("git apply stdin was unavailable"))?
        .write_all(patch)
        .context("writing PR net patch to git apply")?;
    let output = child.wait_with_output().context("waiting for git apply")?;
    if !output.status.success() {
        return Err(eyre!(
            "git apply failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntheticSquashInput {
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub observation: SyntheticObservation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyntheticObservation {
    Success,
    Failure,
    Missing,
    Skipped,
    Cancelled,
    InstrumentFailure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyntheticVerdict {
    Success,
    Failure,
    NotProven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntheticSquashReceipt {
    pub schema_version: String,
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub observation: SyntheticObservation,
    pub verdict: SyntheticVerdict,
    pub findings: Vec<String>,
}

pub fn evaluate_synthetic_squash(input: SyntheticSquashInput) -> SyntheticSquashReceipt {
    let mut findings = Vec::new();
    for (label, identity) in [
        ("PR head", input.pr_head.as_str()),
        ("integration basis", input.integration_basis.as_str()),
        ("synthetic tree", input.synthetic_tree.as_str()),
    ] {
        let identity = identity.trim();
        if identity.is_empty() {
            findings.push(format!("{label} identity is missing"));
        } else if !is_git_object_id(identity) {
            findings
                .push(format!("{label} identity must be a 40-character hexadecimal Git object ID"));
        }
    }

    if matches!(
        input.observation,
        SyntheticObservation::Missing
            | SyntheticObservation::Skipped
            | SyntheticObservation::Cancelled
            | SyntheticObservation::InstrumentFailure
    ) {
        findings.push(format!("synthetic observation is {:?}", input.observation));
    }

    let verdict = if !findings.is_empty() {
        SyntheticVerdict::NotProven
    } else {
        match input.observation {
            SyntheticObservation::Success => SyntheticVerdict::Success,
            SyntheticObservation::Failure => SyntheticVerdict::Failure,
            SyntheticObservation::Missing
            | SyntheticObservation::Skipped
            | SyntheticObservation::Cancelled
            | SyntheticObservation::InstrumentFailure => SyntheticVerdict::NotProven,
        }
    };

    SyntheticSquashReceipt {
        schema_version: "synthetic-squash.v1".to_string(),
        pr_head: input.pr_head,
        integration_basis: input.integration_basis,
        synthetic_tree: input.synthetic_tree,
        observation: input.observation,
        verdict,
        findings,
    }
}

fn is_git_object_id(identity: &str) -> bool {
    identity.len() == 40 && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn input(observation: SyntheticObservation) -> SyntheticSquashInput {
        SyntheticSquashInput {
            pr_head: "0123456789abcdef0123456789abcdef01234567".to_string(),
            integration_basis: "89abcdef0123456789abcdef0123456789abcdef".to_string(),
            synthetic_tree: "fedcba9876543210fedcba9876543210fedcba98".to_string(),
            observation,
        }
    }

    #[test]
    fn success_preserves_separate_integration_identities() {
        let receipt = evaluate_synthetic_squash(input(SyntheticObservation::Success));
        assert_eq!(receipt.verdict, SyntheticVerdict::Success);
        assert_eq!(receipt.pr_head, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(receipt.integration_basis, "89abcdef0123456789abcdef0123456789abcdef");
        assert_eq!(receipt.synthetic_tree, "fedcba9876543210fedcba9876543210fedcba98");
    }

    #[test]
    fn observed_failure_is_distinct_from_not_proven() {
        let receipt = evaluate_synthetic_squash(input(SyntheticObservation::Failure));
        assert_eq!(receipt.verdict, SyntheticVerdict::Failure);
        assert!(receipt.findings.is_empty());
    }

    #[test]
    fn incomplete_observations_are_not_proven() {
        for observation in [
            SyntheticObservation::Missing,
            SyntheticObservation::Skipped,
            SyntheticObservation::Cancelled,
            SyntheticObservation::InstrumentFailure,
        ] {
            let receipt = evaluate_synthetic_squash(input(observation));
            assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
            assert!(!receipt.findings.is_empty());
        }
    }

    #[test]
    fn missing_identity_overrides_success_observation() {
        let mut input = input(SyntheticObservation::Success);
        input.integration_basis.clear();
        let receipt = evaluate_synthetic_squash(input);
        assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
        assert!(receipt.findings.iter().any(|finding| finding.contains("integration basis")));
    }

    #[test]
    fn malformed_identity_and_incomplete_observation_are_both_reported() {
        let mut input = input(SyntheticObservation::Skipped);
        input.synthetic_tree = "placeholder".to_string();
        let receipt = evaluate_synthetic_squash(input);
        assert_eq!(receipt.verdict, SyntheticVerdict::NotProven);
        assert!(receipt.findings.iter().any(|finding| finding.contains("synthetic tree")));
        assert!(receipt.findings.iter().any(|finding| finding.contains("synthetic observation")));
    }

    #[test]
    fn verdict_serializes_with_established_evidence_tokens() -> serde_json::Result<()> {
        let not_proven = serde_json::to_string(&SyntheticVerdict::NotProven)?;
        let failure = serde_json::to_string(&SyntheticVerdict::Failure)?;
        assert_eq!(not_proven, "\"NOT_PROVEN\"");
        assert_eq!(failure, "\"FAILURE\"");
        Ok(())
    }

    fn git(repository: &Path, args: &[&str]) -> Result<String> {
        run_git(repository, args)
    }

    #[test]
    fn constructs_synthetic_tree_without_mutating_source_branches() -> Result<()> {
        let scratch = tempdir()?;
        let repository = scratch.path().join("repository");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "--quiet"])?;
        git(&repository, &["config", "user.email", "test@example.invalid"])?;
        git(&repository, &["config", "user.name", "synthetic-test"])?;

        fs::write(repository.join("base.txt"), "base\n")?;
        git(&repository, &["add", "base.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "base"])?;
        let pr_base = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();
        git(&repository, &["branch", "integration"])?;

        fs::write(repository.join("candidate.txt"), "candidate\n")?;
        git(&repository, &["add", "candidate.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "candidate"])?;
        let pr_head = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        git(&repository, &["switch", "--quiet", "integration"])?;
        fs::write(repository.join("unrelated.txt"), "unrelated\n")?;
        git(&repository, &["add", "unrelated.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "integration"])?;
        let integration_basis = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        let result = construct_synthetic_squash(SyntheticSquashRequest {
            repository: &repository,
            pr_base: &pr_base,
            pr_head: &pr_head,
            integration_basis: &integration_basis,
        })?;

        assert_eq!(result.pr_head, pr_head);
        assert_eq!(result.integration_basis, integration_basis);
        assert_eq!(result.cleanup, SyntheticCleanup::Complete);
        assert_eq!(git(&repository, &["rev-parse", "integration"])?.trim(), integration_basis);
        assert_eq!(git(&repository, &["rev-parse", "HEAD"])?.trim(), integration_basis);
        assert!(
            git(&repository, &["ls-tree", "-r", &result.synthetic_tree])?.contains("candidate.txt")
        );
        assert!(
            git(&repository, &["ls-tree", "-r", &result.synthetic_tree])?.contains("unrelated.txt")
        );
        Ok(())
    }
}
