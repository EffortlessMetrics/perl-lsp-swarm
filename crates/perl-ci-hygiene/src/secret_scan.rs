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
use std::process::Command;
use std::sync::LazyLock;

/// Repo-relative path of the scan allowlist.
///
/// The file lists exact repo-relative paths, one per line; `#` comments and
/// blank lines are ignored. A listed path is skipped by the hook scan and the
/// gate alike. It exists for inert test fixtures that must carry
/// token-shaped bytes; anything listed here is a conscious exemption, not an
/// accident, so entries should be rare and fixture-shaped.
pub const ALLOWLIST_PATH: &str = ".ci/secret-scan-allowlist.txt";

/// Rule name of the generic key-word assignment shape in [`TOKEN_SHAPES`].
///
/// Named so the reference-exclusion gate in the scan loop cannot drift from
/// the shape table.
pub const GENERIC_ASSIGNMENT_RULE: &str = "generic-assignment";

/// Lines the generic rule must not report: a key-word assigned a bare
/// identifier-shaped value terminated by code punctuation.
///
/// Matches `api_key = get_key_from_vault;` (and `,`, `)`, `[` siblings, with
/// optional whitespace before the punctuation) so computed values, kwarg
/// references, and indexed lookups read as references, not credentials. The
/// value run is deliberately the pure identifier charset: a run containing a
/// token-special character (`-`, `.`, `/`, `+`, `=`) still reports — a shell
/// one-liner like `export TOKEN=sk-live-...;` is a literal, not a reference.
/// Fully quoted values never match either: a quoted 16+ character string is
/// indistinguishable from a passphrase, so it stays in subject (fail closed).
///
/// Line granularity is the known limit: a line mixing a bare reference with a
/// second, quoted generic assignment is excluded whole. The anchored
/// family shapes (github/aws/slack/private-key) are unaffected and still fire
/// on such a line.
///
/// Like [`TOKEN_SHAPES`], the spelling must stay valid POSIX ERE: the hooks
/// apply it through `grep -E -v`, and `secret_scan_shell_matches_lib_rules`
/// fails if the embedded shell copy drifts.
pub const GENERIC_REFERENCE_EXCLUSION: &str = "([Aa][Pp][Ii][_-]?[Kk][Ee][Yy]|[Aa][Pp][Ii][_-]?[Tt][Oo][Kk][Ee][Nn]|[Ss][Ee][Cc][Rr][Ee][Tt][_-]?[Kk][Ee][Yy]|[Aa][Cc][Cc][Ee][Ss][Ss][_-]?[Tt][Oo][Kk][Ee][Nn])[[:space:]]*[:=][[:space:]]*[A-Za-z0-9_]{16,}[[:space:]]*([;,)]|\\[)";

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
///
/// Bare-identifier references need a second layer the guard cannot express:
/// `api_key = get_key_from_vault;` is an 18-character value run followed by
/// `;`, which the guard accepts, yet it names a computed value rather than a
/// credential. [`GENERIC_REFERENCE_EXCLUSION`] carves exactly that shape back
/// out (see its contract); the hooks embed the same spelling so both layers
/// stay in lockstep.
pub const TOKEN_SHAPES: [(&str, &str); 5] = [
    ("github-token", "gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22}_[A-Za-z0-9]{59}"),
    ("aws-access-key", "AKIA[0-9A-Z]{16}"),
    ("slack-token", "xox[baprs]-[A-Za-z0-9-]{10,48}"),
    ("private-key", "-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"),
    (
        GENERIC_ASSIGNMENT_RULE,
        "([Aa][Pp][Ii][_-]?[Kk][Ee][Yy]|[Aa][Pp][Ii][_-]?[Tt][Oo][Kk][Ee][Nn]|[Ss][Ee][Cc][Rr][Ee][Tt][_-]?[Kk][Ee][Yy]|[Aa][Cc][Cc][Ee][Ss][Ss][_-]?[Tt][Oo][Kk][Ee][Nn])[[:space:]]*[:=][[:space:]]*[\"']?[A-Za-z0-9_./+=-]{16,}($|[^A-Za-z0-9_./+()=-])",
    ),
];

/// [`TOKEN_SHAPES`] compiled in order, so index `i` of this vector is the
/// regex for `TOKEN_SHAPES[i]`.
static COMPILED_SHAPES: LazyLock<Result<Vec<Regex>, regex::Error>> =
    LazyLock::new(|| TOKEN_SHAPES.iter().map(|(_, ere)| Regex::new(ere)).collect());

/// [`GENERIC_REFERENCE_EXCLUSION`] compiled once, beside the shapes it gates.
static COMPILED_EXCLUSION: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(GENERIC_REFERENCE_EXCLUSION));

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
    /// Loads the allowlist at `repo_root`/[`ALLOWLIST_PATH`] from the working tree.
    ///
    /// Working-tree load serves local, advisory use only — a contributor
    /// previewing exemptions against uncommitted edits. The required gate
    /// must use [`Allowlist::load_from_rev`] instead: the PR-head checkout is
    /// the hostile side of the trust boundary, and a working-tree load lets a
    /// PR widen its own exemptions in the same diff it needs them for.
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

    /// Loads the allowlist blob at `revision`:[`ALLOWLIST_PATH`].
    ///
    /// The required gate reads exemptions from the trusted base revision and
    /// ignores the PR-head version, so a PR cannot exempt itself by widening
    /// the allowlist in the same diff. Deny by default: only a cleanly-absent
    /// allowlist at the base means "no exemptions". An unresolvable revision,
    /// an unreadable blob, or undecodable bytes fail closed with an error. A
    /// `git show` miss on a verified base can only narrow exemptions to none,
    /// which widens the scan rather than narrowing it — the safe direction.
    ///
    /// # Errors
    ///
    /// Returns an error when `git` cannot verify `revision`, when `revision`
    /// does not resolve, when the blob cannot be read, or when its bytes are
    /// not valid UTF-8. A revision that resolves but carries no allowlist
    /// loads as empty, never as an error.
    pub fn load_from_rev(repo_root: &Path, revision: &str) -> Result<Self> {
        let verified = Command::new("git")
            .current_dir(repo_root)
            .args(["cat-file", "-e", revision])
            .status()
            .map_err(|error| {
                eyre!("failed to verify the secret-scan base '{revision}': {error}")
            })?;
        if !verified.success() {
            return Err(eyre!(
                "secret scan evaluated nothing: base '{revision}' does not resolve; failing closed"
            ));
        }
        let spec = format!("{revision}:{ALLOWLIST_PATH}");
        let output =
            Command::new("git").current_dir(repo_root).args(["show", &spec]).output().map_err(
                |error| eyre!("failed to read the secret-scan allowlist at '{spec}': {error}"),
            )?;
        if !output.status.success() {
            // The base resolved, so the allowlist is simply absent there.
            return Ok(Self::default());
        }
        let text = String::from_utf8(output.stdout).map_err(|error| {
            eyre!(
                "the secret-scan allowlist at '{spec}' is not valid UTF-8 ({error}); failing closed"
            )
        })?;
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
/// Returns an error when one of this module's static shapes failed to compile,
/// or when a file boundary cannot be attributed to a path. The latter fails
/// the scan rather than silently skipping the file's hunks.
pub fn scan_unified_diff(diff: &str, allowlist: &Allowlist) -> Result<Vec<SecretFinding>> {
    let shapes = compiled_shapes()?;
    let exclusion = compiled_exclusion()?;
    let mut findings = Vec::new();
    let mut current_file: Option<String> = None;
    let mut in_hunk = false;
    let mut new_line: usize = 0;

    for line in diff.lines() {
        if let Some(path) = post_image_path(line) {
            let Some(file) = path else {
                // An unattributable boundary fails the scan instead of silently
                // dropping the file: every hunk past this line would otherwise
                // report clean without ever being examined.
                return Err(eyre!(
                    "secret scan cannot attribute a diff boundary, refusing to silently skip it: {line}"
                ));
            };
            current_file = Some(file);
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
                scan_line_into(file, new_line, added, shapes, exclusion, &mut findings);
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
    let exclusion = compiled_exclusion()?;
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        scan_line_into(file, index + 1, line, shapes, exclusion, &mut findings);
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
///
/// A generic-assignment match on a line that also matches `exclusion` is a
/// bare-identifier reference ([`GENERIC_REFERENCE_EXCLUSION`]), not a
/// credential, and is not recorded. Every other rule reports unconditionally.
fn scan_line_into(
    file: &str,
    line_number: usize,
    line: &str,
    shapes: &[Regex],
    exclusion: &Regex,
    findings: &mut Vec<SecretFinding>,
) {
    for (index, shape) in shapes.iter().enumerate() {
        if shape.is_match(line) {
            if TOKEN_SHAPES[index].0 == GENERIC_ASSIGNMENT_RULE && exclusion.is_match(line) {
                continue;
            }
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
/// boundary is malformed; the caller fails closed on the latter rather than
/// silently dropping the file. Mirrors `must_context::post_image_path`: the
/// path is anchored on the last ` b/` occurrence, and a rename reports the
/// post-image (`b/`) side.
///
/// Git quotes names holding non-ASCII bytes, quotes, tabs, or backslashes
/// (`core.quotePath`, the default), emitting
/// `diff --git "a/caf\303\251" "b/caf\303\251"`. The separator there is ` "b/`,
/// not ` b/`, so the quoted form is detected by its trailing quote and
/// C-unescaped; without this every hunk in such a file would be unattributed.
fn post_image_path(line: &str) -> Option<Option<String>> {
    let rest = line.strip_prefix("diff --git ")?;
    let raw = if rest.ends_with('"') {
        rest.rfind(" \"b/").and_then(|index| unquote_git_path(&rest[index + 1..]))
    } else {
        rest.rfind(" b/").map(|index| rest[index + 1..].to_owned())
    };
    let path = raw.and_then(|p| p.strip_prefix("b/").map(str::to_owned)).filter(|p| !p.is_empty());
    Some(path)
}

/// Decodes a C-style quoted path as emitted by git (for example `"b/caf\303\251"`).
///
/// Handles the single-letter escapes plus three-digit octal for non-ASCII
/// bytes. Returns `None` on any malformed escape so the caller fails closed.
/// Invalid UTF-8 decodes lossy rather than erroring: a latin-1 filename must
/// still be scanned, and lossy decoding can only mislabel — never skip — its
/// hunks.
fn unquote_git_path(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let escaped = *bytes.get(index)?;
        match escaped {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'\\' | b'"' => out.push(escaped),
            b'0'..=b'3' => {
                let digits = bytes.get(index..index + 3)?;
                if !digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
                    return None;
                }
                let value = (u32::from(digits[0] - b'0') << 6)
                    | (u32::from(digits[1] - b'0') << 3)
                    | u32::from(digits[2] - b'0');
                out.push(value as u8);
                index += 2;
            }
            _ => return None,
        }
        index += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
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

/// Resolves the compiled generic-reference exclusion without panicking.
fn compiled_exclusion() -> Result<&'static Regex> {
    COMPILED_EXCLUSION
        .as_ref()
        .map_err(|error| eyre!("failed to compile the secret-scan generic exclusion: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        Allowlist, GENERIC_REFERENCE_EXCLUSION, TOKEN_SHAPES, compiled_exclusion, compiled_shapes,
        extract_pr_body, post_image_path, scan_text, scan_unified_diff,
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
    fn bare_identifier_references_are_not_credentials() -> Result<()> {
        // A key-word assigned a bare identifier terminated by code punctuation
        // names a computed value, a kwarg reference, or an indexed lookup —
        // not an embedded credential.
        // The `;`- and `,`-terminated cases are format-built: spelling one out
        // as a literal source line would itself flag the guard's own diff
        // (the string quote sits between the value run and the punctuation,
        // defeating the exclusion), while only the runtime value must read
        // as a reference.
        for line in [
            format!("+api_key = {};", "get_key_from_vault"),
            format!("+api_key = {},", "get_key_from_vault"),
            "+connect(api_key=get_key_from_vault)".to_owned(),
            "+api_key = get_key_from_vault ;".to_owned(),
            format!("+secret_key = {},", "load_secret_key_from_env"),
            "+access_token = fetch_access_token_here[0]".to_owned(),
        ] {
            assert_eq!(
                matched_rules("crates/example/src/lib.rs", &[&line])?,
                Vec::<&str>::new(),
                "expected {line} to read as a reference, not a credential"
            );
        }
        Ok(())
    }

    #[test]
    fn unquoted_token_values_still_flag() -> Result<()> {
        // The reference exclusion must not swallow real unquoted values:
        // end-of-line, trailing whitespace, a trailing comment, and a value
        // run holding a token-special character all stay in subject.
        let special = format!("{}-{}", "q".repeat(8), "r".repeat(9));
        for line in [
            format!("+secret-key: {}", "x".repeat(16)),
            format!("+api_key = {}   ", "v".repeat(20)),
            format!("+secret_key: {} # production", "w".repeat(18)),
            format!("+export API_TOKEN={special}; echo deployed"),
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
    fn quoted_values_still_flag() -> Result<()> {
        // Quoted literals report even when statement-terminated (JS/Perl
        // style) or identifier-shaped (a quoted 16+ character string is
        // indistinguishable from a passphrase, so it stays in subject).
        for line in [
            format!("+api_key = \"{}\";", "v".repeat(20)),
            "+api_key = \"get_key_from_vault_abc\";".to_owned(),
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
    fn exclusion_does_not_gate_family_shapes() -> Result<()> {
        // The reference exclusion only gates the generic rule: an anchored
        // family shape on the same line still reports.
        let token = fake_github_token("ghp_");
        let line = format!("+api_key = get_key_from_vault; token = \"{token}\"");
        assert_eq!(
            matched_rules("crates/example/src/lib.rs", &[&line])?,
            vec!["github-token"],
            "a family shape beside a reference must still report"
        );
        Ok(())
    }

    #[test]
    fn exclusion_compiles_and_matches_references() -> Result<()> {
        let exclusion = compiled_exclusion()?;
        assert!(exclusion.is_match("api_key = get_key_from_vault;"));
        assert!(!exclusion.is_match("api_key = \"vVVVVVVVVVVVVVVVVVVVV\""));
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
    fn malformed_boundary_fails_closed() {
        let token = fake_github_token("ghp_");
        for boundary in [
            "diff --git a/broken".to_owned(),
            "diff --git a/broken b/".to_owned(),
            "diff --git \"a/broken\" \"unterminated".to_owned(),
            "diff --git \"a/old\" \"b/ba\\d_decode\"".to_owned(),
        ] {
            let text = format!("{boundary}\n@@ -10,1 +10,1 @@\n+token = \"{token}\"\n");
            assert!(
                scan_unified_diff(&text, &Allowlist::default()).is_err(),
                "expected {boundary} to fail the scan rather than silently skip the file"
            );
        }
    }

    #[test]
    fn added_lines_starting_with_plus_are_scanned() -> Result<()> {
        // A content line that itself starts with `+` gains a second marker in
        // the diff (`++token = …`); stripping one marker must still scan it.
        let token = fake_github_token("ghp_");
        let line = format!("++token = \"{token}\"");
        assert_eq!(
            matched_rules("crates/example/src/lib.rs", &[&line])?,
            vec!["github-token"],
            "an added line whose content starts with + must still be scanned"
        );
        Ok(())
    }

    #[test]
    fn quoted_non_ascii_boundary_is_attributed_and_scanned() -> Result<()> {
        // git quotes non-ASCII names (core.quotePath): the post-image side
        // arrives as `"b/caf\303\251.txt"`, whose separator is ` "b/`, not ` b/`.
        let token = fake_github_token("ghp_");
        let text = format!(
            "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n\
             --- \"a/caf\\303\\251.txt\"\n+++ \"b/caf\\303\\251.txt\"\n\
             @@ -10,1 +10,1 @@\n+token = \"{token}\"\n"
        );

        let findings = scan_unified_diff(&text, &Allowlist::default())?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "caf\u{e9}.txt");
        assert_eq!(findings[0].rule, "github-token");
        Ok(())
    }

    #[test]
    fn allowlist_matches_unquoted_paths() -> Result<()> {
        // The allowlist holds real paths; a quoted boundary must resolve to
        // the same spelling before the exemption check.
        let token = fake_github_token("ghp_");
        let text = format!(
            "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n\
             --- \"a/caf\\303\\251.txt\"\n+++ \"b/caf\\303\\251.txt\"\n\
             @@ -10,1 +10,1 @@\n+token = \"{token}\"\n"
        );
        let allowlist = Allowlist::parse("caf\u{e9}.txt\n");

        assert_eq!(scan_unified_diff(&text, &allowlist)?, vec![]);
        Ok(())
    }

    #[test]
    fn quoted_boundary_with_spaces_and_escapes_is_attributed() {
        assert_eq!(
            post_image_path("diff --git \"a/my\\tfile.txt\" \"b/my\\tfile.txt\""),
            Some(Some("my\tfile.txt".to_owned()))
        );
        assert_eq!(
            post_image_path("diff --git a/plain.txt b/plain.txt"),
            Some(Some("plain.txt".to_owned()))
        );
        assert_eq!(post_image_path("+++ b/plain.txt"), None);
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
        // would flag the guard's own diff on every run. The exclusion travels
        // the same road, so it is pinned both ways too.
        let shapes = compiled_shapes()?;
        let exclusion = compiled_exclusion()?;
        for (name, ere) in TOKEN_SHAPES {
            for (index, shape) in shapes.iter().enumerate() {
                assert!(
                    !shape.is_match(ere),
                    "rule {} matches the source text of rule {name}",
                    TOKEN_SHAPES[index].0
                );
            }
            assert!(
                !exclusion.is_match(ere),
                "the generic exclusion matches the source text of rule {name}"
            );
        }
        for (index, shape) in shapes.iter().enumerate() {
            assert!(
                !shape.is_match(GENERIC_REFERENCE_EXCLUSION),
                "rule {} matches the generic exclusion source text",
                TOKEN_SHAPES[index].0
            );
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
