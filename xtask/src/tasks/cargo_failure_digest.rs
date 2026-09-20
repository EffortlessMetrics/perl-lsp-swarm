//! Turn a failing gate's captured cargo output into a step-summary digest.
//!
//! # Why this exists
//!
//! `ci.yml`'s `Gate summary` step renders one row per gate: name, result,
//! receipt status, exit code, duration. That tells a reader *which* gate went
//! red and nothing about *why*. The failing test name, the panic location and
//! the assertion text are printed only into the job log, and reading the job
//! log is not reliably cheap: the full-log archive redirects to blob storage
//! that several tooling paths cannot reach, `get_check_run` returns an empty
//! `output` for Actions jobs, and the per-job logs API returns only a bounded
//! tail — a noisy trailing step can push the test output out of reach
//! entirely.
//!
//! When reading a failure costs a re-run, suppressing the failure wins on
//! time. That is the behaviour this module is meant to remove the incentive
//! for.
//!
//! `ux-regression-gate.yml` already solves this for one gate, writing the
//! first failing test, the panic location and the repro command straight into
//! `$GITHUB_STEP_SUMMARY`. This module generalises the parse so every gate in
//! a merge-gate shard gets the same treatment, reading the per-gate logs the
//! gate runner already writes to `target/receipts/logs/<gate>.log`.
//!
//! # What it deliberately does not do
//!
//! It classifies nothing. [`super::ux_regression_receipt`] decides whether a
//! UX failure was a budget overrun or a real assertion, because it can read
//! the UX harness's own documented markers. Outside that harness there is no
//! such evidence, so this module reports what cargo printed and stops. A
//! digest that guessed a cause would be worse than the log it replaces.
//!
//! It changes no verdict. The digest is rendered from an already-decided
//! shard summary; it cannot make a gate pass or fail.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result};
use serde_json::Value;

use crate::tasks::cargo_failure_blocks::{failing_test_names, failure_blocks, panic_location};

/// Longest assertion excerpt carried into the summary for one test.
///
/// A step summary is a fixed budget (GitHub truncates at 1 MiB) shared with
/// every other step in the job, and a reader scanning it wants the first few
/// lines that name the mismatch, not a scrollback. The full block stays in the
/// log and in the uploaded receipt artifact.
const ASSERTION_EXCERPT_LINES: usize = 6;

/// How many failing tests are named per gate before the list is elided.
const MAX_TESTS_PER_GATE: usize = 10;

#[derive(Debug, Clone)]
pub struct GateFailureDigestConfig {
    /// Shard summary written by `scripts/ci/run_gate_shard.py`.
    pub summary: PathBuf,
    /// Directory holding `<gate>.log` for each executed gate.
    pub logs: PathBuf,
    /// Markdown destination, appended to when it already exists.
    pub out: PathBuf,
    /// Also print the digest to stdout.
    pub print: bool,
}

/// One failing test as cargo reported it, with no interpretation added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailingTest {
    pub name: String,
    /// `file:line:col` from the test's own block, when it panicked.
    pub panic_location: Option<String>,
    /// First few lines of the block, verbatim.
    pub excerpt: Vec<String>,
}

/// One non-success gate and everything a reader needs to act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateFailure {
    pub gate_name: String,
    pub result: String,
    pub exit_code: Option<i64>,
    /// The exact command, already recorded by the shard runner.
    pub reproduce: Option<String>,
    /// The runner's own note, e.g. why a gate never started.
    pub message: Option<String>,
    pub failing_tests: Vec<FailingTest>,
    /// Set when the gate went red but its log could not be read, so the
    /// absence of named tests is reported as missing evidence rather than as
    /// "no test failed".
    pub log_unavailable: Option<String>,
}

pub fn run(config: GateFailureDigestConfig) -> Result<()> {
    let raw = fs::read_to_string(&config.summary)
        .with_context(|| format!("reading shard summary {}", config.summary.display()))?;
    let summary: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parsing shard summary {}", config.summary.display()))?;

    let failures = collect_failures(&summary, &config.logs);
    let markdown = render(&summary, &failures);

    if let Some(parent) = config.out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    // Written fresh, not appended: `target/` is restored from a shared cache
    // (#12085), so a digest left by an earlier run on an unrelated SHA would
    // otherwise be pasted into this run's step summary ahead of its own.
    fs::write(&config.out, &markdown)
        .with_context(|| format!("writing {}", config.out.display()))?;
    if config.print {
        print!("{markdown}");
    }
    Ok(())
}

/// Read every non-success gate out of a receipt and attach its log.
///
/// Two producers write gate results in this repository and neither shape is
/// going away, so the digest reads both rather than covering the shard lane
/// and leaving the fast-feedback lane with no summary at all:
///
/// - `scripts/ci/run_gate_shard.py` writes a shard summary whose rows carry
///   `result` and `reproduce`;
/// - `xtask gates --receipt` writes a receipt whose rows carry `status`,
///   `command`, `log_path` and an already-extracted `first_failure`.
///
/// A row counts as non-success unless it says `success`, `passed` or
/// `skipped`. Anything else — `failure`, `timeout`, `instrument_failure`,
/// `not_proven`, `cancelled` — is something a reader needs explained, and
/// treating an unrecognised status as success would hide exactly the states
/// that are hardest to diagnose.
fn collect_failures(summary: &Value, logs: &Path) -> Vec<GateFailure> {
    let Some(gates) = gate_rows(summary) else {
        return Vec::new();
    };
    // A gate that never ran is not a failure to explain — it has no log, and
    // rendering one produces a confident "the log could not be read" paragraph
    // about a gate that was correctly skipped. It is counted separately by
    // `render` instead, which is where it belongs: in the claim, not in the
    // diagnosis.
    gates
        .iter()
        .filter(|gate| !is_success(gate) && !is_not_proven(gate))
        .map(|gate| gate_failure(gate, logs))
        .collect()
}

/// The `gates` array, or `None` when the document did not carry one.
///
/// The distinction is the whole point: an empty failure list because every
/// gate passed and an empty failure list because the shape was never read are
/// the same value, and only one of them justifies saying so. `render` asks
/// this before it claims anything. The other producer of this file guards the
/// same way (`scripts/ci/run_gate_shard.py` refuses a document whose `gates`
/// is not a list), so the consumer matching it is the contract, not caution.
fn gate_rows(summary: &Value) -> Option<&Vec<Value>> {
    summary.get("gates").and_then(Value::as_array)
}

fn is_success(gate: &Value) -> bool {
    matches!(gate_status(gate).as_str(), "success" | "passed" | "pass")
}

/// A gate that did not run: short-circuited behind a failed required gate, or
/// quarantined by policy.
///
/// `gates.rs` writes `status: "skip"` for both, with `output_summary: "not
/// run: short-circuited by failed required gate '<name>'"` for the first. A
/// row that never executed proves nothing, so counting it as a success lets a
/// digest report that nineteen of twenty gates passed when sixteen of them
/// never started — the one claim a reader of a red job must not be given.
fn is_not_proven(gate: &Value) -> bool {
    matches!(gate_status(gate).as_str(), "skipped" | "skip")
}

fn gate_status(gate: &Value) -> String {
    gate.get("result")
        .or_else(|| gate.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn gate_failure(gate: &Value, logs: &Path) -> GateFailure {
    let gate_name = gate.get("gate_name").and_then(Value::as_str).unwrap_or_default().to_string();
    let (failing_tests, log_unavailable) = match read_gate_log(logs, gate, &gate_name) {
        Ok(text) => (failing_tests_from_log(&text), None),
        Err(reason) => (first_failure_fallback(gate), Some(reason)),
    };
    GateFailure {
        gate_name,
        result: gate
            .get("result")
            .or_else(|| gate.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        exit_code: gate.get("exit_code").and_then(Value::as_i64),
        // `reproduce` is the shard summary's pre-joined command; `command` is
        // the receipt's.
        reproduce: string_field(gate, "reproduce").or_else(|| string_field(gate, "command")),
        // `message` is the shard summary's; `output_summary` is the receipt's,
        // and it is where `xtask gates` puts an execution error — the status
        // whose log most often does not exist, so dropping it left the reader
        // with nothing at all.
        message: string_field(gate, "message").or_else(|| string_field(gate, "output_summary")),
        failing_tests,
        log_unavailable,
    }
}

/// When the log is gone, the receipt's own `first_failure` is still evidence
/// the runner observed directly, so use it rather than reporting nothing.
fn first_failure_fallback(gate: &Value) -> Vec<FailingTest> {
    let Some(first) = gate.get("first_failure") else {
        return Vec::new();
    };
    let Some(name) = first.get("test").and_then(Value::as_str) else {
        return Vec::new();
    };
    vec![FailingTest {
        name: name.to_string(),
        panic_location: first.get("site").and_then(Value::as_str).map(str::to_owned),
        excerpt: first
            .get("message")
            .and_then(Value::as_str)
            .map(|message| vec![message.to_string()])
            .unwrap_or_default(),
    }]
}

fn string_field(gate: &Value, key: &str) -> Option<String> {
    gate.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

/// A gate that never ran has no log, which is not the same as an unreadable
/// one; both are reported as text rather than silently yielding no tests.
///
/// The receipt's own `log_path` wins when present, because it records where
/// the runner actually wrote, and only falls back to the `<gate>.log`
/// convention otherwise.
fn read_gate_log(logs: &Path, gate: &Value, gate_name: &str) -> Result<String, String> {
    let path = match gate.get("log_path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        // Receipt log paths are relative to the receipt directory, which is
        // the parent of the logs directory (`target/receipts/logs/x.log` is
        // recorded as `logs/x.log`).
        Some(relative) => logs.parent().unwrap_or(logs).join(relative),
        None if gate_name.is_empty() => {
            return Err("the receipt named neither a gate nor a log path".to_string());
        }
        None => logs.join(format!("{gate_name}.log")),
    };
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(error) => Err(format!("{} could not be read: {error}", path.display())),
    }
}

/// Name every failing test in one gate's log, with its panic location and the
/// opening lines of its own block.
///
/// Reading each test's excerpt from its own `---- <name> stdout ----` block,
/// rather than from the whole log, is what stops one test's panic being
/// reported against another's name when several fail in the same run.
pub fn failing_tests_from_log(raw: &str) -> Vec<FailingTest> {
    let mut tests: Vec<FailingTest> = failure_blocks(raw)
        .into_iter()
        .map(|(name, block)| FailingTest {
            name,
            panic_location: panic_location(block),
            excerpt: excerpt(block),
        })
        .collect();

    // Cargo can report a test as FAILED without printing a stdout block for
    // it. Name those too: a reader who knows which test to run is already
    // most of the way there, and an empty list would read as "nothing failed".
    for name in failing_test_names(raw) {
        if !tests.iter().any(|test| test.name == name) {
            tests.push(FailingTest { name, panic_location: None, excerpt: Vec::new() });
        }
    }
    tests
}

/// The opening lines of a block, stopping before cargo's backtrace.
///
/// A backtrace is the longest and least useful part of a failure block for a
/// reader deciding what to do next: it names runtime internals, not the
/// assertion. Keeping it would spend the step-summary budget on the part that
/// is already in the log.
fn excerpt(block: &str) -> Vec<String> {
    block
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take_while(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("stack backtrace:") && !trimmed.starts_with("note: run with")
        })
        .take(ASSERTION_EXCERPT_LINES)
        .map(str::to_owned)
        .collect()
}

/// The head this receipt describes, across both producers' shapes.
///
/// `run_gate_shard.py` writes `subject_sha` at the top level; `xtask gates`
/// has no such field and records the head under `metadata.git_sha_short` /
/// `metadata.git_sha`. Reading only the first left every PR-fast digest
/// headed "Subject `unknown`" -- a digest that exists to make a failure
/// legible must not mislabel which commit failed.
fn subject_of(summary: &Value) -> &str {
    summary
        .get("subject_sha")
        .and_then(Value::as_str)
        .or_else(|| summary.pointer("/metadata/git_sha_short").and_then(Value::as_str))
        .or_else(|| summary.pointer("/metadata/git_sha").and_then(Value::as_str))
        .unwrap_or("unknown")
}

/// How many gates this receipt covers, across both producers' shapes.
///
/// `selected_gates` is the shard summary's; `xtask gates` carries the count at
/// `summary.total_gates`. Falling back to the `gates` array keeps the number
/// honest if neither header is present. Reading only the first made a clean
/// PR-fast run report that all **zero** selected gates succeeded.
fn gate_count(summary: &Value) -> usize {
    summary
        .get("selected_gates")
        .and_then(Value::as_array)
        .map(Vec::len)
        .or_else(|| {
            summary.pointer("/summary/total_gates").and_then(Value::as_u64).map(|n| n as usize)
        })
        .or_else(|| summary.get("gates").and_then(Value::as_array).map(Vec::len))
        .unwrap_or_default()
}

fn render(summary: &Value, failures: &[GateFailure]) -> String {
    let shard = gate_count(summary);
    let subject = subject_of(summary);

    let mut out = String::new();
    out.push_str("\n### Why the gate failed\n\n");

    // No readable `gates` array is not "nothing failed". Saying so under this
    // heading, on a job that is red, is the one output worse than the log this
    // digest replaces.
    let Some(rows) = gate_rows(summary) else {
        let _ = writeln!(
            out,
            "The shard summary for `{subject}` carried no readable `gates` array, so this \
             digest cannot say which gate failed or whether any did. That is missing \
             evidence, not a clean run — read the job log."
        );
        return out;
    };

    let not_proven = rows.iter().filter(|gate| is_not_proven(gate)).count();
    if failures.is_empty() {
        let succeeded = rows.iter().filter(|gate| is_success(gate)).count();
        if not_proven == 0 {
            let _ = writeln!(
                out,
                "Every one of the {shard} selected gate(s) succeeded on `{subject}`."
            );
        } else {
            let _ = writeln!(
                out,
                "No gate failed on `{subject}`, but only {succeeded} of {shard} selected \
                 gate(s) actually ran: {not_proven} were skipped (short-circuited behind a \
                 failed required gate, or quarantined by policy) and prove nothing."
            );
        }
        return out;
    }

    let skipped_note = if not_proven == 0 {
        String::new()
    } else {
        format!(
            " A further {not_proven} gate(s) never ran — short-circuited behind a failed \
             required gate, or quarantined — and are not evidence either way."
        )
    };
    let _ = writeln!(
        out,
        "Subject `{subject}`, {} non-success gate(s).{skipped_note}\n",
        failures.len()
    );

    for failure in failures {
        let exit = failure.exit_code.map(|code| format!(", exit {code}")).unwrap_or_default();
        let _ = writeln!(out, "#### `{}` — {}{}", failure.gate_name, failure.result, exit);
        out.push('\n');

        if let Some(message) = &failure.message {
            let _ = writeln!(out, "{message}\n");
        }

        if let Some(reason) = &failure.log_unavailable {
            let _ = writeln!(
                out,
                "The gate's log could not be read: {reason}. \
                 That is missing evidence, not a clean run.\n"
            );
            if failure.failing_tests.is_empty() {
                let _ = writeln!(out, "No test output is available for this gate.\n");
            } else {
                // The receipt's own `first_failure` outlived the log. Say
                // where it came from, so a reader does not mistake one
                // recovered test for the whole failure set.
                let _ = writeln!(
                    out,
                    "Recovered from the receipt rather than the log, so this may not be \
                     the whole failure set:\n"
                );
                render_failing_tests(&mut out, failure);
            }
        } else if failure.failing_tests.is_empty() {
            let _ = writeln!(
                out,
                "The gate's log names no failing test, so it did not fail on an \
                 assertion. Read the command's own output in the job log.\n"
            );
        } else {
            let _ = writeln!(out, "Failing test(s):\n");
            render_failing_tests(&mut out, failure);
        }

        if let Some(reproduce) = &failure.reproduce {
            let _ = writeln!(out, "Reproduce:\n\n```bash\n{reproduce}\n```\n");
        }
    }
    out
}

/// The longest run of consecutive backticks anywhere in the excerpt, so the
/// fence around it can be made longer than any run it contains.
fn longest_backtick_run(lines: &[String]) -> usize {
    let mut longest = 0usize;
    for line in lines {
        let mut run = 0usize;
        for ch in line.chars() {
            if ch == '`' {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
    }
    longest
}

fn render_failing_tests(out: &mut String, failure: &GateFailure) {
    for test in failure.failing_tests.iter().take(MAX_TESTS_PER_GATE) {
        let _ = writeln!(out, "- `{}`", test.name);
        if let Some(location) = &test.panic_location {
            let _ = writeln!(out, "  - panicked at `{location}`");
        }
        if !test.excerpt.is_empty() {
            // An assertion over markdown can itself contain a fence, which
            // would close this block early and corrupt every section appended
            // after it. Open with a longer one than anything in the excerpt.
            let fence = "`".repeat(longest_backtick_run(&test.excerpt).max(2) + 1);
            let _ = writeln!(out, "  {fence}text");
            for line in &test.excerpt {
                let _ = writeln!(out, "  {line}");
            }
            let _ = writeln!(out, "  {fence}");
        }
    }
    let shown = failure.failing_tests.len().min(MAX_TESTS_PER_GATE);
    if failure.failing_tests.len() > shown {
        let _ = writeln!(
            out,
            "\n…and {} more; the full list is in the job log and the uploaded receipt.",
            failure.failing_tests.len() - shown
        );
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;
    use serde_json::json;

    const ASSERTION_LOG: &str = "\
running 2 tests
test parser::ranges::byte_offsets_round_trip ... FAILED

failures:

---- parser::ranges::byte_offsets_round_trip stdout ----

thread 'parser::ranges::byte_offsets_round_trip' panicked at crates/perl-parser-core/src/ranges.rs:118:9:
assertion `left == right` failed: utf-16 offset drifted
  left: 14
 right: 13

failures:
    parser::ranges::byte_offsets_round_trip

test result: FAILED. 1 passed; 1 failed; 0 ignored
";

    fn summary_with(gate: Value) -> Value {
        json!({
            "subject_sha": "abc1234",
            "selected_gates": ["fmt_gate", "parser_gate"],
            "gates": [
                {
                    "gate_name": "fmt_gate",
                    "result": "success",
                    "exit_code": 0,
                    "reproduce": "cargo fmt --all -- --check",
                },
                gate,
            ],
        })
    }

    fn write_log(dir: &Path, gate: &str, body: &str) -> Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(dir.join(format!("{gate}.log")), body)?;
        Ok(())
    }

    /// The whole point: a reader learns the test, the file and line, and the
    /// command, without opening the job log.
    #[test]
    fn a_failing_assertion_names_its_test_location_and_repro() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(temp.path(), "parser_gate", ASSERTION_LOG)?;
        let summary = summary_with(json!({
            "gate_name": "parser_gate",
            "result": "failure",
            "exit_code": 101,
            "reproduce": "cargo test -p perl-parser-core --locked --lib",
        }));

        let failures = collect_failures(&summary, temp.path());
        let markdown = render(&summary, &failures);

        assert_eq!(failures.len(), 1, "the successful gate must not appear");
        assert!(markdown.contains("`parser::ranges::byte_offsets_round_trip`"));
        assert!(markdown.contains("crates/perl-parser-core/src/ranges.rs:118:9"));
        assert!(markdown.contains("utf-16 offset drifted"));
        assert!(markdown.contains("cargo test -p perl-parser-core --locked --lib"));
        Ok(())
    }

    /// One test's panic location must never be reported against another's
    /// name. This is the failure mode that makes a digest worse than no
    /// digest, because it reads as authoritative.
    #[test]
    fn two_failing_tests_keep_their_own_locations() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(
            temp.path(),
            "lsp_gate",
            "\
failures:

---- lsp::hover::renders_pod stdout ----
thread 'x' panicked at crates/perl-lsp-rs/src/hover.rs:10:1:
assertion failed: doc.is_some()

---- lsp::completion::offers_methods stdout ----
thread 'y' panicked at crates/perl-lsp-rs/src/completion.rs:88:5:
assertion failed: !items.is_empty()

test result: FAILED. 0 passed; 2 failed
",
        )?;
        let summary = summary_with(json!({
            "gate_name": "lsp_gate",
            "result": "failure",
            "exit_code": 101,
            "reproduce": "cargo test -p perl-lsp-rs --locked",
        }));

        let failures = collect_failures(&summary, temp.path());
        let tests = &failures[0].failing_tests;

        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "lsp::hover::renders_pod");
        assert_eq!(
            tests[0].panic_location.as_deref(),
            Some("crates/perl-lsp-rs/src/hover.rs:10:1")
        );
        assert_eq!(tests[1].name, "lsp::completion::offers_methods");
        assert_eq!(
            tests[1].panic_location.as_deref(),
            Some("crates/perl-lsp-rs/src/completion.rs:88:5")
        );
        assert!(
            !tests[0].excerpt.iter().any(|line| line.contains("items.is_empty")),
            "hover's excerpt reached into completion's block: {:?}",
            tests[0].excerpt
        );
        Ok(())
    }

    /// A missing log is missing evidence. Rendering "no failing test" over it
    /// would tell a reader the gate failed on something other than a test,
    /// which is a claim the digest has not earned.
    #[test]
    fn an_unreadable_log_is_reported_as_missing_evidence() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let summary = summary_with(json!({
            "gate_name": "vanished_gate",
            "result": "failure",
            "exit_code": 101,
            "reproduce": "cargo test -p nothing",
        }));

        let failures = collect_failures(&summary, temp.path());
        let markdown = render(&summary, &failures);

        assert!(failures[0].log_unavailable.is_some());
        assert!(markdown.contains("No test output is available"));
        assert!(markdown.contains("missing evidence, not a clean run"));
        assert!(markdown.contains("could not be read"));
        assert!(
            !markdown.contains("did not fail on an assertion"),
            "a missing log must not be reported as a non-assertion failure"
        );
        Ok(())
    }

    /// A gate that failed on something other than a test — a lint, a script,
    /// a timeout — says so, and still carries its repro command.
    #[test]
    fn a_gate_that_failed_without_a_test_says_so() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(temp.path(), "clippy_gate", "error: unused variable `x`\nerror: aborting\n")?;
        let summary = summary_with(json!({
            "gate_name": "clippy_gate",
            "result": "failure",
            "exit_code": 101,
            "reproduce": "cargo clippy --workspace -- -D warnings",
        }));

        let markdown = render(&summary, &collect_failures(&summary, temp.path()));

        assert!(markdown.contains("did not fail on an assertion"));
        assert!(markdown.contains("cargo clippy --workspace -- -D warnings"));
        Ok(())
    }

    /// A gate that never started carries the runner's own reason. Reporting it
    /// with no note would look like an unexplained red.
    #[test]
    fn an_unstarted_gate_carries_the_runners_message() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let summary = summary_with(json!({
            "gate_name": "blocked_gate",
            "result": "not_proven",
            "exit_code": Value::Null,
            "reproduce": "cargo test -p blocked",
            "message": "gate was waiting for dependency result(s): fmt_gate",
        }));

        let markdown = render(&summary, &collect_failures(&summary, temp.path()));

        assert!(markdown.contains("not_proven"));
        assert!(markdown.contains("waiting for dependency result(s): fmt_gate"));
        Ok(())
    }

    /// PR Smoke writes an `xtask gates` receipt, not a shard summary, and had
    /// no step summary at all. Its rows say `status`/`command`/`log_path`.
    #[test]
    fn an_xtask_gates_receipt_is_read_as_well_as_a_shard_summary() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(&temp.path().join("logs"), "test_gate", ASSERTION_LOG)?;
        let receipt = json!({
            "schema_version": "2",
            "subject_sha": "abc1234",
            "gates": [
                {"gate_name": "fmt_gate", "status": "passed", "command": "cargo fmt"},
                {
                    "gate_name": "test_gate",
                    "status": "failed",
                    "exit_code": 101,
                    "command": "cargo test -p perl-parser-core --locked",
                    "log_path": "logs/test_gate.log",
                },
            ],
        });

        let failures = collect_failures(&receipt, &temp.path().join("logs"));
        let markdown = render(&receipt, &failures);

        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].result, "failed");
        assert!(markdown.contains("`parser::ranges::byte_offsets_round_trip`"));
        assert!(markdown.contains("crates/perl-parser-core/src/ranges.rs:118:9"));
        assert!(markdown.contains("cargo test -p perl-parser-core --locked"));
        Ok(())
    }

    /// The two producers put the head and the gate count in different places,
    /// and reading only the shard summary's names silently mislabels every
    /// PR-fast digest: `Subject unknown`, and on a clean run "every one of the
    /// 0 selected gate(s) succeeded". A digest that exists to make a failure
    /// legible must not be confidently wrong about which commit it describes.
    #[test]
    fn an_xtask_gates_receipt_header_names_its_own_subject_and_count() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(&temp.path().join("logs"), "test_gate", ASSERTION_LOG)?;
        // The `xtask gates` shape: no `subject_sha`, no `selected_gates`.
        let receipt = json!({
            "schema_version": "2",
            "metadata": {"git_sha": "0ea6ef4430e0d453d608e992dd8bcfdecf9daedf", "git_sha_short": "0ea6ef4"},
            "summary": {"total_gates": 23},
            "gates": [
                {
                    "gate_name": "test_gate",
                    "status": "failed",
                    "exit_code": 101,
                    "command": "cargo test -p perl-parser-core --locked",
                    "log_path": "logs/test_gate.log",
                },
            ],
        });

        let failures = collect_failures(&receipt, &temp.path().join("logs"));
        let markdown = render(&receipt, &failures);

        assert!(
            markdown.contains("0ea6ef4"),
            "the header must name the receipt's own head, got:\n{markdown}"
        );
        assert!(
            !markdown.contains("Subject `unknown`"),
            "`metadata.git_sha_short` is present, so the subject is not unknown, got:\n{markdown}"
        );
        Ok(())
    }

    /// The clean-run wording carries the count, so the same schema gap made a
    /// successful PR-fast run claim that all zero gates passed.
    #[test]
    fn a_clean_xtask_gates_receipt_reports_its_real_gate_count() -> Result<()> {
        let receipt = json!({
            "schema_version": "2",
            "metadata": {"git_sha_short": "0ea6ef4"},
            "summary": {"total_gates": 23},
            "gates": [{"gate_name": "fmt_gate", "status": "passed", "command": "cargo fmt"}],
        });

        let markdown = render(&receipt, &[]);

        assert!(
            markdown.contains("23 selected gate(s) succeeded"),
            "a clean run must report the gates it actually ran, got:\n{markdown}"
        );
        Ok(())
    }

    /// A status the digest does not recognise must be explained, not assumed
    /// green. `timeout` and `instrument_failure` are the states hardest to
    /// diagnose, so silently passing them would defeat the whole change.
    #[test]
    fn an_unrecognised_status_is_treated_as_needing_explanation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let summary = json!({
            "subject_sha": "abc1234",
            "selected_gates": ["a", "b", "c"],
            "gates": [
                {"gate_name": "a", "result": "timeout", "reproduce": "cargo test -p a"},
                {"gate_name": "b", "result": "instrument_failure", "reproduce": "x"},
                {"gate_name": "c", "result": "skipped"},
            ],
        });

        let failures = collect_failures(&summary, temp.path());

        assert_eq!(failures.len(), 2, "skipped is not a failure; the other two are");
        assert_eq!(failures[0].result, "timeout");
        assert_eq!(failures[1].result, "instrument_failure");
        Ok(())
    }

    /// When the log is gone the receipt's own `first_failure` is still a
    /// direct runner observation. Discarding it would throw away the one
    /// thing that survived.
    #[test]
    fn a_missing_log_still_reports_the_receipts_first_failure() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipt = json!({
            "subject_sha": "abc1234",
            "gates": [{
                "gate_name": "gone",
                "status": "failed",
                "exit_code": 101,
                "command": "cargo test -p gone",
                "first_failure": {
                    "test": "gone::tests::still_named",
                    "site": "src/gone.rs:3",
                    "message": "assertion failed: value.is_some()",
                    "exit_code": 101,
                },
            }],
        });

        let failures = collect_failures(&receipt, temp.path());
        let markdown = render(&receipt, &failures);

        assert!(failures[0].log_unavailable.is_some());
        assert!(markdown.contains("gone::tests::still_named"));
        assert!(markdown.contains("src/gone.rs:3"));
        assert!(markdown.contains("assertion failed: value.is_some()"));
        Ok(())
    }

    /// A backtrace is the least useful and longest part of a failure block.
    /// Carrying it would spend the step-summary budget on what the reader
    /// already has in the log.
    #[test]
    fn an_excerpt_stops_before_the_backtrace() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(
            temp.path(),
            "noisy_gate",
            "\
---- suite::one stdout ----
thread 'suite::one' panicked at src/a.rs:1:1:
assertion `left == right` failed
  left: 0
 right: 1
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
note: run with `RUST_BACKTRACE=full` for a verbose backtrace
",
        )?;
        let summary = summary_with(json!({
            "gate_name": "noisy_gate",
            "result": "failure",
            "reproduce": "cargo test -p noisy",
        }));

        let markdown = render(&summary, &collect_failures(&summary, temp.path()));

        assert!(markdown.contains("assertion `left == right` failed"));
        assert!(markdown.contains("left: 0"));
        assert!(!markdown.contains("stack backtrace"));
        assert!(!markdown.contains("rust_begin_unwind"));
        assert!(!markdown.contains("RUST_BACKTRACE"));
        Ok(())
    }

    /// `xtask gates` writes an execution failure's only human-readable reason
    /// into `output_summary`, and usually leaves no log at all, so reading
    /// `message` alone left the reader with nothing.
    #[test]
    fn a_receipts_execution_error_is_carried_from_output_summary() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let summary = json!({
            "gates": [{
                "gate_name": "spawn_gate",
                "status": "error",
                "output_summary": "Execution error: No such file or directory (os error 2)",
            }]
        });

        let failures = collect_failures(&summary, temp.path());

        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].message.as_deref(),
            Some("Execution error: No such file or directory (os error 2)")
        );
        assert!(render(&summary, &failures).contains("No such file or directory"));
        Ok(())
    }

    /// An assertion over markdown can carry a fence of its own, which would
    /// close the excerpt's block early and corrupt every section appended
    /// after it in the step summary.
    #[test]
    fn an_excerpt_carrying_a_fence_is_wrapped_in_a_longer_one() -> Result<()> {
        let temp = tempfile::tempdir()?;
        write_log(
            temp.path(),
            "hover_gate",
            concat!(
                "---- hover::fenced stdout ----\n",
                "thread 'main' panicked at src/hover.rs:9:5:\n",
                "assertion failed: rendered == \"```perl\\nmy $x;\\n```\"\n",
                "\ntest result: FAILED. 0 passed; 1 failed\n",
            ),
        )?;
        let summary = json!({
            "gates": [{"gate_name": "hover_gate", "result": "failure", "exit_code": 101}]
        });

        let rendered = render(&summary, &collect_failures(&summary, temp.path()));

        assert!(rendered.contains("````text"), "the fence must outgrow the excerpt's own");
        assert!(rendered.contains("```perl"), "the excerpt itself is still shown verbatim");
        Ok(())
    }

    #[test]
    fn a_clean_shard_renders_a_single_line() -> Result<()> {
        let summary = json!({
            "subject_sha": "abc1234",
            "selected_gates": ["fmt_gate"],
            "gates": [{"gate_name": "fmt_gate", "result": "success", "exit_code": 0}],
        });

        let markdown = render(&summary, &collect_failures(&summary, Path::new("/nonexistent")));

        assert!(markdown.contains("Every one of the 1 selected gate(s) succeeded"));
        assert!(!markdown.contains("Reproduce"));
        Ok(())
    }

    /// A gate with hundreds of failing tests must not push every other step's
    /// summary out of the job's step-summary budget.
    #[test]
    fn a_long_failure_list_is_elided_with_a_count() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let log: String =
            (0..25).map(|index| format!("test suite::case_{index} ... FAILED\n")).collect();
        write_log(temp.path(), "wide_gate", &log)?;
        let summary = summary_with(json!({
            "gate_name": "wide_gate",
            "result": "failure",
            "exit_code": 101,
            "reproduce": "cargo test -p wide",
        }));

        let markdown = render(&summary, &collect_failures(&summary, temp.path()));

        assert!(markdown.contains("suite::case_0"));
        assert!(markdown.contains("suite::case_9"));
        assert!(!markdown.contains("suite::case_24"));
        assert!(markdown.contains("…and 15 more"));
        Ok(())
    }

    /// The digest appends. The existing `Gate summary` step writes the gate
    /// table first, and overwriting it would trade one diagnostic for another.
    #[test]
    fn writing_the_digest_replaces_a_file_left_by_another_run() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let out = temp.path().join("summary.md");
        fs::write(&out, "### CI Gate shard: parser_stack\n")?;
        let summary_path = temp.path().join("shard.json");
        fs::write(
            &summary_path,
            serde_json::to_string(&json!({
                "subject_sha": "abc1234",
                "selected_gates": ["fmt_gate"],
                "gates": [{"gate_name": "fmt_gate", "result": "success"}],
            }))?,
        )?;

        run(GateFailureDigestConfig {
            summary: summary_path,
            logs: temp.path().join("logs"),
            out: out.clone(),
            print: false,
        })?;

        let written = fs::read_to_string(&out)?;
        assert!(
            !written.contains("### CI Gate shard: parser_stack"),
            "a digest restored from another run's cached target/ must not ride \
             into this run's summary"
        );
        assert!(
            written.contains("Every one of the 1 selected gate(s) succeeded"),
            "this run's own digest must still be written"
        );
        Ok(())
    }
}
