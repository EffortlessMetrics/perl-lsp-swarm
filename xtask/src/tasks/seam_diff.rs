//! Seam-diff reporter — advisory, read-only slice 1 of issue #3986.
//!
//! `cargo xtask seam-diff --base <epochSHA> [--head <sha>] [--format
//! json|human]` composes the #3985 `change_set::resolve_change_set_with_mode`
//! resolver over a `CommitRange { base: epochSHA, head }` identity to
//! report which "seams" (changed files, plus a coarse changed-crate set) a
//! push changed since a recorded review-epoch marker SHA. It is pure
//! read-only reporting: it registers one new xtask subcommand and prints a
//! report. It changes **no** bot trigger, **no** required check, **no**
//! branch-protection rule, and nothing about what merges — see the
//! "review-epoch marker" convention doc this slice ships alongside
//! (`.claude/reference/review-convergence.md` § Review-epoch markers).
//!
//! # Composition, not duplication
//!
//! This module does not reimplement `git diff` or base-ref resolution — it
//! calls [`change_set::resolve_change_set_with_mode`] exactly like the
//! `change-set` CLI calls [`change_set::resolve_change_set`]
//! (`xtask/src/tasks/change_set.rs`). `ChangeSet` (as read from that
//! module) exposes only `identity`/`base_sha`/`head_sha`/`changed_paths` —
//! no crate or docs/code classification — so:
//!
//! - **`changed_crates`** is derived here from `crates/<name>/...` path
//!   prefixes only (see [`derive_changed_crates`]'s doc comment for why
//!   this is a lightweight derivation, not `ci_scope`'s cargo-metadata-aware
//!   dependency-closure crate detection).
//! - **`is_non_substantive`** reuses `ci_scope::classify_diff` directly for
//!   ordinary paths (that function *is* reachable as a library — `pub fn
//!   classify_diff(files: &[String]) -> String`). Cargo manifests are kept
//!   substantive here because dependency, feature, and package metadata
//!   changes must not be treated as documentation-only re-review work.
//!
//! # Diff mode: `DirectTwoDot`, not the default `MergeBaseThreeDot`
//!
//! `seam_diff`'s `base` is a **fixed review-epoch marker commit**, not a
//! moving branch tip. After a rebase or force-push (the normal re-review
//! trigger), that marker commit is frequently no longer an ancestor of
//! `head`. The default `MergeBaseThreeDot` mode (`base...head`) would then
//! silently diff `merge-base(base, head)..head` instead of the marker's
//! tree against head's tree — under-reporting which reviewed seams
//! changed, or, when `head` was reset back to (an ancestor of) the marker,
//! producing a **false-empty** report even though the two trees genuinely
//! differ. `seam_diff` therefore always requests
//! [`change_set::DiffMode::DirectTwoDot`] (`base..head`, a direct tree
//! comparison with no merge-base indirection) — see
//! `test_seam_diff_reset_to_epoch_ancestor_is_not_falsely_empty` below for
//! the regression proof (RED under `MergeBaseThreeDot`, GREEN under
//! `DirectTwoDot`).

use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};

use crate::tasks::change_set::{self, ArtifactIdentity, DiffMode};
use crate::tasks::ci_scope;

/// Report of which seams (changed files, plus coarse crate membership) a
/// push changed between a recorded review-epoch base SHA and head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamDiffReport {
    /// Repo-relative changed paths, as returned by
    /// [`change_set::resolve_change_set_with_mode`].
    pub changed_files: Vec<String>,
    /// Concrete base commit SHA actually resolved and diffed.
    pub base_sha: String,
    /// Concrete head commit SHA actually resolved and diffed.
    pub head_sha: String,
    /// Coarse crate names touched, derived from `crates/<name>/...` path
    /// prefixes only (see module doc comment).
    pub changed_crates: Vec<String>,
    /// `true` when `changed_files` is empty (base and head produced no
    /// diff).
    pub is_empty: bool,
    /// `true` when the delta is empty, or the reused
    /// `ci_scope::classify_diff` classifies it as `prose_only` or
    /// `docs_as_code` without any Cargo manifest.
    pub is_non_substantive: bool,
}

/// Derive coarse crate membership from `crates/<name>/...` path prefixes.
///
/// This is a lightweight, files-only derivation — **not** `ci_scope`'s
/// cargo-metadata-based direct/reverse-dependency crate closure
/// (`ci_scope::DirectCrate` and its reverse-dep-closure computation, which
/// reads the workspace manifest graph). `change_set::ChangeSet` itself
/// exposes no crate/lane classification at all (verified by reading
/// `xtask/src/tasks/change_set.rs`), so per the #3986 slice-1 spec this
/// derives crate names from changed file paths only.
///
/// TODO (follow-up, out of scope for this slice): compose `ci_scope`'s
/// dependency-graph-aware crate/lane classifier here if seam-diff ever
/// needs reverse-dep-closure accuracy (e.g. "does this change transitively
/// affect crate X, even though crate X's own files didn't change").
fn derive_changed_crates(changed_files: &[String]) -> Vec<String> {
    let mut crates: Vec<String> = changed_files
        .iter()
        .filter_map(|f| f.strip_prefix("crates/"))
        .filter_map(|rest| rest.split('/').next())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    crates.sort();
    crates.dedup();
    crates
}

/// Resolve the seams a push changed between `base_sha` (a recorded
/// review-epoch marker SHA — see `.claude/reference/review-convergence.md`
/// § Review-epoch markers) and `head_sha`, by composing
/// [`change_set::resolve_change_set_with_mode`] in
/// [`DiffMode::DirectTwoDot`] mode — this function does not reimplement
/// git diff or base-ref resolution. See the module doc comment's "Diff
/// mode" section for why `DirectTwoDot` (not the resolver's default
/// `MergeBaseThreeDot`) is required here: `base_sha` is a fixed marker
/// commit that may not be an ancestor of `head_sha` after a rebase, and
/// three-dot's merge-base indirection can under-report or even falsely
/// report an empty diff in that case.
///
/// An invalid/nonexistent `base_sha` (or `head_sha`) propagates
/// `resolve_change_set_with_mode`'s loud [`Err`] (see
/// `change_set::resolve_base_ref`'s doc comment: an explicit base must
/// resolve on its own, never silently substituted) rather than returning a
/// silently-empty report — a bad base must never read as "no seams
/// changed".
pub fn seam_diff(base_sha: &str, head_sha: &str, root: &Path) -> Result<SeamDiffReport> {
    let identity =
        ArtifactIdentity::CommitRange { base: base_sha.to_string(), head: head_sha.to_string() };
    let resolved = change_set::resolve_change_set_with_mode(identity, root, DiffMode::DirectTwoDot)
        .context("seam-diff: failed to resolve change set")?;

    let base_sha = resolved
        .base_sha
        .ok_or_else(|| eyre!("seam-diff: resolver returned no base SHA for a commit range"))?;
    let head_sha = resolved
        .head_sha
        .ok_or_else(|| eyre!("seam-diff: resolver returned no head SHA for a commit range"))?;
    let changed_files = resolved.changed_paths;
    let changed_crates = derive_changed_crates(&changed_files);
    let is_empty = changed_files.is_empty();
    let has_cargo_manifest = changed_files
        .iter()
        .any(|file| file.ends_with("Cargo.toml") || file.ends_with("Cargo.lock"));
    let is_non_substantive = is_empty
        || (!has_cargo_manifest && {
            let diff_class = ci_scope::classify_diff(&changed_files);
            diff_class == "prose_only" || diff_class == "docs_as_code"
        });

    Ok(SeamDiffReport {
        base_sha,
        head_sha,
        changed_files,
        changed_crates,
        is_empty,
        is_non_substantive,
    })
}

// ---------------------------------------------------------------------------
// CLI: `cargo xtask seam-diff` — advisory, read-only reporter (#3986 slice 1).
// ---------------------------------------------------------------------------

/// Configuration for the `seam-diff` subcommand.
pub struct SeamDiffConfig {
    /// Review-epoch marker base SHA to diff from. See
    /// `.claude/reference/review-convergence.md` § Review-epoch markers.
    pub base: String,
    /// Head git ref/SHA to diff to.
    pub head: String,
    /// Output format: `"human"` (default, readable summary) or `"json"`
    /// (machine-readable `SeamDiffReport` contract).
    pub format: String,
    /// Repository root to resolve against. Defaults to the perl-lsp
    /// workspace root (`crate::utils::project_root`) when `None`.
    pub root: Option<std::path::PathBuf>,
}

/// Entry point called from `xtask` main for `cargo xtask seam-diff`.
///
/// Resolves a [`SeamDiffReport`] via [`seam_diff`] and prints it in the
/// requested format. An unrecognized `--format` value is a loud [`Err`],
/// never a silent fallback — mirroring `change_set::run`'s contract, and
/// per PR #4201 review feedback (reject unsupported `--format` values
/// rather than silently defaulting, so a typo doesn't misparse as a
/// different report shape).
pub fn run(config: SeamDiffConfig) -> Result<()> {
    let root = match config.root {
        Some(root) => root,
        None => crate::utils::project_root()?,
    };
    let report = seam_diff(&config.base, &config.head, &root)?;
    println!("{}", render_report(&report, &config.format)?);
    Ok(())
}

fn render_report(report: &SeamDiffReport, format: &str) -> Result<String> {
    let output = match format {
        "human" => {
            let mut output = format!("Seam diff: {} -> {}\n", report.base_sha, report.head_sha);
            output.push_str(&format!("Changed files ({}):\n", report.changed_files.len()));
            for file in &report.changed_files {
                output.push_str(&format!("  {file}\n"));
            }
            output.push_str(&format!(
                "Changed crates ({}): {}\n",
                report.changed_crates.len(),
                report.changed_crates.join(", ")
            ));
            output.push_str(&format!("is_empty: {}\n", report.is_empty));
            output.push_str(&format!("is_non_substantive: {}", report.is_non_substantive));
            output
        }
        "json" => {
            let json = serde_json::json!({
                "base": report.base_sha,
                "head": report.head_sha,
                "changed_files": report.changed_files,
                "changed_crates": report.changed_crates,
                "is_empty": report.is_empty,
                "is_non_substantive": report.is_non_substantive,
            });
            serde_json::to_string_pretty(&json)
                .context("Failed to serialize seam diff report to JSON")?
        }
        other => {
            return Err(eyre!(
                "Unknown --format '{other}'; expected 'json' or 'human'. Refusing to silently \
                 fall back to a default format for an unrecognized value (see PR #4201 review: \
                 reject unsupported --format values rather than treating them as text/default)."
            ));
        }
    };

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;
    use std::fs;
    use std::process::Command;

    fn run_git(dir: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .context("failed to spawn git")?;
        if !status.success() {
            return Err(eyre!("git {args:?} failed in {}", dir.display()));
        }
        Ok(())
    }

    fn init_repo(dir: &Path, branch: &str) -> Result<()> {
        fs::create_dir_all(dir).context("failed to create repo dir")?;
        if run_git(dir, &["init", "-q", "-b", branch]).is_err() {
            // Older git without `-b` on `init`.
            run_git(dir, &["init", "-q"])?;
            run_git(dir, &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")])?;
        }
        run_git(dir, &["config", "user.email", "test@test.local"])?;
        run_git(dir, &["config", "user.name", "Test"])?;
        run_git(dir, &["config", "commit.gpgsign", "false"])?;
        Ok(())
    }

    fn commit_file(dir: &Path, name: &str, contents: &str, message: &str) -> Result<()> {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create fixture file parent dir")?;
        }
        fs::write(&path, contents).context("failed to write fixture file")?;
        run_git(dir, &["add", name])?;
        run_git(dir, &["commit", "-q", "-m", message])?;
        Ok(())
    }

    fn head_sha(dir: &Path) -> Result<String> {
        let output = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .context("failed to run git rev-parse HEAD")?;
        ensure!(output.status.success(), "git rev-parse HEAD failed in {}", dir.display());
        Ok(String::from_utf8(output.stdout)
            .context("git rev-parse output was not valid UTF-8")?
            .trim()
            .to_string())
    }

    #[test]
    fn test_seam_diff_empty_when_base_equals_head() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let epoch = head_sha(&repo)?;

        let report = seam_diff(&epoch, "HEAD", &repo)?;
        ensure!(report.base_sha == epoch, "report must retain the resolved base SHA");
        ensure!(report.head_sha == epoch, "report must retain the resolved head SHA");
        ensure!(
            report.changed_files.is_empty(),
            "expected no changed files, got {:?}",
            report.changed_files
        );
        ensure!(
            report.changed_crates.is_empty(),
            "expected no changed crates, got {:?}",
            report.changed_crates
        );
        ensure!(report.is_empty, "expected is_empty == true");
        ensure!(report.is_non_substantive, "an empty diff must be non_substantive");
        Ok(())
    }

    #[test]
    fn test_seam_diff_code_change_across_crate_is_substantive() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let epoch = head_sha(&repo)?;

        commit_file(
            &repo,
            "crates/perl-parser/src/feature.rs",
            "pub fn feature() {}\n",
            "feat: add feature",
        )?;
        commit_file(
            &repo,
            "crates/perl-lsp/src/serve.rs",
            "pub fn serve() {}\n",
            "feat: add serve",
        )?;

        let report = seam_diff(&epoch, "HEAD", &repo)?;
        ensure!(!report.is_empty, "expected non-empty diff");
        let expected_crates = vec!["perl-lsp".to_string(), "perl-parser".to_string()];
        ensure!(
            report.changed_crates == expected_crates,
            "expected {expected_crates:?}, got {:?}",
            report.changed_crates
        );
        ensure!(!report.is_non_substantive, "a code change across crates must be substantive");
        Ok(())
    }

    #[test]
    fn test_seam_diff_docs_only_change_is_non_substantive() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let epoch = head_sha(&repo)?;

        commit_file(&repo, "docs/reference/GUIDE.md", "guidance\n", "docs: add guide")?;

        let report = seam_diff(&epoch, "HEAD", &repo)?;
        ensure!(!report.is_empty, "expected a non-empty diff (a docs file changed)");
        ensure!(report.changed_crates.is_empty(), "a docs-only change should touch no crates");
        // This asserts the reused classification, not a fallback: docs-only
        // changes classify as `prose_only` via `ci_scope::classify_diff`,
        // which this module composes directly (see module doc comment).
        ensure!(
            report.is_non_substantive,
            "a docs-only change must classify as non-substantive via the reused ci_scope classifier"
        );
        Ok(())
    }

    #[test]
    fn test_seam_diff_cargo_manifest_change_is_substantive() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let epoch = head_sha(&repo)?;

        commit_file(
            &repo,
            "Cargo.toml",
            "[package]\nname = \"fixture\"\n",
            "build: add package metadata",
        )?;

        let report = seam_diff(&epoch, "HEAD", &repo)?;
        ensure!(!report.is_empty, "expected a non-empty manifest diff");
        ensure!(
            !report.is_non_substantive,
            "Cargo manifest changes must remain substantive even when ci_scope classifies TOML as docs-as-code"
        );
        Ok(())
    }

    /// DRIFT/FAILURE path (mutation-sensitive): an invalid/nonexistent base
    /// SHA must produce a loud `Err`, never a silently-empty report. A
    /// report that silently returned `is_empty: true` on a bad base would
    /// be a false "no seams changed" — the exact false-clean class this
    /// test guards against.
    #[test]
    fn test_seam_diff_invalid_base_sha_errs_loudly() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;

        let bogus_base = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let result = seam_diff(bogus_base, "HEAD", &repo);
        let err = result
            .err()
            .ok_or_else(|| eyre!("expected seam_diff to fail loudly on an invalid base SHA"))?;
        // `{err}` only renders the top-level context wrapper
        // ("seam-diff: failed to resolve change set"); the descriptive
        // "does not exist" message lives further down the error chain
        // (`resolve_base_ref`'s Err), so walk the full chain rather than
        // asserting on the top-level Display alone.
        let message =
            err.chain().map(std::string::ToString::to_string).collect::<Vec<_>>().join(": ");
        ensure!(
            message.contains(bogus_base) || message.to_lowercase().contains("does not exist"),
            "expected a descriptive error naming the bad base, got: {message}"
        );
        Ok(())
    }

    /// REBASE-AFTER-EPOCH DRIFT PATH (mandatory mutation-proof test; the
    /// exact bug the codex thread on this module flagged). A recorded
    /// review-epoch marker commit `E` is a fixed SHA — after a
    /// rebase/force-push (the normal re-review trigger), `E` is frequently
    /// no longer an ancestor of `head`. This test builds the sharpest such
    /// scenario: `head` is reset all the way back to `E`'s own parent (a
    /// "reset to base" force-push), so `E` and `head` share a common
    /// ancestor but `E`'s tree and `head`'s tree still genuinely differ
    /// (`epoch_only.rs` exists in `E`'s tree, not in `head`'s).
    ///
    /// Under the resolver's default `MergeBaseThreeDot` mode, `E...head`
    /// diffs `merge-base(E, head)..head` — and since `head == merge-base`
    /// here, that is a **self-diff**: empty, even though `E`'s tree and
    /// `head`'s tree differ. That is the false "no seams changed" this test
    /// exists to catch — silently reporting `is_empty: true` when a
    /// reviewed file was actually removed relative to the epoch is exactly
    /// the false-clean class this whole reporter exists to avoid.
    ///
    /// This test is RED against `MergeBaseThreeDot` and GREEN only once
    /// `seam_diff` uses `DiffMode::DirectTwoDot` (confirmed manually: with
    /// `seam_diff`'s call temporarily reverted to
    /// `change_set::resolve_change_set` — i.e. the default three-dot mode —
    /// this test fails with `is_empty == true` and empty `changed_files`;
    /// restoring `DiffMode::DirectTwoDot` makes it pass).
    #[test]
    fn test_seam_diff_reset_to_epoch_ancestor_is_not_falsely_empty() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let base_commit = head_sha(&repo)?;

        // The review-epoch commit E: one file added on top of base_commit.
        // `.rs` (not `.txt`/`.md`) so `ci_scope::classify_diff` sees this as
        // a `code` change, not `prose_only` — this test's `is_empty` /
        // `changed_files` assertions are what pin the diff-mode bug; the
        // `is_non_substantive` assertion should reflect an ordinary code
        // file, not accidentally exercise the prose classifier.
        commit_file(
            &repo,
            "epoch_only.rs",
            "pub fn reviewed_at_epoch() {}\n",
            "add epoch-only file",
        )?;
        let epoch = head_sha(&repo)?;

        // Simulate a force-push that resets the branch tip back to
        // base_commit — E is no longer an ancestor of the new HEAD at all;
        // in fact HEAD now points at E's own parent. This is the sharpest
        // "epoch not an ancestor of head" case: merge-base(E, HEAD) ==
        // HEAD itself, so three-dot's `E...HEAD` degenerates to a self-diff.
        run_git(&repo, &["reset", "--hard", &base_commit])?;
        let head_after_reset = head_sha(&repo)?;
        ensure!(
            head_after_reset == base_commit,
            "expected HEAD to be reset to base_commit, got {head_after_reset}"
        );

        let report = seam_diff(&epoch, "HEAD", &repo)?;
        ensure!(
            !report.is_empty,
            "false-empty bug: epoch_only.rs genuinely differs between epoch's tree and \
             head's tree after a reset-to-base force-push, but is_empty was true"
        );
        ensure!(
            report.changed_files.contains(&"epoch_only.rs".to_string()),
            "expected epoch_only.rs in changed_files (removed relative to epoch), got {:?}",
            report.changed_files
        );
        ensure!(
            !report.is_non_substantive,
            "a removed non-doc, non-prose file must not classify as non-substantive"
        );
        Ok(())
    }

    #[test]
    fn test_run_rejects_unknown_format() -> Result<()> {
        let tmp = tempfile::tempdir().context("failed to create tempdir")?;
        let repo = tmp.path().join("repo");
        init_repo(&repo, "main")?;
        commit_file(&repo, "README.md", "init\n", "init")?;
        let epoch = head_sha(&repo)?;

        let config = SeamDiffConfig {
            base: epoch,
            head: "HEAD".to_string(),
            format: "makrdown".to_string(),
            root: Some(repo),
        };
        let err = run(config)
            .err()
            .ok_or_else(|| eyre!("expected run() to reject an unknown --format value"))?;
        let message = format!("{err}");
        ensure!(
            message.contains("Unknown --format"),
            "expected an explicit unknown-format error, got: {message}"
        );
        Ok(())
    }

    #[test]
    fn test_render_success_formats_include_resolved_identity_and_shape() -> Result<()> {
        let report = SeamDiffReport {
            base_sha: "base-resolved".to_string(),
            head_sha: "head-resolved".to_string(),
            changed_files: vec!["crates/perl-lsp/src/lib.rs".to_string()],
            changed_crates: vec!["perl-lsp".to_string()],
            is_empty: false,
            is_non_substantive: false,
        };

        let human = render_report(&report, "human")?;
        ensure!(human.contains("Seam diff: base-resolved -> head-resolved"));
        ensure!(human.contains("  crates/perl-lsp/src/lib.rs"));
        ensure!(human.contains("Changed crates (1): perl-lsp"));
        ensure!(human.contains("is_non_substantive: false"));

        let json = render_report(&report, "json")?;
        let value: serde_json::Value =
            serde_json::from_str(&json).context("rendered JSON should parse")?;
        ensure!(value["base"] == "base-resolved");
        ensure!(value["head"] == "head-resolved");
        ensure!(value["changed_files"][0] == "crates/perl-lsp/src/lib.rs");
        ensure!(value["changed_crates"][0] == "perl-lsp");
        ensure!(value["is_empty"] == false);
        ensure!(value["is_non_substantive"] == false);
        Ok(())
    }
}
