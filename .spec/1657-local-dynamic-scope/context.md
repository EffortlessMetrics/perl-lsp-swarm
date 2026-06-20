# Context: Recognize local() as Dynamic Scope Declaration

## Problem Summary

The Perl `local` operator is a dynamic scope declaration — it temporarily modifies the value of a package-scoped variable for the duration of the current block. The scope analyzer does not currently distinguish between:

1. Valid uses: `local` on package variables (declared with `our` or implicitly global)
2. Invalid uses: `local` on lexical variables (declared with `my`) — this is a Perl compile error

### Evidence

**Grammar** (tree-sitter-perl/grammar.js:720-721):
```js
localization_expression: $ => prec(TERMPREC.UNOP, seq(choice('local', 'dynamically'), $._term)),
```

**AST** (Currently produces NodeKind::Unary with op="local"):
- Unary { op: "local", operand: <variable or expression> }
- Unary { op: "dynamically", operand: <variable or expression> }

**Scope Analyzer** (mod.rs:717):
```rust
NodeKind::Unary { op: _, operand } => {
    calls_and_exprs::handle_unary(self, node, operand, scope, ancestors, issues, context);
}
```

The `op` field is discarded via `_` wildcard, so `local` and `dynamically` are treated identically to other unary operators (like `-$x` or `!$x`). No special validation occurs.

### Impact

Users write code like:
```perl
{
    my $x = 1;
    local $x = 2;  # Error: can't localize a lexical
}
```

The scope analyzer does not flag this as an error, missing a class of Perl mistakes.

## Key Decisions

### Decision 1: Recognize local in scope_analyzer, not parser

**Alternatives**:
1. Create AST NodeKind::Localization — parser-level change
2. Handle in scope_analyzer via Unary operator checking (CHOSEN)
3. Ignore entirely (status quo)

**Rationale for CHOSEN**: 
- Parser already produces correct Unary nodes
- Grammar contract is stable (no parser changes needed)
- Validation logic is semantic (scope analyzer's job)
- Avoids ripples to codegen, tree-sitter builds, or parser tests

**Rejected Alt 1**: Too large (parser changes cascade), not necessary (Unary shape is fine)

**Rejected Alt 3**: Misses error detection; doesn't fulfill Perl semantics

### Decision 2: Track is_lexical in Variable struct

**Alternatives**:
1. Add `is_lexical` field to Variable struct (CHOSEN)
2. Infer from parent scope chain (too expensive, ambiguous)
3. Pass declarator as param to handle_localization (simpler locally, harder to maintain)

**Rationale for CHOSEN**:
- Metadata explicitly tracks the kind of declaration
- Available at validation time via find_variable_parts()
- Enables future enhancements (e.g., stricter scope checking, lint rules)
- Clear and maintainable

**Impl detail**: Set is_lexical during Variable construction in declare_variable_parts:
- is_lexical = !is_our (heuristic, since declare_variable_parts is called from handle_variable_declaration which knows the declarator)
- Could be enhanced to pass declarator explicitly, but current heuristic is sufficient and less invasive

### Decision 3: Don't skip builtin special variables

**Alternatives**:
1. Treat builtin special vars as having no is_lexical constraint (CHOSEN)
2. Create special case in handle_localization
3. Skip validation entirely for builtins

**Rationale for CHOSEN**:
- Builtin special vars are already skipped in declarations.rs line 37 (not registered in scope)
- handle_localization will treat them as undeclared/package-scoped (valid)
- No additional logic needed; existing behavior is correct

Example: `local $/` (builtin input record separator)
- Grammar parses as: Unary { op: "local", operand: Variable { sigil: "$", name: "/" } }
- declarations.rs skips registering `$/` in scope (line 37)
- handle_localization finds no Variable, treats as package-scoped (valid)
- No error produced ✓

### Decision 4: Emit LocalOnLexical error immediately

**Alternatives**:
1. Emit error in handle_localization (CHOSEN)
2. Emit during symbol extraction phase
3. Record for post-analysis lint pass

**Rationale for CHOSEN**:
- Scoping is an immediate semantic error (not a style/lint issue)
- Fits into existing IssueKind enum (VariableShadowing, UnusedVariable, etc.)
- LSP integration is automatic (all IssueKind variants are exhaustively matched in error conversion)
- Consistent with existing error detection pattern

## Alternatives Considered

### Alt A: Recognize local as declaration (not use)
**Idea**: Register `local $var` as a declaration in the current scope, similar to `my`.

**Problem**: Perl semantics are incorrect for this. `local` is a **use** and **temporary modification**, not a new binding. It:
1. Requires the variable to already exist (or be implicitly package-scoped)
2. Temporarily saves and restores the value
3. Does not create a new lexical binding

**Rejected**: Violates Perl semantics.

### Alt B: Ignore dynamically entirely
**Idea**: Only handle "local", skip "dynamically".

**Problem**: 
- Perl 5.36+ introduces `dynamically` as an alias for `local` with different syntax
- Semantically equivalent; same validation should apply
- Incomplete coverage

**Rejected**: Incomplete; "dynamically" is valid Perl that needs the same checks.

### Alt C: Simplify is_lexical tracking
**Idea**: Remove is_lexical field; infer from context during validation.

**Problem**:
- Can't infer lexical vs. package scope at validation time without walking parent chain
- Information would need to be reconstructed every time
- Less efficient and harder to maintain

**Rejected**: is_lexical field is the right solution.

### Alt D: Make error only a warning
**Idea**: Emit as Warning or Lint, not Error.

**Problem**:
- Localizing a lexical is a Perl **compile error**, not a style issue
- Should be reported as an error-level diagnostic
- LSP diagnostic severity should be Error

**Rejected**: Incorrect severity.

## Prior Art / Related Issues

### #1518 (Bareword strict false positives)
- Depends on scope analyzer enhancements
- Proper scope handling will help reduce false positives in strict mode checking

### #1654 (State variables scope analyzer)
- Similar scope analyzer feature for `state` variables
- `state` declares a variable once per subroutine, with lexical scope
- Separate implementation, but same analyzer module

### #1664 (Variable initialization semantics)
- Depends on proper scope distinction (my vs. our vs. local vs. state)
- This issue (local) is a prerequisite for accurate initialization analysis

### #1659 (our redeclaration)
- Depends on scope analyzer enhancements
- Proper variable tracking helps validate package-level redeclarations

### #1661 (Scope boundaries in closures)
- Depends on scope analyzer enhancements
- Variable metadata (is_lexical, is_our) will help closure analysis

### Related: DAP Locals scope (#1006)
- Debug Adapter Protocol displays local variables
- Better scope analyzer will improve DAP accuracy

## Scope / Blast Radius Assessment

### Touching

- **Crate**: perl-semantic-analyzer only ✓
- **Module**: scope_analyzer/ only ✓
- **Files**: 
  - `mod.rs` — Variable struct, IssueKind enum, Unary handler (3 changes)
  - `calls_and_exprs.rs` — new handle_localization function (1 change)
  - `tests/scope_and_symbol_tests.rs` — new tests (N/A for spec, done by red-tdd builder)

### Not Touching

- **Parser**: No AST changes; Unary nodes already stable ✓
- **LSP Protocol**: No new Diagnostic fields; existing shape is sufficient ✓
- **perl-workspace**: Symbol table unchanged ✓
- **Other crates**: perl-lsp-rs will consume new IssueKind variant, but LSP error conversion is exhaustive (compile-enforced) ✓

### Backward Compatibility

- **New IssueKind variant**: Additive only; all exhaustive matches in LSP will be flagged at compile time ✓
- **New Variable field**: All construction sites use field initialization syntax (not positional), so adding field is safe ✓
- **New function**: Internal (pub(super)); no external API change ✓

## Testing Strategy

### Unit Tests (within perl-semantic-analyzer)

From acceptance.md §Test-Grid:

1. **Positive**: Package variable with local
2. **Positive**: Undeclared variable with local
3. **Positive**: Builtin special variable with local
4. **Positive**: dynamically operator
5. **Negative**: Lexical variable with local
6. **Negative**: Nested lexical with local
7. **Negative**: Array lexical with local
8. **Adversarial**: Complex expressions (hash deref, etc.)
9. **Adversarial**: Builtin special var under strict mode
10. **State transition**: Scope interaction with surrounding code

### Integration Tests

- Full scope_and_symbol_tests.rs suite must pass
- Existing local-related tests must not regress:
  - scope_local_variable_extracted (line 663)
  - scope_local_named_variable_declaration (line 674)
  - local_input_record_sep_no_false_unused (line 693)

### Lint / Quality

- clippy: No warnings in new code
- fmt: Proper Rust style
- No unwrap/expect/panic/todo in production code

## Implementation Confidence

**High**: 
- All files exist and have been verified ✓
- Root cause is well-understood (op field ignored in Unary handler)
- Changes are localized to 2 files in scope_analyzer/ ✓
- No parser/LSP/workspace changes needed ✓
- Variable struct modification is straightforward ✓
- Heuristic for is_lexical is sound (leverages existing declarator knowledge)

**Medium concerns**:
- Builtin special variable case must remain intact (mitigated by explicit testing)
- Need to verify find_variable_parts method exists (or add it in Step 6)
- Red-TDD builder may discover edge cases in operand expression handling

## Future Enhancements

Once this issue is complete, follow-up work could include:

1. **#1654 (state variables)**: Similar pattern for `state` declarations
2. **#1518 (bareword fixes)**: Use improved scope tracking to reduce false positives
3. **Stricter scope checking**: Flag undeclared variables in strict mode more accurately
4. **DAP improvements**: Leverage Variable metadata for better local variable display
5. **Type narrowing**: Scope metadata could feed into type inference engine

## Links

- **Perl 5 docs**: https://perldoc.perl.org/functions/local
- **Issue #1654**: State variable scope handling (related)
- **Issue #1518**: Bareword strict false positives (depends on this)
- **PARSER_CONTRACTS.md**: Unary operator contract
- **LSP_IMPLEMENTATION_GUIDE.md**: Scope analyzer consumer expectations
