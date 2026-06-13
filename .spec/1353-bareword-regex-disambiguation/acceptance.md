# Acceptance: Issue #1353 — Bareword/Regex Disambiguation

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| `sub my_regex_builder; my_regex_builder /foo\|bar/;` | Bareword followed by `/` where bareword was declared as `sub` earlier in file | `/` is lexed as `RegexMatch` token, parser produces `(regex ...)` node, not `(binary_/ ...)` |
| `sub builder; builder /test/;` | Custom function call with regex argument | `/` is regex delimiter |
| `use constant FOO; FOO /pattern/;` | Constant declared with `use constant` | `/` is regex delimiter (constants are term-introducing like builtins) |
| `my_unknown /foo/;` | Unknown bareword (not declared, not builtin) | `/` is lexed as `Division` (preserves current safe-default behavior) |
| `my $x = 10; $x / 2;` | Division after variable | `/` is lexed as `Division` (existing behavior, must not regress) |
| `print /foo/;` | Builtin function call with regex | `/` is regex delimiter (existing behavior, must not regress) |
| `sub builder; builder /test/ or print "fail";` | Subroutine call with regex in boolean context | Regex recognized; method call chain: `(or (call builder (regex ...)) ...)` |

## §Hazards

### PARSER-1: Recovery from misclassified operators
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `crates/perl-parser-core/src/engine/parser/mod.rs:Parser::new()` | If bareword/regex case fails, previous misparse causes cascading errors (division treats RHS as operand, not term) | Symbol table must be populated BEFORE lexing so mode is correct at token time, not after parse |
| Cascading division errors | Parser enters error recovery on `my_unknown /foo/` (expected `Value`, got regex) when lexer defaults to `ExpectOperator` | Graceful degradation: unknown barewords stay `ExpectOperator` (safe); symbol table lifts known functions to `ExpectTerm` |

### PARSER-2: Symbol table population order
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `crates/perl-parser-core/src/engine/parser_context.rs:ParserContext::new()` | Symbol table built during parse (after lex) is too late; lexer already set mode | Pre-pass or streaming: scan for `sub` declarations BEFORE lexing, store in `LocalSymbolTable`, pass to lexer config |
| Late sub declarations | Forward references: `my_regex_builder /foo/; sub my_regex_builder { ... }` case (sub declared after use) | Document as out-of-scope: pre-pass only looks ahead to EOF for declarations; Perl also requires `use strict 'subs'` to enforce forward ref behavior. Accept limitation in v1. |

### PARSER-3: Lexer mode state consistency
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `crates/perl-lexer/src/lexer/mod.rs:PerlLexer` | If symbol table lookup in mode-setting branch is wrong, bareword is classified incorrectly | Unit-test symbol table lookup in isolation; verify bareword maps to sub/constant before mode change |

### PARSER-4: Regression on unknown barewords
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `crates/perl-lexer/src/lib.rs:1936–1947` | Changing bareword mode logic breaks existing tests that assume unknown bareword → `ExpectOperator` | Preserve exact semantics: only known subs/constants get `ExpectTerm`, unknown stays `ExpectOperator`. Run full test suite before merge. |

### PROCESS-1: Module-level forward references (out of scope v1)
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `use constant`-based regex builders in different modules | Symbol table limited to current file; imports not tracked | Document in context.md as follow-up: cross-module symbol table requires workspace-level tracking (future work, #XXXX). |

### PROCESS-2: Dynamic sub declarations (limitation)
| Surface | Risk | Mitigation |
|---------|------|-----------|
| `eval`-injected subs, `AUTOLOAD`, symbol table mutation at runtime | Symbol table is static, built at parse time | Acceptable: Perl tooling (LSP servers, linters) also use static analysis. Document in context.md. |

## §Contracts

### PARSER_CONTRACTS.md alignment

| Contract | Section | Status | Impact |
|----------|---------|--------|--------|
| **Lexer mode correctness** | Division vs. Regex | Modified | Must maintain: after known function bareword → `ExpectTerm` so `/` is regex. After unknown → `ExpectOperator` (division). Tests verify no regressions. |
| **Indirect object calls** | Indirect calls like `print $fh @list` | Preserved | Symbol table does not affect indirect calls; mode logic untouched for those paths. Verify existing tests pass. |
| **Builtin detection** | Builtins as term-introducers | Preserved | Existing `is_builtin_function()` path unchanged; new subroutine path supplements. |
| **Error recovery** | Parser gracefully degrades on unknown constructs | Preserved | Unknown barewords → safe default (`ExpectOperator`). If misparse occurs, error recovery unchanged. |

### LSP/DAP contracts

| Protocol | Impact | Notes |
|----------|--------|-------|
| LSP semantic tokens | No change | Lexer token type determines semantic token (regex vs operator). Fixing lexer fixes downstream tokens automatically. |
| LSP hover/goto-definition | Enhanced | Once regex is correct, hover over `/foo/` yields regex context, not division context. |

## §API-Shape

### New public types

**`crates/perl-lexer/src/symbol_table.rs`** (NEW FILE)

```rust
/// Local symbol table for tracking subs and constants in a source file
#[derive(Debug, Clone)]
pub struct LocalSymbolTable {
    /// Set of known subroutine names
    pub known_subs: std::collections::HashSet<String>,
    /// Set of known constant names (from `use constant`)
    pub known_constants: std::collections::HashSet<String>,
}

impl LocalSymbolTable {
    /// Create a new empty symbol table
    pub fn new() -> Self { ... }
    
    /// Check if an identifier is a known subroutine
    pub fn is_known_sub(&self, name: &str) -> bool { ... }
    
    /// Check if an identifier is a known constant
    pub fn is_known_constant(&self, name: &str) -> bool { ... }
    
    /// Scan source code and populate symbol table with sub declarations
    pub fn scan_subs(source: &str) -> Result<Self, ScanError> { ... }
}
```

### Modified types

**`crates/perl-lexer/src/config.rs:LexerConfig`**

```rust
#[derive(Debug, Clone)]
pub struct LexerConfig {
    pub parse_interpolation: bool,
    pub track_positions: bool,
    pub max_lookahead: usize,
    // NEW:
    pub symbol_table: Option<Arc<LocalSymbolTable>>,  // pass symbol info to lexer
}
```

**`crates/perl-lexer/src/lib.rs:PerlLexer`**

```rust
// Existing; no signature change. symbol_table accessed via self.config.symbol_table
```

### Modified functions

**`crates/perl-lexer/src/lib.rs` lines 1936–1947** (bareword mode logic)

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

**`crates/perl-parser-core/src/engine/parser_context.rs:ParserContext::new()`** (lines 63–102)

```rust
// BEFORE:
pub fn new(source: String) -> Self {
    let mut tokens = VecDeque::new();
    let position_tracker = PositionTracker::new(source.clone());
    let mut lexer = perl_lexer::PerlLexer::new(&source);
    
    // ... token collection loop ...
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
    
    let mut lexer = perl_lexer::PerlLexer::with_config(&source, config);
    
    // ... token collection loop ...
}
```

### Dup-risk analysis (grep before implementation)

```bash
grep -rn "LexerConfig {" crates/         # All LexerConfig instantiations must add symbol_table field
grep -rn "LexerMode::ExpectOperator"    # Verify no other bareword paths affected
grep -rn "is_builtin_function"          # Ensure builtins still work (no regression)
```

**Caller count:**
- `LexerConfig::default()` used in tests (~15 places). Must add `symbol_table: None` in default impl or leave as-is.
- `PerlLexer::new()` used in tests (~8 places). No signature change; works with default config.
- `PerlLexer::with_config()` used in 1 place (`ParserContext::new()`). Will be updated.

## §Test-Grid

### Positive cases (symbol table resolves bareword → regex)

| Test Name | Input | Assertion | Pass Criteria |
|-----------|-------|-----------|---------------|
| `test_bareword_sub_with_regex` | `sub builder; builder /foo/;` | AST contains `(regex ...)` not `(binary_/ ...)` | Lexer sets `ExpectTerm` after `builder` token |
| `test_builtin_preserves_regex` | `print /foo/;` (existing builtin) | AST contains `(regex ...)` | No regression; builtin path unchanged |
| `test_constant_with_regex` | `use constant FOO; FOO /x/;` | AST contains `(regex ...)` | Constants tracked in symbol table |
| `test_multiple_subs_with_regex` | `sub a; sub b; a /1/; b /2/;` | Both `/` are regex delimiters | Symbol table handles multiple declarations |

### Negative cases (unknown bareword → division, preserves safe default)

| Test Name | Input | Assertion | Pass Criteria |
|-----------|-------|-----------|---------------|
| `test_unknown_bareword_division` | `my_unknown /foo/;` | `/` is lexed as `Division` (not regex) | Unknown barewords stay `ExpectOperator` |
| `test_division_after_paren` | `($x) / 2;` | `/` is lexed as `Division` | Existing regression tests pass |
| `test_division_after_number` | `10 / 3;` | `/` is lexed as `Division` | Existing regression tests pass |

### State transition cases (symbol table correctness)

| Test Name | Input | Assertion | Pass Criteria |
|-----------|-------|-----------|---------------|
| `test_symbol_table_empty_file` | No declarations, `builder /foo/;` | `/` is Division (unknown bareword) | Scan returns empty table |
| `test_symbol_table_forward_ref` | `builder /foo/; sub builder { ... }` (sub after use) | `/` is Division (limitation: forward refs not supported) | Document as acceptable v1 limitation |
| `test_symbol_table_scope_limitation` | Imported sub from module: `use MyModule; MyModule::builder /foo/;` | `/` is Division (module subs not in local table) | Document as follow-up (cross-module tracking) |

### Adversarial cases (symbol table robustness)

| Test Name | Input | Assertion | Pass Criteria |
|-----------|-------|-----------|---------------|
| `test_symbol_table_malformed_sub_declaration` | `sub my_sub ( invalid syntax ) { }` | Scan gracefully degrades; symbol table populated with `my_sub` despite bad body | Pre-pass is regex-based, does not parse; captures name even if declaration is later invalid |
| `test_symbol_table_sub_in_comment` | `# sub fake; sub real; real /x/;` | Only `real` in table (comment skipped) | Scan ignores comment blocks |
| `test_symbol_table_sub_in_string` | `"sub fake"; sub real; real /x/;` | Only `real` in table (string ignored) | Scan is conservative; only captures top-level subs |

## §Blast-Radius

### Consumers (what calls lexer/parser)

| Consumer | Impact | Risk | Mitigation |
|----------|--------|------|-----------|
| `crates/perl-parser/src/lib.rs:Parser` | Parses source code via `ParserContext::new()`. Will now auto-populate symbol table. | Low: change is transparent; no API breakage | Verify `Parser::new()` still works on all test cases. |
| `crates/perl-lsp-rs/src/` | LSP server uses parser to provide diagnostics, hover, goto-def. Symbol table fix improves regex detection in IDE. | Low: fixes bugs, no regressions | Run LSP integration tests; hover/goto-def should no longer misidentify regex as division. |
| `crates/perl-dap/src/` | DAP uses parser for breakpoint handling. No direct symbol table dependency. | None: DAP not affected | No changes needed. |
| `crates/perl-workspace/src/` | Workspace indexing uses parser. No direct symbol table dependency. | Low: improved parse accuracy benefits symbol indexing | Verify workspace symbol resolution still works. |

### Downstream crates

| Crate | Dependency | Risk | Notes |
|-------|-----------|------|-------|
| `perl-lsp` (LSP server) | Depends on `perl-parser-core` | Low | Parser improvements transparently improve LSP. |
| `perl-dap` (DAP server) | Depends on `perl-parser-core` | Low | No changes to DAP surface. |
| `perl-semantic-analyzer` | Depends on `perl-parser-core` | Low | Semantic analysis benefits from improved regex detection. |

### Boundary / must-not-touch

| Boundary | Current Behavior | Must Preserve |
|----------|-----------------|--------------|
| **Builtin function detection** (`crates/perl-lexer/src/lexer/helpers/word_classification.rs:is_builtin_function()`) | Returns true for `print`, `join`, etc. → `ExpectTerm` | Must not change. Existing tests validate. |
| **Error recovery** (`crates/perl-parser-core/src/syntax/error/recovery.rs`) | Parser gracefully skips over misparsed expressions. | Must not change. Symbol table is optional; if absent, behavior identical to today. |
| **Heredoc handling** (`crates/perl-lexer/src/heredoc.rs`) | Mode handling for heredocs is separate. | Must not affect heredoc mode transitions. Verify in tests. |
| **Quote-like operators** (`crates/perl-lexer/src/quote_handler.rs`) | `q//`, `s///`, etc. have own mode logic. | Must not affect. Symbol table only applies to bareword + `/` case. |

### Behavioral contract

| Aspect | Today | After Fix | Test Coverage |
|--------|-------|-----------|----------------|
| **Unknown bareword before `/`** | `/` is division | `/` is division (same) | `test_unknown_bareword_division` |
| **Known builtin before `/`** | `/` is regex (existing) | `/` is regex (same) | `test_builtin_preserves_regex` (no regression) |
| **Known sub declaration before `/`** | `/` is division (bug) | `/` is regex (fixed) | `test_bareword_sub_with_regex` |
| **Division operator** | `/` is division | `/` is division (same) | Existing division tests pass |
| **Regex match operator** | `/` is regex (after keyword/operator) | `/` is regex (same) | Existing regex tests pass |

