use perl_lsp_ux_tests::cargo_failure;

use super::FirstFailure;

/// Parse the first failing test name, panic site, and message from `cargo test` stdout.
///
/// Returns `None` only if the output contains no recognisable failure markers (e.g. a
/// pure compilation error with no test output). All three sub-fields (`test`, `site`,
/// `message`) are individually optional because any one may be absent in edge cases.
///
/// # Patterns detected
///
/// * Test name — `test <path> ... FAILED` or `---- <path> stdout ----`
/// * Panic site — `panicked at '<file>:<line>:<col>:'` (Rust <1.73 style) or
///   `panicked at <file>:<line>:<col>:` (Rust ≥1.73 style)
/// * Message — the first non-empty line that follows the `panicked at` line
///
/// The bytes are read by [`cargo_failure`], which the UX regression receipt also
/// uses, so the two surfaces cannot drift apart on the same cargo output (#16907).
/// What is decided here is the gate's own: the *first* failure, `file:line` for the
/// receipt's `site` field, and the message that follows it.
pub fn parse_first_failure(output: &str, exit_code: i32) -> Option<FirstFailure> {
    let lines: Vec<&str> = output.lines().collect();

    // The `... FAILED` result line wins over the `---- ... stdout ----` header,
    // because it appears first in cargo's report.
    let test_name = lines
        .iter()
        .find_map(|line| cargo_failure::failed_test_name(line).map(str::to_string))
        .or_else(|| {
            lines.iter().find_map(|line| {
                let trimmed = line.trim();
                cargo_failure::failure_block_spans(trimmed)
                    .into_iter()
                    .next()
                    .map(|block| block.name)
            })
        });

    let mut site: Option<String> = None;
    let mut message: Option<String> = None;

    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("panicked at ") {
            continue;
        }
        site = cargo_failure::panic_location(line).map(|location| location.line_only());
        message =
            lines[idx + 1..].iter().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string());
        break;
    }

    if test_name.is_some() || site.is_some() {
        Some(FirstFailure { test: test_name, site, message, exit_code })
    } else {
        None
    }
}

/// Check whether a gate command is a `cargo test`-class command.
///
/// Returns `true` for commands whose first word-token is `cargo` and second is `test`,
/// ignoring leading whitespace and path prefixes. A leading `env` invocation
/// (optionally followed by `-u NAME` unset flags and/or `NAME=VALUE`
/// assignments, per env(1)'s `[flags/assignments] command` grammar) is
/// transparent: the merge-gate `lsp_smoke` gate runs its test binary as
/// `env -u RUSTC_WRAPPER cargo test ...`, and its log is as much a cargo
/// test log as any other.
pub fn is_cargo_test_command(command: &str) -> bool {
    // Gate commands may chain setup steps ahead of the test invocation (for
    // example `cargo build -p perllsp --locked && cargo test ...`). The test
    // output whose failures must be extracted comes from the final segment,
    // so recognition applies to the last `&&`-separated segment.
    let final_segment = command.split("&&").last().unwrap_or("").trim();
    let mut tokens = final_segment.split_whitespace();
    let mut first = tokens.next().unwrap_or("");
    if first == "env" {
        // Skip env(1) arguments until the wrapped command: assignments
        // (`NAME=VALUE`), single-token options, and option+argument pairs
        // such as `-u NAME`. Anything else begins the wrapped command.
        loop {
            let token = tokens.next().unwrap_or("");
            if token.is_empty() {
                return false;
            }
            if token.contains('=') || (token.starts_with('-') && token != "-") {
                if matches!(token, "-u" | "--unset") {
                    tokens.next();
                }
                continue;
            }
            first = token;
            break;
        }
    }
    let is_cargo = first == "cargo" || first.ends_with("/cargo") || first.contains("\\cargo");
    is_cargo && tokens.next().is_some_and(|t| t == "test")
}

#[cfg(test)]
mod tests {
    use super::super::log_reaches_test_execution;
    use color_eyre::eyre::Result;
    use std::fs;

    use super::parse_first_failure;
    use perl_lsp_ux_tests::cargo_failure;
    use perl_tdd_support::must_some_with;

    const CARGO_TEST_COMMAND: &str = "cargo test -p xtask --locked";

    /// A doctest's name is `<file> - <path> (line N)`, so it contains spaces.
    /// Cargo prints that same spaced name on the result line and on the block
    /// header, so the two surfaces must recover the identical string (#16907).
    const DOCTEST_LOG: &str = r#"
running 1 test
test src/lib.rs - item::path (line 12) ... FAILED

failures:

---- src/lib.rs - item::path (line 12) stdout ----
thread 'item::path' panicked at src/lib.rs:12:9:
assertion `left == right` failed

failures:
    src/lib.rs - item::path (line 12)

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;

    const DOCTEST_NAME: &str = "src/lib.rs - item::path (line 12)";

    #[test]
    fn doctest_failure_is_reported_under_its_whole_spaced_name() {
        let failure = must_some_with(
            parse_first_failure(DOCTEST_LOG, 101),
            "a failing doctest must produce a first failure, not a clean run",
        );
        assert_eq!(
            failure.test.as_deref(),
            Some(DOCTEST_NAME),
            "truncating at the first space names a test that did not fail"
        );
        assert_eq!(
            failure.site.as_deref(),
            Some("src/lib.rs:12"),
            "the gate has always reported file:line"
        );
    }

    /// The two surfaces read the same bytes. Before the shared reader they
    /// disagreed here, and a disagreement is the failure mode: a diagnostic that
    /// names one test in the gate summary and a different one in the UX receipt
    /// cannot be acted on.
    #[test]
    fn gate_and_receipt_name_the_same_failing_test() {
        assert_eq!(
            cargo_failure::failed_test_names(DOCTEST_LOG),
            vec![DOCTEST_NAME.to_string()],
            "the shared reader must recover the same name the gate reports"
        );
        let blocks = cargo_failure::failure_block_spans(DOCTEST_LOG);
        assert_eq!(blocks.len(), 1, "the stdout block must be recognised");
        assert_eq!(blocks[0].name, DOCTEST_NAME, "block header and result line are one identity");
        assert!(
            blocks[0].body(DOCTEST_LOG).contains("panicked at src/lib.rs:12:9:"),
            "the block must carry its own panic, so the reader can scope it to this test"
        );
    }

    #[test]
    fn both_surfaces_read_the_same_panic_location() {
        let location = must_some_with(
            cargo_failure::panic_location("thread 'x' panicked at src/lib.rs:12:9:"),
            "the panic line names a location",
        );
        let failure = must_some_with(
            parse_first_failure(DOCTEST_LOG, 101),
            "a first failure exists",
        );
        assert_eq!(
            failure.site.as_deref(),
            Some(location.line_only().as_str()),
            "the gate's file:line is the shared reader's own line, column deliberately dropped"
        );
        assert_eq!(location.with_column(), "src/lib.rs:12:9");
    }

    #[test]
    fn invalid_utf8_line_does_not_hide_a_later_libtest_marker() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let log_path = temp.path().join("gate-invalid-utf8.log");
        let mut with_marker = b"   Compiling xtask v0.17.0\n".to_vec();
        with_marker.extend_from_slice(&[0xff, 0xfe, b'\n']);
        with_marker.extend_from_slice(b"running 2 tests\n");
        fs::write(&log_path, with_marker)?;

        assert_eq!(
            log_reaches_test_execution(CARGO_TEST_COMMAND, &log_path)?,
            Some(true),
            "an invalid UTF-8 line before a later libtest marker must not end the scan"
        );

        let mut compile_only = b"   Compiling xtask v0.17.0\n".to_vec();
        compile_only.extend_from_slice(&[0xff, 0xfe, b'\n']);
        fs::write(&log_path, compile_only)?;
        assert_eq!(
            log_reaches_test_execution(CARGO_TEST_COMMAND, &log_path)?,
            Some(false),
            "invalid UTF-8 without a later marker remains measured compile-only"
        );

        assert!(
            log_reaches_test_execution(CARGO_TEST_COMMAND, &temp.path().join("missing.log"))
                .is_err(),
            "an unreadable log must remain instrumentation-unknown"
        );

        Ok(())
    }
}
