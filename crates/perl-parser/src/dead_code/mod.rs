//! Dead code detection for Perl codebases (stub implementation)
//!
//! This module identifies unused code including unreachable code and unused symbols.
//! Currently a stub implementation to demonstrate the architecture.

use perl_workspace::workspace_index::{SymbolKind, WorkspaceIndex, fs_path_to_uri, uri_to_fs_path};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Types of dead code detected during Perl script analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeadCodeType {
    /// Subroutine defined but never called
    UnusedSubroutine,
    /// Variable declared but never used
    UnusedVariable,
    /// Constant defined but never referenced
    UnusedConstant,
    /// Package declared but never used
    UnusedPackage,
    /// Code that can never be executed
    UnreachableCode,
    /// Conditional branch that is never taken
    DeadBranch,
    /// Module imported but never used
    UnusedImport,
    /// Function exported but never used externally
    UnusedExport,
}

/// A piece of dead code detected during analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCode {
    /// Type of dead code (subroutine, variable, etc.)
    pub code_type: DeadCodeType,
    /// Name of the dead code element if available
    pub name: Option<String>,
    /// File path where the dead code is located
    pub file_path: PathBuf,
    /// Starting line number (1-based)
    pub start_line: usize,
    /// Ending line number (1-based)
    pub end_line: usize,
    /// Human-readable explanation of why this is considered dead code
    pub reason: String,
    /// Confidence level (0.0-1.0) in the detection accuracy
    pub confidence: f32,
    /// Optional suggestion for fixing the dead code
    pub suggestion: Option<String>,
}

/// Dead code analysis result for a Perl workspace
#[derive(Debug, Serialize, Deserialize)]
pub struct DeadCodeAnalysis {
    /// List of all dead code instances found
    pub dead_code: Vec<DeadCode>,
    /// Statistical summary of dead code analysis
    pub stats: DeadCodeStats,
    /// Number of files analyzed in the workspace
    pub files_analyzed: usize,
    /// Total lines of code analyzed
    pub total_lines: usize,
}

/// Statistical summary of dead code analysis results
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeadCodeStats {
    /// Number of unused subroutines detected
    pub unused_subroutines: usize,
    /// Number of unused variables detected
    pub unused_variables: usize,
    /// Number of unused constants detected
    pub unused_constants: usize,
    /// Number of unused packages detected
    pub unused_packages: usize,
    /// Number of unreachable code statements
    pub unreachable_statements: usize,
    /// Number of dead conditional branches
    pub dead_branches: usize,
    /// Total lines of dead code identified
    pub total_dead_lines: usize,
}

/// Dead code detector
pub struct DeadCodeDetector {
    workspace_index: WorkspaceIndex,
    entry_points: HashSet<PathBuf>,
}

impl DeadCodeDetector {
    /// Create a new dead code detector with the given workspace index
    ///
    /// # Arguments
    /// * `workspace_index` - Indexed workspace containing symbol definitions and references
    pub fn new(workspace_index: WorkspaceIndex) -> Self {
        Self { workspace_index, entry_points: HashSet::new() }
    }

    /// Add an entry point (main script)
    pub fn add_entry_point(&mut self, path: PathBuf) {
        self.entry_points.insert(path);
    }

    /// Analyze a single file for dead code
    pub fn analyze_file(&self, file_path: &Path) -> Result<Vec<DeadCode>, String> {
        let uri = fs_path_to_uri(file_path).map_err(|e| e.to_string())?;
        let text = self
            .workspace_index
            .document_store()
            .get_text(&uri)
            .ok_or_else(|| "file not indexed".to_string())?;

        let mut dead = Vec::new();
        let mut block_depth = 0usize;
        let mut terminator: Option<(usize, usize, String)> = None;

        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let current_depth = block_depth;

            if let Some((term_line, term_depth, term_kw)) = &terminator {
                if current_depth < *term_depth {
                    terminator = None;
                } else if current_depth == *term_depth
                    && !trimmed.is_empty()
                    && !trimmed.starts_with('#')
                    && !is_structural_line(trimmed)
                {
                    dead.push(DeadCode {
                        code_type: DeadCodeType::UnreachableCode,
                        name: None,
                        file_path: file_path.to_path_buf(),
                        start_line: i + 1,
                        end_line: i + 1,
                        reason: format!(
                            "Code is unreachable after `{}` on line {}",
                            term_kw, term_line
                        ),
                        confidence: 0.9,
                        suggestion: Some("Remove or restructure this code".to_string()),
                    });
                    break;
                }
            }

            if let Some(term_kw) = detect_unconditional_terminator(trimmed) {
                terminator = Some((i + 1, current_depth, term_kw.to_string()));
            }

            block_depth += line.chars().filter(|&ch| ch == '{').count();
            block_depth = block_depth.saturating_sub(line.chars().filter(|&ch| ch == '}').count());
        }

        // Dead branch detection: scan for constant-condition patterns.
        detect_dead_branches(file_path, &text, &mut dead);

        Ok(dead)
    }

    /// Analyze entire workspace for dead code
    pub fn analyze_workspace(&self) -> DeadCodeAnalysis {
        let docs = self.workspace_index.document_store().all_documents();
        let mut dead_code = Vec::new();
        let mut total_lines = 0;

        // Per-file unreachable code
        for doc in &docs {
            total_lines += doc.text().lines().count();
            if let Some(path) = uri_to_fs_path(&doc.uri) {
                if let Ok(mut file_dead) = self.analyze_file(&path) {
                    dead_code.append(&mut file_dead);
                }
            }
        }

        // Unused symbols across workspace
        for sym in self.workspace_index.find_unused_symbols() {
            let code_type = match sym.kind {
                SymbolKind::Subroutine => DeadCodeType::UnusedSubroutine,
                SymbolKind::Variable(_) => DeadCodeType::UnusedVariable,
                SymbolKind::Constant => DeadCodeType::UnusedConstant,
                SymbolKind::Package => DeadCodeType::UnusedPackage,
                _ => continue,
            };

            let file_path = uri_to_fs_path(&sym.uri).unwrap_or_else(|| PathBuf::from(&sym.uri));

            dead_code.push(DeadCode {
                code_type,
                name: Some(sym.name.clone()),
                file_path,
                start_line: sym.range.start.line as usize + 1,
                end_line: sym.range.end.line as usize + 1,
                reason: "Symbol is never used".to_string(),
                confidence: 0.9,
                suggestion: Some("Remove or use this symbol".to_string()),
            });
        }

        // Compute stats
        let mut stats = DeadCodeStats::default();
        for item in &dead_code {
            let lines = item.end_line.saturating_sub(item.start_line) + 1;
            stats.total_dead_lines += lines;
            match item.code_type {
                DeadCodeType::UnusedSubroutine => stats.unused_subroutines += 1,
                DeadCodeType::UnusedVariable => stats.unused_variables += 1,
                DeadCodeType::UnusedConstant => stats.unused_constants += 1,
                DeadCodeType::UnusedPackage => stats.unused_packages += 1,
                DeadCodeType::UnreachableCode => stats.unreachable_statements += 1,
                DeadCodeType::DeadBranch => stats.dead_branches += 1,
                _ => {}
            }
        }

        DeadCodeAnalysis { dead_code, stats, files_analyzed: docs.len(), total_lines }
    }
}

fn is_structural_line(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '}' || ch == ';')
}

fn detect_unconditional_terminator(trimmed: &str) -> Option<&str> {
    const TERMINATORS: [&str; 4] = ["return", "die", "exit", "CORE::exit"];

    let first = trimmed
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '('))
        .next()
        .unwrap_or_default();
    if !TERMINATORS.contains(&first) {
        return None;
    }

    let after_terminator = &trimmed[first.len()..];
    let remainder = match after_terminator.split_once('#') {
        Some((before_comment, _)) => before_comment,
        None => after_terminator,
    }
    .trim_start();
    if contains_postfix_modifier(remainder) {
        return None;
    }

    Some(first)
}

fn contains_postfix_modifier(remainder: &str) -> bool {
    const POSTFIX_MODIFIERS: [&str; 7] =
        ["if", "unless", "when", "while", "until", "for", "foreach"];
    POSTFIX_MODIFIERS.iter().any(|keyword| contains_keyword(remainder, keyword))
}

fn contains_keyword(text: &str, keyword: &str) -> bool {
    text.match_indices(keyword).any(|(idx, _)| {
        let before = text[..idx].chars().next_back();
        let after = text[idx + keyword.len()..].chars().next();
        is_keyword_boundary(before) && is_keyword_boundary(after)
    })
}

fn is_keyword_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

/// Returns `true` if `condition` is a trivially-false constant expression.
///
/// Matches: `0`, `""`, `''`, `undef`, `(0)`, `( 0 )` — the standard Perl idioms
/// used to write permanently-dead `if`/`while`/`elsif` blocks.
///
/// Note: `for`/`foreach` are intentionally **not** guarded by this function.
/// `for (0) {}` iterates once with `$_ = 0`; it is a list iterator, not a
/// boolean guard, so it is never dead code.
fn is_always_false(condition: &str) -> bool {
    // Strip outer balanced parentheses iteratively to avoid unbounded recursion
    // on adversarially-deep inputs like `((((...0...))))` (#795).
    let c = strip_outer_parens(condition);
    matches!(c, "0" | "\"\"" | "''" | "undef")
}

/// Returns `true` if `condition` is a trivially-true constant expression.
///
/// Matches: `1`, `"1"`, `'1'`, any non-zero integer literal, `(1)` etc.
fn is_always_true(condition: &str) -> bool {
    // Strip outer balanced parentheses iteratively to avoid unbounded recursion
    // on adversarially-deep inputs (#795).
    let c = strip_outer_parens(condition);
    // Non-zero integer literal
    if c.parse::<i64>().is_ok_and(|n| n != 0) {
        return true;
    }
    // Non-zero float literal
    if c.parse::<f64>().is_ok_and(|n| n != 0.0) {
        return true;
    }
    // Quoted non-empty string that is not "0"
    if (c.starts_with('"') && c.ends_with('"') || c.starts_with('\'') && c.ends_with('\''))
        && c.len() > 2
    {
        let inner = &c[1..c.len() - 1];
        return inner != "0";
    }
    false
}

/// Strip all layers of balanced outer parentheses from `condition`, returning
/// a reference to the innermost non-paren-wrapped content.
///
/// For example `"(((0)))"` → `"0"`, `"( x )"` → `"x"`, `"0"` → `"0"`.
///
/// This replaces the previous tail-recursive pattern and avoids stack overflow
/// on deeply-nested inputs (#795).
fn strip_outer_parens(condition: &str) -> &str {
    let mut s = condition.trim();
    while s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        // Only strip if the opening '(' matches the closing ')'.
        // E.g. `"(a)(b)"` must NOT be stripped — the first '(' closes before
        // the last ')'.
        if is_outer_paren_balanced(inner) {
            s = inner.trim();
        } else {
            break;
        }
    }
    s
}

/// Returns `true` when wrapping `s` with `(` and `)` would form a balanced
/// pair — i.e., when the first `(` in the parent expression closes at the
/// very last character.  Equivalently, `inner` has a non-negative paren depth
/// at every prefix.
fn is_outer_paren_balanced(inner: &str) -> bool {
    let mut depth = 0i32;
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// Scan `text` for constant-condition dead branches and append `DeadBranch`
/// entries to `out`.
///
/// Detects:
/// - `if (0) { ... }`  — body is never executed
/// - `while (0) { ... }` — loop body is never executed
/// - `unless (1) { ... }` — equivalent to `if (0)`
/// - `until (1) { ... }` — equivalent to `while (0)`
/// - `else` block following an always-true `if (1)` — dead else branch
///
/// Uses a simple brace-counting heuristic to locate the block extent.
/// Only fires for single-line condition + opening brace patterns (the most
/// common idiom); multi-line conditions are skipped to avoid false positives.
fn detect_dead_branches(file_path: &Path, text: &str, out: &mut Vec<DeadCode>) {
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    let mut i = 0;

    while i < n {
        let trimmed = lines[i].trim();

        // Determine if this line opens a dead branch.
        // We look for: KEYWORD WHITESPACE? ( CONDITION ) WHITESPACE? {
        //
        // `for` and `foreach` are intentionally excluded: they are list
        // iterators in Perl, not boolean guards. `for (0) {}` executes once
        // with $_ = 0; it is not dead code.
        let dead_reason_and_keyword: Option<(String, &str)> = 'detect: {
            for kw in &["if", "while", "elsif", "unless", "until"] {
                let rest = match trimmed.strip_prefix(kw) {
                    Some(r)
                        if r.is_empty()
                            || r.starts_with(|c: char| c.is_whitespace() || c == '(') =>
                    {
                        r.trim_start()
                    }
                    _ => continue,
                };
                // Extract balanced parentheses for the condition.
                if !rest.starts_with('(') {
                    continue;
                }
                let condition = extract_balanced_parens(rest);
                let condition = match condition {
                    Some(c) => c,
                    None => continue,
                };
                // `rest` starts with `(`, condition is `rest[1..idx]`, closing `)` is at
                // index `idx = condition.len() + 1`.  `after_cond` starts at `idx + 1`.
                // We use `.get()` for an explicit bounds-safe slice (#791).
                let after_idx = condition.len() + 2;
                let after_cond = match rest.get(after_idx..) {
                    Some(s) => s.trim(),
                    None => continue,
                };
                // Only fire if opening brace is on the same line.
                if !after_cond.starts_with('{') && !after_cond.is_empty() {
                    continue;
                }
                let inner = condition.trim();

                let reason = if matches!(*kw, "unless" | "until") {
                    // unless/until: body is dead when condition is always-true
                    if is_always_true(inner) {
                        Some(format!(
                            "`{kw}` condition `{inner}` is always true — block is never executed"
                        ))
                    } else {
                        None
                    }
                } else {
                    // if/while/elsif: body is dead when condition is always-false
                    if is_always_false(inner) {
                        Some(format!(
                            "`{kw}` condition `{inner}` is always false — block is never executed"
                        ))
                    } else {
                        None
                    }
                };

                if let Some(r) = reason {
                    break 'detect Some((r, *kw));
                }
            }

            // Also check `else` block following always-true `if`.
            // We handle this by looking back at the previously emitted entry
            // or by a simple heuristic: `} else {` on its own line after an
            // always-true if that we tracked.  This is deferred to a follow-up;
            // for now focus on always-false/always-true keyword conditions.
            None
        };

        if let Some((reason, _kw)) = dead_reason_and_keyword {
            // Find the closing brace of this block by counting brace depth.
            let block_start = i + 1; // 1-based
            let end_line = find_block_end(&lines, i);
            out.push(DeadCode {
                code_type: DeadCodeType::DeadBranch,
                name: None,
                file_path: file_path.to_path_buf(),
                start_line: block_start,
                end_line,
                reason,
                confidence: 0.9,
                suggestion: Some("Remove this dead branch or fix the condition".to_string()),
            });
            // Skip to after the block to avoid nested false positives.
            i = end_line;
            continue;
        }

        i += 1;
    }
}

/// Extract the content of the first balanced `(...)` starting at the
/// beginning of `s`.  Returns the inner content (without the outer parens),
/// or `None` if the parens are unbalanced or `s` doesn't start with `(`.
fn extract_balanced_parens(s: &str) -> Option<&str> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[1..idx]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the 1-based line number of the closing `}` for the block that opens
/// on line `open_line` (0-based index).  Uses simple brace counting.
/// Returns `open_line + 1` (1-based same line) if the block closes on the
/// same line, or the last line of the file if braces are unbalanced.
fn find_block_end(lines: &[&str], open_line: usize) -> usize {
    let mut depth = 0i32;
    for (i, line) in lines.iter().enumerate().skip(open_line) {
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1; // 1-based
                    }
                }
                _ => {}
            }
        }
    }
    lines.len() // fallback: end of file
}

/// Generate a report from dead code analysis
pub fn generate_report(analysis: &DeadCodeAnalysis) -> String {
    let mut report = String::new();

    report.push_str("=== Dead Code Analysis Report ===\n\n");

    report.push_str(&format!("Files analyzed: {}\n", analysis.files_analyzed));
    report.push_str(&format!("Total lines: {}\n", analysis.total_lines));
    report.push_str(&format!("Dead code items: {}\n\n", analysis.dead_code.len()));

    report.push_str("Statistics:\n");
    report.push_str(&format!("  Unused subroutines: {}\n", analysis.stats.unused_subroutines));
    report.push_str(&format!("  Unused variables: {}\n", analysis.stats.unused_variables));
    report.push_str(&format!("  Unused constants: {}\n", analysis.stats.unused_constants));
    report.push_str(&format!("  Unused packages: {}\n", analysis.stats.unused_packages));
    report.push_str(&format!(
        "  Unreachable statements: {}\n",
        analysis.stats.unreachable_statements
    ));
    report.push_str(&format!("  Dead branches: {}\n", analysis.stats.dead_branches));
    report.push_str(&format!("  Total dead lines: {}\n", analysis.stats.total_dead_lines));

    report
}

#[cfg(test)]
mod tests {
    use super::{
        DeadCode, DeadCodeAnalysis, DeadCodeStats, DeadCodeType, contains_keyword,
        contains_postfix_modifier, detect_dead_branches, detect_unconditional_terminator,
        extract_balanced_parens, find_block_end, generate_report, is_always_false, is_always_true,
        is_keyword_boundary, is_outer_paren_balanced, is_structural_line, strip_outer_parens,
    };
    use std::path::{Path, PathBuf};

    fn branches(source: &str) -> Vec<DeadCode> {
        let mut out = Vec::new();
        detect_dead_branches(Path::new("/tmp/probe.pl"), source, &mut out);
        out
    }

    fn reasons(source: &str) -> Vec<String> {
        branches(source).into_iter().map(|item| item.reason).collect()
    }

    // ---- is_structural_line -------------------------------------------------

    #[test]
    fn structural_lines_are_only_closers_and_separators() {
        assert!(is_structural_line("}"));
        assert!(is_structural_line("};"));
        assert!(is_structural_line(";"));
        assert!(is_structural_line("}}};"));
    }

    #[test]
    fn empty_and_substantive_lines_are_not_structural() {
        // The emptiness guard is load-bearing: `chars().all(..)` is vacuously
        // true for "", which would otherwise classify a blank line as closing
        // punctuation.
        assert!(!is_structural_line(""));
        assert!(!is_structural_line("print 1;"));
        assert!(!is_structural_line("{"));
        assert!(!is_structural_line("} else {"));
    }

    // ---- is_keyword_boundary / contains_keyword ------------------------------

    #[test]
    fn keyword_boundary_accepts_absent_and_non_word_neighbours() {
        assert!(is_keyword_boundary(None));
        assert!(is_keyword_boundary(Some(' ')));
        assert!(is_keyword_boundary(Some(';')));
        assert!(is_keyword_boundary(Some('$')));
        assert!(is_keyword_boundary(Some('"')));
    }

    #[test]
    fn keyword_boundary_rejects_word_neighbours() {
        assert!(!is_keyword_boundary(Some('a')));
        assert!(!is_keyword_boundary(Some('Z')));
        assert!(!is_keyword_boundary(Some('7')));
        assert!(!is_keyword_boundary(Some('_')));
    }

    #[test]
    fn contains_keyword_requires_both_boundaries() {
        assert!(contains_keyword("x if y", "if"));
        assert!(contains_keyword("if", "if"));
        assert!(contains_keyword("($x) if ($y)", "if"));
        // Embedded occurrences must not match: `notify` and `iffy` both contain
        // the literal substring `if`.
        assert!(!contains_keyword("$notify", "if"));
        assert!(!contains_keyword("iffy", "if"));
        assert!(!contains_keyword("half_if_done", "if"));
        assert!(!contains_keyword("", "if"));
    }

    #[test]
    fn contains_keyword_scans_past_a_non_boundary_first_hit() {
        // The first `if` is embedded in `notify`; a scan that stopped at the
        // first substring hit would miss the real postfix modifier that follows.
        assert!(contains_keyword("$notify if $ready", "if"));
    }

    // ---- contains_postfix_modifier -------------------------------------------

    #[test]
    fn every_postfix_modifier_keyword_is_recognised() {
        for keyword in ["if", "unless", "when", "while", "until", "for", "foreach"] {
            assert!(
                contains_postfix_modifier(&format!("$x {keyword} $y")),
                "`{keyword}` should be recognised as a postfix modifier"
            );
        }
    }

    #[test]
    fn non_modifier_remainders_are_rejected() {
        assert!(!contains_postfix_modifier(""));
        assert!(!contains_postfix_modifier("$value;"));
        assert!(!contains_postfix_modifier("$uniform;"));
    }

    // ---- detect_unconditional_terminator -------------------------------------

    #[test]
    fn each_terminator_keyword_is_detected() {
        assert_eq!(detect_unconditional_terminator("return;"), Some("return"));
        assert_eq!(detect_unconditional_terminator("die 'boom';"), Some("die"));
        assert_eq!(detect_unconditional_terminator("exit(1);"), Some("exit"));
        assert_eq!(detect_unconditional_terminator("CORE::exit;"), Some("CORE::exit"));
        assert_eq!(detect_unconditional_terminator("return"), Some("return"));
    }

    #[test]
    fn non_terminator_statements_are_rejected() {
        assert_eq!(detect_unconditional_terminator("print 1;"), None);
        assert_eq!(detect_unconditional_terminator(""), None);
        assert_eq!(detect_unconditional_terminator("}"), None);
        // Prefix-only matches must not count as terminators.
        assert_eq!(detect_unconditional_terminator("returning();"), None);
        assert_eq!(detect_unconditional_terminator("diet();"), None);
    }

    #[test]
    fn postfix_modifiers_disqualify_a_terminator() {
        assert_eq!(detect_unconditional_terminator("return if $done;"), None);
        assert_eq!(detect_unconditional_terminator("return 1 unless $ok;"), None);
        assert_eq!(detect_unconditional_terminator("die 'x' while $retry;"), None);
        assert_eq!(detect_unconditional_terminator("exit(2) until $done;"), None);
    }

    #[test]
    fn a_trailing_comment_is_not_mistaken_for_a_postfix_modifier() {
        // Everything from `#` onward is commentary, so an `if` there must not
        // suppress detection.
        assert_eq!(detect_unconditional_terminator("return; # if we get here"), Some("return"));
        assert_eq!(detect_unconditional_terminator("die 'x'; # unless retried"), Some("die"));
    }

    #[test]
    fn a_modifier_word_inside_an_identifier_does_not_disqualify() {
        assert_eq!(detect_unconditional_terminator("return $notify;"), Some("return"));
        assert_eq!(detect_unconditional_terminator("return $uniform;"), Some("return"));
    }

    // ---- is_outer_paren_balanced ---------------------------------------------

    #[test]
    fn inner_text_that_could_be_wrapped_is_balanced() {
        assert!(is_outer_paren_balanced(""));
        assert!(is_outer_paren_balanced("0"));
        assert!(is_outer_paren_balanced("(a)"));
        assert!(is_outer_paren_balanced("(a)(b)"));
        // A trailing unclosed `(` never drives depth negative, so wrapping is
        // still considered viable by this predicate.
        assert!(is_outer_paren_balanced("a("));
    }

    #[test]
    fn inner_text_that_closes_early_is_not_balanced() {
        assert!(!is_outer_paren_balanced(")"));
        assert!(!is_outer_paren_balanced("a)(b"));
        assert!(!is_outer_paren_balanced("()) ("));
    }

    // ---- strip_outer_parens ---------------------------------------------------

    #[test]
    fn strip_outer_parens_removes_every_balanced_layer() {
        assert_eq!(strip_outer_parens("0"), "0");
        assert_eq!(strip_outer_parens("(0)"), "0");
        assert_eq!(strip_outer_parens("(((0)))"), "0");
        assert_eq!(strip_outer_parens("  (  x  )  "), "x");
        assert_eq!(strip_outer_parens("()"), "");
    }

    #[test]
    fn strip_outer_parens_keeps_sibling_groups_intact() {
        // `(a)(b)`'s first `(` closes before the final `)`, so the outer pair is
        // not a real wrapper and must survive.
        assert_eq!(strip_outer_parens("(a)(b)"), "(a)(b)");
        assert_eq!(strip_outer_parens("(a) && (b)"), "(a) && (b)");
        assert_eq!(strip_outer_parens("x"), "x");
    }

    #[test]
    fn strip_outer_parens_terminates_on_deep_nesting() {
        // Regression guard for #795: this used to recurse and overflow.
        let deep = format!("{}0{}", "(".repeat(5_000), ")".repeat(5_000));
        assert_eq!(strip_outer_parens(&deep), "0");
    }

    // ---- is_always_false -------------------------------------------------------

    #[test]
    fn perl_false_constants_are_always_false() {
        assert!(is_always_false("0"));
        assert!(is_always_false("\"\""));
        assert!(is_always_false("''"));
        assert!(is_always_false("undef"));
        assert!(is_always_false("((0))"));
        assert!(is_always_false("  0  "));
    }

    #[test]
    fn non_constant_and_true_conditions_are_not_always_false() {
        assert!(!is_always_false("1"));
        assert!(!is_always_false("$x"));
        assert!(!is_always_false(""));
        assert!(!is_always_false("\"0\""));
        assert!(!is_always_false("undefined"));
        // Conservative by design: `0.0` is falsy in Perl but is not one of the
        // recognised literal spellings, so no dead branch is claimed.
        assert!(!is_always_false("0.0"));
    }

    // ---- is_always_true --------------------------------------------------------

    #[test]
    fn perl_true_constants_are_always_true() {
        assert!(is_always_true("1"));
        assert!(is_always_true("-1"));
        assert!(is_always_true("42"));
        assert!(is_always_true("(1)"));
        assert!(is_always_true("0.5"));
        assert!(is_always_true("\"1\""));
        assert!(is_always_true("'yes'"));
        // Perl's only false strings are "" and "0"; "00" and "0.0" are true.
        assert!(is_always_true("'00'"));
        assert!(is_always_true("\"0.0\""));
    }

    #[test]
    fn perl_false_and_unknown_conditions_are_not_always_true() {
        assert!(!is_always_true("0"));
        assert!(!is_always_true("0.0"));
        assert!(!is_always_true("\"0\""));
        assert!(!is_always_true("'0'"));
        assert!(!is_always_true("\"\""));
        assert!(!is_always_true("''"));
        assert!(!is_always_true("$x"));
        assert!(!is_always_true(""));
        assert!(!is_always_true("undef"));
    }

    #[test]
    fn a_quoted_literal_needs_matching_delimiters() {
        // Mismatched or unterminated quoting is not a literal this may judge.
        assert!(!is_always_true("\"abc"));
        assert!(!is_always_true("abc\""));
        assert!(!is_always_true("\"abc'"));
    }

    // ---- extract_balanced_parens ------------------------------------------------

    #[test]
    fn extract_balanced_parens_returns_the_first_balanced_group() {
        assert_eq!(extract_balanced_parens("(0)"), Some("0"));
        assert_eq!(extract_balanced_parens("()"), Some(""));
        assert_eq!(extract_balanced_parens("(a(b)c) {"), Some("a(b)c"));
        assert_eq!(extract_balanced_parens("(x) and (y)"), Some("x"));
    }

    #[test]
    fn extract_balanced_parens_rejects_unopened_and_unclosed_input() {
        assert_eq!(extract_balanced_parens(""), None);
        assert_eq!(extract_balanced_parens("x"), None);
        assert_eq!(extract_balanced_parens(" (0)"), None);
        assert_eq!(extract_balanced_parens("(0"), None);
        assert_eq!(extract_balanced_parens("((0)"), None);
    }

    // ---- find_block_end ----------------------------------------------------------

    #[test]
    fn find_block_end_reports_the_closing_brace_line() {
        let lines = vec!["if (0) {", "    print 1;", "}", "print 2;"];
        assert_eq!(find_block_end(&lines, 0), 3);
    }

    #[test]
    fn find_block_end_handles_a_single_line_block() {
        let lines = vec!["if (0) { print 1; }", "print 2;"];
        assert_eq!(find_block_end(&lines, 0), 1);
    }

    #[test]
    fn find_block_end_falls_back_to_end_of_file_when_unbalanced() {
        let lines = vec!["if (0) {", "    print 1;"];
        assert_eq!(find_block_end(&lines, 0), 2);
    }

    #[test]
    fn find_block_end_skips_lines_before_the_opener() {
        let lines = vec!["}", "if (0) {", "    print 1;", "}"];
        // Starting at index 1 must ignore the stray closer on line 1.
        assert_eq!(find_block_end(&lines, 1), 4);
    }

    // ---- detect_dead_branches -----------------------------------------------------

    #[test]
    fn always_false_guards_are_reported_as_dead_branches() {
        for keyword in ["if", "while", "elsif"] {
            let source = format!("{keyword} (0) {{\n    print 1;\n}}\n");
            let found = branches(&source);
            assert_eq!(found.len(), 1, "`{keyword} (0)` should yield one dead branch");
            assert_eq!(found[0].code_type, DeadCodeType::DeadBranch);
            assert_eq!(found[0].start_line, 1);
            assert_eq!(found[0].end_line, 3);
            assert!(found[0].reason.contains("always false"), "reason: {}", found[0].reason);
        }
    }

    #[test]
    fn always_true_inverted_guards_are_reported_as_dead_branches() {
        for keyword in ["unless", "until"] {
            let source = format!("{keyword} (1) {{\n    print 1;\n}}\n");
            let found = branches(&source);
            assert_eq!(found.len(), 1, "`{keyword} (1)` should yield one dead branch");
            assert!(found[0].reason.contains("always true"), "reason: {}", found[0].reason);
        }
    }

    #[test]
    fn inverted_guards_are_not_judged_by_the_always_false_rule() {
        // `unless (0)` and `until (0)` always run — the opposite direction of
        // the `if (0)` rule. Applying the wrong polarity here is the most
        // likely wrong implementation.
        assert!(reasons("unless (0) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("until (0) {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn forward_guards_are_not_judged_by_the_always_true_rule() {
        assert!(reasons("if (1) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("while (1) {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn list_iterators_are_never_dead_branches() {
        // `for (0) { }` iterates once with $_ = 0; it is not a boolean guard.
        assert!(reasons("for (0) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("foreach (0) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("for ('') {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn keyword_prefixes_inside_identifiers_do_not_open_a_branch() {
        assert!(reasons("iffy(0) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("untilled (1) {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn dynamic_conditions_are_left_alone() {
        assert!(reasons("if ($x) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("while (@queue) {\n    print 1;\n}\n").is_empty());
        assert!(reasons("").is_empty());
        assert!(reasons("print 1;\n").is_empty());
    }

    #[test]
    fn a_multi_line_condition_is_skipped() {
        // The opening brace must be on the condition's line; anything else is
        // out of scope for this heuristic rather than a guess.
        assert!(reasons("if (0) # comment\n{\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn an_unbalanced_condition_is_skipped() {
        assert!(reasons("if (0 {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn a_condition_without_parentheses_is_skipped() {
        assert!(reasons("if 0 {\n    print 1;\n}\n").is_empty());
    }

    #[test]
    fn scanning_resumes_after_a_reported_block() {
        let source = "if (0) {\n    print 1;\n}\nprint 2;\nwhile (0) {\n    print 3;\n}\nunless (1) {\n    print 4;\n}\n";
        let found = branches(source);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].start_line, 1);
        assert_eq!(found[1].start_line, 5);
        assert_eq!(found[2].start_line, 8);
    }

    #[test]
    fn a_nested_dead_branch_is_not_double_reported() {
        // The outer block is already dead; re-reporting its interior would
        // inflate the count without adding information.
        let found = branches("if (0) {\n    if (0) {\n        print 1;\n    }\n}\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start_line, 1);
        assert_eq!(found[0].end_line, 5);
    }

    #[test]
    fn reported_branches_carry_actionable_metadata() {
        let found = branches("if (0) {\n    print 1;\n}\n");
        assert_eq!(found.len(), 1);
        let file_paths: Vec<&PathBuf> = found.iter().map(|item| &item.file_path).collect();
        assert_eq!(file_paths, vec![&PathBuf::from("/tmp/probe.pl")]);
        assert!(found.iter().all(|item| item.name.is_none()));
        assert!(found.iter().all(|item| (item.confidence - 0.9).abs() < f32::EPSILON));
        assert!(found.iter().all(|item| item.suggestion.is_some()));
    }

    // ---- generate_report -----------------------------------------------------------

    fn sample_analysis() -> DeadCodeAnalysis {
        DeadCodeAnalysis {
            dead_code: vec![DeadCode {
                code_type: DeadCodeType::UnusedSubroutine,
                name: Some("helper".to_string()),
                file_path: PathBuf::from("/tmp/a.pl"),
                start_line: 4,
                end_line: 6,
                reason: "Symbol is never used".to_string(),
                confidence: 0.9,
                suggestion: None,
            }],
            stats: DeadCodeStats {
                unused_subroutines: 1,
                unused_variables: 2,
                unused_constants: 3,
                unused_packages: 4,
                unreachable_statements: 5,
                dead_branches: 6,
                total_dead_lines: 7,
            },
            files_analyzed: 8,
            total_lines: 9,
        }
    }

    #[test]
    fn the_report_surfaces_every_statistic() {
        let report = generate_report(&sample_analysis());
        assert!(report.contains("Files analyzed: 8"));
        assert!(report.contains("Total lines: 9"));
        assert!(report.contains("Dead code items: 1"));
        assert!(report.contains("Unused subroutines: 1"));
        assert!(report.contains("Unused variables: 2"));
        assert!(report.contains("Unused constants: 3"));
        assert!(report.contains("Unused packages: 4"));
        assert!(report.contains("Unreachable statements: 5"));
        assert!(report.contains("Dead branches: 6"));
        assert!(report.contains("Total dead lines: 7"));
    }

    #[test]
    fn an_empty_report_states_zero_rather_than_omitting_rows() {
        let report = generate_report(&DeadCodeAnalysis {
            dead_code: Vec::new(),
            stats: DeadCodeStats::default(),
            files_analyzed: 0,
            total_lines: 0,
        });
        assert!(report.contains("Dead code items: 0"));
        assert!(report.contains("Dead branches: 0"));
    }
}
