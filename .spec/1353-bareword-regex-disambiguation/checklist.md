# Implementation Checklist: Issue #1353 — Bareword/Regex Disambiguation

## Overview
Add a pre-lexing symbol table that tracks `sub` declarations in a file, allowing the lexer to correctly treat bareword function names as term-introducers when followed by `/`, so the `/` is lexed as regex instead of division.

## Phase 1: Symbol Table Infrastructure

### Step 1: Create symbol_table.rs module
**File:** `crates/perl-lexer/src/symbol_table.rs`

**What to add:**
- `#[derive(Debug, Clone)] pub struct LocalSymbolTable { pub known_subs: HashSet<String>, pub known_constants: HashSet<String> }`
- `impl LocalSymbolTable { pub fn new() -> Self { ... } }`
- `pub fn is_known_sub(&self, name: &str) -> bool { self.known_subs.contains(name) }`
- `pub fn is_known_constant(&self, name: &str) -> bool { self.known_constants.contains(name) }`
- `pub fn scan_subs(source: &str) -> Result<Self, ScanError> { ... }` — regex-based pre-pass to find `sub NAME` declarations
  - Regex pattern: `\bsub\s+(\w+)` (case-insensitive, respects line boundaries)
  - Skip content inside `"..."`, `'...'`, `q/...`, and `#...` comments
  - Return `Result<Self, ScanError>` where `ScanError` is a simple error type (no context needed, just fail gracefully)

**Verify command:** `cargo clippy -p perl-lexer --lib` (no errors on new module)

**Dependencies:** None. Only uses stdlib `HashSet`, `regex::Regex` (already in cargo.lock for perl-lexer).

### Step 2: Add symbol_table module to lib.rs
**File:** `crates/perl-lexer/src/lib.rs`

**What to add:**
- At top of file (after other `mod` declarations): `mod symbol_table; pub use symbol_table::LocalSymbolTable;`

**Verify command:** `cargo build -p perl-lexer` (compiles)

### Step 3: Add symbol_table field to LexerConfig
**File:** `crates/perl-lexer/src/config.rs`

**What to change:**
```rust
// BEFORE:
#[derive(Debug, Clone)]
pub struct LexerConfig {
    pub parse_interpolation: bool,
    pub track_positions: bool,
    pub max_lookahead: usize,
}

// AFTER:
#[derive(Debug, Clone)]
pub struct LexerConfig {
    pub parse_interpolation: bool,
    pub track_positions: bool,
    pub max_lookahead: usize,
    pub symbol_table: Option<std::sync::Arc<LocalSymbolTable>>,
}
```

**Update Default impl:**
```rust
impl Default for LexerConfig {
    fn default() -> Self {
        Self {
            parse_interpolation: true,
            track_positions: true,
            max_lookahead: 1024,
            symbol_table: None,  // NEW
        }
    }
}
```

**Imports to add:** `use std::sync::Arc;` at top of file (if not already present). Add `use crate::LocalSymbolTable;`

**Verify command:** `cargo build -p perl-lexer` (compiles)

### Step 4: Update bareword mode logic in lexer
**File:** `crates/perl-lexer/src/lib.rs` — lines 1936–1947 (approx)

**What to change:**
```rust
// BEFORE:
_ => {
    self.mode = LexerMode::ExpectOperator;
}

// AFTER:
_ => {
    // Check if bareword is a known subroutine or constant (term-introducing)
    if let Some(sym_table) = &self.config.symbol_table {
        if sym_table.is_known_sub(text) || sym_table.is_known_constant(text) {
            self.mode = LexerMode::ExpectTerm;
        } else {
            self.mode = LexerMode::ExpectOperator;
        }
    } else {
        self.mode = LexerMode::ExpectOperator;
    }
}
```

**Exact location:** Find line with `_ => { self.mode = LexerMode::ExpectOperator; }` in the bareword handling block (around line 1936 in `try_word()` method). This is the `else` clause for non-keywords.

**Verify command:** `cargo build -p perl-lexer` (compiles)

## Phase 2: Parser Integration

### Step 5: Update ParserContext to populate symbol table
**File:** `crates/perl-parser-core/src/engine/parser_context.rs` — lines 63–102

**What to change:**
```rust
// BEFORE:
pub fn new(source: String) -> Self {
    let mut tokens = VecDeque::new();
    let position_tracker = PositionTracker::new(source.clone());

    // Tokenize the source using mode-aware lexer
    let mut lexer = perl_lexer::PerlLexer::new(&source);
    loop {
        match lexer.next_token() {
            // ... token collection ...
        }
    }
    // ... ParserContext construction ...
}

// AFTER:
pub fn new(source: String) -> Self {
    let mut tokens = VecDeque::new();
    let position_tracker = PositionTracker::new(source.clone());

    // Pre-pass: scan for subroutine and constant declarations
    let symbol_table = perl_lexer::LocalSymbolTable::scan_subs(&source)
        .unwrap_or_else(|_| perl_lexer::LocalSymbolTable::new());
    
    let config = perl_lexer::LexerConfig {
        parse_interpolation: true,
        track_positions: true,
        max_lookahead: 1024,
        symbol_table: Some(Arc::new(symbol_table)),
    };

    // Tokenize the source using mode-aware lexer with symbol table
    let mut lexer = perl_lexer::PerlLexer::with_config(&source, config);
    loop {
        match lexer.next_token() {
            // ... token collection (unchanged) ...
        }
    }
    // ... ParserContext construction ...
}
```

**Imports to add:** `use std::sync::Arc;` at top of file (if not already present)

**Verify command:** `cargo build -p perl-parser-core` (compiles)

## Phase 3: Testing

### Step 6: Add tests for symbol table scanning
**File:** `crates/perl-lexer/tests/symbol_table_tests.rs` (NEW FILE)

**What to add:**
```rust
mod symbol_table_tests {
    use perl_lexer::LocalSymbolTable;

    #[test]
    fn test_symbol_table_scans_single_sub() {
        let source = "sub my_func;\nmy_func /foo/;";
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(table.is_known_sub("my_func"));
    }

    #[test]
    fn test_symbol_table_scans_multiple_subs() {
        let source = "sub a; sub b; sub c;";
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(table.is_known_sub("a"));
        assert!(table.is_known_sub("b"));
        assert!(table.is_known_sub("c"));
    }

    #[test]
    fn test_symbol_table_ignores_sub_in_comment() {
        let source = "# sub fake;\nsub real;";
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(!table.is_known_sub("fake"));
        assert!(table.is_known_sub("real"));
    }

    #[test]
    fn test_symbol_table_ignores_sub_in_string() {
        let source = r#""sub fake";\nsub real;"#;
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(!table.is_known_sub("fake"));
        assert!(table.is_known_sub("real"));
    }

    #[test]
    fn test_symbol_table_empty_file() {
        let source = "print 'hello';";
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(table.known_subs.is_empty());
    }

    #[test]
    fn test_symbol_table_handles_malformed_sub() {
        // Scan is regex-based, captures name even if body is invalid
        let source = "sub bad ( invalid ) { }; sub good;";
        let table = LocalSymbolTable::scan_subs(source).expect("scan failed");
        assert!(table.is_known_sub("bad"));
        assert!(table.is_known_sub("good"));
    }
}
```

**Verify command:** `cargo test -p perl-lexer -- symbol_table` (all tests pass)

### Step 7: Add parser/lexer integration tests
**File:** `crates/perl-parser-core/tests/fix_bareword_regex_disambiguation.rs` (NEW FILE)

**What to add:**
```rust
mod fix_bareword_regex_disambiguation {
    use perl_parser_core::parser::Parser;
    use perl_tdd_support::must;

    #[test]
    fn test_bareword_sub_with_regex() {
        let code = "sub my_regex_builder;\nmy_regex_builder /foo|bar/;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        
        // Should parse regex, not division
        assert!(sexp.contains("regex"), "Should parse /foo|bar/ as regex, got: {}", sexp);
        assert!(!sexp.contains("binary_/"), "Should not parse as division, got: {}", sexp);
    }

    #[test]
    fn test_multiple_subs_with_regex() {
        let code = "sub a; sub b;\na /1/;\nb /2/;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        
        assert!(sexp.matches("regex").count() >= 2, "Should have at least 2 regex patterns");
        assert!(!sexp.contains("binary_/"), "Should not parse as division");
    }

    #[test]
    fn test_unknown_bareword_still_division() {
        // Regression test: unknown bareword should still treat / as division
        let code = "my_unknown /foo/;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        
        // Unknown bareword → ExpectOperator → / is division (safe default)
        assert!(sexp.contains("binary_/"), "Unknown bareword should treat / as division");
    }

    #[test]
    fn test_builtin_regex_preserved() {
        // Regression test: builtins should still work
        let code = "print /foo/;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        
        assert!(sexp.contains("regex"), "Builtin print should still parse /foo/ as regex");
    }

    #[test]
    fn test_division_after_variable_preserved() {
        // Regression test: division should still work
        let code = "$x / 2;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        
        assert!(sexp.contains("binary_/"), "Variable should treat / as division");
    }
}
```

**Verify command:** `cargo test -p perl-parser-core -- fix_bareword_regex_disambiguation` (all tests pass)

### Step 8: Regression test suite
**Command:** `cargo test --workspace` (all tests in all crates pass)

**Specific checks:**
```bash
cargo test -p perl-lexer
cargo test -p perl-parser-core
cargo test -p perl-parser
cargo test -p perl-lsp-rs --lib
```

## Phase 4: Verification

### Step 9: Verify the reproduction case
**Command (manual):**
```bash
cat > /tmp/test_issue_1353.pl << 'EOF'
sub my_regex_builder;
my_regex_builder /foo|bar/;
EOF

cargo run --bin perl-parse --features cli -- /tmp/test_issue_1353.pl
```

**Expected output:** Should contain `(regex ...)` not `(binary_/ ...)`. The S-expression should have a proper regex node.

### Step 10: Code review checks
**Before committing:**

```bash
# Verify no clippy warnings
cargo clippy --workspace

# Verify formatting
cargo xtask fmt

# Verify tests pass
cargo test --workspace --lib
```

## Phase 5: Documentation & Cleanup

### Step 11: Document limitations in context.md
- Forward references not supported (sub declared after use)
- Cross-module subs not tracked (workspace-level symbol table is follow-up)
- Dynamic subs from eval/AUTOLOAD not supported (static analysis limitation)

### Step 12: Final checks before commit

```bash
# Full test suite
cargo test --workspace

# Lint + format
cargo xtask fmt
cargo clippy --workspace

# Verify no uncommitted changes except .spec files
git status
```

## Summary of Changes

| File | Operation | Lines | Summary |
|------|-----------|-------|---------|
| `crates/perl-lexer/src/symbol_table.rs` | CREATE | ~100 | New `LocalSymbolTable` struct with `scan_subs()` pre-pass |
| `crates/perl-lexer/src/lib.rs` | MODIFY | +5 | Add `mod symbol_table; pub use ...` |
| `crates/perl-lexer/src/config.rs` | MODIFY | +3 | Add `symbol_table: Option<Arc<LocalSymbolTable>>` field to `LexerConfig` |
| `crates/perl-lexer/src/lib.rs` | MODIFY | +10 | Update bareword mode logic (lines ~1936–1947) to check symbol table |
| `crates/perl-parser-core/src/engine/parser_context.rs` | MODIFY | +8 | Populate symbol table before lexing in `ParserContext::new()` |
| `crates/perl-lexer/tests/symbol_table_tests.rs` | CREATE | ~80 | Unit tests for symbol table scanning |
| `crates/perl-parser-core/tests/fix_bareword_regex_disambiguation.rs` | CREATE | ~70 | Integration tests for bareword/regex fix |

**Total lines added:** ~276 (code + tests)
**Total lines modified:** ~28
**Risk:** Low (symbol table is optional, unknown barewords preserve existing behavior)
