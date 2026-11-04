//! Production consumer regressions: poison only exact-test descendants, never
//! the test process. Every mutable repository here is disposable and owned.

use super::{merge_base_sha, repository_identity, resolve_sha};
use crate::tasks::{change_set, ci_subject_materializer, staged};
use change_set::{ArtifactIdentity, DiffMode};
use color_eyre::eyre::{Result, ensure, eyre};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = "XTASK_CONSUMER_CONTEXT_ROOT";
const PREFIX: &str = "tasks::ci_contract::git_context_tests::";

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = xtask::git_environment::isolate(Command::new("git"))
        .current_dir(root)
        .args(args)
        .output()?;
    ensure!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn initialize(root: &Path, name: &str) -> Result<String> {
    fs::create_dir(root)?;
    git(root, &["init", "--quiet", "--initial-branch=main"])?;
    git(root, &["config", "user.name", "Consumer Fixture"])?;
    git(root, &["config", "user.email", "fixture@example.invalid"])?;
    git(root, &["config", "core.hooksPath", ".git/fixture-hooks"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["remote", "add", "origin", &format!("https://github.com/owner/{name}.git")])?;
    fs::write(root.join("base.txt"), format!("{name}\n"))?;
    git(root, &["add", "."])?;
    git(root, &["commit", "--quiet", "-m", name])?;
    git(root, &["rev-parse", "HEAD"])
}

fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            ensure!(!entry.file_type()?.is_symlink(), "unexpected fixture symlink");
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.strip_prefix(root)?.to_path_buf(), fs::read(path)?);
            }
        }
    }
    Ok(files)
}

fn poisoned_child(root: &Path, name: &str, index: &Path) -> Result<Command> {
    let caller = root.join("caller");
    let mut child = xtask::git_environment::isolate(Command::new(std::env::current_exe()?));
    child
        .args(["--exact", &format!("{PREFIX}{name}"), "--nocapture"])
        .env(ROOT, root)
        .env("GIT_DIR", caller.join(".git"))
        .env("GIT_COMMON_DIR", caller.join(".git"))
        .env("GIT_WORK_TREE", &caller)
        .env("GIT_OBJECT_DIRECTORY", caller.join(".git/objects"))
        .env("GIT_INDEX_FILE", index)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "consumer.poison")
        .env("GIT_CONFIG_VALUE_0", "present")
        // All fetches use repo-local rewrites to owned local remotes.
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("GIT_TERMINAL_PROMPT", "0");
    Ok(child)
}

fn assert_poison(root: &Path) -> Result<()> {
    ensure!(std::env::var_os("GIT_INDEX_FILE").is_some(), "probe lost the hostile index");
    let output = Command::new("git")
        .current_dir(root.join("target"))
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    ensure!(output.status.success());
    ensure!(
        Path::new(String::from_utf8(output.stdout)?.trim()).canonicalize()?
            == root.join("caller").canonicalize()?,
        "raw Git did not select the disposable caller"
    );
    Ok(())
}

fn completed(root: &Path, name: &str, child: &mut Command, preserve_caller: bool) -> Result<()> {
    let before = snapshot(&root.join("caller"))?;
    let output = child.output()?;
    if preserve_caller {
        ensure!(snapshot(&root.join("caller"))? == before, "{name}: caller bytes changed");
    }
    let stdout = String::from_utf8(output.stdout)?;
    ensure!(
        output.status.success(),
        "{name}: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "wrong test denominator: {stdout}"
    );
    ensure!(
        fs::read_to_string(root.join("completed"))? == name,
        "missing post-assertion completion"
    );
    Ok(())
}

#[test]
fn explicit_queries_ignore_caller_context() -> Result<()> {
    let name = "explicit_queries_ignore_caller_context";
    if let Some(root) = std::env::var_os(ROOT) {
        let root = PathBuf::from(root);
        assert_poison(&root)?;
        let target = root.join("target");
        let base = fs::read_to_string(root.join("base"))?;
        let head = fs::read_to_string(root.join("head"))?;
        ensure!(repository_identity(&target)? == "owner/target");
        ensure!(merge_base_sha(&target, "main", "HEAD")? == base);
        ensure!(resolve_sha(&target, "HEAD")? == head);
        for mode in [DiffMode::MergeBaseThreeDot, DiffMode::DirectTwoDot] {
            let changes = change_set::resolve_change_set_with_mode(
                ArtifactIdentity::CommitRange { base: "auto".into(), head: "HEAD".into() },
                &target,
                mode,
            )?;
            ensure!(changes.base_sha.as_deref() == Some(base.as_str()));
            ensure!(changes.head_sha.as_deref() == Some(head.as_str()));
            ensure!(changes.changed_paths == ["target-change.txt"]);
        }
        let fallback = change_set::resolve_change_set(
            ArtifactIdentity::CommitRange { base: "main".into(), head: "unrelated".into() },
            &target,
        )?;
        ensure!(fallback.changed_paths == ["base.txt", "unrelated.txt"]);
        fs::write(root.join("completed"), name)?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    for repo in ["target", "caller"] {
        let path = root.join(repo);
        let base = initialize(&path, repo)?;
        git(&path, &["update-ref", "refs/remotes/origin/main", &base])?;
        fs::write(path.join(format!("{repo}-change.txt")), repo)?;
        git(&path, &["add", "."])?;
        git(&path, &["commit", "--quiet", "-m", "head"])?;
        let head = git(&path, &["rev-parse", "HEAD"])?;
        git(&path, &["checkout", "-q", "-b", "candidate"])?;
        git(&path, &["branch", "-f", "main", &base])?;
        git(&path, &["checkout", "--quiet", "--orphan", "unrelated"])?;
        git(&path, &["rm", "-r", "--force", "--quiet", "--", "."])?;
        fs::write(path.join("unrelated.txt"), repo)?;
        git(&path, &["add", "."])?;
        git(&path, &["commit", "--quiet", "-m", "unrelated"])?;
        git(&path, &["checkout", "--quiet", "--detach", &head])?;
        if repo == "target" {
            fs::write(root.join("base"), base)?;
            fs::write(root.join("head"), head)?;
        }
    }
    let mut child = poisoned_child(root, name, &root.join("caller/.git/index"))?;
    completed(root, name, &mut child, true)
}

#[test]
fn materializer_fetch_and_objects_stay_in_selected_repository() -> Result<()> {
    let name = "materializer_fetch_and_objects_stay_in_selected_repository";
    if let Some(root) = std::env::var_os(ROOT) {
        let root = PathBuf::from(root);
        assert_poison(&root)?;
        let target = root.join("target");
        let base = fs::read_to_string(root.join("base"))?;
        let head = fs::read_to_string(root.join("head"))?;
        let mode = fs::read_to_string(root.join("mode"))?;
        let receipt = root.join("receipt.json");
        let result = ci_subject_materializer::run(ci_subject_materializer::Config {
            event_name: Some(mode.clone()),
            event_path: (mode == "pull_request_target").then(|| root.join("event.json")),
            repository: Some("owner/target".into()),
            github_sha: None,
            base_sha: (mode == "explicit").then(|| base.clone()),
            head_sha: (mode == "explicit").then(|| head.clone()),
            receipt: receipt.clone(),
            env_file: None,
            root: Some(target.clone()),
        });
        let value: serde_json::Value = serde_json::from_slice(&fs::read(receipt)?)?;
        ensure!(
            git(&target, &["cat-file", "-t", &head])? == "commit",
            "head was not fetched into target"
        );
        if mode == "explicit" {
            // Preserve the existing depth-one fetch policy: the fetched head
            // is a shallow root, so merge-tree refuses unrelated history.
            ensure!(result.is_err(), "exact-SHA shallow history unexpectedly materialized");
            ensure!(value["outcome"] == "fail");
            ensure!(value["failure_stage"] == "merge-tree");
            ensure!(
                fs::read_to_string(target.join(".git/shallow"))?.lines().any(|line| line == head)
            );
            fs::write(root.join("completed"), name)?;
            return Ok(());
        }
        result?;
        ensure!(value["outcome"] == "pass");
        let subject = value["derived_subject_sha"].as_str().ok_or_else(|| eyre!("no subject"))?;
        ensure!(git(&target, &["rev-parse", &format!("{subject}^1")])? == base);
        ensure!(git(&target, &["rev-parse", &format!("{subject}^2")])? == head);
        ensure!(git(&target, &["show", &format!("{subject}:head.txt")])? == "head payload");
        ensure!(git(&target, &["show", &format!("{subject}:base-side.txt")])? == "base payload");
        let tree = git(&target, &["rev-parse", &format!("{subject}^{{tree}}")])?;
        ensure!(tree != git(&target, &["rev-parse", &format!("{base}^{{tree}}")])?);
        ensure!(
            tree != git(&target, &["rev-parse", &format!("{head}^{{tree}}")])?,
            "merge reused an input tree"
        );
        if mode == "pull_request_target" {
            ensure!(git(&target, &["rev-parse", "refs/ci-subject/pr-head"])? == head);
            ensure!(git(&target, &["rev-parse", "refs/ci-subject/pr-merge"])? == head);
        }
        fs::write(root.join("completed"), name)?;
        return Ok(());
    }
    let mut failures = Vec::new();
    for mode in ["explicit", "pull_request_target"] {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let seed = root.join("seed");
        let fork = initialize(&seed, "target")?;
        fs::write(seed.join("base-side.txt"), "base payload\n")?;
        git(&seed, &["add", "."])?;
        git(&seed, &["commit", "--quiet", "-m", "base side"])?;
        let base = git(&seed, &["rev-parse", "HEAD"])?;
        let remote = root.join("remote.git");
        git(
            root,
            &["init", "--quiet", "--bare", remote.to_str().ok_or_else(|| eyre!("remote path"))?],
        )?;
        git(
            &seed,
            &[
                "push",
                "--quiet",
                remote.to_str().ok_or_else(|| eyre!("remote path"))?,
                "HEAD:refs/heads/main",
            ],
        )?;
        let target = root.join("target");
        git(
            root,
            &[
                "clone",
                "--quiet",
                "--no-local",
                "--branch",
                "main",
                remote.to_str().ok_or_else(|| eyre!("remote path"))?,
                target.to_str().ok_or_else(|| eyre!("target path"))?,
            ],
        )?;
        git(&seed, &["checkout", "--quiet", "-b", "head", &fork])?;
        fs::write(seed.join("head.txt"), "head payload\n")?;
        git(&seed, &["add", "."])?;
        git(&seed, &["commit", "--quiet", "-m", "head"])?;
        let head = git(&seed, &["rev-parse", "HEAD"])?;
        git(
            &seed,
            &[
                "push",
                "--quiet",
                remote.to_str().ok_or_else(|| eyre!("remote path"))?,
                "HEAD:refs/pull/42/head",
                "HEAD:refs/pull/42/merge",
            ],
        )?;
        let caller = root.join("caller");
        git(
            root,
            &[
                "clone",
                "--quiet",
                "--no-local",
                seed.to_str().ok_or_else(|| eyre!("seed path"))?,
                caller.to_str().ok_or_else(|| eyre!("caller path"))?,
            ],
        )?;
        for path in [&target, &caller] {
            git(path, &["remote", "set-url", "origin", "https://github.com/owner/target.git"])?;
            git(
                path,
                &[
                    "config",
                    &format!("url.{}.insteadOf", remote.to_string_lossy()),
                    "https://github.com/owner/target.git",
                ],
            )?;
        }
        let missing = xtask::git_environment::isolate(Command::new("git"))
            .current_dir(&target)
            .args(["cat-file", "-e", &head])
            .output()?;
        ensure!(!missing.status.success(), "fetch precondition was vacuous");
        fs::write(root.join("base"), &base)?;
        fs::write(root.join("head"), &head)?;
        fs::write(root.join("mode"), mode)?;
        fs::write(
            root.join("event.json"),
            serde_json::json!({
                "repository": {"full_name": "owner/target"},
                "pull_request": {"number": 42,
                    "base": {"sha": base, "repo": {"full_name": "owner/target"}},
                    "head": {"sha": head, "repo": {"full_name": "fork/target"}}}
            })
            .to_string(),
        )?;
        let mut child = poisoned_child(root, name, &caller.join(".git/index"))?;
        if let Err(error) = completed(root, name, &mut child, true) {
            failures.push(format!("{mode}: {error:#}"));
        }
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn staged_tree_preserves_selected_git_directory_and_alternate_index() -> Result<()> {
    let name = "staged_tree_preserves_selected_git_directory_and_alternate_index";
    if let Some(root) = std::env::var_os(ROOT) {
        let root = PathBuf::from(root);
        assert_poison(&root)?;
        let target = root.join("target");
        let oid = staged::staged_tree_oid(&target)?;
        ensure!(
            staged::read_staged_path_text(&target, "base.txt", None)?
                == staged::StagedPathText::Present("alternate staged\n".into())
        );
        let changes = change_set::resolve_change_set(
            ArtifactIdentity::StagedTree { oid: oid.clone() },
            &target,
        )?;
        ensure!(
            changes.base_sha.as_deref() == Some(fs::read_to_string(root.join("base"))?.as_str()),
            "staged informational base used the decoy repository"
        );
        ensure!(changes.changed_paths == ["base.txt"]);
        // Change the live alternate index after capture; the frozen tree must
        // retain the earlier staged blob, excluding worktree/untracked content.
        let status = Command::new("git").current_dir(&target).args(["add", "base.txt"]).status()?;
        ensure!(status.success());
        ensure!(
            staged::read_staged_path_text(&target, "base.txt", Some(&oid))?
                == staged::StagedPathText::Present("alternate staged\n".into())
        );
        ensure!(
            staged::read_staged_path_text(&target, "base.txt", None)?
                == staged::StagedPathText::Present("unstaged\n".into())
        );
        let frozen = change_set::resolve_change_set(ArtifactIdentity::StagedTree { oid }, &target)?;
        ensure!(frozen == changes);
        fs::write(root.join("completed"), name)?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    initialize(&root.join("target"), "decoy")?;
    let caller = root.join("caller");
    let base = initialize(&caller, "caller")?;
    fs::write(root.join("base"), base)?;
    fs::write(caller.join("base.txt"), "alternate staged\n")?;
    git(&caller, &["add", "base.txt"])?;
    let alternate = caller.join(".git/alternate-index");
    fs::copy(caller.join(".git/index"), &alternate)?;
    fs::write(caller.join("base.txt"), "default staged\n")?;
    git(&caller, &["add", "base.txt"])?;
    fs::write(caller.join("base.txt"), "unstaged\n")?;
    fs::write(caller.join("untracked.txt"), "unique untracked\n")?;
    let default_before = fs::read(caller.join(".git/index"))?;
    let mut child = poisoned_child(root, name, &alternate)?;
    completed(root, name, &mut child, false)?;
    ensure!(fs::read(caller.join(".git/index"))? == default_before, "default index changed");
    ensure!(fs::read_to_string(caller.join("base.txt"))? == "unstaged\n");
    ensure!(fs::read_to_string(caller.join("untracked.txt"))? == "unique untracked\n");
    Ok(())
}
