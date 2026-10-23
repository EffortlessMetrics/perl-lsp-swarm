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
    // because it appears first in cargo's report. The header is a whole-log
    // scan rather than a per-line one, so it costs one pass and only runs when
    // no result line named a failure.
    let test_name = lines
        .iter()
        .find_map(|line| cargo_failure::failed_test_name(line).map(str::to_string))
        .or_else(|| {
            cargo_failure::failure_block_spans(output).into_iter().next().map(|block| block.name)
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

    /// The drift this PR removes is a *disagreement between two surfaces*, so
    /// the guard has to put both surfaces in one test. `xtask` is the only
    /// crate that can see both, which is the whole reason this test lives here.
    ///
    /// It runs the real receipt over the same bytes and compares its two
    /// named fields against the gate's. A test that only called the shared
    /// reader would pass even if one consumer stopped using it.
    #[test]
    fn gate_and_receipt_agree_on_the_same_log() -> anyhow::Result<()> {
        let log = std::env::temp_dir().join("xtask-first-failure-agreement.log");
        std::fs::write(&log, DOCTEST_LOG)?;

        let receipt = perl_lsp_ux_tests::regression_receipt::run(
            perl_lsp_ux_tests::regression_receipt::UxRegressionReceiptConfig {
                input: log.clone(),
                receipt: None,
                sha: Some("agreement".to_string()),
                exit_status_file: None,
            },
        )?;
        let receipt_json: serde_json::Value = match &receipt {
            perl_lsp_ux_tests::regression_receipt::UxRegressionReceiptOutput::Payload(text) => {
                serde_json::from_str(text)?
            }
            perl_lsp_ux_tests::regression_receipt::UxRegressionReceiptOutput::Written(path) => {
                serde_json::from_str(&std::fs::read_to_string(path)?)?
            }
        };
        let _ = std::fs::remove_file(&log);

        let failure =
            must_some_with(parse_first_failure(DOCTEST_LOG, 101), "a first failure exists");

        assert_eq!(
            receipt_json["first_failing_test"].as_str(),
            failure.test.as_deref(),
            "the receipt and the gate must name the same failing test"
        );
        let gate_site = failure.site.as_deref().unwrap_or_default();
        let receipt_site = receipt_json["panic_location"].as_str().unwrap_or_default();
        assert_eq!(
            gate_site,
            receipt_site.rsplit_once(':').map(|(head, _)| head).unwrap_or(receipt_site),
            "the gate's file:line must be the receipt's file:line:column, minus the column"
        );
        assert_eq!(gate_site, "src/lib.rs:12", "and neither may be silently absent");
        Ok(())
    }

    /// The `... FAILED` result line is absent here, so the name comes from the
    /// block header. Cargo prints headers in failure order, and this alignment
    /// with that order is a disclosed behaviour change, so it is pinned rather
    /// than left to whichever header the old loop happened to end on.
    #[test]
    fn a_header_only_log_names_the_first_failing_block() {
        let log = "failures:\n\n---- first::test stdout ----\nboom\n---- second::test stdout ----\nboom\n";
        let failure =
            must_some_with(parse_first_failure(log, 101), "a block header names a failing test");
        assert_eq!(
            failure.test.as_deref(),
            Some("first::test"),
            "headers are printed in failure order, so the first is the first failure"
        );
    }

    /// The gate reports whatever `path:line` a panic printed. The receipt has a
    /// stricter path grammar, and applying the receipt's rule in the shared
    /// reader would narrow the gate's evidence — `ci_explain` classifies on
    /// `site.is_some()`, so a refused path turns a code regression into
    /// `unknown`.
    #[test]
    fn the_gate_still_reports_a_path_the_receipt_would_refuse() {
        let failure = must_some_with(
            parse_first_failure("thread 'x' panicked at 9lives/src/lib.rs:42:8:", 101),
            "a panic line names a site",
        );
        assert_eq!(
            failure.site.as_deref(),
            Some("9lives/src/lib.rs:42"),
            "sharing the parse must not narrow what the gate accepts"
        );
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
