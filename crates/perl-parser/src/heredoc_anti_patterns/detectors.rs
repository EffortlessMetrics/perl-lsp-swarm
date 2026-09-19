use regex::Regex;
use std::collections::{HashMap, HashSet};
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
///
/// Deliberately keeps the newline horizon that #3597 removed from the regex
/// code block and eval patterns. Those two constructs must span newlines to
/// reach a terminator; a dynamic delimiter has no such need, so widening this
/// one would only add false positives on multi-line left shifts such as
/// `1 << ${\nfoo}` without recovering any real detection.
static DYNAMIC_DELIMITER_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"<<\s*\$\{[^}\n]+\}|<<\s*\$\w+|<<\s*`[^`\n]+`"));

/// Pattern for identifying common source filter modules.
static SOURCE_FILTER_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"use\s+Filter::(Simple|Util::Call|cpp|exec|sh|decrypt|tee)"));

/// Pattern for identifying heredocs inside eval strings.
///
/// An `eval` string that declares a heredoc must span newlines to reach its
/// terminator, so the class is bounded by the closing quote alone rather than
/// by a newline horizon. See the module docs for the governing measurement
/// (#3597). The regex-code-block detector no longer uses a regex at all: it
/// scans for `(?{`/`(??{` openers and matches braces, so #14390's opener
/// discipline lives in [`regex_code_block_matches`].
static EVAL_HEREDOC_PATTERN: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"\beval\s+(?:'[^']*<<[^']*'|"[^"]*<<[^"]*")"#));

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

/// A heredoc declaration: `<<EOF`, `<<'EOF'`, `<<"EOF"`, ``<<`CMD` ``,
/// `<<\EOF`, and the `<<~` indented forms. A bare or backslash delimiter must
/// be adjacent to `<<` — whitespace before an unquoted word makes it a left
/// shift, not a heredoc — while the quoted forms may be separated. Perl has no
/// `<<-` heredoc; that spelling is shell and is deliberately not matched.
static HEREDOC_DECL_PATTERN: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r#"<<(~?)(?:\s*'([^'\n]*)'|\s*"([^"\n]*)"|\s*`([^`\n]*)`|\\([A-Za-z_]\w*)|([A-Za-z_]\w*))"#,
    )
});

/// Whether the `<<` at `start` is in term position, where Perl reads a heredoc,
/// rather than operator position, where it reads a left shift.
///
/// Perl decides this by what precedes the `<<`, not by how the delimiter is
/// spelled — verified against `perl -c` 5.38, which accepts `1<<FOO`, `$y<<FOO`
/// and `f()<<FOO` with no `FOO` line anywhere, but rejects `print <<FOO` and
/// `my $t = <<FOO` with "Can't find string terminator". So a `<<` following a
/// complete term (a number, a variable, a closing bracket, a string) is a shift,
/// and one following an operator, separator, opener, or bareword function name
/// is a heredoc.
///
/// Errors here are deliberately asymmetric, which is what makes a lexical rule
/// acceptable at all. Answering `false` for a real heredoc only leaves its body
/// unmasked, which is the pre-mask status quo; answering `true` for a shift
/// blanks live code. Every uncertain case therefore resolves to `false`.
///
/// The residual is the unqualified bareword, and it is not fixable lexically:
/// Perl consults the symbol table. `perl -c` 5.38 reads `somefunc<<FOO` as a
/// shift when no such sub is declared, but `Foo::bar<<FOO` as a *heredoc* when
/// `Foo::bar` is defined. A mask that cannot see declarations cannot reproduce
/// that, so barewords stay term position — matching `print`, `say`, `return`
/// and `warn`, which are what actually precede a heredoc — and the fail-safe
/// backstops the rest, since an unmatched operand masks nothing.
fn heredoc_is_in_term_position(code: &str, start: usize) -> bool {
    let prefix = code[..start].trim_end();
    let Some(previous) = prefix.chars().next_back() else {
        return true; // start of file: nothing to shift
    };

    if matches!(previous, ')' | ']' | '\'' | '"' | '`') {
        return false;
    }

    // `print {$fh} <<EOF` and `print ${fh} <<EOF` are heredocs, `$h{k} << FOO`
    // a shift. A brace group is an indirect filehandle only when a list
    // operator introduces it — directly for the block form, or through the
    // sigil for the braced-scalar form.
    if previous == '}' {
        let Some(open) = matching_open_brace(prefix) else {
            return false;
        };
        let before_brace = &prefix[..open];
        let introducer = before_brace.strip_suffix('$').unwrap_or(before_brace);
        return trailing_bareword(introducer)
            .is_some_and(|word| FILEHANDLE_OPERATORS.contains(&word));
    }

    if !(previous.is_alphanumeric() || previous == '_') {
        return true; // an operator, separator, or opener
    }

    // A word, possibly a `::`-qualified path. A bareword function name
    // (`print <<EOF`) takes a term after it, but a number, a method call, or a
    // qualified name that is not a builtin is itself a complete term.
    let path_start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_' || *ch == ':'))
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    let path = &prefix[path_start..];

    if path.starts_with(|ch: char| ch.is_ascii_digit()) {
        return false;
    }

    let before = &prefix[..path_start];

    // A sigilled variable is a complete term — `$y << FOO` shifts — unless a
    // list operator precedes it, which makes it an indirect filehandle and puts
    // the `<<` back in argument position: `print $fh <<EOF`.
    //
    // Only `$` qualifies. An indirect filehandle slot holds a scalar, a block or
    // a bareword, never an array, hash or code sigil, and `perl -c` 5.38 reads
    // `print @a <<FOO`, `print %h <<FOO` and `print &f <<FOO` as shifts.
    if let Some(without_sigil) = before.strip_suffix('$') {
        return trailing_bareword(without_sigil)
            .is_some_and(|word| FILEHANDLE_OPERATORS.contains(&word));
    }
    if before.ends_with(['@', '%', '&']) {
        return false;
    }

    if before.ends_with("->") {
        return false;
    }

    // `CORE::print <<EOF` is a heredoc but `CORE::time << FOO` is a shift: the
    // list operators take an argument, the nullary builtins are terms. Only the
    // former are admitted, so an unlisted builtin costs masking rather than
    // blanking code.
    if let Some(builtin) = path.strip_prefix("CORE::") {
        return TERM_TAKING_OPERATORS.contains(&builtin);
    }
    // `Foo::CONST << FOO` and `$Foo::bar << FOO` are shifts; a qualified name
    // resolves to a value rather than opening an argument list. `Foo::bar <<EOF`
    // with `Foo::bar` a declared sub is the one heredoc this gives up, and
    // giving it up only costs masking.
    !path.contains("::")
}

/// Perl list operators that take an argument list, so a `<<` after one opens a
/// heredoc. Nullary builtins are deliberately absent: `perl -c` 5.38 reads
/// `CORE::time <<M` as a shift because `CORE::time` is already a complete term.
const TERM_TAKING_OPERATORS: [&str; 9] =
    ["print", "printf", "say", "warn", "die", "return", "push", "join", "sprintf"];

/// The operators that also accept an indirect filehandle before their
/// arguments, as `print $fh <<EOF` and `print {$fh} <<EOF`.
const FILEHANDLE_OPERATORS: [&str; 3] = ["print", "printf", "say"];

/// How far back a filehandle block may be matched from its closing brace.
///
/// A block used as a filehandle holds an expression yielding a handle, so it is
/// short even when written across lines; 256 bytes covers `{$fh}` through
/// `{ $self->{handles}{err} }` with room to spare. Anything longer is not a
/// filehandle block, and declining to mask costs only coverage.
const FILEHANDLE_BLOCK_BUDGET: usize = 256;

/// The identifier ending `prefix`, ignoring trailing whitespace.
fn trailing_bareword(prefix: &str) -> Option<&str> {
    let prefix = prefix.trim_end();
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_'))
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    (start < prefix.len()).then(|| &prefix[start..])
}

/// Offset of the `{` matching the `}` that ends `prefix`.
///
/// The search is capped at [`FILEHANDLE_BLOCK_BUDGET`] bytes rather than at the
/// start of the line: a filehandle block may be written across lines, but it is
/// always short. A fixed budget keeps the cost `O(1)` per candidate, which the
/// bound exists for — an unbounded backwards scan from every `}` that precedes a
/// `<<` is quadratic on adversarial input.
fn matching_open_brace(prefix: &str) -> Option<usize> {
    let floor = prefix.len().saturating_sub(FILEHANDLE_BLOCK_BUDGET);
    let bytes = prefix.as_bytes();
    let mut depth = 0usize;

    for idx in (floor..bytes.len()).rev() {
        match bytes[idx] {
            b'}' => depth += 1,
            b'{' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

/// Byte ranges of heredoc *bodies* in `code`, terminator line included and the
/// `<<DELIM` declaration excluded.
///
/// `scan_code` is the masked view, used only to reject a declaration that
/// begins inside a comment or string literal. Both views substitute
/// byte-for-byte, so offsets index either identically.
///
/// The traversal is monotone and indexed, which is load-bearing rather than
/// stylistic. Walking lines and searching forward for each terminator is
/// quadratic on input this detector must survive: `print <<A;` repeated with no
/// terminator anywhere makes every declaration scan to end of file. Instead each
/// line is registered once in a terminator index, and the declarations — already
/// in source order — are walked with pointers that never move backwards, so the
/// pass is `O(n + d log n)` in lines `n` and declarations `d`.
fn heredoc_body_ranges(code: &str, scan_code: &str) -> Vec<(usize, usize)> {
    let Some(decl_pattern) = compiled(&HEREDOC_DECL_PATTERN) else {
        return Vec::new();
    };

    let declarations: Vec<(usize, bool, &str)> = decl_pattern
        .captures_iter(code)
        .filter_map(|capture| {
            let whole = capture.get(0)?;
            if scan_code.get(whole.start()..whole.start() + 2) != Some("<<") {
                return None;
            }
            if !heredoc_is_in_term_position(code, whole.start()) {
                return None;
            }
            let indented = capture.get(1).is_some_and(|tilde| !tilde.as_str().is_empty());
            let delimiter = (2..=6).find_map(|group| capture.get(group))?.as_str();
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

    // Terminator index: line text to the ascending line numbers carrying it.
    // A plain heredoc ends on a line equal to its delimiter; a `<<~` heredoc
    // ends on a line whose trimmed text equals it, so the two need separate
    // keys — folding them into one map would let an indented line terminate a
    // plain heredoc. Exactly one `\r` is stripped, not a run of them: a CRLF
    // file ends the terminator line with one, and eating more would let
    // `DELIM\r\r` close a heredoc it does not close.
    let mut exact: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut trimmed: HashMap<&str, Vec<usize>> = HashMap::new();
    for (line, &(start, end, _)) in lines.iter().enumerate() {
        let raw = &code[start..end];
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        exact.entry(text).or_default().push(line);
        // `<<~` permits indentation *before* the terminator and nothing after
        // it, so only leading whitespace is removed. `perl -c` 5.38 rejects
        // `  EOF   ` as a terminator ("Can't find string terminator"), and
        // accepting it here would end the body on a line Perl treats as text,
        // exposing the rest of the real body to the brace scan.
        trimmed.entry(text.trim_start()).or_default().push(line);
    }

    // First line at or after `from` whose text terminates `delimiter`.
    let terminator_at = |indented: bool, delimiter: &str, from: usize| -> Option<usize> {
        let index = if indented { &trimmed } else { &exact };
        let lines = index.get(delimiter)?;
        lines.get(lines.partition_point(|&line| line < from)).copied()
    };

    let mut ranges = Vec::new();
    let mut declaration = 0;
    let mut line = 0;
    // First line not already known to sit inside a masked body. Declarations
    // before it are heredoc *text*, not declarations.
    let mut resume_line = 0;

    while declaration < declarations.len() {
        let (at, _, _) = declarations[declaration];
        while line + 1 < lines.len() && at >= lines[line].1 {
            line += 1;
        }

        if line < resume_line {
            declaration += 1;
            continue;
        }

        // Declarations on one line stack: their bodies follow in order.
        let group_start = declaration;
        while declaration < declarations.len() && declarations[declaration].0 < lines[line].1 {
            declaration += 1;
        }

        let body_start = lines[line].2;
        let mut cursor = line + 1;
        // Only bodies whose terminator was actually found may be blanked. A
        // declaration that never terminates — an unterminated heredoc, or a
        // left shift such as `1 << FOO` that only looks like one — must not
        // blank the remainder of the file, because blanking is what hides
        // later constructs from the detector. Mis-reading `<<` then costs
        // nothing rather than blinding every subsequent line.
        let mut terminated_through = None;
        for &(_, indented, delimiter) in &declarations[group_start..declaration] {
            let Some(terminator) = terminator_at(indented, delimiter, cursor) else {
                break;
            };
            cursor = terminator + 1;
            terminated_through = Some(cursor);
        }

        if let Some(end_line) = terminated_through {
            let body_end = lines[end_line - 1].2;
            if body_end > body_start {
                ranges.push((body_start, body_end));
            }
            resume_line = end_line;
        }
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

    // Opener discipline carried over from #14390: `(?{` and the postponed
    // `(??{` both open code blocks; `(???{` is not valid Perl and must not
    // match. `(?{` cannot occur inside `(??{` (the second `?` precedes the
    // brace), so the first opener found is unambiguous.
    while let Some(relative_start) = scan_code[search_from..].find('(') {
        let start = search_from + relative_start;
        let rest = &scan_code.as_bytes()[start..];
        let opening_brace = if rest.starts_with(b"(??{") {
            start + 3
        } else if rest.starts_with(b"(?{") {
            start + 2
        } else {
            search_from = start + 1;
            continue;
        };

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
    fn id(&self) -> DetectorId {
        DetectorId::RegexCodeBlock
    }

    fn availability(&self) -> DetectorState {
        // The opener scan itself is infallible; the body mask it depends on is
        // what can degrade, so that is the pattern whose compilation is
        // reported (#13692).
        required_state(&[("HEREDOC_DECL_PATTERN", compiled(&HEREDOC_DECL_PATTERN).is_some())])
    }

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

struct EvalHeredocDetector;

/// The keyword every [`EVAL_HEREDOC_PATTERN`] match starts with, used to check a
/// match origin against the masked view of the source.
const EVAL_KEYWORD: &str = "eval";

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
    fn id(&self) -> DetectorId {
        DetectorId::EvalString
    }

    fn availability(&self) -> DetectorState {
        required_state(&[
            ("EVAL_HEREDOC_PATTERN", compiled(&EVAL_HEREDOC_PATTERN).is_some()),
            // The body mask feeding the match-origin check.
            ("HEREDOC_DECL_PATTERN", compiled(&HEREDOC_DECL_PATTERN).is_some()),
        ])
    }

    fn detect(
        &self,
        code: &str,
        offset: usize,
        line_starts: &[usize],
    ) -> Vec<(AntiPattern, Location)> {
        let mut results = Vec::new();

        // This detector must scan raw source: masking blanks the contents of
        // the very quoted string it needs to look inside. So the mask is used
        // only to reject matches that *begin* somewhere that is not code.
        // `mask_non_code_regions` substitutes byte-for-byte, so offsets align.
        //
        // Heredoc bodies are part of "not code" here and `mask_non_code_regions`
        // does not blank them, so an `eval '...<<...'` sitting in heredoc *text*
        // would otherwise seed a real PL805 for an eval that never runs. The
        // same body mask the regex detector uses closes that (#14352).
        let Some(pattern) = compiled(&EVAL_HEREDOC_PATTERN) else {
            return Vec::new();
        };
        let masked = mask_non_code_regions(code);
        let scan_code = blank_ranges(&masked, &heredoc_body_ranges(code, &masked));

        for cap in pattern.captures_iter(code) {
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
        let status = detection_status(&detectors);

        DetectionReport { diagnostics, detectors, status }
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
