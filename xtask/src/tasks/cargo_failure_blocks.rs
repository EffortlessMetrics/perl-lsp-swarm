//! Shared reader for cargo's failing-test report.
//!
//! Cargo prints one `---- <test name> stdout ----` block per failing test in
//! its trailing `failures:` report. Splitting on that header is what lets each
//! failing test be read from its own evidence instead of from the whole log,
//! where one test's wording silently reclassifies another's (#15988).
//!
//! This module holds only the mechanical read: where the blocks are, what the
//! test is called, and where it panicked. What a block *means* is the caller's
//! judgement — [`super::ux_regression_receipt`] can tell a budget overrun from
//! a real assertion because it knows the UX harness's own markers, and
//! [`super::cargo_failure_digest`] deliberately refuses to guess outside it.
//!
//! One reader, two consumers, on purpose: two parsers of the same cargo output
//! would drift, and a diagnostic surface that disagrees with itself is worse
//! than none.

use std::sync::LazyLock;

use regex::Regex;

#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
// The name capture is non-greedy up to the ` ... FAILED` anchor rather than a
// run of non-whitespace: a doctest's name contains spaces
// (`src/lib.rs - item::path (line 12)`), and a pattern that stopped at the
// first one matched nothing at all for the whole line — silently reporting a
// failing doctest gate as having no failing test.
static FAILED_TEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"test\s+(.+?)\s+\.\.\.\s+FAILED").expect("failed test regex must compile")
});

// Matches both pre-1.73 format ("panicked at 'msg', path:row:col") and
// post-1.73 format ("panicked at path:row:col:") where the location appears
// directly after "panicked at " without a quoted message.
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static PANIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"panicked at (?:'[^']*',\s*)?([a-zA-Z][^:\s][^:]*:\d+:\d+)")
        .expect("panic regex must compile")
});

#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static FAILURE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^-{4}\s+(.+?)\s+stdout\s+-{4}\s*$")
        .expect("failure block header regex must compile")
});

/// Split a cargo log into `(test name, that test's block)` pairs, in the order
/// cargo printed them, keeping the first block when a name repeats.
///
/// Each block ends where the next header begins, or where cargo's run-level
/// trailer starts, so the last failing test does not inherit the summary lines
/// that follow every failure report.
pub fn failure_blocks(raw: &str) -> Vec<(String, &str)> {
    let headers: Vec<(String, usize, usize)> = FAILURE_BLOCK_RE
        .captures_iter(raw)
        .filter_map(|capture| {
            let header = capture.get(0)?;
            let name = capture.get(1)?;
            Some((name.as_str().to_string(), header.end(), header.start()))
        })
        .collect();

    let mut blocks: Vec<(String, &str)> = Vec::new();
    for (index, (name, body_start, _)) in headers.iter().enumerate() {
        if blocks.iter().any(|(seen, _)| seen == name) {
            continue;
        }
        let body_end = headers.get(index + 1).map_or(raw.len(), |(_, _, next_start)| *next_start);
        let block = raw.get(*body_start..body_end).unwrap_or_default();
        blocks.push((name.clone(), trim_run_trailer(block)));
    }
    blocks
}

/// Every test cargo marked `FAILED` on a `test ... ... FAILED` line, in order,
/// without duplicates.
///
/// Cargo can report a failure without printing a stdout block for it, so this
/// is the fallback that keeps such a test from going unnamed.
pub fn failing_test_names(raw: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in raw.lines() {
        let Some(name) = FAILED_TEST_RE.captures(line).and_then(|capture| capture.get(1)) else {
            continue;
        };
        let name = name.as_str().to_string();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The `file:line:col` of the first panic inside one block, if it panicked.
pub fn panic_location(block: &str) -> Option<String> {
    block.lines().find_map(|line| PANIC_RE.captures(line).map(|capture| capture[1].to_string()))
}

/// Trim cargo's run-level trailer off the end of a block.
fn trim_run_trailer(block: &str) -> &str {
    let mut offset = 0usize;
    for line in block.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("failures:") || trimmed.starts_with("test result:") {
            return block.get(..offset).unwrap_or(block);
        }
        offset += line.len();
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;

    const TWO_FAILURES: &str = "\
running 3 tests
test suite::alpha ... FAILED
test suite::beta ... ok
test suite::gamma ... FAILED

failures:

---- suite::alpha stdout ----

thread 'suite::alpha' panicked at crates/thing/src/lib.rs:42:9:
assertion `left == right` failed
  left: 2
 right: 1

---- suite::gamma stdout ----

thread 'suite::gamma' panicked at crates/other/src/run.rs:7:5:
deadline expired after 5s

failures:
    suite::alpha
    suite::gamma

test result: FAILED. 1 passed; 2 failed; 0 ignored
";

    #[test]
    fn each_block_stops_at_the_next_header() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "suite::alpha");
        assert!(blocks[0].1.contains("left: 2"));
        assert!(
            !blocks[0].1.contains("deadline expired"),
            "alpha's block must not reach into gamma's: {:?}",
            blocks[0].1
        );
        Ok(())
    }

    #[test]
    fn the_last_block_stops_before_the_run_trailer() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        let gamma = blocks[1].1;
        assert!(gamma.contains("deadline expired after 5s"));
        assert!(
            !gamma.contains("test result: FAILED"),
            "the last block inherited cargo's run trailer: {gamma:?}"
        );
        assert!(
            !gamma.contains("    suite::alpha"),
            "the last block inherited the failures list: {gamma:?}"
        );
        Ok(())
    }

    #[test]
    fn a_panic_location_is_read_from_its_own_block() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        assert_eq!(panic_location(blocks[0].1).as_deref(), Some("crates/thing/src/lib.rs:42:9"));
        assert_eq!(panic_location(blocks[1].1).as_deref(), Some("crates/other/src/run.rs:7:5"));
        Ok(())
    }

    #[test]
    fn a_block_without_a_panic_reports_no_location() -> Result<()> {
        let blocks = failure_blocks("---- suite::quiet stdout ----\nno panic here\n");

        assert_eq!(panic_location(blocks[0].1), None);
        Ok(())
    }

    #[test]
    fn failed_lines_name_every_failing_test_once() -> Result<()> {
        assert_eq!(
            failing_test_names(TWO_FAILURES),
            vec!["suite::alpha".to_string(), "suite::gamma".to_string()]
        );
        Ok(())
    }

    #[test]
    fn a_log_with_no_failures_yields_nothing() -> Result<()> {
        let clean = "running 2 tests\ntest a ... ok\ntest b ... ok\n\ntest result: ok.\n";

        assert!(failure_blocks(clean).is_empty());
        assert!(failing_test_names(clean).is_empty());
        Ok(())
    }

    /// The pre-1.73 panic format is still what some vendored and older
    /// toolchain output prints, and dropping it would silently lose the
    /// location on exactly those logs.
    #[test]
    fn the_legacy_quoted_panic_format_still_yields_a_location() -> Result<()> {
        let block = "thread 'x' panicked at 'assertion failed: a == b', src/lib.rs:9:1\n";

        assert_eq!(panic_location(block).as_deref(), Some("src/lib.rs:9:1"));
        Ok(())
    }

    /// `doctest_contract_proof` is a required merge gate and cargo names a
    /// doctest with spaces in it. A capture that stopped at the first space
    /// matched the line not at all, so a red doctest gate reported no failing
    /// test — and the digest then said it had not failed on an assertion.
    #[test]
    fn a_doctest_name_with_spaces_is_read_whole() -> Result<()> {
        let raw = concat!(
            "test src/lib.rs - documented (line 2) ... FAILED\n",
            "\nfailures:\n\n",
            "---- src/lib.rs - documented (line 2) stdout ----\n",
            "thread 'main' panicked at src/lib.rs:4:1:\n",
            "assertion failed: false\n",
        );

        assert_eq!(failing_test_names(raw), vec!["src/lib.rs - documented (line 2)"]);
        let blocks = failure_blocks(raw);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "src/lib.rs - documented (line 2)");
        assert_eq!(panic_location(blocks[0].1).as_deref(), Some("src/lib.rs:4:1"));
        Ok(())
    }

    #[test]
    fn a_repeated_block_header_keeps_the_first_occurrence() -> Result<()> {
        let raw = "---- t stdout ----\nfirst\n---- t stdout ----\nsecond\n";

        let blocks = failure_blocks(raw);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].1.contains("first"));
        assert!(!blocks[0].1.contains("second"));
        Ok(())
    }
}
