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

/// Runs the guard against the change between `base` and the working tree, plus
/// the PR body when this run carries a GitHub event payload.
///
/// Returns the process exit code: `0` when nothing token-shaped was added,
/// `1` otherwise. Findings name file, line, and rule — never the matched
/// text.
///
/// # Errors
///
/// Returns an error when an explicitly requested base ref does not resolve,
/// when `git diff` cannot be executed, when the event payload cannot be read
/// or parsed, or when the allowlist exists but cannot be read.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        // Not a pass: nothing was compared. Said plainly so a green line is
        // never mistaken for evidence that no secret was added.
        println!(
            "{YELLOW}• secret scan not evaluated{NC}: no base ref resolved (tried: {}). \
             Pass --base to name one.",
            base_candidates().join(", ")
        );
        return Ok(0);
    };
    let base_sha = merge_base(repo_root, &requested_base);
    let diff = read_diff(repo_root, &base_sha)?;
    let allowlist = Allowlist::load(repo_root)?;
    let mut findings = secret_scan::scan_unified_diff(&diff, &allowlist)?;

    let body_scope = scan_pr_body_into(&mut findings)?;
    let allowlist_scope = if allowlist.is_empty() {
        "no allowlisted paths".to_owned()
    } else {
        format!("{} allowlisted paths", allowlist.len())
    };

    if findings.is_empty() {
        println!(
            "{GREEN}✅ No token-shaped secrets added{NC} (base: {requested_base}; {body_scope}; {allowlist_scope})"
        );
        return Ok(0);
    }

    println!(
        "{RED}❌ Secret scan found token-shaped additions{NC} (base: {requested_base}; {body_scope})"
    );
    for finding in &findings {
        println!("  {}:{} [{}]", finding.file, finding.line, finding.rule);
    }
    println!();
    println!("Rotate the credential if it is real, then remove it from the change entirely.");
    println!(
        "Inert test fixtures belong on the documented escape: list the repo-relative path in {} (one per line).",
        secret_scan::ALLOWLIST_PATH
    );
    Ok(1)
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
