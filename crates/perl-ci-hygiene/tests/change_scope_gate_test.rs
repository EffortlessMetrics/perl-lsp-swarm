#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! Gate-level regressions for `check-change-scope`'s git subjects (#17433).
//!
//! The unit tests pin the pure comparison (`parse` / `is_admitted` /
//! `find_unadmitted` / `parse_name_status_z`). These tests pin the shell around
//! them: which commit the staleness check compares against, and which paths the
//! diff reports. Each test builds a fixture repo in scratch, drives the real
//! binary through `CARGO_BIN_EXE_perl-ci-hygiene`, and asserts the exit code
//! plus the decisive stdout line. No git command ever touches the checkout.
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
        let path = env::temp_dir()
            .join(format!("perl-ci-hygiene-scope-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        // The binary locates the repo root by walking up to `Cargo.toml`.
        fs::write(path.join("Cargo.toml"), "[workspace]\n")?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `git` with a hermetic identity: `-c` flags travel on the command
    /// line, so the fixture neither reads nor writes the developer's config.
    /// Repository-location overrides are stripped (see `GIT_LOCATION_VARS`):
    /// git hooks set them and the pre-push hook runs `cargo test`, so without
    /// the strip a hook-launched suite would commit fixtures into the real repo.
    fn git(&self, args: &[&str]) -> TestResult<String> {
        let mut command = Command::new("git");
        scrub_git_location(&mut command);
        let output = command
            .current_dir(&self.path)
            .args([
                "-c",
                "user.email=scope-gate-test@example.invalid",
                "-c",
                "user.name=scope-gate-test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn mkdir(&self, rel: &str) -> TestResult<()> {
        fs::create_dir_all(self.path.join(rel))?;
        Ok(())
    }

    fn write(&self, rel: &str, content: &str) -> TestResult<()> {
        let path = self.path.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }

    fn commit_all(&self, message: &str) -> TestResult<()> {
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-qm", message])?;
        Ok(())
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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

fn run_change_scope(repo: &Path, base: &str) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    let mut command = Command::new(bin);
    scrub_git_location(&mut command);
    Ok(command.args(["check-change-scope", "--base", base]).current_dir(repo).output()?)
}

/// Repository-location overrides that would retarget a child git (or a gate
/// binary shelling to git) away from its `current_dir`. Same canonical set
/// as `git_command()` in `git_hooks.rs` plus the object-store lookups.
const GIT_LOCATION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
];

/// Strips repository-location overrides from a child command so it cannot
/// escape its `current_dir` into the caller's repository.
fn scrub_git_location(command: &mut Command) {
    for var in GIT_LOCATION_VARS {
        command.env_remove(var);
    }
}

/// A scope file the target branch advanced after divergence must not activate
/// on this branch: the worktree copy is byte-identical to the merge-base blob,
/// so the gate is unevaluated — even though the branch touches files outside
/// that old scope. Comparing against the base-ref tip instead enforced the
/// inherited declaration and failed the unrelated branch.
#[test]
fn declaration_changed_only_on_target_branch_is_inert() -> TestResult {
    let repo = TempRepo::new("target-advance")?;
    repo.git(&["init", "-q", "-b", "main"])?;
    repo.write(".agents/change-scope", "src/\n")?;
    repo.write("src/tool.rs", "fn tool() {}\n")?;
    repo.write("tests/tool.rs", "#[test]\nfn tool() {}\n")?;
    repo.commit_all("base with inherited scope")?;
    repo.git(&["checkout", "-qb", "feature"])?;
    // The target advances its own declaration after divergence.
    repo.git(&["checkout", "-q", "main"])?;
    repo.write(".agents/change-scope", "docs/\n")?;
    repo.commit_all("main narrows its scope")?;
    // The feature branch never touched the declaration; it edits inside the
    // old scope and next door.
    repo.git(&["checkout", "-q", "feature"])?;
    repo.write("src/tool.rs", "fn tool() { fixed(); }\n")?;
    repo.write("tests/tool.rs", "#[test]\nfn tool() { fixed(); }\n")?;
    repo.commit_all("feature touches src and tests")?;

    let out = run_change_scope(repo.path(), "main")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "inherited declaration must stay inert, got: {stdout}");
    assert!(
        stdout.contains("not evaluated"),
        "stale declaration must report unevaluated, got: {stdout}"
    );
    Ok(())
}

/// Non-vacuity for the merge-base staleness check: a branch that rewrites its
/// own declaration IS judged against the new text — the merge-base comparison
/// must not launder a narrowed scope back into the old one.
#[test]
fn declaration_changed_on_pr_branch_is_enforced() -> TestResult {
    let repo = TempRepo::new("branch-scope")?;
    repo.git(&["init", "-q", "-b", "main"])?;
    repo.write(".agents/change-scope", "src/\n")?;
    repo.write("src/other.rs", "fn other() {}\n")?;
    repo.commit_all("base")?;
    repo.git(&["checkout", "-qb", "feature"])?;
    repo.write(".agents/change-scope", "src/sub/\n")?;
    repo.write("src/other.rs", "fn other() { fixed(); }\n")?;
    repo.commit_all("narrow scope and touch outside it")?;

    let out = run_change_scope(repo.path(), "main")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(1),
        "narrowed scope must fail on src/other.rs, got: {stdout}"
    );
    assert!(
        stdout.contains("outside the declared scope"),
        "failure must name the scope violation, got: {stdout}"
    );
    assert!(stdout.contains("src/other.rs"), "failure must name src/other.rs, got: {stdout}");
    Ok(())
}

/// Renaming an out-of-scope file into an admitted directory deletes the
/// original: the gate must name the source, not pass on the destination.
/// Rename detection is pinned on in the fixture so the bypass shape (one
/// `R100` record, destination-only under `--name-only`) is deterministic and
/// not a function of ambient git config.
#[test]
fn rename_from_outside_scope_into_scope_is_flagged() -> TestResult {
    let repo = TempRepo::new("rename")?;
    repo.git(&["init", "-q", "-b", "main"])?;
    repo.git(&["config", "diff.renames", "true"])?;
    repo.write("docs/guide.md", "# guide\n")?;
    repo.commit_all("base")?;
    repo.git(&["checkout", "-qb", "feature"])?;
    repo.write(".agents/change-scope", "src/\n")?;
    repo.mkdir("src")?;
    repo.git(&["mv", "docs/guide.md", "src/guide.md"])?;
    repo.git(&["add", "-A"])?;

    let out = run_change_scope(repo.path(), "main")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "rename source must fail the gate, got: {stdout}");
    assert!(stdout.contains("docs/guide.md"), "gate must name the rename source, got: {stdout}");
    Ok(())
}
