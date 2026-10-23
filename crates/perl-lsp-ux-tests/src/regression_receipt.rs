use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::cargo_failure;
use crate::taxonomy::{UxComponent, UxFailureClass, UxRoute, route_for_failure_class};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

// The mechanical read of cargo's failing-test report — result lines, stdout
// block spans, panic locations — is owned by [`crate::cargo_failure`] and
// shared with the merge-gate first-failure reader. This module keeps only the
// interpretation: which failure decides the class, and what the receipt says
// about it (#16907).
//
// `WaitEnd::Deadline` is the one harness outcome documented to mean "nothing
// decided": a live stream that simply did not produce the awaited observation in
// time. `WaitEnd::describe` renders it with this exact wording, so matching it is
// evidence from the harness rather than a guess about the wording of a panic.
// See crates/perl-lsp-ux-tests/src/observation.rs.
const DEADLINE_MARKER: &str = "deadline expired after";

#[derive(Debug, Clone)]
pub struct UxRegressionReceiptConfig {
    pub input: PathBuf,
    pub receipt: Option<PathBuf>,
    pub sha: Option<String>,
    pub exit_status_file: Option<PathBuf>,
}

/// Data for the command that presents the receipt to its user.
pub enum UxRegressionReceiptOutput {
    Written(PathBuf),
    Payload(String),
}

impl fmt::Display for UxRegressionReceiptOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Written(path) => {
                write!(formatter, "Wrote UX regression receipt: {}", path.display())
            }
            Self::Payload(payload) => formatter.write_str(payload),
        }
    }
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

pub fn run(config: UxRegressionReceiptConfig) -> Result<UxRegressionReceiptOutput> {
    validate_patterns()?;
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
        Ok(UxRegressionReceiptOutput::Written(path))
    } else {
        Ok(UxRegressionReceiptOutput::Payload(payload))
    }
}

fn validate_patterns() -> Result<()> {
    // The receipt owns no cargo-output pattern of its own. It still checks the
    // shared reader's before classifying, because a pattern that failed to
    // compile answers "no failing test" — indistinguishable from a clean run.
    cargo_failure::validate_patterns().map_err(|error| anyhow::anyhow!("UX receipt {error}"))
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
        lines.iter().find_map(|line| cargo_failure::failed_test_name(line).map(str::to_string));
    let panic_location =
        first_failing_test.as_ref().and_then(|name| panic_location_for_test(raw, name));
    let scenario = first_failing_test.as_ref().and_then(|name| scenario_from_test_name(name));
    let namable = first_failing_test.as_ref().filter(|name| can_name_a_command(name));
    let workflow = namable.and_then(|name| workflow_from_test_name(name));

    let failing_tests = discriminate_failing_tests(raw);

    let canonical_repro = namable.map(|name| {
        format!("cargo test -p perl-lsp-ux-tests {name} -- --test-threads=1 --nocapture")
    });

    let friendly_repro = namable.map(|name| {
        // Extract just the test function name (after ::) for the shorthand command.
        let short = name.split("::").last().unwrap_or(name);
        format!("just ux-tests {short}")
    });

    let has_failed_test = first_failing_test.is_some()
        || lines.iter().any(|line| line.contains("test result: FAILED"));
    let has_passing_summary = lines.iter().any(|line| line.contains("test result: ok"));
    let command_succeeded = exit_status.map(|status| status == 0).unwrap_or(true);
    let result =
        if command_succeeded && has_passing_summary && !has_failed_test { "pass" } else { "fail" }
            .to_string();
    let blocking = result != "pass";

    // A passing run asserts no failure, so its receipt must not classify one.
    // `run_failure_class` falls through to the whole-log word scan when no test
    // failed, and a green log mentions "baseline" on every run (`Compiling
    // perl-lsp-ux-baselines` is enough), which claimed `BaselineDrift` and
    // routed a passing run to `update_baseline` — advice to move a baseline on
    // a run that failed nothing (#16287). `Unknown`/`Triage` is the honest
    // neutral: it says nothing rather than something wrong, keeps both field
    // types, and nothing routes on the class on a pass because `merge_action`
    // is already `merge_allowed`.
    let failure_class = if result == "pass" {
        UxFailureClass::Unknown
    } else {
        run_failure_class(&failing_tests, raw)
    };
    let route = route_for_failure_class(failure_class);
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
        }
    }
    .to_string();
    let human_summary = if result == "pass" {
        "UX regression passed; merge_allowed.".to_string()
    } else {
        let test = first_failing_test.as_deref().unwrap_or("unknown_test");
        let repro = canonical_repro.as_deref().unwrap_or("see ux-regression.log");
        let discrimination = first_failing_test
            .as_deref()
            .and_then(|name| failing_tests.iter().find(|failing| failing.name == name))
            .map(describe_discrimination)
            .unwrap_or_default();
        format!(
            "UX regression failed in {test}; classified as {failure_class:?}{discrimination}; repro: {repro}"
        )
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
/// (#16148) so there is exactly one span implementation. The spans themselves
/// are read by [`cargo_failure::failure_block_spans`], which the merge-gate
/// first-failure reader also uses (#16907).
fn failure_block_spans(raw: &str) -> Vec<(String, usize, usize)> {
    cargo_failure::failure_block_spans(raw)
        .into_iter()
        .map(|block| (block.name, block.body_start, block.body_end))
        .collect()
}

/// The panic site of one named failing test, read from that test's own stdout
/// block only (#16148). The receipt is flat, so adjacent fields read as one
/// pair: a whole-log scan reports a later test's crash site under the first
/// test's name when the first test fails without panicking.
fn panic_location_for_test(raw: &str, name: &str) -> Option<String> {
    let block =
        cargo_failure::failure_block_spans(raw).into_iter().find(|block| block.name == name)?;
    let body = block_body(block.body(raw));
    // A line that resolves to no column, or to a path outside the receipt's
    // grammar, is not one it can report — but that must skip *that line* and
    // keep scanning, or a single column-less panic earlier in the block would
    // discard a later, reportable one.
    body.lines().find_map(|line| {
        let location = cargo_failure::panic_location(line)?;
        (location.column.is_some() && cargo_failure::is_plausible_path(&location.path))
            .then(|| location.with_column())
    })
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

    // Cargo reported these failures but printed no stdout block for them. Name them
    // and admit the mode is unknown rather than borrowing the whole-log class.
    //
    // This runs even when other failures did produce blocks. A blockless failure the
    // array silently dropped was invisible to every consumer: to `first_failing_test`'s
    // own lookup, which then printed no per-test clause at all instead of saying
    // `not discriminated`, and to `no_failing_test_compared_anything`, which would
    // have read a run as crash-only while an unexplained failure sat beside the
    // crash. Raised in review as `#discussion_r4058216209`.
    for name in cargo_failure::failed_test_names(raw) {
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

/// Say, in the one sentence a reader actually sees, what the first failing test's
/// own block proved.
///
/// `failure_class` is read from the whole log, so it answers a question nobody asked:
/// #16103 records a run where an unrelated one-file diff was published as
/// `provider_regression` / `fix_provider`, and a later one where the same shape came
/// back as `timeout` — the arm taken depends on which words happen to appear
/// elsewhere in the log. `failing_tests` already carries the per-block reading that
/// can tell a spent budget from a rejected observation, and it reaches the receipt
/// file only. The check surface prints `human_summary`, so the discrimination has to
/// travel in there to be read at all.
///
/// An undiscriminated mode is reported as such rather than dropped. "This log did not
/// say" is the honest answer, and it is the one that tells a triager to open the log
/// instead of trusting the class beside it.
fn describe_discrimination(failing: &UxFailingTest) -> String {
    let mode = match failing.mode {
        UxFailureMode::BudgetExceeded => {
            "a bounded wait expired, so nothing about the change was decided"
        }
        UxFailureMode::AssertionFailed => "an assertion rejected an observed value",
        UxFailureMode::AssertionOnAbsentObservation => {
            "an assertion failed on an absent observation, which a regression and a spent budget both produce"
        }
        UxFailureMode::Panic => "the test panicked without an assertion",
        UxFailureMode::Unknown => "its block carried no evidence this classifier reads",
    };
    match (failing.discriminated, failing.evidence.as_deref()) {
        (true, Some(evidence)) => format!("; per-test evidence: {mode} ({})", evidence.trim()),
        (true, None) => format!("; per-test evidence: {mode}"),
        (false, _) => format!("; per-test evidence: not discriminated — {mode}"),
    }
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

/// Removes the lines a scenario's own diagnostic detail block encloses.
///
/// A detail block describes a scenario's internals and may contain any word,
/// so letting it reach a substring scan lets it veto or reroute a verdict the
/// real evidence already decided. Shared by `classification_input` and
/// `failing_test_own_input` so the whole-log and per-test readers cannot drift
/// apart on this rule (#16713).
fn strip_diagnostic_detail(raw: &str) -> String {
    let mut in_detail = false;
    let mut retained = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("UX_SCENARIO_DETAIL_BEGIN:") {
            in_detail = true;
        } else if trimmed == "UX_SCENARIO_DETAIL_END" {
            in_detail = false;
        } else if !in_detail {
            retained.push(line);
        }
    }
    retained.join("\n")
}

/// The text `infer_failure_class` is allowed to read.
///
/// Two kinds of line are removed because they describe something other than the
/// failure: a scenario's own diagnostic detail block, and the result line of a test
/// that passed or was skipped. Both are present on every run and neither says
/// anything about why this run failed.
///
/// libtest prints one result line per test, unconditionally, and cargo tees the
/// whole run into the log the class is read from. A test that passed contributes
/// exactly that one line — its own name — because its stdout is captured. Those
/// names are not evidence about the test that failed, and they decided the class:
/// the single UX test function whose name contains `baseline` passes on every run,
/// which routed unrelated failures to `update_baseline` (#16103). libtest spells a
/// pass `ok` and a skip `ignored` in lower case and a failure `FAILED` in upper, so
/// the failing test's own line survives this filter.
fn classification_input(raw: &str) -> String {
    strip_diagnostic_detail(raw)
        .lines()
        .filter(|line| {
            !matches!(
                cargo_failure::result_line(line).map(|(_, outcome)| outcome),
                Some(cargo_failure::TestOutcome::Passed | cargo_failure::TestOutcome::Ignored)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
/// One ambiguous case is not left alone, because its remedy is actively unsafe.
/// `BaselineDrift` routes to `update_baseline`, which tells a reader to accept the
/// observed value as the new expectation. A test that panicked produced no observed
/// value, so there is nothing to accept, and following that instruction would widen
/// a budget or rewrite a snapshot on the strength of a crash. When every failing
/// test is a proven crash (`no_failing_test_compared_anything`), the baseline
/// verdict is therefore withdrawn in favour of `Unknown`, which routes to
/// `Triage`. That is deliberately not a claim about what did go wrong — naming the
/// right class for a panic is the open taxonomy question in #16103 — only that this
/// run cannot be answered with a baseline. `blocking` does not read the class, so
/// the gate still fails the run either way.
///
/// The ambiguous cases therefore keep whatever the whole-log scan already gave
/// them. That scan is unreliable, which is the defect behind #16205, but this
/// claim is only that proven budget evidence should beat it. Widening the claim
/// to cases the evidence cannot settle would be guessing with more steps.
fn run_failure_class(failing_tests: &[UxFailingTest], raw: &str) -> UxFailureClass {
    let every_failure_is_an_expired_budget = !failing_tests.is_empty()
        && failing_tests.iter().all(|test| test.mode == UxFailureMode::BudgetExceeded);

    if every_failure_is_an_expired_budget {
        return UxFailureClass::Timeout;
    }

    let scanned = infer_failure_class(&classification_input(raw));

    if scanned == UxFailureClass::BaselineDrift && no_failing_test_compared_anything(failing_tests)
    {
        return UxFailureClass::Unknown;
    }

    // #16609. `scanned` may have found its word in prose that belongs to no failing
    // test — a cache step, a crate being compiled, an unrelated line — while every
    // failing test did print a block that never mentions a baseline. Then nothing
    // about this run is a comparison to accept, and `update_baseline` is the unsafe
    // remedy: it tells a reader to widen a budget or move a number that was never
    // the thing that changed.
    //
    // The fallback is re-read from the failing tests' own evidence rather than
    // simply withdrawn, because these blocks usually do name the failure.
    //
    // Scoping the scan to the blocks outright was tried and rejected (#16358): a
    // failure cargo printed no block for leaves the run with nothing to read, and
    // the real baseline comparison it hid would lose its remedy. So the whole-log
    // scan keeps the last word whenever ANY failing test is unattributed, and only
    // steps aside when every one of them is accounted for.
    if scanned == UxFailureClass::BaselineDrift && every_failing_test_is_attributed(failing_tests) {
        let own = failing_test_own_input(raw, failing_tests);
        if !mentions_drift(&own.to_ascii_lowercase()) {
            return infer_failure_class(&own);
        }
    }

    scanned
}

/// True when every failing test is a proven crash.
///
/// A baseline or snapshot failure is an assertion: two values were produced and one
/// was rejected. `classify_failure_mode` records an assertion in a block as
/// `AssertionFailed` or `AssertionOnAbsentObservation`, and reaches `Panic` only
/// after reading the whole block and finding no assertion line at all. So a run
/// whose every failing test is `Panic` contains no comparison, and the word that
/// produced `BaselineDrift` came from somewhere in the log that did not fail.
///
/// Every other mode leaves the class alone, and `Unknown` is the one worth naming.
/// It does not mean "no comparison" — it means `classify_failure_mode` recognised
/// no marker, which is also what a failure with no stdout block at all produces. An
/// unrecognised baseline mismatch is exactly that shape, so reading `Unknown` as
/// corroboration would take `update_baseline` away from a real baseline failure on
/// the strength of having learned nothing about it. Review raised that
/// (`#discussion_r4058216213`) against a first version that admitted `Unknown`
/// alongside `Panic`; requiring affirmative evidence from every failing test is the
/// same precedence `run_failure_class` already applies to budgets one level up.
/// `AssertionFailed` beside a panic may well be the baseline comparison the class
/// names, and `BudgetExceeded` returns at the deadline marker without reading
/// further, so such a block is not known to be free of an assertion either.
fn no_failing_test_compared_anything(failing_tests: &[UxFailingTest]) -> bool {
    !failing_tests.is_empty() && failing_tests.iter().all(|test| test.mode == UxFailureMode::Panic)
}

/// The two words that name a baseline or snapshot comparison.
///
/// Shared with `infer_failure_class`'s own `BaselineDrift` arm so that deciding
/// whether a run's evidence contains drift and deciding that the same evidence
/// produces the class cannot drift apart from one another.
fn mentions_drift(lower: &str) -> bool {
    lower.contains("baseline") || lower.contains("snapshot")
}

/// True when every failing test is attributed to evidence of its own.
///
/// `discriminate_failing_tests` records `Unknown` both for a failure cargo printed
/// no stdout block for and for a block carrying no marker this classifier reads.
/// Either way the log said nothing about that test, so the whole-run scan is the
/// only remaining thing that can speak for it — and one such failure is enough to
/// leave the scan in charge, because the others' blocks are then not the whole
/// story. This is the reason a run-class guard cannot simply re-read the blocks.
fn every_failing_test_is_attributed(failing_tests: &[UxFailingTest]) -> bool {
    !failing_tests.is_empty()
        && failing_tests.iter().all(|test| test.mode != UxFailureMode::Unknown)
}

/// The text the failing tests account for themselves: each one's own `... FAILED`
/// result line and its own stdout block, run-level trailer trimmed.
///
/// This is the failing-test-local counterpart of `classification_input`, which
/// still admits every line belonging to no test at all — a cache step, a crate
/// name being compiled, a path. Those lines are how a stray `baseline` reached a
/// run whose failing test had no baseline to move. The test's own result line is
/// kept because the line that reports a failing test is its own evidence, exactly
/// as `classify_keeps_the_failing_test_own_result_line_as_evidence` requires.
fn failing_test_own_input<'a>(raw: &'a str, failing_tests: &[UxFailingTest]) -> String {
    let mut owned: Vec<&'a str> = Vec::new();

    for line in raw.lines() {
        let Some(name) = cargo_failure::failed_test_name(line) else {
            continue;
        };
        if failing_tests.iter().any(|test| test.name == name) {
            owned.push(line);
        }
    }

    for (name, start, end) in failure_block_spans(raw) {
        if !failing_tests.iter().any(|test| test.name == name) {
            continue;
        }
        if let Some(block) = raw.get(start..end) {
            owned.push(block_body(block));
        }
    }

    // A detail block inside a failing test's stdout is exactly as inert as one
    // anywhere else in the log: `classification_input` never lets it reach a
    // substring scan, and neither may the per-test re-read, or a detail line
    // mentioning `baseline` would veto the local fallback the whole-log reader
    // already applied (#16713).
    strip_diagnostic_detail(&owned.join("\n"))
}

fn infer_failure_class(raw: &str) -> UxFailureClass {
    let lower = raw.to_ascii_lowercase();
    if looks_like_scenario_19_race(&lower) {
        UxFailureClass::TestRace
    } else if looks_like_scenario_14_provider_regression(&lower) {
        UxFailureClass::ProviderRegression
    } else if lower.contains("fixture matrix") || lower.contains("matrix drift") {
        UxFailureClass::MatrixDrift
    } else if mentions_drift(&lower) {
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

/// Whether a test name can be turned into a `workflow` and a runnable repro.
///
/// Those fields are built from a `::`-delimited Rust test path, and every name
/// libtest prints is whitespace-free except a doctest's
/// `<file> - <path> (line N)`. Splitting that one on `::` yields the
/// meaningless `path (line 12)`, and interpolating it into a command yields
/// `cargo test -p perl-lsp-ux-tests src/lib.rs - item::path (line 12) -- …`,
/// which no shell runs. Deriving them anyway would replace an honest absence
/// with a confident wrong answer, so a name this receipt cannot name a command
/// for contributes no workflow and no repro (#16907).
fn can_name_a_command(test: &str) -> bool {
    !test.is_empty() && !test.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::{bail, ensure};

    /// The `file:line:column` the shared cargo-output reader recovers from a
    /// panic line, formatted the way the receipt has always reported it. The
    /// reader and its grammar moved to [`cargo_failure`] (#16907); the receipt's
    /// own two acceptance rules — a column must be present, and the path must
    /// satisfy the receipt's grammar — did not, so this mirrors the production
    /// filter in `panic_location_for_test` exactly.
    fn panic_location_str(line: &str) -> Option<String> {
        let location = cargo_failure::panic_location(line)?;
        (location.column.is_some() && cargo_failure::is_plausible_path(&location.path))
            .then(|| location.with_column())
    }

    #[test]
    fn receipt_reads_cargo_output_through_the_shared_reader() {
        // The receipt's own patterns moved to `cargo_failure` (#16907). It must
        // still refuse to classify if that reader's patterns do not compile.
        assert!(cargo_failure::validate_patterns().is_ok());
    }

    /// A doctest's name is libtest's `<file> - <path> (line N)`, so it contains
    /// spaces. Cargo prints that same name on both the `... FAILED` result line
    /// and the `---- <name> stdout ----` block header, so the two are the same
    /// identity and a reader that cannot hold spaces reports a *different* test
    /// than the one that failed.
    ///
    /// Verbatim shape of a real failing doctest run (#16907).
    const FAILING_DOCTEST_LOG: &str = r#"running 1 test
test src/lib.rs - item::path (line 12) ... FAILED

failures:

---- src/lib.rs - item::path (line 12) stdout ----
thread 'item::path' panicked at src/lib.rs:12:9:
assertion `left == right` failed
  left: 1
 right: 2

failures:
    src/lib.rs - item::path (line 12)

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;

    /// The doctest's name, exactly as cargo prints it on both the result line
    /// and the block header.
    const DOCTEST_NAME: &str = "src/lib.rs - item::path (line 12)";

    #[test]
    fn doctest_failure_keeps_the_whole_spaced_name() {
        let receipt = classify(FAILING_DOCTEST_LOG, Some("abc123".to_string()));

        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some("src/lib.rs - item::path (line 12)"),
            "a doctest name contains spaces, so truncating it names a test that did not fail"
        );
        let names: Vec<&str> =
            receipt.failing_tests.iter().map(|test| test.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["src/lib.rs - item::path (line 12)"],
            "the stdout block header carries the same spaced name, so it must agree with the \
             result line rather than produce a second, shorter identity"
        );
    }

    /// Naming the test correctly is only half the receipt. The fields derived
    /// from the name — `workflow`, and the two repro commands — are built for a
    /// `::`-delimited Rust test path. A doctest name is neither, and splitting
    /// `src/lib.rs - item::path (line 12)` on `::` yields `path (line 12)`.
    /// Deriving them anyway would turn an honest absence into a confident wrong
    /// answer, so a name that cannot be named as a command contributes none.
    #[test]
    fn doctest_name_yields_no_workflow_and_no_runnable_repro() {
        let receipt = classify(FAILING_DOCTEST_LOG, Some("abc123".to_string()));

        assert_eq!(receipt.scenario, None, "not a UX scenario test");
        assert_eq!(
            receipt.workflow, None,
            "`path (line 12)` is not a workflow; a name with spaces must not be split into one"
        );
        assert_eq!(
            receipt.canonical_repro, None,
            "`cargo test … src/lib.rs - item::path (line 12) -- …` is not a runnable command"
        );
        assert_eq!(receipt.friendly_repro, None, "nor is `just ux-tests path (line 12)`");
        // The failure itself is still reported — refusing to invent a command is
        // not the same as losing the failure.
        assert_eq!(receipt.first_failing_test.as_deref(), Some(DOCTEST_NAME));
    }

    /// A log written on Windows ends every line with `\r\n`. `$` under `(?m)`
    /// sits before the `\n`, so a block header must tolerate the `\r` or every
    /// block goes unread while the result line still parses — a receipt that
    /// names the failing test and then says nothing about it.
    #[test]
    fn a_crlf_log_still_yields_its_block_and_panic() {
        let crlf = FAILING_DOCTEST_LOG.replace('\n', "\r\n");
        let receipt = classify(&crlf, Some("abc123".to_string()));

        assert_eq!(
            receipt.first_failing_test.as_deref(),
            Some(DOCTEST_NAME),
            "a result line must survive CRLF"
        );
        assert_eq!(receipt.failing_tests.len(), 1, "the stdout block header must survive CRLF too");
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("src/lib.rs:12:9"),
            "and the block must still carry its own panic"
        );
    }

    /// A column-less panic line is one the receipt cannot report, but it must
    /// skip that line rather than abandon the test's whole block.
    #[test]
    fn a_column_less_panic_does_not_hide_a_later_reportable_one() {
        let log = "running 1 test\ntest tasks::a::b ... FAILED\n\nfailures:\n\n\
---- tasks::a::b stdout ----\n\
thread 'a' panicked at src/lib.rs:42:\n\
thread 'b' panicked at src/other.rs:7:3:\n\
\n\
failures:\n    tasks::a::b\n\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let receipt = classify(log, Some("abc123".to_string()));
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("src/other.rs:7:3"),
            "the second panic carries a column and is the one the receipt reports"
        );
    }

    /// A pre-1.73 quoted panic whose message carries no colon must still reach
    /// the receipt. The shared reader's strict ≥1.73 parse *succeeds* on such a
    /// line with the whole `'<message>', path` token as the path, the receipt's
    /// path grammar refuses that token, and the location goes silent — where the
    /// receipt's predecessor reported `src/lib.rs:42:5`. (A message whose only
    /// colons are Rust's `::` escapes by accident: those colons make the strict
    /// parse's line field non-numeric, so the fallback fires anyway.)
    /// `panic_location` therefore has to skip the strict attempt for a quoted
    /// payload (#16907 review).
    #[test]
    fn a_colon_free_pre_1_73_message_still_reports_its_location() {
        let log = "running 1 test\n\
test ux_scenario_01_startup::start ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_01_startup::start stdout ----\n\
thread 'main' panicked at 'explicit panic', src/lib.rs:42:5:\n\
\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("pre173".to_string()));
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("src/lib.rs:42:5"),
            "the quoted message must not swallow the panic location behind it"
        );
    }

    /// The gate reports whatever `path:line` a panic printed; the receipt has a
    /// stricter path grammar. Sharing the parse must not make the gate stricter,
    /// because `ci_explain` classifies on `site.is_some()` — a rejected path
    /// turns a code regression into `unknown`.
    #[test]
    fn a_path_the_receipt_refuses_is_still_parsed_for_the_gate() -> anyhow::Result<()> {
        let line = "thread 'x' panicked at 9lives/src/lib.rs:42:8:";
        let location = cargo_failure::panic_location(line).ok_or_else(|| {
            anyhow::anyhow!("the shared reader parses path:line:column structurally: {line:?}")
        })?;
        assert_eq!(location.line_only(), "9lives/src/lib.rs:42");
        assert!(
            !cargo_failure::is_plausible_path(&location.path),
            "and the receipt is free to refuse to report it"
        );
        Ok(())
    }

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
    fn panic_re_matches_modern_rust_format() -> Result<()> {
        // Rust 1.73+ format: "panicked at path:row:col:" with no quoted message.
        let line = "thread 'test' panicked at crates/perl-lsp-rs/src/lib.rs:42:8:";
        let location =
            panic_location_str(line).ok_or_else(|| anyhow::anyhow!("modern panic format"))?;
        assert_eq!(&location, "crates/perl-lsp-rs/src/lib.rs:42:8");
        Ok(())
    }

    #[test]
    fn panic_re_matches_absolute_path_panic() -> Result<()> {
        // Absolute Unix-style paths are what rustc prints when the panicking
        // frame is not under the workspace root (a dependency's own `unwrap`, a
        // `registry/src/...` frame, or any build whose `CARGO_MANIFEST_DIR` is
        // not a prefix of the compiled file). The first-character class used to
        // be `[a-zA-Z]`, so these lines never matched and `panic_location` was
        // silently absent — exactly the missing-evidence bug #16147 names.
        let line = "thread 'x' panicked at /home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1:";
        let location =
            panic_location_str(line).ok_or_else(|| anyhow::anyhow!("absolute panic path"))?;
        assert_eq!(&location, "/home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1");
        Ok(())
    }

    #[test]
    fn panic_re_matches_dot_relative_path_panic() -> Result<()> {
        // `./`-relative paths appear from some toolchain and vendoring
        // configurations. Like the absolute case above, the old regex refused
        // to match because `.` is not a letter.
        let line = "thread 'x' panicked at ./xtask/src/a.rs:7:1:";
        let location =
            panic_location_str(line).ok_or_else(|| anyhow::anyhow!("relative panic path"))?;
        assert_eq!(&location, "./xtask/src/a.rs:7:1");
        Ok(())
    }

    #[test]
    fn panic_re_matches_cargo_registry_panic() -> Result<()> {
        // The case the digest fails hardest on is a panic inside a
        // dependency, which is exactly where the reader has the least context
        // to diagnose from the test name alone. The registry path lives under
        // an absolute prefix (`/root/.cargo/...`) so the old `[a-zA-Z]` anchor
        // rejected it on the leading `/`.
        let line = "thread 'x' panicked at /root/.cargo/registry/src/index/foo-1.0/src/lib.rs:3:4:";
        let location =
            panic_location_str(line).ok_or_else(|| anyhow::anyhow!("registry panic path"))?;
        assert_eq!(&location, "/root/.cargo/registry/src/index/foo-1.0/src/lib.rs:3:4");
        Ok(())
    }

    #[test]
    fn panic_re_classify_extracts_panic_location_from_absolute_path() {
        // End-to-end: a failing test's own stdout block carrying a panic in a
        // dependency frame must surface the absolute path on the receipt.
        // The block binds the location to the first failing test (#16148).
        let log = "running 1 test\n\
test ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix stdout ----\n\
thread 'x' panicked at /home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1:\n\
boom\n\
\n\
failures:\n\
    ux_scenario_19_diagnostics_lifecycle::scenario_19_diagnostics_clear_after_fix\n\
\n\
test result: FAILED. 0 passed; 1 failed";
        let receipt = classify(log, Some("abs-sha".to_string()));
        assert_eq!(
            receipt.panic_location.as_deref(),
            Some("/home/runner/work/perl-lsp-swarm/xtask/src/a.rs:7:1"),
            "panic_location must surface the absolute path through classify()"
        );
    }

    #[test]
    fn panic_re_first_character_anchor_still_excludes_arbitrary_text() {
        // The first-character class `[a-zA-Z./]` keeps the old anchor's
        // narrowness: a leading space, a leading digit, or a leading `-` still
        // cannot start a captured location. This protects against the failure
        // mode the issue warns about — letting the regex swallow any prose
        // after `panicked at`. Whitespace, digits, and `-` all stay excluded.
        for line in [
            "thread 'x' panicked at  something happened: 100:200",
            "thread 'x' panicked at 9lives/src/lib.rs:42:8:",
            "thread 'x' panicked at -/weird/path.rs:42:8:",
        ] {
            assert!(panic_location_str(line).is_none(), "leading {line:?} must not match, but did");
        }
    }

    #[test]
    fn panic_re_whitespace_inside_path_is_not_captured() -> Result<()> {
        // The path segments are whitespace-free by grammar: a real panic
        // location is one token, so `./` followed by a space is prose, not a
        // path. The `[^:]*` tail used to admit that whitespace and captured
        // `./ something:100:200` as a bogus `panic_location` (review finding
        // on #16189, FC-WHITESPACE-PATH-GRAMMAR); `[^:\s]*` refuses it.
        for line in [
            "thread 'x' panicked at ./ something:100:200",
            "thread 'x' panicked at ./a b.rs:100:200",
        ] {
            assert!(
                panic_location_str(line).is_none(),
                "whitespace inside {line:?} must not be captured as a path, but was"
            );
        }
        // A genuine `./`-relative location with no inner whitespace still
        // matches, proving the negative control is not over-narrow.
        let line = "thread 'x' panicked at ./xtask/src/a.rs:100:200:";
        let location =
            panic_location_str(line).ok_or_else(|| anyhow::anyhow!("relative panic path"))?;
        assert_eq!(&location, "./xtask/src/a.rs:100:200");
        Ok(())
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
    fn the_human_summary_carries_the_first_failing_tests_own_evidence() {
        // #16103: `failure_class` is read from the whole log, and here it reads the
        // scenario-14 provider vocabulary and calls the run a provider regression —
        // while the test that actually failed first ran out of budget. The check
        // surface prints `human_summary` and nothing else, so unless the per-test
        // reading travels in there, the only sentence a triager sees is the wrong one.
        let receipt = classify(TWO_FAILURES_LOG, Some("sha".to_string()));
        assert!(
            receipt.human_summary.contains("classified as ProviderRegression"),
            "the whole-run class stays, so existing readers see what they saw: {}",
            receipt.human_summary
        );
        assert!(
            receipt.human_summary.contains("a bounded wait expired"),
            "the first failing test's own mode must reach the printed sentence: {}",
            receipt.human_summary
        );
        assert!(
            receipt.human_summary.contains("deadline expired after 5000ms"),
            "and the line it was read from, so the receipt shows its work: {}",
            receipt.human_summary
        );
    }

    #[test]
    fn an_undiscriminated_failure_says_so_rather_than_borrowing_a_class() {
        // The counterpart control. A block with nothing this classifier reads must
        // not silently inherit the whole-run class's confidence: "not discriminated"
        // is what sends a triager to the log instead of to the wrong file.
        let log = "running 1 test\n\
test ux_scenario_61_package_boundary_receiver_inline_completion_quality::scenario_61_receipt ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_scenario_61_package_boundary_receiver_inline_completion_quality::scenario_61_receipt stdout ----\n\
Error: the run ended without a verdict\n\
\n\
failures:\n\
    ux_scenario_61_package_boundary_receiver_inline_completion_quality::scenario_61_receipt\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored";
        let receipt = classify(log, None);
        assert!(!receipt.failing_tests[0].discriminated, "the block carries no marker");
        assert!(
            receipt.human_summary.contains("not discriminated"),
            "an unread block must be reported as unread: {}",
            receipt.human_summary
        );
    }

    #[test]
    fn a_passing_run_keeps_its_summary_unchanged() {
        // The addition is for failures only; a green run's sentence is a contract
        // other readers already parse.
        let receipt =
            classify("running 1 test\ntest ok_test ... ok\ntest result: ok. 1 passed", None);
        assert_eq!(receipt.human_summary, "UX regression passed; merge_allowed.");
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
    fn diagnostic_detail_inside_a_failing_block_cannot_veto_the_local_fallback() -> Result<()> {
        // Devin Review on #16713. The whole-log reader strips diagnostic detail
        // blocks before its substring scan, but the per-test re-read did not, so
        // a detail line mentioning `baseline` inside the failing test's stdout
        // vetoed the local fallback and kept `update_baseline` for what the
        // test's own evidence names as an assertion failure.
        let log = "Restored baseline snapshot cache in 0.4s\n\
failures:\n\n\
---- ux_scenario_20_completion::hard_assert stdout ----\n\
assertion failed: missing completion\n\
UX_SCENARIO_DETAIL_BEGIN: `scenario_20`\n\
baseline diagnostic context\n\
UX_SCENARIO_DETAIL_END\n\
\n\
failures:\n\
    ux_scenario_20_completion::hard_assert\n\
\n\
test result: FAILED. 0 passed; 1 failed";

        let receipt = classify(log, None);
        let [test] = receipt.failing_tests.as_slice() else {
            bail!(
                "the log carries exactly one failing test block, got {}",
                receipt.failing_tests.len()
            );
        };
        ensure!(
            test.mode == UxFailureMode::AssertionFailed,
            "the block's assertion line is the evidence the fallback rests on, got {:?}",
            test.mode
        );
        ensure!(
            receipt.failure_class == UxFailureClass::ProviderRegression,
            "inert detail inside the block must not preserve BaselineDrift, got {:?}",
            receipt.failure_class
        );
        ensure!(
            receipt.merge_action != "update_baseline",
            "a detail word must not restore the forbidden remedy, got {}",
            receipt.merge_action
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
    fn a_passing_run_publishes_no_failure_classification() {
        // `Compiling perl-lsp-ux-baselines` appears in every green log. Before
        // #16287 the whole-log class scan read it, so a passing receipt claimed
        // `baseline_drift` with route `baseline_update` — a green run advising
        // the one remedy the repository forbids without a failure.
        let log = "   Compiling perl-lsp-ux-baselines v0.1.0\n\
                   running 3 tests\n\
                   test ux_scenario_01_startup::start ... ok\n\
                   test result: ok. 3 passed; 0 failed";
        let receipt = classify(log, None);
        assert_eq!(receipt.result, "pass");
        assert_eq!(
            receipt.failure_class,
            UxFailureClass::Unknown,
            "a passing run asserts no failure; the neutral class says nothing"
        );
        assert_eq!(receipt.route, UxRoute::Triage);
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

    // ── #16103: a crash is never answered with a baseline remedy ───────────
    //
    // `update_baseline` tells a reader to accept the observed value as the new
    // expectation. A panicking test produced no observed value. These pin the
    // guard and, just as importantly, its limit.

    /// One failing test, which panicked with no assertion anywhere in its block,
    /// while the word that drives `BaselineDrift` sits in a cargo status line that
    /// has nothing to do with the failure. This is the shape the gate published on
    /// job 106147516331.
    const PANIC_WITH_INCIDENTAL_BASELINE_LOG: &str = "   Compiling perl-lsp-ux-baselines v0.1.0\n\
running 1 test\n\
test ux_latency_document_symbols_returns_real_process_shape ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_latency_document_symbols_returns_real_process_shape stdout ----\n\
thread 'ux_latency_document_symbols_returns_real_process_shape' (6514) panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:331:5:\n\
called `Option::unwrap()` on a `None` value\n\
\n\
failures:\n\
    ux_latency_document_symbols_returns_real_process_shape\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored";

    #[test]
    fn a_crash_is_never_answered_with_a_baseline_remedy() {
        let receipt = classify(PANIC_WITH_INCIDENTAL_BASELINE_LOG, None);

        assert_eq!(
            receipt.failing_tests.len(),
            1,
            "the fixture has exactly one failing test, and it panicked"
        );
        assert_eq!(receipt.failing_tests[0].mode, UxFailureMode::Panic);

        assert_ne!(
            receipt.merge_action, "update_baseline",
            "there is no observed value to accept as a new baseline: the test crashed"
        );
        assert!(
            matches!(receipt.failure_class, UxFailureClass::Unknown),
            "the baseline verdict is withdrawn, not replaced with another guess"
        );
        assert_eq!(receipt.route, UxRoute::Triage, "a crash with no comparison goes to a human");
        assert_eq!(receipt.merge_action, "triage");
        assert!(receipt.blocking, "withdrawing the class must not soften the gate");
        assert!(
            receipt.human_summary.contains("panicked"),
            "the reader still gets the evidence that decided it: {}",
            receipt.human_summary
        );
    }

    /// The same incidental word, but now one failing test really did compare two
    /// values. That assertion may be the baseline comparison the class names, so
    /// the class must survive.
    const BASELINE_ASSERTION_BESIDE_A_PANIC_LOG: &str = "   Compiling perl-lsp-ux-baselines v0.1.0\n\
running 2 tests\n\
test ux_latency_raw_rpc::ux_latency_hover_within_baseline ... FAILED\n\
test ux_latency_raw_rpc::ux_latency_document_symbols_returns_real_process_shape ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_latency_raw_rpc::ux_latency_hover_within_baseline stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:212:5:\n\
assertion `left <= right` failed: hover exceeded its recorded baseline\n\
  left: 910\n\
 right: 400\n\
\n\
---- ux_latency_raw_rpc::ux_latency_document_symbols_returns_real_process_shape stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:331:5:\n\
called `Option::unwrap()` on a `None` value\n\
\n\
test result: FAILED. 0 passed; 2 failed; 0 ignored";

    #[test]
    fn a_real_baseline_assertion_beside_a_crash_keeps_its_remedy() {
        let receipt = classify(BASELINE_ASSERTION_BESIDE_A_PANIC_LOG, None);

        assert_eq!(receipt.failing_tests[0].mode, UxFailureMode::AssertionFailed);
        assert_eq!(receipt.failing_tests[1].mode, UxFailureMode::Panic);

        assert!(
            matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "a block that compared two real values is exactly what the class is for"
        );
        assert_eq!(
            receipt.merge_action, "update_baseline",
            "the guard withdraws an unsupported verdict; it must not withdraw a supported one"
        );
    }

    /// Two failing tests where the FIRST printed no stdout block at all and the
    /// second did. Cargo does this whenever a failure produces no captured output.
    const BLOCKLESS_FIRST_FAILURE_LOG: &str = "running 2 tests\n\
test ux_scenario_01_startup::start ... FAILED\n\
test ux_latency_raw_rpc::ux_latency_hover_is_prompt ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_latency_raw_rpc::ux_latency_hover_is_prompt stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:212:5:\n\
hover wait ended: deadline expired after 5000ms with the stream still live\n\
\n\
failures:\n\
    ux_scenario_01_startup::start\n\
    ux_latency_raw_rpc::ux_latency_hover_is_prompt\n\
\n\
test result: FAILED. 0 passed; 2 failed; 0 ignored";

    #[test]
    fn a_blockless_failure_survives_beside_one_that_printed_a_block() {
        let receipt = classify(BLOCKLESS_FIRST_FAILURE_LOG, None);

        assert_eq!(
            receipt.failing_tests.len(),
            2,
            "a failure cargo printed no block for is still a failure: {:?}",
            receipt.failing_tests.iter().map(|test| &test.name).collect::<Vec<_>>()
        );

        let blockless =
            receipt.failing_tests.iter().find(|test| test.name == "ux_scenario_01_startup::start");
        assert_eq!(
            blockless.map(|test| test.mode),
            Some(UxFailureMode::Unknown),
            "the blockless failure must be recorded, and as unexplained"
        );
        assert_eq!(
            blockless.map(|test| test.discriminated),
            Some(false),
            "no block is no evidence"
        );

        let with_block = receipt
            .failing_tests
            .iter()
            .find(|test| test.name == "ux_latency_raw_rpc::ux_latency_hover_is_prompt");
        assert_eq!(
            with_block.map(|test| test.mode),
            Some(UxFailureMode::BudgetExceeded),
            "the block-backed failure keeps its own reading"
        );
        assert_eq!(
            with_block.map(|test| test.evidence.is_some()),
            Some(true),
            "its deadline line is still its evidence"
        );

        assert!(
            receipt.human_summary.contains("not discriminated"),
            "the first failing test is the blockless one, so the sentence must say so: {}",
            receipt.human_summary
        );
    }

    /// The same incidental `baseline` as the crash fixture, but the co-failure is a
    /// plausible baseline comparison that printed no block — so nothing is known
    /// about it.
    const UNKNOWN_BESIDE_A_CRASH_LOG: &str = "   Compiling perl-lsp-ux-baselines v0.1.0\n\
running 2 tests\n\
test ux_scenario_07_baseline_probe::compares ... FAILED\n\
test ux_latency_raw_rpc::ux_latency_document_symbols_returns_real_process_shape ... FAILED\n\
\n\
failures:\n\
\n\
---- ux_latency_raw_rpc::ux_latency_document_symbols_returns_real_process_shape stdout ----\n\
thread 'main' panicked at crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs:331:5:\n\
called `Option::unwrap()` on a `None` value\n\
\n\
failures:\n\
    ux_scenario_07_baseline_probe::compares\n\
    ux_latency_raw_rpc::ux_latency_document_symbols_returns_real_process_shape\n\
\n\
test result: FAILED. 0 passed; 2 failed; 0 ignored";

    #[test]
    fn an_unexplained_co_failure_keeps_the_baseline_remedy() {
        // `Unknown` means no marker was recognised, not that no comparison
        // happened — an unrecognised baseline mismatch has exactly this shape.
        // Letting it corroborate the crash would take `update_baseline` away from a
        // real baseline failure on the strength of having learned nothing.
        let receipt = classify(UNKNOWN_BESIDE_A_CRASH_LOG, None);

        assert_eq!(receipt.failing_tests.len(), 2);
        assert!(
            receipt.failing_tests.iter().any(|test| test.mode == UxFailureMode::Unknown),
            "the blockless failure must reach the guard as unexplained"
        );
        assert!(
            matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "the guard needs affirmative crash evidence from every failing test"
        );
        assert_eq!(receipt.merge_action, "update_baseline");
    }

    /// #16609. The run's only `baseline` sits in a cache step that belongs to no
    /// failing test, and the failing test's own block rejects a real value without
    /// ever naming a baseline. There is nothing here to accept as a new expectation.
    const STRAY_BASELINE_PROSE_BESIDE_AN_ASSERTION_LOG: &str = "Restored baseline snapshot cache in 0.4s\n\
running 1 test\n\
test ux_scenario_20_real_workspace_providers::module_completion_surfaces_a_real_symbol ... FAILED\n\
\n\
failures:\n\n\
---- ux_scenario_20_real_workspace_providers::module_completion_surfaces_a_real_symbol stdout ----\n\
assertion failed: the completion response carried no symbol for a real module\n\
\n\
test result: FAILED. 0 passed; 1 failed";

    #[test]
    fn a_stray_baseline_word_does_not_claim_a_run_whose_own_blocks_are_assertions() -> Result<()> {
        // The measured shape from job 106081131409: an assertion failure answered
        // with `update_baseline` because the whole-log scan matched the word in
        // incidental output. The block-backed failure is a plain assertion, so
        // `every_failing_test_is_attributed` holds and the scan has to step aside.
        let receipt = classify(STRAY_BASELINE_PROSE_BESIDE_AN_ASSERTION_LOG, None);

        let [assertion] = receipt.failing_tests.as_slice() else {
            bail!("the log carries one block-backed failure, got {}", receipt.failing_tests.len());
        };
        ensure!(
            assertion.mode == UxFailureMode::AssertionFailed,
            "the block is what this claim rests on, got {:?}",
            assertion.mode
        );
        ensure!(
            !matches!(receipt.failure_class, UxFailureClass::BaselineDrift),
            "no failing test mentioned a baseline, so the run is not a baseline to move, got {:?}",
            receipt.failure_class
        );
        ensure!(
            receipt.merge_action != "update_baseline",
            "the remedy must not instruct a reader to move a number this run never produced"
        );
        ensure!(
            receipt.merge_action == "fix_provider",
            "re-reading the failing test's own evidence names the failure it actually made"
        );
        Ok(())
    }

    /// The same stray prose, now beside an expired wait. `update_baseline` is the
    /// one remedy the repository forbids on a bounded-wait failure, and the
    /// starvation probe's own block is what must answer for it.
    const STRAY_BASELINE_PROSE_BESIDE_A_SPENT_BUDGET_LOG: &str = "Restored baseline snapshot cache in 0.4s\n\
running 2 tests\n\
test ux_latency_raw_rpc::hover ... FAILED\n\
test ux_scenario_20_real_workspace_providers::module_completion_surfaces_a_real_symbol ... FAILED\n\
\n\
failures:\n\n\
---- ux_latency_raw_rpc::hover stdout ----\n\
wait ended: deadline expired after 5000ms with the stream still live\n\
\n\
---- ux_scenario_20_real_workspace_providers::module_completion_surfaces_a_real_symbol stdout ----\n\
assertion failed: the completion response carried no symbol for a real module\n\
\n\
test result: FAILED. 0 passed; 2 failed";

    #[test]
    fn a_stray_baseline_word_does_not_claim_a_run_beside_a_spent_budget() {
        // Not every failure is a budget, so the run-level budget shortcut does not
        // fire and the stray word would otherwise decide the class on its own.
        let receipt = classify(STRAY_BASELINE_PROSE_BESIDE_A_SPENT_BUDGET_LOG, None);

        assert!(
            receipt.failing_tests.iter().any(|test| test.mode == UxFailureMode::BudgetExceeded),
            "the starvation block is what this claim rests on, got {:?}",
            receipt.failing_tests.iter().map(|test| test.mode).collect::<Vec<_>>()
        );
        assert_ne!(
            receipt.failure_class,
            UxFailureClass::BaselineDrift,
            "a spent wait and an assertion are not a baseline to accept, got {:?}",
            receipt.failure_class
        );
        assert_ne!(
            receipt.merge_action, "update_baseline",
            "widening a budget until the noise fits is the remedy this forbids"
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
