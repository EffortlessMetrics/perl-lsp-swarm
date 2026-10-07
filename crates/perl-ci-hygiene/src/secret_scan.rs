//! Token-shape secret scan: the rule authority for the local hooks and the required gate.
//!
//! The git hooks (`git_hooks.rs`, binary) embed these shapes as `grep -E`
//! patterns so a staged or pushed token is refused with zero build; the
//! required `secret_scan` gate runs the same shapes over the PR diff plus the
//! PR body server-side, so `--no-verify` cannot bypass both layers (#17428).
//!
//! Every shape is written in the intersection of POSIX ERE and the `regex`
//! crate (explicit character classes, no `\s`, no lookaround), so one spelling
//! serves both engines. [`TOKEN_SHAPES`] is the authority; the binary-side
//! `secret_scan_shell_matches_lib_rules` test fails if the embedded shell copy
//! drifts.
//!
//! Findings name the file, the post-image line, and the rule — never the
//! matched text. Printing a secret into a terminal or a CI log would spread
//! the very credential the scan exists to contain.

use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

/// Repo-relative path of the scan allowlist.
///
/// The file lists exact repo-relative paths, one per line; `#` comments and
/// blank lines are ignored. A listed path is skipped by the hook scan and the
/// gate alike. It exists for inert test fixtures that must carry
/// token-shaped bytes; anything listed here is a conscious exemption, not an
/// accident, so entries should be rare and fixture-shaped.
pub const ALLOWLIST_PATH: &str = ".ci/secret-scan-allowlist.txt";

/// (rule name, ERE) pairs defining every token shape in subject.
///
/// The spelling must stay valid POSIX ERE: the hooks run these through
/// `grep -E`, which has no `\s`, no non-capturing groups, and no lookaround.
/// Keep the set narrow — each shape is a high-precision credential family,
/// not a heuristic — and add the documented allowlist entry point
/// ([`ALLOWLIST_PATH`]) rather than widening a shape when a fixture needs
/// token-shaped bytes.
///
/// The generic rule's trailing guard (`$` or a non-value, non-paren
/// character) keeps call-shaped values out of subject: without it,
/// `api_key = get_key_from_vault()` reads as an 18-character credential.
/// ERE has no lookaround, so the guard consumes one character instead.
pub const TOKEN_SHAPES: [(&str, &str); 5] = [
    ("github-token", "gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22}_[A-Za-z0-9]{59}"),
    ("aws-access-key", "AKIA[0-9A-Z]{16}"),
    ("slack-token", "xox[baprs]-[A-Za-z0-9-]{10,48}"),
    ("private-key", "-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"),
    (
        "generic-assignment",
        "([Aa][Pp][Ii][_-]?[Kk][Ee][Yy]|[Aa][Pp][Ii][_-]?[Tt][Oo][Kk][Ee][Nn]|[Ss][Ee][Cc][Rr][Ee][Tt][_-]?[Kk][Ee][Yy]|[Aa][Cc][Cc][Ee][Ss][Ss][_-]?[Tt][Oo][Kk][Ee][Nn])[[:space:]]*[:=][[:space:]]*[\"']?[A-Za-z0-9_./+=-]{16,}($|[^A-Za-z0-9_./+()=-])",
    ),
];

/// [`TOKEN_SHAPES`] compiled in order, so index `i` of this vector is the
/// regex for `TOKEN_SHAPES[i]`.
static COMPILED_SHAPES: LazyLock<Result<Vec<Regex>, regex::Error>> =
    LazyLock::new(|| TOKEN_SHAPES.iter().map(|(_, ere)| Regex::new(ere)).collect());

/// One token-shaped line.
///
/// Deliberately content-free: `file`, `line`, and `rule` locate the finding
/// without reproducing the credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    /// Repository-relative path of the file the line was added on, or the
    /// pseudo-path `pull-request body` for PR title/body findings.
    pub file: String,
    /// Post-image line number, or `0` when the hunk header was unparseable.
    /// The line is still reported: an unreadable header must not hide a shape.
    pub line: usize,
    /// Rule name from [`TOKEN_SHAPES`] that matched.
    pub rule: &'static str,
}

/// Exact-path allowlist loaded from [`ALLOWLIST_PATH`].
#[derive(Debug, Default)]
pub struct Allowlist {
    paths: HashSet<String>,
}

impl Allowlist {
    /// Loads the allowlist at `repo_root`/[`ALLOWLIST_PATH`].
    ///
    /// A missing file means "no exemptions" and loads as empty. A file that
    /// exists but cannot be read is an error: silently ignoring it would scan
    /// paths the repository meant to exempt (a fail-open on intent), while
    /// silently exempting nothing is the wrong default in the other
    /// direction — either way an unreadable policy file is not a state to
    /// guess about.
    ///
    /// # Errors
    ///
    /// Returns an error when the allowlist file exists but cannot be read.
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(ALLOWLIST_PATH);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(eyre!(
                    "failed to read the secret-scan allowlist at {}: {error}",
                    path.display()
                ));
            }
        };
        Ok(Self::parse(&text))
    }

    /// Parses allowlist text: trimmed lines, skipping blanks and `#` comments.
    ///
    /// Matching is exact and whole-line; there are no globs and no inline
    /// comments, so a path containing `#` or `*` still matches itself.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let paths = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect();
        Self { paths }
    }

    /// Returns `true` when `path` is an exact allowlisted entry.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    /// Number of allowlisted paths, for scope reporting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Returns `true` when no path is allowlisted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Reports every added line in `diff` matching any [`TOKEN_SHAPES`] rule.
///
/// `diff` is unified-diff text as produced by `git diff`. Findings are
/// returned in diff order. Unlike assertion-scoped guards, every file type is
/// in subject — tokens hide in fixtures, docs, and config as readily as in
/// source — and allowlisted paths are skipped whole.
///
/// # Errors
///
/// Returns an error when one of this module's static shapes failed to compile.
pub fn scan_unified_diff(diff: &str, allowlist: &Allowlist) -> Result<Vec<SecretFinding>> {
    let shapes = compiled_shapes()?;
    let mut findings = Vec::new();
    let mut current_file: Option<String> = None;
    let mut in_hunk = false;
    let mut new_line: usize = 0;

    for line in diff.lines() {
        if let Some(path) = post_image_path(line) {
            current_file = path;
            in_hunk = false;
            continue;
        }

        if !in_hunk {
            if line.starts_with("--- ") || line.starts_with("+++ ") {
                if line == "+++ /dev/null" {
                    current_file = None;
                }
                continue;
            }
            if line.starts_with("@@") {
                if current_file.as_ref().is_some_and(|file| !allowlist.contains(file)) {
                    new_line = parse_hunk_new_start(line).unwrap_or(0);
                    in_hunk = true;
                }
                continue;
            }
            continue;
        }

        if line.starts_with("@@") {
            new_line = parse_hunk_new_start(line).unwrap_or(0);
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            if let Some(file) = current_file.as_ref() {
                scan_line_into(file, new_line, added, shapes, &mut findings);
            }
            if new_line > 0 {
                new_line += 1;
            }
        } else if line.starts_with(' ') || line.is_empty() {
            // Context advances the post-image line without being scanned.
            if new_line > 0 {
                new_line += 1;
            }
        }
        // Removed lines and `\ No newline` markers are neither scanned nor counted.
    }

    Ok(findings)
}

/// Reports every line of `text` matching any [`TOKEN_SHAPES`] rule.
///
/// `file` is a display label (the PR-body pseudo-path in gate use); lines are
/// numbered from 1.
///
/// # Errors
///
/// Returns an error when one of this module's static shapes failed to compile.
pub fn scan_text(file: &str, text: &str) -> Result<Vec<SecretFinding>> {
    let shapes = compiled_shapes()?;
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        scan_line_into(file, index + 1, line, shapes, &mut findings);
    }
    Ok(findings)
}

/// Extracts the scannable PR text (title plus body) from a GitHub event payload.
///
/// Returns `None` when the payload carries no `pull_request` object — push and
/// schedule events have no PR body to scan. A present-but-empty title and body
/// still return `Some`, so the gate log distinguishes "no PR in this event"
/// from "PR scanned".
///
/// # Errors
///
/// Returns an error when `event_json` is not valid JSON. A corrupt event
/// payload means the body cannot be verified, which fails closed.
pub fn extract_pr_body(event_json: &str) -> Result<Option<String>> {
    let value: serde_json::Value = serde_json::from_str(event_json)
        .map_err(|error| eyre!("failed to parse the GitHub event payload as JSON: {error}"))?;
    let Some(pr) = value.get("pull_request") else {
        return Ok(None);
    };
    let title = pr.get("title").and_then(serde_json::Value::as_str).unwrap_or("");
    let body = pr.get("body").and_then(serde_json::Value::as_str).unwrap_or("");
    Ok(Some(format!("{title}\n{body}")))
}

/// Records a finding per rule matching `line`.
fn scan_line_into(
    file: &str,
    line_number: usize,
    line: &str,
    shapes: &[Regex],
    findings: &mut Vec<SecretFinding>,
) {
    for (index, shape) in shapes.iter().enumerate() {
        if shape.is_match(line) {
            findings.push(SecretFinding {
                file: file.to_owned(),
                line: line_number,
                rule: TOKEN_SHAPES[index].0,
            });
        }
    }
}

/// Extracts the post-image path from a `diff --git a/OLD b/NEW` boundary line.
///
/// Returns `None` when `line` is not a file boundary, and `Some(None)` when the
/// boundary is malformed. Mirrors `must_context::post_image_path`: the path is
/// anchored on the last ` b/` occurrence, and a rename reports the post-image
/// (`b/`) side.
fn post_image_path(line: &str) -> Option<Option<String>> {
    let rest = line.strip_prefix("diff --git ")?;
    let Some(index) = rest.rfind(" b/") else {
        return Some(None);
    };
    let path = &rest[index + " b/".len()..];
    if path.is_empty() { Some(None) } else { Some(Some(path.to_owned())) }
}

/// Parses the post-image start line out of an `@@ -a,b +c,d @@` header.
fn parse_hunk_new_start(header: &str) -> Option<usize> {
    let after_plus = header.split('+').nth(1)?;
    let digits = after_plus.split([',', ' ']).next()?;
    digits.parse().ok()
}

/// Resolves the compiled shapes without panicking on failure.
fn compiled_shapes() -> Result<&'static Vec<Regex>> {
    COMPILED_SHAPES
        .as_ref()
        .map_err(|error| eyre!("failed to compile a secret-scan token shape: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        Allowlist, TOKEN_SHAPES, compiled_shapes, extract_pr_body, scan_text, scan_unified_diff,
    };
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

    /// Scans `body` as the added lines of one file and returns the matched rules.
    fn matched_rules(path: &str, body: &[&str]) -> Result<Vec<&'static str>> {
        let findings = scan_unified_diff(&diff(path, body), &Allowlist::default())?;
        Ok(findings.iter().map(|finding| finding.rule).collect())
    }

    // Token builders assemble inert fakes from fragments so this file itself
    // carries no token-shaped string for the scanner to flag.
    fn fake_github_token(prefix: &str) -> String {
        format!("{prefix}{}", "F".repeat(36))
    }

    fn fake_fine_grained_pat() -> String {
        format!("{}_pat_{}_{}", "github", "a".repeat(22), "b".repeat(59))
    }

    fn fake_aws_key() -> String {
        format!("AKIA{}", "C".repeat(16))
    }

    fn fake_slack_token() -> String {
        format!("xoxb-{}", "a1".repeat(5))
    }

    #[test]
    fn github_token_shape_matches_all_prefix_variants() -> Result<()> {
        for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
            let line = format!("+token = \"{}\"", fake_github_token(prefix));
            assert_eq!(
                matched_rules("crates/example/src/lib.rs", &[&line])?,
                vec!["github-token"],
                "expected {prefix}… to match the github-token rule"
            );
        }
        Ok(())
    }

    #[test]
    fn fine_grained_pat_shape_matches() -> Result<()> {
        let line = format!("+token = \"{}\"", fake_fine_grained_pat());
        assert_eq!(matched_rules("crates/example/src/lib.rs", &[&line])?, vec!["github-token"]);
        Ok(())
    }

    #[test]
    fn aws_access_key_shape_matches() -> Result<()> {
        let line = format!("+aws_access_key_id = {}", fake_aws_key());
        assert_eq!(matched_rules("infra/config.toml", &[&line])?, vec!["aws-access-key"]);
        Ok(())
    }

    #[test]
    fn slack_token_shape_matches() -> Result<()> {
        let line = format!("+notify_token = \"{}\"", fake_slack_token());
        assert_eq!(matched_rules("crates/example/src/lib.rs", &[&line])?, vec!["slack-token"]);
        Ok(())
    }

    #[test]
    fn private_key_header_matches_all_key_types() -> Result<()> {
        for header in [
            concat!("-----BEGIN ", "PRIVATE KEY-----"),
            concat!("-----BEGIN ", "RSA PRIVATE KEY-----"),
            concat!("-----BEGIN ", "EC PRIVATE KEY-----"),
            concat!("-----BEGIN ", "OPENSSH PRIVATE KEY-----"),
            concat!("-----BEGIN ", "DSA PRIVATE KEY-----"),
        ] {
            let line = format!("+{header}");
            assert_eq!(
                matched_rules("deploy/key.pem", &[&line])?,
                vec!["private-key"],
                "expected {header} to match the private-key rule"
            );
        }
        Ok(())
    }

    #[test]
    fn generic_assignment_matches_keyword_case_and_separator_variants() -> Result<()> {
        for line in [
            format!("+api_key = \"{}\"", "v".repeat(20)),
            format!("+API_TOKEN='{}'", "w".repeat(20)),
            format!("+secret-key: {}", "x".repeat(16)),
            format!("+Access_Token = {}", "y".repeat(24)),
        ] {
            assert_eq!(
                matched_rules("crates/example/src/lib.rs", &[&line])?,
                vec!["generic-assignment"],
                "expected {line} to match the generic-assignment rule"
            );
        }
        Ok(())
    }

    #[test]
    fn near_miss_shapes_are_clean() -> Result<()> {
        let short_suffix = format!("ghp_{}", "F".repeat(35));
        let broken_suffix = format!("ghp_{}-{}", "F".repeat(20), "F".repeat(15));
        let short_aws = format!("AKIA{}", "C".repeat(15));
        let short_value = format!("api_key = \"{}\"", "v".repeat(15));
        for line in [
            format!("+token = \"{short_suffix}\""),
            format!("+token = \"{broken_suffix}\""),
            format!("+aws_access_key_id = {short_aws}"),
            "+notify = \"xoxz-abcdefghij\"".to_owned(),
            concat!("+-----BEGIN ", "CERTIFICATE-----").to_owned(),
            format!("+{short_value}"),
            "+api_key = \"\"".to_owned(),
            "+api_key = get_key_from_vault()".to_owned(),
        ] {
            assert_eq!(
                matched_rules("crates/example/src/lib.rs", &[&line])?,
                Vec::<&str>::new(),
                "expected {line} to be clean"
            );
        }
        Ok(())
    }

    #[test]
    fn call_shaped_values_are_not_credentials() -> Result<()> {
        // The value run must end at end-of-line or a non-value, non-paren
        // character; a `(` immediately after the run reads as a call.
        for line in [
            "+api_key = get_key_from_vault()",
            "+access_token = fetch_access_token()",
            "+secret_key=load_secret_key_from_env()",
        ] {
            assert_eq!(
                matched_rules("crates/example/src/lib.rs", &[line])?,
                Vec::<&str>::new(),
                "expected {line} to read as a call, not a credential"
            );
        }
        Ok(())
    }

    #[test]
    fn added_lines_are_scanned_removed_and_context_are_not() -> Result<()> {
        let token = fake_github_token("ghp_");
        let removed = format!("-old_token = \"{token}\"");
        let context = format!(" keep_token = \"{token}\"");
        let added = format!("+new_token = \"{token}\"");
        let findings = scan_unified_diff(
            &diff("crates/example/src/lib.rs", &[&removed, &context, &added]),
            &Allowlist::default(),
        )?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "github-token");
        assert_eq!(findings[0].line, 11);
        Ok(())
    }

    #[test]
    fn line_numbers_track_the_post_image_across_hunks() -> Result<()> {
        let token = fake_github_token("ghp_");
        let path = "crates/example/src/lib.rs";
        let text = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n\
             @@ -1,1 +10,1 @@\n+first = \"{token}\"\n\
             @@ -50,2 +60,3 @@\n context\n+second = \"{token}\"\n"
        );

        let findings = scan_unified_diff(&text, &Allowlist::default())?;

        assert_eq!(findings.iter().map(|finding| finding.line).collect::<Vec<_>>(), vec![10, 61]);
        Ok(())
    }

    #[test]
    fn allowlisted_paths_are_skipped_whole() -> Result<()> {
        let token = fake_github_token("ghp_");
        let line = format!("+token = \"{token}\"");
        let allowlist = Allowlist::parse("crates/example/src/fixture.rs\n");

        let findings =
            scan_unified_diff(&diff("crates/example/src/fixture.rs", &[&line]), &allowlist)?;
        assert_eq!(findings, vec![]);

        let findings =
            scan_unified_diff(&diff("crates/example/src/other.rs", &[&line]), &allowlist)?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }

    #[test]
    fn deleted_files_have_no_post_image_to_flag() -> Result<()> {
        let token = fake_github_token("ghp_");
        let path = "crates/example/src/lib.rs";
        let text = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ /dev/null\n\
             @@ -10,1 +0,0 @@\n-token = \"{token}\"\n"
        );

        assert_eq!(scan_unified_diff(&text, &Allowlist::default())?, vec![]);
        Ok(())
    }

    #[test]
    fn all_extensions_are_in_subject() -> Result<()> {
        let token = fake_github_token("ghp_");
        let line = format!("+token = \"{token}\"");
        for path in ["docs/notes.md", "infrastructure/config.toml", "Makefile", "scripts/deploy"] {
            assert_eq!(
                matched_rules(path, &[&line])?.len(),
                1,
                "expected token-shaped additions in {path} to be in subject"
            );
        }
        Ok(())
    }

    #[test]
    fn malformed_boundary_drops_the_current_file() -> Result<()> {
        let token = fake_github_token("ghp_");
        let text = format!("diff --git a/broken\n@@ -10,1 +10,1 @@\n+token = \"{token}\"\n");

        assert_eq!(scan_unified_diff(&text, &Allowlist::default())?, vec![]);
        Ok(())
    }

    #[test]
    fn unparseable_hunk_header_still_reports_with_unknown_line() -> Result<()> {
        let token = fake_github_token("ghp_");
        let path = "crates/example/src/lib.rs";
        let text = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n\
             @@ malformed @@\n+token = \"{token}\"\n"
        );

        let findings = scan_unified_diff(&text, &Allowlist::default())?;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 0);
        Ok(())
    }

    #[test]
    fn pr_body_extractor_reads_title_and_body() -> Result<()> {
        let text =
            extract_pr_body(r#"{"pull_request": {"title": "Rotate", "body": "token inside"}}"#)?;
        assert_eq!(text, Some("Rotate\ntoken inside".to_owned()));
        Ok(())
    }

    #[test]
    fn pr_body_extractor_returns_none_without_pull_request() -> Result<()> {
        assert_eq!(extract_pr_body(r#"{"ref": "refs/heads/main"}"#)?, None);
        assert_eq!(extract_pr_body(r#"{"pull_request": null}"#)?, Some("\n".to_owned()));
        Ok(())
    }

    #[test]
    fn pr_body_extractor_rejects_malformed_json() {
        assert!(extract_pr_body("{not json").is_err());
    }

    #[test]
    fn pr_body_text_is_scanned_with_body_line_numbers() -> Result<()> {
        let token = fake_github_token("ghp_");
        let findings =
            scan_text("pull-request body", &format!("first line\npasted: {token}\nlast"))?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "pull-request body");
        assert_eq!(findings[0].line, 2);
        assert_eq!(findings[0].rule, "github-token");
        Ok(())
    }

    #[test]
    fn allowlist_parses_paths_and_ignores_comments_and_blanks() {
        let allowlist =
            Allowlist::parse("# fixtures only\n\ncrates/a/fixture.rs\n  crates/b/fixture.rs  \n");
        assert!(allowlist.contains("crates/a/fixture.rs"));
        assert!(allowlist.contains("crates/b/fixture.rs"));
        assert!(!allowlist.contains("crates/a/other.rs"));
        assert!(!allowlist.contains("# fixtures only"));
        assert!(!allowlist.contains(""));
        assert_eq!(allowlist.len(), 2);
        assert!(!allowlist.is_empty());
        assert!(Allowlist::default().is_empty());
    }

    #[test]
    fn rule_patterns_do_not_match_their_own_source_text() -> Result<()> {
        // Self-hosting pin: the shapes ship inside the hook scripts, the gate
        // source, and this test module, so a shape matching its own spelling
        // would flag the guard's own diff on every run.
        let shapes = compiled_shapes()?;
        for (name, ere) in TOKEN_SHAPES {
            for (index, shape) in shapes.iter().enumerate() {
                assert!(
                    !shape.is_match(ere),
                    "rule {} matches the source text of rule {name}",
                    TOKEN_SHAPES[index].0
                );
            }
        }
        Ok(())
    }

    #[test]
    fn findings_carry_no_line_content() -> Result<()> {
        let token = fake_github_token("ghp_");
        let findings = scan_text("pull-request body", &token)?;

        assert_eq!(findings.len(), 1);
        assert!(
            !format!("{:?}", findings[0]).contains(&token),
            "a finding must name the location, never reproduce the credential"
        );
        Ok(())
    }
}
