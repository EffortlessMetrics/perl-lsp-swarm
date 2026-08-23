//! TAP (Test Anything Protocol) compatibility projection for LSP consumers.
//!
//! [`perl_test_facts`] is the canonical server-side TAP parser. This module
//! preserves the historical `perl-lsp-rs-core` result shapes while consumers
//! migrate to the richer canonical report. It does not implement a second TAP
//! grammar.
//!
//! `perl-lsp` still does not run tests itself: `yath`, `prove`, `perl`, or a
//! reviewed project command produce TAP. The pure facts crate reads that output;
//! product layers associate the result with source/test identity and render it.

pub use perl_test_facts::{
    TapAssertion as CanonicalTapAssertion, TapAssertionOutcome as CanonicalTapAssertionOutcome,
    TapAssertionStatus as CanonicalTapAssertionStatus, TapPlan as CanonicalTapPlan,
    TapReport as CanonicalTapReport,
};

/// A TODO/SKIP directive attached to a test line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapDirective {
    /// `# TODO reason` — a known-failing test; a `not ok` here is expected and
    /// is not counted as a hard failure.
    Todo(String),
    /// `# SKIP reason` — the test was not run.
    Skip(String),
}

/// A single `ok` / `not ok` line projected for historical LSP consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapTest {
    /// The 1-based test number, if present.
    pub number: Option<usize>,
    /// Whether the line began with `ok` (as opposed to `not ok`).
    pub ok: bool,
    /// The test description (text after the number, before any directive).
    pub description: String,
    /// A TODO/SKIP directive, if any.
    pub directive: Option<TapDirective>,
    /// Nesting depth from TAP indentation (0 = top level).
    pub depth: usize,
    /// Diagnostic lines retained by the canonical parser.
    pub diagnostics: Vec<String>,
    /// Source file reported by the runner, if present.
    pub file: Option<String>,
    /// Source line reported by the runner, if present.
    pub line: Option<usize>,
    /// First `got`/`received` value reported by the runner, if present.
    pub got: Option<String>,
    /// First `expected`/`wanted` value reported by the runner, if present.
    pub expected: Option<String>,
}

impl TapTest {
    /// Whether this line is a hard failure: a `not ok` that is not TODO/SKIP.
    pub fn is_failure(&self) -> bool {
        !self.ok && !self.is_todo() && !self.is_skipped()
    }

    /// Whether this test was skipped.
    pub fn is_skipped(&self) -> bool {
        matches!(self.directive, Some(TapDirective::Skip(_)))
    }

    /// Whether this test was marked TODO.
    pub fn is_todo(&self) -> bool {
        matches!(self.directive, Some(TapDirective::Todo(_)))
    }
}

/// Historical projection of the top-level TAP plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapPlan {
    /// Number of planned top-level assertions.
    pub count: usize,
    /// Skip-all reason for a zero-count `# SKIP` plan, if present.
    pub skip_all: Option<String>,
}

/// Aggregate counts for the compatibility report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TapSummary {
    /// Total parsed assertions, including nested assertions.
    pub total: usize,
    /// Hard failures (`not ok` without TODO/SKIP).
    pub failed: usize,
    /// Passing assertions (`ok` without SKIP).
    pub passed: usize,
    /// Skipped assertions.
    pub skipped: usize,
    /// TODO assertions, passing or failing.
    pub todo: usize,
}

/// Historical LSP-facing projection of a canonical TAP report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TapReport {
    /// Top-level plan, if present.
    pub plan: Option<TapPlan>,
    /// Assertions in protocol order.
    pub tests: Vec<TapTest>,
    /// Bailout reason, if the run bailed out.
    pub bailed_out: Option<String>,
    /// Aggregate counts.
    pub summary: TapSummary,
}

impl TapReport {
    /// Hard failures in this report.
    pub fn failures(&self) -> Vec<&TapTest> {
        self.tests.iter().filter(|test| test.is_failure()).collect()
    }

    /// Whether there are no hard failures and no bailout.
    ///
    /// Plan mismatch remains an independent structural result.
    pub fn passed(&self) -> bool {
        self.bailed_out.is_none() && self.summary.failed == 0
    }

    /// Return `(actual, planned)` when the top-level assertion count differs.
    pub fn plan_mismatch(&self) -> Option<(usize, usize)> {
        let plan = self.plan.as_ref()?;
        if plan.skip_all.is_some() {
            return None;
        }
        let actual = self.tests.iter().filter(|test| test.depth == 0).count();
        (actual != plan.count).then_some((actual, plan.count))
    }
}

/// Result of focusing a compatibility report on one buffered subtest summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtestFocus {
    /// Requested subtest name.
    pub name: String,
    /// Whether a matching summary line was found.
    pub found: bool,
    /// Whether the matching summary line passed.
    pub passed: bool,
    /// Nested hard failures immediately preceding the summary line.
    pub inner_failed: usize,
}

/// Parse TAP into the canonical dependency-free result model.
///
/// New consumers should prefer this function so structural diagnostics, YAML
/// blocks, unknown raw records, and TAP stream line numbers are retained.
#[must_use]
pub fn parse_tap_facts(output: &str) -> CanonicalTapReport {
    perl_test_facts::parse_tap(output)
}

/// Parse TAP and project it into the historical LSP-facing result shape.
///
/// This function exists for source compatibility. Parsing itself is delegated
/// to [`perl_test_facts::parse_tap`].
#[must_use]
pub fn parse_tap(output: &str) -> TapReport {
    project_report(&parse_tap_facts(output))
}

/// Focus a parsed report on a named buffered subtest.
///
/// Test2/Test::More generally emit indented assertions followed by a depth-zero
/// summary assertion. This helper attributes the contiguous nested failures to
/// that summary. It does not claim the subtest executed in isolation.
pub fn focus_subtest(report: &TapReport, name: &str) -> Option<SubtestFocus> {
    let summary_index =
        report.tests.iter().position(|test| test.depth == 0 && test.description == name)?;
    let summary = &report.tests[summary_index];

    let mut inner_failed = 0;
    for test in report.tests[..summary_index].iter().rev() {
        if test.depth == 0 {
            break;
        }
        if test.is_failure() {
            inner_failed += 1;
        }
    }

    Some(SubtestFocus {
        name: name.to_string(),
        found: true,
        passed: !summary.is_failure(),
        inner_failed,
    })
}

fn project_report(report: &CanonicalTapReport) -> TapReport {
    let tests = report.assertions.iter().map(project_assertion).collect();
    let plan = report.plan.as_ref().map(project_plan);
    let summary = TapSummary {
        total: report.assertions.len(),
        failed: report.failed_count(),
        passed: report.passed_count(),
        skipped: report.skipped_count(),
        todo: report.todo_count(),
    };

    TapReport { plan, tests, bailed_out: report.bail_out.clone(), summary }
}

fn project_assertion(assertion: &CanonicalTapAssertion) -> TapTest {
    let directive = match assertion.status {
        CanonicalTapAssertionStatus::Todo => Some(TapDirective::Todo(
            directive_reason(assertion.directive.as_deref(), "todo").unwrap_or_default(),
        )),
        CanonicalTapAssertionStatus::Skip => Some(TapDirective::Skip(
            directive_reason(assertion.directive.as_deref(), "skip").unwrap_or_default(),
        )),
        _ => None,
    };

    let mut diagnostics = assertion.diagnostic_lines.clone();
    diagnostics.extend(assertion.diagnostics.iter().cloned());

    TapTest {
        number: assertion.number.and_then(|number| usize::try_from(number).ok()),
        ok: matches!(assertion.outcome, CanonicalTapAssertionOutcome::Pass),
        description: assertion.name.clone().unwrap_or_default(),
        directive,
        depth: assertion.depth,
        diagnostics,
        file: assertion.source_file.clone(),
        line: assertion.source_line,
        got: assertion.got.clone(),
        expected: assertion.expected.clone(),
    }
}

fn project_plan(plan: &CanonicalTapPlan) -> TapPlan {
    let count = if plan.end < plan.start {
        0
    } else {
        let count = u64::from(plan.end) - u64::from(plan.start) + 1;
        usize::try_from(count).unwrap_or(usize::MAX)
    };
    let skip_all =
        (count == 0).then(|| directive_reason(plan.directive.as_deref(), "skip")).flatten();

    TapPlan { count, skip_all }
}

fn directive_reason(directive: Option<&str>, expected_kind: &str) -> Option<String> {
    let directive = directive?.trim();
    let split = directive.find(char::is_whitespace).unwrap_or(directive.len());
    let kind = &directive[..split];
    if !kind.eq_ignore_ascii_case(expected_kind) {
        return None;
    }
    Some(directive[split..].trim().to_string())
}

#[cfg(test)]
mod tests;
