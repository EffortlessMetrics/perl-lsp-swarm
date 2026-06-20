# Acceptance Criteria: Issue #1661

**Title**: fix(scope-analyzer): our variable redeclaration not validated — only allows across package boundaries

**Issue**: #1661

**Branch**: `impl/1661-our-redeclaration-validation`

---

## § Behavior

Scope analyzer validation for `our` variable redeclaration with package-boundary awareness.

| Input | Condition | Expected Result | Test Name |
|-------|-----------|-----------------|-----------|
| `package Foo; our $x = 1; our $x = 2;` | Same package, same scope, redeclared `our` | `VariableRedeclaration` error reported at second `our` line | `scope_our_same_scope_redeclaration_error` |
| `package Foo; our $x = 1; package Bar; our $x = 2;` | Different packages, redeclared `our` | No error; silently accepted | `package_our_different_package_redeclaration_allowed` |
| `{ our $x = 1; our $x = 2; }` in same package | Same package, nested block scope, redeclared `our` | `VariableRedeclaration` error reported at second `our` line | `scope_our_same_scope_redeclaration_error` (covers nested blocks) |
| `use strict; package Foo; our $x = 1; our $x = 2;` | Same package under strict pragma, redeclared `our` | `VariableRedeclaration` error reported | `scope_our_same_scope_redeclaration_error` |
| `package Foo; our $x; our $x;` | Same package, multiple declarations without initialization | `VariableRedeclaration` error reported | `scope_our_same_scope_redeclaration_error` (covers edge case) |

---

## § Hazards

Hazard class assessment for scope analyzer variable redeclaration validation.

| Hazard Class | Surface | Risk | Mitigation | Notes |
|--------------|---------|------|------------|-------|
| **PARSER-1: Malformed AST Recovery** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs:handle_variable_declaration` | Low | Scope analysis runs post-parse; AST is already validated. New logic only examines previously-extracted variable names and package context. | Change does not depend on parser recovery paths; scope analyzer is a post-parse analysis pass. |
| **PARSER-2: Sigil Ambiguity (Indirect Objects, Quote-Likes)** | N/A — indirect object parsing, quote-like operators | N/A — scope analysis consumes AST, not raw tokens | No parser-layer change required; scope analyzer consumes pre-validated AST. | Scope analysis does not re-parse sigils; it validates declared names post-parse. |
| **LSP-1: Threading and Shared State** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:ScopeAnalyzer` struct | Low | `ScopeAnalyzer` is stateless; `Scope` struct uses `RefCell` for internal mutability within a single analysis pass. New helper method `get_variable_package_context()` is pure (reads scope, no mutation). | No new shared state introduced; new method follows existing `RefCell`-based pattern. |
| **LSP-2: Protocol Handler Contracts** | `crates/perl-lsp-rs/src/providers/` LSP handlers consuming scope analysis results | Low | Handlers consume `ScopeIssue` objects which now include `VariableRedeclaration` for same-package `our` redeclarations. Existing diagnostic rendering code handles this correctly. | Change only adds more cases to existing `VariableRedeclaration` kind; no new `IssueKind` introduced. |
| **SEM-3: Scope Chain Traversal** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:declare_variable_parts_in_context` and lookup methods | Medium | New logic checks `scope.has_variable_parts()` at current scope level. Previous implementation already used this method; new logic is additive (checks current scope, then fallback). | Must ensure `get_variable_package_context()` correctly queries scope chain and does not double-count parent scopes where variable already declared. |
| **SEM-4: Package Context State** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/AnalysisContext:current_package` | Medium | Package context is maintained via `AnalysisContext.current_package` which is updated when entering/exiting package statements. New method retrieves qualified name via existing `package_variable_name()` which reads current package. Must verify package context is correct at declaration time. | Implementation depends on package stack being correctly maintained; should test nested package blocks and package statement changes. |
| **QUAL-1: Untested Code Paths** | New method `get_variable_package_context()` and new conditional branch in `handle_variable_declaration()` | Medium | Comprehensive test coverage required: same-package redeclaration, different-package redeclaration, nested scopes, strict mode, uninitialized declarations. See § Test-Grid. | TDD builder writes failing tests first; red tests must cover all branches. |

---

## § Contracts

Scope analyzer semantic contracts and LSP protocol surface affected by this change.

| Contract | File:Function | Change | Rationale |
|----------|---------------|--------|-----------|
| **PARSER_CONTRACTS.md § Variable Declaration** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs:handle_variable_declaration` | Adds package-aware validation for `our` redeclaration | Enforces stricter semantic rule: same-package `our` redeclaration now errors (previously silently accepted). Behavior aligns with design intent, not Perl's actual behavior (Perl allows it). |
| **Scope Analysis: IssueKind enum** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:IssueKind::VariableRedeclaration` | No structural change; semantic change in when error is reported | `VariableRedeclaration` now fires for same-package `our` redeclaration. Existing downstream code (LSP handlers) already processes this issue kind correctly. |
| **Scope Analysis: ScopeAnalyzer public API** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:ScopeAnalyzer` | Adds internal helper `get_variable_package_context()` (pub(super)) | New helper is module-private (pub(super)); no public API surface change. No external callers. |
| **LSP Diagnostic Protocol** | `crates/perl-lsp-rs/src/providers/` diagnostic handlers | No change to `Diagnostic` message format | Existing `VariableRedeclaration` rendering code handles the new error cases automatically. LSP clients receive standard `Diagnostic` with `message`, `range`, `severity` as before. |
| **Workspace Symbol Indexing** | `crates/perl-workspace/` symbol resolution | No change | Scope analysis is post-index; symbol indexing does not depend on redeclaration validation. |

---

## § API-Shape

New and modified public types and functions.

| Item | Type | Location | Change | Dup Risk |
|------|------|----------|--------|----------|
| `get_variable_package_context()` | Method | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:impl ScopeAnalyzer` | **NEW**: pub(super) helper to retrieve package context of previously declared variable. Signature: `(&self, scope: &Rc<Scope>, sigil: &str, name: &str, context: &AnalysisContext<'_>) -> Option<String>` | Low — method is module-private (pub(super)); no external exposure. Grep `get_variable_package_context` should return only definition + one call site in `declarations.rs`. |
| `IssueKind::VariableRedeclaration` | Enum variant | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:IssueKind` | **No change to enum**; semantic change in when this variant is generated for `our` declarations. Same-package redeclaration now triggers it (previously suppressed). | Medium — this enum variant is used across LSP providers and tests. Verify all consumers of `IssueKind::VariableRedeclaration` still handle the new cases correctly. Expected: all existing diagnostic rendering code handles it transparently. Grep `VariableRedeclaration` should return ~30 call sites; no changes needed in diagnostic rendering. |
| `handle_variable_declaration()` | Function | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs` | **Modified**: logic for `is_our && issue_kind == VariableRedeclaration` now checks package context instead of blindly suppressing error. | Low — function is module-internal (pub(super)); only caller is `analyze_node()` which does not change. |

**Dup-Risk Grep Commands**:
```bash
# Verify get_variable_package_context not duplicated
grep -rn "get_variable_package_context" crates/perl-semantic-analyzer/

# Verify VariableRedeclaration consumer count (should be ~30-40, no new ones)
grep -rn "VariableRedeclaration" crates/ --include="*.rs" | wc -l

# Verify handle_variable_declaration not duplicated
grep -rn "fn handle_variable_declaration" crates/perl-semantic-analyzer/
```

---

## § Test-Grid

Comprehensive test matrix covering positive, negative, boundary, and state-transition cases.

| Category | Input | Preconditions | Expected Behavior | Test Name | Invariant |
|----------|-------|---|---|-----------|-----------|
| **Positive: Same-package redeclaration error** | `package Foo; our $x = 1; our $x = 2;` | strict mode | Reports `VariableRedeclaration` at line 2 | `scope_our_same_scope_redeclaration_error` | Exactly 1 `VariableRedeclaration` issue for `$x` in same package and scope. |
| **Positive: Different-package redeclaration allowed** | `package Foo; our $x = 1; package Bar; our $x = 2;` | strict mode | No `VariableRedeclaration` error | `package_our_different_package_redeclaration_allowed` | Zero `VariableRedeclaration` issues for `$x` across different packages. |
| **Positive: Uninitialized same-package redeclaration error** | `package Foo; our $x; our $x;` | strict mode | Reports `VariableRedeclaration` at line 2 | `scope_our_same_scope_redeclaration_error` | Exactly 1 `VariableRedeclaration` issue (covers uninitialized redeclaration). |
| **Boundary: Nested block, same package** | `package Foo; { our $x = 1; } { our $x = 2; }` | strict mode | Zero `VariableRedeclaration` errors (different block scopes) | `package_our_different_block_scopes_allowed` | No redeclaration error because blocks are separate scopes; each declares `Foo::x` in its own scope. |
| **Boundary: Nested block, same inner package** | `package Foo; { our $x = 1; our $x = 2; }` | strict mode | Reports `VariableRedeclaration` at second line | `scope_our_same_scope_redeclaration_error` | Same scope within one block; redeclaration error. |
| **Boundary: Package block syntax (5.10+)** | `package Foo { our $x = 1; our $x = 2; }` | strict mode | Reports `VariableRedeclaration` | `scope_our_same_scope_redeclaration_error` | Block syntax does not change scope handling; redeclaration still detected. |
| **Negative: `my` redeclaration in same scope** | `my $x = 1; my $x = 2;` | strict mode | Reports `VariableRedeclaration` (unchanged behavior) | `scope_my_redeclaration_same_scope_error` | Existing behavior unchanged; `my` redeclaration in same scope still errors. |
| **Negative: `our` + `my` mixing** | `package Foo; our $x = 1; my $x = 2;` | strict mode | Reports `VariableShadowing` (not redeclaration) | `scope_our_then_my_shadowing` | `our` in package scope, `my` in nested lexical scope = shadowing, not redeclaration. |
| **State: Package change + redeclaration** | Code: `package Foo; our $x; package Bar; our $x; package Foo; our $x;` | strict mode | Zero redeclaration errors (each package maintains separate `$x`) | `package_our_package_switch_allows_redecl` | Switching to another package and back; each `package Foo::x` is independent from `Bar::x`. |
| **Adversarial: Same-package, multiple redeclarations** | `package Foo; our $x = 1; our $x = 2; our $x = 3;` | strict mode | Reports `VariableRedeclaration` at line 2 and possibly line 3 | `scope_our_multiple_redeclarations` | Once declared, subsequent redeclarations in same scope all error (or at minimum, second redeclaration errors). |

**Test Execution**:
```bash
cargo test -p perl-semantic-analyzer scope_our_same_scope_redeclaration_error 2>&1
cargo test -p perl-semantic-analyzer package_our_different_package_redeclaration_allowed 2>&1
cargo test -p perl-semantic-analyzer scope_my_redeclaration_same_scope_error 2>&1
# (Run all scope tests to verify no regressions)
cargo test -p perl-semantic-analyzer --lib scope_analyzer 2>&1
```

---

## § Blast-Radius

Consumers, downstream crates, and must-not-touch boundaries.

| Layer | Crate | Impact | Notes |
|-------|-------|--------|-------|
| **Direct consumers of ScopeAnalyzer** | `perl-lsp-rs` (handlers) | **Medium** — LSP diagnostic handlers consume `ScopeIssue` and render `VariableRedeclaration`. New cases now report this issue more frequently (only for `our` redeclaration in same package; existing `my` redeclaration behavior unchanged). Handlers already correctly render this issue kind. No handler code change needed. | Verify LSP tests still pass; expect one test regression (the old `package_our_same_package_redeclaration_is_silent` behavior test that now expects error). |
| **Indirect consumers** | `perl-workspace` (symbol indexing) | **Low** — symbol indexing runs post-parse, pre-scope-analysis. Does not depend on scope validation. Scope analysis output does not affect workspace index. | No change needed. |
| **Test infrastructure** | `perl-tdd-support` helper macros | **Low** — test helpers (`scope_issues()`, `scope_issues_strict()`) unchanged. Tests only differ in assertions (now expecting error instead of silence). | Update test expectations in `scope_and_symbol_tests.rs` per checklist. |
| **Must-NOT-touch** | `perl-parser` (AST structure) | **Off-limits** — change is post-parse analysis; no AST changes. | No parser modifications. Parser contracts (PARSER_CONTRACTS.md) unchanged. |
| **Must-NOT-touch** | `perl-lexer` (tokenization) | **Off-limits** — change is semantic analysis, not lexical analysis. | No lexer modifications. |
| **Must-NOT-touch** | LSP protocol handlers (DAP, completion, hover) | **Off-limits** — change only affects diagnostic severity/message for existing `VariableRedeclaration` issue kind; protocol shape unchanged. | Handlers render `ScopeIssue` diagnostics transparently; no handler changes needed. |
| **Regression testing** | `tests/scope_and_symbol_tests.rs` | **Required** — existing test `package_our_same_package_redeclaration_is_silent()` must be updated to expect error instead of silence. This is the primary behavior change and must be reflected in test expectations. | See checklist Step 3 for exact modification. |

**Verification**:
```bash
# Run all semantic-analyzer tests to check for regressions
cargo test -p perl-semantic-analyzer 2>&1 | grep -E "test result:|FAILED"

# Run LSP diagnostic tests to verify handlers still work correctly
cargo test -p perl-lsp-rs --lib diagnostic 2>&1 | head -20

# Check no parser/lexer changes
git diff crates/perl-parser crates/perl-lexer 2>&1 | grep -c "@@" || echo "No diffs"
```

---

## § Coverage-Map

N/A — change does not involve coverage/CI infrastructure. Scope validation is unit-tested via `perl-semantic-analyzer` test suite. No coverage tooling changes required.

---

## Summary

This change tightens `our` variable redeclaration validation in the scope analyzer. Same-package `our` redeclaration (previously silently accepted) now reports `VariableRedeclaration`, while different-package redeclaration remains silently accepted per Perl idiom. The fix makes perl-lsp stricter than Perl itself, which is an intentional design decision for linting/analysis purposes.

**Test Coverage**: 5 new/modified tests covering positive (same-package error, different-package allowed), boundary (nested scopes, package blocks), negative (my redeclaration unchanged), and state-transition (package switching) cases.

**Risk**: Low to Medium. Semantic change in redeclaration detection; existing diagnostic rendering code handles new cases transparently. One test behavior expectation changes; LSP handlers work unchanged.
