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

/// Repository-location variables that redirect git away from the caller's
/// directory: `GIT_DIR`/`GIT_WORK_TREE`/`GIT_COMMON_DIR` retarget the
/// repository itself (the set `scripts/cargo_admitted.py` refuses to run
/// under), and the index/object/prefix overrides redirect the remaining
/// lookup. Git exports these when it invokes hooks, and the generated
/// pre-push hook runs `cargo test` — without the strip below, a
/// hook-launched suite would `git init`/`commit` fixture content into the
/// developer's real repository and the gate would scan the wrong tree.
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

fn git(repo: &Path, args: &[&str]) -> TestResult<String> {
    let mut command = Command::new("git");
    scrub_git_location(&mut command);
    let output = command.args(args).current_dir(repo).output()?;
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
/// developer's (or CI's) ambient `CI` / `GITHUB_BASE_REF` / `CI_SCOPE_BASE` /
/// `GITHUB_EVENT_NAME` / `GITHUB_EVENT_PATH` cannot leak into the fixture.
/// Pass `ci = Some("1")` for the CI posture. Extra variables (e.g. a push
/// event payload) layer on after the scrub.
fn run_check(repo: &Path, args: &[&str], ci: Option<&str>) -> TestResult<Output> {
    run_check_with_env(repo, args, ci, &[])
}

/// [`run_check`] with additional environment layered on after the scrub, so
/// tests can simulate one wired variable (like the push event payload)
/// while the rest of the ambient environment stays out.
fn run_check_with_env(
    repo: &Path,
    args: &[&str],
    ci: Option<&str>,
    extra_env: &[(&str, &str)],
) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    let mut command = Command::new(bin);
    command
        .arg("check-no-verify-recover")
        .args(args)
        .current_dir(repo)
        .env_remove("CI")
        .env_remove("CI_SCOPE_BASE")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_EVENT_PATH");
    scrub_git_location(&mut command);
    if let Some(value) = ci {
        command.env("CI", value);
    }
    for (key, value) in extra_env {
        command.env(key, value);
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

#[test]
fn ci_multi_commit_fixture_without_baseline_fails_closed() -> TestResult {
    // (1a): a multi-commit push with no resolvable main line must not narrow
    // to HEAD~1 in CI — the earlier violating commit would escape the scan
    // and the gate would report green. With the tail excluded, nothing
    // resolves and the run fails closed instead.
    let repo = TempRepo::new("multi-commit-ci")?;
    repo.commit_as(
        "file.txt",
        "smuggled\n",
        "earlier bypassed commit",
        "xtask hook tests",
        "xtask@example.invalid",
    )?;
    repo.commit("file.txt", "clean head\n", "clean head commit")?;

    let output = run_check(repo.path(), &[], Some("1"))?;

    assert!(
        !output.status.success(),
        "CI without a baseline must fail closed, not scan just HEAD: {}",
        combined(&output)
    );
    assert!(
        combined(&output).contains("cannot resolve a merge baseline in CI"),
        "{}",
        combined(&output)
    );
    Ok(())
}

#[test]
fn local_multi_commit_fixture_still_scans_previous_commit() -> TestResult {
    // (1a) counterpart: the HEAD~1 tail stays for local runs, so a developer
    // re-running the gate without --base still scans the last commit.
    let repo = TempRepo::new("multi-commit-local")?;
    repo.commit("file.txt", "base\n", "base commit")?;
    repo.commit("file.txt", "second\n", "second commit")?;

    let output = run_check(repo.path(), &[], None)?;

    assert!(output.status.success(), "a clean local range must pass: {}", combined(&output));
    assert!(
        stdout(&output).contains("1 commit(s) scanned"),
        "local auto-resolution must keep the HEAD~1 fallback: {}",
        combined(&output)
    );
    Ok(())
}

#[test]
fn ci_scope_base_overrides_branch_ref_resolving_to_head() -> TestResult {
    // A manual CI_SCOPE_BASE override wins candidate selection, so an
    // operator can always name the honest baseline explicitly.
    let repo = TempRepo::new("scope-base-wins")?;
    let base = repo.commit("file.txt", "base\n", "base commit")?;
    let violating = repo.commit_as(
        "file.txt",
        "smuggled\n",
        "bypassed commit gate",
        "xtask hook tests",
        "xtask@example.invalid",
    )?;
    repo.commit("file.txt", "head\n", "head commit")?;
    // Simulate the post-push shape: a main-line ref sitting exactly at HEAD.
    git(repo.path(), &["update-ref", "refs/heads/main", "HEAD"])?;

    let output = run_check_with_env(repo.path(), &[], Some("1"), &[("CI_SCOPE_BASE", &base)])?;

    assert!(
        !output.status.success(),
        "the override previous tip must expose the pushed-range violation: {}",
        combined(&output)
    );
    let text = stdout(&output);
    assert!(text.contains(&violating), "missing violating sha: {text}");
    assert!(text.contains(&base), "the scan must use the override base: {text}");
    Ok(())
}

#[test]
fn push_event_payload_previous_tip_resolves_the_range() -> TestResult {
    // (1b): on a push to main, origin/main can resolve to HEAD, which would
    // scan an empty HEAD..HEAD range. The gate reads the push event's
    // previous tip from the GITHUB_EVENT_PATH payload — no workflow wiring,
    // which ci_subject forbids — so the pushed range is actually scanned.
    let repo = TempRepo::new("payload-base-wins")?;
    let base = repo.commit("file.txt", "base\n", "base commit")?;
    let violating = repo.commit_as(
        "file.txt",
        "smuggled\n",
        "bypassed commit gate",
        "xtask hook tests",
        "xtask@example.invalid",
    )?;
    repo.commit("file.txt", "head\n", "head commit")?;
    // Simulate the post-push shape: a main-line ref sitting exactly at HEAD.
    git(repo.path(), &["update-ref", "refs/heads/main", "HEAD"])?;
    let payload_path = repo.path().join("event.json");
    fs::write(&payload_path, format!("{{\"before\": \"{base}\", \"ref\": \"x\"}}"))?;
    let payload_str = payload_path.to_string_lossy().into_owned();

    let output = run_check_with_env(
        repo.path(),
        &[],
        Some("1"),
        &[("GITHUB_EVENT_NAME", "push"), ("GITHUB_EVENT_PATH", payload_str.as_str())],
    )?;

    assert!(
        !output.status.success(),
        "the payload previous tip must expose the pushed-range violation: {}",
        combined(&output)
    );
    let text = stdout(&output);
    assert!(text.contains(&violating), "missing violating sha: {text}");
    assert!(text.contains(&base), "the scan must use the payload base: {text}");
    Ok(())
}

/// Marker the hostile-environment parent sets in the child probe's
/// environment, carrying the hostile repository's path. Absent in normal
/// suite runs, where the probe is a no-op pass.
const HOSTILE_PROBE_MARKER: &str = "PERL_NO_VERIFY_HOSTILE_PROBE";

#[test]
fn fixture_commands_ignore_hostile_git_location_env() -> TestResult {
    // The generated pre-push hook runs `cargo test`, and git exports
    // repository-location variables to hooks. Re-enter this test binary as a
    // child carrying the full hostile set (pointed at a real scratch repo, so
    // a leak would land fixture commits there observably) and prove the
    // fixture helpers still operate on their own temp repository. A child
    // process — not parent `set_var` — keeps this parallel-safe and
    // serial-ratchet-clean.
    let hostile = TempRepo::new("hostile-target")?;
    hostile.commit("real.txt", "real\n", "hostile baseline")?;
    let hostile_git_dir = hostile.path().join(".git");

    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg("hostile_git_env_probe_child_entry")
        .arg("--exact")
        .arg("--nocapture")
        .env("GIT_DIR", &hostile_git_dir)
        .env("GIT_WORK_TREE", hostile.path())
        .env("GIT_COMMON_DIR", &hostile_git_dir)
        .env("GIT_INDEX_FILE", hostile_git_dir.join("index"))
        .env("GIT_OBJECT_DIRECTORY", hostile_git_dir.join("objects"))
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", "")
        .env("GIT_PREFIX", "")
        .env(HOSTILE_PROBE_MARKER, hostile.path());
    let output = command.output()?;
    assert!(
        output.status.success(),
        "fixture helpers must survive hostile git-location env: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The hostile repo must be untouched: no fixture commit may have landed
    // in it. Read it with a clean observer (explicit removals, not the
    // helpers under test) so a broken helper cannot mask its own leak.
    let subjects = clean_observer_log(hostile.path())?;
    assert!(
        !subjects.contains("hostile-probe-marker"),
        "fixture commit leaked into the hostile repo: {subjects}"
    );
    Ok(())
}

/// Reads `git log --format=%s` with repository-location overrides explicitly
/// removed: a clean observer for asserting where commits actually landed.
fn clean_observer_log(repo: &Path) -> TestResult<String> {
    let mut command = Command::new("git");
    scrub_git_location(&mut command);
    let output = command.args(["log", "--format=%s"]).current_dir(repo).output()?;
    if !output.status.success() {
        return Err(format!(
            "observer git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn hostile_git_env_probe_child_entry() -> TestResult {
    // Child entry for `fixture_commands_ignore_hostile_git_location_env`,
    // re-executed with hostile git-location variables inherited. A no-op
    // pass when the marker is absent (ordinary suite runs).
    let hostile = match env::var_os(HOSTILE_PROBE_MARKER) {
        Some(path) => PathBuf::from(path),
        None => return Ok(()),
    };

    let repo = TempRepo::new("hostile-probe-fixture")?;
    let base = repo.commit("file.txt", "base\n", "hostile-probe-marker base")?;
    repo.commit_as(
        "file.txt",
        "smuggled\n",
        "hostile-probe-marker bypassed commit",
        "xtask hook tests",
        "xtask@example.invalid",
    )?;

    // The fixture commits must exist in the fixture repo (clean-observer
    // read: the helpers under test must not be their own witness here).
    assert!(repo.path().join(".git").is_dir(), "fixture repo was not initialized");
    let subjects = clean_observer_log(repo.path())?;
    assert!(
        subjects.contains("hostile-probe-marker bypassed commit"),
        "fixture commit missing from the fixture repo: {subjects}"
    );

    // And the gate itself must scan the fixture repo, not the hostile one:
    // the violation is in the fixture, so a correct scan fails naming it,
    // while a scan redirected into the clean hostile repo would pass.
    let output = run_check(repo.path(), &["--base", &base], None)?;
    assert!(
        !output.status.success(),
        "the gate must scan the fixture repo under hostile env: {}",
        combined(&output)
    );
    assert!(
        stdout(&output).contains("xtask hook tests"),
        "missing identity value: {}",
        combined(&output)
    );

    // Belt and braces inside the child too: nothing fixture-shaped in H.
    let hostile_subjects = clean_observer_log(&hostile)?;
    assert!(
        !hostile_subjects.contains("hostile-probe-marker"),
        "fixture commit leaked into the hostile repo: {hostile_subjects}"
    );
    Ok(())
}
