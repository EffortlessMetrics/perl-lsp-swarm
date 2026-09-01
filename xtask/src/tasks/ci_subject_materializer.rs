//! Trusted, deterministic PR integration-subject materialization (#14512).
//!
//! This command runs from the base workflow checkout. It never checks out or
//! executes candidate source: the candidate is only fetched as an immutable
//! object and merged into a tree by Git.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Context, Result, eyre};
use serde::Serialize;
use serde_json::Value;

use super::ci_subject::{self, CiEventKind, CiSubjectConfig, SubjectInput};

const SCHEMA_VERSION: &str = "ci-subject-materialization.v1";
const PRODUCER: &str = "cargo-xtask-ci-subject-materializer";
const MECHANISM: &str = "git-merge-tree-write-tree";
const GIT_COMMIT_DATE: &str = "2000-01-01T00:00:00+0000";

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: &'static str,
    producer: &'static str,
    event_name: String,
    repository: String,
    event_base_sha: Option<String>,
    event_head_sha: Option<String>,
    fetched_head_sha: Option<String>,
    observed_merge_ref_sha: Option<String>,
    observed_merge_ref_parents: Vec<String>,
    derived_subject_sha: Option<String>,
    derived_subject_tree_sha: Option<String>,
    merge_mechanism: &'static str,
    git_version: String,
    outcome: &'static str,
    failure_stage: Option<String>,
    error: Option<String>,
}

pub struct Config {
    pub event_name: Option<String>,
    pub event_path: Option<PathBuf>,
    pub repository: Option<String>,
    pub github_sha: Option<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub receipt: PathBuf,
    pub env_file: Option<PathBuf>,
    pub root: Option<PathBuf>,
}

pub fn run(config: Config) -> Result<()> {
    let root = config.root.clone().unwrap_or(crate::utils::project_root()?);
    let event_name = config
        .event_name
        .clone()
        .or_else(|| std::env::var("GITHUB_EVENT_NAME").ok())
        .unwrap_or_else(|| "explicit".to_string());
    let repository = config
        .repository
        .clone()
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .unwrap_or_default();
    let git_version = git_version(&root).unwrap_or_else(|_| "unknown".to_string());
    let mut receipt = Receipt {
        schema_version: SCHEMA_VERSION,
        producer: PRODUCER,
        event_name: event_name.clone(),
        repository,
        event_base_sha: None,
        event_head_sha: None,
        fetched_head_sha: None,
        observed_merge_ref_sha: None,
        observed_merge_ref_parents: Vec::new(),
        derived_subject_sha: None,
        derived_subject_tree_sha: None,
        merge_mechanism: MECHANISM,
        git_version,
        outcome: "fail",
        failure_stage: None,
        error: None,
    };

    let result = materialize(&root, &config, &mut receipt);
    if let Err(error) = result {
        receipt.error = Some(error.to_string());
        write_receipt(&config.receipt, &receipt)?;
        return Err(error);
    }
    receipt.outcome = "pass";
    write_receipt(&config.receipt, &receipt)?;
    if let Some(path) = config.env_file {
        write_env(
            &path,
            receipt.derived_subject_sha.as_deref(),
            receipt.derived_subject_tree_sha.as_deref(),
        )?;
    }
    println!("ci subject materialization: PASS ({})", config.receipt.display());
    Ok(())
}

fn materialize(root: &Path, config: &Config, receipt: &mut Receipt) -> Result<()> {
    let subject_config = CiSubjectConfig {
        event_name: config.event_name.clone(),
        event_path: config.event_path.clone(),
        repository: config.repository.clone(),
        github_sha: config.github_sha.clone(),
        base_sha: config.base_sha.clone(),
        head_sha: config.head_sha.clone(),
        receipt: config.receipt.clone(),
        root: Some(root.to_path_buf()),
    };
    let input = ci_subject::input_from_config(&subject_config)
        .map_err(|error| stage_error(receipt, "event-input", error))?;
    receipt.event_base_sha = Some(input.base_sha.clone());
    receipt.event_head_sha = Some(input.head_sha.clone());

    ci_subject::validate_sha(&input.base_sha, "event base")
        .map_err(|error| stage_error(receipt, "base-validation", error))?;
    ci_subject::validate_sha(&input.head_sha, "event head")
        .map_err(|error| stage_error(receipt, "head-validation", error))?;
    ci_subject::ensure_commit(root, &input.base_sha)
        .map_err(|error| stage_error(receipt, "base-fetch", error))?;
    ci_subject::ensure_commit(root, &input.head_sha)
        .map_err(|error| stage_error(receipt, "head-fetch", error))?;

    if input.event_kind == CiEventKind::PullRequest {
        let event = read_event(config)?;
        let number = event
            .get("pull_request")
            .and_then(|value| value.get("number"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                stage_error(receipt, "head-ref", eyre!("pull request number is required"))
            })?;
        let refspec = format!("refs/pull/{number}/head");
        ci_subject::git_stdout(
            root,
            &["fetch", "--no-tags", "--no-write-fetch-head", "origin", &refspec],
        )
        .map_err(|error| stage_error(receipt, "head-ref", error))?;
        let fetched = ci_subject::git_stdout(root, &["rev-parse", "FETCH_HEAD^{commit}"])
            .map_err(|error| stage_error(receipt, "head-ref", error))?;
        receipt.fetched_head_sha = Some(fetched.clone());
        if fetched != input.head_sha {
            return Err(stage_error(
                receipt,
                "head-ref",
                eyre!(
                    "pull request head ref resolved to {fetched}, event head is {}",
                    input.head_sha
                ),
            ));
        }
    } else {
        receipt.fetched_head_sha = Some(input.head_sha.clone());
    }

    let tree =
        merge_tree(root, &input).map_err(|error| stage_error(receipt, "merge-tree", error))?;
    let subject = synthetic_commit(root, &input, &tree)
        .map_err(|error| stage_error(receipt, "subject-commit", error))?;
    receipt.derived_subject_tree_sha = Some(tree);
    receipt.derived_subject_sha = Some(subject);
    Ok(())
}

fn merge_tree(root: &Path, input: &SubjectInput) -> Result<String> {
    let output = Command::new("git")
        .args(["merge-tree", "--write-tree", &input.base_sha, &input.head_sha])
        .current_dir(root)
        .output()
        .context("running git merge-tree --write-tree")?;
    if !output.status.success() {
        return Err(eyre!(
            "git merge-tree reported a conflict: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        ));
    }
    let output_text =
        String::from_utf8(output.stdout).context("git merge-tree output is not UTF-8")?;
    let tree = output_text
        .lines()
        .next()
        .map(str::trim)
        .ok_or_else(|| eyre!("git merge-tree returned no tree"))?;
    ci_subject::validate_sha(tree, "derived tree").map_err(|error| eyre!(error))?;
    Ok(tree.to_string())
}

fn synthetic_commit(root: &Path, input: &SubjectInput, tree: &str) -> Result<String> {
    let mut child = Command::new("git")
        .args(["commit-tree", tree, "-p", &input.base_sha, "-p", &input.head_sha, "-F", "-"])
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "perl-lsp trusted subject")
        .env("GIT_AUTHOR_EMAIL", "ci-subject@invalid")
        .env("GIT_COMMITTER_NAME", "perl-lsp trusted subject")
        .env("GIT_COMMITTER_EMAIL", "ci-subject@invalid")
        .env("GIT_AUTHOR_DATE", GIT_COMMIT_DATE)
        .env("GIT_COMMITTER_DATE", GIT_COMMIT_DATE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting git commit-tree")?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or_else(|| eyre!("git commit-tree stdin unavailable"))?
        .write_all(b"perl-lsp trusted integration subject\n")?;
    let output = child.wait_with_output().context("waiting for git commit-tree")?;
    if !output.status.success() {
        return Err(eyre!(
            "git commit-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let subject = String::from_utf8(output.stdout)?.trim().to_string();
    ci_subject::validate_sha(&subject, "derived subject").map_err(|error| eyre!(error))?;
    Ok(subject)
}

fn read_event(config: &Config) -> Result<Value> {
    let path = config
        .event_path
        .clone()
        .or_else(|| std::env::var_os("GITHUB_EVENT_PATH").map(PathBuf::from))
        .ok_or_else(|| eyre!("GitHub event path is required"))?;
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).context("parsing GitHub event JSON")
}

fn git_version(root: &Path) -> Result<String> {
    ci_subject::git_stdout(root, &["--version"]).map_err(|error| eyre!(error))
}

fn stage_error(
    receipt: &mut Receipt,
    stage: &str,
    error: impl std::fmt::Display,
) -> color_eyre::eyre::Report {
    receipt.failure_stage = Some(stage.to_string());
    eyre!("{}", error)
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(receipt)?)?;
    Ok(())
}

fn write_env(path: &Path, subject: Option<&str>, tree: Option<&str>) -> Result<()> {
    let subject = subject.ok_or_else(|| eyre!("successful materialization has no subject SHA"))?;
    let tree = tree.ok_or_else(|| eyre!("successful materialization has no subject tree SHA"))?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "SUBJECT_SHA={subject}")?;
            writeln!(file, "SUBJECT_TREE_SHA={tree}")
        })
        .with_context(|| format!("writing GitHub environment file {}", path.display()))
}
