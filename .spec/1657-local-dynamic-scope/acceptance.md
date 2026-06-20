# Acceptance Spec: Recognize local() as Dynamic Scope Declaration

## §Behavior

Scope analyzer should recognize `local` expressions as dynamic scope declarations and validate them appropriately.

| Input / Condition | Expected Result | Test Name |
|---|---|---|
| `my $x; local $x;` (localize lexical var) | Error: `LocalOnLexical` with variable name `$x` | `scope_analyzer_local_on_lexical_error` |
| `our $x; local $x;` (localize package var) | No error; variable treated as used in dynamic scope | `scope_analyzer_local_on_package_var_allowed` |
| `local $unexisting = 5;` (localize undeclared package var) | No error; valid Perl (package var implicit) | `scope_analyzer_local_undeclared_package_var_ok` |
| `local $/;` (builtin special var) | No error; variable not registered in scope; no spurious diagnostics | `scope_analyzer_local_builtin_special_var_ok` |
| Nested block: `{ my $x; { local $x; } }` | Error: `LocalOnLexical` at inner block for `$x` | `scope_analyzer_local_nested_block_error` |
| `dynamically $count` (Perl 5.36+, alias for local) | Same as `local` — error if lexical, ok if package | `scope_analyzer_dynamically_treated_like_local` |

## §Hazards

| Hazard Class | Surface | Risk | Mitigation |
|---|---|---|---|
| **Scope-1: False negatives — local on lexical not caught** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` — Unary handler line 717, calls_and_exprs.rs handle_unary, Variable struct tracking | Med | New `LocalOnLexical` IssueKind; track `is_lexical` in Variable; inspect operand in handle_localization before acceptance |
| **Scope-2: Regression — existing local() tests fail** | `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs` — scope_local_variable_extracted, scope_local_named_variable_declaration, local_input_record_sep_no_false_unused | Med | Run full test suite; ensure builtin special var case (declarator == "local" && is_builtin_global) is not disturbed by new handle_localization logic |
| **Scope-3: False positives — non-local unary operators mishandled** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` line 717 — Unary { op, operand } branch | Low | Conditional check: `if op == "local" \|\| op == "dynamically"` routes to new handler; all other ops route to handle_unary as before |
| **Scope-4: Variable metadata races — is_lexical field not initialized** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` — Variable struct line 108, declare_variable_parts line 213-221 | Low | Add `is_lexical: bool` field during Variable construction; set from declarator (`is_lexical = declarator == "my"`); initialize in all code paths |
| **Scope-5: API breakage — IssueKind enum change** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` — IssueKind enum line 69; consumers in LSP and tests | Low | New IssueKind::LocalOnLexical variant is additive; LSP error-reporting code will match all variants via exhaustive patterns (already compile-enforced) |
| **Scope-6: Integration — localization on complex operands** | `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/calls_and_exprs.rs` — handle_localization recursion | Low | Walk operand via analyzer.analyze_node() to catch nested expressions; test adversarially: `local $x->{key}`, `local @array[0..2]` |

## §Contracts

### PARSER_CONTRACTS.md References

1. **Unary Operator Shape** — Grammar line 720-721 `localization_expression` produces `NodeKind::Unary { op: "local" | "dynamically", operand: Node }`. No AST changes; parsing contract is stable.

2. **Variable Node Representation** — Operand is either `NodeKind::Variable { sigil, name }` (simple var) or complex expression. Analyzer must recurse into complex operands.

### LSP Protocol

N/A — Scope analyzer is upstream of LSP; no new protocol surface. Error reporting uses existing `Diagnostic.message` and `range` fields.

### ScopeAnalyzer Contract

1. **Declaration vs. Use** — `local` is a **use** of an existing variable name (package or lexical), not a declaration. If the variable was declared as `my` (lexical), localization is an error. If declared as `our` or implicitly package-scoped, it's valid.

2. **Symbol Table** — Builtin special variables (`$/`, `$,`, etc.) are skipped from scope entry (handled in declarations.rs line 37). New code must not re-register them in handle_localization.

3. **Error Reporting** — All IssueKind variants are exhaustive in LSP error conversion; new variant will be caught at compile time if not handled.

## §API-Shape

### New Types / Enums / Variants

1. **IssueKind::LocalOnLexical** (enum variant)
   - Added to `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` IssueKind enum line 69
   - Consumed by: LSP error diagnostics (exhaustive match, compile-enforced), tests
   - Dup-risk: None — variant name unique within enum

2. **Variable.is_lexical** (struct field)
   - Added to `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs` Variable struct line 108
   - Set during `declare_variable_parts()` call from `handle_variable_declaration()` (declarator == "my")
   - Read in new `handle_localization()` function
   - Dup-risk: None — field name unique within Variable struct

### New Functions

1. **calls_and_exprs::handle_localization<'a>()**
   - Signature: `pub(super) fn handle_localization<'a>(..., operand: &'a Node, ...) -> ()`
   - Inserted in: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/calls_and_exprs.rs` after handle_unary
   - Called from: mod.rs line 717 Unary handler, gated on `op == "local" || op == "dynamically"`
   - Responsibility: Extract variable from operand; check is_lexical flag; emit LocalOnLexical issue if present; recurse into operand

### Changed Signatures

1. **ScopeAnalyzer::declare_variable_parts()** (line 182-224)
   - Does not change signature
   - Changes: Pass `is_lexical = (declarator == "my")` when constructing Variable (line 215)

### Caller Count

- `handle_variable_declaration()`: Called ~1x per `VariableDeclaration` node during analysis; no change to caller count
- New `handle_localization()`: Called from Unary handler (line 717) when op matches "local" or "dynamically"
- Estimated call sites: ~5-20 per typical Perl file with `local` expressions

## §Test-Grid

| Test Category | Test Name | Input | Assertion |
|---|---|---|---|
| **Positive** | `scope_analyzer_local_on_package_var_allowed` | `our $x = 1; { local $x = 2; }` | No `LocalOnLexical` error; variable marked used |
| **Positive** | `scope_analyzer_local_undeclared_package_var_ok` | `{ local $y = 5; }` | No error; no undeclared variable error (implicit package var) |
| **Positive** | `scope_analyzer_dynamically_treated_like_local` | `dynamically $count = 10;` | Same handling as `local`; no error if package var |
| **Negative** | `scope_analyzer_local_on_lexical_error` | `{ my $x = 1; local $x = 2; }` | Error: `IssueKind::LocalOnLexical`, variable_name `$x` |
| **Negative** | `scope_analyzer_local_nested_lexical_error` | `{ my $x; { local $x; } }` | Error at inner block: `LocalOnLexical` for `$x` |
| **Negative** | `scope_analyzer_local_on_array_lexical_error` | `my @arr = (1,2,3); local @arr;` | Error: `LocalOnLexical`, variable_name `@arr` |
| **Adversarial** | `scope_analyzer_local_complex_expr` | `local $hash{$key};` | Recurses into operand; no crash; same validation applies to `$hash` |
| **Adversarial** | `scope_analyzer_local_builtin_special` | `use strict; local $/ = undef;` | No spurious UnusedVariable or UndeclaredVariable for `$/` |
| **State-transition** | `scope_analyzer_local_then_use_after_block` | `{ my $x = 1; { local $x = 2; } print $x; }` | Outer `$x` is used after block; inner local is valid; no error on outer use |

## §Blast-Radius

### Consumers (crates/modules that depend on perl-semantic-analyzer)

1. **crates/perl-lsp-rs** — LSP server routes scope analyzer output to `textDocument/publishDiagnostics`
   - Impact: New `LocalOnLexical` error will appear in diagnostics
   - Scope: Medium — new error class is informational, does not change existing error reporting

2. **crates/perl-workspace** — Cross-file symbol indexing and scope analysis
   - Impact: None — workspace module reads ScopeIssue but does not check IssueKind variants
   - Scope: Low

3. **Tests: crates/perl-semantic-analyzer/tests/** — Existing scope tests must not regress
   - Scope: Medium — new handle_localization must not break builtin special var handling (line 37 of declarations.rs)

### Boundary

- **Must not touch**: Parser (AST shape, NodeKind enum) — Unary nodes already exist and are stable
- **Must not touch**: perl-workspace symbol table — no new symbol types needed
- **Must not touch**: LSP protocol — existing Diagnostic shape is sufficient

### Risk Summary

- **Touching 1 crate** (perl-semantic-analyzer) ✓
- **Touching 1 module** (scope_analyzer/) ✓
- **New public API**: 1 enum variant (additive, no breakage) ✓
- **Changed signatures**: 0 ✓
- **Regression risk**: Medium (builtin special var case must remain intact) — mitigated by full test suite run
