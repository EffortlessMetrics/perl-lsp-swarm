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
                } else if is_block_continuation(trimmed) {
                    // `} else {`, `} elsif`, `} continue {` etc. open a new
                    // reachable branch at the same depth, so the terminator
                    // from the preceding block does not apply (#4656).
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
    if trimmed.is_empty() {
        return false;
    }
    // Pure closing braces / semicolons: `}`, `};`, `}}`.
    if trimmed.chars().all(|ch| ch == '}' || ch == ';') {
        return true;
    }
    // Block-continuation constructs that follow a closing `}` and open a new
    // block at the same depth. Without these, `} else {` after a `return`
    // would be flagged as unreachable code (#4656).
    is_block_continuation(trimmed)
}

/// Whether `trimmed` starts with a block-continuation construct (`} else {`,
/// `} elsif`, etc.) that opens a new reachable branch and clears any prior
/// terminator.
fn is_block_continuation(trimmed: &str) -> bool {
    const BLOCK_CONTINUATIONS: [&str; 5] =
        ["} else {", "} elsif", "} continue {", "} unless {", "} while"];
    BLOCK_CONTINUATIONS.iter().any(|pat| trimmed.starts_with(pat))
}

fn detect_unconditional_terminator(trimmed: &str) -> Option<&str> {
    // Statements that unconditionally end execution of the current scope.
    // `goto` jumps away, `exec` replaces the process, `croak`/`confess` throw.
    // (`last`/`next`/`redo` are excluded because they are conditional on loop
    // context and would cause false positives at file scope — the analyzer is
    // text-based and cannot track loop nesting.)
    const TERMINATORS: [&str; 8] =
        ["return", "die", "exit", "CORE::exit", "goto", "exec", "croak", "confess"];

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
    //! In-crate branch-coverage proof for the private helpers in this module.
    //!
    //! Each helper has at least one positive and one negative case so that
    //! polarity inversions, dropped guards, and first-hit-only scans are caught
    //! by named tests rather than blanket failure. These complement (do not
    //! replace) the integration tests in `crates/perl-parser/tests/` which prove
    //! composition through `WorkspaceIndex`.

    use super::*;

    // ---- is_structural_line ----

    #[test]
    fn structural_line_empty_is_not_structural() {
        // `trimmed` is empty only when the original line was whitespace-only;
        // is_structural_line("") returns false so an empty line after a
        // terminator does NOT clear the terminator (it has no brace content).
        assert!(!is_structural_line(""));
    }

    #[test]
    fn structural_line_closing_brace_is_structural() {
        assert!(is_structural_line("}"));
        assert!(is_structural_line("};"));
        assert!(is_structural_line("}}"));
    }

    #[test]
    fn structural_line_code_is_not_structural() {
        // A real statement is not structural, so it CAN be flagged unreachable.
        assert!(!is_structural_line("print 1;"));
    }

    #[test]
    fn structural_line_block_continuation_is_structural() {
        // `} else {` is structural — it clears the terminator.
        assert!(is_structural_line("} else {"));
        assert!(is_structural_line("} elsif ($x) {"));
        assert!(is_structural_line("} continue {"));
    }

    // ---- is_block_continuation ----

    #[test]
    fn block_continuation_recognizes_all_forms() {
        assert!(is_block_continuation("} else {"));
        assert!(is_block_continuation("} elsif ($x) {"));
        assert!(is_block_continuation("} continue {"));
        assert!(is_block_continuation("} unless {"));
        assert!(is_block_continuation("} while (1) {"));
    }

    #[test]
    fn block_continuation_rejects_non_continuation() {
        assert!(!is_block_continuation("print 1;"));
        assert!(!is_block_continuation("}"));
        assert!(!is_block_continuation("else {")); // missing leading `}`
    }

    // ---- detect_unconditional_terminator ----

    #[test]
    fn terminator_detects_plain_keywords() {
        assert_eq!(detect_unconditional_terminator("return;"), Some("return"));
        assert_eq!(detect_unconditional_terminator("die;"), Some("die"));
        assert_eq!(detect_unconditional_terminator("exit;"), Some("exit"));
        assert_eq!(detect_unconditional_terminator("CORE::exit;"), Some("CORE::exit"));
        assert_eq!(detect_unconditional_terminator("goto LABEL;"), Some("goto"));
        assert_eq!(detect_unconditional_terminator("exec('cmd');"), Some("exec"));
        assert_eq!(detect_unconditional_terminator("croak 'nope';"), Some("croak"));
        assert_eq!(detect_unconditional_terminator("confess 'oops';"), Some("confess"));
    }

    #[test]
    fn terminator_rejects_non_terminator() {
        assert_eq!(detect_unconditional_terminator("print 1;"), None);
        assert_eq!(detect_unconditional_terminator("my $x = 1;"), None);
    }

    #[test]
    fn terminator_rejects_postfix_modifier() {
        // `return if $x` is conditional — not a terminator.
        assert_eq!(detect_unconditional_terminator("return if $x;"), None);
        assert_eq!(detect_unconditional_terminator("die unless $ok;"), None);
        assert_eq!(detect_unconditional_terminator("exit when $done;"), None);
        assert_eq!(detect_unconditional_terminator("return for @list;"), None);
        assert_eq!(detect_unconditional_terminator("return foreach @list;"), None);
    }

    #[test]
    fn terminator_accepts_postfix_comment() {
        // A trailing comment after a real terminator is fine.
        assert_eq!(detect_unconditional_terminator("return; # done"), Some("return"));
    }

    #[test]
    fn terminator_keyword_as_substring_is_not_terminator() {
        // `return` inside `returns` must not match — boundary check.
        assert_eq!(detect_unconditional_terminator("returns_value();"), None);
    }

    // ---- contains_keyword ----

    #[test]
    fn contains_keyword_respects_boundaries() {
        assert!(contains_keyword("if $x", "if"));
        assert!(contains_keyword("$x if 1", "if"));
        assert!(!contains_keyword("swifty", "if")); // 'if' inside a word
        assert!(!contains_keyword("notify", "if")); // 'if' inside a word
    }

    #[test]
    fn keyword_boundary_classification() {
        assert!(is_keyword_boundary(None)); // start/end of string
        assert!(is_keyword_boundary(Some(' ')));
        assert!(is_keyword_boundary(Some('$')));
        assert!(!is_keyword_boundary(Some('a')));
        assert!(!is_keyword_boundary(Some('_')));
        assert!(!is_keyword_boundary(Some('1')));
    }

    // ---- is_always_false ----

    #[test]
    fn always_false_recognizes_standard_idioms() {
        assert!(is_always_false("0"));
        assert!(is_always_false("\"\""));
        assert!(is_always_false("''"));
        assert!(is_always_false("undef"));
        assert!(is_always_false("(0)"));
        assert!(is_always_false("( 0 )"));
        assert!(is_always_false("(((0)))"));
    }

    #[test]
    fn always_false_rejects_non_false() {
        assert!(!is_always_false("1"));
        assert!(!is_always_false("0 but true")); // not a recognized idiom
        assert!(!is_always_false("$x"));
        assert!(!is_always_false("0.1"));
        assert!(!is_always_false("0x0")); // hex — not matched (intentional)
    }

    // ---- is_always_true ----

    #[test]
    fn always_true_recognizes_non_zero_int() {
        assert!(is_always_true("1"));
        assert!(is_always_true("42"));
        assert!(is_always_true("-1"));
        assert!(is_always_true("(1)"));
    }

    #[test]
    fn always_true_recognizes_non_zero_float() {
        assert!(is_always_true("1.5"));
        assert!(is_always_true("0.1"));
        assert!(is_always_true("-0.5"));
    }

    #[test]
    fn always_true_recognizes_non_empty_string() {
        assert!(is_always_true("\"hello\""));
        assert!(is_always_true("'hello'"));
        assert!(is_always_true("\"1\""));
    }

    #[test]
    fn always_true_rejects_zero_and_empty_string_and_zero_string() {
        // Integer zero is falsy in Perl.
        assert!(!is_always_true("0"));
        // Empty quoted strings are falsy.
        assert!(!is_always_true("\"\""));
        assert!(!is_always_true("''"));
        // The string "0" is also falsy in Perl.
        assert!(!is_always_true("\"0\""));
        assert!(!is_always_true("'0'"));
    }

    #[test]
    fn always_true_rejects_variables_and_expressions() {
        assert!(!is_always_true("$x"));
        assert!(!is_always_true("1 + 1"));
    }

    // ---- strip_outer_parens / is_outer_paren_balanced ----

    #[test]
    fn strip_outer_parens_simple() {
        assert_eq!(strip_outer_parens("0"), "0");
        assert_eq!(strip_outer_parens("(0)"), "0");
        assert_eq!(strip_outer_parens("(((0)))"), "0");
        assert_eq!(strip_outer_parens("( 0 )"), "0");
    }

    #[test]
    fn strip_outer_parens_does_not_strip_unbalanced() {
        // `(a)(b)` — the first `(` closes before the last `)`, so stripping
        // would be wrong. Must NOT strip.
        assert_eq!(strip_outer_parens("(a)(b)"), "(a)(b)");
    }

    #[test]
    fn is_outer_paren_balanced_cases() {
        assert!(is_outer_paren_balanced("0"));
        assert!(is_outer_paren_balanced("(a)"));
        assert!(!is_outer_paren_balanced("a)(b")); // depth goes negative
    }

    // ---- extract_balanced_parens ----

    #[test]
    fn extract_balanced_parens_valid() {
        assert_eq!(extract_balanced_parens("(0)"), Some("0"));
        assert_eq!(extract_balanced_parens("(a, b)"), Some("a, b"));
        assert_eq!(extract_balanced_parens("((nested))"), Some("(nested)"));
    }

    #[test]
    fn extract_balanced_parens_missing_or_unbalanced() {
        assert_eq!(extract_balanced_parens("0"), None); // doesn't start with `(`
        assert_eq!(extract_balanced_parens("(unbalanced"), None);
        assert_eq!(extract_balanced_parens(""), None);
    }

    // ---- find_block_end ----

    #[test]
    fn find_block_end_same_line() {
        let lines = vec!["sub foo { return 1; }"];
        assert_eq!(find_block_end(&lines, 0), 1);
    }

    #[test]
    fn find_block_end_multiline() {
        let lines = vec!["if (1) {", "  print;", "}"];
        assert_eq!(find_block_end(&lines, 0), 3);
    }

    #[test]
    fn find_block_end_unbalanced_falls_back_to_eof() {
        let lines = vec!["if (1) {", "  print;"]; // no closing brace
        assert_eq!(find_block_end(&lines, 0), 2);
    }

    // ---- detect_dead_branches ----

    fn dead_branch_reasons(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        detect_dead_branches(Path::new("/test.pl"), text, &mut out);
        out.into_iter().map(|d| d.reason).collect()
    }

    #[test]
    fn detect_dead_branches_if_zero() {
        let reasons = dead_branch_reasons("if (0) {\n  print;\n}\n");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("always false"));
    }

    #[test]
    fn detect_dead_branches_while_zero() {
        let reasons = dead_branch_reasons("while (0) {\n  print;\n}\n");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("always false"));
    }

    #[test]
    fn detect_dead_branches_unless_one() {
        let reasons = dead_branch_reasons("unless (1) {\n  print;\n}\n");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("always true"));
    }

    #[test]
    fn detect_dead_branches_until_one() {
        let reasons = dead_branch_reasons("until (1) {\n  print;\n}\n");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("always true"));
    }

    #[test]
    fn detect_dead_branches_living_condition_not_flagged() {
        // A real variable condition is never dead.
        let reasons = dead_branch_reasons("if ($x) {\n  print;\n}\n");
        assert!(reasons.is_empty(), "living condition should not be flagged: {reasons:?}");
    }

    #[test]
    fn detect_dead_branches_if_one_not_flagged() {
        // `if (1)` is always-true so the if-body executes — NOT dead.
        // Only the *else* would be dead, and else-detection is deferred.
        let reasons = dead_branch_reasons("if (1) {\n  print;\n}\n");
        assert!(reasons.is_empty(), "if(1) body is reachable, not dead: {reasons:?}");
    }

    #[test]
    fn detect_dead_branches_for_loop_not_flagged() {
        // `for`/`foreach` are list iterators, not boolean guards.
        // `for (0) {}` runs once with $_ = 0 — not dead.
        let reasons = dead_branch_reasons("for (0) {\n  print;\n}\n");
        assert!(reasons.is_empty(), "for(0) is a list iterator, not dead: {reasons:?}");
        let reasons = dead_branch_reasons("foreach (0) {\n  print;\n}\n");
        assert!(reasons.is_empty(), "foreach(0) is a list iterator, not dead: {reasons:?}");
    }

    #[test]
    fn detect_dead_branches_nested_inside_dead_skipped() {
        // A dead branch skips to its block end, so a nested dead branch
        // inside it is not separately reported.
        let text = "if (0) {\n  if (0) {\n    print;\n  }\n}\n";
        let reasons = dead_branch_reasons(text);
        assert_eq!(reasons.len(), 1, "nested dead branch should be skipped: {reasons:?}");
    }

    #[test]
    fn detect_dead_branches_elsif_zero() {
        // `elsif` on its own line is detected. Note: `} elsif` on the same
        // line as a closing brace is NOT detected because the scanner matches
        // `elsif` only as a line-start prefix — this is a known limitation of
        // the text-based analyzer, documented here as a behavioral boundary.
        let reasons = dead_branch_reasons("elsif (0) {\n  die;\n}\n");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("always false"));
    }

    #[test]
    fn detect_dead_branches_multiline_condition_skipped() {
        // Multi-line conditions are skipped to avoid false positives.
        let text = "if\n(0) {\n  print;\n}\n";
        let reasons = dead_branch_reasons(text);
        assert!(reasons.is_empty(), "multiline condition should be skipped: {reasons:?}");
    }

    // ---- contains_postfix_modifier ----

    #[test]
    fn contains_postfix_modifier_detects_all() {
        assert!(contains_postfix_modifier("if $x"));
        assert!(contains_postfix_modifier("unless $x"));
        assert!(contains_postfix_modifier("when $x"));
        assert!(contains_postfix_modifier("while $x"));
        assert!(contains_postfix_modifier("until $x"));
        assert!(contains_postfix_modifier("for @list"));
        assert!(contains_postfix_modifier("foreach @list"));
    }

    #[test]
    fn contains_postfix_modifier_rejects_word_substring() {
        // `swifty` contains "if" but it's inside a word — must not match.
        assert!(!contains_postfix_modifier("swifty"));
        // No modifier at all.
        assert!(!contains_postfix_modifier("$x;"));
    }

    // ---- generate_report ----

    #[test]
    fn generate_report_contains_all_stats() {
        let analysis = DeadCodeAnalysis {
            dead_code: vec![],
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
        };
        let report = generate_report(&analysis);
        assert!(report.contains("Files analyzed: 8"));
        assert!(report.contains("Total lines: 9"));
        assert!(report.contains("Unused subroutines: 1"));
        assert!(report.contains("Unused variables: 2"));
        assert!(report.contains("Unused constants: 3"));
        assert!(report.contains("Unused packages: 4"));
        assert!(report.contains("Unreachable statements: 5"));
        assert!(report.contains("Dead branches: 6"));
        assert!(report.contains("Total dead lines: 7"));
    }
}
