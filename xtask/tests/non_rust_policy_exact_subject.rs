//! Exact-subject binding contract for the trusted Non-Rust policy workflow (#14413).
//!
//! `pull_request_target`'s `merge_commit_sha` may describe a stale synthetic
//! merge. The workflow must instead construct, bind, and evaluate the merge of
//! the event's exact base and head while executing only trusted-base code.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_yaml_ng::Value;
use tempfile::TempDir;

#[path = "support/workflow_bash.rs"]
mod workflow_bash;

use workflow_bash::bash_executable;

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow() -> Result<Value> {
    let path = project_root().join(".github/workflows/non-rust-policy.yml");
    let source =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_yaml_ng::from_str(&source)?)
}

fn bind_run_block() -> Result<String> {
    named_run_block("Fetch and bind subject Git object")
}

fn named_run_block(step_name: &str) -> Result<String> {
    workflow()?
        .get("jobs")
        .and_then(|jobs| jobs.get("exact-tree"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| step.get("name").and_then(Value::as_str) == Some(step_name))
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("exact-tree must contain the subject binding run block"))
}

fn run(command: &str, args: &[&str], cwd: &Path) -> Result<Output> {
    Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("running `{command} {}`", args.join(" ")))
}

fn run_ok(command: &str, args: &[&str], cwd: &Path) -> Result<Output> {
    let output = run(command, args, cwd)?;
    if !output.status.success() {
        bail!(
            "`{command} {}` failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn git_output(args: &[&str], cwd: &Path) -> Result<String> {
    let output = run_ok("git", args, cwd)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .context("git output must be UTF-8")
}

fn write(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

struct PrFixture {
    _temp: TempDir,
    trusted: PathBuf,
    base_sha: String,
    head_sha: String,
}

fn pr_fixture() -> Result<PrFixture> {
    let temp = tempfile::tempdir()?;
    let remote = temp.path().join("remote.git");
    let seed = temp.path().join("seed");
    let trusted = temp.path().join("trusted");
    fs::create_dir_all(&seed)?;

    run_ok(
        "git",
        &["init", "--bare", remote.to_str().ok_or_else(|| anyhow!("remote path"))?],
        temp.path(),
    )?;
    run_ok("git", &["init"], &seed)?;
    run_ok("git", &["config", "user.name", "test"], &seed)?;
    run_ok("git", &["config", "user.email", "test@example.invalid"], &seed)?;
    write(&seed.join("subject.txt"), "base\n")?;
    run_ok("git", &["add", "subject.txt"], &seed)?;
    run_ok("git", &["commit", "-m", "base"], &seed)?;
    run_ok("git", &["branch", "-M", "main"], &seed)?;
    run_ok(
        "git",
        &["remote", "add", "origin", remote.to_str().ok_or_else(|| anyhow!("remote path"))?],
        &seed,
    )?;
    run_ok("git", &["push", "origin", "main"], &seed)?;

    run_ok("git", &["switch", "-c", "candidate"], &seed)?;
    write(&seed.join("subject.txt"), "candidate\n")?;
    run_ok("git", &["add", "subject.txt"], &seed)?;
    run_ok("git", &["commit", "-m", "candidate"], &seed)?;
    let head_sha = git_output(&["rev-parse", "HEAD"], &seed)?;
    run_ok("git", &["push", "origin", "candidate"], &seed)?;

    // Advance the target branch after the candidate forked. This proves the
    // synthetic subject uses the event's exact advanced base.
    run_ok("git", &["switch", "main"], &seed)?;
    write(&seed.join("base-advance.txt"), "advanced base\n")?;
    run_ok("git", &["add", "base-advance.txt"], &seed)?;
    run_ok("git", &["commit", "-m", "advance base"], &seed)?;
    let base_sha = git_output(&["rev-parse", "HEAD"], &seed)?;
    run_ok("git", &["push", "origin", "main"], &seed)?;

    run_ok(
        "git",
        &[
            "--git-dir",
            remote.to_str().ok_or_else(|| anyhow!("remote path"))?,
            "update-ref",
            "refs/pull/42/head",
            &head_sha,
        ],
        temp.path(),
    )?;

    run_ok(
        "git",
        &[
            "clone",
            remote.to_str().ok_or_else(|| anyhow!("remote path"))?,
            trusted.to_str().ok_or_else(|| anyhow!("trusted path"))?,
        ],
        temp.path(),
    )?;
    run_ok("git", &["switch", "main"], &trusted)?;

    Ok(PrFixture { _temp: temp, trusted, base_sha, head_sha })
}

#[test]
fn pull_request_target_constructs_a_subject_from_exact_base_and_head() -> Result<()> {
    let workflow = workflow()?;
    let subject_selector = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("exact-tree"))
        .and_then(|job| job.get("env"))
        .and_then(|env| env.get("SUBJECT_SHA"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("exact-tree must set SUBJECT_SHA"))?;
    ensure!(
        !subject_selector.contains("merge_commit_sha"),
        "pull_request_target must not trust a stale merge_commit_sha"
    );

    let run = bind_run_block()?;
    for required in [
        "git merge-tree --write-tree \"$BASE_SHA\" \"$PR_HEAD_SHA\"",
        "refs/pull/$PR_NUMBER/head:refs/remotes/origin/non-rust-policy-pr-head",
        "refs/remotes/origin/non-rust-policy-pr-head^{commit}",
        "git commit-tree \"$merge_tree\" -p \"$BASE_SHA\" -p \"$PR_HEAD_SHA\"",
        "GIT_AUTHOR_DATE=2000-01-01T00:00:00Z",
        "GIT_COMMITTER_DATE=2000-01-01T00:00:00Z",
        "git rev-parse \"$SUBJECT_SHA^1\"",
        "git rev-parse \"$SUBJECT_SHA^2\"",
        "git rev-parse \"$SUBJECT_SHA^{tree}\"",
        "echo \"subject_sha=$SUBJECT_SHA\" >> \"$GITHUB_OUTPUT\"",
    ] {
        ensure!(run.contains(required), "binding run block missing `{required}`");
    }
    let workflow_text =
        fs::read_to_string(project_root().join(".github/workflows/non-rust-policy.yml"))?;
    ensure!(
        workflow_text.contains("python3 - \"$workflow\"")
            && workflow_text.contains("run_bodies")
            && workflow_text.contains("github\\.event\\.pull_request"),
        "trusted workflow must structurally inspect executable run bodies"
    );
    let guard = named_run_block("Verify trusted workflow contract")?;
    let guard_output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &guard])
        .current_dir(project_root())
        .output()
        .context("executing trusted workflow contract guard")?;
    if !guard_output.status.success() {
        bail!(
            "trusted workflow guard failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            guard_output.status.code(),
            String::from_utf8_lossy(&guard_output.stdout),
            String::from_utf8_lossy(&guard_output.stderr)
        );
    }

    // The guard must reject an executable run body that interpolates an
    // untrusted pull-request field, including when the body is represented by
    // a YAML block scalar.
    let guard_fixture = tempfile::tempdir()?;
    let guard_workflow = guard_fixture.path().join(".github/workflows");
    fs::create_dir_all(&guard_workflow)?;
    let malicious_step = concat!(
        "      - name: Untrusted interpolation fixture\n",
        "        run: >-\n",
        "          echo \"${{ github.event.pull_request.head.sha }}\"\n\n",
    );
    // Keep the original guard step and insert the hostile run as its own
    // sequence item. This exercises the parser's step-boundary handling in
    // addition to its block-scalar handling.
    let malicious_workflow = workflow_text.replacen(
        "      - name: Verify trusted workflow contract\n",
        &format!("{malicious_step}      - name: Verify trusted workflow contract\n"),
        1,
    );
    let malicious_path = guard_workflow.join("non-rust-policy.yml");
    write(&malicious_path, &malicious_workflow)?;
    let malicious_guard = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &guard])
        .current_dir(guard_fixture.path())
        .output()
        .context("executing malicious trusted workflow guard fixture")?;
    ensure!(
        !malicious_guard.status.success(),
        "trusted workflow guard must reject pull-request interpolation"
    );
    ensure!(
        String::from_utf8_lossy(&malicious_guard.stderr)
            .contains("trusted run body interpolates pull-request event data"),
        "trusted workflow guard must report the interpolation reason; stderr: {}",
        String::from_utf8_lossy(&malicious_guard.stderr)
    );

    let fixture = pr_fixture()?;
    let output_path = fixture.trusted.join("github-output");
    let env_path = fixture.trusted.join("github-env");
    write(&output_path, "")?;
    write(&env_path, "")?;
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &run])
        .current_dir(&fixture.trusted)
        .env("BASE_SHA", &fixture.base_sha)
        .env("PR_HEAD_SHA", &fixture.head_sha)
        .env("PR_NUMBER", "42")
        .env("SUBJECT_SHA", "0000000000000000000000000000000000000000")
        .env("GITHUB_OUTPUT", &output_path)
        .env("GITHUB_ENV", &env_path)
        .output()
        .context("executing trusted subject-binding block")?;
    if !output.status.success() {
        bail!(
            "trusted subject binding failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output_value = fs::read_to_string(&output_path)?;
    let subject_sha = output_value
        .lines()
        .find_map(|line| line.strip_prefix("subject_sha="))
        .ok_or_else(|| anyhow!("binding must emit subject_sha"))?;
    ensure!(subject_sha != "0000000000000000000000000000000000000000");

    // If the pull-request ref has moved since the event was captured, the
    // binding must fail before creating or exporting a synthetic subject.
    write(&output_path, "")?;
    write(&env_path, "")?;
    let stale = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &run])
        .current_dir(&fixture.trusted)
        .env("BASE_SHA", &fixture.base_sha)
        .env("PR_HEAD_SHA", "1111111111111111111111111111111111111111")
        .env("PR_NUMBER", "42")
        .env("SUBJECT_SHA", "0000000000000000000000000000000000000000")
        .env("GITHUB_OUTPUT", &output_path)
        .env("GITHUB_ENV", &env_path)
        .output()
        .context("executing stale-head subject-binding block")?;
    ensure!(!stale.status.success(), "stale PR head must fail closed");
    ensure!(
        fs::read_to_string(&output_path)?.is_empty(),
        "stale PR head must not export a synthetic subject"
    );

    // The synthetic subject is reproducible across reruns, independent of
    // wall-clock metadata on the runner.
    write(&output_path, "")?;
    write(&env_path, "")?;
    let rerun = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &run])
        .current_dir(&fixture.trusted)
        .env("BASE_SHA", &fixture.base_sha)
        .env("PR_HEAD_SHA", &fixture.head_sha)
        .env("PR_NUMBER", "42")
        .env("SUBJECT_SHA", "0000000000000000000000000000000000000000")
        .env("GITHUB_OUTPUT", &output_path)
        .env("GITHUB_ENV", &env_path)
        .output()
        .context("rerunning trusted subject-binding block")?;
    if !rerun.status.success() {
        bail!(
            "trusted subject binding rerun failed with {:?}\nstdout:\n{}\nstderr:\n{}",
            rerun.status.code(),
            String::from_utf8_lossy(&rerun.stdout),
            String::from_utf8_lossy(&rerun.stderr)
        );
    }
    let rerun_output = fs::read_to_string(&output_path)?;
    let rerun_subject_sha = rerun_output
        .lines()
        .find_map(|line| line.strip_prefix("subject_sha="))
        .ok_or_else(|| anyhow!("binding rerun must emit subject_sha"))?;
    assert_eq!(rerun_subject_sha, subject_sha);

    assert_eq!(
        git_output(&["rev-parse", &format!("{subject_sha}^1")], &fixture.trusted)?,
        fixture.base_sha
    );
    assert_eq!(
        git_output(&["rev-parse", &format!("{subject_sha}^2")], &fixture.trusted)?,
        fixture.head_sha
    );
    assert_eq!(
        git_output(&["rev-parse", &format!("{subject_sha}^{{tree}}")], &fixture.trusted)?,
        git_output(
            &["merge-tree", "--write-tree", &fixture.base_sha, &fixture.head_sha],
            &fixture.trusted
        )?
    );
    assert_eq!(
        git_output(&["show", &format!("{subject_sha}:base-advance.txt")], &fixture.trusted,)?,
        "advanced base"
    );
    assert_eq!(fs::read_to_string(&env_path)?, format!("SUBJECT_SHA={subject_sha}\n"));
    Ok(())
}
