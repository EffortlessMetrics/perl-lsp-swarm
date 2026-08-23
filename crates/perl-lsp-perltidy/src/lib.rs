//! Native-first Perl formatting with optional `perltidy` compatibility.
//!
//! This crate isolates Perl formatting concerns behind a small API so the
//! broader tooling crate can focus on composition rather than formatter
//! implementation details. The default LSP formatter uses the Rust-native
//! [`NativeFormatter`]; [`PerlTidyFormatter`] remains an explicit
//! subprocess-backed compatibility adapter for projects that still require
//! exact `perltidy` behavior.

#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used, clippy::expect_used))]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

use perl_subprocess_runtime::SubprocessRuntime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub mod native;

pub use native::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatDiagnostic,
    FormatDiagnosticSeverity, FormatDoc, FormatResult, FormatterMode, KeywordSpacing,
    NativeFormatter, PerlFormatter, TextEdit, TextPosition, TextRange, TrailingComma,
};

/// Configuration for perltidy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerlTidyConfig {
    /// Maximum line length.
    pub maximum_line_length: Option<u32>,
    /// Indent size (spaces).
    ///
    /// `None` means "not configured": the caller decides the indent width.
    /// The LSP formatting path falls back to the editor's `tabSize`, and
    /// [`PerlTidyConfig::to_args`] emits no `--indent-columns`, leaving
    /// `perltidy`'s own default in force.
    pub indent_columns: Option<u32>,
    /// Use tabs instead of spaces.
    ///
    /// `None` means "not configured", with the same fallback rules as
    /// [`PerlTidyConfig::indent_columns`]: the LSP formatting path falls back
    /// to the editor's `insertSpaces`, and [`PerlTidyConfig::to_args`] emits
    /// neither `--tabs` nor `--notabs`.
    pub tabs: Option<bool>,
    /// Opening brace on same line.
    pub opening_brace_on_new_line: Option<bool>,
    /// Cuddled else.
    pub cuddled_else: Option<bool>,
    /// Space after keyword.
    pub space_after_keyword: Option<bool>,
    /// Add trailing commas.
    pub add_trailing_commas: Option<bool>,
    /// Vertical alignment.
    pub vertical_alignment: Option<bool>,
    /// Block comment indentation.
    pub block_comment_indentation: Option<u32>,
    /// Custom perltidyrc file path.
    pub profile: Option<String>,
    /// Additional command line arguments.
    pub extra_args: Vec<String>,
    /// Timeout in seconds for the perltidy subprocess. Default: 10.
    pub timeout_secs: u64,
}

impl Default for PerlTidyConfig {
    fn default() -> Self {
        Self {
            maximum_line_length: Some(80),
            // Indentation is deliberately unset by default so that an
            // unconfigured project keeps deferring to the editor's `tabSize` /
            // `insertSpaces`, while an explicitly configured value wins. The
            // presets below opt in explicitly.
            indent_columns: None,
            tabs: None,
            opening_brace_on_new_line: Some(false),
            cuddled_else: Some(true),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(false),
            vertical_alignment: Some(true),
            block_comment_indentation: Some(0),
            profile: None,
            extra_args: Vec::new(),
            timeout_secs: 10,
        }
    }
}

impl PerlTidyConfig {
    /// Create a config for PBP (Perl Best Practices) style.
    #[must_use]
    pub fn pbp() -> Self {
        Self {
            maximum_line_length: Some(78),
            indent_columns: Some(4),
            tabs: Some(false),
            opening_brace_on_new_line: Some(false),
            cuddled_else: Some(false),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(true),
            vertical_alignment: Some(true),
            block_comment_indentation: Some(0),
            profile: None,
            extra_args: vec!["--perl-best-practices".to_string()],
            timeout_secs: 10,
        }
    }

    /// Create a config for GNU style.
    #[must_use]
    pub fn gnu() -> Self {
        Self {
            maximum_line_length: Some(79),
            indent_columns: Some(2),
            tabs: Some(false),
            opening_brace_on_new_line: Some(true),
            cuddled_else: Some(false),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(false),
            vertical_alignment: Some(false),
            block_comment_indentation: Some(2),
            profile: None,
            extra_args: vec!["--gnu-style".to_string()],
            timeout_secs: 10,
        }
    }

    /// Convert the configuration to `perltidy` command-line arguments.
    #[must_use]
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(profile) = &self.profile {
            args.push(format!("--profile={profile}"));
            if let Some(indent) = self.indent_columns {
                args.push(format!("--indent-columns={indent}"));
            }
            if let Some(tabs) = self.tabs {
                args.push(if tabs { "--tabs".to_string() } else { "--notabs".to_string() });
            }
            args.extend(self.extra_args.clone());
            return args;
        }

        if let Some(len) = self.maximum_line_length {
            args.push(format!("--maximum-line-length={len}"));
        }

        if let Some(indent) = self.indent_columns {
            args.push(format!("--indent-columns={indent}"));
        }

        if let Some(tabs) = self.tabs {
            if tabs {
                args.push("--tabs".to_string());
            } else {
                args.push("--notabs".to_string());
            }
        }

        if let Some(brace) = self.opening_brace_on_new_line {
            if brace {
                args.push("--opening-brace-on-new-line".to_string());
            } else {
                args.push("--opening-brace-always-on-right".to_string());
            }
        }

        if let Some(cuddle) = self.cuddled_else {
            if cuddle {
                args.push("--cuddled-else".to_string());
            } else {
                args.push("--nocuddled-else".to_string());
            }
        }

        if let Some(space) = self.space_after_keyword {
            if space {
                args.push("--space-after-keyword".to_string());
            } else {
                args.push("--nospace-after-keyword".to_string());
            }
        }

        if let Some(comma) = self.add_trailing_commas {
            if comma {
                args.push("--add-trailing-commas".to_string());
            } else {
                args.push("--no-add-trailing-commas".to_string());
            }
        }

        if let Some(align) = self.vertical_alignment {
            if align {
                args.push("--vertical-alignment".to_string());
            } else {
                args.push("--no-vertical-alignment".to_string());
            }
        }

        if let Some(indent) = self.block_comment_indentation {
            args.push(format!("--block-comment-indentation={indent}"));
        }

        args.extend(self.extra_args.clone());
        args
    }
}

/// Perltidy formatter.
pub struct PerlTidyFormatter {
    config: PerlTidyConfig,
    cache: HashMap<String, String>,
    runtime: Arc<dyn SubprocessRuntime>,
}

impl PerlTidyFormatter {
    /// Creates a new formatter with the given configuration and runtime.
    #[must_use]
    pub fn new(config: PerlTidyConfig, runtime: Arc<dyn SubprocessRuntime>) -> Self {
        Self { config, cache: HashMap::new(), runtime }
    }

    /// Creates a new formatter with the OS subprocess runtime (non-WASM only).
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn with_os_runtime(config: PerlTidyConfig) -> Self {
        use perl_subprocess_runtime::OsSubprocessRuntime;
        // OsSubprocessRuntime::with_timeout normalizes zero to 1s; clamp here
        // as well so the formatter's floor is explicit at its own seam.
        let timeout = config.timeout_secs.max(1);
        Self::new(config, Arc::new(OsSubprocessRuntime::with_timeout(timeout)))
    }

    /// Format Perl code.
    pub fn format(&mut self, code: &str) -> Result<String, String> {
        if let Some(cached) = self.cache.get(code) {
            return Ok(cached.clone());
        }

        let mut args = self.config.to_args();
        args.push("-st".to_string());
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let output = self
            .runtime
            .run_command("perltidy", &args_refs, Some(code.as_bytes()))
            .map_err(|e| e.message)?;

        if !output.success() {
            return Err(format!("Perltidy failed: {}", output.stderr_lossy()));
        }

        let formatted = String::from_utf8(output.stdout)
            .map_err(|e| format!("Invalid UTF-8 from perltidy: {e}"))?;
        self.cache.insert(code.to_string(), formatted.clone());
        Ok(formatted)
    }

    /// Format a file in place.
    pub fn format_file(&self, file_path: &Path) -> Result<(), String> {
        let mut args = self.config.to_args();
        args.push("--".to_string());
        args.push(file_path.to_string_lossy().into_owned());
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let output =
            self.runtime.run_command("perltidy", &args_refs, None).map_err(|e| e.message)?;

        if !output.success() {
            return Err(format!("Perltidy failed: {}", output.stderr_lossy()));
        }

        Ok(())
    }

    /// Clear any memoized formatting results.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Return the number of memoized formatting results.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Format a range of code.
    pub fn format_range(
        &mut self,
        code: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<String, String> {
        if start_line > end_line {
            return Err(
                "Invalid line range: start line must be less than or equal to end line".to_string()
            );
        }

        let lines: Vec<&str> = code.lines().collect();

        if start_line as usize >= lines.len() || end_line as usize >= lines.len() {
            return Err("Line range out of bounds".to_string());
        }

        let range_code = lines[start_line as usize..=end_line as usize].join("\n");
        let formatted_range = self.format(&range_code)?;

        let mut result = Vec::new();
        if start_line > 0 {
            result.extend_from_slice(&lines[0..start_line as usize]);
        }
        result.extend(formatted_range.lines());
        if (end_line as usize) < lines.len() - 1 {
            result.extend_from_slice(&lines[(end_line as usize + 1)..]);
        }

        Ok(result.join("\n"))
    }

    /// Get formatting suggestions without applying them.
    pub fn get_suggestions(&mut self, code: &str) -> Result<Vec<FormatSuggestion>, String> {
        let formatted = self.format(code)?;
        if formatted == code {
            return Ok(Vec::new());
        }

        let orig_lines: Vec<&str> = code.lines().collect();
        let fmt_lines: Vec<&str> = formatted.lines().collect();
        let mut suggestions = Vec::new();
        let max_lines = orig_lines.len().max(fmt_lines.len());

        for i in 0..max_lines {
            match (orig_lines.get(i), fmt_lines.get(i)) {
                (Some(orig), Some(fmt)) if orig != fmt => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: (*orig).to_string(),
                    formatted: (*fmt).to_string(),
                    description: "Line formatting change".to_string(),
                }),
                (Some(orig), None) => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: (*orig).to_string(),
                    formatted: String::new(),
                    description: "Line removed by formatting".to_string(),
                }),
                (None, Some(fmt)) => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: String::new(),
                    formatted: (*fmt).to_string(),
                    description: "Line added by formatting".to_string(),
                }),
                _ => {}
            }
        }

        Ok(suggestions)
    }
}

/// A formatting suggestion.
#[derive(Debug, Clone)]
pub struct FormatSuggestion {
    /// Zero-based line number where the change applies.
    pub line: u32,
    /// Original line content before formatting.
    pub original: String,
    /// Suggested formatted line content.
    pub formatted: String,
    /// Human-readable description of the formatting change.
    pub description: String,
}

/// Built-in formatter for when `perltidy` is unavailable.
pub struct BuiltInFormatter {
    config: PerlTidyConfig,
}

impl BuiltInFormatter {
    /// Creates a new built-in formatter with the given configuration.
    #[must_use]
    pub fn new(config: PerlTidyConfig) -> Self {
        Self { config }
    }

    /// Apply basic indentation-based formatting without invoking `perltidy`.
    #[must_use]
    pub fn format(&self, code: &str) -> String {
        let mut result = String::new();
        let mut delimiter_stack = Vec::new();
        let mut delimiter_scan_state = DelimiterScanState::default();
        let lines: Vec<&str> = code.lines().collect();
        let had_trailing_newline = code.ends_with('\n');
        let indent_str = if self.config.tabs.unwrap_or(false) {
            "\t".to_string()
        } else {
            " ".repeat(self.config.indent_columns.unwrap_or(4) as usize)
        };

        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let leading_closers = count_matching_leading_closers(trimmed, &delimiter_stack);
            let indent_level = delimiter_stack.len().saturating_sub(leading_closers);

            if !trimmed.is_empty() {
                for _ in 0..indent_level {
                    result.push_str(&indent_str);
                }
                result.push_str(trimmed);
            }

            let is_last_line = index + 1 == lines.len();
            if !is_last_line || had_trailing_newline {
                result.push('\n');
            }

            apply_delimiter_events_with_state(
                trimmed,
                &mut delimiter_stack,
                &mut delimiter_scan_state,
            );
        }

        result
    }
}

fn count_matching_leading_closers(line: &str, delimiter_stack: &[char]) -> usize {
    let mut stack_index = delimiter_stack.len();
    let mut matched = 0;

    for closer in line.chars().take_while(|ch| matches!(ch, '}' | ')' | ']')) {
        let Some(&opening) =
            stack_index.checked_sub(1).and_then(|index| delimiter_stack.get(index))
        else {
            break;
        };
        if matching_closer(opening) != Some(closer) {
            break;
        }
        matched += 1;
        stack_index -= 1;
    }

    matched
}

fn apply_delimiter_events_with_state(
    line: &str,
    delimiter_stack: &mut Vec<char>,
    state: &mut DelimiterScanState,
) {
    for delimiter in significant_delimiters_with_state(line, state) {
        match delimiter {
            opening @ ('{' | '(' | '[') => delimiter_stack.push(opening),
            closer @ ('}' | ')' | ']') => {
                if delimiter_stack.last().copied().and_then(matching_closer) == Some(closer) {
                    delimiter_stack.pop();
                }
            }
            _ => {}
        }
    }
}

fn matching_closer(opening: char) -> Option<char> {
    match opening {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    }
}

#[derive(Default)]
struct DelimiterScanState {
    in_single: bool,
    in_double: bool,
    regex_closer: Option<char>,
    regex_opener: Option<char>,
    regex_nesting: usize,
    regex_char_class: bool,
    regex_is_substitution: bool,
    pending_replacement: Option<(Option<char>, char)>,
    escaped: bool,
    last_non_whitespace: Option<char>,
    second_last_non_whitespace: Option<char>,
}

impl DelimiterScanState {
    fn record_non_whitespace(&mut self, ch: char) {
        self.second_last_non_whitespace = self.last_non_whitespace;
        self.last_non_whitespace = Some(ch);
    }

    fn clear_regex(&mut self) {
        self.regex_closer = None;
        self.regex_opener = None;
        self.regex_nesting = 0;
        self.regex_char_class = false;
        self.regex_is_substitution = false;
    }
}

fn significant_delimiters_with_state(line: &str, state: &mut DelimiterScanState) -> Vec<char> {
    let mut delimiters = Vec::new();
    let chars: Vec<char> = line.chars().collect();

    for (index, ch) in chars.iter().copied().enumerate() {
        if state.escaped {
            state.escaped = false;
            continue;
        }

        if ch == '\\' {
            state.escaped = true;
            continue;
        }

        if state.regex_closer.is_none()
            && let Some((replacement_opener, replacement_closer)) = state.pending_replacement
        {
            if replacement_opener.is_some() {
                if let Some((opener, closer, nesting)) = replacement_delimiter(ch) {
                    state.pending_replacement = None;
                    state.regex_opener = opener;
                    state.regex_closer = Some(closer);
                    state.regex_nesting = nesting;
                    state.regex_char_class = false;
                    state.regex_is_substitution = false;
                    state.record_non_whitespace(ch);
                    continue;
                }
            } else {
                state.pending_replacement = None;
                state.regex_opener = None;
                state.regex_closer = Some(replacement_closer);
                state.regex_nesting = 0;
                state.regex_char_class = false;
                state.regex_is_substitution = false;
            }
        }

        if state.regex_closer.is_some() {
            if ch == '[' {
                state.regex_char_class = true;
            } else if ch == ']' && state.regex_char_class {
                state.regex_char_class = false;
            } else if !state.regex_char_class {
                if state.regex_opener.is_some() && Some(ch) == state.regex_opener {
                    state.regex_nesting += 1;
                } else if Some(ch) == state.regex_closer {
                    let replacement = state.regex_is_substitution;
                    let replacement_opener = state.regex_opener;
                    let replacement_closer = state.regex_closer;
                    if state.regex_opener.is_some() && state.regex_nesting > 1 {
                        state.regex_nesting -= 1;
                    } else {
                        state.clear_regex();
                        if replacement && let Some(closer) = replacement_closer {
                            state.pending_replacement = Some((replacement_opener, closer));
                        }
                    }
                    state.record_non_whitespace(ch);
                }
            }
            continue;
        }

        if state.in_single {
            if ch == '\'' {
                state.in_single = false;
                state.record_non_whitespace(ch);
            }
            continue;
        }

        if state.in_double {
            if ch == '"' {
                state.in_double = false;
                state.record_non_whitespace(ch);
            }
            continue;
        }

        if let Some((regex_opener, regex_closer, regex_nesting, regex_is_substitution)) =
            regex_start(&chars, index, ch, state)
        {
            // Ignore delimiters inside Perl regex and quote-like forms. The
            // state is carried across physical lines so a multiline pattern
            // cannot leak its contents into the formatter's stack.
            state.regex_opener = regex_opener;
            state.regex_closer = Some(regex_closer);
            state.regex_nesting = regex_nesting;
            state.regex_char_class = false;
            state.regex_is_substitution = regex_is_substitution;
            state.record_non_whitespace(ch);
            continue;
        }

        if ch == '\'' {
            state.in_single = true;
            state.record_non_whitespace(ch);
            continue;
        }

        if ch == '"' {
            state.in_double = true;
            state.record_non_whitespace(ch);
            continue;
        }

        if ch == '#' {
            break;
        }

        if matches!(ch, '{' | '(' | '[' | '}' | ')' | ']') {
            delimiters.push(ch);
        }

        if !ch.is_whitespace() {
            state.record_non_whitespace(ch);
        }
    }

    delimiters
}

fn regex_start(
    chars: &[char],
    index: usize,
    ch: char,
    state: &DelimiterScanState,
) -> Option<(Option<char>, char, usize, bool)> {
    let quote_like = preceding_word(chars, index);
    let is_quote_like = matches!(
        quote_like.as_deref(),
        Some("m" | "q" | "qr" | "qq" | "qw" | "qx" | "s" | "tr" | "y")
    );
    let is_substitution = matches!(quote_like.as_deref(), Some("s" | "tr" | "y"));

    if is_quote_like && let Some((opener, closer, nesting)) = replacement_delimiter(ch) {
        return Some((opener, closer, nesting, is_substitution));
    }

    if ch != '/' {
        return None;
    }

    let starts_expression = state.last_non_whitespace.is_none()
        || matches!(
            state.last_non_whitespace,
            Some('(' | '[' | '{' | '=' | '!' | '?' | ':' | ',' | ';' | '~')
        )
        || (state.last_non_whitespace == Some('~')
            && matches!(state.second_last_non_whitespace, Some('=') | Some('!')))
        || matches!(
            quote_like.as_deref(),
            Some("m" | "q" | "qr" | "qq" | "qw" | "qx" | "s" | "tr" | "y")
        );

    starts_expression.then_some((None, '/', 0, false))
}

fn replacement_delimiter(ch: char) -> Option<(Option<char>, char, usize)> {
    match ch {
        '{' => Some((Some('{'), '}', 1)),
        '(' => Some((Some('('), ')', 1)),
        '[' => Some((Some('['), ']', 1)),
        '/' => Some((None, '/', 0)),
        _ => None,
    }
}

fn preceding_word(chars: &[char], index: usize) -> Option<String> {
    let mut end = index;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && chars[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::{
        DelimiterScanState, apply_delimiter_events_with_state, count_matching_leading_closers,
        significant_delimiters_with_state,
    };

    #[test]
    fn count_matching_leading_closers_requires_typed_matches() {
        let stack = vec!['{', '('];

        assert_eq!(count_matching_leading_closers(")", &stack), 1);
        assert_eq!(count_matching_leading_closers("})", &stack), 0);
    }

    #[test]
    fn apply_delimiter_events_preserves_unmatched_closers() {
        let mut stack = vec!['{', '('];
        let mut state = DelimiterScanState::default();

        apply_delimiter_events_with_state("})", &mut stack, &mut state);

        assert_eq!(stack, vec!['{']);
    }

    #[test]
    fn significant_delimiters_ignores_regex_character_classes() {
        let mut state = DelimiterScanState::default();

        assert_eq!(
            significant_delimiters_with_state("if ($x =~ /[[]/) {", &mut state),
            vec!['(', ')', '{']
        );
    }
}
