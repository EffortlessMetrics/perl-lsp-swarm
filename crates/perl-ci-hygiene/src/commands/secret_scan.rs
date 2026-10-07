//! Required-gate entry for the token-shape secret scan (#17428).
//!
//! The local hooks refuse token-shaped staged and pushed content with zero
//! build; this command re-scans the same shapes over the PR diff plus the PR
//! body server-side, so `--no-verify` cannot bypass both layers. Base
//! resolution is shared with the must-context guard; the shapes, the diff
//! walk, and the PR-body extractor live in the `secret_scan` lib module,
//! where the gate-run `--lib` tests enforce them.

use color_eyre::eyre::{Result, eyre};
use perl_ci_hygiene::secret_scan::{self, Allowlist};
use std::path::Path;
use std::process::Command;

use super::must_context::{base_candidates, merge_base, resolve_base};
use crate::{GREEN, NC, RED, YELLOW};

/// Pseudo-path attributing PR title/body findings.
const PR_BODY_FILE: &str = "pull-request body";

/// SHA of the empty tree, the "everything is new" scope for a baseless run.
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Runs the guard against the change between `base` and the working tree, plus
/// the PR body when this run carries a GitHub event payload.
///
/// The endpoint diff misses a secret added and removed inside the range, so
/// every commit's tree delta between the merge base and `HEAD` is scanned too
/// (merges split per parent); findings identical across legs are reported
/// once. Findings name file, line, and rule — never the matched text.
///
/// Returns the process exit code: `0` when nothing token-shaped was added,
/// `1` otherwise.
///
/// # Errors
///
/// Returns an error when an explicitly requested base ref does not resolve,
/// when `git diff` or `git log` cannot be executed, when the event payload
/// cannot be read or parsed, when the allowlist exists but cannot be read, or
/// — in CI only — when no base ref resolves. That last arm is the fail-closed
/// direction for a required gate: an unevaluated run must never report green
/// where a merge decision reads it. Outside CI the same state stays exit `0`
/// with a plainly-worded "not evaluated" line, since a local checkout without
/// any candidate base (a fresh single-commit tree, an unborn `HEAD`) has no
/// subject to judge and must not fail the contributor's gate.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        // Not a pass: nothing was compared. Said plainly so a green line is
        // never mistaken for evidence that no secret was added — and in CI,
        // not a pass at all: the required gate fails instead of reporting
        // unevaluated green.
        return no_base_outcome(is_ci(), &base_candidates().join(", "));
    };
    let base_sha = merge_base(repo_root, &requested_base);
    let diff = read_diff(repo_root, &base_sha)?;
    let history = read_history_diff(repo_root, &base_sha)?;
    let allowlist = Allowlist::load(repo_root)?;
    let mut findings = secret_scan::scan_unified_diff(&diff, &allowlist)?;
    let history_commits = count_history_commits(&history);
    findings.extend(secret_scan::scan_unified_diff(&history, &allowlist)?);
    dedupe_findings(&mut findings);

    let body_scope = scan_pr_body_into(&mut findings)?;
    let history_scope = if history_commits == 0 {
        "no prior commits in range".to_owned()
    } else if history_commits == 1 {
        "1 prior commit scanned".to_owned()
    } else {
        format!("{history_commits} prior commits scanned")
    };
    let allowlist_scope = if allowlist.is_empty() {
        "no allowlisted paths".to_owned()
    } else {
        format!("{} allowlisted paths", allowlist.len())
    };

    if findings.is_empty() {
        println!(
            "{GREEN}✅ No token-shaped secrets added{NC} (base: {requested_base}; {body_scope}; {history_scope}; {allowlist_scope})"
        );
        return Ok(0);
    }

    println!(
        "{RED}❌ Secret scan found token-shaped additions{NC} (base: {requested_base}; {body_scope}; {history_scope})"
    );
    for finding in &findings {
        println!("  {}:{} [{}]", finding.file, finding.line, finding.rule);
    }
    println!();
    println!("Rotate the credential if it is real, then remove it from the change entirely.");
    println!(
        "History findings name the line where the secret was introduced: rotate even when a later commit already removed it."
    );
    println!(
        "Inert test fixtures belong on the documented escape: list the repo-relative path in {} (one per line).",
        secret_scan::ALLOWLIST_PATH
    );
    Ok(1)
}

/// Reports whether this run executes under CI, where an unevaluated required
/// gate must fail rather than pass.
fn is_ci() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some() || std::env::var_os("CI").is_some()
}

/// Outcome of a run that resolved no base ref: fail closed in CI, exit `0`
/// with a plainly-worded notice locally.
///
/// Split from [`check`] so both arms are unit-testable without mutating
/// process-global environment in parallel tests; `ci` is [`is_ci`].
fn no_base_outcome(ci: bool, tried: &str) -> Result<i32> {
    if ci {
        println!(
            "{RED}❌ Secret scan not evaluated{NC}: no base ref resolved in CI (tried: {tried}). \
             Failing closed rather than passing an unscanned change."
        );
        return Err(eyre!(
            "secret scan evaluated nothing: no base ref resolved (tried: {tried}); pass --base to name one"
        ));
    }
    println!(
        "{YELLOW}• secret scan not evaluated{NC}: no base ref resolved (tried: {tried}). \
         Pass --base to name one."
    );
    Ok(0)
}

/// Drops findings identical across the endpoint and history legs, keeping
/// first-seen order. A secret still present at `HEAD` is found by both legs;
/// reporting it twice would read as two secrets.
fn dedupe_findings(findings: &mut Vec<secret_scan::SecretFinding>) {
    let mut seen = std::collections::HashSet::new();
    findings.retain(|finding| seen.insert((finding.file.clone(), finding.line, finding.rule)));
}

/// Scans the PR title and body when a GitHub event payload is available.
///
/// Returns a scope fragment for the gate log. A run without `GITHUB_EVENT_PATH`
/// (local use, push and schedule events) scans the diff only; a payload that
/// names no pull request scans the diff only and says so.
fn scan_pr_body_into(findings: &mut Vec<secret_scan::SecretFinding>) -> Result<String> {
    let Ok(event_path) = std::env::var("GITHUB_EVENT_PATH") else {
        return Ok("no PR event body".to_owned());
    };
    let event = std::fs::read_to_string(&event_path).map_err(|error| {
        eyre!("failed to read the GitHub event payload at '{event_path}': {error}")
    })?;
    let Some(text) = secret_scan::extract_pr_body(&event)? else {
        return Ok("event carries no pull request".to_owned());
    };
    findings.extend(secret_scan::scan_text(PR_BODY_FILE, &text)?);
    Ok("PR body scanned".to_owned())
}

/// Reads the whole-tree diff from `base` to the working tree, zero context.
///
/// Zero context keeps hunks minimal and the read small; context lines are
/// never scanned. `base` is already the merge base, and the range is two-dot
/// so staged and unstaged edits are included.
///
/// The `a/`/`b/` prefixes are pinned explicitly. The scanner finds file
/// boundaries by the ` b/` separator, so an ambient `diff.noprefix=true`
/// would emit boundaries it cannot attribute and silently report every file
/// as clean — a false green driven by config the guard does not own.
fn read_diff(repo_root: &Path, base: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--unified=0", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", base])
        .output()
        .map_err(|error| eyre!("failed to run `git diff` against '{base}': {error}"))?;

    if !output.status.success() {
        return Err(eyre!(
            "`git diff {base}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Reads every commit's tree delta between `base` and `HEAD` as one patch
/// stream, so a secret added and removed inside the range is still scanned.
///
/// `-m` splits merge commits per parent; `--format=commit %H` labels each
/// commit with a line the scanner ignores (it is neither a boundary, a hunk
/// header, nor an added line). With an empty-tree base the range is `HEAD`
/// alone, since a tree sha is not a revision endpoint — and with an unborn
/// `HEAD` there are no commits at all, so a failed `git log` degrades to an
/// empty history rather than erroring a subject that cannot exist. Any other
/// failure fails the gate closed.
fn read_history_diff(repo_root: &Path, base: &str) -> Result<String> {
    let range = if base == EMPTY_TREE_SHA { "HEAD".to_owned() } else { format!("{base}..HEAD") };
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "log",
            "-p",
            "-m",
            "--format=commit %H",
            "--no-color",
            "--unified=0",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &range,
        ])
        .output()
        .map_err(|error| eyre!("failed to run `git log` for '{range}': {error}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    if super::must_context::ref_exists(repo_root, "HEAD") {
        return Err(eyre!(
            "`git log {range}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::new())
}

/// Counts the `commit <sha>` format lines in history output.
///
/// Patch lines can never collide: added lines carry a `+` marker, context a
/// leading space, and removed lines a `-`.
fn count_history_commits(history: &str) -> usize {
    history.lines().filter(|line| line.starts_with("commit ")).count()
}

#[cfg(test)]
mod tests {
    use super::{check, count_history_commits, dedupe_findings, no_base_outcome};
    use color_eyre::eyre::Result;
    use perl_ci_hygiene::secret_scan::SecretFinding;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Reports a secret added and removed inside the range: the endpoint diff
    /// is clean, so only the history leg can find it.
    #[test]
    fn check_reports_a_secret_removed_inside_the_range() -> Result<()> {
        let repo = fixture_repo()?;
        let token = format!("ghp_{}", "D".repeat(36));
        commit_file(&repo, "ok.rs", "benign content\n", "benign base")?;
        let base = rev_parse(&repo, "HEAD")?;
        commit_file(&repo, "leak.rs", &format!("token = \"{token}\"\n"), "add token")?;
        commit_file(&repo, "leak.rs", "token = \"rotated\"\n", "remove token")?;

        let code = check(&repo, Some(&base))?;

        assert_eq!(code, 1, "a secret removed inside the range must still be reported");
        std::fs::remove_dir_all(repo)?;
        Ok(())
    }

    /// Reports nothing on a clean tree.
    #[test]
    fn check_is_quiet_on_a_clean_tree() -> Result<()> {
        // Under CI the runner's own event payload is in subject too; its
        // content is not this test's fixture, so the quiet assertion only
        // runs where no event payload is present.
        if std::env::var_os("GITHUB_EVENT_PATH").is_some() {
            return Ok(());
        }
        let repo = fixture_repo()?;
        commit_file(&repo, "ok.rs", "benign content\n", "benign work")?;
        let base = rev_parse(&repo, "HEAD")?;

        let code = check(&repo, Some(&base))?;

        assert_eq!(code, 0, "a clean tree must scan quiet");
        std::fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn check_without_any_base_is_unevaluated_locally() -> Result<()> {
        // A fixture with no commits and no refs resolves no candidate base.
        // The local arm is exit 0 with a notice; under CI the same state
        // fails closed, which no_base_outcome_fails_closed_in_ci pins
        // without touching process-global environment.
        if super::is_ci() {
            return Ok(());
        }
        let repo = fixture_repo()?;

        let code = check(&repo, None)?;

        assert_eq!(code, 0, "a baseless local run stays exit 0 with a notice");
        std::fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn no_base_outcome_fails_closed_in_ci() -> Result<()> {
        use color_eyre::eyre::eyre;

        assert_eq!(no_base_outcome(false, "origin/main, main")?, 0);
        let Err(error) = no_base_outcome(true, "origin/main, main") else {
            return Err(eyre!("expected the CI arm to fail closed"));
        };
        assert!(
            error.to_string().contains("origin/main, main"),
            "the CI failure must name the tried candidates: {error}"
        );
        Ok(())
    }

    #[test]
    fn dedupe_keeps_first_and_drops_identical_findings() {
        let finding =
            |line: usize| SecretFinding { file: "leak.rs".to_owned(), line, rule: "github-token" };
        let mut findings = vec![finding(10), finding(12), finding(10)];

        dedupe_findings(&mut findings);

        assert_eq!(findings, vec![finding(10), finding(12)]);
    }

    #[test]
    fn history_commit_count_ignores_patch_lines() {
        let history = "commit abc123\ndiff --git a/leak.rs b/leak.rs\n@@ -1 +1 @@\n+commitizen = true\n context\ncommit def456\n";
        assert_eq!(count_history_commits(history), 2);
        assert_eq!(count_history_commits(""), 0);
    }

    fn fixture_repo() -> Result<PathBuf> {
        let repo = std::env::temp_dir().join(format!(
            "perl-ci-hygiene-secret-scan-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir_all(&repo)?;
        git(&repo, &["init", "--quiet", "-b", "main"])?;
        git(&repo, &["config", "user.email", "secret-scan-test@example.invalid"])?;
        git(&repo, &["config", "user.name", "secret-scan-test"])?;
        git(&repo, &["config", "commit.gpgsign", "false"])?;
        Ok(repo)
    }

    fn commit_file(repo: &Path, name: &str, content: &str, message: &str) -> Result<()> {
        std::fs::write(repo.join(name), content)?;
        git(repo, &["add", name])?;
        git(repo, &["commit", "--quiet", "-m", message])?;
        Ok(())
    }

    fn rev_parse(repo: &Path, reference: &str) -> Result<String> {
        let output =
            Command::new("git").current_dir(repo).args(["rev-parse", reference]).output()?;
        if !output.status.success() {
            return Err(color_eyre::eyre::eyre!(
                "git rev-parse {reference} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn git(repo: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git").current_dir(repo).args(args).status()?;
        if !status.success() {
            return Err(color_eyre::eyre::eyre!("git {} failed", args.join(" ")));
        }
        Ok(())
    }
}
