# Context: Issue #1850 — Semantic Tokens Multiline Token Length Fix

## Problem Statement

Semantic tokens that span multiple lines are currently emitted with `length = 0`. According to the LSP semantic tokens specification (LSP 3.17+), multiline tokens should have their length set to the number of UTF-16 code units from the token start to the end of the **starting line**, not 0.

This causes LSP clients (VSCode, Emacs, Vim, etc.) to incorrectly render syntax highlighting for tokens that cross line boundaries, such as:
- Heredoc strings (`<<SQL ... END`)
- Multiline interpolated strings
- Method declarations or package declarations split across lines
- SQL keyword matches inside heredoc bodies spanning lines

### Current Behavior
```rust
let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
```

When `sl != el` (start line differs from end line), length is hardcoded to 0. This violates LSP spec.

### Expected Behavior
```rust
let eol_col = get_eol_col(text, sl);  // Get EOL column on start line
let len = if sl == el { 
    ec.saturating_sub(sc)  // Single-line: normal length
} else { 
    eol_col.saturating_sub(sc)  // Multiline: chars from start to EOL
};
```

## Root Cause

The fix was likely deferred during initial implementation because:
1. Most Perl code fits within single lines; multiline tokens are edge cases.
2. UTF-16 position calculation is non-trivial (multi-byte chars, emoji, tabs).
3. LSP spec requires understanding of incremental encoding rules.

The pattern appears in 16 locations in `semantic_tokens.rs`, suggesting copy-paste from an early version that never addressed the multiline case.

## Decisions

### Decision 1: Add `get_eol_col` helper function
**Choice:** Create a private helper `fn get_eol_col(text: &str, line_idx: u32) -> u32` to abstract EOL column computation.

**Rationale:** 
- Avoids repeating the UTF-16 counting logic across 16 callsites.
- Centralizes the potential performance optimization (caching line offsets).
- Makes code more readable and maintainable.

**Alternative considered:** Inline the logic at each site.
- **Rejected:** Prone to bugs; harder to review; impossible to optimize globally.

### Decision 2: Use `char.len_utf16()` for UTF-16 unit counting
**Choice:** Count UTF-16 code units by iterating chars and calling `char.len_utf16()`.

**Rationale:**
- Rust's `std::char::len_utf16()` handles emoji, surrogates, and multi-byte chars correctly.
- LSP spec mandates UTF-16 positions (not bytes, not char count).
- Simple, correct, avoids manual surrogate pair logic.

**Alternative considered:** Use byte offsets directly.
- **Rejected:** LSP clients expect UTF-16, not byte offsets; this would break all multiline highlighting.

### Decision 3: Fix all 16 token-length sites uniformly
**Choice:** Apply the same `if sl == el` branch pattern to all locations (lexer tokens, AST tokens, SQL/JSON injection, etc.).

**Rationale:**
- Consistency; all token types should follow the same rule.
- Avoids subtle bugs where one path fixes the issue but others don't.
- Single semantic model across the module.

**Alternative considered:** Fix only the most common multiline case.
- **Rejected:** Incomplete; other paths (heredoc injection, method declarations) would still be broken.

### Decision 4: No breaking API changes
**Choice:** Keep `collect_semantic_tokens` signature and return type unchanged.

**Rationale:**
- Existing callers require no code changes.
- Token output format (`Vec<EncodedToken>`) is stable.
- This is a bug fix (correctness improvement), not a feature.

**Alternative considered:** Restructure token collection API.
- **Rejected:** Unnecessary churn; internal improvement is sufficient.

## Alternatives Rejected

### Alternative A: Set multiline token length to full span end
**Proposal:** For multiline tokens, set `len = ec.saturating_sub(sc)` (same as single-line).

**Why rejected:** LSP spec explicitly forbids this. Per [LSP §7.17.1](https://microsoft.github.io/language-server-protocol/specifications/specification-3-17-0/#textDocument_semanticTokens):
> "The length of the token in UTF-16 character units... For each line, the token length must refer only to the characters on that line."

Using the full span (multiple lines) as the length is protocol-noncompliant.

### Alternative B: Emit separate tokens for each line of a multiline token
**Proposal:** Split `<<SQL ... END` into three tokens: one per line.

**Why rejected:**
- Loses semantic information (which tokens form a single heredoc).
- Requires AST restructuring.
- Harder to implement and test.
- Current approach (single token with correct length) is simpler and matches LSP semantics.

### Alternative C: Cache all line EOL positions upfront
**Proposal:** Build a `Vec<u32>` of EOL columns before the main loop.

**Why rejected:**
- Micro-optimization; not needed for typical Perl files (<10k tokens).
- Adds complexity; can be deferred if profiling shows need.
- Simple `get_eol_col` with lazy computation is sufficient.

(Can be done in a future perf pass if needed.)

## Prior Art / References

### LSP SemanticTokens Specification
- **Source:** [LSP 3.17.0 Specification](https://microsoft.github.io/language-server-protocol/specifications/specification-3-17-0/#textDocument_semanticTokens)
- **Clause:** §7.17.1 — "semanticTokens/full request":
  > "The semantic tokens are returned as a flat array. The client will decode it using the semantic token legend..."
  > "Each token is [deltaLine, deltaStartChar, length, tokenType, tokenModifierSet]"
  > "length is the number of UTF-16 code units for this token."
  > "For each line after the first, tokens must be ordered by column."

### Perl Lexer Integration
- **Source:** `crates/perl-lexer/src/lib.rs`
- **Relevance:** Token boundaries come from lexer; `PerlLexer::next_token()` returns `Token { start, end, ... }` as byte offsets. The `to_pos16` callback converts to LSP positions.

### UTF-16 Encoding in Rust
- **Source:** [Rust std::char::len_utf16 docs](https://doc.rust-lang.org/std/primitive.char.html#method.len_utf16)
- **Relevance:** Emoji (U+1F600 "😀") encodes as 2 UTF-16 units (surrogate pair). ASCII and most Latin chars are 1 unit. Custom counting is error-prone; `char.len_utf16()` is canonical.

### Perl AST and Token Spans
- **Source:** `crates/perl-parser-core/src/ast.rs` — `Node { location: Span { start, end }, ... }`
- **Relevance:** Token positions are **byte offsets** into the source text. Conversion to LSP (line, col) happens via `to_pos16` callback.

### LSP Capability Contract
- **Source:** `docs/reference/LSP_CAPABILITY_CONTRACT.md` (if exists) or implicit in `features.toml`
- **Relevance:** perl-lsp advertises `semanticTokensProvider` capability; clients expect LSP-compliant responses.

## Testing Strategy

### Unit Tests
1. **`test_eol_col_utf16_boundaries`** — `get_eol_col` with ASCII, multi-byte, emoji.
2. **`test_eol_col_emoji_surrogates`** — Verify emoji counted as 2 UTF-16 units.
3. **`test_eol_col_tab_character`** — Tab counted as 1 UTF-16 unit (visual width irrelevant).
4. **`test_eol_col_empty_line`** — Empty line returns 0.

### Integration Tests
1. **`test_collect_semantic_tokens_multiline_heredoc`** — Full Perl code with heredoc spanning 3+ lines.
2. **`test_collect_semantic_tokens_multiline_variable`** — Interpolated string with variable across lines.
3. **`test_collect_semantic_tokens_multiline_method`** — Method declaration split across lines.
4. **`test_eol_col_per_token_line`** — Multiple tokens on different lines; verify each gets correct `eol_col`.

### Regression Tests
1. **`test_collect_semantic_tokens_sql_single_line`** — Single-line SQL keyword still works.
2. Existing test suite must pass unchanged (no breaking changes to API).

## Known Issues & Caveats

### Performance (Deferred)
Computing `text.lines().nth()` for each of 16 token sites is O(n) per token in worst case. For a 10k-token file, this could be slow. **Mitigation:** Deferred to future perf pass; profile first. Simple caching of line offsets is a straightforward optimization if needed.

### Multi-byte characters and grapheme clusters
The fix assumes `char.len_utf16()` is sufficient. This is true for UTF-16 code units but **not** for grapheme clusters (e.g., emoji + zero-width joiner = 1 visual unit but multiple code units). LSP spec uses code units, so this is correct. However, editors may have column width issues. **Mitigation:** Document assumption; LSP spec defines the boundary (code units).

### Empty files, missing lines
If `line_idx >= text.lines().count()`, `get_eol_col` returns 0. This is safe (saturating arithmetic). **Mitigation:** Tested in `test_eol_col_empty_line`.

## Follow-up Work

1. **Performance profiling:** If 10k+ token files show slowdown, implement line-offset caching.
2. **Multi-line token UI:** Consider whether VSCode/Emacs rendering of multiline tokens is correct; may need client-side fixes.
3. **Heredoc indentation:** Verify indented heredocs (`<<~SQL`) still work correctly.
4. **Interpolated strings:** Test complex cases like `"$var{key}[0]"` across lines.

## Links

- **Issue:** #1850 (this issue)
- **Related issues:** None identified yet; check for heredoc or multiline tokenization bugs.
- **LSP Spec:** https://microsoft.github.io/language-server-protocol/specifications/specification-3-17-0/#textDocument_semanticTokens
- **Perl Lexer:** `crates/perl-lexer/src/lib.rs`
- **Semantic Tokens Module:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/`
- **LSP Capability Contract:** `docs/reference/LSP_CAPABILITY_CONTRACT.md` (if exists)
