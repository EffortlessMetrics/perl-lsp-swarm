//! Read-only, fail-closed Git ancestry classification.
//!
//! A missing merge base is not proof of unrelated history when the checkout is
//! shallow, partial, or missing one of the requested commit objects. This module
//! keeps those evidence states distinct so callers cannot turn an incomplete
//! local graph into branch replay, closure, or history-recovery authority.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Versioned receipt schema emitted by the ancestry classifier.
pub const GIT_ANCESTRY_SCHEMA_VERSION: &str = "git_ancestry.v1";

/// The strongest ancestry disposition proved by the inspected local graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AncestryDisposition {
    /// The requested base commit is an ancestor of the requested head commit.
    Ancestor,
    /// Both commits are present and related, but base is not an ancestor of head.
    Diverged,
    /// Both commits are present in a complete-enough local graph with no merge base.
    Unrelated,
    /// The repository is shallow, so absence outside the retained boundary is unknown.
    NotProvenShallow,
    /// The repository is a partial/promisor clone, so omitted ancestry is possible.
    NotProvenPartialClone,
    /// One or both requested commit objects cannot be resolved locally.
    NotProvenMissingObject,
    /// The caller supplied an empty or option-like revision value.
    InvalidInput,
    /// Git or repository inspection failed before a domain result could be proved.
    InstrumentFailure,
}

impl AncestryDisposition {
    /// Stable machine spelling used by human and JSON projections.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ancestor => "ancestor",
            Self::Diverged => "diverged",
            Self::Unrelated => "unrelated",
            Self::NotProvenShallow => "not_proven_shallow",
            Self::NotProvenPartialClone => "not_proven_partial_clone",
            Self::NotProvenMissingObject => "not_proven_missing_object",
            Self::InvalidInput => "invalid_input",
            Self::InstrumentFailure => "instrument_failure",
        }
    }

    /// Stable process exit code for shell and workflow consumers.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Ancestor | Self::Diverged => 0,
            Self::Unrelated => 2,
            Self::NotProvenShallow | Self::NotProvenPartialClone | Self::NotProvenMissingObject => {
                3
            }
            Self::InvalidInput | Self::InstrumentFailure => 4,
        }
    }
}

/// Exact local observations and the ancestry proposition they support.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AncestryReceipt {
    /// Receipt schema identity.
    pub schema_version: String,
    /// Caller-supplied repository path.
    pub repository: String,
    /// Git-reported repository root, when inspection reached it.
    pub repository_root: Option<String>,
    /// Git directory identity, when available.
    pub git_dir: Option<String>,
    /// Common Git directory identity, when available.
    pub git_common_dir: Option<String>,
    /// Caller-supplied base revision.
    pub base: String,
    /// Caller-supplied head revision.
    pub head: String,
    /// Resolved base commit SHA, when available.
    pub base_sha: Option<String>,
    /// Resolved head commit SHA, when available.
    pub head_sha: Option<String>,
    /// Proven merge base, when one exists.
    pub merge_base: Option<String>,
    /// Whether Git reports a shallow repository.
    pub is_shallow_repository: Option<bool>,
    /// Whether local Git configuration declares a promisor/partial clone.
    pub is_partial_clone: Option<bool>,
    /// Whether the base commit object resolved locally.
    pub base_object_exists: bool,
    /// Whether the head commit object resolved locally.
    pub head_object_exists: bool,
    /// `base` is an ancestor of `head`, when the complete relation was tested.
    pub base_is_ancestor_of_head: Option<bool>,
    /// `head` is an ancestor of `base`, when the complete relation was tested.
    pub head_is_ancestor_of_base: Option<bool>,
    /// Strongest proved disposition.
    pub disposition: AncestryDisposition,
    /// Bounded explanation of the disposition.
    pub reason: String,
    /// Operator actions that can make incomplete evidence decidable.
    pub guidance: Vec<String>,
    /// Local evidence limitations that prevent a stronger proposition.
    pub limitations: Vec<String>,
}

impl AncestryReceipt {
    fn new(repository: &Path, base: &str, head: &str) -> Self {
        Self {
            schema_version: GIT_ANCESTRY_SCHEMA_VERSION.to_string(),
            repository: display_path(repository),
            repository_root: None,
            git_dir: None,
            git_common_dir: None,
            base: base.to_string(),
            head: head.to_string(),
            base_sha: None,
            head_sha: None,
            merge_base: None,
            is_shallow_repository: None,
            is_partial_clone: None,
            base_object_exists: false,
            head_object_exists: false,
            base_is_ancestor_of_head: None,
            head_is_ancestor_of_base: None,
            disposition: AncestryDisposition::InstrumentFailure,
            reason: "repository inspection did not complete".to_string(),
            guidance: Vec::new(),
            limitations: Vec::new(),
        }
    }

    fn finish(mut self, disposition: AncestryDisposition, reason: impl Into<String>) -> Self {
        self.disposition = disposition;
        self.reason = reason.into();
        self
    }

    /// Stable human projection of the same receipt used for JSON output.
    #[must_use]
    pub fn render_human(&self) -> String {
        let mut lines = vec![
            format!("git-ancestry: {}", self.disposition.as_str()),
            format!("repository: {}", self.repository),
            format!("base: {} -> {}", self.base, display_option(self.base_sha.as_deref())),
            format!("head: {} -> {}", self.head, display_option(self.head_sha.as_deref())),
            format!("merge-base: {}", display_option(self.merge_base.as_deref())),
            format!("shallow: {}", display_bool_option(self.is_shallow_repository)),
            format!("partial-clone: {}", display_bool_option(self.is_partial_clone)),
            format!("reason: {}", self.reason),
        ];
        lines.extend(self.guidance.iter().map(|item| format!("guidance: {item}")));
        lines.extend(self.limitations.iter().map(|item| format!("limitation: {item}")));
        lines.push(String::new());
        lines.join("\n")
    }
}

#[derive(Debug)]
struct GitOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl GitOutput {
    fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    fn no_match(&self) -> bool {
        self.code == Some(1)
    }

    fn diagnostic(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.lines().next().unwrap_or(stderr).to_string();
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return stdout.lines().next().unwrap_or(stdout).to_string();
        }
        match self.code {
            Some(code) => format!("git exited with status {code}"),
            None => "git terminated without an exit status".to_string(),
        }
    }
}

/// Classify one exact base/head pair without fetching or mutating repository state.
#[must_use]
pub fn classify_ancestry(repository: &Path, base: &str, head: &str) -> AncestryReceipt {
    let mut receipt = AncestryReceipt::new(repository, base, head);

    if let Some(problem) = invalid_revision(base, "base").or_else(|| invalid_revision(head, "head"))
    {
        return receipt.finish(AncestryDisposition::InvalidInput, problem);
    }

    let repository_root = match run_git(repository, &["rev-parse", "--show-toplevel"]) {
        Ok(output) if output.succeeded() => output.stdout.trim().to_string(),
        Ok(output) => {
            receipt.limitations.push(output.diagnostic());
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "the requested path is not an inspectable Git worktree",
            );
        }
        Err(error) => {
            receipt.limitations.push(error);
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "Git could not be executed for repository inspection",
            );
        }
    };
    receipt.repository_root = Some(normalize_path_text(&repository_root));
    receipt.git_dir = git_path_observation(repository, "--git-dir");
    receipt.git_common_dir = git_path_observation(repository, "--git-common-dir");

    let shallow = match run_git(repository, &["rev-parse", "--is-shallow-repository"]) {
        Ok(output) if output.succeeded() => match parse_git_bool(&output.stdout) {
            Some(value) => value,
            None => {
                receipt.limitations.push(format!(
                    "unexpected shallow-repository value {:?}",
                    output.stdout.trim()
                ));
                return receipt.finish(
                    AncestryDisposition::InstrumentFailure,
                    "Git returned an invalid shallow-repository observation",
                );
            }
        },
        Ok(output) => {
            receipt.limitations.push(output.diagnostic());
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "Git could not determine whether the repository is shallow",
            );
        }
        Err(error) => {
            receipt.limitations.push(error);
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "Git could not determine whether the repository is shallow",
            );
        }
    };
    receipt.is_shallow_repository = Some(shallow);

    let partial = match partial_clone_observation(repository) {
        Ok(value) => value,
        Err(error) => {
            receipt.limitations.push(error);
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "Git could not determine partial-clone state",
            );
        }
    };
    receipt.is_partial_clone = Some(partial);

    if shallow {
        receipt.guidance.push(
            "deepen the checkout before making ancestry-based replay or closure decisions, for example `git fetch --unshallow` or `git fetch --deepen=<n>`"
                .to_string(),
        );
        receipt.limitations.push(
            "the local shallow boundary can hide a real merge base and make an interior commit appear to be a root"
                .to_string(),
        );
        return receipt.finish(
            AncestryDisposition::NotProvenShallow,
            "the checkout is shallow; local absence is not proof of unrelated history",
        );
    }

    if partial {
        receipt.guidance.push(
            "materialize the required commit graph in a complete clone before making ancestry-based replay or closure decisions"
                .to_string(),
        );
        receipt.limitations.push(
            "promisor/partial-clone configuration allows required ancestry objects to be absent locally"
                .to_string(),
        );
        return receipt.finish(
            AncestryDisposition::NotProvenPartialClone,
            "the checkout is partial; local absence is not proof of unrelated history",
        );
    }

    receipt.base_sha = resolve_commit(repository, base);
    receipt.head_sha = resolve_commit(repository, head);
    receipt.base_object_exists = receipt.base_sha.is_some();
    receipt.head_object_exists = receipt.head_sha.is_some();

    if !receipt.base_object_exists || !receipt.head_object_exists {
        let missing = match (receipt.base_object_exists, receipt.head_object_exists) {
            (false, false) => "base and head commit objects are unavailable",
            (false, true) => "the base commit object is unavailable",
            (true, false) => "the head commit object is unavailable",
            (true, true) => "a required commit object is unavailable",
        };
        receipt.guidance.push(
            "fetch or otherwise materialize the exact missing commit objects, then rerun the classifier"
                .to_string(),
        );
        receipt.limitations.push(
            "an unresolved revision cannot distinguish a bad ref from locally missing retained history"
                .to_string(),
        );
        return receipt.finish(AncestryDisposition::NotProvenMissingObject, missing);
    }

    let (base_sha, head_sha) = match (receipt.base_sha.as_deref(), receipt.head_sha.as_deref()) {
        (Some(base_sha), Some(head_sha)) => (base_sha.to_string(), head_sha.to_string()),
        _ => {
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "resolved commit identities were lost before ancestry classification",
            );
        }
    };

    let merge_base = match run_git(repository, &["merge-base", &base_sha, &head_sha]) {
        Ok(output) if output.succeeded() => Some(output.stdout.trim().to_string()),
        Ok(output) if output.no_match() => None,
        Ok(output) => {
            receipt.limitations.push(output.diagnostic());
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "git merge-base failed without proving related or unrelated history",
            );
        }
        Err(error) => {
            receipt.limitations.push(error);
            return receipt.finish(
                AncestryDisposition::InstrumentFailure,
                "git merge-base could not be executed",
            );
        }
    };

    let Some(merge_base) = merge_base else {
        return receipt.finish(
            AncestryDisposition::Unrelated,
            "both commit objects are present in a non-shallow, non-partial graph and no merge base exists",
        );
    };
    // A merge base equal to one of the requested commits means that commit is an
    // ancestor of the other, so no extra `git merge-base --is-ancestor` process is
    // needed to establish either direction of the relation.
    let base_is_ancestor = merge_base == base_sha;
    let head_is_ancestor = merge_base == head_sha;
    receipt.merge_base = Some(merge_base);
    receipt.base_is_ancestor_of_head = Some(base_is_ancestor);
    receipt.head_is_ancestor_of_base = Some(head_is_ancestor);

    if base_is_ancestor {
        receipt.finish(
            AncestryDisposition::Ancestor,
            "the requested base is an ancestor of the requested head",
        )
    } else if head_is_ancestor {
        receipt.finish(
            AncestryDisposition::Diverged,
            "the histories are related, but the requested head is behind the requested base",
        )
    } else {
        receipt.finish(
            AncestryDisposition::Diverged,
            "the histories share a merge base but neither requested commit contains the other",
        )
    }
}

fn invalid_revision(value: &str, role: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(format!("{role} revision is empty"));
    }
    if value.starts_with('-') {
        return Some(format!("{role} revision must not be option-like"));
    }
    if value.chars().any(|character| matches!(character, '\0' | '\n' | '\r')) {
        return Some(format!("{role} revision contains a prohibited control character"));
    }
    None
}

fn git_path_observation(repository: &Path, flag: &str) -> Option<String> {
    match run_git(repository, &["rev-parse", flag]) {
        Ok(output) if output.succeeded() => {
            Some(normalize_git_path(repository, output.stdout.trim()))
        }
        _ => None,
    }
}

fn normalize_git_path(repository: &Path, value: &str) -> String {
    let path = PathBuf::from(value);
    let joined = if path.is_absolute() { path } else { repository.join(path) };
    normalize_path_text(&joined.to_string_lossy())
}

fn partial_clone_observation(repository: &Path) -> Result<bool, String> {
    // No `--local`: promisor configuration can also live in worktree-specific
    // config when `extensions.worktreeConfig` is enabled, and missing it would
    // let a partial clone be misclassified as complete.
    let output = run_git(repository, &["config", "--get-regexp", r"^remote\..*\.promisor$"])?;
    if output.succeeded() {
        return Ok(!output.stdout.trim().is_empty());
    }
    if output.no_match() {
        return Ok(false);
    }
    Err(format!("partial-clone probe failed: {}", output.diagnostic()))
}

fn resolve_commit(repository: &Path, revision: &str) -> Option<String> {
    let specification = format!("{revision}^{{commit}}");
    match run_git(repository, &["rev-parse", "--verify", "--end-of-options", &specification]) {
        Ok(output) if output.succeeded() => {
            let sha = output.stdout.trim();
            (!sha.is_empty()).then(|| sha.to_string())
        }
        _ => None,
    }
}

fn run_git(repository: &Path, arguments: &[&str]) -> Result<GitOutput, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .map_err(|error| format!("failed to execute git {}: {error}", arguments.join(" ")))?;
    Ok(GitOutput {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn parse_git_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn display_option(value: Option<&str>) -> &str {
    value.unwrap_or("not_available")
}

fn display_bool_option(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "not_available",
    }
}

fn display_path(path: &Path) -> String {
    normalize_path_text(&path.to_string_lossy())
}

fn normalize_path_text(value: &str) -> String {
    value.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, bail};
    use std::fs;

    #[test]
    fn complete_linear_history_is_ancestor() -> Result<()> {
        let repository = initialized_repository()?;
        let base = git(&repository, &["rev-parse", "HEAD"])?;
        commit_file(&repository, "second.txt", "second\n", "second")?;

        let receipt = classify_ancestry(repository.path(), &base, "HEAD");

        assert_eq!(receipt.disposition, AncestryDisposition::Ancestor);
        assert_eq!(receipt.base_is_ancestor_of_head, Some(true));
        assert_eq!(receipt.disposition.exit_code(), 0);
        Ok(())
    }

    #[test]
    fn related_sibling_branches_are_diverged() -> Result<()> {
        let repository = initialized_repository()?;
        let base = git(&repository, &["rev-parse", "HEAD"])?;
        git(&repository, &["switch", "-c", "left", &base])?;
        commit_file(&repository, "left.txt", "left\n", "left")?;
        let left = git(&repository, &["rev-parse", "HEAD"])?;
        git(&repository, &["switch", "-c", "right", &base])?;
        commit_file(&repository, "right.txt", "right\n", "right")?;
        let right = git(&repository, &["rev-parse", "HEAD"])?;

        let receipt = classify_ancestry(repository.path(), &left, &right);

        assert_eq!(receipt.disposition, AncestryDisposition::Diverged);
        assert!(receipt.merge_base.is_some());
        assert_eq!(receipt.base_is_ancestor_of_head, Some(false));
        assert_eq!(receipt.head_is_ancestor_of_base, Some(false));
        Ok(())
    }

    #[test]
    fn complete_orphan_histories_are_unrelated() -> Result<()> {
        let repository = initialized_repository()?;
        let original = git(&repository, &["rev-parse", "HEAD"])?;
        git(&repository, &["switch", "--orphan", "orphan"])?;
        git(&repository, &["rm", "-rf", "--ignore-unmatch", "."])?;
        commit_file(&repository, "orphan.txt", "orphan\n", "orphan")?;
        let orphan = git(&repository, &["rev-parse", "HEAD"])?;

        let receipt = classify_ancestry(repository.path(), &original, &orphan);

        assert_eq!(receipt.disposition, AncestryDisposition::Unrelated);
        assert!(receipt.merge_base.is_none());
        assert_eq!(receipt.disposition.exit_code(), 2);
        Ok(())
    }

    #[test]
    fn shallow_clone_is_not_proven_before_ref_resolution() -> Result<()> {
        let source = initialized_repository()?;
        commit_file(&source, "second.txt", "second\n", "second")?;
        commit_file(&source, "third.txt", "third\n", "third")?;
        let clone_parent = tempfile::tempdir()?;
        let clone = clone_parent.path().join("repository");
        let source_arg = source.path().to_string_lossy().into_owned();
        let clone_arg = clone.to_string_lossy().into_owned();
        git_at(
            clone_parent.path(),
            &["clone", "--depth", "1", "--no-local", &source_arg, &clone_arg],
        )?;

        let receipt = classify_ancestry(&clone, "HEAD~2", "HEAD");

        assert_eq!(receipt.disposition, AncestryDisposition::NotProvenShallow);
        assert_eq!(receipt.is_shallow_repository, Some(true));
        assert!(receipt.reason.contains("not proof of unrelated history"));
        assert_eq!(receipt.disposition.exit_code(), 3);
        Ok(())
    }

    #[test]
    fn promisor_configuration_is_not_proven() -> Result<()> {
        let repository = initialized_repository()?;
        git(&repository, &["config", "remote.origin.promisor", "true"])?;

        let receipt = classify_ancestry(repository.path(), "HEAD", "HEAD");

        assert_eq!(receipt.disposition, AncestryDisposition::NotProvenPartialClone);
        assert_eq!(receipt.is_partial_clone, Some(true));
        Ok(())
    }

    #[test]
    fn missing_commit_object_is_not_unrelated() -> Result<()> {
        let repository = initialized_repository()?;

        let receipt = classify_ancestry(
            repository.path(),
            "1111111111111111111111111111111111111111",
            "HEAD",
        );

        assert_eq!(receipt.disposition, AncestryDisposition::NotProvenMissingObject);
        assert!(!receipt.base_object_exists);
        assert!(receipt.head_object_exists);
        Ok(())
    }

    #[test]
    fn option_like_revision_is_invalid_input() -> Result<()> {
        let repository = initialized_repository()?;

        let receipt = classify_ancestry(repository.path(), "--all", "HEAD");

        assert_eq!(receipt.disposition, AncestryDisposition::InvalidInput);
        assert_eq!(receipt.disposition.exit_code(), 4);
        Ok(())
    }

    #[test]
    fn non_repository_is_instrument_failure() -> Result<()> {
        let directory = tempfile::tempdir()?;

        let receipt = classify_ancestry(directory.path(), "HEAD", "HEAD");

        assert_eq!(receipt.disposition, AncestryDisposition::InstrumentFailure);
        assert!(receipt.repository_root.is_none());
        Ok(())
    }

    #[test]
    fn classification_does_not_mutate_refs_index_or_worktree() -> Result<()> {
        let repository = initialized_repository()?;
        fs::write(repository.path().join("untracked.txt"), "untracked\n")?;
        let refs_before = git(&repository, &["show-ref", "--head"])?;
        let status_before = git(&repository, &["status", "--porcelain=v1"])?;
        let index_before = fs::read(repository.path().join(".git/index"))?;

        let receipt = classify_ancestry(repository.path(), "HEAD", "HEAD");

        let refs_after = git(&repository, &["show-ref", "--head"])?;
        let status_after = git(&repository, &["status", "--porcelain=v1"])?;
        let index_after = fs::read(repository.path().join(".git/index"))?;
        assert_eq!(receipt.disposition, AncestryDisposition::Ancestor);
        assert_eq!(refs_before, refs_after);
        assert_eq!(status_before, status_after);
        assert_eq!(index_before, index_after);
        Ok(())
    }

    #[test]
    fn human_projection_preserves_subject_and_limitations() -> Result<()> {
        let repository = initialized_repository()?;
        let mut receipt = classify_ancestry(
            repository.path(),
            "1111111111111111111111111111111111111111",
            "HEAD",
        );
        receipt.limitations.push("bounded limitation".to_string());

        let rendered = receipt.render_human();

        assert!(rendered.contains("not_proven_missing_object"));
        assert!(rendered.contains("1111111111111111111111111111111111111111"));
        assert!(rendered.contains("bounded limitation"));
        Ok(())
    }

    fn initialized_repository() -> Result<tempfile::TempDir> {
        let repository = tempfile::tempdir()?;
        git(&repository, &["init", "--initial-branch", "main"])?;
        git(&repository, &["config", "user.name", "test"])?;
        git(&repository, &["config", "user.email", "test@example.com"])?;
        commit_file(&repository, "tracked.txt", "base\n", "base")?;
        Ok(repository)
    }

    fn commit_file(
        repository: &tempfile::TempDir,
        path: &str,
        contents: &str,
        message: &str,
    ) -> Result<()> {
        fs::write(repository.path().join(path), contents)?;
        git(repository, &["add", "--", path])?;
        git(repository, &["commit", "-m", message])?;
        Ok(())
    }

    fn git(repository: &tempfile::TempDir, arguments: &[&str]) -> Result<String> {
        git_at(repository.path(), arguments)
    }

    fn git_at(repository: &Path, arguments: &[&str]) -> Result<String> {
        let output = Command::new("git").args(arguments).current_dir(repository).output()?;
        if !output.status.success() {
            bail!(
                "git {} failed with status {}\nstdout:\n{}\nstderr:\n{}",
                arguments.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout)
            .context("git command returned non-UTF-8 output")
            .map(|value| value.trim().to_string())
    }
}
