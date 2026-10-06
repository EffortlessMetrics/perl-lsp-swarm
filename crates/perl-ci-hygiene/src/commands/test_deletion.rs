//! Guard against deleting tests (or hollowing their assertions) to fake green (#17405).
//!
//! Nothing executable stopped an agent from deleting a failing test file or
//! gutting its assertions: the ignored-count ratchet counts only `#[ignore]`
//! attributes and has no required-tier caller, and test/assertion *removal*
//! had no guard at all. A weakened suite is indistinguishable from a fixed one
//! to every greenness check, so the only backstop was substantive human review.
//!
//! This guard diffs the Rust sources changed between a base commit and the
//! working tree and fails when a change:
//!
//! - deletes a test file (a removed `.rs` file whose base content carried a
//!   test attribute, or any removed file under a `tests/` directory or with a
//!   test-file name), or
//! - removes test cases (`#[test]` / `#[tokio::test]` / `#[rstest]` count
//!   decreases in a modified file), or
//! - hollows assertions (assertion-macro count decreases in a modified file).
//!
//! Counts are compared per file, on code with comments and string/char literal
//! bodies stripped, so commenting out a test reads as removing it and deleting
//! a doc example containing `assert!` does not. Moving a test from file A to
//! file B still flags file A: per-file accounting is what lets the gate name
//! files, and cross-file netting would let a deleted real test hide behind an
//! added trivial one. A move deserves a linked justification marker like any
//! other removal.
//!
//! ## Justification marker
//!
//! Legitimate removals happen (dead features, rewritten suites). A removal is
//! excused when the change itself introduces a linked justification marker of
//! the form `TEST-DELETION-JUSTIFIED(#NNNN): reason`, either on an added diff
//! line (any file type) or in a commit message between base and HEAD. The
//! marker must be *introduced by the change under review*: a marker that
//! already sits on untouched base lines cannot justify a new removal, so
//! markers cannot be pre-positioned. The gate still lists everything removed,
//! so reviewers see exactly what the marker covers.
//!
//! ## Base pinning
//!
//! The baseline is read from the base side only: file contents come from
//! `git show <merge-base-sha>:<path>`, i.e. immutable git objects the
//! candidate cannot alter. The candidate controls the head side (working tree)
//! and nothing else. The check binary itself runs from the candidate tree,
//! like every `xtask gates` guard; pinning the *checker* to the base commit
//! (the dependency-review pattern applied to code) is a repo-wide property
//! none of those guards has and stays a documented residual risk.
//!
//! ## Vocabulary
//!
//! Test attributes mirror the `serial_test` policy's test-function reader:
//! `test`, `tokio::test`, `rstest`. Assertion signals are the `assert*!` /
//! `debug_assert*!` families, the `perl-test-must` family (`must*`, with and
//! without `_with`), and `.expect(` / `.expect_err(`. Deliberately excluded:
//!
//! - `.unwrap(` — removing unwraps is progress the unwrap ratchet demands;
//!   counting it would flag every unwrap cleanup as weakening.
//! - `todo!` / `unimplemented!` — removing them is implementing the work.
//! - `panic!` — owned separately by the `panic_test` identity ratchet.
//! - free-function `expect(` — only the `.expect(` method form is in subject.
//!
//! [`scan_snapshots`] is pure over caller-supplied file contents;
//! [`justification_in_text`] is pure over diff/commit text; [`check`] is the
//! thin shell that obtains both from `git` and reports the findings.

use color_eyre::eyre::{Context, Result, eyre};
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use crate::{GREEN, NC, RED, YELLOW};

/// Matches a test-function attribute: `#[test]`, `#[tokio::test]`, `#[rstest]`.
///
/// The attribute name must follow the opening bracket (modulo whitespace), so
/// `#[cfg(test)]` — where `test` sits inside a `cfg(...)` argument — never
/// matches. Counted on comment/string-stripped code (see [`strip_code`]).
static TEST_ATTR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"#\s*\[\s*(?:test|tokio::test|rstest)\b"));

/// Matches an `assert*!` / `debug_assert*!` macro invocation.
///
/// The leading `(?:^|[^A-Za-z0-9_])` boundary keeps `reassert!` and
/// `my_assert_eq!` from matching. Alternation order is irrelevant: `assert`
/// cannot complete inside `assert_eq!` because `_` is neither whitespace nor
/// `!`, so the regex backtracks to the longer alternative.
static ASSERT_MACRO_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[^A-Za-z0-9_])(?:assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne)\s*!",
    )
});

/// Matches a `perl-test-must` helper call, with or without `_with`.
///
/// Same boundary shape as the bare-must matcher in `must_context`:
/// `helper_must(` does not match, and the trailing `\s*\(` keeps `_with`
/// variants distinct from their bare siblings for matching purposes — both
/// count, because both are assertion signals.
static MUST_CALL_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[^A-Za-z0-9_])(?:must|must_some|must_err|must_with|must_some_with|must_err_with)\s*\(",
    )
});

/// Matches the `.expect(` / `.expect_err(` method-call form.
static EXPECT_CALL_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\.(?:expect|expect_err)\s*\("));

/// Matches a linked test-deletion justification marker.
///
/// Canonical form: `TEST-DELETION-JUSTIFIED(#NNNN)`. The numeric issue link is
/// mandatory, mirroring the `ignored_tests_check_refs` convention that every
/// weakening must cite a tracking issue. Case-sensitive: prose that merely
/// mentions test deletion must not excuse it.
static JUSTIFICATION_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"TEST-DELETION-JUSTIFIED\s*\(#\d+\)"));

/// Test counts and assertion counts for one file revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FileCounts {
    /// `#[test]` / `#[tokio::test]` / `#[rstest]` attributes.
    pub(crate) tests: usize,
    /// Assertion signals (assert-family + must-family + expect-family).
    pub(crate) assertions: usize,
}

/// One file's base/head contents for the pure scanner.
///
/// `base_content` is `None` for added files; `head_content` is `None` for
/// deleted files. `path` is the repository-relative post-image path (the
/// pre-image path for deletions), used for reporting and test-file-shape
/// classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSnapshot {
    /// Repository-relative path naming the finding.
    pub(crate) path: String,
    /// File text at the base commit, or `None` when the file is added.
    pub(crate) base_content: Option<String>,
    /// Working-tree file text, or `None` when the file is deleted.
    pub(crate) head_content: Option<String>,
}

/// One weakening the scanner found in a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeletionFinding {
    /// Repository-relative path of the weakened file.
    pub(crate) file: String,
    /// What was removed.
    pub(crate) kind: DeletionKind,
}

/// The shape of a single-file weakening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeletionKind {
    /// A test file was deleted outright.
    MissingFile {
        /// Test attributes the base revision carried.
        base_tests: usize,
    },
    /// Test cases were removed from a surviving file.
    FewerTests {
        /// Test attributes at base.
        base: usize,
        /// Test attributes at head.
        head: usize,
    },
    /// Assertions were removed from a surviving file.
    FewerAssertions {
        /// Assertion signals at base.
        base: usize,
        /// Assertion signals at head.
        head: usize,
    },
}

/// Counts test attributes and assertion signals in Rust source text.
///
/// Comments (line, nested block, and doc — all `//`/`/* */` forms) and
/// string/char literal bodies are stripped before counting, so commented-out
/// tests read as removed and prose mentioning `assert!` never counts.
pub(crate) fn count_source(text: &str) -> Result<FileCounts> {
    let code = strip_code(text);
    let test_re = compiled_regex(&TEST_ATTR_RE, "test-attribute")?;
    let assert_re = compiled_regex(&ASSERT_MACRO_RE, "assert-macro")?;
    let must_re = compiled_regex(&MUST_CALL_RE, "must-call")?;
    let expect_re = compiled_regex(&EXPECT_CALL_RE, "expect-call")?;
    Ok(FileCounts {
        tests: test_re.find_iter(&code).count(),
        assertions: assert_re.find_iter(&code).count()
            + must_re.find_iter(&code).count()
            + expect_re.find_iter(&code).count(),
    })
}

/// Reports every weakening in `snapshots`, in input order.
///
/// Added files (no base content) are never findings; deleted files are
/// findings only when the base revision carried a test attribute or the path
/// itself is test-file-shaped; modified files are findings when either count
/// strictly decreases.
pub(crate) fn scan_snapshots(snapshots: &[FileSnapshot]) -> Result<Vec<DeletionFinding>> {
    let mut findings = Vec::new();
    for snapshot in snapshots {
        let Some(base_text) = snapshot.base_content.as_deref() else {
            continue;
        };
        let base_counts = count_source(base_text)?;
        let Some(head_text) = snapshot.head_content.as_deref() else {
            if base_counts.tests > 0 || is_test_file_path(&snapshot.path) {
                findings.push(DeletionFinding {
                    file: snapshot.path.clone(),
                    kind: DeletionKind::MissingFile { base_tests: base_counts.tests },
                });
            }
            continue;
        };
        let head_counts = count_source(head_text)?;
        if head_counts.tests < base_counts.tests {
            findings.push(DeletionFinding {
                file: snapshot.path.clone(),
                kind: DeletionKind::FewerTests { base: base_counts.tests, head: head_counts.tests },
            });
        }
        if head_counts.assertions < base_counts.assertions {
            findings.push(DeletionFinding {
                file: snapshot.path.clone(),
                kind: DeletionKind::FewerAssertions {
                    base: base_counts.assertions,
                    head: head_counts.assertions,
                },
            });
        }
    }
    Ok(findings)
}

/// Returns the first justification marker in `text`, if any.
///
/// `text` is added-diff lines or commit messages — both sources the change
/// under review introduces. Base-side text is never searched, so a marker that
/// predates the change cannot justify a new removal.
pub(crate) fn justification_in_text(text: &str) -> Result<Option<String>> {
    let marker = compiled_regex(&JUSTIFICATION_RE, "justification-marker")?;
    Ok(marker.find(text).map(|matched| matched.as_str().to_owned()))
}

/// Returns `true` when a diff path is test-file-shaped by location or name.
///
/// A path is test-file-shaped when any component is `tests`, or when the file
/// name ends in `_test.rs`, `_tests.rs`, or `tests.rs`. Base content carrying
/// a test attribute also qualifies a deleted file (see [`scan_snapshots`]);
/// this predicate covers test helpers that carry no attribute themselves.
pub(crate) fn is_test_file_path(path: &str) -> bool {
    const TEST_SUFFIXES: [&str; 3] = ["_test.rs", "_tests.rs", "tests.rs"];
    let file = Path::new(path);
    if file.components().any(|component| component.as_os_str() == "tests") {
        return true;
    }
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)))
}

/// Blanks comments and string/char literal bodies, preserving newlines.
///
/// `//` line comments run to end of line; `/* */` block comments nest as in
/// Rust; ordinary strings honour backslash escapes; raw strings honour any
/// hash count; char literals match only the `'x'` / `'\e'` shapes so a
/// lifetime (`&'a str`) never opens one. Every blanked byte becomes a space,
/// so token boundaries — and therefore counts — are unaffected by what the
/// prose happened to contain.
fn strip_code(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                out.push(' ');
                index += 1;
            }
            continue;
        }
        if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            let mut depth = 0usize;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    out.push_str("  ");
                    index += 2;
                    depth += 1;
                } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    out.push_str("  ");
                    index += 2;
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if chars[index] == '\n' {
                    out.push('\n');
                    index += 1;
                } else {
                    out.push(' ');
                    index += 1;
                }
            }
            continue;
        }
        if let Some(end) = raw_string_end(&chars, index) {
            for blanked in &chars[index..end] {
                out.push(if *blanked == '\n' { '\n' } else { ' ' });
            }
            index = end;
            continue;
        }
        if chars[index] == '"' {
            out.push(' ');
            index += 1;
            while index < chars.len() && chars[index] != '"' && chars[index] != '\n' {
                // A backslash escapes the next character, `\"` included; an
                // unterminated string ends at the newline rather than eating
                // the rest of the file.
                let step = if chars[index] == '\\' { 2 } else { 1 };
                for _ in 0..step.min(chars.len() - index) {
                    out.push(' ');
                }
                index += step;
            }
            if index < chars.len() && chars[index] == '"' {
                out.push(' ');
                index += 1;
            }
            continue;
        }
        if let Some(end) = char_literal_end(&chars, index) {
            for _ in index..end {
                out.push(' ');
            }
            index = end;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Index just past a raw string starting at `index`, or `None`.
///
/// Any hash count is honoured (unlike single-line masking, whole-text lexing
/// has no ceiling to document). An unterminated body runs to end of input.
fn raw_string_end(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] != 'r' {
        return None;
    }
    if index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_') {
        return None;
    }
    let mut cursor = index + 1;
    while chars.get(cursor) == Some(&'#') {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'"') {
        return None;
    }
    let hashes = cursor - (index + 1);
    cursor += 1;
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

/// Index just past a char literal starting at `index`, or `None`.
///
/// Only the `'x'` and `'\e'` shapes open a literal. A lone `'` (a lifetime
/// such as `'a`, or a stray quote) is ordinary code: treating it as an opener
/// would swallow everything up to the next `'` elsewhere in the file.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] != '\'' {
        return None;
    }
    if index > 0 && chars[index - 1] == '\'' {
        return None;
    }
    let (body_len, closed) = if chars.get(index + 1) == Some(&'\\') {
        (2, chars.get(index + 3) == Some(&'\''))
    } else {
        (1, chars.get(index + 2) == Some(&'\''))
    };
    if closed { Some(index + 1 + body_len + 1) } else { None }
}

/// One `.rs` path changed between base and working tree.
struct ChangedEntry {
    /// Pre-image path (`None` for added files).
    old_path: Option<String>,
    /// Post-image path (`None` for deleted files).
    new_path: Option<String>,
}

/// Runs the guard against the change between `base` and the working tree.
///
/// Returns the process exit code: `0` when nothing was removed or every
/// removal carries a justification marker introduced by the change itself,
/// `1` otherwise.
///
/// # Errors
///
/// Returns an error when no usable base ref can be resolved and the caller
/// named one explicitly, or when `git` cannot produce the change listing.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        // Not a pass: nothing was compared. Said plainly so a green line is
        // never mistaken for evidence that no test was removed.
        println!(
            "{YELLOW}• test-deletion guard not evaluated{NC}: no base ref resolved (tried: {}). \
             Pass --base to name one.",
            base_candidates().join(", ")
        );
        return Ok(0);
    };
    let base = merge_base(repo_root, &requested_base);
    let entries = read_changed_entries(repo_root, &base)?;
    let snapshots = load_snapshots(repo_root, &base, &entries)?;
    let findings = scan_snapshots(&snapshots)?;

    let compared = snapshots.len();
    if findings.is_empty() {
        let (tests_delta, assertions_delta) = total_deltas(&snapshots)?;
        println!(
            "{GREEN}✅ No test files, test cases, or assertions removed{NC} \
             (base: {requested_base}; compared {compared} changed Rust files: \
             tests {tests_delta:+}, assertions {assertions_delta:+})"
        );
        return Ok(0);
    }

    if let Some(marker) = find_justification(repo_root, &base)? {
        println!(
            "{GREEN}✅ Test removals justified by {marker}{NC} (base: {requested_base}; \
             the listing below is what the marker covers — review it)"
        );
        report_findings(&findings);
        return Ok(0);
    }

    println!("{RED}❌ test suite weakened{NC} (base: {requested_base})");
    report_findings(&findings);
    println!();
    println!(
        "If the removal is legitimate, introduce a linked justification marker in this change:"
    );
    println!("  TEST-DELETION-JUSTIFIED(#issue): reason");
    println!("on an added line or in a commit message between base and HEAD.");
    Ok(1)
}

/// Prints every finding with its per-file delta.
fn report_findings(findings: &[DeletionFinding]) {
    for finding in findings {
        match &finding.kind {
            DeletionKind::MissingFile { base_tests } => {
                println!(
                    "  {YELLOW}removed test file{NC}: {} (base carried {base_tests} test(s))",
                    finding.file
                );
            }
            DeletionKind::FewerTests { base, head } => {
                println!(
                    "  {YELLOW}removed {} test(s){NC}: {} ({base} → {head})",
                    base - head,
                    finding.file
                );
            }
            DeletionKind::FewerAssertions { base, head } => {
                println!(
                    "  {YELLOW}removed {} assertion(s){NC}: {} ({base} → {head})",
                    base - head,
                    finding.file
                );
            }
        }
    }
}

/// Sums base→head deltas over snapshots that have both revisions.
fn total_deltas(snapshots: &[FileSnapshot]) -> Result<(i64, i64)> {
    let mut tests_delta = 0i64;
    let mut assertions_delta = 0i64;
    for snapshot in snapshots {
        let (Some(base_text), Some(head_text)) =
            (snapshot.base_content.as_deref(), snapshot.head_content.as_deref())
        else {
            continue;
        };
        let base = count_source(base_text)?;
        let head = count_source(head_text)?;
        tests_delta += head.tests as i64 - base.tests as i64;
        assertions_delta += head.assertions as i64 - base.assertions as i64;
    }
    Ok((tests_delta, assertions_delta))
}

/// Searches added diff lines (all file types) and in-range commit messages for
/// a justification marker introduced by the change.
fn find_justification(repo_root: &Path, base: &str) -> Result<Option<String>> {
    let added = read_added_lines(repo_root, base)?;
    if let Some(marker) = justification_in_text(&added)? {
        return Ok(Some(marker));
    }
    let messages = read_commit_messages(repo_root, base);
    Ok(justification_in_text(&messages)?.map(|marker| format!("{marker} (commit message)")))
}

/// Loads base/head contents for every changed `.rs` entry.
///
/// Base contents come from `git show <base>:<path>` — the base-pinned side.
/// Head contents are read from the working tree. In CI the tree is clean, so
/// head equals HEAD; locally, staged and unstaged edits are in subject.
fn load_snapshots(
    repo_root: &Path,
    base: &str,
    entries: &[ChangedEntry],
) -> Result<Vec<FileSnapshot>> {
    let mut snapshots = Vec::with_capacity(entries.len());
    for entry in entries {
        let base_content = entry
            .old_path
            .as_deref()
            .map(|path| read_base_file(repo_root, base, path))
            .transpose()?;
        // A surviving path must read: the name-status listing reflects the
        // working tree, so a missing file here is an inconsistency, not a
        // deletion — fail closed with the path named.
        let head_content = entry
            .new_path
            .as_deref()
            .map(|path| {
                std::fs::read_to_string(repo_root.join(path))
                    .with_context(|| format!("reading working-tree file {path}"))
            })
            .transpose()?;
        let Some(path) = entry.new_path.clone().or_else(|| entry.old_path.clone()) else {
            continue;
        };
        snapshots.push(FileSnapshot { path, base_content, head_content });
    }
    Ok(snapshots)
}

/// Lists `.rs` files changed between `base` and the working tree.
///
/// Rename detection is on (`-M`): without it a moved test file reads as a
/// deletion plus an addition and a pure move fails the gate. Output is
/// NUL-separated (`-z`) so paths with spaces or quotes parse exactly.
/// Unmerged (`U`) entries are skipped — a mid-conflict tree is unevaluable,
/// and the conflict-markers gate owns that state.
fn read_changed_entries(repo_root: &Path, base: &str) -> Result<Vec<ChangedEntry>> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--name-status", "-M", "-z", "--no-color", base, "--", "*.rs"])
        .output()
        .map_err(|error| eyre!("failed to run `git diff` against '{base}': {error}"))?;
    if !output.status.success() {
        return Err(eyre!(
            "`git diff {base}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_name_status_z(&output.stdout)
}

/// Parses NUL-separated `git diff --name-status -z` output.
///
/// Records are `STATUS\0PATH\0`, except renames/copies which carry two paths:
/// `R100\0OLD\0NEW\0`. Only `.rs` paths reach the caller — the pathspec
/// already filters, and this re-checks so a surprising git is still honest.
fn parse_name_status_z(raw: &[u8]) -> Result<Vec<ChangedEntry>> {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0');
    let mut entries = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let kind = status.chars().next().ok_or_else(|| eyre!("empty status in git name-status"))?;
        if kind == 'U' {
            let _ = fields.next();
            continue;
        }
        if kind == 'R' || kind == 'C' {
            let (Some(old), Some(new)) = (fields.next(), fields.next()) else {
                return Err(eyre!("truncated rename record in git name-status"));
            };
            if old.is_empty() || new.is_empty() {
                return Err(eyre!("truncated rename record in git name-status"));
            }
            if is_rust_path(new) {
                entries.push(ChangedEntry {
                    old_path: Some(old.to_owned()),
                    new_path: Some(new.to_owned()),
                });
            }
            continue;
        }
        let Some(path) = fields.next() else {
            return Err(eyre!("truncated record in git name-status"));
        };
        if path.is_empty() {
            return Err(eyre!("truncated record in git name-status"));
        }
        if !is_rust_path(path) {
            continue;
        }
        match kind {
            'A' => entries.push(ChangedEntry { old_path: None, new_path: Some(path.to_owned()) }),
            'D' => entries.push(ChangedEntry { old_path: Some(path.to_owned()), new_path: None }),
            'M' | 'T' => entries.push(ChangedEntry {
                old_path: Some(path.to_owned()),
                new_path: Some(path.to_owned()),
            }),
            _ => {
                return Err(eyre!("unrecognized git status '{status}' for {path}"));
            }
        }
    }
    Ok(entries)
}

/// Returns `true` when a diff path names a Rust source file.
fn is_rust_path(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|ext| ext == "rs")
}

/// Reads a file's base revision from the immutable object store.
///
/// This is the base-pinned half of the comparison: `base` is the merge-base
/// SHA, and `git show` serves bytes the candidate cannot alter.
fn read_base_file(repo_root: &Path, base: &str, path: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["show", &format!("{base}:{path}")])
        .output()
        .map_err(|error| eyre!("failed to run `git show {base}:{path}`: {error}"))?;
    if !output.status.success() {
        return Err(eyre!(
            "cannot read base revision of {path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Collects every added line of the change, across all file types.
///
/// The justification marker may live anywhere the change adds text — a code
/// comment, a changelog entry, a doc note — so this diff is unfiltered, unlike
/// the `.rs`-only change listing.
fn read_added_lines(repo_root: &Path, base: &str) -> Result<String> {
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
    let diff = String::from_utf8_lossy(&output.stdout);
    Ok(diff
        .lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .map(|line| line.strip_prefix('+').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Returns the commit messages between `base` and HEAD.
///
/// Best-effort by design: an unborn HEAD or a `git log` failure yields no
/// messages rather than failing the gate — violations then stand on the
/// added-lines search alone, which is the fail-closed direction.
fn read_commit_messages(repo_root: &Path, base: &str) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(["log", "--format=%B", &format!("{base}..HEAD")])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// Candidate base refs tried, in order, when no explicit base is supplied.
///
/// Same chain as `must_context`: CI-provided scope first, then the main
/// branch, then the previous commit.
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
/// An explicitly requested base that does not resolve is an error; automatic
/// resolution finding nothing is "no subject to evaluate", which [`check`]
/// reports as unevaluated rather than as a violation.
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
/// The scan then compares that commit against the **working tree**, so a test
/// a contributor deleted but not yet committed is in subject. In CI the tree
/// is clean, so the range agrees with `base...HEAD`.
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

/// Resolves a `LazyLock`-compiled regex without panicking on failure.
fn compiled_regex<'a>(
    lock: &'a LazyLock<Result<Regex, regex::Error>>,
    name: &str,
) -> Result<&'a Regex> {
    lock.as_ref().map_err(|error| eyre!("failed to compile the {name} regex: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        DeletionKind, FileCounts, FileSnapshot, count_source, is_test_file_path,
        justification_in_text, parse_name_status_z, scan_snapshots, strip_code,
    };
    use color_eyre::eyre::Result;

    const TWO_TESTS: &str = r#"
#[test]
fn keeps_working() {
    assert_eq!(1 + 1, 2);
}

#[test]
fn also_keeps_working() {
    assert!(true);
}
"#;

    fn modified(path: &str, base: &str, head: &str) -> FileSnapshot {
        FileSnapshot {
            path: path.to_owned(),
            base_content: Some(base.to_owned()),
            head_content: Some(head.to_owned()),
        }
    }

    fn deleted(path: &str, base: &str) -> FileSnapshot {
        FileSnapshot {
            path: path.to_owned(),
            base_content: Some(base.to_owned()),
            head_content: None,
        }
    }

    fn added(path: &str, head: &str) -> FileSnapshot {
        FileSnapshot {
            path: path.to_owned(),
            base_content: None,
            head_content: Some(head.to_owned()),
        }
    }

    #[test]
    fn deleting_a_test_file_is_a_finding_naming_the_file() -> Result<()> {
        let findings = scan_snapshots(&[deleted("crates/example/tests/wiped.rs", TWO_TESTS)])?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "crates/example/tests/wiped.rs");
        assert_eq!(findings[0].kind, DeletionKind::MissingFile { base_tests: 2 });
        Ok(())
    }

    #[test]
    fn deleting_a_test_helper_without_attributes_is_a_finding_by_path() -> Result<()> {
        let helper = "pub fn fixture() -> u32 {\n    7\n}\n";
        let findings = scan_snapshots(&[deleted("crates/example/tests/helper.rs", helper)])?;

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "crates/example/tests/helper.rs");
        Ok(())
    }

    #[test]
    fn deleting_a_production_module_is_not_a_finding() -> Result<()> {
        let module = "pub fn compute(value: u32) -> u32 {\n    value.saturating_add(1)\n}\n";
        let findings = scan_snapshots(&[deleted("crates/example/src/removed.rs", module)])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn removing_a_test_case_is_a_finding_with_delta() -> Result<()> {
        let head = r#"
#[test]
fn keeps_working() {
    assert_eq!(1 + 1, 2);
}
"#;
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", TWO_TESTS, head)])?;

        assert!(
            findings.iter().any(|finding| {
                finding.file == "crates/example/src/lib.rs"
                    && finding.kind == DeletionKind::FewerTests { base: 2, head: 1 }
            }),
            "expected a FewerTests(2 → 1) finding, got: {findings:?}"
        );
        Ok(())
    }

    #[test]
    fn hollowing_assertions_is_a_finding_with_delta() -> Result<()> {
        let head = r#"
#[test]
fn keeps_working() {
}

#[test]
fn also_keeps_working() {
    assert!(true);
}
"#;
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", TWO_TESTS, head)])?;

        assert!(
            findings.iter().any(|finding| {
                finding.file == "crates/example/src/lib.rs"
                    && finding.kind == DeletionKind::FewerAssertions { base: 2, head: 1 }
            }),
            "expected a FewerAssertions(2 → 1) finding, got: {findings:?}"
        );
        // The tests themselves survived, so no FewerTests finding joins it.
        assert!(
            !findings.iter().any(|finding| matches!(finding.kind, DeletionKind::FewerTests { .. })),
            "hollowing assertions must not also report removed tests: {findings:?}"
        );
        Ok(())
    }

    #[test]
    fn commenting_out_a_test_reads_as_removing_it() -> Result<()> {
        let head = r#"
#[test]
fn keeps_working() {
    assert_eq!(1 + 1, 2);
}

// #[test]
// fn also_keeps_working() {
//     assert!(true);
// }
"#;
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", TWO_TESTS, head)])?;

        assert_eq!(findings.len(), 2, "commented-out test and assertion: {findings:?}");
        Ok(())
    }

    #[test]
    fn addition_only_changes_are_clean() -> Result<()> {
        let grown = format!("{TWO_TESTS}\n#[test]\nfn brand_new() {{\n    assert!(true);\n}}\n");
        let findings = scan_snapshots(&[
            modified("crates/example/src/lib.rs", TWO_TESTS, &grown),
            added("crates/example/tests/extra.rs", TWO_TESTS),
        ])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn unchanged_files_and_pure_refactors_are_clean() -> Result<()> {
        let renamed = TWO_TESTS.replace("keeps_working", "still_working");
        let findings = scan_snapshots(&[
            modified("crates/example/src/lib.rs", TWO_TESTS, TWO_TESTS),
            modified("crates/example/src/other.rs", TWO_TESTS, &renamed),
        ])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn removing_a_doc_example_mentioning_assert_is_clean() -> Result<()> {
        let base =
            "/// Example:\n/// ```\n/// assert_eq!(1, 1);\n/// ```\npub fn documented() {}\n";
        let head = "pub fn documented() {}\n";
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", base, head)])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn removing_unwrap_and_todo_counts_is_clean() -> Result<()> {
        // Unwrap/todo removal is progress the sibling ratchets demand; it must
        // never read as hollowing assertions. The head revision drops both
        // without adding any counted signal, so an implementation that counted
        // them would report 3 → 1 here.
        let base = "#[test]\nfn works() {\n    let value = load().unwrap();\n    assert_eq!(value, 7);\n    let _ = todo!();\n}\n";
        let head = "#[test]\nfn works() {\n    let value = load();\n    assert_eq!(value, 7);\n}\n";
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", base, head)])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn expect_to_must_with_migration_keeps_assertion_count_stable() -> Result<()> {
        let base = "#[test]\nfn works() {\n    let value = load().expect(\"the fixture\");\n    assert_eq!(value, 7);\n}\n";
        let head = "#[test]\nfn works() {\n    let value = must_with(load(), \"the fixture\");\n    assert_eq!(value, 7);\n}\n";
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", base, head)])?;

        assert_eq!(findings, vec![]);
        Ok(())
    }

    #[test]
    fn tokio_and_rstest_attributes_count_as_tests() -> Result<()> {
        let base = "#[tokio::test]\nasync fn async_case() {}\n#[rstest]\nfn table_case() {}\n";
        assert_eq!(count_source(base)?.tests, 2);

        let head = "#[tokio::test]\nasync fn async_case() {}\n";
        let findings = scan_snapshots(&[modified("crates/example/src/lib.rs", base, head)])?;
        assert_eq!(
            findings,
            vec![super::DeletionFinding {
                file: "crates/example/src/lib.rs".to_owned(),
                kind: DeletionKind::FewerTests { base: 2, head: 1 },
            }]
        );
        Ok(())
    }

    #[test]
    fn cfg_test_attribute_is_not_a_test_case() -> Result<()> {
        assert_eq!(count_source("#[cfg(test)]\nmod tests {}\n")?, FileCounts::default());
        // `test` inside other attribute arguments never counts either.
        assert_eq!(count_source("#[cfg_attr(test, allow(dead_code))]\nfn f() {}\n")?.tests, 0);
        Ok(())
    }

    #[test]
    fn lookalike_assert_identifiers_do_not_count() -> Result<()> {
        let text = "fn f() {\n    reassert!(true);\n    my_assert_eq!(1, 1);\n}\n";
        assert_eq!(count_source(text)?.assertions, 0);
        Ok(())
    }

    #[test]
    fn string_and_char_bodies_do_not_count() -> Result<()> {
        let text = "fn f() {\n    let attr = \"#[test]\";\n    let slash = '/';\n    assert!(attr.len() > slash.len_utf8());\n}\n";
        assert_eq!(count_source(text)?, FileCounts { tests: 0, assertions: 1 });
        Ok(())
    }

    #[test]
    fn lifetimes_do_not_swallow_code() -> Result<()> {
        // A naive `'` lexer would read `&'a` as opening a char literal and eat
        // everything up to the next quote — hiding the `assert!` below.
        let text = "fn f<'a>(value: &'a str) -> &'a str {\n    assert!(!value.is_empty());\n    value\n}\n";
        assert_eq!(count_source(text)?, FileCounts { tests: 0, assertions: 1 });
        Ok(())
    }

    #[test]
    fn nested_block_comments_do_not_leak_code() -> Result<()> {
        let text = "/* outer /* inner assert!(false); */ still comment */\nfn f() {}\n";
        assert_eq!(count_source(text)?, FileCounts::default());
        Ok(())
    }

    #[test]
    fn raw_strings_of_any_hash_count_do_not_count() -> Result<()> {
        let text = "fn f() {\n    let a = r#\"#[test]\"#;\n    let b = r####\"assert!(false);\"####;\n    assert!(a.len() + b.len() > 0);\n}\n";
        assert_eq!(count_source(text)?, FileCounts { tests: 0, assertions: 1 });
        Ok(())
    }

    #[test]
    fn strip_preserves_newlines_and_token_boundaries() -> Result<()> {
        // Blanking must not fuse tokens across a removed comment: `foo` + `/**/`
        // + `assert!` stays two tokens, never `fooassert!`.
        let stripped = strip_code("foo/**/assert!(true);\n");
        assert!(stripped.contains('\n'));
        assert_eq!(count_source("foo/**/assert!(true);\n")?.assertions, 1);
        Ok(())
    }

    #[test]
    fn justification_marker_requires_a_linked_issue() -> Result<()> {
        assert_eq!(
            justification_in_text("TEST-DELETION-JUSTIFIED(#17405): suite rewritten")?.as_deref(),
            Some("TEST-DELETION-JUSTIFIED(#17405)")
        );
        assert_eq!(justification_in_text("TEST-DELETION-JUSTIFIED: no link")?, None);
        assert_eq!(justification_in_text("test-deletion-justified(#1)")?, None);
        assert_eq!(justification_in_text("ordinary change")?, None);
        Ok(())
    }

    #[test]
    fn test_file_path_shapes_are_recognized() {
        assert!(is_test_file_path("crates/example/tests/integration.rs"));
        assert!(is_test_file_path("crates/example/tests/helpers/fixture.rs"));
        assert!(is_test_file_path("crates/example/src/parser_test.rs"));
        assert!(is_test_file_path("crates/example/src/parser_tests.rs"));
        assert!(is_test_file_path("crates/example/src/tests.rs"));
        assert!(!is_test_file_path("crates/example/src/lib.rs"));
        assert!(!is_test_file_path("crates/example/src/contest.rs"));
        assert!(!is_test_file_path("crates/example/src/latest.rs"));
    }

    #[test]
    fn name_status_parsing_handles_all_record_shapes() -> Result<()> {
        let raw = b"M\0src/kept.rs\0A\0src/added.rs\0D\0src/dropped.rs\0R100\0src/old.rs\0src/new.rs\0T\0src/typed.rs\0U\0src/conflicted.rs\0";
        let entries = parse_name_status_z(raw)?;

        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].new_path.as_deref(), Some("src/kept.rs"));
        assert_eq!(entries[0].old_path.as_deref(), Some("src/kept.rs"));
        assert_eq!(entries[1].old_path, None);
        assert_eq!(entries[2].new_path, None);
        assert_eq!(entries[3].old_path.as_deref(), Some("src/old.rs"));
        assert_eq!(entries[3].new_path.as_deref(), Some("src/new.rs"));
        assert_eq!(entries[4].new_path.as_deref(), Some("src/typed.rs"));
        Ok(())
    }

    #[test]
    fn name_status_parsing_rejects_truncated_and_unknown_records() {
        assert!(parse_name_status_z(b"M\0").is_err());
        assert!(parse_name_status_z(b"R100\0only-old.rs\0").is_err());
        assert!(parse_name_status_z(b"X\0src/weird.rs\0").is_err());
    }
}
