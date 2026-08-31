use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::heredoc_anti_patterns::model::{AntiPattern, Diagnostic, Location, Severity};
use crate::heredoc_anti_patterns::utils::{
    build_line_starts, location_from_start, mask_non_code_regions,
};

/// Scans Perl source for heredoc-related anti-patterns and produces [`Diagnostic`]s.
///
/// Construct with [`AntiPatternDetector::new`], then call [`detect_all`] with the
/// source text. The detector runs all seven built-in pattern checkers and returns
/// the results sorted by byte offset so callers receive problems in source order.
///
/// [`detect_all`]: AntiPatternDetector::detect_all
pub struct AntiPatternDetector {
    patterns: Vec<Box<dyn PatternDetector>>,
}

trait PatternDetector: Send + Sync {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)>;
    fn diagnose(&self, pattern: &AntiPattern) -> Option<Diagnostic>;
}

// Format heredoc detector
struct FormatHeredocDetector;

/// Pattern for identifying format declarations
static FORMAT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"(?m)^\s*format\s+(\w+)\s*=\s*$") {
        Ok(re) => re,
        Err(_) => unreachable!("FORMAT_PATTERN regex failed to compile"),
    });

/// Pattern for extracting heredoc delimiter declarations.
static HEREDOC_DELIMITER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r#"<<\s*['"`]?([A-Za-z_][A-Za-z0-9_]*)['"`]?"#) {
        Ok(re) => re,
        Err(_) => unreachable!("HEREDOC_DELIMITER_PATTERN regex failed to compile"),
    });

impl PatternDetector for FormatHeredocDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();
        let scan_code = mask_non_code_regions(code);

        for cap in FORMAT_PATTERN.captures_iter(&scan_code) {
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
                            heredoc_delimiter: extract_heredoc_delimiter(source_body),
                        },
                        location,
                    ));
                }
            }
        }

        results
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

// BEGIN-time heredoc detector
struct BeginTimeHeredocDetector;

/// Pattern for identifying BEGIN block openings
static BEGIN_BLOCK_START_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"\bBEGIN\s*\{") {
        Ok(re) => re,
        Err(_) => unreachable!("BEGIN_BLOCK_START_PATTERN regex failed to compile"),
    });

fn extract_heredoc_delimiter(body: &str) -> String {
    HEREDOC_DELIMITER_PATTERN
        .captures(body)
        .and_then(|captures| captures.get(1).map(|delimiter| delimiter.as_str().to_string()))
        .unwrap_or_else(|| "UNKNOWN".to_string())
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

impl PatternDetector for BeginTimeHeredocDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();
        let scan_code = mask_non_code_regions(code);

        for begin_match in BEGIN_BLOCK_START_PATTERN.find_iter(&scan_code) {
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

// Dynamic delimiter detector
struct DynamicDelimiterDetector;

/// Pattern for identifying dynamic heredoc delimiters.
///
/// Deliberately keeps the newline horizon that #3597 removed from the regex
/// code block and eval patterns. Those two constructs must span newlines to
/// reach a terminator; a dynamic delimiter has no such need, so widening this
/// one would only add false positives on multi-line left shifts such as
/// `1 << ${\nfoo}` without recovering any real detection.
static DYNAMIC_DELIMITER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"<<\s*\$\{[^}\n]+\}|<<\s*\$\w+|<<\s*`[^`\n]+`") {
        Ok(re) => re,
        Err(_) => unreachable!("DYNAMIC_DELIMITER_PATTERN regex failed to compile"),
    });

impl PatternDetector for DynamicDelimiterDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();
        let scan_code = mask_non_code_regions(code);

        for cap in DYNAMIC_DELIMITER_PATTERN.captures_iter(&scan_code) {
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

// Source filter detector
struct SourceFilterDetector;

/// Pattern for identifying common source filter modules
static SOURCE_FILTER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    match Regex::new(r"use\s+Filter::(Simple|Util::Call|cpp|exec|sh|decrypt|tee)") {
        Ok(re) => re,
        Err(_) => unreachable!("SOURCE_FILTER_PATTERN regex failed to compile"),
    }
});

impl PatternDetector for SourceFilterDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();
        let scan_code = mask_non_code_regions(code);

        for cap in SOURCE_FILTER_PATTERN.captures_iter(&scan_code) {
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

// Regex heredoc detector
struct RegexHeredocDetector;

/// A heredoc declaration: `<<EOF`, `<<'EOF'`, `<<"EOF"`, and the `<<~` indented
/// forms. A bare delimiter must be adjacent to `<<` — whitespace before an
/// unquoted word makes it a left shift, not a heredoc — while the quoted forms
/// may be separated.
static HEREDOC_DECL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    match Regex::new(r#"<<(~?)(?:\s*'([^'\n]*)'|\s*"([^"\n]*)"|([A-Za-z_]\w*))"#) {
        Ok(re) => re,
        Err(_) => unreachable!("HEREDOC_DECL_PATTERN regex failed to compile"),
    }
});

/// Byte ranges of heredoc *bodies* in `code`, terminator line included and the
/// `<<DELIM` declaration excluded.
///
/// `scan_code` is the masked view, used only to reject a declaration that
/// begins inside a comment or string literal. Both views substitute
/// byte-for-byte, so offsets index either identically.
fn heredoc_body_ranges(code: &str, scan_code: &str) -> Vec<(usize, usize)> {
    let declarations: Vec<(usize, bool, &str)> = HEREDOC_DECL_PATTERN
        .captures_iter(code)
        .filter_map(|capture| {
            let whole = capture.get(0)?;
            if scan_code.get(whole.start()..whole.start() + 2) != Some("<<") {
                return None;
            }
            let indented = capture.get(1).is_some_and(|tilde| !tilde.as_str().is_empty());
            let delimiter =
                capture.get(2).or_else(|| capture.get(3)).or_else(|| capture.get(4))?.as_str();
            (!delimiter.is_empty()).then_some((whole.start(), indented, delimiter))
        })
        .collect();

    if declarations.is_empty() {
        return Vec::new();
    }

    // (start, end-without-newline, end-with-newline) for each line.
    let mut lines = Vec::new();
    let mut line_start = 0;
    for (idx, byte) in code.bytes().enumerate() {
        if byte == b'\n' {
            lines.push((line_start, idx, idx + 1));
            line_start = idx + 1;
        }
    }
    if line_start <= code.len() {
        lines.push((line_start, code.len(), code.len()));
    }

    let mut ranges = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let (start, end, after) = lines[index];
        // Declarations on this line stack: their bodies follow in order.
        let on_this_line: Vec<_> =
            declarations.iter().filter(|(at, _, _)| *at >= start && *at < end).collect();

        if on_this_line.is_empty() {
            index += 1;
            continue;
        }

        let body_start = after;
        let mut cursor = index + 1;

        for (_, indented, delimiter) in on_this_line {
            while cursor < lines.len() {
                let (body_line_start, body_line_end, _) = lines[cursor];
                let text = &code[body_line_start..body_line_end];
                let terminated =
                    if *indented { text.trim() == *delimiter } else { text == *delimiter };
                cursor += 1;
                if terminated {
                    break;
                }
            }
        }

        if cursor > index + 1 {
            let body_end = lines[cursor - 1].2;
            if body_end > body_start {
                ranges.push((body_start, body_end));
            }
        }

        index = cursor.max(index + 1);
    }

    ranges
}

/// Blank `ranges` in `scan_code`, preserving newlines and byte length so the
/// result still indexes identically to the source.
fn blank_ranges(scan_code: &str, ranges: &[(usize, usize)]) -> String {
    if ranges.is_empty() {
        return scan_code.to_string();
    }

    let mut bytes = scan_code.as_bytes().to_vec();
    for &(start, end) in ranges {
        let end = end.min(bytes.len());
        if start >= end {
            continue;
        }
        for byte in &mut bytes[start..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }

    // Ranges are line-aligned, so no multi-byte character is split and every
    // replacement byte is ASCII; the fallback keeps this total regardless.
    String::from_utf8(bytes).unwrap_or_else(|_| scan_code.to_string())
}

fn regex_code_block_matches(scan_code: &str) -> Vec<usize> {
    let mut matches = Vec::new();
    let mut search_from = 0;

    while let Some(relative_start) = scan_code[search_from..].find("(?{") {
        let start = search_from + relative_start;
        let opening_brace = start + 2;

        // A malformed outer block cannot contain a complete diagnostic. Stop
        // here rather than rescanning the same suffix from every nested
        // candidate and turning malformed input into quadratic work.
        let Some(closing_brace) = find_matching_brace(scan_code, opening_brace) else {
            break;
        };

        if scan_code[opening_brace + 1..closing_brace].contains("<<") {
            matches.push(start);
        }

        search_from = closing_brace + 1;
    }

    matches
}

impl PatternDetector for RegexHeredocDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        // Braces in heredoc *text* are data, not Perl block structure, and
        // `mask_non_code_regions` does not blank heredoc bodies. Without this
        // second pass an unmatched `{` in a body suppresses the diagnostic —
        // and, because the scan stops at an unmatched outer block, every later
        // one too — while a `}` in a body can fabricate a block boundary that
        // was never there. Masking locally keeps the shared mask, which feeds
        // all seven detectors, unchanged (#14352).
        let masked = mask_non_code_regions(code);
        let scan_code = blank_ranges(&masked, &heredoc_body_ranges(code, &masked));
        regex_code_block_matches(&scan_code)
            .into_iter()
            .map(|start| {
                let location = location_from_start(line_starts, offset, start);
                (AntiPattern::RegexCodeBlockHeredoc { location: location.clone() }, location)
            })
            .collect()
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

// Eval heredoc detector
struct EvalHeredocDetector;

/// The keyword every [`EVAL_HEREDOC_PATTERN`] match starts with, used to check a
/// match origin against the masked view of the source.
const EVAL_KEYWORD: &str = "eval";

/// Pattern for identifying heredocs inside eval strings.
///
/// An `eval` string that declares a heredoc must span newlines to reach its
/// terminator, so the class is bounded by the closing quote alone rather than by
/// a newline horizon. See the module docs for the governing measurement (#3597).
static EVAL_HEREDOC_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r#"\beval\s+(?:'[^']*<<[^']*'|"[^"]*<<[^"]*")"#) {
        Ok(re) => re,
        Err(_) => unreachable!("EVAL_HEREDOC_PATTERN regex failed to compile"),
    });

/// Whether the `eval` matched at `start` is the builtin rather than a lookalike.
///
/// `scan_code` is the masked view, which substitutes byte-for-byte, so `start`
/// indexes it and the raw source identically. Two lookalikes are rejected:
///
/// * a match seeded inside a comment or string literal — masking blanks those,
///   so the keyword no longer reads as `eval` at this offset;
/// * a package-qualified call such as `Foo::eval`, which the pattern's leading
///   `\b` admits because `:` is not a word character.
///
/// `CORE::eval` is the one qualified spelling that stays: it explicitly names
/// the builtin and bypasses any override. `CORE::GLOBAL::eval` deliberately does
/// *not* — that package is the override slot, so calling it by name invokes a
/// user-defined replacement, which is the same "some other function" case as
/// `Foo::eval`.
fn eval_match_is_builtin(scan_code: &str, start: usize) -> bool {
    if scan_code.get(start..start + EVAL_KEYWORD.len()) != Some(EVAL_KEYWORD) {
        return false;
    }

    let prefix = &scan_code[..start];
    let path_start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_' || *ch == ':'))
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());

    match prefix[path_start..].strip_suffix("::") {
        None => true,
        Some(qualifier) => qualifier == "CORE",
    }
}

impl PatternDetector for EvalHeredocDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();

        // This detector must scan raw source: masking blanks the contents of
        // the very quoted string it needs to look inside. So the mask is used
        // only to reject matches that *begin* in a comment or string literal.
        // `mask_non_code_regions` substitutes byte-for-byte, so offsets align.
        let scan_code = mask_non_code_regions(code);

        for cap in EVAL_HEREDOC_PATTERN.captures_iter(code) {
            if let Some(match_pos) = cap.get(0) {
                let start = match_pos.start();
                if !eval_match_is_builtin(&scan_code, start) {
                    continue;
                }

                let location = location_from_start(line_starts, offset, start);

                results.push((
                    AntiPattern::EvalStringHeredoc { location: location.clone() },
                    location,
                ));
            }
        }

        results
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

// Tied handle detector
struct TiedHandleDetector;

/// Pattern for identifying tie statements
static TIE_PATTERN: LazyLock<Regex> = LazyLock::new(|| match Regex::new(r"tie\s+([*$]\w+)") {
    Ok(re) => re,
    Err(_) => unreachable!("TIE_PATTERN regex failed to compile"),
});

/// Pattern for identifying print statements that write heredocs to a handle.
static PRINT_HEREDOC_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"print\s+([*$]?\w+)\s+<<") {
        Ok(re) => re,
        Err(_) => unreachable!("PRINT_HEREDOC_PATTERN regex failed to compile"),
    });

impl PatternDetector for TiedHandleDetector {
    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();
        let scan_code = mask_non_code_regions(code);

        // First collect tied handles in normalized form:
        // *FH -> FH, $fh -> $fh.
        let mut tied_handles = HashSet::new();
        for cap in TIE_PATTERN.captures_iter(&scan_code) {
            if let Some(handle_match) = cap.get(1) {
                let raw_handle = handle_match.as_str();
                let normalized = raw_handle.strip_prefix('*').unwrap_or(raw_handle);
                tied_handles.insert(normalized.to_string());
            }
        }

        // Use a single static regex for all print-heredoc matches, then filter
        // by whether the handle is in the tied set. This avoids O(n) Regex
        // compilations (one per tied handle) and is faster for large files.
        for cap in PRINT_HEREDOC_PATTERN.captures_iter(&scan_code) {
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

impl Default for AntiPatternDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl AntiPatternDetector {
    /// Create a detector pre-loaded with all seven built-in pattern checkers.
    pub fn new() -> Self {
        Self {
            patterns: vec![
                Box::new(FormatHeredocDetector),
                Box::new(BeginTimeHeredocDetector),
                Box::new(DynamicDelimiterDetector),
                Box::new(SourceFilterDetector),
                Box::new(RegexHeredocDetector),
                Box::new(EvalHeredocDetector),
                Box::new(TiedHandleDetector),
            ],
        }
    }

    /// Run all pattern checkers against `code` and return diagnostics sorted by offset.
    pub fn detect_all(&self, code: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let line_starts = build_line_starts(code);

        for detector in &self.patterns {
            let patterns = detector.detect(code, 0, &line_starts);
            for (pattern, _) in patterns {
                if let Some(diagnostic) = detector.diagnose(&pattern) {
                    diagnostics.push(diagnostic);
                }
            }
        }

        diagnostics.sort_by_key(|d| match &d.pattern {
            AntiPattern::FormatHeredoc { location, .. }
            | AntiPattern::BeginTimeHeredoc { location, .. }
            | AntiPattern::DynamicHeredocDelimiter { location, .. }
            | AntiPattern::SourceFilterHeredoc { location, .. }
            | AntiPattern::RegexCodeBlockHeredoc { location, .. }
            | AntiPattern::EvalStringHeredoc { location, .. }
            | AntiPattern::TiedHandleHeredoc { location, .. } => location.offset,
        });

        diagnostics
    }

    /// Format a list of diagnostics as a human-readable plain-text report.
    ///
    /// Prints a header, a count, and one entry per diagnostic including its
    /// severity, location, explanation, optional suggested fix, and references.
    pub fn format_report(&self, diagnostics: &[Diagnostic]) -> String {
        let mut report = String::from("Anti-Pattern Analysis Report\n");
        report.push_str("============================\n\n");

        if diagnostics.is_empty() {
            report.push_str("No problematic patterns detected.\n");
            return report;
        }

        report.push_str(&format!("Found {} problematic patterns:\n\n", diagnostics.len()));

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

        report
    }
}

#[cfg(test)]
mod tests;
