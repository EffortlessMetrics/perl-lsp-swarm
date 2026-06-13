# Implementation Checklist: Issue #1356 — `when` and `default` Statement Modifiers

## Overview
The parser currently rejects `when` and `default` used as statement modifiers (postfix form), such as:
- `print "matched\n" when $cond;` (simple statement modifier)
- `print "matched\n" when $_ == 5;` inside a given block (statement modifier in given context)
- `print "default\n" default;` (default as statement modifier)

The parser treats `when`/`default` only as block-level constructs inside `given` statements.

## Root Cause
1. `is_stmt_modifier_kind()` in `helpers.rs` already includes `TokenKind::When` but NOT `TokenKind::Default`
2. The `parse_given_block()` function (line 727 in `control_flow.rs`) **strictly requires** `when (cond) { ... }` or `default { ... }` block syntax
3. It does NOT allow statements with `when`/`default` modifiers (e.g., `stmt when cond;`)

## Fix Strategy
Two changes are required:

### Change 1: Add `when` and `default` to statement modifier keywords list
**File**: `crates/perl-parser-core/src/engine/parser/helpers.rs` (line 8-19)
**Current code**:
```rust
fn is_stmt_modifier_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::If
            | TokenKind::Unless
            | TokenKind::While
            | TokenKind::Until
            | TokenKind::For
            | TokenKind::When      // <-- Already present!
            | TokenKind::Foreach
    )
}
```
**Action**: Add `TokenKind::Default` to the matches list (after `TokenKind::When`)

### Change 2: Refactor `parse_given_block()` to allow statements with modifiers
**File**: `crates/perl-parser-core/src/engine/parser/control_flow.rs` (line 727-754)
**Current behavior**: Strictly expects `when (cond) {...}` or `default {...}` at statement level
**New behavior**: Allow either:
  - Block form: `when (cond) { ... }` → parse as `NodeKind::When { condition, body }`
  - Modifier form: `stmt when cond;` → parse as `StatementModifier { statement, modifier: "when", condition }`
  - Same for `default`

**Implementation strategy**:
1. Parse a statement normally (allowing arbitrary code)
2. Check if the next token is `when` or `default`
3. If yes and modifier matches statement modifier rules → create `StatementModifier` node
4. If no `when`/`default` → error (no standalone statements allowed in given blocks)

---

## Ordered Implementation Steps

### Step 1: Add `TokenKind::Default` to statement modifier list
**File**: `crates/perl-parser-core/src/engine/parser/helpers.rs`
**Lines**: 8-19 (the `is_stmt_modifier_kind` function)
**Action**: Change the `matches!` pattern to include `TokenKind::Default`
**Verification**: `cargo clippy -p perl-parser-core --lib`

### Step 2: Refactor `parse_given_block()` to support statement modifiers
**File**: `crates/perl-parser-core/src/engine/parser/control_flow.rs`
**Lines**: 727-754
**Current logic**:
```rust
fn parse_given_block(&mut self) -> ParseResult<Node> {
    let start = self.current_position();
    self.expect(TokenKind::LeftBrace)?;
    let mut statements = Vec::new();
    while self.peek_kind() != Some(TokenKind::RightBrace) && !self.tokens.is_eof() {
        match self.peek_kind() {
            Some(TokenKind::When) => {
                statements.push(self.parse_when_statement()?);  // Block form only
            }
            Some(TokenKind::Default) => {
                statements.push(self.parse_default_statement()?);  // Block form only
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected 'when' or 'default' in given block",
                    self.current_position(),
                ));
            }
        }
    }
    // ...
}
```

**New logic**:
1. Lookahead: if peek is `when` or `default`, attempt to parse as block form first
2. If block form fails (no opening paren), try statement form with modifier
3. If neither works, error

Pseudo-code:
```rust
while self.peek_kind() != Some(TokenKind::RightBrace) && !self.tokens.is_eof() {
    // Try to parse statement with modifier
    // First, check if this is a when/default block or statement
    match self.peek_kind() {
        Some(TokenKind::When) => {
            if self.is_when_block_form() {
                statements.push(self.parse_when_statement()?);
            } else {
                statements.push(self.parse_statement_with_when_modifier()?);
            }
        }
        Some(TokenKind::Default) => {
            if self.is_default_block_form() {
                statements.push(self.parse_default_statement()?);
            } else {
                statements.push(self.parse_statement_with_default_modifier()?);
            }
        }
        _ => {
            return Err(ParseError::syntax(
                "Expected 'when' or 'default' in given block",
                self.current_position(),
            ));
        }
    }
}
```

**Helper functions** to check for block form:
- `is_when_block_form()`: peek is `when`, then check second token is `(`
- `is_default_block_form()`: peek is `default`, then check second token is `{`

**New functions** for modifier form:
- May reuse existing `parse_statement_modifier()` logic, OR
- Create lightweight helpers to consume the statement and apply modifier

**Verification**: `cargo test -p perl-parser-core statement_modifier_tests`

### Step 3: Write and run red tests
**File**: `crates/perl-parser-core/src/engine/parser/statement_modifier_tests.rs` (or new test file)
**Tests**:
1. `test_modifier_when_simple()` — `print "matched\n" when $condition;`
2. `test_modifier_when_with_function_call()` — `do_x() when $cond;`
3. `test_modifier_when_with_assignment()` — `$x = 1 when $cond;`
4. `test_modifier_when_in_given_block()` — `given (5) { print "matched\n" when $_ == 5; }`
5. `test_modifier_default()` — `print "default\n" default;`
6. `test_modifier_default_in_given_block()` — `given ($x) { print "default\n" default; }`

**Verification**: All six tests should pass after Step 2

### Step 4: Verify AST representation
**Verification**: AST contains correct `StatementModifier { statement, modifier: "when"/"default", condition }` nodes
**Check**: Run test from test_corpus: `cargo test -p perl-parser` and verify `statement_modifier_comprehensive.pl` parses without errors

### Step 5: Format and lint
**Command**: `cargo xtask fmt`
**Command**: `cargo clippy -p perl-parser-core --lib`

---

## Test Coverage

### Positive tests (should parse)
- Statement modifier after simple print: `print "ok" when $cond;`
- Statement modifier after function call: `do_x() when $cond;`
- Statement modifier after assignment: `$x = 1 when $cond;`
- Statement modifier inside given block: `given ($var) { print "ok" when $_; }`
- Default modifier: `print "default" default;`
- Default modifier in given block: `given ($var) { print "default" default; }`
- Nested modifiers (when + if): `print "ok" when $a if $b;` (when is primary, if is nested)

### Negative tests (should still error or be unsupported)
- Bareword statement without modifier inside given block (already errors)
- Other statements inside given block without modifier (should error)

### AST shape verification
- `StatementModifier { statement: Node, modifier: String, condition: Node }`
- Ensure `modifier` field contains "when" or "default"

---

## Dependencies & Compilation Order

1. **No struct changes needed** — `StatementModifier` already supports arbitrary modifier names
2. **No token changes needed** — `TokenKind::When` and `TokenKind::Default` already exist
3. **Only parser logic changes** required

Compilation should succeed at each step because:
- Step 1 adds a variant to an existing match (no type changes)
- Step 2 refactors control flow (no API changes)
- Steps 3-5 are testing and formatting

---

## Files Modified

| File | Lines | Change Type |
|------|-------|------------|
| `crates/perl-parser-core/src/engine/parser/helpers.rs` | 8-19 | Add `TokenKind::Default` to `is_stmt_modifier_kind()` |
| `crates/perl-parser-core/src/engine/parser/control_flow.rs` | 727-754 | Refactor `parse_given_block()` to support statement modifiers |
| `crates/perl-parser-core/src/engine/parser/statement_modifier_tests.rs` | (end) | Add tests for when/default modifiers |

---

## Verify Commands

```bash
# After Step 1
cargo clippy -p perl-parser-core --lib

# After Step 2
cargo test -p perl-parser-core statement_modifier_tests

# Final verification
cargo test -p perl-parser
cargo xtask fmt
cargo clippy -p perl-parser-core
```
