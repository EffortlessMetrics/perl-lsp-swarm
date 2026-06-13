# Implementation Checklist: #1404 — Semantic: Dead code detection for unused private subroutines

## Change order (compiles at each step)

### Step 1: Add UnusedPrivateSubroutine to DiagnosticCode enum
- **File:** `crates/perl-diagnostics/src/codes/mod.rs`
- **Change:** Add new variant to `DiagnosticCode` enum in PL300-PL399 (Subroutine) range
- **Details:** 
  - Add after line 97 (after `MissingPodCoverage`): 
    ```rust
    /// Subroutine starting with underscore is defined but never called
    UnusedPrivateSubroutine,
    ```
- **Verify:** `cargo check -p perl-diagnostics`

### Step 2: Add diagnostic code mapping in codes/metadata.rs
- **File:** `crates/perl-diagnostics/src/codes/metadata.rs`
- **Change:** Add mapping case in `DiagnosticCode::as_str()` match arm
- **Details:**
  - Find the match in `impl DiagnosticCode` block (around line 35-99)
  - Add case after line 60 (after `MissingPodCoverage` mapping):
    ```rust
    Self::UnusedPrivateSubroutine => "PL305",
    ```
- **Depends on:** Step 1
- **Verify:** `cargo check -p perl-diagnostics`

### Step 3: Add UnusedPrivateSubroutine to IssueKind enum
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Add new variant to `IssueKind` enum
- **Details:**
  - Add after line 89 (after `CaptureVarWithoutRegexMatch`):
    ```rust
    /// A subroutine starting with underscore is defined but never referenced
    UnusedPrivateSubroutine,
    ```
- **Depends on:** Step 1, 2
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 4: Update scope_analyzer/mod.rs to detect unused private subroutines
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Implement detection logic in ScopeAnalyzer::analyze() after variable analysis
- **Details:**
  - Find the `pub fn analyze()` method (around line 200+)
  - After variable and parameter analysis, add subroutine reference analysis
  - Collect all `sub_*` declarations from symbol table
  - For each subroutine named `_*`, check if it appears in any SymbolReference
  - If no references found, emit ScopeIssue with `IssueKind::UnusedPrivateSubroutine`
  - Key consideration: Only flag subroutines where name starts with `_` (single underscore)
- **Depends on:** Step 3
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 5: Add UnusedPrivateSubroutine case to scope.rs diagnostics provider
- **File:** `crates/perl-lsp-rs-core/src/providers/diagnostics/scope.rs`
- **Change:** Update match arms in `scope_issues_to_diagnostics()` function
- **Details:**
  - In the severity match (around line 31-42), add:
    ```rust
    IssueKind::UnusedPrivateSubroutine => DiagnosticSeverity::Warning,
    ```
  - In the code match (around line 44-55), add:
    ```rust
    IssueKind::UnusedPrivateSubroutine => DiagnosticCode::UnusedPrivateSubroutine,
    ```
  - Also update the tags assignment (around line 66) to include UnusedPrivateSubroutine in the unnecessary tag list:
    ```rust
    tags: if matches!(issue.kind, IssueKind::UnusedVariable | IssueKind::UnusedParameter | IssueKind::UnusedPrivateSubroutine) {
    ```
- **Depends on:** Step 1, 2, 3
- **Verify:** `cargo check -p perl-lsp-rs-core`

### Step 6: Update scope_issues_to_diagnostics_with_semantics() function
- **File:** `crates/perl-lsp-rs-core/src/providers/diagnostics/scope.rs` (continue reading from line 126)
- **Change:** Add UnusedPrivateSubroutine case to the semantic version of the converter
- **Details:**
  - Find the match expression inside `scope_issues_to_diagnostics_with_semantics()`
  - Ensure UnusedPrivateSubroutine is handled (should fall through to default behavior: always emit)
  - Verify the function still compiles with the new issue kind
- **Depends on:** Step 3, 5
- **Verify:** `cargo check -p perl-lsp-rs-core`

### Step 7: Add test for UnusedPrivateSubroutine detection
- **File:** `crates/perl-semantic-analyzer/tests/scope_analyzer_tests.rs` (or create if missing)
- **Change:** Add a test case for unused private subroutine detection
- **Details:**
  - Test should parse Perl code with:
    - `sub _helper { }` — unused private subroutine
    - `sub public_func { }` — public subroutine (should not warn)
    - A normal call to `public_func()`
  - Verify that exactly one ScopeIssue of kind UnusedPrivateSubroutine is emitted for `_helper`
  - Verify that public_func generates no issue
- **Depends on:** Step 3, 4
- **Verify:** `cargo test -p perl-semantic-analyzer --lib scope_analyzer`

### Step 8: Final verification
- **Verify:** 
  - `cargo test -p perl-diagnostics`
  - `cargo test -p perl-semantic-analyzer`
  - `cargo test -p perl-lsp-rs-core`
  - `cargo xtask fmt`
  - `cargo clippy -p perl-diagnostics -p perl-semantic-analyzer -p perl-lsp-rs-core`

## Callers and consumers

- `ScopeIssue` is consumed by: `crates/perl-lsp-rs-core/src/providers/diagnostics/scope.rs` (line 27, 126)
- `IssueKind::*` is pattern-matched in scope.rs diagnostics provider (lines 31-42, 44-55)
- `DiagnosticCode::*` is used throughout LSP provider infrastructure

## Scope boundary

### Files IN scope:
- `crates/perl-diagnostics/src/codes/mod.rs` — add enum variant
- `crates/perl-diagnostics/src/codes/metadata.rs` — add code mapping
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` — add IssueKind and detection logic
- `crates/perl-lsp-rs-core/src/providers/diagnostics/scope.rs` — add diagnostic conversion
- `crates/perl-semantic-analyzer/tests/scope_analyzer_tests.rs` — add test (or create)

### Files OUT of scope (no changes):
- Parser (crates/perl-parser/)
- Symbol extractor (crates/perl-semantic-analyzer/src/analysis/symbol.rs)
- Workspace index (crates/perl-semantic-analyzer/src/analysis/index.rs)
- Configuration (crates/perl-lsp-config/) — deferred to future issue
- DAP (crates/perl-dap/)

## Flags for builder

1. **Subroutine reference tracking**: The scope analyzer must distinguish between subroutine *definitions* (from symbol table) and subroutine *references* (from symbol references). The implementation should track calls to subroutines and check for absence of calls for private subroutines.

2. **Single underscore only**: Only flag subroutines where the name starts with exactly one underscore followed by a letter (e.g., `_helper`, `_internal`). Do NOT flag `__` (dunder) or variable-like `_` alone.

3. **No configuration yet**: The issue mentions opt-out via `.perl-lsp.toml` but this is deferred to a follow-up issue. For now, the diagnostic is always emitted (no per-user suppression).

4. **Single-file scope**: This implementation only detects unused private subroutines *within a single file*. Cross-file references are not tracked yet (e.g., if `_helper` is defined in file A but called from file B, it will be flagged as unused in file A). This is acceptable for v0.17 — future work can enhance to workspace-wide tracking.

5. **Anon subroutines**: The symbol extractor marks anonymous subroutines with name `<anon>`. Ensure these are skipped (they cannot start with `_` anyway).

6. **Test corpus**: Consider testing with:
   - Simple case: `sub _helper { } sub main { }` — flag `_helper`
   - Reference case: `sub _helper { } sub main { _helper(); }` — no flag
   - Public case: `sub helper { } sub main { }` — no flag (public subroutine)
   - Multiple: `sub _a { } sub _b { _b(); }` — flag `_a` only
