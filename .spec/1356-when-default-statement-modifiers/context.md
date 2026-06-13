# Context & Decision Log: Issue #1356

## Problem Statement

The perl-lsp parser rejects valid Perl 5.10+ syntax when `when` or `default` are used as statement modifiers (postfix form), particularly inside `given` blocks. For example:

```perl
given (5) {
    print "When modifier: matched 5\n" when $_ == 5;  # FAILS: Expected 'when' or 'default' in given block
}
```

The error message is misleading: the parser *expects* block-form `when`, not recognizing the postfix modifier form.

## Scope & Impact

**Scope**: Perl 5.10+ feature (`when` and `default` keywords introduced in switch/given statement)

**Affected code locations**:
- Any Perl codebase using statement modifiers with `when` or `default`
- Particularly inside `given` blocks (the most common context)

**Upstream**: No; this is a parser feature request for modern Perl syntax

**Prior art**:
- The parser already recognizes `when` in the `is_stmt_modifier_kind()` list (line 16 of helpers.rs)
- But `default` is missing, and the `parse_given_block()` function rejects modifier form entirely

## Key Decisions

### Decision 1: Reuse existing `StatementModifier` node type
**Chosen**: Yes, no new AST node needed.
**Rationale**: The `StatementModifier { statement, modifier: String, condition }` node already supports arbitrary modifier names. Adding `when` and `default` requires no schema changes, only parser logic updates.
**Alternative considered**: Create separate `WhenModifier` and `DefaultModifier` nodes. Rejected for simplicity and consistency with `if`, `unless`, etc.

### Decision 2: Support both block and modifier forms in given block
**Chosen**: Yes, allow mixed forms.
**Rationale**: Perl allows both forms in given blocks:
```perl
given ($x) {
    when (5) { print "block form\n"; }         # Block: when (cond) { ... }
    print "modifier form\n" when $_ == 5;      # Modifier: stmt when cond;
}
```
The parser should distinguish via lookahead: `when (` = block, `stmt when` = modifier.

**Alternative considered**: Support modifier form only; deprecate block form. Rejected; block form is standard and must remain.

### Decision 3: Implement via lookahead in `parse_given_block()`
**Chosen**: Yes, add `is_when_block_form()` and `is_default_block_form()` helpers.
**Rationale**: Lookahead is already used elsewhere in the parser (e.g., `is_keyword_before_fat_arrow()`). Avoids backtracking and keeps logic clear.
**Alternative considered**: Use backtracking/error recovery. Rejected; lookahead is simpler and matches Perl's own parser strategy.

### Decision 4: Add `TokenKind::Default` to statement modifier list
**Chosen**: Yes, simple one-line fix.
**Rationale**: The `is_stmt_modifier_kind()` function is the authoritative list of statement modifiers. `default` is a valid modifier in Perl (rare, but valid), so it belongs here.
**Alternative considered**: Document `default` as unsupported pending further review. Rejected; Perl semantics are clear; no reason to delay.

## Architecture

### Parser Flow (Current State)

1. `parse_statement()` → `parse_statement_inner()`
2. Detects `given` keyword → `parse_given_statement()`
3. Parses expression in parens, then calls `parse_given_block()`
4. `parse_given_block()` iterates:
   - Expects `when` or `default` keyword at start of statement
   - Calls `parse_when_statement()` or `parse_default_statement()` (both expect block form)
   - Both expect `(cond) { ... }` syntax
   - Throws error if not found

### Parser Flow (After Fix)

1. Same as above until `parse_given_block()`
2. `parse_given_block()` now checks:
   - Is this a `when` keyword? → Check lookahead:
     - If next token is `(` → block form, call `parse_when_statement()`
     - Else → modifier form, parse statement then apply `when` modifier
   - Is this a `default` keyword? → Check lookahead:
     - If next token is `{` → block form, call `parse_default_statement()`
     - Else → modifier form, parse statement then apply `default` modifier
   - Else → error (no bare statements in given block)

## Testing Strategy

### Test Categories

1. **Block form preservation** (must not break):
   - `when (cond) { ... }` still parses as `When` node
   - `default { ... }` still parses as `Default` node

2. **Modifier form support** (new):
   - `stmt when cond;` parses as `StatementModifier { modifier: "when" }`
   - `stmt default;` parses as `StatementModifier { modifier: "default" }`

3. **Mixed forms in given block** (critical):
   - Both forms in same block: `given ($x) { when (1) { print "1"; } print "default" default; }`

4. **Error recovery** (robustness):
   - Malformed given blocks still error appropriately
   - No tokens consumed past error boundary

### Test Corpus

Existing test file: `test_corpus/statement_modifier_comprehensive.pl` (line 71-75) contains exact case from issue:
```perl
given (5) {
    print "When modifier: matched 5\n" when $_ == 5;
    print "When modifier: matched 10\n" when $_ == 10;
}
```
This file should parse clean after the fix.

## Risks & Mitigations

### Risk 1: Lookahead Ambiguity
**Risk**: Peek second token to distinguish block vs modifier; if implementation is wrong, could mispars valid code.
**Mitigation**: Add explicit tests for both forms in same block; verify s-exp output matches expected shape.

### Risk 2: Error Recovery in Given Block
**Risk**: Refactoring error path could break recovery, leaving parser in inconsistent state.
**Mitigation**: Test error cases explicitly; ensure `synchronize()` is called if needed; verify no tokens escape block.

### Risk 3: Whitespace/Position Tracking
**Risk**: Multiline modifiers could have incorrect `SourceLocation` tracking, breaking LSP hover/diagnostics.
**Mitigation**: Verify `statement.location.start` and `condition.location.end` are set correctly; test multiline cases.

## Perl Semantics Verification

**Perl 5 reference**: `given`/`when` introduced in Perl 5.10 (Smart Match operator)
**Valid syntax**: Both forms are valid in Perl 5.10+:
```perl
use feature 'switch';
given ($value) {
    # Block form
    when (5) {
        print "block form\n";
    }
    # Modifier form (rare but valid)
    print "modifier form\n" when $_ == 5;
}

# Modifier form outside given block (also valid)
print "matched\n" when $value == 5;
default { print "default block\n"; };

# Default as modifier (rare, unusual)
print "default case\n" default;
```

**Source**: Perl docs, perlsyn.pod (Smart matching section)

## Alternatives Rejected

### Alternative A: Support only modifier form; reject block form
**Why rejected**: Block form is standard and widely used; breaking change not justified by scope.

### Alternative B: Use backtracking instead of lookahead
**Why rejected**: Adds complexity; lookahead is standard parser technique already used in this codebase.

### Alternative C: Create new NodeKind variants (WhenModifier, DefaultModifier)
**Why rejected**: Unnecessary; `StatementModifier` is generic and already used for other modifiers.

### Alternative D: Document as "unsupported" and close issue
**Why rejected**: Perl syntax is well-defined; no technical reason to delay; would worsen UX for valid code.

## Backwards Compatibility

### Breaking Changes: None
- `when` (block form) parsing unchanged
- `default` (block form) parsing unchanged
- `if`/`unless`/`while`/`until`/`for`/`foreach` (modifiers) parsing unchanged
- Error messages improve (fewer false positives)

### Snapshot Changes Expected
- Test corpus files with `when`/`default` modifiers will now parse clean instead of ERROR
- Snapshots should be updated to reflect correct parse (not a regression)

## Related Issues

- Issue #1356 (this one) — statement modifier `when`/`default` in given blocks
- No known related open issues; this is standalone

## Links

- **Perl docs**: https://perldoc.perl.org/perlsyn#Switch-statements (given/when/default section)
- **Parser contracts**: `docs/reference/PARSER_CONTRACTS.md` (§Given/When/Default Blocks, §Statement Modifiers)
- **Test corpus**: `test_corpus/statement_modifier_comprehensive.pl` (line 71-75)
- **AST reference**: `crates/perl-ast/src/ast.rs:NodeKind::StatementModifier` (line 1872-1880)
