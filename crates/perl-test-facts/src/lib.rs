//! Pure TAP result facts for native Perl test intelligence.
//!
//! This crate parses TAP text only. Test execution, subprocess management, and
//! source-file discovery belong to runtime adapters and workspace consumers.
//! Runner-emitted source locations are retained as facts when they appear in
//! TAP diagnostics; retaining them does not inspect or discover source files.

#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

use std::collections::HashSet;

/// The outcome of one TAP assertion.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapAssertionStatus {
    /// The assertion passed.
    Pass,
    /// The assertion failed.
    Fail,
    /// The assertion was skipped.
    Skip,
    /// The assertion is expected to fail.
    Todo,
    /// The assertion used an unsupported directive or could not be classified.
    Unknown,
}

impl TapAssertionStatus {
    /// Return a stable wire label for this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Todo => "todo",
            Self::Unknown => "unknown",
        }
    }
}

/// The raw pass/fail outcome before TODO or SKIP classification.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapAssertionOutcome {
    /// The assertion line began with `ok`.
    Pass,
    /// The assertion line began with `not ok`.
    Fail,
}

/// A TAP plan declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapPlan {
    /// First assertion number in the plan.
    pub start: u32,
    /// Last assertion number in the plan.
    pub end: u32,
    /// Optional plan directive, including its reason.
    pub directive: Option<String>,
    /// TAP stream line containing the plan.
    pub line: usize,
}

/// One parsed TAP assertion.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapAssertion {
    /// TAP stream line containing the assertion.
    pub line: usize,
    /// Nesting depth from TAP indentation (zero for top-level assertions).
    pub depth: usize,
    /// Optional assertion number.
    pub number: Option<u32>,
    /// Classified assertion outcome.
    pub status: TapAssertionStatus,
    /// Raw pass/fail outcome preserved independently from directives.
    pub outcome: TapAssertionOutcome,
    /// Optional assertion description.
    pub name: Option<String>,
    /// Optional directive, including its reason.
    pub directive: Option<String>,
    /// Raw YAML diagnostic block lines associated with this assertion.
    pub diagnostics: Vec<String>,
    /// Non-YAML diagnostic lines associated with this assertion.
    pub diagnostic_lines: Vec<String>,
    /// Runner-emitted source file parsed from an `at FILE line N.` diagnostic,
    /// when present; this does not discover or inspect the file.
    pub source_file: Option<String>,
    /// Runner-emitted source line parsed from an `at FILE line N.` diagnostic,
    /// when present; this does not discover or inspect the file.
    pub source_line: Option<usize>,
    /// First value parsed from a `got:` or `received:` diagnostic, when present.
    pub got: Option<String>,
    /// First value parsed from an `expected:` or `wanted:` diagnostic, when present.
    pub expected: Option<String>,
}

/// Parsed TAP output.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TapReport {
    /// Declared TAP protocol version, when present.
    pub version: Option<u32>,
    /// TAP plan, when present.
    pub plan: Option<TapPlan>,
    /// Assertions in source order.
    pub assertions: Vec<TapAssertion>,
    /// Bailout reason, when the test run stopped early.
    pub bail_out: Option<String>,
    /// Parser and structural diagnostics that prevent a fully trusted result.
    pub diagnostics: Vec<String>,
    /// Unrecognized non-comment lines retained as non-fatal raw evidence.
    pub raw_lines: Vec<String>,
}

impl TapReport {
    /// Count assertions with the requested status.
    #[must_use]
    pub fn count(&self, status: TapAssertionStatus) -> usize {
        self.assertions.iter().filter(|assertion| assertion.status == status).count()
    }

    /// Return the number of passing assertions.
    #[must_use]
    pub fn passed_count(&self) -> usize {
        self.assertions
            .iter()
            .filter(|assertion| {
                assertion.outcome == TapAssertionOutcome::Pass
                    && assertion.status != TapAssertionStatus::Skip
            })
            .count()
    }

    /// Return the number of failing assertions.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.assertions
            .iter()
            .filter(|assertion| {
                assertion.outcome == TapAssertionOutcome::Fail
                    && assertion.status != TapAssertionStatus::Todo
                    && assertion.status != TapAssertionStatus::Skip
            })
            .count()
    }

    /// Return the number of skipped assertions.
    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.count(TapAssertionStatus::Skip)
    }

    /// Return the number of TODO assertions.
    #[must_use]
    pub fn todo_count(&self) -> usize {
        self.count(TapAssertionStatus::Todo)
    }

    /// Return the number of assertions that need interpretation or repair.
    #[must_use]
    pub fn unknown_count(&self) -> usize {
        self.count(TapAssertionStatus::Unknown)
    }

    /// Return whether the report has no hard assertion failures or bailout.
    ///
    /// Plan mismatches and structural diagnostics are reported independently;
    /// callers that require a structurally valid report must inspect
    /// [`Self::diagnostics`] and the plan separately. A plan-less or empty
    /// report can still be a hard success; callers that require proof that a
    /// producer completed must also require [`Self::plan`].
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.bail_out.is_none() && self.failed_count() == 0
    }
}

/// Parse TAP text into stable, execution-independent facts.
///
/// The parser intentionally retains unsupported or malformed constructs as
/// diagnostics. It does not execute commands, interpret YAML, or discover
/// source files. Runner-emitted locations in `at FILE line N.` diagnostics are
/// parsed as reported facts; TAP stream line numbers remain separate.
#[must_use]
pub fn parse_tap(source: &str) -> TapReport {
    let mut report = TapReport::default();
    let mut yaml_block: Option<PendingYaml> = None;
    let mut last_assertion: Option<(usize, usize, usize)> = None;
    let normalized_source = source.replace("\r\n", "\n").replace('\r', "\n");

    for (index, raw_line) in normalized_source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        if let Some(mut block) = yaml_block.take() {
            let trimmed = line.trim();
            let indentation = yaml_indentation_width(line);
            if trimmed.is_empty() || trimmed.starts_with('#') {
                block.lines.push(line.to_owned());
                yaml_block = Some(block);
                continue;
            }
            if trimmed == "..." && indentation == Some(block.indentation) {
                block.lines.push(line.to_owned());
                if let Some(assertion) = report.assertions.get_mut(block.assertion_index) {
                    assertion.diagnostics.append(&mut block.lines);
                } else {
                    report.diagnostics.push(format!(
                        "line {line_number}: YAML diagnostics have no preceding assertion"
                    ));
                }
                last_assertion = None;
                continue;
            }

            if let Some(indentation) = indentation
                && indentation >= block.indentation
            {
                block.lines.push(line.to_owned());
                yaml_block = Some(block);
                continue;
            }

            report.raw_lines.append(&mut block.lines);
            report.diagnostics.push(format!(
                "line {line_number}: YAML diagnostics block interrupted before terminator"
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            if let Some((assertion_index, assertion_depth, _)) = last_assertion
                && indentation_depth(line) == Some(assertion_depth)
                && let Some(assertion) = report.assertions.get_mut(assertion_index)
            {
                apply_diagnostic(assertion, trimmed);
            }
            continue;
        }

        if report.bail_out.is_some() {
            if parse_bailout(trimmed).is_some() {
                report.diagnostics.push(format!("line {line_number}: duplicate bailout"));
            } else {
                report.raw_lines.push(line.to_owned());
            }
            continue;
        }

        if trimmed == "---" {
            let Some(indentation) = yaml_indentation_width(line) else {
                report.raw_lines.push(line.to_owned());
                report.diagnostics.push(format!(
                    "line {line_number}: YAML diagnostics indentation must use spaces only"
                ));
                last_assertion = None;
                continue;
            };
            if let Some((assertion_index, _, expected_indentation)) = last_assertion {
                if assertion_index + 1 == report.assertions.len()
                    && indentation == expected_indentation
                {
                    yaml_block = Some(PendingYaml {
                        lines: vec![line.to_owned()],
                        assertion_index,
                        indentation,
                    });
                } else {
                    report.raw_lines.push(line.to_owned());
                    report.diagnostics.push(format!(
                        "line {line_number}: YAML diagnostics are not attached to the preceding assertion"
                    ));
                    last_assertion = None;
                }
            } else {
                report.raw_lines.push(line.to_owned());
                report.diagnostics.push(format!(
                    "line {line_number}: YAML diagnostics have no preceding assertion"
                ));
                last_assertion = None;
            }
            continue;
        }

        let Some(depth) = indentation_depth(line) else {
            report.raw_lines.push(line.to_owned());
            last_assertion = None;
            continue;
        };

        if let Some(rest) = trimmed.strip_prefix("TAP version") {
            if depth != 0 {
                report.raw_lines.push(line.to_owned());
                last_assertion = None;
                continue;
            }
            if report.version.is_some() {
                report.diagnostics.push(format!("line {line_number}: duplicate TAP version"));
            }
            match rest.trim().parse::<u32>() {
                Ok(version) => report.version = Some(version),
                Err(_) => report
                    .diagnostics
                    .push(format!("line {line_number}: invalid TAP version declaration")),
            }
            last_assertion = None;
            continue;
        }

        if let Some(reason) = parse_bailout(trimmed) {
            if report.bail_out.is_some() {
                report.diagnostics.push(format!("line {line_number}: duplicate bailout"));
            } else {
                report.bail_out = Some(reason.to_owned());
            }
            last_assertion = None;
            continue;
        }

        if depth == 0 {
            if let Some(plan) = parse_plan(trimmed, line_number) {
                if let Some(existing) = report.plan.as_ref() {
                    report.diagnostics.push(format!(
                        "line {line_number}: duplicate TAP plan; previous plan was on line {}",
                        existing.line
                    ));
                    report.plan = Some(plan);
                } else {
                    report.plan = Some(plan);
                }
                last_assertion = None;
                continue;
            }

            if looks_like_plan(trimmed) {
                report.diagnostics.push(format!("line {line_number}: invalid TAP plan"));
                last_assertion = None;
                continue;
            }
        } else if looks_like_plan(trimmed) {
            // A subtest owns its own plan. The first implementation exposes
            // the top-level plan and retains nested protocol records as raw
            // evidence until nested-plan facts have a dedicated model.
            report.raw_lines.push(line.to_owned());
            last_assertion = None;
            continue;
        }

        if let Some(assertion) = parse_assertion(trimmed, line_number, depth) {
            report.assertions.push(assertion);
            last_assertion =
                Some((report.assertions.len() - 1, depth, leading_indent_width(line) + 2));
            continue;
        }

        report.raw_lines.push(line.to_owned());
        last_assertion = None;
    }

    if let Some(mut block) = yaml_block {
        if let Some(assertion) = report.assertions.get_mut(block.assertion_index) {
            assertion.diagnostics.append(&mut block.lines);
            report.diagnostics.push("unterminated YAML diagnostics block".to_owned());
        } else {
            report.raw_lines.append(&mut block.lines);
            report.diagnostics.push("YAML diagnostics have no preceding assertion".to_owned());
        }
    }

    validate_plan(&mut report);
    report
}

#[derive(Debug)]
struct PendingYaml {
    lines: Vec<String>,
    assertion_index: usize,
    indentation: usize,
}

fn leading_indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .map(|character| if character == '\t' { 4 } else { 1 })
        .sum()
}

fn yaml_indentation_width(line: &str) -> Option<usize> {
    let mut width = 0usize;
    for character in line.chars() {
        match character {
            ' ' => width += 1,
            '\t' => return None,
            _ => break,
        }
    }
    Some(width)
}

fn indentation_depth(line: &str) -> Option<usize> {
    let mut spaces = 0usize;
    let mut depth = 0usize;
    for character in line.chars() {
        match character {
            ' ' => spaces += 1,
            '\t' if spaces == 0 => depth += 1,
            '\t' => return None,
            _ => break,
        }
        if spaces == 4 {
            depth += 1;
            spaces = 0;
        }
    }
    (spaces == 0).then_some(depth)
}

fn parse_bailout(line: &str) -> Option<&str> {
    const PREFIX: &str = "Bail out!";
    let prefix = line.get(..PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let remainder = &line[PREFIX.len()..];
    (remainder.is_empty() || remainder.chars().next().is_some_and(char::is_whitespace))
        .then(|| remainder.trim())
}

fn parse_plan(line: &str, line_number: usize) -> Option<TapPlan> {
    let (token, remainder) = split_first_token(line);
    let (start, end) = token.split_once("..")?;
    let start = start.parse::<u32>().ok()?;
    let end = end.parse::<u32>().ok()?;
    let remainder = remainder.trim();
    let directive = if remainder.is_empty() {
        None
    } else {
        Some(remainder.strip_prefix('#')?.trim().to_owned())
    };
    Some(TapPlan { start, end, directive, line: line_number })
}

fn looks_like_plan(line: &str) -> bool {
    let (token, _) = split_first_token(line);
    token.contains("..") && token.chars().next().is_some_and(|character| character.is_ascii_digit())
}

fn parse_assertion(line: &str, line_number: usize, depth: usize) -> Option<TapAssertion> {
    let (status, remainder) = if let Some(rest) = status_remainder(line, "not ok") {
        (TapAssertionStatus::Fail, rest)
    } else if let Some(rest) = status_remainder(line, "ok") {
        (TapAssertionStatus::Pass, rest)
    } else {
        return None;
    };

    let outcome = if status == TapAssertionStatus::Pass {
        TapAssertionOutcome::Pass
    } else {
        TapAssertionOutcome::Fail
    };
    let (number, remainder) = parse_number(remainder);
    let (name, directive) = split_directive(remainder);
    let (status, directive) = classify_directive(status, directive);
    Some(TapAssertion {
        line: line_number,
        depth,
        number,
        status,
        outcome,
        name,
        directive,
        diagnostics: Vec::new(),
        diagnostic_lines: Vec::new(),
        source_file: None,
        source_line: None,
        got: None,
        expected: None,
    })
}

fn apply_diagnostic(assertion: &mut TapAssertion, line: &str) {
    let diagnostic = line.strip_prefix('#').map_or(line, str::trim);
    assertion.diagnostic_lines.push(diagnostic.to_owned());
    if let Some((file, source_line)) = parse_source_location(diagnostic) {
        if assertion.source_file.is_none() {
            assertion.source_file = Some(file);
        }
        if assertion.source_line.is_none() {
            assertion.source_line = Some(source_line);
        }
    }
    if assertion.got.is_none() {
        assertion.got = diagnostic_value(diagnostic, &["got:", "received:"]);
    }
    if assertion.expected.is_none() {
        assertion.expected = diagnostic_value(diagnostic, &["expected:", "wanted:"]);
    }
}

fn diagnostic_value(diagnostic: &str, labels: &[&str]) -> Option<String> {
    let lower = diagnostic.to_ascii_lowercase();
    labels.iter().find_map(|label| {
        lower.strip_prefix(label).and_then(|_| {
            let value = diagnostic[label.len()..].trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
    })
}

fn parse_source_location(diagnostic: &str) -> Option<(String, usize)> {
    let prefix = diagnostic.get(..3)?;
    if !prefix.eq_ignore_ascii_case("at ") {
        return None;
    }
    let rest = &diagnostic[3..];
    let lower = rest.to_ascii_lowercase();
    let marker = " line ";
    let marker_index = lower.rfind(marker)?;
    let file = rest[..marker_index].trim();
    let source_line = rest[marker_index + marker.len()..]
        .trim()
        .trim_end_matches('.')
        .trim()
        .parse::<usize>()
        .ok()?;
    (!file.is_empty()).then(|| (file.to_owned(), source_line))
}

fn status_remainder<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if line == prefix {
        return Some("");
    }
    let rest = line.strip_prefix(prefix)?;
    rest.chars().next().filter(|character| character.is_whitespace())?;
    Some(rest.trim_start())
}

fn parse_number(remainder: &str) -> (Option<u32>, &str) {
    let (token, rest) = split_first_token(remainder);
    match token.parse::<u32>() {
        Ok(number) => (Some(number), rest.trim_start()),
        Err(_) => (None, remainder),
    }
}

fn split_directive(remainder: &str) -> (Option<String>, Option<String>) {
    let Some(hash_index) = remainder.char_indices().find_map(|(index, character)| {
        (character == '#'
            && (index == 0 || remainder[..index].chars().last().is_some_and(char::is_whitespace)))
        .then_some(index)
    }) else {
        return (normalize_name(remainder), None);
    };
    let (name, directive) = remainder.split_at(hash_index);
    let directive = directive.strip_prefix('#').unwrap_or(directive);
    let directive = directive.trim();
    let kind = directive.split_whitespace().next().unwrap_or_default();
    if !kind.eq_ignore_ascii_case("skip") && !kind.eq_ignore_ascii_case("todo") {
        return (normalize_name(remainder), None);
    }
    let directive = (!directive.is_empty()).then(|| directive.to_owned());
    (normalize_name(name), directive)
}

fn normalize_name(name: &str) -> Option<String> {
    let name = name.trim();
    let name = name.strip_prefix('-').map_or(name, str::trim_start);
    (!name.is_empty()).then(|| name.to_owned())
}

fn classify_directive(
    status: TapAssertionStatus,
    directive: Option<String>,
) -> (TapAssertionStatus, Option<String>) {
    let Some(directive_text) = directive.as_deref() else {
        return (status, directive);
    };
    let Some(kind) = directive_text.split_whitespace().next() else {
        return (status, directive);
    };
    if kind.eq_ignore_ascii_case("skip") {
        (TapAssertionStatus::Skip, directive)
    } else if kind.eq_ignore_ascii_case("todo") {
        (TapAssertionStatus::Todo, directive)
    } else {
        (TapAssertionStatus::Unknown, directive)
    }
}

fn split_first_token(line: &str) -> (&str, &str) {
    let Some(index) = line.find(char::is_whitespace) else {
        return (line, "");
    };
    (&line[..index], &line[index..])
}

fn validate_plan(report: &mut TapReport) {
    let Some(plan) = report.plan.as_ref() else {
        return;
    };
    let expected =
        if plan.end >= plan.start { u64::from(plan.end) - u64::from(plan.start) + 1 } else { 0 };
    let expected_count = match usize::try_from(expected) {
        Ok(count) => count,
        Err(_) => {
            report.diagnostics.push(format!(
                "plan on line {} declares too many assertions for this platform",
                plan.line
            ));
            return;
        }
    };
    let top_level: Vec<&TapAssertion> =
        report.assertions.iter().filter(|assertion| assertion.depth == 0).collect();
    let top_level_count = top_level.len();
    if expected_count != top_level_count {
        report.diagnostics.push(format!(
            "plan on line {} declares {expected} assertions but {} were parsed",
            plan.line, top_level_count
        ));
    }

    if top_level.iter().any(|assertion| assertion.line < plan.line)
        && top_level.iter().any(|assertion| assertion.line > plan.line)
    {
        report
            .diagnostics
            .push(format!("plan on line {} appears between top-level assertions", plan.line));
    }

    let mut seen_numbers = HashSet::new();
    for assertion in top_level {
        if let Some(number) = assertion.number {
            if !seen_numbers.insert(number) {
                report.diagnostics.push(format!(
                    "line {}: duplicate top-level assertion number {number}",
                    assertion.line
                ));
            }
            if number < plan.start || number > plan.end {
                report.diagnostics.push(format!(
                    "line {}: assertion number {number} is outside plan {}..{}",
                    assertion.line, plan.start, plan.end
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TapAssertionOutcome, TapAssertionStatus, parse_tap};

    #[test]
    fn parses_passing_named_assertions() {
        let report = parse_tap("TAP version 13\r\nok 1 - loads\r\nok 2 - saves\r\n1..2\r\n");

        assert_eq!(report.version, Some(13));
        assert_eq!(report.plan.as_ref().map(|plan| plan.end), Some(2));
        assert_eq!(report.passed_count(), 2);
        assert_eq!(report.assertions[0].name.as_deref(), Some("loads"));
        assert!(report.is_success());
    }

    #[test]
    fn classifies_skip_and_todo_directives() {
        let report =
            parse_tap("1..2\nok 1 - unavailable # SKIP platform\nnot ok 2 - later # TODO fix\n");

        assert_eq!(report.skipped_count(), 1);
        assert_eq!(report.todo_count(), 1);
        assert_eq!(report.assertions[0].status, TapAssertionStatus::Skip);
        assert_eq!(report.assertions[1].status, TapAssertionStatus::Todo);
        assert!(report.is_success());
    }

    #[test]
    fn retains_yaml_diagnostics_on_the_failed_assertion() {
        let report = parse_tap(
            "not ok 1 - computes\n  ---\n  message: wrong value\n  got: 2\n  expected: 3\n  ...\n1..1\n",
        );

        assert_eq!(report.failed_count(), 1);
        assert_eq!(report.assertions[0].diagnostics.len(), 5);
        assert_eq!(report.assertions[0].diagnostics[1], "  message: wrong value");
        assert!(!report.is_success());
    }

    #[test]
    fn reports_bailout_and_structural_mismatch() {
        let report = parse_tap("TAP version 13\nok 1 - starts\n1..2\nBail out! database offline\n");

        assert_eq!(report.bail_out.as_deref(), Some("database offline"));
        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(!report.is_success());
    }

    #[test]
    fn retains_non_yaml_diagnostics_and_source_locations() {
        let report = parse_tap(
            "not ok 1 - computes\n# at t/example.t line 12.\n# got: 2\n# expected: 3\n# got: later\n1..1\n",
        );

        assert_eq!(
            report.assertions[0].diagnostic_lines,
            vec!["at t/example.t line 12.", "got: 2", "expected: 3", "got: later"]
        );
        assert_eq!(report.assertions[0].source_file.as_deref(), Some("t/example.t"));
        assert_eq!(report.assertions[0].source_line, Some(12));
        assert_eq!(report.assertions[0].got.as_deref(), Some("2"));
        assert_eq!(report.assertions[0].expected.as_deref(), Some("3"));
    }

    #[test]
    fn does_not_attach_dedented_diagnostics_to_nested_assertions() {
        let report = parse_tap(
            "ok 1 - parent\n    not ok 1 - child\n    # got: child\n# expected: parent\n1..1\n",
        );

        assert_eq!(report.assertions.len(), 2);
        assert_eq!(report.assertions[1].got.as_deref(), Some("child"));
        assert_eq!(report.assertions[1].expected, None);
    }

    #[test]
    fn rejects_tab_indentation_for_yaml_diagnostics() {
        let report = parse_tap("not ok 1 - broken\n\t  ---\n    message: raw\n    ...\n");

        assert_eq!(report.assertions[0].diagnostics, Vec::<String>::new());
        assert!(report.raw_lines.iter().any(|line| line.contains("---")));
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("indentation must use spaces only"))
        );
    }

    #[test]
    fn duplicate_plans_replace_the_stored_plan() {
        let report = parse_tap("1..1\n1..2\n1..3\n");

        assert_eq!(report.plan.as_ref().map(|plan| plan.end), Some(3));
        assert_eq!(report.diagnostics.len(), 3);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.contains("line 3: duplicate TAP plan; previous plan was on line 2")
        }));
    }

    #[test]
    fn unnamed_directives_and_raw_outcomes_remain_distinct() {
        let report = parse_tap("ok 1 # TODO later\nnot ok 2 # TODO pending\n1..2\n");

        assert_eq!(report.assertions[0].status, TapAssertionStatus::Todo);
        assert_eq!(report.assertions[0].outcome, TapAssertionOutcome::Pass);
        assert_eq!(report.assertions[1].status, TapAssertionStatus::Todo);
        assert_eq!(report.assertions[1].outcome, TapAssertionOutcome::Fail);
        assert_eq!(report.passed_count(), 1);
        assert_eq!(report.failed_count(), 0);
        assert!(report.is_success());
    }

    #[test]
    fn reports_unknown_directives_and_unrecognized_lines() {
        let report = parse_tap("ok 1 - check # FLAKY\nthis is not TAP\n1..1\n");

        assert_eq!(report.unknown_count(), 0);
        assert_eq!(report.assertions[0].name.as_deref(), Some("check # FLAKY"));
        assert_eq!(report.raw_lines, vec!["this is not TAP"]);
        assert!(report.diagnostics.is_empty());
        assert!(report.is_success());
    }

    #[test]
    fn keeps_hash_in_url_description_and_requires_a_real_directive_delimiter() {
        let report = parse_tap("not ok 1 - https://example.test/#TODO\n1..1\n");

        assert_eq!(report.assertions[0].name.as_deref(), Some("https://example.test/#TODO"));
        assert_eq!(report.assertions[0].status, TapAssertionStatus::Fail);
        assert_eq!(report.assertions[0].directive, None);
        assert!(!report.is_success());
    }

    #[test]
    fn normalizes_lone_carriage_return_line_endings() {
        let report = parse_tap("ok 1 - lone CR\r1..1\r");

        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.assertions[0].name.as_deref(), Some("lone CR"));
        assert_eq!(report.plan.as_ref().map(|plan| plan.end), Some(1));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn stops_semantic_parsing_after_a_case_insensitive_bailout() {
        let report = parse_tap("ok 1 - starts\nbAIL OUT! stopped\nok 2 - after\n1..2\n");

        assert_eq!(report.bail_out.as_deref(), Some("stopped"));
        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.plan, None);
        assert_eq!(report.raw_lines, vec!["ok 2 - after", "1..2"]);
        assert!(!report.is_success());
    }

    #[test]
    fn rejects_a_plan_between_top_level_assertions() {
        let report = parse_tap("ok 1 - first\n1..2\nok 2 - second\n");

        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("between top-level assertions"))
        );
    }

    #[test]
    fn reports_duplicate_top_level_assertion_numbers() {
        let report = parse_tap("1..2\nok 1 - first\nok 1 - duplicate\n");

        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("duplicate top-level assertion number 1"))
        );
    }

    #[test]
    fn retains_malformed_partial_indentation_as_raw_evidence() {
        let report = parse_tap("  ok 1 - malformed\nok 1 - valid\n1..1\n");

        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.assertions[0].name.as_deref(), Some("valid"));
        assert_eq!(report.raw_lines, vec!["  ok 1 - malformed"]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn does_not_attach_yaml_after_an_intervening_plan() {
        let report = parse_tap("not ok 1 - broken\n  ---\n  message: wrong\n  ...\n1..1\n");

        assert_eq!(report.assertions[0].diagnostics.len(), 3);
        assert!(report.diagnostics.is_empty());

        let interrupted = parse_tap("not ok 1 - broken\n  ---\n1..1\n  message: raw\n  ...\n");
        assert!(
            interrupted
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("interrupted before terminator"))
        );
        assert_eq!(interrupted.assertions.len(), 1);
        assert_eq!(interrupted.assertions[0].diagnostics, Vec::<String>::new());
    }

    #[test]
    fn treats_tap_looking_yaml_scalars_as_yaml_content() {
        let report = parse_tap("not ok 1 - broken\n  ---\n  ok 2\n  ...\n1..1\n");

        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.assertions[0].diagnostics, vec!["  ---", "  ok 2", "  ..."]);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn attaches_yaml_after_blank_or_comment_lines() {
        let report = parse_tap("not ok 1 - broken\n\n# separated\n  ---\n  message: raw\n  ...\n");

        assert_eq!(
            report.assertions[0].diagnostics,
            vec!["  ---".to_owned(), "  message: raw".to_owned(), "  ...".to_owned()]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn rejected_yaml_marker_clears_adjacency_for_later_markers() {
        let report = parse_tap("not ok 1 - broken\n   ---\n  ---\n  message: raw\n  ...\n");

        assert_eq!(report.assertions[0].diagnostics, Vec::<String>::new());
        assert_eq!(report.raw_lines, vec!["   ---", "  ---", "  message: raw", "  ..."]);
        assert_eq!(report.diagnostics.len(), 2);
    }

    #[test]
    fn rejects_non_ascii_yaml_indentation() {
        let report = parse_tap("not ok 1 - broken\n\u{00a0}  ---\n  message: raw\n  ...\n");

        assert_eq!(report.assertions[0].diagnostics, Vec::<String>::new());
        assert!(report.raw_lines.iter().any(|line| line.contains("---")));
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("not attached to the preceding assertion"))
        );
    }

    #[test]
    fn does_not_treat_a_bailout_prefix_as_a_bailout() {
        let report = parse_tap("Bail out!ish text\nok 1 - valid\n1..1\n");

        assert_eq!(report.bail_out, None);
        assert_eq!(report.assertions.len(), 1);
        assert_eq!(report.raw_lines, vec!["Bail out!ish text"]);
        assert!(report.is_success());
    }

    #[test]
    fn preserves_subtest_depth_and_validates_only_the_top_level_plan() {
        let report = parse_tap("TAP version 13\n    1..1\n    ok 1 - inner\nok 1 - child\n1..1\n");

        assert_eq!(report.assertions.len(), 2);
        assert_eq!(report.assertions[0].depth, 1);
        assert_eq!(report.assertions[1].depth, 0);
        assert_eq!(report.raw_lines, vec!["    1..1"]);
        assert!(report.diagnostics.is_empty());
        assert!(report.is_success());
    }

    #[test]
    fn reports_invalid_plan_and_assertion_outside_plan() {
        let invalid = parse_tap("1..oops\nok 1 - check\n");
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| diagnostic.contains("invalid TAP plan"))
        );

        let outside = parse_tap("1..1\nok 2 - outside\n");
        assert!(outside.diagnostics.iter().any(|diagnostic| diagnostic.contains("outside plan")));
        assert!(outside.is_success());
    }

    #[test]
    fn reports_unterminated_yaml_diagnostics() {
        let report = parse_tap("not ok 1 - broken\n  ---\n  message: incomplete\n");

        assert_eq!(report.assertions[0].diagnostics.len(), 2);
        assert!(report.diagnostics.iter().any(|diagnostic| diagnostic.contains("unterminated")));
    }
}
