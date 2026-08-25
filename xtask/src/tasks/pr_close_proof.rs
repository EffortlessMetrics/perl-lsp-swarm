//! `cargo xtask landing-proof` — canonical landing/content proof verifier.
//!
//! Implements the landing-proof layer of
//! [`docs/agents/CLOSE_PROOF_POLICY.md`](../../docs/agents/CLOSE_PROOF_POLICY.md):
//! it proves merge-base ancestry and, optionally, content survival. It never
//! evaluates semantic issue completion and never authorizes an issue close;
//! every receipt reports `semantic_completion: "not_evaluated"`.
//!
//! # Subcommand
//!
//! ```text
//! cargo xtask landing-proof --commit <sha> --canonical-main <ref>
//!                            [--substance-grep <string>]
//!                            [--format json]
//! ```
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |------|---------|
//! | `0` | Commit **is** an ancestor of canonical-main (landing proof passes) |
//! | `1` | Error (git not available, bad SHA format, I/O failure) |
//! | `2` | Commit is **not** an ancestor — landing proof failed |
//!
//! Exit code 2 is distinct from 1 (error) so callers can branch on ancestry
//! without conflating "not reachable" with "git failed".
//!
//! # Output (human, default)
//!
//! ```text
//! LANDING-PROVEN   sha abc1234 is ancestor of origin/main
//!                  semantic_completion: not_evaluated
//! ```
//! or
//! ```text
//! NOT-REACHABLE   sha abc1234 is NOT ancestor of origin/main
//!                 semantic_completion: not_evaluated
//! ```
//!
//! # Output (--format json)
//!
//! ```json
//! {
//!   "schema_version": "landing_proof.v1",
//!   "commit_reachable": true,
//!   "commit": "abc1234",
//!   "canonical_main": "origin/main",
//!   "content_survives": null,
//!   "semantic_completion": "not_evaluated"
//! }
//! ```
//! `content_survives` is only present when `--substance-grep` is passed.

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

// ---------------------------------------------------------------------------
// Public config
// ---------------------------------------------------------------------------

/// Output format for `landing-proof`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseProofFormat {
    Human,
    Json,
}

/// Configuration for `landing-proof`.
pub struct CloseProofConfig {
    /// The commit SHA to check.
    pub commit: String,
    /// The canonical main ref (e.g. `origin/main`).
    pub canonical_main: String,
    /// Optional substance grep string. When present, additionally runs
    /// `git grep -F <string> <canonical_main>` to detect the
    /// `in-ancestry-but-content-overwritten` class.
    pub substance_grep: Option<String>,
    /// Output format.
    pub format: CloseProofFormat,
}

// ---------------------------------------------------------------------------
// Receipt identity
// ---------------------------------------------------------------------------

/// Schema identity of the machine-readable landing-proof receipt.
///
/// Downstream semantic-close evidence admission consumes this exact value to
/// know which bounded evidence vocabulary it received. Bump the version when
/// the receipt vocabulary changes.
pub const LANDING_PROOF_SCHEMA_V1: &str = "landing_proof.v1";

/// The only `semantic_completion` value this command may ever emit.
///
/// Landing ancestry and content survival carry no semantic-close authority;
/// semantic issue completion is owned by the semantic-close contract.
pub const SEMANTIC_COMPLETION_NOT_EVALUATED: &str = "not_evaluated";

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// Machine-readable output for `--format json`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CloseProofOutput {
    /// Receipt schema identity; always `landing_proof.v1`.
    pub schema_version: String,
    /// Whether `commit` is an ancestor of `canonical_main`.
    pub commit_reachable: bool,
    /// The commit SHA that was checked.
    pub commit: String,
    /// The canonical main ref that was checked against.
    pub canonical_main: String,
    /// Whether the substance grep string still exists in `canonical_main`.
    /// `None` when `--substance-grep` was not provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_survives: Option<bool>,
    /// Semantic issue completion is deliberately outside this proof's authority.
    pub semantic_completion: String,
}

// ---------------------------------------------------------------------------
// Entry point — returns ExitCode so main.rs can propagate the exit code
// correctly without process::exit
// ---------------------------------------------------------------------------

/// Run the landing-proof check.
///
/// Exit behaviour (caller must honour the returned value):
/// - `Ok(true)` — commit **is** ancestor; caller exits 0
/// - `Ok(false)` — commit is **not** ancestor; caller exits 2
/// - `Err(_)` — git error; caller exits 1
///
/// The caller (main.rs dispatch) is responsible for calling
/// `std::process::exit(2)` when `Ok(false)` is returned, keeping the
/// exit-code contract (0/1/2) intact.
pub fn run(config: CloseProofConfig) -> Result<bool> {
    let reachable = check_ancestry(&config.commit, &config.canonical_main)?;

    let content_survives = if let Some(ref grep_str) = config.substance_grep {
        Some(check_substance(grep_str, &config.canonical_main)?)
    } else {
        None
    };

    let output = CloseProofOutput {
        schema_version: LANDING_PROOF_SCHEMA_V1.to_string(),
        commit_reachable: reachable,
        commit: config.commit.clone(),
        canonical_main: config.canonical_main.clone(),
        content_survives,
        semantic_completion: SEMANTIC_COMPLETION_NOT_EVALUATED.to_string(),
    };

    match config.format {
        CloseProofFormat::Json => {
            let json = serde_json::to_string_pretty(&output).context("serializing JSON output")?;
            println!("{json}");
        }
        CloseProofFormat::Human => {
            print_human(&output);
        }
    }

    Ok(reachable)
}

// ---------------------------------------------------------------------------
// git operations
// ---------------------------------------------------------------------------

/// Returns `true` if `commit` is an ancestor of `canonical_main`.
///
/// Runs: `git merge-base --is-ancestor <commit> <canonical_main>`
/// Exit 0 → is ancestor; exit 1 → not ancestor; other → error.
fn check_ancestry(commit: &str, canonical_main: &str) -> Result<bool> {
    validate_sha_format(commit)?;

    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, canonical_main])
        .status()
        .with_context(|| {
            format!("running `git merge-base --is-ancestor {commit} {canonical_main}`")
        })?;

    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(code) => {
            color_eyre::eyre::bail!(
                "`git merge-base --is-ancestor` exited with unexpected code {code}"
            )
        }
        None => color_eyre::eyre::bail!("`git merge-base --is-ancestor` killed by signal"),
    }
}

/// Returns `true` if `grep_str` appears in `canonical_main` (Rule 3 of CLOSE_PROOF_POLICY.md).
///
/// Runs: `git grep -F <grep_str> <canonical_main> -- :/`
///
/// The `:/` pathspec anchors the search at the repository root: without it
/// `git grep` silently limits the search to the caller's current directory,
/// which would make `content_survives` depend on where the command was run.
fn check_substance(grep_str: &str, canonical_main: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["grep", "-F", grep_str, canonical_main, "--", ":/"])
        .output()
        .with_context(|| format!("running `git grep -F {grep_str:?} {canonical_main}`"))?;

    // exit 0 = found, exit 1 = not found, other = error
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(code) => {
            color_eyre::eyre::bail!("`git grep` exited with unexpected code {code}")
        }
        None => color_eyre::eyre::bail!("`git grep` killed by signal"),
    }
}

/// Minimal SHA validation — rejects obviously wrong inputs (empty, contains spaces,
/// obviously not hex).  Does not require exactly 40 chars to support short SHAs.
fn validate_sha_format(sha: &str) -> Result<()> {
    if sha.trim().is_empty() {
        color_eyre::eyre::bail!("commit SHA must not be empty");
    }
    if sha.contains(' ') || sha.contains('\t') {
        color_eyre::eyre::bail!("commit SHA must not contain whitespace: {sha:?}");
    }
    // Accept hex chars only (short or full SHA).
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        color_eyre::eyre::bail!("commit SHA must be hex characters only, got: {sha:?}");
    }
    if sha.len() < 7 {
        color_eyre::eyre::bail!("commit SHA too short (minimum 7 chars): {sha:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Human output
// ---------------------------------------------------------------------------

fn print_human(output: &CloseProofOutput) {
    if output.commit_reachable {
        println!("LANDING-PROVEN   {} is ancestor of {}", output.commit, output.canonical_main);
        println!("            semantic_completion: not_evaluated");
        if let Some(survives) = output.content_survives {
            if survives {
                println!(
                    "            content_survives: YES — substance grep found in {}",
                    output.canonical_main
                );
            } else {
                println!(
                    "WARNING     content_survives: NO — commit is ancestor but substance was overwritten"
                );
                println!(
                    "            Class: in-ancestry-but-content-overwritten (see CLOSE_PROOF_POLICY.md Rule 3)"
                );
            }
        }
    } else {
        println!(
            "NOT-REACHABLE   {} is NOT an ancestor of {}",
            output.commit, output.canonical_main
        );
        println!("                semantic_completion: not_evaluated");
        println!("                Landing proof failed — commit has not landed on canonical main.");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- SHA validation ---------------------------------------------------

    #[test]
    fn test_validate_sha_empty_fails() -> Result<()> {
        assert!(validate_sha_format("").is_err());
        assert!(validate_sha_format("   ").is_err());
        Ok(())
    }

    #[test]
    fn test_validate_sha_with_spaces_fails() -> Result<()> {
        assert!(validate_sha_format("abc 123").is_err());
        Ok(())
    }

    #[test]
    fn test_validate_sha_non_hex_fails() -> Result<()> {
        assert!(validate_sha_format("not-a-sha").is_err());
        assert!(validate_sha_format("zzzzzzzz").is_err());
        Ok(())
    }

    #[test]
    fn test_validate_sha_too_short_fails() -> Result<()> {
        assert!(validate_sha_format("abc123").is_err()); // 6 chars
        Ok(())
    }

    #[test]
    fn test_validate_sha_valid_short() -> Result<()> {
        validate_sha_format("abc1234")?; // 7-char short SHA
        Ok(())
    }

    #[test]
    fn test_validate_sha_valid_full() -> Result<()> {
        validate_sha_format("4b271dff5a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d")?;
        Ok(())
    }

    // ----- Output structure -------------------------------------------------

    fn receipt(reachable: bool, content_survives: Option<bool>) -> CloseProofOutput {
        CloseProofOutput {
            schema_version: LANDING_PROOF_SCHEMA_V1.to_string(),
            commit_reachable: reachable,
            commit: "abc1234d".to_string(),
            canonical_main: "origin/main".to_string(),
            content_survives,
            semantic_completion: SEMANTIC_COMPLETION_NOT_EVALUATED.to_string(),
        }
    }

    #[test]
    fn test_landing_proof_v1_schema_is_ratcheted() -> Result<()> {
        let output = receipt(true, Some(true));
        let json: serde_json::Value = serde_json::from_str(&serde_json::to_string(&output)?)?;

        assert_eq!(json["schema_version"], "landing_proof.v1");
        assert_eq!(json["semantic_completion"], "not_evaluated");
        assert_eq!(json["commit_reachable"], true);
        assert_eq!(json["content_survives"], true);

        let mut keys: Vec<&str> =
            json.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "canonical_main",
                "commit",
                "commit_reachable",
                "content_survives",
                "schema_version",
                "semantic_completion"
            ]
        );
        Ok(())
    }

    #[test]
    fn test_receipt_never_emits_close_authority_vocabulary() -> Result<()> {
        // #10381: neither ancestry success nor content survival may reintroduce
        // the old `allowed_close_reasons` / "safe to close" vocabulary.
        for reachable in [true, false] {
            for content in [None, Some(true), Some(false)] {
                let json = serde_json::to_string(&receipt(reachable, content))?;
                assert!(!json.contains("allowed_close_reasons"), "json: {json}");
                assert!(!json.contains("safe to close"), "json: {json}");
            }
        }
        Ok(())
    }

    #[test]
    fn test_semantic_completion_not_evaluated_for_every_result() -> Result<()> {
        // The load-bearing negative assertion: every possible landing/content
        // result leaves semantic completion unevaluated.
        for reachable in [true, false] {
            for content in [None, Some(true), Some(false)] {
                let output = receipt(reachable, content);
                assert_eq!(
                    output.semantic_completion, SEMANTIC_COMPLETION_NOT_EVALUATED,
                    "reachable={reachable} content_survives={content:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn test_output_json_serialization_roundtrip() -> Result<()> {
        let output = receipt(true, Some(true));
        let json = serde_json::to_string(&output)?;
        let parsed: CloseProofOutput = serde_json::from_str(&json)?;
        assert_eq!(output, parsed);
        Ok(())
    }

    #[test]
    fn test_content_survives_omitted_when_none() -> Result<()> {
        let output = receipt(true, None);
        let json = serde_json::to_string(&output)?;
        assert!(!json.contains("content_survives"), "should be omitted when None");
        assert!(json.contains("semantic_completion"));
        assert!(json.contains("not_evaluated"));
        Ok(())
    }

    // ----- Live repo ancestry checks ----------------------------------------
    //
    // Uses known commits from the repo's own history.  These are integration
    // tests — they shell to git and require the repo to be available.

    #[test]
    fn test_current_head_is_ancestor_of_itself() -> Result<()> {
        let output = Command::new("git").args(["rev-parse", "--verify", "HEAD^{commit}"]).output();
        let Ok(output) = output else {
            // git not available — skip
            return Ok(());
        };
        if !output.status.success() {
            // not a git checkout, shallow probe failure, etc. — skip
            return Ok(());
        }

        let sha = String::from_utf8(output.stdout)?.trim().to_string();
        if sha.is_empty() {
            return Ok(());
        }

        let reachable = check_ancestry(&sha, &sha)?;
        assert!(reachable, "current HEAD should be an ancestor of itself");
        Ok(())
    }

    #[test]
    fn test_fabricated_sha_is_not_ancestor_or_error() -> Result<()> {
        // A SHA that cannot exist in the repo.  Either "not ancestor" (exit 1)
        // or "object not found" which git reports as exit 128.
        // We accept both: check_ancestry returns Ok(false) or Err.
        let sha = "0000000000000000000000000000000000000000";
        // validate_sha_format should pass (it's hex).
        validate_sha_format(sha)?;
        // The actual ancestry check may error or return false — both are fine.
        // We just verify it doesn't claim "reachable".
        match check_ancestry(sha, "origin/main") {
            Ok(true) => panic!("all-zeros SHA should not be an ancestor"),
            Ok(false) => {} // expected: not ancestor
            Err(_) => {}    // also fine: git rejected it
        }
        Ok(())
    }

    // ----- Failure-state discrimination --------------------------------------
    //
    // #10381: the command must keep "not reachable", "content overwritten",
    // "malformed input", and "git/instrument failure" distinct, and none of
    // them may become semantic completion.

    /// Resolve `refname` to a full SHA, returning `None` when git or the ref
    /// is unavailable (tests degrade to skip in that case).
    fn resolve_ref(refname: &str) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("{refname}^{{commit}}")])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
        if sha.is_empty() { None } else { Some(sha) }
    }

    /// First root commit reachable from HEAD, `None` when unavailable.
    fn root_commit() -> Option<String> {
        let output =
            Command::new("git").args(["rev-list", "--max-parents=0", "HEAD"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8(output.stdout).ok()?;
        stdout.lines().next().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string)
    }

    #[test]
    fn test_malformed_sha_is_input_failure_not_unreachable() -> Result<()> {
        // Malformed input must be an error (exit-1 class), never Ok(false)
        // and never Ok(true).
        let result = check_ancestry("not-a-sha", "HEAD");
        assert!(result.is_err(), "malformed SHA must be an instrument/input failure");
        Ok(())
    }

    #[test]
    fn test_unknown_ref_is_instrument_failure_not_unreachable() -> Result<()> {
        let Some(sha) = resolve_ref("HEAD") else { return Ok(()) };
        // A ref that cannot exist: git merge-base exits 128, which must
        // surface as Err (instrument failure), not Ok(false).
        let result = check_ancestry(&sha, "refs/definitely/missing-10381");
        assert!(result.is_err(), "unknown canonical ref must be an instrument failure");
        Ok(())
    }

    #[test]
    fn test_head_is_not_ancestor_of_root_commit() -> Result<()> {
        let (Some(head), Some(root)) = (resolve_ref("HEAD"), root_commit()) else {
            return Ok(());
        };
        if head == root {
            // Single-commit repo: property does not apply.
            return Ok(());
        }
        let reachable = check_ancestry(&head, &root)?;
        assert!(!reachable, "HEAD cannot be an ancestor of its own root commit");
        Ok(())
    }

    #[test]
    fn test_content_survival_true_and_false_are_distinct() -> Result<()> {
        if resolve_ref("HEAD").is_none() {
            return Ok(());
        }
        // "Close-Proof Policy" is the title of docs/agents/CLOSE_PROOF_POLICY.md,
        // present on main long before this change. The absent needle is
        // assembled at runtime so it never appears contiguously in the tree —
        // a literal here would be found by the grep once this file is committed.
        let absent_needle = format!("zzz-10381-{}-definitely-{}", "substance", "absent");
        let present = check_substance("Close-Proof Policy", "HEAD")?;
        let absent = check_substance(&absent_needle, "HEAD")?;
        assert!(present, "known-present string must survive in HEAD");
        assert!(!absent, "crafted-absent string must not survive in HEAD");
        Ok(())
    }

    #[test]
    fn test_run_reachable_receipt_is_landing_only() -> Result<()> {
        let Some(head) = resolve_ref("HEAD") else { return Ok(()) };
        let reachable = run(CloseProofConfig {
            commit: head.clone(),
            canonical_main: head,
            substance_grep: None,
            format: CloseProofFormat::Json,
        })?;
        assert!(reachable, "HEAD is an ancestor of itself");
        Ok(())
    }

    #[test]
    fn test_run_malformed_sha_fails_before_git() -> Result<()> {
        let result = run(CloseProofConfig {
            commit: "not-a-sha".to_string(),
            canonical_main: "origin/main".to_string(),
            substance_grep: None,
            format: CloseProofFormat::Json,
        });
        assert!(result.is_err(), "malformed SHA must fail as input error");
        Ok(())
    }
}
