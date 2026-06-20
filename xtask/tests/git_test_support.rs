// Integration tests import this support module with different helper subsets.
#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output, Stdio},
};

/// Initialize a minimal git repo in `dir` with one initial commit.
pub fn init_git_repo(dir: &Path) -> Result<()> {
    git_cmd(&["init", "-b", "master"], Some(dir)).or_else(|_| git_cmd(&["init"], Some(dir)))?;
    git_cmd(&["config", "user.email", "test@test.com"], Some(dir))?;
    git_cmd(&["config", "user.name", "Test"], Some(dir))?;
    // Rename branch to master if needed (older git).
    let _ = git_cmd(&["checkout", "-b", "master"], Some(dir));
    Ok(())
}

/// Run a git command in `cwd`. Returns an error if the command fails.
pub fn git_cmd(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = StdCommand::new("git");
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("git {:?} failed with {:?}", args, status.code());
    }
    Ok(())
}

/// Stage and commit a set of files in `repo`.
pub fn add_and_commit(repo: &Path, files: &[(&str, &str)], message: &str) -> Result<()> {
    for (name, content) in files {
        let path = repo.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
    }
    git_cmd(&["add", "."], Some(repo))?;
    git_cmd(&["commit", "-m", message], Some(repo))?;
    Ok(())
}

pub fn current_head(root: &Path) -> Result<String> {
    git_stdout_with_worktree_fallback(root, &["rev-parse", "HEAD"])
        .context("running git rev-parse HEAD")
}

fn git_stdout_with_worktree_fallback(root: &Path, args: &[&str]) -> Result<String> {
    let output = git_output(root, args, None)?;
    if output.status.success() {
        return output_to_string(args, output);
    }

    if let Some(context) = worktree_git_context(root)? {
        let fallback = git_output(&context.work_tree, args, Some(&context.git_dir))?;
        if fallback.status.success() {
            return output_to_string(args, fallback);
        }
    }

    output_to_string(args, output)
}

fn git_output(root: &Path, args: &[&str], git_dir: Option<&Path>) -> Result<Output> {
    let mut command = StdCommand::new("git");
    command.args(args).current_dir(root);
    if let Some(git_dir) = git_dir {
        command.env("GIT_DIR", git_dir).env("GIT_WORK_TREE", root);
    }
    command.output().context("running git command")
}

fn output_to_string(args: &[&str], output: Output) -> Result<String> {
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

struct WorktreeGitContext {
    git_dir: PathBuf,
    work_tree: PathBuf,
}

fn worktree_git_context(root: &Path) -> Result<Option<WorktreeGitContext>> {
    for work_tree in root.ancestors() {
        let git_file = work_tree.join(".git");
        if git_file.is_dir() {
            return Ok(None);
        }
        let contents = match fs::read_to_string(&git_file) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", git_file.display()));
            }
        };
        let Some(raw_git_dir) = contents.trim().strip_prefix("gitdir:") else {
            return Ok(None);
        };
        return Ok(Some(WorktreeGitContext {
            git_dir: resolve_git_dir(work_tree, raw_git_dir.trim()),
            work_tree: work_tree.to_path_buf(),
        }));
    }
    Ok(None)
}

fn resolve_git_dir(work_tree: &Path, raw_git_dir: &str) -> PathBuf {
    let direct = PathBuf::from(raw_git_dir);
    if direct.is_absolute() || direct.exists() {
        return direct;
    }
    if let Some(translated) = translate_windows_git_dir_for_unix(raw_git_dir) {
        return translated;
    }
    work_tree.join(raw_git_dir)
}

#[cfg(unix)]
fn translate_windows_git_dir_for_unix(raw: &str) -> Option<PathBuf> {
    let mut chars = raw.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let rest = chars.as_str().trim_start_matches(['/', '\\']).replace('\\', "/");
    Some(Path::new("/mnt").join(drive.to_ascii_lowercase().to_string()).join(rest))
}

#[cfg(not(unix))]
fn translate_windows_git_dir_for_unix(_raw: &str) -> Option<PathBuf> {
    None
}
