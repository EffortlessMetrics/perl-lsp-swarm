# Implementation Checklist: #1756 — Fix ReDoS vulnerabilities in heredoc anti-pattern regex patterns

## Overview

The heredoc anti-pattern detector uses unbounded character classes (`[^X]+`, `[^X]*`) that trigger catastrophic backtracking on pathological input (unclosed delimiters). This fix bounds all vulnerable patterns to line endings and adds a soft size limit to prevent O(n²) regex matching during normal LSP operation.

## Change order (compiles at each step)

### Step 1: Fix DYNAMIC_DELIMITER_PATTERN
- **File:** `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs` (line 230-234)
- **Change:** Replace unbounded `[^}]+` and `[^`]+` with bounded versions anchored to line end
- **Old pattern:** `r"<<\s*\$\{[^}]+\}|<<\s*\$\w+|<<\s*`[^`]+`"`
- **New pattern:** `r"<<\s*\$\{[^}\n]+\}|<<\s*\$\w+|<<\s*`[^`\n]+`"`
- **Details:** Replace `[^}]+` with `[^}\n]+` and `[^`]+` with `[^`\n]+` to prevent matching past newlines
- **Verify:** `cargo check -p perl-parser`

### Step 2: Fix REGEX_HEREDOC_PATTERN
- **File:** `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs` (line 336-340)
- **Change:** Replace unbounded `[^}]*` with line-bounded version
- **Old pattern:** `r"\(\?\{[^}]*<<[^}]*\}"`
- **New pattern:** `r"\(\?\{[^}\n]*<<[^}\n]*\}"`
- **Details:** Replace both `[^}]*` patterns with `[^}\n]*` to prevent catastrophic backtracking on unclosed braces
- **Verify:** `cargo check -p perl-parser`

### Step 3: Fix EVAL_HEREDOC_PATTERN
- **File:** `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs` (line 386-390)
- **Change:** Replace unbounded `[^']*` and `[^"]*` with bounded versions
- **Old pattern:** `r#"eval\s+(?:'[^']*<<[^']*'|"[^"]*<<[^"]*")\"#`
- **New pattern:** `r#"eval\s+(?:'[^\n']*<<[^\n']*'|"[^\n"]*<<[^\n"]*")"#`
- **Details:** Replace `[^']*` with `[^\n']*` and `[^"]*` with `[^\n"]*` to prevent backtracking on unclosed quotes
- **Verify:** `cargo check -p perl-parser`

### Step 4: Fix @EXPORT pattern in moniker.rs
- **File:** `crates/perl-lsp-rs/src/runtime/language/moniker.rs` (line 277)
- **Change:** Replace unbounded character class in @EXPORT qw regex
- **Old pattern:** `r"@EXPORT(?:_OK)?\s*=\s*qw[(\[{/<|!]([^)\]}/|!>]+)[)\]}/|!>]"`
- **New pattern:** `r"@EXPORT(?:_OK)?\s*=\s*qw[(\[{/<|!]([^\n)\]}/|!>]+)[)\]}/|!>]"`
- **Details:** Replace `[^)\]}/|!>]+` with `[^\n)\]}/|!>]+` to prevent backtracking on unclosed delimiters
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 5: Verify all regex patterns compile
- **File:** Both files modified above
- **Change:** Ensure Regex compilation doesn't fail
- **Verify:** `cargo test -p perl-parser --lib && cargo test -p perl-lsp-rs --lib`

### Step 6: Final verification
- **Verify:** 
  ```bash
  cargo test -p perl-parser --lib && \
  cargo test -p perl-lsp-rs --lib && \
  cargo xtask fmt && \
  cargo clippy -p perl-parser && \
  cargo clippy -p perl-lsp-rs
  ```

## Callers and consumers

- `DYNAMIC_DELIMITER_PATTERN.captures_iter()` called from `DynamicDelimiterDetector::detect()` in `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs:246`
- `REGEX_HEREDOC_PATTERN.captures_iter()` called from `RegexHeredocDetector::detect()` in `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs:352`
- `EVAL_HEREDOC_PATTERN.captures_iter()` called from `EvalHeredocDetector::detect()` in `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs:401`
- `EXPORT_QW_RE.captures_iter()` called from `is_symbol_exported()` in `crates/perl-lsp-rs/src/runtime/language/moniker.rs:281`
- All patterns are ultimately called from `detect_heredoc_antipatterns()` in `crates/perl-lsp-rs-core/src/providers/diagnostics/heredoc_antipatterns.rs:12` on every document change

## Scope boundary

**Files IN scope:**
- `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs` (4 pattern definitions)
- `crates/perl-lsp-rs/src/runtime/language/moniker.rs` (1 pattern definition)

**Files OUT of scope:**
- All other files in the codebase
- No new structs, functions, or public API surface
- No changes to AST or protocol handling
- No changes to LSP dispatch or DAP protocol
- No test infrastructure changes beyond red-TDD test additions

## Flags for builder

- **Regex change scope:** Small and localized — only regex patterns change, no logic changes
- **Backtracking risk:** Bounded patterns complete in O(n) time instead of O(n²) catastrophic backtracking
- **Multiline input:** The fix anchors to `\n`, which means:
  - Valid: `<<${foo}`  (captured)
  - Valid: `<<${foo\n` (stops at newline, not captured, safe)
  - Valid: `<<${foo bar` on same line (captured if continues on same line)
  - Safe input that spanned multiple lines will now only match on the same line as `<<` — acceptable tradeoff for safety
- **Acceptable accuracy loss:** The fix may miss anti-patterns that span multiple lines (rare in practice). This is a safe tradeoff: security (prevent DoS) > exhaustive diagnostics.
