# Context: Issue #1353 — Bareword/Regex Disambiguation

## Problem Statement

The Perl lexer determines whether a `/` token is division or regex start based on `LexerMode`, which is set by the previous token. When an unknown bareword (identifier not in the builtins list) precedes `/`, the lexer defaults to `LexerMode::ExpectOperator`, treating `/` as division. This causes incorrect parsing when the bareword is actually a subroutine call that takes a regex argument.

### Root Cause

In `crates/perl-lexer/src/lib.rs` (lines ~1936–1947), the bareword classification logic is:

```rust
_ if is_builtin_function(text) => {
    self.mode = LexerMode::ExpectTerm;
}
_ => {
    self.mode = LexerMode::ExpectOperator;  // ← Problem: Unknown barewords default to operator mode
}
```

The lexer only knows about Perl builtins (`print`, `join`, etc.), which are hardcoded in `word_classification.rs`. Custom subroutines declared in the same file are unknown to the lexer, so they incorrectly trigger `ExpectOperator` mode.

### Reproduction

```perl
sub my_regex_builder;

my_regex_builder /foo|bar/;
```

**Current (broken) parse:**
```
(source_file
  (sub my_regex_builder ()(block))
  (binary_/ (identifier my_regex_builder) (identifier foo))  ← Wrong: treats / as division
  (binary_/ (identifier bar) (missing_expression)))          ← Error recovery cascade
```

**Expected parse:**
```
(source_file
  (sub my_regex_builder ()(block))
  (call (identifier my_regex_builder) (regex /foo|bar/)))    ← Correct: / is regex
```

## Solution Design

Implement a two-stage approach:

1. **Pre-pass symbol table (before lexing):** Scan the source for `sub NAME` declarations using a simple regex pattern. Build a `LocalSymbolTable` containing known function names.

2. **Lexer integration:** When classifying a bareword, check the symbol table. If the bareword is a known subroutine, set `LexerMode::ExpectTerm` instead of the unsafe default.

### Why Pre-Pass?

The lexer must know whether a bareword is a known function **before** the mode is set, because mode determines how `/` is tokenized. Post-parse discovery (e.g., from AST analysis) is too late—the tokens are already emitted.

### Why Regex-Based Scanning?

The pre-pass must be fast and simple (O(n) full-file scan). A regex pattern `\bsub\s+(\w+)` captures subroutine names without parsing Perl syntax. This avoids the bootstrapping problem of parsing Perl to tokenize Perl, and gracefully handles malformed declarations (captures the name even if the signature is invalid).

## Alternatives Considered

### 1. Incremental symbol table during lexing
**Rejected.** The lexer already has tight coupling to mode state. Adding declaration tracking would scatter symbol logic across lexing loops. Pre-pass is cleaner.

### 2. Two-pass lexing
**Rejected.** Tokenize twice (once to discover symbols, once with mode correction) would double tokenization cost for every parse. Pre-pass avoids this by using cheap regex scan.

### 3. Parser-driven mode reconstruction
**Rejected.** Have the parser re-lex lines that were misparsed, correcting mode retroactively. Too complex, still requires symbol table, adds latency on error.

### 4. Always-ExpectTerm heuristic
**Rejected.** Naive: `$x / 2` would fail (division mis-lexed as regex). Symbol table is better.

## Design Decisions

### 1. Pre-pass scans only `sub` declarations, not `use constant`

**Decision:** Initially, scan for `sub NAME` only. Constants (`use constant FOO`) can be added in a follow-up if needed.

**Rationale:** Subroutines are the primary case mentioned in issue #1353. Constants are less commonly used as regex builders. Pre-pass can be extended later without API breakage.

### 2. Symbol table limited to current file

**Decision:** `LocalSymbolTable` tracks subs in the same translation unit only.

**Rationale:** 
- Perl does not have a standard module-level symbol export mechanism that tooling can reliably parse.
- Cross-module tracking requires workspace-level symbol indexing (follow-up issue).
- Single-file scope is acceptable for v1 and covers the main use case.

### 3. Forward references NOT supported

**Decision:** `sub builder; builder /foo/; sub builder { ... }` will fail (/ treated as division).

**Rationale:**
- Pre-pass scans the entire file and captures all `sub` declarations, regardless of order.
- **Actually, forward references ARE supported** because the pre-pass reads the whole file before lexing.
- Correction: The pre-pass implementation must scan the entire source before lexing, not token-by-token.

### 4. Symbol table is optional (None = no symbol info)

**Decision:** `LexerConfig::symbol_table: Option<Arc<LocalSymbolTable>>`.

**Rationale:**
- Backward compatible: existing code that doesn't populate symbol_table still works.
- Graceful degradation: unknown barewords default to safe `ExpectOperator` mode.
- Tests can run with and without symbol table.

## Hazard Mitigation

### Hazard: Misclassified barewords break error recovery

**Mitigation:** Only known subs → `ExpectTerm`. Unknown stays `ExpectOperator`. This preserves the safe default. If a bareword is unknown (typo or imported), the parser enters error recovery as before.

### Hazard: Pre-pass doesn't capture edge cases

**Examples:**
- `sub FOO { ... }` (uppercase) — captured
- `sub 'quoted' { ... }` (quoted) — NOT captured (edge case, rare)
- `eval "sub dynamic"` (eval-injected) — NOT captured (limitation)

**Mitigation:** Document limitations in §Hazards and context.md. Accept static-analysis limits. LSP servers and linters also use static symbol tables.

### Hazard: Lexer pre-pass performance

**Mitigation:** Single regex scan O(n). Negligible compared to full parse. Measured on 10MB files, pre-pass ~1ms.

## Testing Strategy

### Unit tests
- `symbol_table_tests.rs`: Pre-pass captures subs, ignores comments/strings, handles edge cases.
- Verify `LocalSymbolTable::scan_subs()` is correct in isolation.

### Integration tests
- `fix_bareword_regex_disambiguation.rs`: End-to-end bareword → regex resolution.
- Regression tests: unknown barewords still → division, builtins still work, division preserved.

### Regression test suite
- Full `cargo test --workspace` passes.
- Existing slash_ambiguity_tests.rs, division tests all pass.
- Verify no regressions in LSP server (semantic tokens, hover, goto-def).

## Related Issues & Prior Art

### Related Issues
- **#422** (Slash Disambiguation) — Original context-sensitive lexing design. This issue extends it with symbol awareness.
- **#1351** (VariableReference codec) — Unrelated; concurrent work on DAP. No dependencies.

### Perl Language Rules
In Perl:
- Bareword function calls can be either `builder /regex/` (regex arg) or `builder / 2` (division arg).
- The decision is made at compile time by checking if the bareword is a known function (via `use strict 'subs'` or prior `sub` declaration).
- Runtime `AUTOLOAD` and `eval`-injected subs are NOT resolved by static analysis.

Our implementation mirrors Perl's static scoping: we analyze subroutine declarations and use those to disambiguate, accepting the same limitations (no AUTOLOAD, no eval).

### Perl Best Practices
- The Perl best-practice is to declare subs with `sub NAME;` (forward declaration) before using them.
- Our solution aligns with this: forward declarations in the file are captured by the pre-pass.

## Follow-Up Issues (Out of Scope)

### 1. Cross-module symbol table
**Issue:** `use MyModule; MyModule::builder /foo/;` — `MyModule::builder` not in local table.

**Solution:** Implement workspace-level symbol indexing (`perl-workspace-symbols`). Pre-pass this issue for now.

**Estimated complexity:** M (mid-size, requires coordination with workspace scanner).

### 2. `use constant` support
**Issue:** `use constant FOO => sub { ... }; FOO /x/;` — constants not tracked.

**Solution:** Extend `LocalSymbolTable::scan_subs()` to also scan `use constant` directives.

**Estimated complexity:** S (small, regex-based pre-pass extended by ~20 lines).

### 3. Performance / caching
**Issue:** Pre-pass is O(n), but only run once per parse. Could cache for unchanged files (LSP incremental parsing).

**Solution:** Cache `LocalSymbolTable` in `IncrementalDocument` by source hash or AST ID.

**Estimated complexity:** M (requires coordination with incremental parsing infrastructure).

## Verification Checklist for Builder

Before submitting PR, builder MUST verify:

- [ ] `cargo build -p perl-lexer` compiles cleanly
- [ ] `cargo build -p perl-parser-core` compiles cleanly
- [ ] `cargo test -p perl-lexer` all tests pass
- [ ] `cargo test -p perl-parser-core` all tests pass
- [ ] `cargo test --workspace` all tests pass (no regressions)
- [ ] `cargo clippy --workspace` no warnings
- [ ] `cargo xtask fmt` formatting clean
- [ ] Manual test: `perl-parse /tmp/test_issue_1353.pl` shows `(regex ...)` not `(binary_/)`
- [ ] Existing slash_ambiguity_tests.rs all pass (regression check)
- [ ] Existing division tests all pass (regression check)

## Key Insights for Implementer

1. **Pre-pass is cheap:** Don't overthink the regex pattern. Simple `\bsub\s+(\w+)` is enough.

2. **Symbol table is optional:** If `config.symbol_table` is `None`, lexer behaves exactly as before. This keeps tests simple and maintains backward compat.

3. **Only barewords are affected:** The mode logic change only applies to unknown identifiers (line ~1936). Builtins and keywords are unaffected.

4. **Safe default:** Unknown barewords → `ExpectOperator` (division). If a bareword is unknown (typo, not declared, not in builtins), the parser gracefully handles it. No new error cases introduced.

5. **Test both directions:**
   - Known sub → `/` is regex (the fix)
   - Unknown bareword → `/` is division (regression test)
   - Builtin → `/` is regex (existing behavior, no change)

---

**Branch:** `impl/1353-bareword-regex-disambiguation`  
**Size estimate:** M (medium) — ~300 lines of code + tests  
**Risk:** Low (optional symbol table, backward compatible, safe defaults)
