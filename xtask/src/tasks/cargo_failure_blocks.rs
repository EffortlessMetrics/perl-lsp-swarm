//! Xtask-facing adapters over the shared cargo failing-test reader.
//!
//! The mechanical read — block spans, failing-test names, panic location — lives
//! in [`perl_lsp_ux_tests::cargo_failure`]. This module keeps only what that
//! reader deliberately leaves to the consumer: first-occurrence uniqueness,
//! carriage-return status-line splitting, run-trailer trimming, and the digest's
//! `file:line:col` panic form with the receipt's plausible-path grammar applied.
//!
//! Two parsers of the same cargo output would drift, and a diagnostic surface
//! that disagrees with itself is worse than none (#16907).

use perl_lsp_ux_tests::cargo_failure;

/// Split a cargo log into `(test name, that test's block)` pairs, in the order
/// cargo printed them, keeping the first block when a name repeats.
///
/// Each block ends where the next header begins, or where cargo's run-level
/// trailer starts, so the last failing test does not inherit the summary lines
/// that follow every failure report.
pub fn failure_blocks(raw: &str) -> Vec<(String, &str)> {
    let mut blocks: Vec<(String, &str)> = Vec::new();
    for span in cargo_failure::failure_block_spans(raw) {
        if blocks.iter().any(|(seen, _)| seen == &span.name) {
            continue;
        }
        let body = trim_run_trailer(span.body(raw));
        blocks.push((span.name, body));
    }
    blocks
}

/// Every test cargo marked `FAILED` on a `test ... ... FAILED` line, in order,
/// without duplicates.
///
/// Cargo can report a failure without printing a stdout block for it, so this
/// is the fallback that keeps such a test from going unnamed.
pub fn failing_test_names(raw: &str) -> Vec<String> {
    // `str::lines` (and therefore the shared `failed_test_names`) splits on
    // `\n` only, and CI logs carry bare `\r` where a progress writer rewrote a
    // line in place. Two logical status lines then arrive as one physical line.
    // Normalize first so each status line is its own record.
    let normalized = raw.replace('\r', "\n");
    let mut names: Vec<String> = Vec::new();
    for name in cargo_failure::failed_test_names(&normalized) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The `file:line:col` of the first panic inside one block, if it panicked.
///
/// The parse is [`cargo_failure::panic_location`]; the digest then applies
/// [`cargo_failure::is_plausible_path`] so a whitespace-bearing or
/// first-character-junk token is not published as a location. The gate's
/// `file:line` form does not apply that grammar — see
/// `gates::first_failure`.
pub fn panic_location(block: &str) -> Option<String> {
    block.lines().find_map(|line| {
        cargo_failure::panic_location(line)
            .filter(|location| cargo_failure::is_plausible_path(&location.path))
            .map(|location| location.with_column())
    })
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

    /// Absolute Unix-style paths are what rustc prints when the panicking
    /// frame is not under the workspace root. The first-character class used
    /// to be `[a-zA-Z]`, so these lines never matched and `panic_location`
    /// was silently absent — the missing-evidence bug #16147 names.
    ///
    /// Ported from #16189's UX-receipt copy of this regex.
    #[test]
    fn panic_re_matches_absolute_path_panic() -> Result<()> {
        let line = "thread 'x' panicked at /home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1:";
        let located = panic_location(line);
        ensure!(
            located.as_deref() == Some("/home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1"),
            "an absolute panic location must be captured, got {located:?}"
        );
        Ok(())
    }

    /// `./`-relative paths appear from some toolchain and vendoring
    /// configurations. Like the absolute case, the old regex refused them
    /// because `.` is not a letter.
    #[test]
    fn panic_re_matches_dot_relative_path_panic() -> Result<()> {
        let line = "thread 'x' panicked at ./xtask/src/a.rs:7:1:";
        let located = panic_location(line);
        ensure!(
            located.as_deref() == Some("./xtask/src/a.rs:7:1"),
            "a ./-relative panic location must be captured, got {located:?}"
        );
        Ok(())
    }

    /// The case the digest fails hardest on is a panic inside a dependency,
    /// which is exactly where the reader has the least context to diagnose
    /// from the test name alone. The registry path lives under an absolute
    /// prefix (`/root/.cargo/...`) so the old `[a-zA-Z]` anchor rejected it
    /// on the leading `/`.
    #[test]
    fn panic_re_matches_cargo_registry_panic() -> Result<()> {
        let line = "thread 'x' panicked at /root/.cargo/registry/src/index/foo-1.0/src/lib.rs:3:4:";
        let located = panic_location(line);
        ensure!(
            located.as_deref() == Some("/root/.cargo/registry/src/index/foo-1.0/src/lib.rs:3:4"),
            "a cargo-registry panic location must be captured, got {located:?}"
        );
        Ok(())
    }

    /// The first-character class `[a-zA-Z./]` keeps the old anchor's
    /// narrowness: a leading space, digit, or `-` still cannot start a
    /// captured location. This is the failure mode #16147 warns about —
    /// letting the regex swallow prose after `panicked at`.
    #[test]
    fn panic_re_first_character_anchor_still_excludes_arbitrary_text() -> Result<()> {
        for line in [
            "thread 'x' panicked at  something happened: 100:200",
            "thread 'x' panicked at 9lives/src/lib.rs:42:8:",
            "thread 'x' panicked at -/weird/path.rs:42:8:",
        ] {
            let located = panic_location(line);
            ensure!(located.is_none(), "leading {line:?} must not match, but got {located:?}");
        }
        Ok(())
    }

    /// Path segments are whitespace-free by grammar. The `[^:]*` tail used
    /// to admit inner whitespace and captured `./ something:100:200` as a
    /// bogus location (review finding on #16189); `[^:\s]*` refuses it.
    #[test]
    fn panic_re_whitespace_inside_path_is_not_captured() -> Result<()> {
        for line in [
            "thread 'x' panicked at ./ something:100:200",
            "thread 'x' panicked at ./a b.rs:100:200",
        ] {
            let located = panic_location(line);
            ensure!(
                located.is_none(),
                "whitespace inside {line:?} must not be captured as a path, but got {located:?}"
            );
        }
        let line = "thread 'x' panicked at ./xtask/src/a.rs:100:200:";
        let located = panic_location(line);
        ensure!(
            located.as_deref() == Some("./xtask/src/a.rs:100:200"),
            "a genuine ./-relative location must still match, got {located:?}"
        );
        Ok(())
    }

    /// The drift #16907 names is a disagreement between two surfaces on a
    /// doctest-shaped name. After the adapters share one reader, both entry
    /// points must recover the same name and the same `./`-relative panic.
    #[test]
    fn doctest_name_and_dot_relative_panic_agree_with_the_shared_reader() -> Result<()> {
        let raw = concat!(
            "test src/lib.rs - foo (line 3) ... FAILED\n",
            "\nfailures:\n\n",
            "---- src/lib.rs - foo (line 3) stdout ----\n",
            "thread 'main' panicked at ./src/lib.rs:4:1:\n",
            "assertion failed: false\n",
        );
        let doctest = "src/lib.rs - foo (line 3)";
        let panic_line = "thread 'main' panicked at ./src/lib.rs:4:1:";

        let adapter_names = failing_test_names(raw);
        let shared_names = cargo_failure::failed_test_names(raw);
        ensure!(
            adapter_names == shared_names,
            "adapter names {adapter_names:?} must equal shared names {shared_names:?}"
        );
        ensure!(
            adapter_names == vec![doctest.to_string()],
            "the doctest name must be read whole, got {adapter_names:?}"
        );

        let adapter_blocks = failure_blocks(raw);
        let shared_blocks = cargo_failure::failure_block_spans(raw);
        ensure!(
            adapter_blocks.len() == 1 && shared_blocks.len() == 1,
            "one failure block on both surfaces, adapter {} shared {}",
            adapter_blocks.len(),
            shared_blocks.len()
        );
        let adapter_block = at(&adapter_blocks, 0, "failure_blocks")?;
        let shared_block = at(&shared_blocks, 0, "failure_block_spans")?;
        ensure!(
            adapter_block.0 == shared_block.name,
            "adapter block name {:?} must equal shared block name {:?}",
            adapter_block.0,
            shared_block.name
        );
        ensure!(
            adapter_block.0 == doctest,
            "the block carries the whole doctest name, got {:?}",
            adapter_block.0
        );

        let adapter_panic = panic_location(adapter_block.1);
        let shared_panic =
            cargo_failure::panic_location(panic_line).map(|location| location.with_column());
        ensure!(
            adapter_panic == shared_panic,
            "adapter panic {adapter_panic:?} must equal shared panic {shared_panic:?}"
        );
        ensure!(
            adapter_panic.as_deref() == Some("./src/lib.rs:4:1"),
            "a ./-relative panic location must be captured, got {adapter_panic:?}"
        );
        Ok(())
    }
}
