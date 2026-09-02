//! Trusted, deterministic PR integration-subject materialization (#14512).
//!
//! This command runs from the base workflow checkout. It never checks out or
//! executes candidate source: the candidate is only fetched as an immutable
//! object and merged into a tree by Git.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Context, Result, eyre};
use serde::Serialize;
use serde_json::Value;

use super::ci_subject::{self, CiEventKind, CiSubjectConfig, SubjectInput};

const SCHEMA_VERSION: &str = "ci-subject-materialization.v1";
const PRODUCER: &str = "cargo-xtask-ci-subject-materializer";
const MECHANISM: &str = "git-merge-tree-write-tree";
const GIT_COMMIT_DATE: &str = "2000-01-01T00:00:00+0000";
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETAINED_GIT_OUTPUT: usize = 64 * 1024;

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
    if let Some(path) = config.env_file {
        if let Err(error) = write_env(
            &path,
            receipt.derived_subject_sha.as_deref(),
            receipt.derived_subject_tree_sha.as_deref(),
        ) {
            receipt.failure_stage = Some("environment-export".to_string());
            receipt.error = Some(error.to_string());
            write_receipt(&config.receipt, &receipt)?;
            return Err(error);
        }
    }
    receipt.outcome = "pass";
    write_receipt(&config.receipt, &receipt)?;
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
    let local_repository = ci_subject::repository_identity(root)
        .map_err(|error| stage_error(receipt, "repository", error))?;
    ci_subject::ensure_repository(&input.repository, &local_repository)
        .map_err(|error| stage_error(receipt, "repository", error))?;
    receipt.event_base_sha = Some(input.base_sha.clone());
    receipt.event_head_sha = Some(input.head_sha.clone());

    ci_subject::validate_sha(&input.base_sha, "event base")
        .map_err(|error| stage_error(receipt, "base-validation", error))?;
    ci_subject::validate_sha(&input.head_sha, "event head")
        .map_err(|error| stage_error(receipt, "head-validation", error))?;
    ci_subject::ensure_commit(root, &input.base_sha)
        .map_err(|error| stage_error(receipt, "base-fetch", error))?;
    if input.event_kind != CiEventKind::PullRequest {
        ci_subject::ensure_commit(root, &input.head_sha)
            .map_err(|error| stage_error(receipt, "head-fetch", error))?;
    }

    if input.event_kind == CiEventKind::PullRequest {
        let event =
            read_event(config).map_err(|error| stage_error(receipt, "event-input", error))?;
        let number = event
            .get("pull_request")
            .and_then(|value| value.get("number"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                stage_error(receipt, "head-ref", eyre!("pull request number is required"))
            })?;
        let refspec = format!("refs/pull/{number}/head");
        let local_head_ref = "refs/ci-subject/pr-head";
        let head_refspec = format!("+{refspec}:{local_head_ref}");
        git_stdout_bounded(root, &["fetch", "--no-tags", "origin", &head_refspec])
            .map_err(|error| stage_error(receipt, "head-ref", error))?;
        let fetched =
            git_stdout_bounded(root, &["rev-parse", &format!("{local_head_ref}^{{commit}}")])
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
        ci_subject::ensure_commit(root, &input.head_sha)
            .map_err(|error| stage_error(receipt, "head-fetch", error))?;
        let local_merge_ref = "refs/ci-subject/pr-merge";
        let merge_refspec = format!("+refs/pull/{number}/merge:{local_merge_ref}");
        if git_stdout_bounded(root, &["fetch", "--no-tags", "origin", &merge_refspec]).is_ok()
            && let Ok(observed) =
                git_stdout_bounded(root, &["rev-parse", &format!("{local_merge_ref}^{{commit}}")])
        {
            receipt.observed_merge_ref_sha = Some(observed.clone());
            if let Ok(parents) =
                git_stdout_bounded(root, &["rev-list", "--parents", "-n", "1", &observed])
            {
                receipt.observed_merge_ref_parents =
                    parents.split_whitespace().skip(1).map(str::to_string).collect();
            }
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
    let output = run_git_bounded(
        Command::new("git")
            .args(["merge-tree", "--write-tree", &input.base_sha, &input.head_sha])
            .current_dir(root),
        None,
    )
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
    let mut command = Command::new("git");
    let output = run_git_bounded(
        command
            .args(["commit-tree", tree, "-p", &input.base_sha, "-p", &input.head_sha, "-F", "-"])
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "perl-lsp trusted subject")
            .env("GIT_AUTHOR_EMAIL", "ci-subject@invalid")
            .env("GIT_COMMITTER_NAME", "perl-lsp trusted subject")
            .env("GIT_COMMITTER_EMAIL", "ci-subject@invalid")
            .env("GIT_AUTHOR_DATE", GIT_COMMIT_DATE)
            .env("GIT_COMMITTER_DATE", GIT_COMMIT_DATE),
        Some(b"perl-lsp trusted integration subject\n"),
    )
    .context("running git commit-tree")?;
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

fn run_git_bounded(command: &mut Command, input: Option<&[u8]>) -> Result<Output> {
    let mut child = command
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| eyre!("git stdout unavailable"))?;
    let stderr = child.stderr.take().ok_or_else(|| eyre!("git stderr unavailable"))?;
    let stdout_thread = thread::spawn(|| drain_output(stdout));
    let stderr_thread = thread::spawn(|| drain_output(stderr));
    if let Some(input) = input {
        let mut stdin = child.stdin.take().ok_or_else(|| eyre!("git command stdin unavailable"))?;
        if let Err(error) = stdin.write_all(input) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(error.into());
        }
        drop(stdin);
    }
    let status = wait_bounded(&mut child)?;
    let stdout = stdout_thread.join().map_err(|_| eyre!("git stdout reader panicked"))?;
    let stderr = stderr_thread.join().map_err(|_| eyre!("git stderr reader panicked"))?;
    Ok(Output { status, stdout, stderr })
}

fn drain_output(mut reader: impl Read) -> Vec<u8> {
    let mut retained = Vec::with_capacity(MAX_RETAINED_GIT_OUTPUT);
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = MAX_RETAINED_GIT_OUTPUT.saturating_sub(retained.len());
                retained.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    retained
}

fn wait_bounded(child: &mut Child) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + GIT_COMMAND_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            return Err(eyre!("git command exceeded {} seconds", GIT_COMMAND_TIMEOUT.as_secs()));
        }
        thread::sleep(Duration::from_millis(25));
    }
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
    git_stdout_bounded(root, &["--version"])
}

fn git_stdout_bounded(root: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_bounded(Command::new("git").args(args).current_dir(root), None)
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(eyre!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|stdout| stdout.trim().to_string())
        .context("git command output is not UTF-8")
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
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| eyre!("receipt path has no file name"))?;
    let temporary =
        parent.join(format!(".{}.tmp-{}", file_name.to_string_lossy(), std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(receipt)?)?;
    replace_receipt_file(&temporary, path)?;
    Ok(())
}

fn replace_receipt_file(temporary: &Path, destination: &Path) -> Result<()> {
    replace_receipt_file_with(temporary, destination, |from, to| fs::rename(from, to))
}

fn replace_receipt_file_with<F>(temporary: &Path, destination: &Path, mut rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    match rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(first_error) if destination.is_file() => {
            let file_name =
                destination.file_name().ok_or_else(|| eyre!("receipt path has no file name"))?;
            let backup = destination
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .join(format!(".{}.bak-{}", file_name.to_string_lossy(), std::process::id()));

            rename(destination, &backup).with_context(|| {
                format!("preserving existing receipt after replacement failed: {}", first_error)
            })?;

            match rename(temporary, destination) {
                Ok(()) => {
                    fs::remove_file(&backup)
                        .with_context(|| format!("removing receipt backup {}", backup.display()))?;
                    Ok(())
                }
                Err(replacement_error) => {
                    if let Err(restore_error) = rename(&backup, destination) {
                        return Err(eyre!(
                            "receipt replacement failed ({replacement_error}); restoring prior receipt failed ({restore_error}); prior receipt preserved at {}",
                            backup.display()
                        ));
                    }
                    Err(eyre!("receipt replacement failed: {replacement_error}"))
                }
            }
        }
        Err(error) => Err(error.into()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure, eyre};

    fn git(root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git").args(args).current_dir(root).output()?;
        if !output.status.success() {
            return Err(eyre!("git {} failed", args.join(" ")));
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    #[test]
    fn explicit_subject_is_stable_and_exports_identity() -> Result<()> {
        let temp = tempfile::tempdir()?;
        git(temp.path(), &["init", "--quiet"])?;
        git(temp.path(), &["config", "user.name", "test"])?;
        git(temp.path(), &["config", "user.email", "test@example.invalid"])?;
        git(temp.path(), &["remote", "add", "origin", "https://github.com/owner/repo.git"])?;
        fs::write(temp.path().join("tracked.txt"), "base\n")?;
        git(temp.path(), &["add", "tracked.txt"])?;
        git(temp.path(), &["commit", "--quiet", "-m", "base"])?;
        let base = git(temp.path(), &["rev-parse", "HEAD"])?;
        fs::write(temp.path().join("tracked.txt"), "head\n")?;
        git(temp.path(), &["add", "tracked.txt"])?;
        git(temp.path(), &["commit", "--quiet", "-m", "head"])?;
        let head = git(temp.path(), &["rev-parse", "HEAD"])?;

        let first = temp.path().join("first.json");
        let first_env = temp.path().join("first.env");
        let config = || Config {
            event_name: Some("explicit".to_string()),
            event_path: None,
            repository: Some("owner/repo".to_string()),
            github_sha: None,
            base_sha: Some(base.clone()),
            head_sha: Some(head.clone()),
            receipt: first.clone(),
            env_file: Some(first_env.clone()),
            root: Some(temp.path().to_path_buf()),
        };
        run(config())?;
        let first_value: Value = serde_json::from_slice(&fs::read(&first)?)?;
        let subject = first_value["derived_subject_sha"]
            .as_str()
            .ok_or_else(|| eyre!("receipt omitted subject SHA"))?
            .to_string();
        let tree = first_value["derived_subject_tree_sha"]
            .as_str()
            .ok_or_else(|| eyre!("receipt omitted subject tree SHA"))?
            .to_string();
        ensure!(first_value["outcome"] == "pass");
        ensure!(fs::read_to_string(&first_env)?.contains(&format!("SUBJECT_SHA={subject}")));
        ensure!(fs::read_to_string(&first_env)?.contains(&format!("SUBJECT_TREE_SHA={tree}")));

        let second = temp.path().join("second.json");
        let second_env = temp.path().join("second.env");
        let mut second_config = config();
        second_config.receipt = second.clone();
        second_config.env_file = Some(second_env);
        run(second_config)?;
        let second_value: Value = serde_json::from_slice(&fs::read(second)?)?;
        ensure!(second_value["derived_subject_sha"] == subject);
        ensure!(second_value["derived_subject_tree_sha"] == tree);
        ensure!(second_value["event_base_sha"] == base);
        ensure!(second_value["event_head_sha"] == head);
        Ok(())
    }

    #[test]
    fn pull_request_target_materializes_exact_head_and_rejects_stale_ref() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let remote = temp.path().join("remote.git");
        git(temp.path(), &["init", "--quiet"])?;
        git(temp.path(), &["config", "user.name", "test"])?;
        git(temp.path(), &["config", "user.email", "test@example.invalid"])?;
        git(
            temp.path(),
            &["init", "--bare", remote.to_str().ok_or_else(|| eyre!("remote path is not UTF-8"))?],
        )?;
        git(temp.path(), &["remote", "add", "origin", "https://github.com/owner/repo.git"])?;
        // Git's URL rewrite needs the actual local remote path as its value;
        // configure it separately because the path contains platform-specific
        // separators on Windows.
        git(
            temp.path(),
            &[
                "config",
                &format!("url.{}.insteadOf", remote.to_string_lossy()),
                "https://github.com/owner/repo.git",
            ],
        )?;
        fs::write(temp.path().join("tracked.txt"), "base\n")?;
        git(temp.path(), &["add", "tracked.txt"])?;
        git(temp.path(), &["commit", "--quiet", "-m", "base"])?;
        let base = git(temp.path(), &["rev-parse", "HEAD"])?;
        fs::write(temp.path().join("tracked.txt"), "head\n")?;
        git(temp.path(), &["add", "tracked.txt"])?;
        git(temp.path(), &["commit", "--quiet", "-m", "head"])?;
        let head = git(temp.path(), &["rev-parse", "HEAD"])?;
        let root_path =
            temp.path().to_str().ok_or_else(|| eyre!("fixture root path is not UTF-8"))?;
        git(&remote, &["fetch", "--quiet", root_path, "HEAD:refs/heads/fixture-head"])?;
        git(&remote, &["update-ref", "refs/pull/42/head", &head])?;

        let event_path = temp.path().join("event.json");
        fs::write(
            &event_path,
            serde_json::json!({
                "repository": {"full_name": "owner/repo"},
                "pull_request": {
                    "number": 42,
                    "base": {"sha": base, "repo": {"full_name": "owner/repo"}},
                    "head": {"sha": head, "repo": {"full_name": "fork/repo"}}
                }
            })
            .to_string(),
        )?;
        let receipt = temp.path().join("receipt.json");
        run(Config {
            event_name: Some("pull_request_target".to_string()),
            event_path: Some(event_path.clone()),
            repository: Some("owner/repo".to_string()),
            github_sha: None,
            base_sha: None,
            head_sha: None,
            receipt: receipt.clone(),
            env_file: None,
            root: Some(temp.path().to_path_buf()),
        })?;
        let value: Value = serde_json::from_slice(&fs::read(&receipt)?)?;
        ensure!(value["outcome"] == "pass");
        ensure!(value["event_head_sha"] == head);
        ensure!(value["fetched_head_sha"] == head);
        let subject =
            value["derived_subject_sha"].as_str().ok_or_else(|| eyre!("subject missing"))?;
        ensure!(git(temp.path(), &["rev-parse", &format!("{subject}^1")])? == base);
        ensure!(git(temp.path(), &["rev-parse", &format!("{subject}^2")])? == head);

        git(&remote, &["update-ref", "refs/pull/42/head", &base])?;
        let stale_receipt = temp.path().join("stale.json");
        let error = run(Config {
            event_name: Some("pull_request_target".to_string()),
            event_path: Some(event_path),
            repository: Some("owner/repo".to_string()),
            github_sha: None,
            base_sha: None,
            head_sha: None,
            receipt: stale_receipt.clone(),
            env_file: None,
            root: Some(temp.path().to_path_buf()),
        })
        .expect_err("stale PR head must fail closed");
        ensure!(error.to_string().contains("pull request head ref resolved"));
        let stale: Value = serde_json::from_slice(&fs::read(stale_receipt)?)?;
        ensure!(stale["outcome"] == "fail");
        ensure!(stale["failure_stage"] == "head-ref");
        Ok(())
    }

    #[test]
    fn missing_input_retains_typed_failure_receipt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipt = temp.path().join("failure.json");
        let config = Config {
            event_name: Some("explicit".to_string()),
            event_path: None,
            repository: Some("owner/repo".to_string()),
            github_sha: None,
            base_sha: None,
            head_sha: None,
            receipt: receipt.clone(),
            env_file: None,
            root: Some(temp.path().to_path_buf()),
        };
        let result = run(config);
        ensure!(result.is_err());
        let value: Value = serde_json::from_slice(&fs::read(receipt)?)?;
        ensure!(value["outcome"] == "fail");
        ensure!(value["failure_stage"] == "event-input");
        ensure!(value["error"].as_str().is_some());
        Ok(())
    }

    #[test]
    fn failed_receipt_replacement_restores_prior_receipt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let destination = temp.path().join("receipt.json");
        let temporary = temp.path().join(".receipt.json.tmp");
        let backup = temp.path().join(format!(".receipt.json.bak-{}", std::process::id()));
        fs::write(&destination, b"prior receipt")?;
        fs::write(&temporary, b"new receipt")?;

        let mut calls = 0;
        let result = replace_receipt_file_with(&temporary, &destination, |from, to| {
            calls += 1;
            if calls == 1 {
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "occupied"));
            }
            if calls == 3 {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"));
            }
            fs::rename(from, to)
        });

        ensure!(result.is_err());
        ensure!(fs::read(&destination)? == b"prior receipt");
        ensure!(!backup.exists());
        ensure!(temporary.exists());
        Ok(())
    }

    #[test]
    fn failed_receipt_restore_failure_retains_backup_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let destination = temp.path().join("receipt.json");
        let temporary = temp.path().join(".receipt.json.tmp");
        let backup = temp.path().join(format!(".receipt.json.bak-{}", std::process::id()));
        fs::write(&destination, b"prior receipt")?;
        fs::write(&temporary, b"new receipt")?;

        let mut calls = 0;
        let result = replace_receipt_file_with(&temporary, &destination, |from, to| {
            calls += 1;
            if calls == 1 || calls == 3 || calls == 4 {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"));
            }
            fs::rename(from, to)
        });

        let error =
            result.expect_err("replacement must fail when restoration is blocked").to_string();
        ensure!(error.contains("prior receipt preserved at"));
        ensure!(fs::read(&backup)? == b"prior receipt");
        ensure!(!destination.exists());
        ensure!(temporary.exists());
        Ok(())
    }

    #[test]
    fn receipt_directory_is_not_replaced_by_a_file() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let destination = temp.path().join("receipt.json");
        let temporary = temp.path().join(".receipt.json.tmp");
        fs::create_dir(&destination)?;
        fs::write(&temporary, b"new receipt")?;

        let result = replace_receipt_file(&temporary, &destination);

        ensure!(result.is_err());
        ensure!(destination.is_dir());
        ensure!(temporary.exists());
        Ok(())
    }

    #[test]
    fn bounded_git_runner_drains_and_caps_large_output() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut command = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            command.args(["/C", "for /L %i in (1,1,100000) do @echo x"]);
        } else {
            command.args(["-c", "yes x | head -c 200000"]);
        }
        command.current_dir(temp.path());
        let output = run_git_bounded(&mut command, None)?;
        ensure!(output.status.success());
        ensure!(output.stdout.len() <= MAX_RETAINED_GIT_OUTPUT);
        ensure!(output.stderr.is_empty());
        Ok(())
    }
}
