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
///
/// Raw forms are matched longest-first, up to three hashes, then the ordinary
/// form (honouring backslash escapes). A regex cannot balance an arbitrary hash
/// count, so three is an explicit ceiling: `r####"…"####` is not in subject.
/// The tree contains no raw-string `.expect` at all today, and the ceiling is
/// documented rather than silent so that a bypass is a known one.
///
/// `\s*` after `.expect(` spans newlines, so this matches the rustfmt-wrapped
/// form where the explanation sits on its own line — which is why the removed
/// side is joined before matching, exactly as the added side is.
static EXPECT_CONTEXT_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r####"\.expect\(\s*(?:r###"(?<raw3>(?s:.)*?)"###|r##"(?<raw2>(?s:.)*?)"##|r#"(?<raw1>(?s:.)*?)"#|r"(?<raw0>[^"]*)"|"(?<plain>(?:[^"\\]|\\(?s:.))*)")"####,
    )
});

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
    /// Every removed line of the hunk, joined, so an `.expect(…)` whose
    /// explanation rustfmt wrapped onto its own line is still one match.
    /// Scanning removed lines individually missed exactly that form, which is
    /// the common shape for the sentence-length explanations `_with` exists to
    /// carry.
    removed_text: String,
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
    fn into_finding(self, expect_re: &Regex) -> Option<ContextDrop> {
        if self.bare_calls.is_empty() {
            return None;
        }
        // The explanation must survive as a string literal, quotes included.
        // Matching the bare substring would let an identifier that happens to
        // contain it (`load_a` "carrying" the context `a`) hide a real drop.
        let dropped_contexts: Vec<String> = expect_re
            .captures_iter(&self.removed_text)
            .filter_map(|capture| {
                ["raw3", "raw2", "raw1", "raw0", "plain"]
                    .iter()
                    .find_map(|name| capture.name(name))
                    .map(|matched| matched.as_str().to_owned())
            })
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
            close_hunk(&mut hunk, &mut findings, expect_re);
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
            close_hunk(&mut hunk, &mut findings, expect_re);
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
            active.removed_text.push_str(removed);
            active.removed_text.push('\n');
        } else if let Some(added) = line.strip_prefix('+') {
            // The unmasked line feeds the survival search, which must be able
            // to find an explanation *inside* a string literal. Call detection
            // uses the masked copy, so `// call must(x) to migrate` is prose,
            // not a helper invocation.
            active.added_text.push_str(added);
            active.added_text.push('\n');
            if bare_re.is_match(&mask_literals_and_line_comments(added)) {
                active.bare_calls.push(added.trim().to_owned());
            }
        }
    }

    close_hunk(&mut hunk, &mut findings, expect_re);
    Ok(findings)
}

/// Finalizes `hunk`, pushing its finding onto `findings` when it has one.
fn close_hunk(
    hunk: &mut Option<HunkAccumulator>,
    findings: &mut Vec<ContextDrop>,
    expect_re: &Regex,
) {
    if let Some(finding) = hunk.take().and_then(|active| active.into_finding(expect_re)) {
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

/// Blanks string-literal bodies and `//` comment text, preserving every other
/// byte, so a `must(` that is prose rather than code is not read as a call.
///
/// Whole-line scope is deliberate: with `--unified=0` there is no reliable
/// cross-line lexer state in a diff, and a line is the largest unit that can be
/// masked without one. A `must(` inside a *multi-line* string body therefore
/// still reads as a call — no worse than not masking at all, and the remedy for
/// that and every other false positive is the same one line (`_with`, or keep
/// the explanation).
///
/// Block comments are not handled, for the same reason: `/* … */` spans lines.
fn mask_literals_and_line_comments(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut masked = String::with_capacity(line.len());
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            masked.extend(std::iter::repeat_n(' ', chars.len() - index));
            break;
        }

        if let Some(end) = raw_string_body_end(&chars, index) {
            // `r`, its hashes and the opening quote stay; the body is blanked.
            let body_start = index + raw_string_opener_len(&chars, index);
            masked.extend(chars[index..body_start].iter());
            masked.extend(std::iter::repeat_n(' ', end - body_start));
            index = end;
            continue;
        }

        if chars[index] == '"' {
            masked.push('"');
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                // A backslash escapes the next character, `\"` included.
                let step = if chars[index] == '\\' { 2 } else { 1 };
                masked.extend(std::iter::repeat_n(' ', step.min(chars.len() - index)));
                index += step;
            }
            // Consume the closing quote here. Leaving it for the outer loop
            // would read it as the *opening* quote of the next string, masking
            // the real code between two literals — which hid a genuine
            // `IpAddr::V6(must(…))` call sitting between them.
            if index < chars.len() {
                masked.push('"');
                index += 1;
            }
            continue;
        }

        masked.push(chars[index]);
        index += 1;
    }

    masked
}

/// Length of a raw-string opener (`r`, its hashes, and the quote) at `index`,
/// or `0` when one does not start there.
fn raw_string_opener_len(chars: &[char], index: usize) -> usize {
    if chars[index] != 'r' {
        return 0;
    }
    if index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_') {
        return 0;
    }
    let mut cursor = index + 1;
    while chars.get(cursor) == Some(&'#') {
        cursor += 1;
    }
    if chars.get(cursor) == Some(&'"') { cursor + 1 - index } else { 0 }
}

/// Index just past the body of a raw string starting at `index`, or `None` when
/// no raw string starts there. An unterminated body runs to end of line.
/// "Just past the body" includes the closing quote and its trailing hashes:
/// the masker consumes the whole literal, so leaving the closing quote in the
/// input would make the ordinary-`"` branch treat it as an *opening* quote and
/// mask the real code (e.g. a `must(…)` call) that follows the literal.
fn raw_string_body_end(chars: &[char], index: usize) -> Option<usize> {
    let opener = raw_string_opener_len(chars, index);
    if opener == 0 {
        return None;
    }
    let hashes = opener - 2; // minus the leading `r` and the opening quote
    let mut cursor = index + opener;
    while cursor < chars.len() {
        if chars[cursor] == '"'
            && chars[cursor + 1..].iter().take(hashes).filter(|c| **c == '#').count() == hashes
        {
            return Some((cursor + 1 + hashes).min(chars.len()));
        }
        cursor += 1;
    }
    Some(chars.len())
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
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        // Not a pass: nothing was compared. Said plainly so a green line is
        // never mistaken for evidence that no explanation was dropped.
        println!(
            "{YELLOW}• must* context guard not evaluated{NC}: no base ref resolved (tried: {}). \
             Pass --base to name one.",
            base_candidates().join(", ")
        );
        return Ok(0);
    };
    let base = merge_base(repo_root, &requested_base);
    let diff = read_diff(repo_root, &base)?;
    let findings = scan_unified_diff(&diff)?;

    if findings.is_empty() {
        println!(
            "{GREEN}✅ No assertion context dropped by a must* migration{NC} (base: {requested_base})"
        );
        return Ok(0);
    }

    println!("{RED}❌ must* migration dropped assertion context{NC} (base: {requested_base})");
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
///
/// An explicitly requested base that does not resolve is an error: the caller
/// named a subject that does not exist. Auto-resolution finding nothing is not
/// — that is "no subject to evaluate", which [`check`] reports as an
/// unevaluated run rather than as a violation. A shallow clone with no
/// `origin/main` and no `HEAD~1` must not be able to fail this guard, because
/// an absent diff is not a dropped explanation.
fn resolve_base(repo_root: &Path, requested: Option<&str>) -> Result<Option<String>> {
    if let Some(base) = requested {
        if ref_exists(repo_root, base) {
            return Ok(Some(base.to_owned()));
        }
        return Err(eyre!("base ref '{base}' does not resolve in {}", repo_root.display()));
    }

    Ok(base_candidates().into_iter().find(|candidate| ref_exists(repo_root, candidate)))
}

/// Resolves the merge base of `base` and `HEAD`, falling back to `base` itself.
///
/// The scan then diffs that commit against the **working tree**, so a migration
/// a contributor has edited but not yet committed is in subject. Diffing
/// `base...HEAD` instead reported a clean result for exactly the uncommitted
/// change the local gate exists to catch. In CI the tree is clean, so the two
/// ranges agree.
fn merge_base(repo_root: &Path, base: &str) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(["merge-base", base, "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| base.to_owned())
}

/// Returns `true` when `git rev-parse --verify` resolves `reference`.
fn ref_exists(repo_root: &Path, reference: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Reads the Rust-source diff from `base` to the working tree, zero context.
///
/// Zero context keeps hunks minimal, so a removal and an addition are paired
/// only when they are genuinely adjacent. `base` is already the merge base, and
/// the range is two-dot so staged and unstaged edits are included.
///
/// The `a/`/`b/` prefixes are pinned explicitly. [`post_image_path`] finds the
/// file boundary by the ` b/` separator, so an ambient `diff.noprefix=true`
/// would emit boundaries this scanner cannot attribute and silently report
/// every file as clean — a false green driven by config the guard does not
/// own.
fn read_diff(repo_root: &Path, base: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "diff",
            "--unified=0",
            "--no-color",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base,
            "--",
            "*.rs",
        ])
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
    fn a_rustfmt_wrapped_expect_still_reports_its_dropped_explanation() -> Result<()> {
        // rustfmt breaks any `.expect("…")` whose explanation is long enough,
        // which is precisely the sentence-length case `_with` exists for.
        // Scanning removed lines one at a time never saw the literal.
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                "-    let value = load()",
                "-        .expect(",
                r#"-            "the fixture declares Example with an explanation long enough to wrap","#,
                "-        );",
                "+    let value = must(load());",
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].dropped_contexts,
            vec!["the fixture declares Example with an explanation long enough to wrap".to_owned()]
        );
        Ok(())
    }

    #[test]
    fn a_wrapped_expect_carried_into_a_with_call_stays_clean() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                "-    let value = load()",
                "-        .expect(",
                r#"-            "the fixture declares Example with an explanation long enough to wrap","#,
                "-        );",
                "+    let value = must_with(",
                "+        load(),",
                r#"+        "the fixture declares Example with an explanation long enough to wrap","#,
                "+    );",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn raw_string_explanations_are_in_subject() -> Result<()> {
        for (removed, expected) in [
            (
                r#"-        let v = load().expect(r"the raw fixture declares Example");"#,
                "the raw fixture declares Example",
            ),
            (
                r##"-        let v = load().expect(r#"the hashed "raw" fixture"#);"##,
                r#"the hashed "raw" fixture"#,
            ),
            (
                r###"-        let v = load().expect(r##"the twice-hashed "#raw" fixture"##);"###,
                r##"the twice-hashed "#raw" fixture"##,
            ),
        ] {
            let text =
                diff("crates/example/src/lib.rs", &[removed, "+        let v = must(load());"]);

            let findings = scan_unified_diff(&text)?;

            assert_eq!(findings.len(), 1, "expected {removed} to be flagged");
            assert_eq!(findings[0].dropped_contexts, vec![expected.to_owned()]);
        }
        Ok(())
    }

    #[test]
    fn a_must_call_written_in_a_comment_is_not_a_call() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let v = load().expect("the fixture declares Example");"#,
                "+        let v = load()?;",
                "+        // migrate the remaining sites with must(x) once #14291 lands",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn a_must_call_written_inside_a_string_literal_is_not_a_call() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let v = load().expect("the fixture declares Example");"#,
                "+        let v = load()?;",
                r#"+        let hint = "call must(x) to migrate";"#,
            ],
        );

        assert_eq!(scan_unified_diff(&text)?, vec![]);
        Ok(())
    }

    #[test]
    fn a_call_between_two_string_literals_stays_visible() -> Result<()> {
        // Verbatim from PR #12000 (`ai_destination_policy_tests.rs`). Masking
        // string bodies must not swallow the code *between* two literals: an
        // earlier masker consumed the closing quote as the next opening quote
        // and lost this genuine drop.
        let text = diff(
            "crates/perl-lsp-rs-core/tests/ai_destination_policy_tests.rs",
            &[
                r#"-            IpAddr::V6("fd00::1".parse().expect("valid ipv6 literal")),"#,
                r#"+            ("https://[fd00::1]/v1", IpAddr::V6(must("fd00::1".parse()))),"#,
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].dropped_contexts, vec!["valid ipv6 literal".to_owned()]);
        Ok(())
    }

    #[test]
    fn masking_leaves_a_real_call_beside_prose_visible() -> Result<()> {
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let v = load().expect("the fixture declares Example");"#,
                "+        // prose mentioning must(x) must not hide the real call below",
                "+        let v = must(load());",
            ],
        );

        assert_eq!(scan_unified_diff(&text)?.len(), 1);
        Ok(())
    }

    #[test]
    fn masking_a_raw_string_does_not_hide_the_call_that_follows_it() -> Result<()> {
        // The raw-string masker must consume the closing quote (and hashes) of
        // the literal. Leaving the quote in the input made the ordinary-`"`
        // branch read it as an *opening* quote and mask the rest of the line —
        // hiding a real `must(` call that follows the literal on the same line
        // (the #12000 subject shape, `IpAddr::V6(must("fd00::1".parse()))`).
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let v = load().expect("valid ipv6 literal");"#,
                r#"+        let prefix = r"fd00:"; let v = IpAddr::V6(must(load()));"#,
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(
            findings.len(),
            1,
            "a bare must( following a raw-string literal on one line must still be reported"
        );
        assert_eq!(findings[0].dropped_contexts, vec!["valid ipv6 literal".to_owned()]);
        Ok(())
    }

    #[test]
    fn unrelated_edits_sharing_one_hunk_are_a_known_false_positive() -> Result<()> {
        // Evidence is correlated at hunk scope, not structurally: telling
        // "this `must` replaced that `.expect`" apart from "two unrelated edits
        // landed adjacent" needs a Rust-aware diff matcher, which is a
        // different claim. `--unified=0` keeps hunks minimal, so this needs
        // genuinely adjacent unrelated edits. Pinned so the behaviour is
        // visible rather than accidental; the remedy is always one line.
        let text = diff(
            "crates/example/src/lib.rs",
            &[
                r#"-        let a = load_a().expect("the first authority resolves");"#,
                "-        let b = load_b().unwrap();",
                "+        let a = load_a()?;",
                "+        let b = must(load_b());",
            ],
        );

        let findings = scan_unified_diff(&text)?;

        assert_eq!(findings.len(), 1, "hunk-scope correlation reports this pairing");
        assert_eq!(findings[0].dropped_contexts, vec!["the first authority resolves".to_owned()]);
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
