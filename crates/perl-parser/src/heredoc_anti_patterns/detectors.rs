use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::heredoc_anti_patterns::model::{
    AntiPattern, DetectionReport, DetectionStatus, DetectorFailureReason, DetectorId,
    DetectorObservation, DetectorState, Diagnostic, HeredocDelimiter, Location, Severity,
};
use crate::heredoc_anti_patterns::utils::{
    build_line_starts, location_from_start, mask_non_code_regions,
};

/// Scans Perl source for heredoc-related anti-patterns and produces [`Diagnostic`]s.
///
/// Construct with [`AntiPatternDetector::new`], then call [`detect_all_report`]
/// with the source text. [`detect_all`] is a diagnostics-only projection and is
/// not the completeness authority: an empty vector can be a complete-clean scan
/// or a partial scan with unavailable detectors.
///
/// [`detect_all`]: AntiPatternDetector::detect_all
/// [`detect_all_report`]: AntiPatternDetector::detect_all_report
pub struct AntiPatternDetector {
    patterns: Vec<Box<dyn PatternDetector>>,
}

trait PatternDetector: Send + Sync {
    fn id(&self) -> DetectorId;
    fn availability(&self) -> DetectorState;
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)>;
    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic>;
}

fn compiled(pattern: &'static LazyLock<Result<Regex, regex::Error>>) -> Option<&'static Regex> {
    pattern.as_ref().ok()
}

fn unavailable(pattern_ids: &[&'static str]) -> DetectorState {
    DetectorState::Unavailable {
        reason: DetectorFailureReason::PatternUnavailable { pattern_ids: pattern_ids.to_vec() },
    }
}

fn limited(pattern_ids: &[&'static str]) -> DetectorState {
    DetectorState::Limited {
        reason: DetectorFailureReason::PatternUnavailable { pattern_ids: pattern_ids.to_vec() },
    }
}

fn required_state(required: &[(&'static str, bool)]) -> DetectorState {
    let missing: Vec<&'static str> =
        required.iter().filter(|(_, ok)| !*ok).map(|(id, _)| *id).collect();
    if missing.is_empty() { DetectorState::Complete } else { unavailable(&missing) }
}

fn detection_status(observations: &[DetectorObservation]) -> DetectionStatus {
    if observations.is_empty() {
        return DetectionStatus::Unavailable;
    }
    let any_ran = observations.iter().any(|obs| obs.state.ran());
    let all_complete = observations.iter().all(|obs| matches!(obs.state, DetectorState::Complete));
    if all_complete {
        DetectionStatus::Complete
    } else if any_ran {
        DetectionStatus::Partial
    } else {
        DetectionStatus::Unavailable
    }
}

/// Pattern for identifying format declarations.
static FORMAT_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*format\s+(\w+)\s*=\s*$"));

/// Pattern for extracting heredoc delimiter declarations.
static HEREDOC_DELIMITER_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"<<\s*['"`]?([A-Za-z_][A-Za-z0-9_]*)['"`]?"#));

/// Pattern for identifying BEGIN block openings.
static BEGIN_BLOCK_START_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\bBEGIN\s*\{"));

/// Pattern for identifying dynamic heredoc delimiters.
static DYNAMIC_DELIMITER_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"<<\s*\$\{[^}\n]+\}|<<\s*\$\w+|<<\s*`[^`\n]+`"));

/// Pattern for identifying common source filter modules.
static SOURCE_FILTER_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"use\s+Filter::(Simple|Util::Call|cpp|exec|sh|decrypt|tee)"));

/// Pattern for identifying heredocs inside regex code blocks.
static REGEX_HEREDOC_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\(\?\{[^}\n]*<<[^}\n]*\}"));

/// Pattern for identifying heredocs inside eval strings.
static EVAL_HEREDOC_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"eval\s+(?:'[^\n']*<<[^\n']*'|"[^\n"]*<<[^\n"]*")"#));

/// Pattern for identifying tie statements.
static TIE_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"tie\s+([*$]\w+)"));

/// Pattern for identifying print statements that write heredocs to a handle.
static PRINT_HEREDOC_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"print\s+([*$]?\w+)\s+<<"));

struct FormatHeredocDetector;

impl PatternDetector for FormatHeredocDetector {
    fn id(&self) -> DetectorId {
        DetectorId::FormatHeredoc
    }

    fn availability(&self) -> DetectorState {
        format_availability(
            compiled(&FORMAT_PATTERN).is_some(),
            compiled(&HEREDOC_DELIMITER_PATTERN).is_some(),
        )
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_format_heredoc(
            code,
            offset,
            line_starts,
            compiled(&FORMAT_PATTERN),
            compiled(&HEREDOC_DELIMITER_PATTERN),
        )
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        let AntiPattern::FormatHeredoc { format_name, .. } = pattern else {
            return None;
        };

        Some(Diagnostic {
            severity: Severity::Warning,
            pattern: pattern.clone(),
            message: format!("Heredoc declared inside format '{}'", format_name),
            explanation: "Heredocs inside format declarations are often handled specially by the Perl interpreter and can be difficult to parse statically.".to_string(),
            suggested_fix: Some("Consider moving the heredoc outside the format or using a simple string if possible.".to_string()),
            references: vec!["perldoc perlform".to_string()],
        })
    }
}

fn format_availability(format_ok: bool, delimiter_ok: bool) -> DetectorState {
    match (format_ok, delimiter_ok) {
        (true, true) => DetectorState::Complete,
        (true, false) => limited(&["HEREDOC_DELIMITER_PATTERN"]),
        (false, true) => unavailable(&["FORMAT_PATTERN"]),
        (false, false) => unavailable(&["FORMAT_PATTERN", "HEREDOC_DELIMITER_PATTERN"]),
    }
}

fn detect_format_heredoc(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    format_pattern: Option<&Regex>,
    delimiter_pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(format_pattern) = format_pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    for cap in format_pattern.captures_iter(&scan_code) {
        if let (Some(match_pos), Some(name_match)) = (cap.get(0), cap.get(1)) {
            let format_name = name_match.as_str().to_string();
            let location = location_from_start(line_starts, offset, match_pos.start());

            // Look for heredoc marker inside format body (simplified)
            let body_start = match_pos.end();
            let body_end = code[body_start..].find("\n.").unwrap_or(code.len() - body_start);
            let body = &scan_code[body_start..body_start + body_end];
            let source_body = &code[body_start..body_start + body_end];

            if body.contains("<<") {
                results.push((
                    AntiPattern::FormatHeredoc {
                        location: location.clone(),
                        format_name,
                        heredoc_delimiter: extract_heredoc_delimiter_with(
                            delimiter_pattern,
                            source_body,
                        ),
                    },
                    location,
                ));
            }
        }
    }

    results
}

struct BeginTimeHeredocDetector;

impl PatternDetector for BeginTimeHeredocDetector {
    fn id(&self) -> DetectorId {
        DetectorId::BeginTimeHeredoc
    }

    fn availability(&self) -> DetectorState {
        required_state(&[(
            "BEGIN_BLOCK_START_PATTERN",
            compiled(&BEGIN_BLOCK_START_PATTERN).is_some(),
        )])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_begin_time_heredoc(code, offset, line_starts, compiled(&BEGIN_BLOCK_START_PATTERN))
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        if let AntiPattern::BeginTimeHeredoc { .. } = pattern {
            Some(Diagnostic {
                severity: Severity::Error,
                pattern: pattern.clone(),
                message: "Heredoc declared during BEGIN-time".to_string(),
                explanation: "Heredocs declared inside BEGIN blocks are evaluated during the compilation phase. This can lead to complex side effects that are difficult to track statically.".to_string(),
                suggested_fix: Some("Move the heredoc declaration out of the BEGIN block if it doesn't need to be evaluated during compilation.".to_string()),
                references: vec!["perldoc perlmod".to_string()],
            })
        } else {
            None
        }
    }
}

fn detect_begin_time_heredoc(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    begin_pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(begin_pattern) = begin_pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    for begin_match in begin_pattern.find_iter(&scan_code) {
        let Some(opening_brace_rel) = begin_match.as_str().rfind('{') else {
            continue;
        };
        let opening_brace_idx = begin_match.start() + opening_brace_rel;
        let Some(closing_brace_idx) = find_matching_brace(&scan_code, opening_brace_idx) else {
            continue;
        };
        let block_content = &scan_code[opening_brace_idx + 1..closing_brace_idx];

        if !block_content.contains("<<") {
            continue;
        }

        let location = location_from_start(line_starts, offset, begin_match.start());

        results.push((
            AntiPattern::BeginTimeHeredoc {
                location: location.clone(),
                heredoc_content: block_content.to_string(),
                side_effects: vec!["Phase-dependent parsing".to_string()],
            },
            location,
        ));
    }

    results
}

fn extract_heredoc_delimiter_with(pattern: Option<&Regex>, body: &str) -> HeredocDelimiter {
    let Some(regex) = pattern else {
        return HeredocDelimiter::Unavailable;
    };
    regex
        .captures(body)
        .and_then(|captures| captures.get(1).map(|delimiter| delimiter.as_str().to_string()))
        .map(HeredocDelimiter::Extracted)
        .unwrap_or(HeredocDelimiter::Unknown)
}

fn find_matching_brace(code: &str, opening_brace_idx: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut depth = 0usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for (idx, &byte) in bytes.iter().enumerate().skip(opening_brace_idx) {
        let ch = byte as char;

        if escaped {
            escaped = false;
            continue;
        }

        if in_single_quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == '\'' {
                in_single_quote = false;
            }
            continue;
        }

        if in_double_quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double_quote = false;
            }
            continue;
        }

        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }

    None
}

struct DynamicDelimiterDetector;

impl PatternDetector for DynamicDelimiterDetector {
    fn id(&self) -> DetectorId {
        DetectorId::DynamicDelimiter
    }

    fn availability(&self) -> DetectorState {
        required_state(&[(
            "DYNAMIC_DELIMITER_PATTERN",
            compiled(&DYNAMIC_DELIMITER_PATTERN).is_some(),
        )])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_dynamic_delimiter(code, offset, line_starts, compiled(&DYNAMIC_DELIMITER_PATTERN))
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        let AntiPattern::DynamicHeredocDelimiter { expression, .. } = pattern else {
            return None;
        };

        Some(Diagnostic {
            severity: Severity::Warning,
            pattern: pattern.clone(),
            message: format!("Dynamic heredoc delimiter: {}", expression),
            explanation: "Using variables or expressions as heredoc delimiters makes it impossible to know the terminator without executing the code.".to_string(),
            suggested_fix: Some("Use a literal string as the heredoc terminator.".to_string()),
            references: vec!["perldoc perlop".to_string()],
        })
    }
}

fn detect_dynamic_delimiter(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(pattern) = pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    for cap in pattern.captures_iter(&scan_code) {
        if let Some(match_pos) = cap.get(0) {
            let expression = match_pos.as_str().to_string();
            let location = location_from_start(line_starts, offset, match_pos.start());

            results.push((
                AntiPattern::DynamicHeredocDelimiter { location: location.clone(), expression },
                location,
            ));
        }
    }

    results
}

struct SourceFilterDetector;

impl PatternDetector for SourceFilterDetector {
    fn id(&self) -> DetectorId {
        DetectorId::SourceFilter
    }

    fn availability(&self) -> DetectorState {
        required_state(&[("SOURCE_FILTER_PATTERN", compiled(&SOURCE_FILTER_PATTERN).is_some())])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_source_filter(code, offset, line_starts, compiled(&SOURCE_FILTER_PATTERN))
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        let AntiPattern::SourceFilterHeredoc { module, .. } = pattern else {
            return None;
        };

        Some(Diagnostic {
            severity: Severity::Error,
            pattern: pattern.clone(),
            message: format!("Source filter detected: Filter::{}", module),
            explanation: "Source filters rewrite the source code before it's parsed. Static analysis cannot reliably predict the state of the code after filtering.".to_string(),
            suggested_fix: Some("Avoid using source filters. They are considered problematic and often replaced by better alternatives like Devel::Declare or modern Perl features.".to_string()),
            references: vec!["perldoc Filter::Simple".to_string()],
        })
    }
}

fn detect_source_filter(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(pattern) = pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    for cap in pattern.captures_iter(&scan_code) {
        if let (Some(match_pos), Some(module_match)) = (cap.get(0), cap.get(1)) {
            let filter_module = module_match.as_str().to_string();
            let location = location_from_start(line_starts, offset, match_pos.start());

            results.push((
                AntiPattern::SourceFilterHeredoc {
                    location: location.clone(),
                    module: filter_module,
                },
                location,
            ));
        }
    }

    results
}

struct RegexHeredocDetector;

impl PatternDetector for RegexHeredocDetector {
    fn id(&self) -> DetectorId {
        DetectorId::RegexCodeBlock
    }

    fn availability(&self) -> DetectorState {
        required_state(&[("REGEX_HEREDOC_PATTERN", compiled(&REGEX_HEREDOC_PATTERN).is_some())])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_regex_heredoc(code, offset, line_starts, compiled(&REGEX_HEREDOC_PATTERN))
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        if let AntiPattern::RegexCodeBlockHeredoc { .. } = pattern {
            Some(Diagnostic {
                severity: Severity::Warning,
                pattern: pattern.clone(),
                message: "Heredoc inside regex code block".to_string(),
                explanation: "Declaring heredocs inside (?{ ... }) or (??{ ... }) blocks is extremely rare and difficult to parse correctly.".to_string(),
                suggested_fix: None,
                references: vec!["perldoc perlre".to_string()],
            })
        } else {
            None
        }
    }
}

fn detect_regex_heredoc(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(pattern) = pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    for cap in pattern.captures_iter(&scan_code) {
        if let Some(match_pos) = cap.get(0) {
            let location = location_from_start(line_starts, offset, match_pos.start());

            results.push((
                AntiPattern::RegexCodeBlockHeredoc { location: location.clone() },
                location,
            ));
        }
    }

    results
}

struct EvalHeredocDetector;

impl PatternDetector for EvalHeredocDetector {
    fn id(&self) -> DetectorId {
        DetectorId::EvalString
    }

    fn availability(&self) -> DetectorState {
        required_state(&[("EVAL_HEREDOC_PATTERN", compiled(&EVAL_HEREDOC_PATTERN).is_some())])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_eval_heredoc(code, offset, line_starts, compiled(&EVAL_HEREDOC_PATTERN))
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        if let AntiPattern::EvalStringHeredoc { .. } = pattern {
            Some(Diagnostic {
                severity: Severity::Warning,
                pattern: pattern.clone(),
                message: "Heredoc inside eval string".to_string(),
                explanation: "Heredocs declared inside strings passed to eval require double parsing and can hide malicious or complex code.".to_string(),
                suggested_fix: Some("Consider using a block eval or moving the heredoc outside the eval string.".to_string()),
                references: vec!["perldoc -f eval".to_string()],
            })
        } else {
            None
        }
    }
}

fn detect_eval_heredoc(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let Some(pattern) = pattern else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for cap in pattern.captures_iter(code) {
        if let Some(match_pos) = cap.get(0) {
            let location = location_from_start(line_starts, offset, match_pos.start());

            results.push((AntiPattern::EvalStringHeredoc { location: location.clone() }, location));
        }
    }

    results
}

struct TiedHandleDetector;

impl PatternDetector for TiedHandleDetector {
    fn id(&self) -> DetectorId {
        DetectorId::TiedHandle
    }

    fn availability(&self) -> DetectorState {
        required_state(&[
            ("TIE_PATTERN", compiled(&TIE_PATTERN).is_some()),
            ("PRINT_HEREDOC_PATTERN", compiled(&PRINT_HEREDOC_PATTERN).is_some()),
        ])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_tied_handle(
            code,
            offset,
            line_starts,
            compiled(&TIE_PATTERN),
            compiled(&PRINT_HEREDOC_PATTERN),
        )
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        let AntiPattern::TiedHandleHeredoc { handle_name, .. } = pattern else {
            return None;
        };

        Some(Diagnostic {
            severity: Severity::Info,
            pattern: pattern.clone(),
            message: format!("Heredoc written to tied handle '{}'", handle_name),
            explanation: "Writing to a tied handle invokes custom code. The behavior of heredoc output depends on the tied class implementation.".to_string(),
            suggested_fix: None,
            references: vec!["perldoc -f tie".to_string()],
        })
    }
}

fn detect_tied_handle(
    code: &str,
    offset: usize,
    line_starts: &[usize],
    tie_pattern: Option<&Regex>,
    print_pattern: Option<&Regex>,
) -> Vec<(AntiPattern, Location)> {
    let (Some(tie_pattern), Some(print_pattern)) = (tie_pattern, print_pattern) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    let scan_code = mask_non_code_regions(code);

    // First collect tied handles in normalized form:
    // *FH -> FH, $fh -> $fh.
    let mut tied_handles = HashSet::new();
    for cap in tie_pattern.captures_iter(&scan_code) {
        if let Some(handle_match) = cap.get(1) {
            let raw_handle = handle_match.as_str();
            let normalized = raw_handle.strip_prefix('*').unwrap_or(raw_handle);
            tied_handles.insert(normalized.to_string());
        }
    }

    // Use a single static regex for all print-heredoc matches, then filter
    // by whether the handle is in the tied set. This avoids O(n) Regex
    // compilations (one per tied handle) and is faster for large files.
    for cap in print_pattern.captures_iter(&scan_code) {
        let (Some(match_pos), Some(handle_match)) = (cap.get(0), cap.get(1)) else {
            continue;
        };

        let raw_print_handle = handle_match.as_str();
        let normalized_print_handle =
            raw_print_handle.strip_prefix('*').unwrap_or(raw_print_handle);

        if tied_handles.contains(normalized_print_handle) {
            let location = location_from_start(line_starts, offset, match_pos.start());
            results.push((
                AntiPattern::TiedHandleHeredoc {
                    location: location.clone(),
                    handle_name: normalized_print_handle.to_string(),
                },
                location,
            ));
        }
    }

    results
}

#[cfg(test)]
struct ForcedUnavailableDetector {
    id: DetectorId,
    reason: DetectorFailureReason,
}

#[cfg(test)]
impl PatternDetector for ForcedUnavailableDetector {
    fn id(&self) -> DetectorId {
        self.id
    }

    fn availability(&self) -> DetectorState {
        DetectorState::Unavailable { reason: self.reason.clone() }
    }

    fn detect(
        &self,
        _code: &str,
        _offset: usize,
        _line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        Vec::new()
    }

    fn diagnose(&self, _pattern: &AntiPattern) -> Option<Diagnostic> {
        None
    }
}

impl Default for AntiPatternDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn production_pattern_detectors() -> Vec<Box<dyn PatternDetector>> {
    vec![
        Box::new(FormatHeredocDetector),
        Box::new(BeginTimeHeredocDetector),
        Box::new(DynamicDelimiterDetector),
        Box::new(SourceFilterDetector),
        Box::new(RegexHeredocDetector),
        Box::new(EvalHeredocDetector),
        Box::new(TiedHandleDetector),
    ]
}

#[cfg(test)]
struct ForcedLimitedFormatDetector;

#[cfg(test)]
impl PatternDetector for ForcedLimitedFormatDetector {
    fn id(&self) -> DetectorId {
        DetectorId::FormatHeredoc
    }

    fn availability(&self) -> DetectorState {
        limited(&["HEREDOC_DELIMITER_PATTERN"])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        detect_format_heredoc(code, offset, line_starts, compiled(&FORMAT_PATTERN), None)
    }

    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic> {
        FormatHeredocDetector.diagnose(pattern)
    }
}

impl AntiPatternDetector {
    /// Create a detector pre-loaded with all seven built-in pattern checkers.
    pub fn new() -> Self {
        Self { patterns: production_pattern_detectors() }
    }

    #[cfg(test)]
    fn from_pattern_detectors(patterns: Vec<Box<dyn PatternDetector>>) -> Self {
        Self { patterns }
    }

    /// Run all pattern checkers against `code` and return diagnostics sorted by offset.
    ///
    /// This is a diagnostics-only compatibility projection of
    /// [`Self::detect_all_report`]. An empty vector does not mean the scan was
    /// complete. Completeness-sensitive callers must use the report.
    pub fn detect_all(&self, code: &str) -> Vec<Diagnostic> {
        self.detect_all_report(code).diagnostics
    }

    /// Run all pattern checkers and return findings plus per-detector status.
    ///
    /// A failed pattern disables only the detector that depends on it.
    /// Independent detectors still run. Partial and unavailable scans are
    /// distinct from a complete clean result.
    pub fn detect_all_report(&self, code: &str) -> DetectionReport {
        let mut diagnostics = Vec::new();
        let mut detectors = Vec::with_capacity(self.patterns.len());
        let line_starts = build_line_starts(code);

        for detector in &self.patterns {
            let state = detector.availability();
            if state.ran() {
                for (pattern, _) in detector.detect(code, 0, &line_starts) {
                    if let Some(diagnostic) = detector.diagnose(&pattern) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
            detectors.push(DetectorObservation { id: detector.id(), state });
        }

        detectors.sort_by_key(|obs| obs.id);
        diagnostics.sort_by_key(|diagnostic| diagnostic.pattern.offset());

        DetectionReport { diagnostics, detectors, status: detection_status(&detectors) }
    }

    /// Format a list of diagnostics as a human-readable plain-text report.
    ///
    /// Prints a header, a count, and one entry per diagnostic including its
    /// severity, location, explanation, optional suggested fix, and references.
    ///
    /// This projection cannot express scan completeness. An empty slice prints
    /// as "no problematic patterns" even when detectors were unavailable.
    /// Completeness-sensitive callers must use [`Self::format_detection_report`].
    pub fn format_report(&self, diagnostics: &[Diagnostic]) -> String {
        let mut report = String::from("Anti-Pattern Analysis Report\n");
        report.push_str("============================\n\n");

        if diagnostics.is_empty() {
            report.push_str("No problematic patterns detected.\n");
            return report;
        }

        report.push_str(&format!("Found {} problematic patterns:\n\n", diagnostics.len()));
        self.append_diagnostic_entries(&mut report, diagnostics);
        report
    }

    /// Format a [`DetectionReport`], including completeness status.
    ///
    /// Partial-empty and unavailable scans are not printed as complete-clean.
    pub fn format_detection_report(&self, detection: &DetectionReport) -> String {
        let mut report = String::from("Anti-Pattern Analysis Report\n");
        report.push_str("============================\n\n");
        report.push_str(&format!("Status: {}\n", detection.status.as_str()));

        for observation in &detection.detectors {
            report.push_str(&format!(
                "Detector {}: {}\n",
                observation.id.as_str(),
                detector_state_label(&observation.state)
            ));
        }
        report.push('\n');

        match detection.status {
            DetectionStatus::Unavailable => {
                report.push_str("Analysis unavailable: no detector completed.\n");
            }
            DetectionStatus::Partial if detection.diagnostics.is_empty() => {
                report.push_str(
                    "Partial analysis: one or more detectors were unavailable. No findings from available detectors.\n",
                );
            }
            DetectionStatus::Complete if detection.diagnostics.is_empty() => {
                report.push_str("No problematic patterns detected.\n");
            }
            _ => {
                report.push_str(&format!(
                    "Found {} problematic patterns:\n\n",
                    detection.diagnostics.len()
                ));
                self.append_diagnostic_entries(&mut report, &detection.diagnostics);
            }
        }

        report
    }

    fn append_diagnostic_entries(&self, report: &mut String, diagnostics: &[Diagnostic]) {
        for (i, diag) in diagnostics.iter().enumerate() {
            report.push_str(&format!(
                "{}. {} ({})\n",
                i + 1,
                diag.message,
                match diag.severity {
                    Severity::Error => "ERROR",
                    Severity::Warning => "WARNING",
                    Severity::Info => "INFO",
                }
            ));

            report.push_str(&format!(
                "   Location: {}\n",
                match &diag.pattern {
                    AntiPattern::FormatHeredoc { location, .. }
                    | AntiPattern::BeginTimeHeredoc { location, .. }
                    | AntiPattern::DynamicHeredocDelimiter { location, .. }
                    | AntiPattern::SourceFilterHeredoc { location, .. }
                    | AntiPattern::RegexCodeBlockHeredoc { location, .. }
                    | AntiPattern::EvalStringHeredoc { location, .. }
                    | AntiPattern::TiedHandleHeredoc { location, .. } =>
                        format!("line {}, column {}", location.line, location.column),
                }
            ));

            report.push_str(&format!("   Explanation: {}\n", diag.explanation));

            if let Some(fix) = &diag.suggested_fix {
                report.push_str(&format!(
                    "   Suggested fix:\n     {}\n",
                    fix.lines().collect::<Vec<_>>().join("\n     ")
                ));
            }

            if !diag.references.is_empty() {
                report.push_str(&format!("   References: {}\n", diag.references.join(", ")));
            }

            report.push('\n');
        }
    }
}

fn detector_state_label(state: &DetectorState) -> String {
    match state {
        DetectorState::Complete => "complete".to_string(),
        DetectorState::Limited { reason } => {
            format!("limited ({})", failure_pattern_ids(reason).join(", "))
        }
        DetectorState::Unavailable { reason } => {
            format!("unavailable ({})", failure_pattern_ids(reason).join(", "))
        }
    }
}

fn failure_pattern_ids(reason: &DetectorFailureReason) -> &[&'static str] {
    match reason {
        DetectorFailureReason::PatternUnavailable { pattern_ids } => pattern_ids,
    }
}

#[cfg(test)]
mod tests;
