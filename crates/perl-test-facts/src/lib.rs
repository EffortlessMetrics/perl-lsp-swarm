//! Pure TAP result facts for native Perl test intelligence.
//!
//! This crate parses TAP text only. Test execution, subprocess management, and
//! source-file discovery belong to runtime adapters and workspace consumers.

#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

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
    /// Optional assertion description.
    pub name: Option<String>,
    /// Optional directive, including its reason.
    pub directive: Option<String>,
    /// Raw YAML diagnostic block lines associated with this assertion.
    pub diagnostics: Vec<String>,
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
        self.count(TapAssertionStatus::Pass)
    }

    /// Return the number of failing assertions.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.count(TapAssertionStatus::Fail)
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
    /// [`Self::diagnostics`] and the plan separately.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.bail_out.is_none() && self.failed_count() == 0
    }
}

/// Parse TAP text into stable, execution-independent facts.
///
/// The parser intentionally retains unsupported or malformed constructs as
/// diagnostics. It does not execute commands, interpret YAML, or infer source
/// locations beyond TAP line numbers.
#[must_use]
pub fn parse_tap(source: &str) -> TapReport {
    let mut report = TapReport::default();
    let mut yaml_block: Option<Vec<String>> = None;

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let depth = indentation_depth(line);

        if let Some(block) = yaml_block.as_mut() {
            block.push(line.to_owned());
            if line.trim() == "..." {
                if let Some(assertion) = report.assertions.last_mut() {
                    assertion.diagnostics.append(block);
                } else {
                    report.diagnostics.push(format!(
                        "line {line_number}: YAML diagnostics have no preceding assertion"
                    ));
                }
                yaml_block = None;
            }
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("TAP version") {
            if depth != 0 {
                report.raw_lines.push(line.to_owned());
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
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Bail out!") {
            if report.bail_out.is_some() {
                report.diagnostics.push(format!("line {line_number}: duplicate bailout"));
            } else {
                report.bail_out = Some(rest.trim().to_owned());
            }
            continue;
        }

        if trimmed == "---" {
            yaml_block = Some(vec![line.to_owned()]);
            continue;
        }

        if depth == 0 {
            if let Some(plan) = parse_plan(trimmed, line_number) {
                if let Some(existing) = report.plan.as_ref() {
                    report.diagnostics.push(format!(
                        "line {line_number}: duplicate TAP plan; first plan was on line {}",
                        existing.line
                    ));
                } else {
                    report.plan = Some(plan);
                }
                continue;
            }

            if looks_like_plan(trimmed) {
                report.diagnostics.push(format!("line {line_number}: invalid TAP plan"));
                continue;
            }
        } else if looks_like_plan(trimmed) {
            // A subtest owns its own plan. The first implementation exposes
            // the top-level plan and retains nested protocol records as raw
            // evidence until nested-plan facts have a dedicated model.
            report.raw_lines.push(line.to_owned());
            continue;
        }

        if let Some(assertion) = parse_assertion(trimmed, line_number, depth) {
            report.assertions.push(assertion);
            continue;
        }

        report.raw_lines.push(line.to_owned());
    }

    if let Some(block) = yaml_block {
        if let Some(assertion) = report.assertions.last_mut() {
            assertion.diagnostics.extend(block);
            report.diagnostics.push("unterminated YAML diagnostics block".to_owned());
        } else {
            report.diagnostics.push("YAML diagnostics have no preceding assertion".to_owned());
        }
    }

    validate_plan(&mut report);
    report
}

fn indentation_depth(line: &str) -> usize {
    let mut spaces = 0usize;
    for character in line.chars() {
        match character {
            ' ' => spaces += 1,
            '\t' => spaces += 4,
            _ => break,
        }
    }
    spaces / 4
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

    let (number, remainder) = parse_number(remainder);
    let (name, directive) = split_directive(remainder);
    let (status, directive) = classify_directive(status, directive);
    Some(TapAssertion {
        line: line_number,
        depth,
        number,
        status,
        name,
        directive,
        diagnostics: Vec::new(),
    })
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
    let Some((name, directive)) = remainder.split_once('#') else {
        return (normalize_name(remainder), None);
    };
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
        Err(_) => usize::MAX,
    };
    let top_level_count = report.assertions.iter().filter(|assertion| assertion.depth == 0).count();
    if expected_count != top_level_count {
        report.diagnostics.push(format!(
            "plan on line {} declares {expected} assertions but {} were parsed",
            plan.line, top_level_count
        ));
    }
    for assertion in &report.assertions {
        if assertion.depth == 0
            && let Some(number) = assertion.number
            && (number < plan.start || number > plan.end)
        {
            report.diagnostics.push(format!(
                "line {}: assertion number {number} is outside plan {}..{}",
                assertion.line, plan.start, plan.end
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TapAssertionStatus, parse_tap};

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
    fn reports_unknown_directives_and_unrecognized_lines() {
        let report = parse_tap("ok 1 - check # FLAKY\nthis is not TAP\n1..1\n");

        assert_eq!(report.unknown_count(), 0);
        assert_eq!(report.assertions[0].name.as_deref(), Some("check # FLAKY"));
        assert_eq!(report.raw_lines, vec!["this is not TAP"]);
        assert!(report.diagnostics.is_empty());
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
        let report = parse_tap("not ok 1 - broken\n  ---\n  message: incomplete\n1..1\n");

        assert_eq!(report.assertions[0].diagnostics.len(), 3);
        assert!(report.diagnostics.iter().any(|diagnostic| diagnostic.contains("unterminated")));
    }
}
