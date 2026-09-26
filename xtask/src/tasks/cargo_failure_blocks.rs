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
//
// Widening the capture makes anchoring mandatory. libtest prints a status line
// and nothing else on it, but a log also carries test stdout and tool prose,
// and an unanchored `(.+?)` turns any line mentioning `test` before a
// ` ... FAILED` into a name: `[ux] completion test for module Foo ... FAILED`
// yielded `for module Foo`. That is worse than yielding nothing, because the
// caller renders it into a paste-ready `cargo test <filter>` command, and a
// filter matching no test exits 0 — the reader pastes it, sees green, and
// concludes the CI failure does not reproduce. Anchored to a whole line, the
// name must be the only thing between `test` and the marker.
static FAILED_TEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*test\s+([^\r\n]+?)\s+\.\.\.\s+FAILED\s*$")
        .expect("failed test regex must compile")
});

// Matches both pre-1.73 format ("panicked at 'msg', path:row:col") and
// post-1.73 format ("panicked at path:row:col:") where the location appears
// directly after "panicked at " without a quoted message. The first character
// class accepts a letter (relative paths like `crates/...`), `.` (`./`-relative
// paths), or `/` (absolute paths) so panics whose frame is outside the
// workspace root — a dependency's own `unwrap`, a `registry/src/...` frame, or
// any build whose `CARGO_MANIFEST_DIR` is not a prefix of the compiled file —
// are still captured (#16147). The `[^:\s]` segments forbid whitespace and
// inner `:` across the whole path, so a token like `./ something:100:200` —
// whitespace inside the "path" — cannot be captured as a location (review
// finding on #16189, FC-WHITESPACE-PATH-GRAMMAR).
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
pub(crate) static PANIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"panicked at (?:'[^']*',\s*)?([a-zA-Z./][^:\s][^:\s]*:\d+:\d+)")
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
    // `str::lines` splits on `\n` only, and CI logs carry bare `\r` where a
    // progress writer rewrote a line in place. Two logical status lines then
    // arrive as one physical line, and an anchored pattern sees neither. Split
    // on the carriage return too, so each status line is anchored on its own.
    for line in raw.lines().flat_map(|line| line.split('\r')) {
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
    use color_eyre::eyre::{Result, ensure, eyre};

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

    /// Fallible element access. `xs[i]` panics, which the fallible-test policy
    /// excludes; this carries the same proposition as an error naming the
    /// observed length, so a shape regression reports itself instead of
    /// unwinding.
    fn at<'a, T>(xs: &'a [T], index: usize, what: &str) -> Result<&'a T> {
        xs.get(index)
            .ok_or_else(|| eyre!("{what}: wanted element {index}, but only {} exist", xs.len()))
    }

    #[test]
    fn each_block_stops_at_the_next_header() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        ensure!(blocks.len() == 2, "both failure blocks must be found, got {}", blocks.len());
        let alpha = at(&blocks, 0, "failure_blocks")?;
        ensure!(alpha.0 == "suite::alpha", "the first block's own name, got {:?}", alpha.0);
        ensure!(
            alpha.1.contains("left: 2"),
            "alpha's own assertion text is missing: {:?}",
            alpha.1
        );
        ensure!(
            !alpha.1.contains("deadline expired"),
            "alpha's block must not reach into gamma's: {:?}",
            alpha.1
        );
        Ok(())
    }

    #[test]
    fn the_last_block_stops_before_the_run_trailer() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        let gamma = at(&blocks, 1, "failure_blocks")?.1;
        ensure!(
            gamma.contains("deadline expired after 5s"),
            "gamma's own failure text is missing: {gamma:?}"
        );
        ensure!(
            !gamma.contains("test result: FAILED"),
            "the last block inherited cargo's run trailer: {gamma:?}"
        );
        ensure!(
            !gamma.contains("    suite::alpha"),
            "the last block inherited the failures list: {gamma:?}"
        );
        Ok(())
    }

    #[test]
    fn a_panic_location_is_read_from_its_own_block() -> Result<()> {
        let blocks = failure_blocks(TWO_FAILURES);

        let first = panic_location(at(&blocks, 0, "failure_blocks")?.1);
        let second = panic_location(at(&blocks, 1, "failure_blocks")?.1);
        ensure!(
            first.as_deref() == Some("crates/thing/src/lib.rs:42:9"),
            "the first block's own panic location, got {first:?}"
        );
        ensure!(
            second.as_deref() == Some("crates/other/src/run.rs:7:5"),
            "the second block's own panic location, got {second:?}"
        );
        Ok(())
    }

    #[test]
    fn a_block_without_a_panic_reports_no_location() -> Result<()> {
        let blocks = failure_blocks("---- suite::quiet stdout ----\nno panic here\n");

        let located = panic_location(at(&blocks, 0, "failure_blocks")?.1);
        ensure!(
            located.is_none(),
            "a block with no panic must report no location, got {located:?}"
        );
        Ok(())
    }

    #[test]
    fn failed_lines_name_every_failing_test_once() -> Result<()> {
        let names = failing_test_names(TWO_FAILURES);

        ensure!(
            names == vec!["suite::alpha".to_string(), "suite::gamma".to_string()],
            "every failing test exactly once, in log order, got {names:?}"
        );
        Ok(())
    }

    #[test]
    fn a_log_with_no_failures_yields_nothing() -> Result<()> {
        let clean = "running 2 tests\ntest a ... ok\ntest b ... ok\n\ntest result: ok.\n";

        ensure!(
            failure_blocks(clean).is_empty(),
            "a clean log has no failure blocks, got {:?}",
            failure_blocks(clean)
        );
        ensure!(
            failing_test_names(clean).is_empty(),
            "a clean log names no failing test, got {:?}",
            failing_test_names(clean)
        );
        Ok(())
    }

    /// The pre-1.73 panic format is still what some vendored and older
    /// toolchain output prints, and dropping it would silently lose the
    /// location on exactly those logs.
    #[test]
    fn the_legacy_quoted_panic_format_still_yields_a_location() -> Result<()> {
        let block = "thread 'x' panicked at 'assertion failed: a == b', src/lib.rs:9:1\n";

        let located = panic_location(block);
        ensure!(
            located.as_deref() == Some("src/lib.rs:9:1"),
            "the pre-1.73 format must still yield a location, got {located:?}"
        );
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

        let names = failing_test_names(raw);
        ensure!(
            names == vec!["src/lib.rs - documented (line 2)"],
            "the doctest name must be read whole, got {names:?}"
        );
        let blocks = failure_blocks(raw);
        ensure!(blocks.len() == 1, "one failure block, got {}", blocks.len());
        let block = at(&blocks, 0, "failure_blocks")?;
        ensure!(
            block.0 == "src/lib.rs - documented (line 2)",
            "the block carries the whole name too, got {:?}",
            block.0
        );
        let located = panic_location(block.1);
        ensure!(
            located.as_deref() == Some("src/lib.rs:4:1"),
            "the doctest's panic location, got {located:?}"
        );
        Ok(())
    }

    #[test]
    fn a_repeated_block_header_keeps_the_first_occurrence() -> Result<()> {
        let raw = "---- t stdout ----\nfirst\n---- t stdout ----\nsecond\n";

        let blocks = failure_blocks(raw);

        ensure!(
            blocks.len() == 1,
            "a repeated header must not open a second block, got {}",
            blocks.len()
        );
        let block = at(&blocks, 0, "failure_blocks")?;
        ensure!(block.1.contains("first"), "the first occurrence must be kept: {:?}", block.1);
        ensure!(!block.1.contains("second"), "the later occurrence must not be: {:?}", block.1);
        Ok(())
    }

    /// A widened name capture has to be anchored to a whole status line, or
    /// prose mentioning `test` before a ` ... FAILED` becomes a test name.
    ///
    /// This is not a cosmetic nit: the caller renders the name into a
    /// paste-ready `cargo test <filter>` command, and a filter matching no
    /// test exits 0. A reader who pastes it sees green and concludes the CI
    /// failure does not reproduce locally, which is strictly worse than being
    /// told no test was named.
    #[test]
    fn prose_mentioning_a_test_is_not_a_failing_test_name() -> Result<()> {
        for line in [
            "[ux] completion test for module Foo::Bar ... FAILED",
            "  latest test of the hover path ... FAILED",
            "note: the test crates/x/README.md - usage (line 4) ... FAILED",
            "warning: a doctest src/lib.rs - f (line 9) ... FAILED",
        ] {
            ensure!(
                failing_test_names(line).is_empty(),
                "prose was read as a failing test name: {line:?} yielded {:?}",
                failing_test_names(line)
            );
        }
        Ok(())
    }

    /// The doctest win the widened capture bought must survive the anchoring.
    #[test]
    fn an_anchored_capture_still_takes_a_whole_doctest_name() -> Result<()> {
        let doctest = failing_test_names("test src/lib.rs - item::path (line 12) ... FAILED");
        ensure!(
            doctest == vec!["src/lib.rs - item::path (line 12)".to_string()],
            "an anchored capture must still take the whole doctest name, got {doctest:?}"
        );
        let plain = failing_test_names("test suite::case ... FAILED");
        ensure!(
            plain == vec!["suite::case".to_string()],
            "an ordinary status line must still yield its name, got {plain:?}"
        );
        Ok(())
    }

    /// A progress writer rewriting a line in place leaves a bare carriage
    /// return, which `str::lines` does not split on. Two logical status lines
    /// then arrive as one physical line; anchoring alone would read the whole
    /// thing as a name, or miss it entirely.
    #[test]
    fn a_bare_carriage_return_separates_two_status_lines() -> Result<()> {
        let names = failing_test_names("test a::b ... ok\rtest c::d ... FAILED");
        ensure!(
            names == vec!["c::d".to_string()],
            "a bare carriage return must separate the two status lines, got {names:?}"
        );
        Ok(())
    }
}
