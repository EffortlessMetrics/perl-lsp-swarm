# Acceptance Criteria: Issue #1356 — Statement Modifier `when` and `default`

## §Behavior

| Condition | Input | Expected Result | Notes |
|-----------|-------|-----------------|-------|
| Simple `when` modifier | `print "matched\n" when $cond;` | Parses as `StatementModifier { modifier: "when" }` with no error | Perl 5.10+ standard |
| Function call with `when` modifier | `do_x() when $cond;` | Parses as `StatementModifier { modifier: "when" }` | Common idiom |
| Assignment with `when` modifier | `$x = 1 when $cond;` | Parses as `StatementModifier { modifier: "when" }` | Valid Perl |
| `when` modifier in given block | `given ($var) { print "ok" when $_; }` | Parses without error; statement is `StatementModifier` not `When` block | Previously failed with "Expected 'when' or 'default' in given block" |
| Simple `default` modifier | `print "default\n" default;` | Parses as `StatementModifier { modifier: "default" }` | Rare but valid Perl |
| `default` modifier in given block | `given ($var) { print "default\n" default; }` | Parses without error; statement is `StatementModifier` not `Default` block | Complements `when` modifier fix |
| `when` block form (unchanged) | `when ($cond) { ... }` | Still parses as `When { condition, body }` block | Existing behavior must not break |
| `default` block form (unchanged) | `default { ... }` | Still parses as `Default { body }` block | Existing behavior must not break |
| Chained modifiers | `print "ok" when $a unless $b;` | Parses correctly with modifier precedence | `when` is primary, `unless` is nested (if supported) |

---

## §Hazards

### PARSER-1: Statement vs Block Distinction (Ambiguous Lookahead)

**Surface**: `crates/perl-parser-core/src/engine/parser/control_flow.rs:parse_given_block()`

**Hazard**: Inside a given block, parser must distinguish between:
- Block form: `when (cond) { stmt; }` — parse as `When` node
- Modifier form: `stmt when cond;` — parse as `StatementModifier` node

Both start with `when` keyword, requiring lookahead to `(` vs next token.

**Invariant**: Parser must use consistent lookahead (peek second token) to determine form, and fail gracefully if neither matches.

**Mitigation**:
1. Add `is_when_block_form()` helper: check `peek() == TokenKind::When && peek_second() == TokenKind::LeftParen`
2. Add `is_default_block_form()` helper: check `peek() == TokenKind::Default && peek_second() == TokenKind::LeftBrace`
3. Fall back to modifier form only after block form check fails
4. Write tests for both forms in same given block (e.g., mix `when (1) { ... }` and `stmt when 2;`)

**Test obligation**: `test_when_block_and_modifier_mixed_in_given()` — verify both forms coexist

---

### PARSER-2: Modifier Keyword Coverage (Incomplete List)

**Surface**: `crates/perl-parser-core/src/engine/parser/helpers.rs:is_stmt_modifier_kind()` (line 8-19)

**Hazard**: `TokenKind::Default` was not in the statement modifier list. If other keywords (e.g., `given` in rare contexts) are used as modifiers, they may be missed.

**Invariant**: All statement modifier keywords (`if`, `unless`, `while`, `until`, `for`, `foreach`, `when`, `default`) must be present in `is_stmt_modifier_kind()` match list.

**Mitigation**:
1. Verify Perl docs for all valid statement modifiers (RFC or camel book)
2. Add `TokenKind::Default` to line 16 (after `TokenKind::When`)
3. Grep codebase for uses of `is_stmt_modifier_kind()` to ensure no callers assume a fixed set

**Test obligation**: `test_modifier_keyword_coverage()` — assert all 8 keywords are recognized

---

### PARSER-3: Recovery in Given Block (Error Synchronization)

**Surface**: `crates/perl-parser-core/src/engine/parser/control_flow.rs:parse_given_block()` error branch

**Hazard**: If a statement inside given block is neither `when` nor `default` block/modifier, parser errors with "Expected 'when' or 'default' in given block". Malformed input (e.g., typo `whe $x;`) may produce unhelpful error or leave parser in bad state.

**Invariant**: Parser must synchronize cleanly after error, not consume tokens past block boundary (`}`).

**Mitigation**:
1. Error recovery should not consume beyond the problematic statement
2. Ensure `synchronize()` is called if needed
3. Test with malformed given blocks: `given (1) { print "ok"; }` (no when/default)

**Test obligation**: `test_given_block_error_recovery()` — verify error doesn't corrupt subsequent parsing

---

### PARSER-4: AST Shape Consistency (NodeKind Field)

**Surface**: `crates/perl-ast/src/ast.rs:NodeKind::StatementModifier` (line 1872-1880)

**Hazard**: `StatementModifier { modifier: String }` field must accept arbitrary string names. If code assumes a fixed set of modifiers, adding `when` and `default` may break downstream analysis.

**Invariant**: Modifier field is a `String`, not an enum. Consumers must not assume a closed set.

**Mitigation**:
1. Verify `to_sexp()` output format for `StatementModifier { modifier: "when" }`
2. Grep for consumers of `NodeKind::StatementModifier` to check for hardcoded modifier names
3. Add test: s-exp output must include `statement_modifier_when` and `statement_modifier_default`

**Test obligation**: `test_sexp_shape_for_when_default()` — verify s-exp contains correct fragment

---

### PARSER-5: Statement Context (Modifier Applicability)

**Surface**: `crates/perl-parser-core/src/engine/parser/statements.rs:parse_statement_modifier()`

**Hazard**: Not all statements can take `when`/`default` modifiers in Perl. For example, `when (cond) { ... }` itself cannot be a statement with a modifier: `when (1) {} when $x;` is invalid.

**Invariant**: `when` and `default` modifiers apply to normal statements, not to control structures that already consume `when`/`default` as keywords (like `given`, `when` blocks, `default` blocks).

**Mitigation**:
1. `parse_statement_modifier()` is called AFTER a statement is parsed
2. Parser should never attempt to apply modifier to incomplete/control-flow statements
3. Test: ensure `given (1) { when (2) { print "ok" } when 3; }` parses correctly (outer form is block, inner modifier)

**Test obligation**: `test_modifier_on_control_structures()` — verify no double-modifier on same statement

---

### PARSER-6: Whitespace & Formatting (AST Position Tracking)

**Surface**: `crates/perl-parser-core/src/engine/parser/control_flow.rs` and `statements.rs` location tracking

**Hazard**: `StatementModifier` node's `SourceLocation` must accurately span from statement start to condition end. Multiline statements (`print\n    "ok"\nwhen $x;`) must track correct positions for LSP diagnostics and hover.

**Invariant**: `location.start` points to statement start, `location.end` points to modifier condition end.

**Mitigation**:
1. Verify `parse_statement_modifier()` sets location correctly: `start = statement.location.start`, `end = condition.location.end`
2. Test with multiline modifier: ensure error reporting points to correct line

**Test obligation**: `test_multiline_modifier_location()` — verify location tracking is accurate

---

## §Contracts

### PARSER_CONTRACTS.md Sections Affected

**Control Flow Constructs**:
- §Given/When/Default Blocks: Defines `When` and `Default` as block-level constructs inside `given`. This change extends the contract: `when` and `default` are ALSO valid as statement modifiers (outside or at statement level within given block context).

**Statement Modifiers**:
- §Statement Modifiers: Documents postfix conditional/loop forms (`if`, `unless`, `while`, `until`, `for`, `foreach`). Must be updated to include `when` and `default`.

### LSP Protocol Impact
- **Hover/Definition**: No change; statement modifiers are already handled
- **Diagnostics**: Parser errors will change from "Expected 'when' or 'default' in given block" to valid parse for modifier form
- **Completion**: No new completions; `when` and `default` already suggested after statements

### Internal Parser Contracts
- `is_stmt_modifier_kind()` must return true for `TokenKind::When` and `TokenKind::Default`
- `parse_statement_modifier()` must handle arbitrary modifier names without special-casing

---

## §API-Shape

### New AST Nodes
None. Reuses existing `NodeKind::StatementModifier { statement, modifier, condition }`.

### Modified AST Nodes
`NodeKind::StatementModifier` — no field changes, but modifier string now accepts "when" and "default".

### New Functions
None required; refactoring consolidates logic.

**Modified functions**:
- `parse_given_block()` — adds branching logic to detect block vs modifier form
- `is_stmt_modifier_kind()` — adds `TokenKind::Default` to matches

### Caller Count & Scope
- `is_stmt_modifier_kind()` — called from `parse_statement_inner()` (line 183), `parse_expression_statement()` (line 481), and `finish_expression_from()` (line 507). Adding a variant does not break callers.
- `parse_given_block()` — called from `parse_given_statement()` (line 717). Internal refactoring; no impact on callers.

### Dup-Risk Grep
```bash
grep -n "when.*modifier\|default.*modifier" crates/perl-parser-core/src/engine/parser/*.rs
grep -n "is_stmt_modifier_kind" crates/perl-parser-core/src/engine/parser/*.rs
grep -n "parse_given_block\|parse_when_statement\|parse_default_statement" crates/perl-parser-core/src/engine/parser/*.rs
```
Expected: Only existing occurrences; no hidden dependencies.

---

## §Test-Grid

### Positive Test Cases (should pass)

| Test Name | Input | Expected S-exp Fragment | Invariant |
|-----------|-------|------------------------|-----------|
| `test_modifier_when_simple` | `print "ok" when $cond;` | `statement_modifier_when` | Modifier recognized; no ERROR node |
| `test_modifier_when_function` | `do_x() when $cond;` | `statement_modifier_when` | Function call as statement |
| `test_modifier_when_assignment` | `$x = 1 when $cond;` | `statement_modifier_when` | Assignment as statement |
| `test_modifier_when_in_given` | `given (5) { print "ok" when $_; }` | `statement_modifier_when` | Modifier in given block (CRITICAL) |
| `test_modifier_default` | `print "default" default;` | `statement_modifier_default` | Default keyword recognized |
| `test_modifier_default_in_given` | `given (5) { print "default" default; }` | `statement_modifier_default` | Default in given block (CRITICAL) |
| `test_when_block_unchanged` | `when ($cond) { print "ok"; }` | `when` (node, not modifier) | Block form not broken |
| `test_default_block_unchanged` | `default { print "ok"; }` | `default` (node, not modifier) | Block form not broken |
| `test_mixed_forms_in_given` | `given (5) { when (1) { print "1"; } print "default" default; }` | Both `when` and `statement_modifier_default` | Block and modifier forms coexist |

### Negative Test Cases (should error or reject)

| Test Name | Input | Expected Behavior | Invariant |
|-----------|-------|-------------------|-----------|
| `test_given_requires_when_default` | `given (1) { print "ok"; }` | ERROR: "Expected 'when' or 'default' in given block" | Non-modifier statements still rejected |
| `test_modifier_on_invalid_statement` | `sub foo() { print "ok" when $x; } when $y;` | Parse subroutine OK; modifier on orphaned `when` is error | Modifier doesn't apply to control keywords |

### Adversarial Test Cases (edge cases, error conditions)

| Test Name | Input | Expected Behavior | Invariant |
|-----------|-------|-------------------|-----------|
| `test_multiline_when_modifier` | `print\n    "ok"\nwhen $cond;` | Parses; location tracks correctly | Whitespace/newlines don't break parser |
| `test_nested_modifier_when_if` | `print "ok" when $a if $b;` | Parses with correct precedence | Both modifiers apply (if supported) |
| `test_when_modifier_with_complex_condition` | `print "ok" when $x > 0 && $y < 10;` | Parses correctly | Complex expression in condition |
| `test_given_block_error_recovery` | `given (1) { invalid syntax here when 1; print "ok"; }` | Error on invalid; recovers to parse subsequent when | Error doesn't corrupt block |
| `test_typo_whe_vs_when` | `print "ok" whe $x;` | ERROR: unrecognized bareword | Typos caught |

### State-Transition Test Cases (parser state consistency)

| Test Name | Input Sequence | Expected | Invariant |
|-----------|----------------|----------|-----------|
| `test_given_block_state_after_error` | `given (1) { when (1) { print "ok"; } bad_when_syntax when 2; }` | Error node; subsequent valid `when` parses | Parser state recovers within block |
| `test_statement_modifier_twice` | `$x = 1 when $a when $b;` | First modifier parsed; second is error or part of condition | No double-modifier-on-same-statement |

---

## §Blast-Radius

### Consumers

| Consumer | Impact | Must-test |
|----------|--------|-----------|
| `perl-lsp-rs` (hover, diagnostics) | Reduced parse errors in given blocks; error messages improve | Run full LSP test suite on given block code |
| `perl-semantic-analyzer` | May see `StatementModifier` nodes where before were ERRORs; must handle gracefully | Verify semantic analysis doesn't crash on when/default modifiers |
| `perl-test-corpus` | `statement_modifier_comprehensive.pl` now parses; may affect test snapshots | Re-run snapshots; check for false negatives |
| `perl-workspace` (symbol indexing) | Symbol references inside when/default modifiers now indexed (instead of lost in ERROR) | Verify references are captured correctly |

### Downstream Crates

**Direct dependents** (import from `perl-parser-core`):
- `perl-parser` — uses Parser; test suite
- `perl-lsp-rs` — consumes AST; must verify on real code

**Indirect dependents**:
- `perl-semantic-analyzer` — analyzes AST; must handle new `StatementModifier` variants
- `perl-workspace` — traverses AST for symbols; must handle modifiers

### Must-Not-Touch Boundary

- **LSP protocol**: No changes; modifiers are internal to AST
- **Token definitions**: No new tokens; `TokenKind::When` and `TokenKind::Default` already exist
- **AST NodeKind enum**: No new variants; reuses `StatementModifier`
- **Public parser API**: `Parser::new()`, `parse()` unchanged

### Test Regression Risk

- **Existing snapshots**: `statement_modifier_tests.rs` snapshots should not change (same s-exp format)
- **Given block tests**: Previously ERROR nodes may now parse correctly; snapshots will change (expected)
- **CPAN corpus**: If any corpus code uses `when`/`default` modifiers, previously failed parses will now succeed

---

## Summary

This fix removes a false parse error for valid Perl 5.10+ syntax (`when` and `default` statement modifiers), specifically inside `given` blocks. The parser already recognizes these as modifiers in standalone contexts (top-level statements), but rejects them inside given blocks due to overly strict parsing logic.

**Scope**: Confined to `perl-parser-core` (helpers.rs, control_flow.rs); no API changes, no new types.

**Risk**: Low. Parser refactoring is localized. AST reuses existing node types. Error recovery must be tested.

**Benefit**: Enables parsing of modern Perl code using statement modifiers in given blocks, improving LSP diagnostics and symbol indexing coverage.
