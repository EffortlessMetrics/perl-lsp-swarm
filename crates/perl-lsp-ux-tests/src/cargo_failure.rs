//! The mechanical read of cargo's failing-test report.
//!
//! Two surfaces in this repository read the same bytes and must not disagree:
//! the UX regression receipt ([`crate::regression_receipt`]) and the merge-gate
//! first-failure reader (`xtask`'s `gates::parse_first_failure`, which reaches
//! this module through xtask's existing `perl-lsp-ux-tests` dependency). A
//! diagnostic that names one test on one surface and a different one on the
//! other is worse than one that names neither, so the *bytes* are read once here
//! and each consumer keeps its own interpretation of the result.
//!
//! What belongs here is only what cargo printed: which tests failed, where each
//! one's stdout block starts and ends, and where it panicked. What a consumer
//! does with that — which failure to report, how to format a location, whether a
//! budget overrun is an assertion failure — stays with the consumer.

use std::sync::LazyLock;

use regex::Regex;

/// One libtest result line: `test <name> ... <outcome>`.
///
/// Anchored to the whole line, with the name taken as everything between `test`
/// and the outcome. Two properties follow, and both are load-bearing:
///
/// * A doctest's name is `<file> - <path> (line N)`, so it contains spaces.
///   Capturing a single whitespace-free token reports a test that did not fail.
/// * Anchoring means prose cannot invent a name. A line such as
///   `note: the test foo ... FAILED` is not a result line, and must not be read
///   as one.
///
/// `[ \t]` rather than `\s` throughout: `\s` matches a newline, which would let
/// one match straddle a blank line and adopt the next line's name.
static RESULT_LINE_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*test[ \t]+(.+?)[ \t]+\.\.\.[ \t]+(ok|FAILED|ignored)\b")
});

/// Cargo's trailing report prints one `---- <name> stdout ----` block per
/// failing test. Splitting on that header is what lets each failing test be
/// read from its own evidence instead of from the whole log, where one test's
/// wording silently reclassifies another's (#15988).
///
/// Anchored and space-tolerant for the same reason as [`RESULT_LINE_RE`]: a
/// doctest prints its full spaced name on the block header too, so a reader
/// that truncates it here disagrees with itself between the result line and the
/// block.
///
/// The trailing `[ \t\r]*` admits a CRLF header. `$` under `(?m)` sits before
/// the `\n`, so a bare `$` would reject every block in a log written on
/// Windows while still reading the result lines — the receipt would then name
/// the failing test and refuse to say anything about it.
static FAILURE_BLOCK_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?m)^-{4}[ \t]+(.+?)[ \t]+stdout[ \t]+-{4}[ \t\r]*$"));

/// The outcome libtest spells after `...` on a result line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    Ignored,
}

/// The name and outcome of one libtest result line, if the line is one.
pub fn result_line(line: &str) -> Option<(&str, TestOutcome)> {
    let captures = RESULT_LINE_RE.as_ref().ok()?.captures(line)?;
    let name = captures.get(1)?.as_str();
    if name.is_empty() {
        return None;
    }
    let outcome = match captures.get(2)?.as_str() {
        "FAILED" => TestOutcome::Failed,
        "ok" => TestOutcome::Passed,
        _ => TestOutcome::Ignored,
    };
    Some((name, outcome))
}

/// The name of a test that reported `FAILED`, if this line is its result line.
pub fn failed_test_name(line: &str) -> Option<&str> {
    match result_line(line) {
        Some((name, TestOutcome::Failed)) => Some(name),
        _ => None,
    }
}

/// Every test that reported `FAILED`, in the order the lines appear.
pub fn failed_test_names(raw: &str) -> Vec<String> {
    raw.lines().filter_map(failed_test_name).map(str::to_string).collect()
}

/// One failing test's stdout block: its name and the range of `raw` its body
/// occupies. The body starts just after the header and ends where the next
/// header begins, or at the end of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureBlock {
    /// Test name exactly as cargo printed it on the header.
    pub name: String,
    /// Byte offset where the block's body starts.
    pub body_start: usize,
    /// Byte offset where the block's body ends.
    pub body_end: usize,
}

impl FailureBlock {
    /// This block's own stdout, or an empty slice if the recorded range no
    /// longer lands on character boundaries.
    ///
    /// The range starts immediately after the header, so the newline that
    /// terminates the header line is the first character here. That is the
    /// pre-existing span contract; a consumer trims its run-level trailer
    /// separately.
    pub fn body<'a>(&self, raw: &'a str) -> &'a str {
        raw.get(self.body_start..self.body_end).unwrap_or_default()
    }
}

/// Whether this module's patterns compiled, named if one did not.
///
/// A reader whose pattern failed to build answers "no failing test" and "no
/// panic" — indistinguishable from a clean run. Consumers that must fail loud
/// rather than report a false clean call this first.
pub fn validate_patterns() -> Result<(), String> {
    for (name, pattern) in
        [("cargo result line", &*RESULT_LINE_RE), ("cargo failure block", &*FAILURE_BLOCK_RE)]
    {
        if let Err(error) = pattern {
            return Err(format!("{name} pattern failed to compile: {error}"));
        }
    }
    Ok(())
}

/// One stdout block per `---- <name> stdout ----` header, in log order.
pub fn failure_block_spans(raw: &str) -> Vec<FailureBlock> {
    let Some(re) = FAILURE_BLOCK_RE.as_ref().ok() else {
        return Vec::new();
    };
    let mut headers: Vec<(String, usize, usize)> = Vec::new();
    for capture in re.captures_iter(raw) {
        let (Some(header), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        headers.push((name.as_str().to_string(), header.end(), header.start()));
    }
    headers
        .iter()
        .enumerate()
        .map(|(index, (name, body_start, _))| FailureBlock {
            name: name.clone(),
            body_start: *body_start,
            body_end: headers
                .get(index + 1)
                .map_or(raw.len(), |(_, _, next_header_start)| *next_header_start),
        })
        .collect()
}

/// Where a test panicked, as cargo printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicLocation {
    /// Path token as printed, with a Windows drive letter's colon rejoined.
    pub path: String,
    pub line: u64,
    /// Absent when the panic printed no column. Consumers decide whether a
    /// column-less location is good enough to report.
    pub column: Option<u64>,
}

impl PanicLocation {
    /// `path:line:column`, the form that carries the column when cargo printed
    /// one.
    pub fn with_column(&self) -> String {
        match self.column {
            Some(column) => format!("{}:{}:{}", self.path, self.line, column),
            None => format!("{}:{}", self.path, self.line),
        }
    }

    /// `path:line`, the form for consumers that report a line only.
    pub fn line_only(&self) -> String {
        format!("{}:{}", self.path, self.line)
    }
}

/// The panic location on one line, if it is a panic line.
///
/// Handles both the pre-1.73 form (`panicked at '<message>', path:row:col`) and
/// the ≥1.73 form (`panicked at path:row:col:`), in which the location follows
/// `panicked at ` with no quoted message.
pub fn panic_location(line: &str) -> Option<PanicLocation> {
    let trimmed = line.trim();
    let panic_at = trimmed.find("panicked at ")? + "panicked at ".len();
    let rest = trimmed[panic_at..].trim_end_matches(':');
    // The ≥1.73 form is tried first, exactly as the gate reader has always done
    // it — except when the payload opens with a quote. A leading `'` is the
    // pre-1.73 form's signature (`panicked at '<message>', path:row:col`), and
    // no ≥1.73 location begins with one. Skipping, not merely attempting, the
    // strict parse there is load-bearing: for a message without a colon of its
    // own, the strict form does not reject the line — it *succeeds*, with the
    // whole `'<message>', path` token as its path — and the pre-1.73 form below
    // never gets its turn. The receipt then refused that path outright and
    // reported no location at all for an ordinary `panic!` (#16907 review). A
    // message whose only colons are Rust's `::` escapes without this guard:
    // those colons land in the strict parse's line field and make it
    // non-numeric, so the fallback fires anyway.
    let strict = if rest.starts_with('\'') { None } else { parse_location(rest) };
    strict.or_else(|| parse_location(after_quoted_message(rest)))
}

fn after_quoted_message(rest: &str) -> &str {
    rest.rfind("', ").map_or("", |at| &rest[at + 3..])
}

fn parse_location(rest: &str) -> Option<PanicLocation> {
    let parts: Vec<&str> = rest.splitn(4, ':').collect();
    if parts.len() < 2 {
        return None;
    }
    // A Windows drive letter is a one-character first segment, rejoined to the
    // path that follows it. Without this, `C:\src\lib.rs:42:5` reads as the
    // path `C`, which no consumer can act on. `parts.len() >= 3` is what
    // distinguishes a drive from a bare one-character name.
    let (path, line_field, line_index) = if parts[0].len() == 1
        && parts[0].chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && parts.len() >= 3
    {
        (format!("{}:{}", parts[0], parts[1]), parts[2], 3)
    } else {
        (parts[0].to_string(), parts[1], 2)
    };
    // Whether `path` is usable as a location is deliberately *not* decided here.
    // The two consumers disagree about it, and that disagreement predates this
    // module: the gate reports whatever `path:line` the panic printed, while the
    // receipt has a documented grammar that rejects a leading non-path character
    // and any whitespace inside the path. Applying the receipt's rule here would
    // silently narrow the gate's evidence — `ci_explain` classifies on
    // `first_failure.site.is_some()`, so a rejected path turns a code regression
    // into `unknown`. Each consumer applies its own rule; see
    // [`is_plausible_path`].
    let line = line_field.parse::<u64>().ok()?;
    let column = parts.get(line_index).and_then(|field| field.parse::<u64>().ok());
    Some(PanicLocation { path, line, column })
}

/// Whether a path is one this repository is willing to report as a location.
///
/// The receipt's rule, exported so it stays the receipt's: a leading letter, `.`
/// or `/`, and no whitespace or inner colon anywhere, so a token like
/// `./ something:100:200` cannot be captured as a location. The gate
/// deliberately does not apply this — see [`panic_location`].
pub fn is_plausible_path(path: &str) -> bool {
    // A drive-qualified path starts at its letter; otherwise the first character
    // admits relative (`crates/...`), dot-relative (`./...`) and absolute
    // (`/...`) forms, so a panic outside the workspace root is still captured.
    let first_ok = matches!(
        path.chars().next(),
        Some('C'..='Z') | Some('a'..='z') | Some('.') | Some('/') | Some('\\')
    );
    if !first_ok || path.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    // Past the drive prefix no further colon may appear, so `./ a:1:2` — a
    // whitespace-bearing "path" — cannot be read as a location.
    let past_prefix = match path.get(1..2) {
        Some(":") => path.get(3..).unwrap_or(""),
        _ => path.get(1..).unwrap_or(""),
    };
    !past_prefix.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_compile() {
        assert!(RESULT_LINE_RE.is_ok());
        assert!(FAILURE_BLOCK_RE.is_ok());
    }

    /// A doctest's name is `<file> - <path> (line N)`. Capturing a single
    /// whitespace-free token reports a test that did not fail (#16907).
    const DOCTEST_NAME: &str = "src/lib.rs - item::path (line 12)";

    #[test]
    fn doctest_result_line_keeps_the_whole_spaced_name() {
        let line = format!("test {DOCTEST_NAME} ... FAILED");
        assert_eq!(failed_test_name(&line), Some(DOCTEST_NAME));
    }

    #[test]
    fn doctest_block_header_keeps_the_whole_spaced_name() {
        let raw = format!("---- {DOCTEST_NAME} stdout ----\nbody\n");
        let blocks = failure_block_spans(&raw);
        assert_eq!(blocks.len(), 1, "the header must be recognised at all");
        assert_eq!(blocks[0].name, DOCTEST_NAME);
        assert_eq!(blocks[0].body(&raw), "\nbody\n");
    }

    #[test]
    fn ordinary_test_names_are_read_unchanged() {
        assert_eq!(failed_test_name("test tasks::a::b ... FAILED"), Some("tasks::a::b"));
        assert_eq!(
            result_line("test tasks::a::b ... ok"),
            Some(("tasks::a::b", TestOutcome::Passed))
        );
        assert_eq!(
            result_line("  test tasks::a::b ... ignored"),
            Some(("tasks::a::b", TestOutcome::Ignored))
        );
    }

    #[test]
    fn prose_cannot_invent_a_test_name() {
        // Not a result line: it does not begin with `test`, so anchoring rejects
        // it even though the old unanchored reader matched the tail of it.
        assert_eq!(failed_test_name("[ux] completion test for module Foo ... FAILED"), None);
        assert_eq!(failed_test_name("note: the test foo ... FAILED"), None);
    }

    #[test]
    fn a_result_line_cannot_adopt_the_next_line_name() {
        // `^` with `\s` would let `[ \t]*`/`[ \t]+` matter; a blank line between
        // a non-result line and a real result line must not merge them.
        let raw = "note: nothing here\n\ntest tasks::a::b ... FAILED\n";
        assert_eq!(failed_test_names(raw), vec!["tasks::a::b".to_string()]);
    }

    #[test]
    fn block_spans_end_at_the_next_header() {
        let raw = "---- one stdout ----\nfirst\n---- two stdout ----\nsecond\n";
        let blocks = failure_block_spans(raw);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name, "one");
        assert_eq!(blocks[0].body(raw), "\nfirst\n");
        assert_eq!(blocks[1].name, "two");
        assert_eq!(blocks[1].body(raw), "\nsecond\n");
    }

    /// A location the test needs, or an error naming the case that failed — the
    /// crate denies `expect`, and a bare `unwrap` would not say which panic form
    /// stopped being read.
    fn location_or(line: &str, case: &str) -> anyhow::Result<PanicLocation> {
        panic_location(line).ok_or_else(|| anyhow::anyhow!("{case}: no location in {line:?}"))
    }

    #[test]
    fn panic_location_reads_the_modern_form_with_its_column() -> anyhow::Result<()> {
        let line = "thread 'x' panicked at crates/a/b.rs:2859:9:";
        let location = location_or(line, "modern panic form")?;
        assert_eq!(location.path, "crates/a/b.rs");
        assert_eq!(location.line, 2859);
        assert_eq!(location.column, Some(9));
        assert_eq!(location.with_column(), "crates/a/b.rs:2859:9");
        assert_eq!(location.line_only(), "crates/a/b.rs:2859");
        Ok(())
    }

    #[test]
    fn panic_location_reads_the_pre_1_73_quoted_form() -> anyhow::Result<()> {
        let line = "thread 'x' panicked at 'assertion failed: x == y', src/module.rs:42:5";
        let location = location_or(line, "pre-1.73 panic form")?;
        assert_eq!(location.path, "src/module.rs");
        assert_eq!(location.line, 42);
        assert_eq!(location.column, Some(5));
        Ok(())
    }

    /// A pre-1.73 message that itself carries no colon must not be swallowed by
    /// the strict ≥1.73 parse. `panicked at 'explicit panic', src/lib.rs:42:5`
    /// hands that parse a numeric line field behind a path-shaped first token,
    /// so it *succeeds* — with the whole `'<message>', path` token as its path —
    /// and the fallback never gets its turn. The receipt's grammar then refuses
    /// the bogus path and the location goes unreported. A message whose only
    /// colons are Rust's `::` escapes by accident: they land in the strict
    /// parse's line field and make it non-numeric, so the fallback fires. Both
    /// are asserted so the ordering cannot regress in either direction
    /// (#16907 review).
    #[test]
    fn a_pre_1_73_message_without_a_colon_is_not_swallowed_by_the_strict_parse()
    -> anyhow::Result<()> {
        let no_colon_at_all = location_or(
            "thread 'main' panicked at 'explicit panic', src/lib.rs:42:5",
            "message with no colon at all",
        )?;
        assert_eq!(no_colon_at_all.path, "src/lib.rs");
        assert_eq!(no_colon_at_all.line, 42);
        assert_eq!(no_colon_at_all.column, Some(5));
        assert!(
            is_plausible_path(&no_colon_at_all.path),
            "the recovered location is one the receipt reports"
        );

        let path_separators_only = location_or(
            "thread 'main' panicked at 'called `Option::unwrap()` on a `None` value', \
             src/lib.rs:42:5",
            "message whose only colons are `::`",
        )?;
        assert_eq!(path_separators_only.path, "src/lib.rs");
        assert_eq!(path_separators_only.line, 42);
        assert_eq!(path_separators_only.column, Some(5));
        Ok(())
    }

    #[test]
    fn panic_location_rejoins_a_windows_drive_letter() -> anyhow::Result<()> {
        let line = r"thread 'x' panicked at C:\src\module.rs:42:5:";
        let location = location_or(line, "drive-qualified path")?;
        assert_eq!(location.path, r"C:\src\module.rs");
        assert_eq!(location.line, 42);
        assert_eq!(location.column, Some(5));
        Ok(())
    }

    /// The reader parses `path:line:column` structurally and refuses to judge
    /// the path: the gate reports whatever was printed, and only the receipt
    /// applies a path grammar. So a whitespace-bearing "path" is still parsed —
    /// what it is *not* is a path the receipt will report.
    #[test]
    fn a_whitespace_bearing_path_is_parsed_but_not_plausible() -> anyhow::Result<()> {
        let location = location_or("panicked at ./ something:100:200", "whitespace path")?;
        assert_eq!(location.path, "./ something");
        assert_eq!(location.line, 100);
        assert_eq!(location.column, Some(200));
        assert!(
            !is_plausible_path(&location.path),
            "the receipt's grammar is the consumer's call, exposed not imposed"
        );
        Ok(())
    }

    #[test]
    fn plausible_paths_are_accepted_and_junk_is_not() {
        for path in ["crates/a/b.rs", "src/lib.rs", "/abs/path.rs", "./rel.rs", r"C:\src\a.rs"] {
            assert!(is_plausible_path(path), "{path:?} is a real path shape");
        }
        for path in ["./ something", "9lives/src/lib.rs", "-/weird.rs", "a b.rs"] {
            assert!(!is_plausible_path(path), "{path:?} is not a path the receipt reports");
        }
    }

    #[test]
    fn panic_location_absent_when_the_line_is_not_a_panic() {
        assert_eq!(panic_location("assertion `left == right` failed"), None);
    }
}
