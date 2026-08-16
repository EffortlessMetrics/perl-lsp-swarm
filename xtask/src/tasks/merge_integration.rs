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
/// All identities are full commit IDs. The PR base is used to derive the PR's
/// merge base and net patch; neither the PR branch nor the integration base is
/// mutated.
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

// Declared public surface of the synthetic-squash primitive: the constructor
// and its serde report schema. `with_synthetic_squash` is live (integration_proof
// uses it); these entry points and schema types have no caller yet. Deleting a
// documented primitive and its wire schema to satisfy dead_code would drop the
// contract, not dead code.
#[allow(dead_code)]
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
    with_synthetic_squash(request, |construction, _worktree| Ok(construction.clone()))
}

/// Construct a synthetic squash, keep its isolated worktree alive while the
/// caller runs proof, and remove it before returning.
pub fn with_synthetic_squash<T>(
    request: SyntheticSquashRequest<'_>,
    operation: impl FnOnce(&SyntheticSquashConstruction, &Path) -> Result<T>,
) -> Result<T> {
    for (label, identity) in [
        ("PR base", request.pr_base),
        ("PR head", request.pr_head),
        ("integration basis", request.integration_basis),
    ] {
        validate_commit_object(request.repository, label, identity)?;
    }

    let merge_base = run_git(request.repository, &["merge-base", request.pr_base, request.pr_head])
        .context("deriving the PR merge base")?;
    let patch = run_git_bytes(
        request.repository,
        &["diff", "--binary", "--full-index", merge_base.trim(), request.pr_head],
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
        let synthetic_tree = if patch.is_empty() {
            run_git(&worktree, &["rev-parse", "HEAD^{tree}"])
                .context("reading unchanged synthetic tree")?
        } else {
            apply_patch(&worktree, &patch).context("applying the PR net patch")?;
            run_git(&worktree, &["write-tree"]).context("writing synthetic tree")?
        };
        Ok(SyntheticSquashConstruction {
            pr_head: request.pr_head.to_owned(),
            integration_basis: request.integration_basis.to_owned(),
            synthetic_tree: synthetic_tree.trim().to_owned(),
            cleanup: SyntheticCleanup::Complete,
        })
    })();

    let operation = construction.and_then(|construction| operation(&construction, &worktree));
    let cleanup = run_git(
        request.repository,
        &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
    );

    match (operation, cleanup) {
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

fn validate_commit_object(repository: &Path, label: &str, identity: &str) -> Result<()> {
    validate_full_commit_id(label, identity)?;
    let object_type = run_git(repository, &["cat-file", "-t", identity])
        .with_context(|| format!("validating {label} object {identity}"))?;
    if object_type.trim() != "commit" {
        return Err(eyre!(
            "{label} identity {identity} must name a commit object, found {}",
            object_type.trim()
        ));
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
        .args(["apply", "--index", "--3way", "--whitespace=nowarn"])
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

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntheticSquashInput {
    pub pr_head: String,
    pub integration_basis: String,
    pub synthetic_tree: String,
    pub observation: SyntheticObservation,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyntheticVerdict {
    Success,
    Failure,
    NotProven,
}

#[allow(dead_code)]
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
            pr_base: &integration_basis,
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

    #[test]
    fn rejects_annotated_tag_object_ids() -> Result<()> {
        let scratch = tempdir()?;
        let repository = scratch.path().join("repository");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "--quiet"])?;
        git(&repository, &["config", "user.email", "test@example.invalid"])?;
        git(&repository, &["config", "user.name", "synthetic-test"])?;

        fs::write(repository.join("base.txt"), "base\n")?;
        git(&repository, &["add", "base.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "base"])?;
        git(&repository, &["tag", "-a", "release", "-m", "release"])?;
        let tag_object =
            git(&repository, &["rev-parse", "refs/tags/release^{tag}"])?.trim().to_owned();

        let error = validate_commit_object(&repository, "PR base", &tag_object)
            .err()
            .ok_or_else(|| eyre!("expected annotated tag object to be rejected"))?;
        assert!(error.to_string().contains("must name a commit object"));
        Ok(())
    }

    #[test]
    fn unchanged_trees_return_the_integration_basis_tree() -> Result<()> {
        let scratch = tempdir()?;
        let repository = scratch.path().join("repository");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "--quiet"])?;
        git(&repository, &["config", "user.email", "test@example.invalid"])?;
        git(&repository, &["config", "user.name", "synthetic-test"])?;

        fs::write(repository.join("base.txt"), "base\n")?;
        git(&repository, &["add", "base.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "base"])?;
        let base = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        let result = construct_synthetic_squash(SyntheticSquashRequest {
            repository: &repository,
            pr_base: &base,
            pr_head: &base,
            integration_basis: &base,
        })?;

        assert_eq!(result.synthetic_tree, git(&repository, &["rev-parse", "HEAD^{tree}"])?.trim());
        assert_eq!(result.cleanup, SyntheticCleanup::Complete);
        Ok(())
    }

    #[test]
    fn three_way_application_preserves_compatible_context_movement() -> Result<()> {
        let scratch = tempdir()?;
        let repository = scratch.path().join("repository");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "--quiet"])?;
        git(&repository, &["config", "user.email", "test@example.invalid"])?;
        git(&repository, &["config", "user.name", "synthetic-test"])?;

        fs::write(repository.join("shared.txt"), "one\ntwo\nthree\nfour\nfive\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "base"])?;
        git(&repository, &["branch", "integration"])?;

        fs::write(repository.join("shared.txt"), "one\ntwo\nthree\nfour\npr-five\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "candidate"])?;
        let pr_head = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        git(&repository, &["switch", "--quiet", "integration"])?;
        fs::write(repository.join("shared.txt"), "one\nintegration-two\nthree\nfour\nfive\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "integration"])?;
        let integration_basis = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        let result = construct_synthetic_squash(SyntheticSquashRequest {
            repository: &repository,
            pr_base: &integration_basis,
            pr_head: &pr_head,
            integration_basis: &integration_basis,
        })?;
        let merged = git(&repository, &["show", &format!("{}:shared.txt", result.synthetic_tree)])?;

        assert!(merged.contains("integration-two"));
        assert!(merged.contains("pr-five"));
        Ok(())
    }

    #[test]
    fn run_git_reports_nonzero_exit_status_and_stderr() -> Result<()> {
        let scratch = tempdir()?;
        let result = run_git(scratch.path(), &["rev-parse", "HEAD"]);
        let error = result.err().ok_or_else(|| eyre!("expected git failure"))?;

        let message = error.to_string();
        assert!(message.contains("git command failed"), "unexpected error: {message}");
        Ok(())
    }

    #[test]
    fn apply_patch_reports_rejected_input() -> Result<()> {
        let scratch = tempdir()?;
        let result = apply_patch(scratch.path(), b"not a git patch\n");
        let error = result.err().ok_or_else(|| eyre!("expected git apply failure"))?;

        let message = format!("{error:?}");
        assert!(message.contains("git apply failed"), "unexpected error: {message}");
        Ok(())
    }

    #[test]
    fn construction_reports_patch_failure_and_removes_worktree() -> Result<()> {
        let scratch = tempdir()?;
        let repository = scratch.path().join("repository");
        fs::create_dir(&repository)?;
        git(&repository, &["init", "--quiet"])?;
        git(&repository, &["config", "user.email", "test@example.invalid"])?;
        git(&repository, &["config", "user.name", "synthetic-test"])?;

        fs::write(repository.join("shared.txt"), "base\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "base"])?;
        let pr_base = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();
        git(&repository, &["branch", "integration"])?;

        fs::write(repository.join("shared.txt"), "candidate\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "candidate"])?;
        let pr_head = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        git(&repository, &["switch", "--quiet", "integration"])?;
        fs::write(repository.join("shared.txt"), "integration\n")?;
        git(&repository, &["add", "shared.txt"])?;
        git(&repository, &["commit", "--quiet", "-m", "integration"])?;
        let integration_basis = git(&repository, &["rev-parse", "HEAD"])?.trim().to_owned();

        let result = construct_synthetic_squash(SyntheticSquashRequest {
            repository: &repository,
            pr_base: &pr_base,
            pr_head: &pr_head,
            integration_basis: &integration_basis,
        });
        let error = result.err().ok_or_else(|| eyre!("expected patch conflict"))?;

        let message = format!("{error:?}");
        assert!(message.contains("git apply failed"), "unexpected error: {message}");
        let worktrees = git(&repository, &["worktree", "list"])?.lines().count();
        assert_eq!(worktrees, 1, "synthetic worktree should be removed after failure");
        Ok(())
    }
}
