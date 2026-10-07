#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! End-to-end proof for `check-no-verify-recover` (#17430): a placeholder
//! identity smuggled into the pushed range (what `commit --no-verify` allows)
//! fails the required-tier path with a naming message, while clean, empty, and
//! unevaluable ranges behave.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new(label: &str) -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let unique = format!("{}-{}-{nanos}", std::process::id(), label);
        let path = env::temp_dir().join(format!("perl-ci-hygiene-noverify-{unique}"));
        fs::create_dir_all(&path)?;
        fs::write(path.join("Cargo.toml"), "[workspace]\n")?;
        // A unique initial branch keeps the no-base fixtures hermetic: none of
        // the auto-resolution candidates may accidentally resolve.
        git(&path, &["init", "--quiet", "-b", &format!("noverify-t9-{unique}")])?;
        git(&path, &["config", "user.email", "trap-suite@example.invalid"])?;
        git(&path, &["config", "user.name", "trap-suite"])?;
        git(&path, &["config", "commit.gpgsign", "false"])?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Commits `content` as `filename` under the repo-local clean identity.
    fn commit(&self, filename: &str, content: &str, message: &str) -> TestResult<String> {
        fs::write(self.path.join(filename), content)?;
        git(&self.path, &["add", filename])?;
        git(&self.path, &["commit", "--quiet", "-m", message])?;
        git(&self.path, &["rev-parse", "HEAD"])
    }

    /// Commits `content` as `filename` under an explicit identity, simulating
    /// what `commit --no-verify` admits past the skipped pre-commit hook.
    fn commit_as(
        &self,
        filename: &str,
        content: &str,
        message: &str,
        name: &str,
        email: &str,
    ) -> TestResult<String> {
        fs::write(self.path.join(filename), content)?;
        git(&self.path, &["add", filename])?;
        git(
            &self.path,
            &[
                "-c",
                &format!("user.name={name}"),
                "-c",
                &format!("user.email={email}"),
                "commit",
                "--quiet",
                "-m",
                message,
            ],
        )?;
        git(&self.path, &["rev-parse", "HEAD"])
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git(repo: &Path, args: &[&str]) -> TestResult<String> {
    let output = Command::new("git").args(args).current_dir(repo).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn perl_ci_hygiene_binary() -> TestResult<PathBuf> {
    env::var_os("CARGO_BIN_EXE_perl-ci-hygiene").map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "CARGO_BIN_EXE_perl-ci-hygiene was not set by cargo",
        )
        .into()
    })
}

/// Runs the gate with a scrubbed base-resolution environment, so the
/// developer's (or CI's) ambient `CI` / `GITHUB_BASE_REF` / `CI_SCOPE_BASE`
/// cannot leak into the fixture. Pass `ci = Some("1")` for the CI posture.
fn run_check(repo: &Path, args: &[&str], ci: Option<&str>) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    let mut command = Command::new(bin);
    command
        .arg("check-no-verify-recover")
        .args(args)
        .current_dir(repo)
        .env_remove("CI")
        .env_remove("CI_SCOPE_BASE")
        .env_remove("GITHUB_BASE_REF");
    if let Some(value) = ci {
        command.env("CI", value);
    }
    Ok(command.output()?)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn placeholder_identity_commit_in_range_fails_naming_the_commit() -> TestResult {
    let repo = TempRepo::new("violating-range")?;
    let base = repo.commit("file.txt", "base\n", "base commit")?;
    let violating = repo.commit_as(
        "file.txt",
        "smuggled\n",
        "bypassed commit gate",
        "xtask hook tests",
        "xtask@example.invalid",
    )?;

    let output = run_check(repo.path(), &["--base", &base], None)?;

    assert!(!output.status.success(), "a placeholder identity in range must fail");
    let text = stdout(&output);
    assert!(text.contains("no-verify re-cover"), "missing gate name: {text}");
    // One violating commit with all four slots placeholder still counts as
    // one commit, not four.
    assert!(text.contains("1 commit(s) in"), "missing distinct-commit count: {text}");
    assert!(text.contains(&violating), "missing violating sha: {text}");
    assert!(text.contains("xtask hook tests"), "missing identity value: {text}");
    assert!(text.contains("author name"), "missing slot name: {text}");
    assert!(text.contains("bypassed commit gate"), "missing subject: {text}");
    assert!(text.contains("--unset-all user.name"), "missing remedy: {text}");
    assert!(text.contains("--reset-author"), "missing rewrite order: {text}");
    Ok(())
}

#[test]
fn placeholder_committer_with_clean_author_still_fails() -> TestResult {
    let repo = TempRepo::new("committer-only")?;
    let base = repo.commit("file.txt", "base\n", "base commit")?;
    // Clean author, placeholder committer: the hook's OR semantics refuse on
    // any one matching slot, and so must the re-cover.
    fs::write(repo.path().join("file.txt"), "smuggled\n")?;
    git(repo.path(), &["add", "file.txt"])?;
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Codex Release Validation",
            "-c",
            "user.email=codex-release-validation@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--author=Dev Human <dev@example.com>",
            "-m",
            "committer-only placeholder",
        ],
    )?;
    let violating = git(repo.path(), &["rev-parse", "HEAD"])?;

    let output = run_check(repo.path(), &["--base", &base], None)?;

    assert!(!output.status.success(), "a placeholder committer must fail");
    let text = stdout(&output);
    assert!(text.contains(&violating), "missing violating sha: {text}");
    assert!(text.contains("committer name"), "missing slot name: {text}");
    assert!(text.contains("Codex Release Validation"), "missing identity value: {text}");
    Ok(())
}

#[test]
fn clean_range_passes() -> TestResult {
    let repo = TempRepo::new("clean-range")?;
    let base = repo.commit("file.txt", "base\n", "base commit")?;
    repo.commit("file.txt", "second\n", "second commit")?;

    let output = run_check(repo.path(), &["--base", &base], None)?;

    assert!(output.status.success(), "a clean range must pass: {}", combined(&output));
    assert!(stdout(&output).contains("No placeholder git identity"), "{}", combined(&output));
    Ok(())
}

#[test]
fn empty_range_passes() -> TestResult {
    let repo = TempRepo::new("empty-range")?;
    repo.commit("file.txt", "base\n", "base commit")?;

    let output = run_check(repo.path(), &["--base", "HEAD"], None)?;

    assert!(output.status.success(), "an empty range must pass: {}", combined(&output));
    Ok(())
}

#[test]
fn unresolvable_explicit_base_fails() -> TestResult {
    let repo = TempRepo::new("bad-base")?;
    repo.commit("file.txt", "base\n", "base commit")?;

    let output = run_check(repo.path(), &["--base", "does-not-exist"], None)?;

    assert!(!output.status.success(), "an unresolvable --base must fail");
    assert!(combined(&output).contains("does not resolve"), "{}", combined(&output));
    Ok(())
}

#[test]
fn unresolvable_auto_base_warns_locally() -> TestResult {
    // Single commit on a unique branch with no remote: none of the auto
    // candidates resolve, and the scrubbed environment adds none.
    let repo = TempRepo::new("no-base-local")?;
    repo.commit("file.txt", "lone\n", "lone commit")?;

    let output = run_check(repo.path(), &[], None)?;

    assert!(output.status.success(), "locally an unevaluated run must pass: {}", combined(&output));
    assert!(stdout(&output).contains("not evaluated"), "{}", combined(&output));
    Ok(())
}

#[test]
fn unresolvable_auto_base_fails_closed_in_ci() -> TestResult {
    let repo = TempRepo::new("no-base-ci")?;
    repo.commit("file.txt", "lone\n", "lone commit")?;

    let output = run_check(repo.path(), &[], Some("1"))?;

    assert!(!output.status.success(), "CI without a baseline must fail closed");
    assert!(
        combined(&output).contains("cannot resolve a merge baseline in CI"),
        "{}",
        combined(&output)
    );
    Ok(())
}
