# Implementation Checklist: #1854 — add recursion depth guard to parse_unary

## Change order (compiles at each step)

### Step 1: Wrap parse_unary entry point with recursion guard
- **File:** `crates/perl-parser-core/src/engine/parser/expressions/unary.rs`
- **Change:** Wrap the `parse_unary` function body with `with_recursion_guard`
- **Details:** Transform:
  ```rust
  fn parse_unary(&mut self) -> ParseResult<Node> {
      // existing code
  }
  ```
  To:
  ```rust
  fn parse_unary(&mut self) -> ParseResult<Node> {
      self.with_recursion_guard(|s| s.parse_unary_inner())
  }
  ```
  And rename current implementation to `parse_unary_inner(&mut self) -> ParseResult<Node>`.
- **Verify:** `cargo check -p perl-parser-core`

### Step 2: Update internal recursive calls to parse_unary_inner
- **File:** `crates/perl-parser-core/src/engine/parser/expressions/unary.rs`
- **Change:** Replace all internal recursive calls to `self.parse_unary()` with `self.parse_unary_inner()` within the unary.rs file (lines 100, 174, 210, 253, 281, 509, 528, 557)
- **Details:** These calls are within the same function (now parse_unary_inner), so they should call the inner function directly to avoid re-entering the guard on each recursion step
- **Verify:** `cargo check -p perl-parser-core`

### Step 3: Write integration test for recursion depth protection
- **File:** `crates/perl-parser-core/tests/parse_unary_recursion_guard.rs` (CREATE)
- **Change:** Add test that verifies parse_unary rejects deeply nested unary expressions
- **Details:** Create a Perl expression with deeply nested unary operators (e.g., `!!!...!!!$x` with 200+ negations) and verify it returns NestingTooDeep error
- **Verify:** `cargo test -p perl-parser-core --test parse_unary_recursion_guard`

### Step 4: Verify recursive calls from other files still work
- **File:** `crates/perl-parser-core/src/engine/parser/expressions/postfix.rs` (verify only)
- **Change:** No change needed - calls to `self.parse_unary()` from other modules continue to use the guarded entry point
- **Verify:** `cargo check -p perl-parser-core`

### Step 5: Final verification
- **Verify:** 
  ```bash
  cargo test -p perl-parser-core
  cargo xtask fmt
  cargo clippy -p perl-parser-core
  ```

## Callers and consumers

- `parse_unary()` is called from: 
  - `crates/perl-parser-core/src/engine/parser/expressions/postfix.rs` (line 100)
  - `crates/perl-parser-core/src/engine/parser/expressions/precedence.rs` (lines 821, 1020)
  - `crates/perl-parser-core/src/engine/parser/expressions/unary.rs` (internally, lines 100, 174, 210, 253, 281, 509, 528, 557)

## Scope boundary

Files IN scope:
- `crates/perl-parser-core/src/engine/parser/expressions/unary.rs` (primary change)
- `crates/perl-parser-core/tests/parse_unary_recursion_guard.rs` (new test file)

Files OUT of scope:
- Other parser files (postfix.rs, precedence.rs, etc.) — they call parse_unary but do not need modification
- Other crates (perl-lsp-rs, perl-dap, etc.)

## Flags for builder

- The guarded entry point (`parse_unary`) must be a thin wrapper that immediately calls `with_recursion_guard`, while the heavy lifting moves to `parse_unary_inner`
- Internal recursive calls within unary.rs must call `parse_unary_inner` directly (not `parse_unary`) to avoid guard re-entry per recursion step
- External callers (from postfix.rs and precedence.rs) continue to call `parse_unary()` — they implicitly use the guard on entry
- The recursion depth is tracked per top-level call to `parse_unary()`, not per internal recursive step
