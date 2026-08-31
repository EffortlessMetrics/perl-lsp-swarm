//! Guard against silently dropping assertion context when `.expect("…")` call
//! sites migrate to the `perl-test-must` helpers (#14291).
//!
//! `perl-test-must` exposes two families of assertion-boundary helpers:
//!
//! - `must` / `must_some` / `must_err` — no explanation is carried;
//! - `must_with` / `must_some_with` / `must_err_with` — the
//!   "context-preserving counterpart", which retains an `expect`-style
//!   explanation in the panic diagnostic.
//!
//! A cleanup sweep that rewrites `foo().expect("the fixture declares Example")`
//! into `must(foo())` compiles, keeps the test passing, and silently deletes the
//! sentence that says *which input* failed. The failure is only visible later,
//! during triage, when the panic no longer names the scenario.
//!
//! This is only detectable **relative to a change**: the tree after the sweep
//! carries a bare `must*` call and no trace of the sentence that used to be
//! there. Roughly four thousand legitimate bare `must*` call sites already exist
//! on `main`, and `perl-test-must` deliberately keeps the bare signatures as
//! part of its public contract, so a whole-tree ban is neither correct nor
//! possible. The guard therefore reads a unified diff and reports the exact
//! migration shape: a hunk that **removes** an `.expect("…")` explanation and
//! **adds** a bare `must*` call while the explanation itself does not survive
//! anywhere on the hunk's added side.
//!
//! Tracking the explanation string rather than counting `_with` calls keeps the
//! guard honest about the rewrites that do preserve it. `must_with(load(), "…")`
//! is the intended form, but a sweep that lifts the sentence into a preceding
//! `assert!(…, "…")` also keeps it reachable at failure time, and both read as
//! clean.
//!
//! [`scan_unified_diff`] is a pure function over diff text; [`check`] is the
//! thin shell that obtains the diff from `git` and reports the findings.

use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use crate::{GREEN, NC, RED, YELLOW};

/// Matches the explanation string of an `.expect("…")` call and captures it.
///
/// Only string-literal explanations count: they are the assertion context the
/// `_with` variants exist to carry. A non-literal argument
/// (`.expect(&format!(…))`) is deliberately out of subject.
static EXPECT_CONTEXT_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"\.expect\(\s*"((?:[^"\\]|\\.)*)""#));

/// Matches a bare `must` / `must_some` / `must_err` call.
///
/// The leading `(?:^|[^A-Za-z0-9_])` boundary keeps `helper_must(` from
/// matching, and requiring `\s*\(` immediately after the name keeps the `_with`
/// variants out: in `must_with(` the character after `must` is `_`, so neither
/// the `must` alternative nor the longer alternatives can complete.
static BARE_MUST_CALL_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?:^|[^A-Za-z0-9_])(must|must_some|must_err)\s*\("));

/// One hunk in which assertion context was dropped by a `must*` migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextDrop {
    /// Repository-relative path of the file the hunk belongs to.
    pub(crate) file: String,
    /// First line of the hunk on the post-image side, for `file:line` reporting.
    pub(crate) new_start_line: usize,
    /// The `.expect("…")` explanations the hunk removed without carrying them
    /// over to its added side, in source order.
    pub(crate) dropped_contexts: Vec<String>,
    /// The bare `must*` call lines the hunk added, in source order.
    pub(crate) bare_calls: Vec<String>,
}

/// Accumulates one hunk's evidence while the diff is being read.
#[derive(Default)]
struct HunkAccumulator {
    file: String,
    new_start_line: usize,
    removed_contexts: Vec<String>,
    bare_calls: Vec<String>,
    /// Every added line of the hunk, joined, so an explanation can be looked
    /// for wherever it landed — a `_with` argument, a preceding `assert!`, or a
    /// reflowed multi-line call.
    added_text: String,
}

impl HunkAccumulator {
    /// Returns the finding for this hunk, or `None` when the hunk is clean.
    ///
    /// A hunk is a violation when it adds at least one bare `must*` call and at
    /// least one removed `.expect("…")` explanation appears nowhere on the
    /// added side — that explanation no longer reaches the panic message.
    fn into_finding(self) -> Option<ContextDrop> {
        if self.bare_calls.is_empty() {
            return None;
        }
        // The explanation must survive as a string literal, quotes included.
        // Matching the bare substring would let an identifier that happens to
        // contain it (`load_a` "carrying" the context `a`) hide a real drop.
        let dropped_contexts: Vec<String> = self
            .removed_contexts
            .into_iter()
            .filter(|context| !self.added_text.contains(&format!("\"{context}\"")))
            .collect();
        if dropped_contexts.is_empty() {
            return None;
        }
        Some(ContextDrop {
            file: self.file,
            new_start_line: self.new_start_line,
            dropped_contexts,
            bare_calls: self.bare_calls,
        })
    }
}

/// Reports every hunk in `diff` that drops `.expect("…")` context into a bare
/// `must*` call.
///
/// `diff` is unified-diff text as produced by `git diff`. Findings are returned
/// in diff order. Only `.rs` files are in subject; hunks are evaluated
/// independently, so a removal and an addition that land in different hunks are
/// never paired.
///
/// # Errors
///
/// Returns an error when one of this module's static regexes failed to compile.
pub(crate) fn scan_unified_diff(diff: &str) -> Result<Vec<ContextDrop>> {
    let expect_re = compiled_regex(&EXPECT_CONTEXT_RE, "expect-context")?;
    let bare_re = compiled_regex(&BARE_MUST_CALL_RE, "bare-must-call")?;

    let mut findings = Vec::new();
    let mut current_file: Option<String> = None;
    let mut hunk: Option<HunkAccumulator> = None;

    for line in diff.lines() {
        // The `diff --git` line is the file boundary. Taking the path from it
        // rather than from `+++` keeps an added source line that happens to
        // read `++ …` from being mistaken for a post-image header.
        if let Some(path) = post_image_path(line) {
            close_hunk(&mut hunk, &mut findings);
            current_file = path;
            continue;
        }

        // Between the file boundary and the first `@@`, the `---`/`+++` header
        // pair carries no content. `+++ /dev/null` marks a deletion, whose
        // post-image has nothing to flag.
        if hunk.is_none() && (line.starts_with("--- ") || line.starts_with("+++ ")) {
            if line == "+++ /dev/null" {
                current_file = None;
            }
            continue;
        }

        if line.starts_with("@@") {
            close_hunk(&mut hunk, &mut findings);
            hunk = current_file.as_ref().and_then(|file| {
                is_rust_diff_path(file).then(|| HunkAccumulator {
                    file: file.clone(),
                    new_start_line: parse_hunk_new_start(line).unwrap_or(0),
                    ..HunkAccumulator::default()
                })
            });
            continue;
        }

        let Some(active) = hunk.as_mut() else {
            continue;
        };

        if let Some(removed) = line.strip_prefix('-') {
            for capture in expect_re.captures_iter(removed) {
                if let Some(context) = capture.get(1) {
                    active.removed_contexts.push(context.as_str().to_owned());
                }
            }
        } else if let Some(added) = line.strip_prefix('+') {
            active.added_text.push_str(added);
            active.added_text.push('\n');
            if bare_re.is_match(added) {
                active.bare_calls.push(added.trim().to_owned());
            }
        }
    }

    close_hunk(&mut hunk, &mut findings);
    Ok(findings)
}

/// Finalizes `hunk`, pushing its finding onto `findings` when it has one.
fn close_hunk(hunk: &mut Option<HunkAccumulator>, findings: &mut Vec<ContextDrop>) {
    if let Some(finding) = hunk.take().and_then(HunkAccumulator::into_finding) {
        findings.push(finding);
    }
}

/// Extracts the post-image path from a `diff --git a/OLD b/NEW` boundary line.
///
/// Returns `None` when `line` is not a file boundary, and `Some(None)` when the
/// boundary is malformed — an unparseable boundary drops the current file
/// rather than attributing its hunks to the previous one. A rename reports the
/// post-image (`b/`) path, which is the side a bare `must*` call is added on.
fn post_image_path(line: &str) -> Option<Option<String>> {
    let rest = line.strip_prefix("diff --git ")?;
    // Paths may repeat the separator (`a/b/lib.rs b/b/lib.rs`), so anchor on
    // the last ` b/` occurrence rather than the first.
    let Some(index) = rest.rfind(" b/") else {
        return Some(None);
    };
    let path = &rest[index + " b/".len()..];
    if path.is_empty() { Some(None) } else { Some(Some(path.to_owned())) }
}

/// Returns `true` when a diff path names a Rust source file.
fn is_rust_diff_path(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|ext| ext == "rs")
}

/// Parses the post-image start line out of an `@@ -a,b +c,d @@` header.
fn parse_hunk_new_start(header: &str) -> Option<usize> {
    let after_plus = header.split('+').nth(1)?;
    let digits = after_plus.split([',', ' ']).next()?;
    digits.parse().ok()
}

/// Resolves a `LazyLock`-compiled regex without panicking on failure.
fn compiled_regex<'a>(
    lock: &'a LazyLock<Result<Regex, regex::Error>>,
    name: &str,
) -> Result<&'a Regex> {
    lock.as_ref().map_err(|error| eyre!("failed to compile the {name} regex: {error}"))
}

/// Runs the guard against the change between `base` and `HEAD`.
///
/// Returns the process exit code: `0` when no context was dropped, `1`
/// otherwise.
///
/// # Errors
///
/// Returns an error when no usable base ref can be resolved or when `git diff`
/// cannot be executed.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let base = resolve_base(repo_root, base)?;
    let diff = read_diff(repo_root, &base)?;
    let findings = scan_unified_diff(&diff)?;

    if findings.is_empty() {
        println!("{GREEN}✅ No assertion context dropped by a must* migration{NC} (base: {base})");
        return Ok(0);
    }

    println!("{RED}❌ must* migration dropped assertion context{NC} (base: {base})");
    for finding in &findings {
        println!("  {}:{}", finding.file, finding.new_start_line);
        for context in &finding.dropped_contexts {
            println!("    {YELLOW}dropped context{NC}: \"{context}\"");
        }
        for call in &finding.bare_calls {
            println!("    added bare call: {call}");
        }
    }
    println!();
    println!(
        "Use `must_with` / `must_some_with` / `must_err_with` to preserve the assertion context from `.expect(\"...\")`."
    );
    println!(
        "The bare `must` / `must_some` / `must_err` helpers are only correct when the source carried no explanation to begin with."
    );
    Ok(1)
}

/// Candidate base refs tried, in order, when no explicit base is supplied.
fn base_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("CI_SCOPE_BASE") {
        candidates.push(value);
    }
    if let Ok(value) = std::env::var("GITHUB_BASE_REF") {
        candidates.push(format!("origin/{value}"));
        candidates.push(value);
    }
    candidates.extend(["origin/main".to_owned(), "main".to_owned(), "HEAD~1".to_owned()]);
    candidates
}

/// Selects the first candidate base ref that `git` can resolve.
fn resolve_base(repo_root: &Path, requested: Option<&str>) -> Result<String> {
    if let Some(base) = requested {
        if ref_exists(repo_root, base) {
            return Ok(base.to_owned());
        }
        return Err(eyre!("base ref '{base}' does not resolve in {}", repo_root.display()));
    }

    let candidates = base_candidates();
    candidates
        .iter()
        .find(|candidate| ref_exists(repo_root, candidate))
        .cloned()
        .ok_or_else(|| eyre!("no base ref resolved; tried: {}", candidates.join(", ")))
}

/// Returns `true` when `git rev-parse --verify` resolves `reference`.
fn ref_exists(repo_root: &Path, reference: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Reads the `base...HEAD` Rust-source diff with zero context lines.
///
/// Zero context keeps hunks minimal, so a removal and an addition are paired
/// only when they are genuinely adjacent.
fn read_diff(repo_root: &Path, base: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--unified=0", "--no-color", &format!("{base}...HEAD"), "--", "*.rs"])
        .output()
        .map_err(|error| eyre!("failed to run `git diff` against '{base}': {error}"))?;

    if !output.status.success() {
        return Err(eyre!(
            "`git diff {base}...HEAD` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{ContextDrop, is_rust_diff_path, parse_hunk_new_start, scan_unified_diff};
    use color_eyre::eyre::Result;

    /// Builds a one-file, one-hunk diff around the supplied body lines.
    fn diff(path: &str, body: &[&str]) -> String {
        let mut text = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");
        text.push_str("@@ -10,1 +10,1 @@\n");
        for line in body {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    #[test]
    fn bare_must_replacing_an_expect_drops_context() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let value = load().expect("the fixture declares Example");"#,
                "+        let value = must(load());",
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(
            findings,
            vec![ContextDrop {
                file: "crates/example/src/lib.rs".to_owned(),
                new_start_line: 10,
                dropped_contexts: vec!["the fixture declares Example".to_owned()],
                bare_calls: vec!["let value = must(load());".to_owned()],
            }]
        );
        Ok(())
    }

    #[test]
    fn bare_must_some_and_must_err_are_in_subject() -> Result<()> {
        for call in ["must_some(load())", "must_err(load())"] {
            let text = diff(
                "crates/example/src/lib.rs",
                &[
                    r#"-        let value = load().expect("context");"#,
                    &format!("+        {call};"),
                ],
            );
            assert_eq!(scan_unified_diff(&text)?.len(), 1, "expected {call} to be flagged");
        }
        Ok(())
    }

    #[test]
    fn context_preserving_with_variant_is_clean() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let value = load().expect("the fixture declares Example");"#,
                r#"+        let value = must_with(load(), "the fixture declares Example");"#,
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn every_with_variant_carries_the_removed_explanation() -> Result<()> {
        for name in ["must_with", "must_some_with", "must_err_with"] {
            let text = diff(
                "crates/example/src/lib.rs",
                &[
                    r#"-        let value = load().expect("the fixture declares Example");"#,
                    &format!(
                        r#"+        let value = {name}(load(), "the fixture declares Example");"#
                    ),
                ],
            );
            assert_eq!(scan_unified_diff(&text)?, vec![], "expected {name} to be clean");
        }
        Ok(())
    }

    #[test]
    fn an_explanation_reflowed_onto_its_own_line_still_counts_as_carried() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let value = load().expect("the fixture declares Example");"#,
                "+        let value = must_with(",
                "+            load(),",
                r#"+            "the fixture declares Example","#,
                "+        );",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn an_explanation_lifted_into_an_assert_is_still_reachable() -> Result<()> {
        // Verbatim from PR #12000 (`crates/perl-lsp-rs-core/src/
        // configuration_authority/checked.rs`): the sweep moved the sentence
        // into a preceding `assert!`, so it still names the failing input.
        let text = diff(
            "crates/perl-lsp-rs-core/src/configuration_authority/checked.rs",
            &[
                r#"-        let field = authority_by_id("formatting.engine").expect("missing formatter authority");"#,
                r#"+        assert!(authority_by_id("formatting.engine").is_some(), "missing formatter authority");"#,
                r#"+        let field = must_some(authority_by_id("formatting.engine"));"#,
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn the_pr_12000_profile_authority_migration_is_reported() -> Result<()> {
        // Verbatim from PR #12000 (`crates/perl-lsp-rs-core/src/config/mod.rs`),
        // the migration shape #14291 was filed against: the explanation is
        // deleted outright and a bare `must_some` takes its place.
        let text = diff(
            "crates/perl-lsp-rs-core/src/config/mod.rs",
            &[
                "-            let expected = NativeCriticProfile::parse(raw)",
                r#"-                .expect("boundary fixture must be accepted by the profile authority");"#,
                "+            let expected = must_some(NativeCriticProfile::parse(raw));",
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].dropped_contexts,
            vec!["boundary fixture must be accepted by the profile authority".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn migrating_unwrap_has_no_context_to_drop() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &["-        let value = load().unwrap();", "+        let value = must(load());"],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn removing_an_expect_without_adding_a_bare_call_is_clean() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[r#"-        let value = load().expect("context");"#, "+        let value = load()?;"],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn adding_a_bare_call_without_removing_an_expect_is_clean() -> Result<()> {
        let text = diff("crates/example/src/lib.rs", &["+        let value = must(load());"]);

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn only_the_uncarried_explanation_is_reported_beside_a_with_call() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let a = load_a().expect("the first authority resolves");"#,
                r#"-        let b = load_b().expect("the second authority resolves");"#,
                r#"+        let a = must_with(load_a(), "the first authority resolves");"#,
                "+        let b = must(load_b());",
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].dropped_contexts, vec!["the second authority resolves".to_owned()]);
        Ok(())
    }

    #[test]
    fn identifiers_ending_in_must_are_not_bare_calls() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let value = load().expect("context");"#,
                "+        let value = helper_must(load());",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn separate_hunks_are_never_paired() -> Result<()> {
        let path = "crates/example/src/lib.rs";
        let text = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n\
             @@ -10,1 +10,0 @@\n-        let value = load().expect(\"context\");\n\
             @@ -80,0 +79,1 @@\n+        let other = must(load_other());\n"
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn non_rust_files_are_out_of_subject() -> Result<()> {
        let text = diff(
            "docs/example.md",
            &[
                r#"-        let value = load().expect("context");"#,
                "+        let value = must(load());",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn a_deleted_file_has_no_post_image_to_flag() -> Result<()> {
        let path = "crates/example/src/lib.rs";
        let text = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ /dev/null\n\
             @@ -10,2 +0,0 @@\n-        let value = load().expect(\"context\");\n-        let other = must(load());\n"
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn findings_from_several_files_are_reported_in_diff_order() -> Result<()> {
        let first = diff(
            "crates/a/src/lib.rs",
            &[r#"-        load().expect("a");"#, "+        must(load());"],
        );
        let second = diff(
            "crates/b/src/lib.rs",
            &[r#"-        load().expect("b");"#, "+        must_some(load());"],
        );

        let findings = scan_unified_diff(&format!("{first}{second}"))?;

        assert_eq!(
            findings.iter().map(|finding| finding.file.as_str()).collect::<Vec<_>>(),
            vec!["crates/a/src/lib.rs", "crates/b/src/lib.rs"]
        );
        Ok(())
    }

    #[test]
    fn an_escaped_quote_does_not_truncate_the_captured_context() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[r#"-        load().expect("the \"Example\" fixture");"#, "+        must(load());"],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].dropped_contexts, vec![r#"the \"Example\" fixture"#.to_owned()]);
        Ok(())
    }

    #[test]
    fn hunk_headers_report_the_post_image_line() {
        assert_eq!(parse_hunk_new_start("@@ -10,3 +42,5 @@ fn example() {"), Some(42));
        assert_eq!(parse_hunk_new_start("@@ -10 +42 @@"), Some(42));
        assert_eq!(parse_hunk_new_start("@@ malformed @@"), None);
    }

    #[test]
    fn only_rust_paths_are_in_subject() {
        assert!(is_rust_diff_path("crates/example/src/lib.rs"));
        assert!(!is_rust_diff_path("docs/example.md"));
        assert!(!is_rust_diff_path("Makefile"));
    }
}
