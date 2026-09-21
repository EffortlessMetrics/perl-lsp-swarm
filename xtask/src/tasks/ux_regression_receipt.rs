use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use chrono::Utc;
use color_eyre::eyre::{Context, Result};
use perl_lsp_ux_tests::taxonomy::{UxComponent, UxFailureClass, UxRoute, route_for_failure_class};
use regex::Regex;
use serde::Serialize;

#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static FAILED_TEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"test\s+([^\s]+)\s+\.\.\.\s+FAILED").expect("failed test regex must compile")
});
// Matches both pre-1.73 format ("panicked at 'msg', path:row:col") and
// post-1.73 format ("panicked at path:row:col:") where the location appears
// directly after "panicked at " without a quoted message.
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static PANIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"panicked at (?:'[^']*',\s*)?([a-zA-Z][^:\s][^:]*:\d+:\d+)")
        .expect("panic regex must compile")
});

// Cargo prints one `---- <test name> stdout ----` block per failing test in its
// trailing `failures:` report. Splitting on that header is what lets each failing
// test be classified from its own evidence instead of from the whole log, where one
// test's wording silently reclassifies another's (#15988).
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static FAILURE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^-{4}\s+(\S+)\s+stdout\s+-{4}\s*$")
        .expect("failure block header regex must compile")
});

// `WaitEnd::Deadline` is the one harness outcome documented to mean "nothing
// decided": a live stream that simply did not produce the awaited observation in
// time. `WaitEnd::describe` renders it with this exact wording, so matching it is
// evidence from the harness rather than a guess about the wording of a panic.
// See crates/perl-lsp-ux-tests/src/observation.rs.
const DEADLINE_MARKER: &str = "deadline expired after";

// libtest prints one result line per test, unconditionally, and cargo tees the
// whole run into the log the class is read from. A test that passed contributes
// exactly that one line — its own name — because its stdout is captured. Those
// names are not evidence about the test that failed, and they decided the class:
// the single UX test function whose name contains `baseline` passes on every run,
// which routed unrelated failures to `update_baseline` (#16103). libtest spells a
// pass `ok` and a skip `ignored` in lower case and a failure `FAILED` in upper, so
// the failing test's own line survives this filter.
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static PASSING_TEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*test\s+\S+\s+\.\.\.\s+(?:ok|ignored)\b")
        .expect("passing test regex must compile")
});

#[derive(Debug, Clone)]
pub struct UxRegressionReceiptConfig {
    pub input: PathBuf,
    pub receipt: Option<PathBuf>,
    pub sha: Option<String>,
    pub exit_status_file: Option<PathBuf>,
}

/// Why one test failed, told apart by evidence inside that test's own output.
///
/// The gate's single whole-run `failure_class` cannot answer the question triage
/// actually asks: did this change break something, or did a shared runner make a
/// latency budget expire? Both render as a red check today (#15988). These variants
/// separate the two where the log can prove it and, just as importantly, say so when
/// it cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UxFailureMode {
    /// A bounded wait expired with the stream still live. Nothing about the change
    /// was decided; the observation simply did not arrive inside its budget.
    BudgetExceeded,
    /// An assertion compared a real observed value and rejected it. The change is
    /// the subject; a co-tenant runner is not a sufficient explanation.
    AssertionFailed,
    /// An assertion failed on an absent or empty observation with no budget evidence
    /// in the block. A genuine regression and an expired budget both look like this,
    /// so the mode is reported rather than guessed. Resolving it needs the harness to
    /// carry its `WaitEnd` outcome into the assertion message.
    AssertionOnAbsentObservation,
    /// The test panicked without an assertion: an unwrap, an index, a crash path.
    Panic,
    /// The block carried no evidence this classifier is willing to read.
    Unknown,
}

/// One failing test and the evidence its own output carried.
#[derive(Debug, Clone, Serialize)]
pub struct UxFailingTest {
    /// Fully qualified test name as cargo printed it.
    pub name: String,
    /// Mode inferred from this test's block alone.
    pub mode: UxFailureMode,
    /// True when `mode` rests on an explicit marker in the block rather than on the
    /// absence of one. A consumer should treat a false here as "needs a human".
    pub discriminated: bool,
    /// The single line the mode was read from, for a reader who wants the receipt to
    /// show its work.
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UxRegressionReceipt {
    kind: &'static str,
    schema_version: u32,
    measured_at: String,
    sha: String,
    workflow: Option<String>,
    scenario_file: Option<String>,
    scenario: Option<String>,
    first_failing_test: Option<String>,
    /// Per-test discrimination, one entry per `---- <name> stdout ----` block.
    /// Additive: `failure_class` below keeps its existing whole-run meaning.
    failing_tests: Vec<UxFailingTest>,
    result: String,
    failure_class: UxFailureClass,
    panic_location: Option<String>,
    canonical_repro: Option<String>,
    friendly_repro: Option<String>,
    first_failing_line: Option<String>,
    route: UxRoute,
    blocking: bool,
    merge_action: String,
    human_summary: String,
    component: Option<UxComponent>,
    run_id: Option<String>,
    attempt: Option<u32>,
    platform: Option<String>,
}

pub fn run(config: UxRegressionReceiptConfig) -> Result<()> {
    let raw = fs::read_to_string(&config.input)
        .with_context(|| format!("reading {}", config.input.display()))?;
    let exit_status = config
        .exit_status_file
        .map(|path| {
            let raw_status = fs::read_to_string(&path)
                .with_context(|| format!("reading UX test exit status {}", path.display()))?;
            raw_status
                .trim()
                .parse::<i32>()
                .with_context(|| format!("parsing UX test exit status in {}", path.display()))
        })
        .transpose()?;
    let receipt = classify_with_exit_status(&raw, config.sha, exit_status);
    let payload = serde_json::to_string_pretty(&receipt)?;

    if let Some(path) = config.receipt {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, format!("{payload}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
        println!("Wrote UX regression receipt: {}", path.display());
    } else {
        println!("{payload}");
    }

    Ok(())
}

#[cfg(test)]
fn classify(raw: &str, sha: Option<String>) -> UxRegressionReceipt {
    classify_with_exit_status(raw, sha, None)
}

fn classify_with_exit_status(
    raw: &str,
    sha: Option<String>,
    exit_status: Option<i32>,
) -> UxRegressionReceipt {
    let lines: Vec<&str> = raw.lines().collect();
    let first_fail_line =
        lines.iter().find(|line| line.contains("FAILED")).map(|line| (*line).trim().to_string());
    let first_failing_test =
        lines.iter().find_map(|line| FAILED_TEST_RE.captures(line).map(|cap| cap[1].to_string()));
    let panic_location =
        first_failing_test.as_ref().and_then(|name| panic_location_for_test(raw, name));
    let scenario = first_failing_test.as_ref().and_then(|name| scenario_from_test_name(name));
    let workflow = first_failing_test.as_ref().and_then(|name| workflow_from_test_name(name));

    let failing_tests = discriminate_failing_tests(raw);
    let failure_class = run_failure_class(&failing_tests, raw);

    let canonical_repro = first_failing_test.as_ref().map(|name| {
        format!("cargo test -p perl-lsp-ux-tests {name} -- --test-threads=1 --nocapture")
    });

    let friendly_repro = first_failing_test.as_ref().map(|name| {
        // Extract just the test function name (after ::) for the shorthand command.
        let short = name.split("::").last().unwrap_or(name);
        format!("just ux-tests {short}")
    });

    let route = route_for_failure_class(failure_class);
    let has_failed_test = first_failing_test.is_some()
        || lines.iter().any(|line| line.contains("test result: FAILED"));
    let has_passing_summary = lines.iter().any(|line| line.contains("test result: ok"));
    let command_succeeded = exit_status.map(|status| status == 0).unwrap_or(true);
    let result =
        if command_succeeded && has_passing_summary && !has_failed_test { "pass" } else { "fail" }
            .to_string();
    let blocking = result != "pass";
    let merge_action = if !blocking {
        "merge_allowed"
    } else {
        match failure_class {
            UxFailureClass::TestRace => "quarantine_or_fix_test",
            UxFailureClass::ProviderRegression => "fix_provider",
            UxFailureClass::MatrixDrift => "update_fixture_matrix",
            UxFailureClass::BaselineDrift => "update_baseline",
            UxFailureClass::Timeout => "triage_timeout",
            UxFailureClass::Infra => "fix_ci_infra",
            UxFailureClass::ServerCrash => "fix_crash",
            UxFailureClass::NewTestBug => "fix_test",
            UxFailureClass::Unknown => "triage",
            _ => "triage",
        }
    }
    .to_string();
    let human_summary = if result == "pass" {
        "UX regression passed; merge_allowed.".to_string()
    } else {
        let test = first_failing_test.as_deref().unwrap_or("unknown_test");
        let repro = canonical_repro.as_deref().unwrap_or("see ux-regression.log");
        format!("UX regression failed in {test}; classified as {failure_class:?}; repro: {repro}")
    };

    UxRegressionReceipt {
        kind: "ux_regression_receipt",
        // 2 adds `failing_tests`. Every field of version 1 keeps its meaning, so a
        // reader that ignores the new array behaves exactly as it did before.
        schema_version: 2,
        measured_at: Utc::now().to_rfc3339(),
        sha: sha.unwrap_or_else(|| "unknown".to_string()),
        workflow,
        scenario_file: scenario.clone(),
        scenario,
        first_failing_test,
        failing_tests,
        result,
        failure_class,
        panic_location,
        canonical_repro,
        friendly_repro,
        first_failing_line: first_fail_line,
        route,
        blocking,
        merge_action,
        human_summary,
        component: None,
        run_id: None,
        attempt: None,
        platform: None,
    }
}

/// One failing test's stdout block per cargo `---- <name> stdout ----` header:
/// (test name, where its body starts, where the next header starts).
///
/// Shared by per-block discrimination (#15988) and panic-location scoping
/// (#16148) so there is exactly one span implementation.
fn failure_block_spans(raw: &str) -> Vec<(String, usize, usize)> {
    // (test name, where its body starts, where the next header starts)
    let mut headers: Vec<(String, usize, usize)> = Vec::new();
    for capture in FAILURE_BLOCK_RE.captures_iter(raw) {
        let (Some(header), Some(name)) = (capture.get(0), capture.get(1)) else {
            continue;
        };
        headers.push((name.as_str().to_string(), header.end(), header.start()));
    }
    let mut spans: Vec<(String, usize, usize)> = Vec::new();
    for (index, (name, body_start, _)) in headers.iter().enumerate() {
        let body_end = headers
            .get(index + 1)
            .map_or(raw.len(), |(_, _, next_header_start)| *next_header_start);
        spans.push((name.clone(), *body_start, body_end));
    }
    spans
}

/// The panic site of one named failing test, read from that test's own stdout
/// block only (#16148). The receipt is flat, so adjacent fields read as one
/// pair: a whole-log scan reports a later test's crash site under the first
/// test's name when the first test fails without panicking.
fn panic_location_for_test(raw: &str, name: &str) -> Option<String> {
    for (block_name, body_start, body_end) in failure_block_spans(raw) {
        if block_name != name {
            continue;
        }
        let block = raw.get(body_start..body_end).unwrap_or_default();
        return block_body(block)
            .lines()
            .find_map(|line| PANIC_RE.captures(line).map(|cap| cap[1].to_string()));
    }
    None
}

/// Split cargo's trailing failure report into one block per failing test and
/// classify each from its own evidence.
///
/// Whole-log classification is what makes a latency timeout and a real regression
/// indistinguishable: `infer_failure_class` scans the entire log, so one test
/// mentioning a baseline reclassifies another test's expired budget. Per-block
/// reading keeps each failure's evidence to itself.
fn discriminate_failing_tests(raw: &str) -> Vec<UxFailingTest> {
    let spans = failure_block_spans(raw);

    let mut discriminated: Vec<UxFailingTest> = Vec::new();
    for (name, start, end) in spans {
        if discriminated.iter().any(|existing| existing.name == name) {
            continue;
        }
        let block = raw.get(start..end).unwrap_or_default();
        let (mode, evidence) = classify_failure_mode(block_body(block));
        discriminated.push(UxFailingTest {
            name,
            mode,
            discriminated: mode_is_evidence_backed(mode),
            evidence,
        });
    }

    if !discriminated.is_empty() {
        return discriminated;
    }

    // Cargo reported failures but printed no stdout block for them. Name the tests
    // and admit the mode is unknown rather than borrowing the whole-log class.
    for line in raw.lines() {
        let Some(capture) = FAILED_TEST_RE.captures(line) else {
            continue;
        };
        let Some(name) = capture.get(1) else {
            continue;
        };
        let name = name.as_str().to_string();
        if discriminated.iter().any(|existing| existing.name == name) {
            continue;
        }
        discriminated.push(UxFailingTest {
            name,
            mode: UxFailureMode::Unknown,
            discriminated: false,
            evidence: None,
        });
    }
    discriminated
}

/// Trim cargo's run-level trailer off the end of a block so the last failing test
/// does not inherit the summary lines that follow every failure report.
fn block_body(block: &str) -> &str {
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

/// Read one failing test's block and say what kind of failure it was.
///
/// Precedence is deliberate. A budget marker wins outright, because
/// `WaitEnd::Deadline` is documented as the only outcome meaning "nothing decided" —
/// once a bounded wait expired, whatever assertion fired afterwards was judging an
/// observation that never arrived.
fn classify_failure_mode(block: &str) -> (UxFailureMode, Option<String>) {
    let mut assertion_line: Option<&str> = None;
    let mut panic_line: Option<&str> = None;

    for line in block.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains(DEADLINE_MARKER)
            || lower.contains("timed out after")
            || lower.contains("timed out waiting")
            || lower.contains("test timed out")
        {
            return (UxFailureMode::BudgetExceeded, Some(line.trim().to_string()));
        }
        if assertion_line.is_none() && lower.contains("assertion") && lower.contains("failed") {
            assertion_line = Some(line);
        }
        if panic_line.is_none() && lower.contains("panicked at") {
            panic_line = Some(line);
        }
    }

    if let Some(line) = assertion_line {
        let mode = if observation_reads_absent(block) {
            UxFailureMode::AssertionOnAbsentObservation
        } else {
            UxFailureMode::AssertionFailed
        };
        return (mode, Some(line.trim().to_string()));
    }
    if let Some(line) = panic_line {
        return (UxFailureMode::Panic, Some(line.trim().to_string()));
    }
    (UxFailureMode::Unknown, None)
}

/// True when the block shows the assertion rejecting an empty or missing value.
///
/// These renderings are exactly what a provider returns when its result never
/// arrived, which is why a block matching this is reported as ambiguous instead of
/// being called a regression.
fn observation_reads_absent(block: &str) -> bool {
    let lower = block.to_ascii_lowercase();
    ["got []", "got {}", "got none", "left: []", "right: []", "left: {}", "was empty"]
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Whether a mode rests on a marker that was present, rather than on one that was
/// absent. A consumer should route anything false here to a human.
const fn mode_is_evidence_backed(mode: UxFailureMode) -> bool {
    match mode {
        UxFailureMode::BudgetExceeded | UxFailureMode::AssertionFailed | UxFailureMode::Panic => {
            true
        }
        UxFailureMode::AssertionOnAbsentObservation | UxFailureMode::Unknown => false,
    }
}

/// The text `infer_failure_class` is allowed to read.
///
/// Two kinds of line are removed because they describe something other than the
/// failure: a scenario's own diagnostic detail block, and the result line of a test
/// that passed or was skipped. Both are present on every run and neither says
/// anything about why this run failed.
fn classification_input(raw: &str) -> String {
    let mut in_detail = false;
    let mut retained = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("UX_SCENARIO_DETAIL_BEGIN:") {
            in_detail = true;
        } else if trimmed == "UX_SCENARIO_DETAIL_END" {
            in_detail = false;
        } else if !in_detail && !PASSING_TEST_RE.is_match(line) {
            retained.push(line);
        }
    }
    retained.join("\n")
}

fn scenario_from_test_name(test: &str) -> Option<String> {
    let scenario = test.split("::").next()?;
    if scenario.starts_with("ux_scenario_") { Some(format!("{scenario}.rs")) } else { None }
}

/// The run's class, preferring per-test evidence over the whole-log word scan.
///
/// `infer_failure_class` matches substrings across the entire log, so one
/// incidental "baseline" — a cache step, a path, an unrelated line — classifies
/// the whole run as `BaselineDrift` and routes it to `update_baseline`. On a
/// latency probe that is the one remedy the repository forbids: widening a budget
/// until the noise fits buries the non-determinism instead of reporting it
/// (#16205, measured on job 106081131409).
///
/// `discriminate_failing_tests` has already read each block and said, with the
/// line it read, which failures were expired waits. This applies the same
/// precedence `classify_failure_mode` documents inside a block — a budget marker
/// wins outright, because a wait that expired decided nothing — one level up.
///
/// That precedence does not transfer between tests, though: one test's expired
/// wait says nothing about any other test's failure. So the budget decides the
/// run only when EVERY failing test is a proven expired budget.
///
/// Requiring all of them, rather than merely the absence of a contrary verdict,
/// is deliberate. `Unknown` and `AssertionOnAbsentObservation` are the modes
/// `mode_is_evidence_backed` declines to back, and an unresolved co-failure is
/// not evidence of a flake — it is the absence of evidence. Letting one test's
/// deadline marker speak for it would transfer exactly the precedence the
/// paragraph above denies, and would take `update_baseline` away from a real
/// baseline failure that happened to run beside a slow probe.
///
/// The ambiguous cases therefore keep a substring scan, but the scan's input
/// shrinks to the failing tests' own evidence (#16205). Proven budget evidence
/// beats it one level up, and `Unknown` or an unresolved co-failure still defers
/// to it rather than guessing with more steps.
fn run_failure_class(failing_tests: &[UxFailingTest], raw: &str) -> UxFailureClass {
    let every_failure_is_an_expired_budget = !failing_tests.is_empty()
        && failing_tests.iter().all(|test| test.mode == UxFailureMode::BudgetExceeded);

    if every_failure_is_an_expired_budget {
        UxFailureClass::Timeout
    } else {
        infer_failure_class(&fallback_classification_input(failing_tests, raw))
    }
}

/// The text the whole-run fallback scan is allowed to read (#16205).
///
/// The receipt's `failure_class` is one flat field, so when no failing test carried
/// evidence strong enough to decide it, something must. The scan stays, but prose
/// the gate printed *around* the failure — a cache step, a restored-snapshot note,
/// a baseline path in a startup line — is not the failing test's evidence. On job
/// 106081131409 one such incidental `baseline` classified a run whose only failure
/// was completion starvation as `BaselineDrift` and routed it to `update_baseline`,
/// the remedy the repository forbids for a candidate that returned nothing.
/// Scoping the scan to the failing blocks keeps each test's wording as the only
/// vocabulary that can decide the run. When cargo printed no stdout blocks there
/// is nothing to scope to, so the filtered whole log remains all the evidence
/// there is.
fn fallback_classification_input(failing_tests: &[UxFailingTest], raw: &str) -> String {
    let names: Vec<&str> = failing_tests.iter().map(|test| test.name.as_str()).collect();
    let bodies: Vec<&str> = failure_block_spans(raw)
        .iter()
        .filter(|(name, _, _)| names.contains(&name.as_str()))
        .filter_map(|(_, body_start, body_end)| raw.get(*body_start..*body_end))
        .map(block_body)
        .collect();
    if bodies.is_empty() {
        return classification_input(raw);
    }
    classification_input(&bodies.join("\n"))
}

fn infer_failure_class(raw: &str) -> UxFailureClass {
    let lower = raw.to_ascii_lowercase();
    if looks_like_scenario_19_race(&lower) {
        UxFailureClass::TestRace
    } else if looks_like_scenario_14_provider_regression(&lower) {
        UxFailureClass::ProviderRegression
    } else if lower.contains("fixture matrix") || lower.contains("matrix drift") {
        UxFailureClass::MatrixDrift
    } else if lower.contains("baseline") || lower.contains("snapshot") {
        UxFailureClass::BaselineDrift
    } else if lower.contains("timed out") || lower.contains("timeout") {
        UxFailureClass::Timeout
    } else if lower.contains("race") || lower.contains("flaky") {
        UxFailureClass::TestRace
    } else if lower.contains("panicked") && lower.contains("tests/ux_scenario_") {
        UxFailureClass::NewTestBug
    } else if lower.contains("no such file") || lower.contains("permission denied") {
        UxFailureClass::Infra
    } else if lower.contains("assertion failed") {
        // Check ProviderRegression before the generic panicked/ServerCrash catch-all:
        // a typical assertion failure log contains both "panicked" and "assertion failed",
        // so this branch must precede the ServerCrash arm to remain reachable.
        // Note: we do NOT match on bare "expected" because it appears as a substring of
        // unrelated words like "unexpectedly", causing false positives on ServerCrash logs.
        UxFailureClass::ProviderRegression
    } else if lower.contains("panicked") || lower.contains("server exited") {
        UxFailureClass::ServerCrash
    } else {
        UxFailureClass::Unknown
    }
}

fn looks_like_scenario_19_race(lower: &str) -> bool {
    lower.contains("scenario_19_diagnostics_clear_after_fix")
        && (lower.contains("pre-fix")
            || lower.contains("post-fix")
            || lower.contains("diagnostics race")
            || lower.contains("events:")
            || lower.contains("expected diagnostics to clear"))
}

fn looks_like_scenario_14_provider_regression(lower: &str) -> bool {
    lower.contains("ux_scenario_14_inc_conformance")
        && (lower.contains("goto-definition")
            || lower.contains("include path")
            || lower.contains("include_paths")
            || lower.contains("includepaths")
            || lower.contains("perl5lib")
            || lower.contains("usesysteminc")
            || lower.contains("@inc"))
}

fn workflow_from_test_name(test: &str) -> Option<String> {
    let workflow = test.split("::").nth(1)?;
    if workflow.is_empty() { None } else { Some(workflow.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    use color_eyre::eyre::{bail, ensure};

    #[test]
    fn classify_extracts_structured_fields() {
        // Uses the Rust 1.73+ panic format: "panicked at path:row:col:" (no quoted message).
        // The project toolchain is 1.95, so this is the format actual test output uses.
        let log = "running 1 test\ntest ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix ... FAILED\n\nfailures:\n\n---- ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix stdout ----\nthread 'x' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_19_diagnostics_lifecycle.rs:102:5:\nboom\n\nfailures:\n    ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix\n\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("abc123".to_string()));
        assert_eq!(receipt.sha, "abc123", "sha should match input");
        assert_eq!(
            receipt.scenario.as_deref(),
            Some("ux_scenario_19_diagnostics_lifecycle.rs"),
            "scenario should be extracted from test name"
        );
        assert_eq!(
            receipt.workflow.as_deref(),
            Some("scenario_19_diagnostics_clear_after_fix"),
            "workflow should map to test function name"
        );
        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some("ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix"),
            "test name should be extracted from log"
        );
        assert!(
            matches!(receipt.failure_class, UxFailureClass::NewTestBug),
            "failure_class should be NewTestBug for panicked test in ux_scenario"
        );
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("crates/perl-lsp-ux-tests/tests/ux_scenario_19_diagnostics_lifecycle.rs:102:5"),
            "panic_location should be extracted from panic line (Rust 1.73+ format)"
        );
        assert_eq!(receipt.route, UxRoute::TestFix, "race/new test bug routes to test fix");
    }

    #[test]
    fn classify_timeout_routes_to_timeout_triage() {
        let log = "running 1 test\ntest ux_scenario_01_startup::start_server ... FAILED\ntest timed out after 30s\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha1".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::Timeout),
            "timed out log should classify as Timeout"
        );
        assert_eq!(receipt.route, UxRoute::TimeoutTriage, "Timeout routes to TimeoutTriage");
        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking, "selected test failure must block the receipt");
        assert_eq!(receipt.merge_action, "triage_timeout");
    }

    #[test]
    fn classify_server_crash_routes_to_crash_fix() {
        // ServerCrash: panicked in non-ux_scenario path (e.g., the LSP server process itself).
        // Must not contain "tests/ux_scenario_" (NewTestBug) or "assertion failed" (ProviderRegression).
        let log = "running 1 test\ntest ux_scenario_02_open::open_file ... FAILED\nthread 'server' panicked at crates/perl-lsp-rs/src/provider.rs:55:9:\nserver crashed with SIGABRT\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha2".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::ServerCrash),
            "non-ux_scenario panic should classify as ServerCrash, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::CrashFix, "ServerCrash routes to CrashFix");
    }

    #[test]
    fn classify_unexpected_exit_routes_to_crash_fix() {
        // "unexpectedly" contains the substring "expected", but ProviderRegression only
        // triggers on "assertion failed" — not bare "expected" — so this must classify
        // as ServerCrash, not ProviderRegression.
        let log = "running 1 test\ntest ux_scenario_03_diag::diag_test ... FAILED\nthread 'main' panicked at crates/perl-lsp-rs/src/server.rs:10:1:\nserver exited unexpectedly\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha8".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::ServerCrash),
            "log with 'unexpectedly' (substring of 'expected') should be ServerCrash, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::CrashFix);
    }

    #[test]
    fn classify_matrix_drift_routes_to_fixture_update() {
        let log = "running 2 tests\ntest ux_scenario_05_matrix::check_matrix ... FAILED\nfixture matrix mismatch: expected 3 items, got 4\ntest result: FAILED. 1 passed; 1 failed";
        let receipt = classify(log, Some("sha3".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::MatrixDrift),
            "fixture matrix log should classify as MatrixDrift"
        );
        assert_eq!(receipt.route, UxRoute::FixtureUpdate, "MatrixDrift routes to FixtureUpdate");
        assert_eq!(receipt.merge_action, "update_fixture_matrix");
    }

    #[test]
    fn classify_baseline_drift_routes_to_baseline_update() {
        let log = "running 1 test\ntest ux_scenario_10_hover::hover_type ... FAILED\nbaseline snapshot mismatch\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha4".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "baseline snapshot log should classify as BaselineDrift"
        );
        assert_eq!(
            receipt.route,
            UxRoute::BaselineUpdate,
            "BaselineDrift routes to BaselineUpdate"
        );
        assert_eq!(receipt.merge_action, "update_baseline");
    }

    #[test]
    fn classify_ignores_the_names_of_tests_that_passed() {
        // `just ux-tests` tees one `test <name> ... ok` line per passing test into the
        // same log the class is read from. Exactly one UX test function name in the
        // workspace contains the substring `baseline`, and it passes:
        // crates/perl-lsp-ux-tests/tests/ux_scenario_20_real_workspace_providers.rs.
        // Its presence routed an unrelated failure to `update_baseline` — an
        // instruction to move a number for a test that carries none (#16103).
        let log = "running 2 tests\ntest scenario_20_completion_module_prefix_surfaces_real_baseline_app_hard_assert ... ok\ntest ux_latency_workspace_symbols_sees_open_document_symbols ... FAILED\n\nfailures:\n\n---- ux_latency_workspace_symbols_sees_open_document_symbols stdout ----\nassertion failed: workspace symbols missing alpha\n\ntest result: FAILED. 1 passed; 1 failed";
        let receipt = classify(log, Some("sha-passing-name".to_string()));
        assert!(
            !matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "a passing test's name must not decide the failing test's class, got {:?}",
            receipt.failure_class
        );
        assert_ne!(
            receipt.merge_action, "update_baseline",
            "the receipt must not tell the next reader to update a baseline the failing test does not have"
        );
        assert!(
            matches!(receipt.failure_class, UxFailureClass::ProviderRegression),
            "the bare assertion in the failing test's own block is the evidence, got {:?}",
            receipt.failure_class
        );
    }

    #[test]
    fn classify_keeps_the_failing_test_own_result_line_as_evidence() {
        // The line that reports the *failing* test is its own evidence and must
        // survive the filter, or a class keyed on the test's name stops working.
        let log = "running 1 test\ntest scenario_10_hover_baseline_snapshot ... FAILED\nleft != right\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha-failing-name".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "the failing test's own name is evidence and must still classify, got {:?}",
            receipt.failure_class
        );
    }

    #[test]
    fn classify_provider_regression_routes_to_provider_fix() {
        // ProviderRegression: assertion failure without a panic in a ux_scenario_ path.
        // Must reach the ProviderRegression branch (not be swallowed by ServerCrash).
        let log = "running 1 test\ntest ux_scenario_07_completion::completions ... FAILED\nassertion failed: left == right\n  left: 3\n right: 5\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha5".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::ProviderRegression),
            "assertion-failed log without panic-in-ux_scenario_ should classify as ProviderRegression, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::ProviderFix, "ProviderRegression routes to ProviderFix");
    }

    #[test]
    fn classify_unknown_routes_to_triage() {
        let log = "running 1 test\ntest ux_scenario_99_misc::misc_test ... FAILED\nsome completely unrecognized error message\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha6".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::Unknown),
            "unrecognized log should classify as Unknown"
        );
        assert_eq!(receipt.route, UxRoute::Triage, "Unknown routes to Triage");
    }

    #[test]
    fn classify_sha_unknown_when_none() {
        let log = "test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.sha, "unknown", "None sha should produce 'unknown' in receipt");
    }

    #[test]
    fn classify_result_pass_on_ok_output() {
        let log = "running 5 tests\ntest result: ok. 5 passed; 0 failed";
        let receipt = classify(log, Some("sha7".to_string()));
        assert_eq!(receipt.result, "pass", "log with 'test result: ok' should produce result=pass");
        assert!(!receipt.blocking);
        assert_eq!(receipt.merge_action, "merge_allowed");
    }

    #[test]
    fn classify_ignores_diagnostic_detail_lines() {
        let log = "running 1 test\n\
test ux_scenario_44_editor_trust::scenario_44_real_editor_trust_smoke_receipt ... FAILED\n\
UX_SCENARIO_DETAIL_BEGIN: `scenario_44`\n\
workspace/executeCommand rejected the request: command not allowed; timeout details are diagnostic only\n\
UX_SCENARIO_DETAIL_END\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha-detail".to_string()));
        assert!(
            matches!(receipt.failure_class, UxFailureClass::Unknown),
            "diagnostic detail must not change the scenario receipt classification: {:?}",
            receipt.failure_class
        );
    }

    #[test]
    fn classify_mixed_nested_results_fails_closed_on_selected_failure() {
        let log = "running 1 test\n\
            test helper::setup ... ok\n\
            test result: ok. 1 passed; 0 failed\n\
            test ux_scenario_44_real_editor_trust_smoke_receipt::scenario_44_real_editor_trust_smoke_receipt ... FAILED\n\
            test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("sha-mixed".to_string()));

        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking, "selected test failure must block the receipt");
        assert_ne!(receipt.merge_action, "merge_allowed");
        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some(
                "ux_scenario_44_real_editor_trust_smoke_receipt::scenario_44_real_editor_trust_smoke_receipt"
            )
        );
    }

    #[test]
    fn classify_missing_summary_fails_closed() {
        let receipt = classify("just ux-tests: command failed before test summary", None);

        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking, "missing test summary must block the receipt");
        assert_ne!(receipt.merge_action, "merge_allowed");
    }

    #[test]
    fn classify_nonzero_command_status_fails_closed_after_earlier_passing_summary() {
        let log = "running 1 test\ntest helper::setup ... ok\ntest result: ok. 1 passed; 0 failed";
        let receipt = classify_with_exit_status(log, Some("sha-abort".to_string()), Some(134));

        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking, "nonzero command status must block the receipt");
        assert_ne!(receipt.merge_action, "merge_allowed");
    }

    #[test]
    fn workflow_from_test_name_returns_none_for_bare_name() {
        // A test name with no "::" has no workflow segment.
        assert_eq!(workflow_from_test_name("bare_test"), None);
    }

    #[test]
    fn scenario_from_test_name_returns_none_for_non_ux_prefix() {
        // Module not prefixed with "ux_scenario_" should not produce a scenario.
        assert_eq!(scenario_from_test_name("other_module::some_test"), None);
    }

    /// Dual repro completeness: for representative test names, the receipt
    /// contains both a non-empty `canonical_repro` (cargo test command) and
    /// a non-empty `friendly_repro` (just ux-tests shorthand).
    ///
    /// Feature: ux-readiness-system, Property 2: Dual repro completeness
    /// Validates: Requirements 0.4, 1.9
    #[test]
    fn dual_repro_completeness() -> Result<()> {
        let representative_tests = [
            ("ux_scenario_01_startup::start_server", "start_server"),
            ("ux_scenario_07_completion::completions", "completions"),
            (
                "ux_scenario_14_inc_conformance::scenario_14_include_path_completion_external_module",
                "scenario_14_include_path_completion_external_module",
            ),
            (
                "ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix",
                "scenario_19_diagnostics_clear_after_fix",
            ),
        ];

        for (full_test_name, expected_short) in representative_tests {
            let log = format!(
                "running 1 test\n\
                 test {full_test_name} ... FAILED\n\
                 assertion failed: left == right\n\
                 test result: FAILED. 0 passed; 1 failed"
            );

            let receipt = classify(&log, Some("deadbeef".to_string()));

            // canonical_repro must be Some and non-empty
            let canonical = receipt.canonical_repro.as_deref().unwrap_or("");
            assert!(
                !canonical.is_empty(),
                "canonical_repro should be non-empty for test {full_test_name}"
            );
            assert!(
                canonical.contains("cargo test -p perl-lsp-ux-tests"),
                "canonical_repro should contain 'cargo test -p perl-lsp-ux-tests', got: {canonical}"
            );
            assert!(
                canonical.contains(full_test_name),
                "canonical_repro should contain the full test name, got: {canonical}"
            );

            // friendly_repro must be Some and non-empty
            let friendly = receipt.friendly_repro.as_deref().unwrap_or("");
            assert!(
                !friendly.is_empty(),
                "friendly_repro should be non-empty for test {full_test_name}"
            );
            assert!(
                friendly.contains("just ux-tests"),
                "friendly_repro should contain 'just ux-tests', got: {friendly}"
            );
            assert!(
                friendly.contains(expected_short),
                "friendly_repro should contain the short test name '{expected_short}', got: {friendly}"
            );
        }

        Ok(())
    }

    #[test]
    fn panic_re_matches_modern_rust_format() {
        // Rust 1.73+ format: "panicked at path:row:col:" with no quoted message.
        let line = "thread 'test' panicked at crates/perl-lsp-rs/src/lib.rs:42:8:";
        let cap = PANIC_RE.captures(line).expect("should match modern panic format");
        assert_eq!(&cap[1], "crates/perl-lsp-rs/src/lib.rs:42:8");
    }

    // =========================================================================
    // Scenario 14 / 19 classifier fixture tests (Task 0.4)
    // =========================================================================

    /// Scenario 14 — external module resolution via `includePaths` fails.
    ///
    /// When goto-definition returns empty for a module that should resolve via
    /// `includePaths`, the test assertion produces an "assertion failed" log.
    /// The classifier should identify this as `ProviderRegression` because the
    /// LSP provider failed to resolve a module that the configuration says
    /// should be resolvable.
    #[test]
    fn classifier_extracts_scenario_14_external_module_failure() -> Result<()> {
        // Representative log: an assertion failure from scenario_14 when an
        // external module configured via includePaths fails to resolve.
        // The log contains "assertion failed" which the classifier maps to
        // ProviderRegression (module resolution provider did not honour config).
        let log = "\
running 1 test\n\
test ux_scenario_14_inc_conformance::scenario_14_include_path_completion_external_module ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_14_inc_conformance::scenario_14_include_path_completion_external_module stdout ----\n\
[conformance] mode=completion_external_module | PL701=PASS | goto-def=FAIL | hover=PASS\n\
assertion failed: left == right\n\
  left: false\n\
 right: true\n\
Expected goto-definition to resolve GreetModule from completion scenario; defs=[]\n\
\n\
failures:\n\
    ux_scenario_14_inc_conformance::scenario_14_include_path_completion_external_module\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";

        let receipt = classify(log, Some("fix14a".to_string()));

        assert!(
            matches!(receipt.failure_class, UxFailureClass::ProviderRegression),
            "scenario_14 external module failure should classify as ProviderRegression, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::ProviderFix, "ProviderRegression routes to ProviderFix");
        assert_eq!(
            receipt.scenario.as_deref(),
            Some("ux_scenario_14_inc_conformance.rs"),
            "scenario file should be extracted"
        );
        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking);
        assert_eq!(receipt.merge_action, "fix_provider");

        Ok(())
    }

    /// Scenario 14 — system `@INC` opt-in via `PERL5LIB` / `useSystemInc` fails.
    ///
    /// When the server cannot resolve a module via system `@INC` despite
    /// `useSystemInc: true`, the test assertion produces an "assertion failed"
    /// log. The classifier should identify this as `ProviderRegression` because
    /// the module resolution provider failed to honour the system `@INC`
    /// configuration.
    #[test]
    fn classifier_extracts_scenario_14_system_inc_opt_in_failure() -> Result<()> {
        // Representative log: an assertion failure from scenario_14 when the
        // server cannot resolve a module via system @INC despite useSystemInc
        // being enabled. The "assertion failed" content triggers ProviderRegression.
        let log = "\
running 1 test\n\
test ux_scenario_14_inc_conformance::scenario_14_system_inc ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_14_inc_conformance::scenario_14_system_inc stdout ----\n\
[conformance] mode=system_inc | PL701=FAIL | goto-def=FAIL | hover=PASS\n\
assertion failed: definition should resolve SystemModule.pm via system @INC (PERL5LIB); defs=[]\n\
\n\
failures:\n\
    ux_scenario_14_inc_conformance::scenario_14_system_inc\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";

        let receipt = classify(log, Some("fix14b".to_string()));

        assert!(
            matches!(receipt.failure_class, UxFailureClass::ProviderRegression),
            "scenario_14 system @INC failure should classify as ProviderRegression, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::ProviderFix, "ProviderRegression routes to ProviderFix");
        assert_eq!(
            receipt.scenario.as_deref(),
            Some("ux_scenario_14_inc_conformance.rs"),
            "scenario file should be extracted"
        );
        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some("ux_scenario_14_inc_conformance::scenario_14_system_inc"),
            "test name should be extracted"
        );
        assert_eq!(receipt.result, "fail");

        Ok(())
    }

    /// Scenario 19 — diagnostics race condition during edit lifecycle.
    ///
    /// When the diagnostics clear-after-fix check fails due to a race between
    /// pre-fix and post-fix diagnostic events, the log contains "race" in the
    /// diagnostic context. The classifier should identify this as `TestRace`
    /// because the failure is a non-deterministic timing issue in the test
    /// harness, not a provider regression.
    #[test]
    fn classifier_extracts_scenario_19_diagnostics_race_failure() -> Result<()> {
        let log = "\
running 1 test\n\
test ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix stdout ----\n\
diagnostics race: pre-fix events leaked into post-fix window\n\
Expected diagnostics to clear (or no new errors) after fixing the file; \
events: [Diagnostics { uri: \"file:///tmp/ws/live.pl\", diagnostics: [{\"code\":\"PL001\"}] }]\n\
note: this is a known flaky race condition in the diagnostics drain pipeline\n\
\n\
failures:\n\
    ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";

        let receipt = classify(log, Some("fix19".to_string()));

        assert!(
            matches!(receipt.failure_class, UxFailureClass::TestRace),
            "scenario_19 diagnostics race should classify as TestRace, got {:?}",
            receipt.failure_class
        );
        assert_eq!(receipt.route, UxRoute::TestFix, "TestRace routes to TestFix");
        assert_eq!(
            receipt.scenario.as_deref(),
            Some("ux_scenario_19_diagnostics_lifecycle.rs"),
            "scenario file should be extracted"
        );
        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some("ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix"),
            "test name should be extracted"
        );
        assert_eq!(receipt.result, "fail");
        assert!(receipt.blocking);
        assert_eq!(receipt.merge_action, "quarantine_or_fix_test");

        Ok(())
    }
    // ── #15988: budget-exceeded vs assertion-failed discrimination ──────────
    //
    // The gate fails identically whether a change broke a provider or a shared
    // runner let a latency budget expire. These tests pin the difference to
    // evidence inside each failing test's own block.

    /// Cargo's trailing failure report for two tests that failed for genuinely
    /// different reasons: one bounded wait expired, one assertion compared two real
    /// values and rejected the observed one.
    const TWO_FAILURES_LOG: &str = "running 2 tests\n\
test ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols ... FAILED\n\
test ux_scenario_14_inc_conformance::goto_definition_follows_include ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:352:5:\n\
workspace/symbol wait ended: deadline expired after 5000ms with the stream still live\n\
\n\
---- ux_scenario_14_inc_conformance::goto_definition_follows_include stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_14_inc_conformance.rs:88:5:\n\
assertion `left == right` failed: goto-definition must follow the include path\n\
  left: \"lib/App.pm\"\n\
 right: \"lib/Base.pm\"\n\
\n\
failures:\n\
    ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols\n\
    ux_scenario_14_inc_conformance::goto_definition_follows_include\n\
\n\
test result: FAILED. 0 passed; 2 failed; 0 ignored";

    #[test]
    fn discriminates_an_expired_budget_from_a_real_assertion_failure() {
        let receipt = classify(TWO_FAILURES_LOG, Some("sha".to_string()));
        assert_eq!(receipt.failing_tests.len(), 2, "both failing tests must appear");

        let budget = &receipt.failing_tests[0];
        assert_eq!(
            budget.name,
            "ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols"
        );
        assert_eq!(
            budget.mode,
            UxFailureMode::BudgetExceeded,
            "a WaitEnd::Deadline render means nothing about the change was decided"
        );
        assert!(budget.discriminated, "a deadline marker is evidence, not an inference");

        let regression = &receipt.failing_tests[1];
        assert_eq!(
            regression.name,
            "ux_scenario_14_inc_conformance::goto_definition_follows_include"
        );
        assert_eq!(
            regression.mode,
            UxFailureMode::AssertionFailed,
            "an assertion over two real values is the change's own failure"
        );
        assert!(regression.discriminated);
    }

    #[test]
    fn one_tests_wording_does_not_reclassify_another_tests_budget() {
        // This is the #15988 defect in miniature. Whole-log classification sees the
        // scenario-14 provider vocabulary and calls the entire run a provider
        // regression, which buries the expired budget in the other block.
        let receipt = classify(TWO_FAILURES_LOG, None);
        assert!(
            matches!(receipt.failure_class, UxFailureClass::ProviderRegression),
            "whole-run class is unchanged by this slice, and is exactly the signal that misleads"
        );
        assert_eq!(
            receipt.failing_tests[0].mode,
            UxFailureMode::BudgetExceeded,
            "per-test discrimination must survive the whole-run class"
        );
    }

    #[test]
    fn an_expired_budget_is_not_routed_to_update_the_baseline() -> Result<()> {
        // #16205, measured on job 106081131409. `infer_failure_class` scans the whole
        // log for substrings, so the word "baseline" anywhere in it — here a cache
        // step that has nothing to do with the failure — classified the run as
        // BaselineDrift and routed it to BaselineUpdate. On a latency probe that is
        // the one remedy the repository forbids: widening the budget until the noise
        // fits buries the non-determinism instead of reporting it.
        let log = "Restored baseline snapshot cache in 0.4s\n\
failures:\n\n\
---- ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols stdout ----\n\
wait ended: deadline expired after 5000ms with the stream still live\n\
assertion failed: !symbols.is_empty()\n\
\n\
failures:\n\
    ux_latency_raw_rpc::ux_latency_workspace_symbols_sees_open_document_symbols\n\
\n\
test result: FAILED. 0 passed; 1 failed";

        let receipt = classify(log, None);
        let Some(probe) = receipt.failing_tests.first() else {
            bail!("the log carries one failing test block; none was discriminated");
        };
        ensure!(
            probe.mode == UxFailureMode::BudgetExceeded,
            "the deadline marker is the evidence this rests on, got {:?}",
            probe.mode
        );
        ensure!(
            receipt.failure_class == UxFailureClass::Timeout,
            "a proven expired wait outranks an incidental `baseline` elsewhere in the log, got {:?}",
            receipt.failure_class
        );
        ensure!(
            receipt.route == UxRoute::TimeoutTriage,
            "the route must send triage at the flake, never at the baseline, got {:?}",
            receipt.route
        );
        ensure!(
            receipt.merge_action == "triage_timeout",
            "update_baseline is the forbidden remedy this test exists to prevent, got {}",
            receipt.merge_action
        );
        Ok(())
    }

    #[test]
    fn an_unresolved_co_failure_keeps_the_budget_from_deciding_the_run() -> Result<()> {
        // Devin Review on #16244. An unresolved failure is the ABSENCE of
        // evidence, not evidence of a flake, so one test's deadline marker must
        // not speak for it. Here test B is a real baseline failure whose block
        // carries no marker this classifier will read; if the budget in test A
        // decided the run, B would lose `update_baseline` for having run beside
        // a slow probe.
        let log = "failures:\n\n\
---- ux_latency_raw_rpc::hover stdout ----\n\
wait ended: deadline expired after 5000ms with the stream still live\n\
\n\
---- ux_scenario_31_snapshot::workspace_symbols stdout ----\n\
baseline snapshot mismatch for workspace_symbols\n\
\n\
failures:\n\
    ux_latency_raw_rpc::hover\n\
    ux_scenario_31_snapshot::workspace_symbols\n\
\n\
test result: FAILED. 0 passed; 2 failed";

        let receipt = classify(log, None);
        let [probe, snapshot] = receipt.failing_tests.as_slice() else {
            bail!(
                "the log carries exactly two failing test blocks, got {}",
                receipt.failing_tests.len()
            );
        };
        ensure!(
            probe.mode == UxFailureMode::BudgetExceeded,
            "the first block's deadline marker is what the run would wrongly follow, got {:?}",
            probe.mode
        );
        ensure!(
            !snapshot.discriminated,
            "the second block carries no evidence this classifier backs, got {:?}",
            snapshot.mode
        );
        ensure!(
            receipt.failure_class != UxFailureClass::Timeout,
            "an unresolved co-failure must stop the budget deciding the whole run"
        );
        ensure!(
            receipt.merge_action != "triage_timeout",
            "and must not take update_baseline away from a real baseline failure"
        );
        Ok(())
    }

    #[test]
    fn a_starvation_failure_is_not_routed_to_update_the_baseline() -> Result<()> {
        // #16205 rows two and three, measured on the 2026-09-20 UX runs: a
        // completion-quality failure whose block says the receiver returned no
        // candidates, beside gate prose that mentions a baseline. #16244 taught the
        // run class to trust a proven expired budget; the fallback for every other
        // shape still scanned the whole log, so the same stray token routed this
        // shape to `update_baseline` — the one remedy the repository forbids for a
        // candidate that returned nothing.
        let log = "Restored baseline snapshot cache in 0.4s\n\
failures:\n\n\
---- ux_scenario_52_test_inline_completion_quality_receipt::scenario_52_test_inline_completion_quality_receipt stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_52_test_inline_completion_quality_receipt.rs:97:5:\n\
assertion failed: $self receiver inline completion returned candidates; got []\n\
\n\
test result: FAILED. 0 passed; 1 failed";

        let receipt = classify(log, None);
        ensure!(
            !matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "a token from outside the failing block must not classify it as BaselineDrift, got {:?}",
            receipt.failure_class
        );
        ensure!(
            receipt.merge_action != "update_baseline",
            "a starvation failure must not be told to widen a baseline it does not have, got {}",
            receipt.merge_action
        );
        Ok(())
    }

    #[test]
    fn a_real_assertion_keeps_its_class_even_beside_an_expired_budget() -> Result<()> {
        // The converse guard. One test's expired wait says nothing about another
        // test's assertion over two real values, so a budget must not launder a
        // genuine regression into a timeout. TWO_FAILURES_LOG carries both.
        let receipt = classify(TWO_FAILURES_LOG, None);
        let [budget, assertion] = receipt.failing_tests.as_slice() else {
            bail!(
                "TWO_FAILURES_LOG carries a budget and an assertion, got {}",
                receipt.failing_tests.len()
            );
        };
        ensure!(
            budget.mode == UxFailureMode::BudgetExceeded,
            "the first block is the expired wait, got {:?}",
            budget.mode
        );
        ensure!(
            assertion.mode == UxFailureMode::AssertionFailed,
            "the second block is a real assertion over two values, got {:?}",
            assertion.mode
        );
        ensure!(
            receipt.failure_class == UxFailureClass::ProviderRegression,
            "positive evidence that the change is the subject outranks the budget, got {:?}",
            receipt.failure_class
        );
        Ok(())
    }

    #[test]
    fn an_expired_budget_outranks_the_assertion_it_caused() {
        // A wait that expires usually still ends in an assertion over the empty
        // result. The budget is the cause; reporting the assertion would name the
        // symptom and point triage at the change.
        let log = "failures:\n\n\
---- ux_latency_raw_rpc::hover stdout ----\n\
wait ended: deadline expired after 5000ms with the stream still live\n\
assertion failed: !hovers.is_empty()\n\
\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.failing_tests[0].mode, UxFailureMode::BudgetExceeded);
        assert_eq!(
            receipt.failing_tests[0].evidence.as_deref(),
            Some("wait ended: deadline expired after 5000ms with the stream still live"),
            "the receipt shows the line it read"
        );
    }

    #[test]
    fn an_assertion_on_an_empty_observation_is_reported_as_ambiguous() {
        // Nothing in this block can tell a real regression from a budget that
        // expired before the observation arrived. Saying so is the honest output.
        let log = "failures:\n\n\
---- ux_latency_raw_rpc::symbols stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:352:5:\n\
assertion failed: workspace/symbol must find alpha from an opened e2e document; got []\n\
\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, None);
        assert_eq!(
            receipt.failing_tests[0].mode,
            UxFailureMode::AssertionOnAbsentObservation,
            "an empty observed value is the shape both causes produce"
        );
        assert!(
            !receipt.failing_tests[0].discriminated,
            "an ambiguous block must not claim to be discriminated"
        );
    }

    #[test]
    fn a_bare_panic_is_not_called_an_assertion() {
        let log = "failures:\n\n\
---- ux_scenario_01_startup::start stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_01_startup.rs:12:9:\n\
called `Option::unwrap()` on a `None` value\n\
\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.failing_tests[0].mode, UxFailureMode::Panic);
    }

    #[test]
    fn the_last_block_does_not_inherit_the_run_summary() {
        // Without trimming, the trailing `failures:` list and `test result:` line
        // land inside the final block and can supply evidence the test never wrote.
        let log = "failures:\n\n\
---- ux_latency_raw_rpc::only stdout ----\n\
some detail with no verdict in it\n\
\n\
failures:\n    ux_latency_raw_rpc::only\n\
\n\
test result: FAILED. 0 passed; 1 failed; timed out after 60s";
        let receipt = classify(log, None);
        assert_eq!(
            receipt.failing_tests[0].mode,
            UxFailureMode::Unknown,
            "the summary's wording is not this test's evidence"
        );
    }

    #[test]
    fn failing_tests_is_empty_on_a_passing_run() {
        let log = "running 3 tests\ntest ux_scenario_01_startup::start ... ok\ntest result: ok. 3 passed; 0 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.result, "pass");
        assert!(receipt.failing_tests.is_empty(), "a passing run discriminates nothing");
    }

    #[test]
    fn failures_without_stdout_blocks_are_named_but_not_classified() {
        let log = "running 1 test\ntest ux_scenario_01_startup::start ... FAILED\ntest result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.failing_tests.len(), 1);
        assert_eq!(receipt.failing_tests[0].name, "ux_scenario_01_startup::start");
        assert_eq!(receipt.failing_tests[0].mode, UxFailureMode::Unknown);
        assert!(
            !receipt.failing_tests[0].discriminated,
            "no block means no evidence; the whole-run class must not be borrowed here"
        );
    }

    #[test]
    fn schema_version_records_the_additive_field() {
        let receipt = classify("test result: ok. 1 passed; 0 failed", None);
        assert_eq!(
            receipt.schema_version, 2,
            "consumers pinned to version 1 keep every field they already read"
        );
    }

    #[test]
    fn panic_location_does_not_reach_past_the_first_failing_test() {
        // #16148: the first failing test fails with a plain assertion while a
        // later test panics. The later test's crash site must not be reported
        // under the first test's name.
        let log = "running 2 tests\n\
test ux_scenario_02_open::open_file ... FAILED\n\
test ux_scenario_03_diag::diag_test ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_02_open::open_file stdout ----\n\
assertion `left == right` failed\n\
  left: 1\n\
 right: 2\n\
\n\
---- ux_scenario_03_diag::diag_test stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_03_diag.rs:77:9:\n\
boom\n\
\n\
failures:\n\
    ux_scenario_02_open::open_file\n\
    ux_scenario_03_diag::diag_test\n\
\n\
test result: FAILED. 0 passed; 2 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.first_failing_test.as_deref(), Some("ux_scenario_02_open::open_file"));
        assert!(
            receipt.panic_location.is_none(),
            "a later test's panic site must not be reported under the first test's name, got {:?}",
            receipt.panic_location
        );
        assert!(
            matches!(receipt.failure_class, UxFailureClass::NewTestBug),
            "whole-run class is unchanged by this fix, got {:?}",
            receipt.failure_class
        );
    }

    #[test]
    fn panic_location_reports_the_first_failing_tests_own_panic() {
        let log = "running 2 tests\n\
test ux_scenario_02_open::open_file ... FAILED\n\
test ux_scenario_03_diag::diag_test ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_02_open::open_file stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_02_open.rs:41:5:\n\
boom\n\
\n\
---- ux_scenario_03_diag::diag_test stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_scenario_03_diag.rs:77:9:\n\
boom\n\
\n\
failures:\n\
    ux_scenario_02_open::open_file\n\
    ux_scenario_03_diag::diag_test\n\
\n\
test result: FAILED. 0 passed; 2 failed";
        let receipt = classify(log, None);
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("crates/perl-lsp-ux-tests/tests/ux_scenario_02_open.rs:41:5")
        );
    }
}
