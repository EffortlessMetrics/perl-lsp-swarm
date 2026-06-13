# Acceptance Criteria: #1372 — Parser Panic on User Input

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| Ambiguous regex delimiter (`m#pattern#`) | Uncommented test parses Perl code with alternative regex delimiters | Parser returns valid AST with Regex node (no panic) |
| Quote-like operators with various delimiters (`q{...}`, `q[...]`, `q\|...\|`) | Code uses alternative delimiters in q/qq/qx/qw | Parser returns valid AST with String nodes (no panic) |
| Heredocs with multiple styles (`<<EOF`, `<<'EOF'`, `<<"EOF"`, `<<\`EOF\``) | Code uses different heredoc delimiters and quote modes | Parser returns valid AST with Heredoc nodes (no panic) |
| Unterminated/incomplete structures (unclosed paren, missing terminator) | Code has syntactic errors that previously triggered panic | Parser returns Error recovery node + continues parsing (no panic) |
| Deep nesting + complex expressions | Code with deeply nested data structures, operators, method chains | Parser completes without stack overflow or panic |
| Ambiguous bareword vs function call | Code like `my $x = ambiguous_function;` (could be bareword or function) | Parser returns valid AST with FunctionCall node; ambiguity resolved (no panic) |
| Mixed valid + invalid syntax in same block | Code with some valid statements and some malformed | Parser returns AST with both valid nodes and Error nodes; recovery succeeds (no panic) |

---

## §Hazards

### PARSER-1: Panic on Invalid/Unexpected Input
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Unwrap/expect on None or out-of-bounds in regex/delimiter parsing | `crates/perl-parser-core/src/` (regex module likely) or `crates/perl-parser/src/` (quote-like handler) | Input: `m#pattern#`, `q{...}`, or unterminated string | Replace with `.get()` / `.get_mut()` + Result/.is_none() checks; return Error node on failure |
| Array index panic on byte slicing or token position calculation | `crates/perl-lexer/src/` or `crates/perl-parser-core/src/` (position tracking) | Input: UTF-8 boundary conditions, truncated input, mixed encodings | Use checked arithmetic, `.chars().skip()`, bounds checks before slicing |
| Stack overflow on deeply nested structures | Recursive descent parser in `crates/perl-parser-core/src/` | Input: 1000+ levels of nesting (`{{{{...}}}}`) | Implement depth limit with graceful error; return partial AST instead of stack panic |

### PARSER-2: Panic-on-Input (Process Stability)
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Rust panic in parser crashes LSP server during user editing | Parser library entry point (Parser::new / Parser::parse) | User types ambiguous code in editor; LSP parses live buffer | All parser code must use Result/Option, no unwrap/panic; tests verify graceful degradation |
| Panic in AST node construction or traversal | Any NodeKind constructor or for_each_child iteration | Malformed node state or recovery node constraints violated | Document AST invariants in PARSER_CONTRACTS.md; test recovery node validity |

### PARSER-3: Bounds Violation (Index-Out-of-Bounds)
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Byte index beyond source length during error recovery | `crates/perl-parser-core/src/` position tracking or location assignment | Input: truncated input, slicing UTF-8 sequences incorrectly | Clamp indices: `pos.min(source.len())`, use checked math, test with short inputs |
| String slice on non-UTF8-boundary after error recovery | Recovery node location assignment from parser state | Error in multi-byte UTF-8 character handling during panic recovery | Use `chars()` instead of byte indices where possible; document UTF-8 assumptions |

### PARSER-4: Recovery Node Correctness
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Error nodes placed at wrong location span, breaking downstream consumers | Parser error recovery in quote-like, regex, heredoc modules | Error in source location tracking during recovery | Write test asserting Error node.location is valid (within source bounds) |
| Incomplete child nodes in Error recovery node breaking AST invariants | Error node construction with partial or corrupted children | Missing required children or type mismatches in error variant | Document Error node schema in PARSER_CONTRACTS.md; assert in tests |
| Downstream code panics on Error node kind that violates invariant | LSP/DAP code pattern-matching on NodeKind::Error | Parser produces error node with unexpected structure | Ensure Error nodes have minimal, well-defined schema; update PARSER_CONTRACTS |

### CROSS-SUBSYSTEM: Fuzz Regression (Test Coverage)
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Fix breaks existing parser corpus or snapshot tests | Any parser module changed for panic fix | Behavioral change in AST construction affects existing test snapshots | Run `cargo test -p perl-parser` on full corpus; update insta snapshots if necessary |
| Regression on edge-case inputs previously handled | Recovery logic in quote-like, regex, heredoc parsing | Change breaks handling of valid (though unusual) Perl syntax | Green-TDD adds adversarial tests around panic site; fuzz-bounded sweep before merge |

### CROSS-SUBSYSTEM: Process Stability (Runtime)
| Hazard | Surface | Trigger | Mitigation |
|--------|---------|---------|-----------|
| Parser change introduces new panic path not covered by tests | Any parser module with new error handling | Untested code path triggered by specific input in production | Test coverage for all error paths; PR includes edge-case test assertions |
| Performance regression from bounds checking or error recovery overhead | Parser main loop, hot path for quote-like operators | Large file or deep nesting causes slowdown from new checks | Benchmark before/after; if >10% regression, optimize or defer to follow-up |

---

## §Contracts

**Parser Behavioral Contracts** (from `docs/reference/PARSER_CONTRACTS.md`):

| Contract | Obligation | Applicable To |
|----------|-----------|---------------|
| **Recovery-Node Placement** | Error nodes must be placed at source location of error; must not create negative/invalid spans | All error handling in parser (quote-like, regex, heredoc, recovery logic) |
| **Quote-Like Delimiters** | Parser must support alternative delimiters (`q{...}`, `q[...]`, `q\|...\|`, etc.) without panic | `crates/perl-parser-core/src/quote.rs` or equivalent quote-like handler |
| **Regex Delimiters** | Regex operators (`m#...#`, `m!...!`, etc.) and substitution/transliteration with alternative delimiters must parse | `crates/perl-parser-core/src/` regex module |
| **Heredoc Variants** | Single-quoted, double-quoted, backtick, and indented heredocs must parse; unterminated must degrade gracefully | `crates/perl-parser-core/src/` heredoc handler |
| **Graceful Degradation** | Parser must never panic on arbitrary input; malformed syntax must produce Error nodes or partial AST, not crash | Core invariant across all parser modules |
| **Nested Structure Limits** | Parser must handle nesting up to practical limits (e.g., 1000 levels); beyond limit, return error not stack panic | Recursive descent parser limit handling |

**LSP Protocol Contracts** (impact scope):

| Aspect | Obligation | Impact |
|--------|-----------|--------|
| **Semantic Analysis Input** | Downstream semantic analyzer (`crates/perl-semantic-analyzer/`) must accept AST with Error nodes without panic | Error nodes must be valid AST shape |
| **Diagnostic Generation** | LSP diagnostics provider must handle Error nodes and generate appropriate diagnostics (no null pointer deref) | Error node structure must be documented |

**DAP Protocol Contracts** (impact scope):

| Aspect | Obligation | Impact |
|--------|-----------|--------|
| **Program Execution** | DAP debugger must not crash if debugging Perl file with syntax errors (parser produces Error nodes) | Parser robustness translates to debugger stability |

---

## §API-Shape

### New/Modified Signatures

| What | Change | Impact |
|------|--------|--------|
| Parser error recovery in regex/quote-like modules | Introduce new helper methods (if needed) for bounds-safe string slicing, position tracking, error node construction | Internal to parser; no public API change expected |
| Error node variants | May expand `NodeKind::Error` enum if recovery requires additional context (unlikely) | If expanded, update PARSER_CONTRACTS.md; downstream code via pattern match needs review |
| Parser state machine in quote-like / regex handler | Refactor to avoid panicking index/slice operations | Internal refactoring; no signature change |

### ID-Spaces / Constants

| What | Change | Impact |
|------|--------|--------|
| Error node location span | Ensure always valid (0..source.len()); add assertion in tests | No API change; internal invariant tightening |

### Dup-Risk Grep

**High-risk patterns** (to search after fix):
```bash
grep -r "unwrap()\|expect()\|panic!()" crates/perl-parser*/src/ --include="*.rs"
```

Expected after fix: ZERO hits in parser code (production). Hits only in tests allowed (with `#[allow(...)]`).

**Caller count impact**:

| Surface | Current Callers | Risk |
|---------|-----------------|------|
| `Parser::new()` / `Parser::parse()` | ~50+ (tests, LSP providers, DAP) | HIGH: if parser panics, all downstream code crashes. Fix reduces risk to zero. |
| Error node construction | Semantic analyzer, LSP providers | MEDIUM: if error node shape changes, downstream pattern matches need review. Covered by tests. |

---

## §Test-Grid

### Positive Tests (Parser Handles Valid Perl)

| Input Class | Test Name | Invariant |
|-------------|-----------|-----------|
| Regex with hash delimiter | `test_regex_hash_delimiter` | AST contains Regex node; no panic |
| Quote-like with braces | `test_quote_like_braces` | AST contains String node; no panic |
| Heredoc quoted variant | `test_heredoc_double_quoted` | AST contains Heredoc node; no panic |
| Ambiguous bareword (valid in Perl) | `test_ambiguous_bareword_valid` | AST contains FunctionCall or Bareword node; no panic |
| Nested structures (valid depth) | `test_nested_valid_depth` | AST complete; no panic |

### Negative Tests (Parser Handles Malformed Perl)

| Input Class | Test Name | Invariant |
|-------------|-----------|-----------|
| Unterminated regex | `test_unterminated_regex` | AST contains Error node (not panic); recovery continues |
| Unclosed quote | `test_unclosed_quote` | AST contains Error node; recovery continues |
| Unterminated heredoc | `test_unterminated_heredoc` | AST contains Error node; recovery continues |
| Truncated input | `test_truncated_input` | AST contains Error node; location span valid |
| Mixed valid + invalid | `test_mixed_valid_invalid` | AST contains both valid nodes and Error nodes; no panic |

### Adversarial Tests (Boundary/Stress Cases)

| Input Class | Test Name | Invariant |
|-------------|-----------|-----------|
| UTF-8 boundary conditions | `test_utf8_boundaries` | Parser does not panic on multi-byte slicing; Error node if needed |
| Deep nesting (1000 levels) | `test_deep_nesting_limit` | Parser returns Error node beyond limit, not stack panic |
| Ambiguous delimiters in heredoc | `test_heredoc_quoted_with_unquoted_end` | Parser recovers gracefully; no panic |
| Empty input / whitespace only | `test_empty_input` | Parser returns minimal AST; no panic |
| Single-char input | `test_single_char_input` | Parser handles gracefully; no panic |
| Regex with embedded newline | `test_regex_embedded_newline` | Parser handles or returns Error; no panic |
| Quote-like with unbalanced nesting | `test_quote_unbalanced_nesting` | Parser returns Error node; no panic |

### State Transition Tests (Parser State After Error)

| Transition | Test Name | Invariant |
|-----------|-----------|-----------|
| After Error node, parser continues on valid statement | `test_parser_recovery_after_error` | Parser produces Error node + subsequent valid node in same AST |
| Parser state preserved for nested error recovery | `test_nested_error_recovery` | Multiple errors in nested structure; all recovered with valid AST |
| Position tracking correct after error recovery | `test_position_tracking_post_error` | Next node location is correct (no skipped bytes) |

---

## §Blast-Radius

### Consumers / Downstream Crates

| Crate | Impact | Mitigation |
|-------|--------|-----------|
| `crates/perl-semantic-analyzer/` | Consumes AST from parser; must handle Error nodes | Test semantic analyzer accepts Error nodes; no crashes |
| `crates/perl-lsp-rs/` | Uses parser for live-file analysis; panics crash LSP server | Parser panic → LSP crash (P0 risk). Fix eliminates this path. |
| `crates/perl-dap/` | Uses parser for debugging; panics crash debugger | Parser panic → DAP crash (P0 risk). Fix eliminates this path. |
| `crates/perl-workspace/` | Uses parser for symbol indexing; panics skip file | Parser panic → workspace index failures. Fix ensures index completion. |
| `crates/perl-lsp-providers/` | Diagnostic/hint/completion providers consume AST | Error nodes must be handleable; test coverage required |

### Must-Not-Touch Boundary

| Boundary | Reason |
|----------|--------|
| `crates/perl-parser/` public API (Parser::new, Parser::parse signature) | No changes to public interface; fix is internal to parser implementation |
| LSP protocol (`lsp-types`, language-server-protocol) | No LSP spec changes; parser robustness is transparent |
| DAP protocol (`debug-adapter-protocol`) | No DAP spec changes; parser robustness is transparent |
| Snapshot test expectations (in most cases) | Parser behavior may change (graceful error instead of panic); snapshots updated only if necessary |

### Hidden Dependencies / Assumptions

| Assumption | Risk | Validation |
|-----------|------|-----------|
| Parser state is not corrupted after error recovery | HIGH | Red-TDD + green-TDD tests verify AST validity post-error |
| Downstream code pattern-matches on NodeKind::Error correctly | MEDIUM | Semantic analyzer tests verify no panics on Error nodes |
| Parser location spans are always valid (0..source.len()) | HIGH | Add assertion in error node construction; test with boundary inputs |
| Quote-like and regex delimiters are deterministic | LOW | Existing tests verify delimiter parsing; fix preserves behavior |

---

## §Coverage-Map

### Coverage Obligation

| Surface | Obligation | Test Coverage |
|---------|-----------|----------------|
| Regex alternative delimiters (`m#...#`, `m!...!`, etc.) | Parser must not panic; must produce Regex node | `test_regex_hash_delimiter`, `test_regex_bang_delimiter`, etc. (parametrized) |
| Quote-like alternative delimiters (`q{...}`, `q[...]`, etc.) | Parser must not panic; must produce String node | `test_quote_like_*` (parametrized per delimiter) |
| Heredoc variants | Parser must not panic; must produce Heredoc node | `test_heredoc_quoted`, `test_heredoc_indented`, `test_heredoc_backtick` |
| Unterminated/incomplete constructs | Parser must return Error node, not panic | `test_unterminated_*`, `test_unclosed_*` |
| UTF-8 boundaries | Parser must handle or error gracefully, not panic | `test_utf8_boundaries` |
| Deep nesting | Parser must limit recursion, return Error beyond limit, not panic | `test_deep_nesting_limit` |
| Parser recovery post-error | Subsequent valid syntax must parse correctly | `test_parser_recovery_after_error` |
| LSP / DAP integration | Downstream code must not crash on parser output | Semantic analyzer tests accept Error nodes |

---

## Summary

**What must be true after this issue closes:**

1. **Test `test_incomplete_ambiguous_syntax()` is uncommented and PASSES** (no panic)
2. **All parser code is panic-free** (no unwrap/expect/panic on user input)
3. **Error recovery nodes are produced** for malformed Perl syntax instead of crashes
4. **Downstream LSP/DAP code is hardened** against parser returning Error nodes
5. **Snapshot tests are updated** if parser behavior changed (graceful error instead of panic)
6. **Full test suite passes** (no regressions in corpus, edge cases, or integration)
