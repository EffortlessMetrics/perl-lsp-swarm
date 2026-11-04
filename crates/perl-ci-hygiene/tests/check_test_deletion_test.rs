#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! End-to-end proof for `check-test-deletion` (#17405) over throwaway git repos.
//!
//! The unit tests in `commands::test_deletion` prove the pure scanner in both
//! directions; these tests prove the git shell around it: base resolution,
//! base-pinned reads, working-tree comparison, exit codes, and the marker
//! justification in both of its homes (added diff lines and commit messages).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const TWO_TESTS: &str = r#"#[test]
fn keeps_working() {
    assert_eq!(1 + 1, 2);
}

#[test]
fn also_keeps_working() {
    assert!(true);
}
"#;

struct TempGitRepo {
    path: PathBuf,
}

impl TempGitRepo {
    fn new(label: &str) -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir()
            .join(format!("perl-ci-hygiene-test-deletion-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        fs::write(path.join("Cargo.toml"), "[workspace]\n")?;
        let repo = Self { path };
        repo.git(&["init", "-b", "main", "-q"])?;
        repo.git(&["config", "user.email", "test-deletion@example.invalid"])?;
        repo.git(&["config", "user.name", "Test Deletion Fixture"])?;
        repo.git(&["config", "commit.gpgsign", "false"])?;
        repo.git(&["config", "core.autocrlf", "false"])?;
        Ok(repo)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn git(&self, args: &[&str]) -> TestResult<String> {
        let output = Command::new("git").args(args).current_dir(&self.path).output()?;
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

    fn write(&self, relative: &str, content: &str) -> TestResult<()> {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }

    fn remove(&self, relative: &str) -> TestResult<()> {
        fs::remove_file(self.path.join(relative))?;
        Ok(())
    }

    fn commit_all(&self, message: &str) -> TestResult<String> {
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-q", "-m", message])?;
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_owned())
    }
}

impl Drop for TempGitRepo {
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

fn run_check_test_deletion(repo: &Path, base: &str) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    Ok(Command::new(bin)
        .args(["check-test-deletion", "--base", base])
        .current_dir(repo)
        .output()?)
}

/// A fixture PR that deletes one test file and hollows one assertion must exit
/// nonzero naming both files (uncommitted working-tree edits are in subject).
#[test]
fn deletion_fixture_fails_naming_both_files() -> TestResult {
    let repo = TempGitRepo::new("deletion")?;
    repo.write("crates/example/tests/gone.rs", TWO_TESTS)?;
    repo.write("crates/example/src/hollow.rs", TWO_TESTS)?;
    let base = repo.commit_all("base: two-test suite")?;

    repo.remove("crates/example/tests/gone.rs")?;
    repo.write(
        "crates/example/src/hollow.rs",
        "#[test]\nfn keeps_working() {\n}\n\n#[test]\nfn also_keeps_working() {\n    assert!(true);\n}\n",
    )?;

    let out = run_check_test_deletion(repo.path(), &base)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(out.status.code(), Some(0), "deletion must fail\nstdout: {stdout}");
    assert!(
        stdout.contains("crates/example/tests/gone.rs"),
        "must name the deleted test file\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("crates/example/src/hollow.rs"),
        "must name the hollowed file\nstdout: {stdout}"
    );
    Ok(())
}

/// A fixture that only ADDS tests must exit 0 (non-vacuity control: the gate
/// proves it can pass, not just fail).
#[test]
fn addition_only_fixture_passes() -> TestResult {
    let repo = TempGitRepo::new("addition")?;
    repo.write("crates/example/src/lib.rs", TWO_TESTS)?;
    let base = repo.commit_all("base: two-test suite")?;

    repo.write("crates/example/tests/extra.rs", TWO_TESTS)?;
    repo.write(
        "crates/example/src/lib.rs",
        &format!("{TWO_TESTS}\n#[test]\nfn brand_new() {{\n    assert!(true);\n}}\n"),
    )?;

    let out = run_check_test_deletion(repo.path(), &base)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "addition-only must pass\nstdout: {stdout}");
    Ok(())
}

/// A deletion paired with a justification marker on an added line passes — and
/// still lists what the marker covers so review stays honest.
#[test]
fn justified_removal_on_added_line_passes_and_lists_coverage() -> TestResult {
    let repo = TempGitRepo::new("justified-line")?;
    repo.write("crates/example/tests/gone.rs", TWO_TESTS)?;
    repo.write("crates/example/src/lib.rs", "pub fn kept() {}\n")?;
    let base = repo.commit_all("base: suite plus production module")?;

    repo.remove("crates/example/tests/gone.rs")?;
    repo.write(
        "crates/example/src/lib.rs",
        "pub fn kept() {}\n\n// TEST-DELETION-JUSTIFIED(#17405): obsolete suite removed with the legacy parser.\n",
    )?;

    let out = run_check_test_deletion(repo.path(), &base)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "marker-justified removal must pass\nstdout: {stdout}");
    assert!(
        stdout.contains("TEST-DELETION-JUSTIFIED(#17405)"),
        "must echo the covering marker\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("crates/example/tests/gone.rs"),
        "must still list what the marker covers\nstdout: {stdout}"
    );
    Ok(())
}

/// A deletion justified from a commit message in base..HEAD passes (the
/// committed-shape control for the uncommitted shape above).
#[test]
fn justified_removal_in_commit_message_passes() -> TestResult {
    let repo = TempGitRepo::new("justified-commit")?;
    repo.write("crates/example/tests/gone.rs", TWO_TESTS)?;
    let base = repo.commit_all("base: suite")?;

    repo.remove("crates/example/tests/gone.rs")?;
    repo.commit_all(
        "remove obsolete suite\n\nTEST-DELETION-JUSTIFIED(#17405): legacy parser deleted.",
    )?;

    let out = run_check_test_deletion(repo.path(), &base)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "commit-message-justified removal must pass\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("TEST-DELETION-JUSTIFIED(#17405)"),
        "must echo the covering marker\nstdout: {stdout}"
    );
    Ok(())
}

/// A marker that predates the change (already on base lines) justifies
/// nothing: markers cannot be pre-positioned.
#[test]
fn stale_base_side_marker_does_not_justify() -> TestResult {
    let repo = TempGitRepo::new("stale-marker")?;
    repo.write("crates/example/tests/gone.rs", TWO_TESTS)?;
    repo.write(
        "crates/example/src/lib.rs",
        "pub fn kept() {}\n\n// TEST-DELETION-JUSTIFIED(#17405): earlier, unrelated removal.\n",
    )?;
    let base = repo.commit_all("base: suite plus old marker")?;

    repo.remove("crates/example/tests/gone.rs")?;

    let out = run_check_test_deletion(repo.path(), &base)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        out.status.code(),
        Some(0),
        "stale marker must not justify a new removal\nstdout: {stdout}"
    );
    Ok(())
}

/// An explicitly named base that does not resolve is an error, not a pass.
#[test]
fn unresolvable_explicit_base_errors() -> TestResult {
    let repo = TempGitRepo::new("bad-base")?;
    repo.write("crates/example/src/lib.rs", TWO_TESTS)?;
    repo.commit_all("base: suite")?;

    let out = run_check_test_deletion(repo.path(), "does-not-exist-17405")?;
    assert_ne!(
        out.status.code(),
        Some(0),
        "unresolvable --base must error\nstdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does-not-exist-17405"),
        "must name the bad base ref\nstderr: {stderr}"
    );
    Ok(())
}
