# Acceptance Criteria: Issue #1850 — Semantic Tokens Multiline Token Length

## §Behavior

| Input | Condition | Expected Result | Test Name |
|-------|-----------|-----------------|-----------|
| Heredoc token spanning lines 1–3 | Token start at (1, 5), end at (3, 10) | Length = chars from column 5 to EOL on line 1 | `test_collect_semantic_tokens_multiline_heredoc` |
| Interpolated string var spanning lines 2–4 | Variable start at (2, 8), end at (4, 3) | Length = chars from column 8 to EOL on line 2 | `test_collect_semantic_tokens_multiline_variable` |
| SQL keyword match on same line | Match start at (5, 12), end at (5, 18) | Length = 6 (normal single-line) | `test_collect_semantic_tokens_sql_single_line` |
| JSON key spanning lines 3–5 | Key start at (3, 2), end at (5, 8) | Length = chars from column 2 to EOL on line 3 | `test_collect_semantic_tokens_multiline_json_key` |
| Method declaration multiline | Method start at (7, 0), end at (8, 15) | Length = chars from column 0 to EOL on line 7 | `test_collect_semantic_tokens_multiline_method` |
| Package name multiline | Package start at (1, 8), end at (2, 4) | Length = chars from column 8 to EOL on line 1 | `test_collect_semantic_tokens_multiline_package` |
| Class declaration multiline | Class start at (2, 0), end at (3, 7) | Length = chars from column 0 to EOL on line 2 | `test_collect_semantic_tokens_multiline_class` |

## §Hazards

| Hazard Class | Surface | Description | Mitigation |
|--------------|---------|-------------|-----------|
| **BOUNDARY-OFF-BY-ONE** | `get_eol_col` (new function) | UTF-16 code unit counting at line boundaries may differ from byte-based assumptions; tabs, multi-byte chars, emoji distort column positions. | Unit test `test_eol_col_utf16_boundaries` verifies boundary: single-byte, multi-byte, emoji, tab boundaries. Input: line with `hello\t😀` (hi + tab + emoji). Expected: EOL col in UTF-16 units. |
| **REGRESSION-SINGLE-LINE** | line 521, 365, 394, 559, 579, etc. (all token length sites) | Changing `len = 0` to `eol_col.saturating_sub(sc)` risks breaking single-line token handling if conditional is omitted or `eol_col` is computed wrong. | Test `test_collect_semantic_tokens_sql_single_line` verifies single-line tokens still work: SQL keyword on line 5, column 12–18 must emit length=6. Verify with lexer token loop (line 521). |
| **PERFORMANCE-REPEATED-COMPUTATION** | `get_eol_col` called per-token in hot loop | Calling `text.lines().nth()` for every token (16+ callsites) causes O(n²) behavior on large files with O(n) line scans per token. | Cache line offsets in `collect_semantic_tokens` before main loop; pass `&line_eol_cols: &[u32]` to helpers. Benchmark: 10k tokens, measure time before/after. |
| **ENCODING-MISMATCH-UTF16** | `get_eol_col` UTF-16 counter | LSP assumes UTF-16 code unit positions, but Rust strings are UTF-8 byte slices. Miscounting `char.len_utf16()` (surrogates in emoji) breaks multiline tokens. | Unit test `test_eol_col_emoji_surrogates`: input `"hello😀end"` (hello=5, emoji=2 UTF-16 units, end=3, total=10). Expected: `get_eol_col(...) == 10`. Verify emoji 😀 is 2 units, not 1. |
| **STATE-CORRUPTION-PREVIOUS-TOKEN** | `eol_col` calculation in loop (line 521+) | Computing `eol_col` inside loop for every token may use wrong line if `sl` shifts but `eol_col` is reused. | Test `test_eol_col_per_token_line`: emit tokens on lines 1, 3, 5, 7 with varying start columns. Verify each token's `eol_col` matches its line, not previous token's line. |
| **EDGE-CASE-EMPTY-LINES** | `get_eol_col` with empty line | Empty line (just `\n`) should return `eol_col == 0`; if `text.lines()` skips empty lines or `nth()` off-by-one, multiline token at EOL column 0 may get wrong length. | Test `test_eol_col_empty_line`: input `"line1\n\nline3"`, line 1 (empty). Expected: `get_eol_col(text, 1) == 0`. Token on line 1 column 0 should have length = 0. |

## §Contracts

| Protocol | Component | Contract Clause | Verified By |
|----------|-----------|-----------------|-------------|
| **LSP SemanticTokens** | `textDocument/semanticTokens/full` | Token length for multiline tokens (deltaLine != 0 across lines) must be UTF-16 char count from token start to EOL of **start line**, not 0 or full span. Per [LSP spec §7.17.1](https://microsoft.github.io/language-server-protocol/specifications/specification-3-17-0/#textDocument_semanticTokens): "length is the number of UTF-16 code units" for each line segment. | Test `test_collect_semantic_tokens_multiline_heredoc`: emit <<SQL...heredoc across 3 lines, verify token at (1,5) has length = columns 5–EOL on line 1, not 0. |
| **Perl AST** | `Node.location` | Token location spans from `node.location.start` to `node.location.end` (byte offsets). Conversion to (line, col) via `to_pos16` must be precise for multiline spans. | Test `test_collect_semantic_tokens_multiline_method`: method keyword + declaration spanning 2 lines; verify (sl, sc, el, ec) are correct byte offsets and conversion is accurate. |
| **Perl Lexer** | `PerlLexer::next_token()` | Heredoc bodies, interpolated strings, comments can span multiple lines. Token span is (start, end) in byte offsets; `to_pos16` translates to LSP positions. | Test `test_collect_semantic_tokens_multiline_variable`: interpolated string `"prefix#{$var}suffix"` where `$var` spans lines 2–4; verify variable token emits correct (line, col, len). |
| **UTF-16 Encoding** | LSP pos16 conversion | Column positions in LSP are UTF-16 code unit offsets, not bytes or chars. `char.len_utf16()` must count surrogates (emoji, some symbols = 2 units, ASCII = 1 unit). | Helper `get_eol_col` test: line `"hello😀"` must emit eol_col=7 (hello=5 + emoji=2), not 6 or 8. Unit test `test_eol_col_emoji_surrogates`. |

## §API-Shape

| Item | Kind | Signature / Definition | Change | Visibility | Dup-Risk Grep | Caller Count |
|------|------|------------------------|--------|-----------|-----------------|--------------|
| `get_eol_col` | Function | `fn(text: &str, line_idx: u32) -> u32` | **New** | Private (module-local) | `grep -n "get_eol_col"` → 16 calls in semantic_tokens.rs | 16 (all in semantic_tokens module) |
| Token length fix at line 521 | Code | `let eol_col = get_eol_col(text, sl);` + `let len = if sl == el { ... } else { eol_col.saturating_sub(sc) };` | **Modified** (was `else { 0 }`) | N/A | `grep -n "let len = if sl == el"` → 16 occurrences | ~16 in token collection loop + AST walk |
| Test: `test_collect_semantic_tokens_multiline_heredoc` | Test | `fn() -> Result<(), Box<dyn std::error::Error>>` | **New** | Private (`#[cfg(test)]`) | `grep -n "test_collect_semantic_tokens_multiline"` | 1 (tests module) |
| Test: `test_collect_semantic_tokens_multiline_variable` | Test | `fn() -> Result<(), Box<dyn std::error::Error>>` | **New** | Private (`#[cfg(test)]`) | `grep -n "test_collect_semantic_tokens_multiline"` | 1 (tests module) |
| Test: `test_eol_col_utf16_boundaries` | Test | `fn()` | **New** | Private (`#[cfg(test)]`) | `grep -n "test_eol_col"` | 1 (tests module) |
| Test: `test_eol_col_emoji_surrogates` | Test | `fn()` | **New** | Private (`#[cfg(test)]`) | `grep -n "test_eol_col"` | 1 (tests module) |

## §Test-Grid

| Scenario | Test Name | Input | Assertion | Pass Condition |
|----------|-----------|-------|-----------|---|
| **Positive: Multiline heredoc token** | `test_collect_semantic_tokens_multiline_heredoc` | Perl code: `<<SQL\nselect...\nfrom...\nEND` | `tokens.iter().find(token where token spans lines 0–2)` → `length == eol_col_of_line_0 - token_start_col` | Token length equals chars from start col to EOL on line 0 |
| **Positive: Single-line SQL keyword (sanity check)** | `test_collect_semantic_tokens_sql_single_line` | Perl code: `print "SELECT * FROM table";` (SELECT on line 0, cols 8–14) | SQL keyword token at (0, 8, 6, ...) | Token length = 6 (normal single-line behavior unchanged) |
| **Positive: Multiline variable in interpolated string** | `test_collect_semantic_tokens_multiline_variable` | Perl code: `"foo #{\n$var\n} bar"` (variable on line 1) | Variable token spanning (0, ?) → (2, ?) | Length = chars from start col to EOL on line 0 (or line 1 if var starts on line 1) |
| **Positive: Multiline method declaration** | `test_collect_semantic_tokens_multiline_method` | Perl code: `method foo\n  ($x, $y) { ... }` (method keyword on line 0) | Method token at (0, 0, len, ...) | Length = chars from 0 to EOL on line 0 |
| **Negative: Zero-length token at EOL** | `test_eol_col_empty_line` | Perl code: `"line1\n\nline3"` (empty line 1) | `get_eol_col(text, 1)` should return 0 | Empty line → eol_col = 0; token at (1, 0) has length 0 |
| **Adversarial: UTF-16 emoji surrogate** | `test_eol_col_emoji_surrogates` | Line text: `"hello😀"` (5 ASCII + 1 emoji = 5 + 2 UTF-16 units) | `get_eol_col(...)` returns 7, not 6 or 8 | Emoji correctly counted as 2 UTF-16 units |
| **Adversarial: Tab character width in UTF-16** | `test_eol_col_tab_character` | Line text: `"col\there"` (3 + tab + 4 = 8 UTF-16 units) | `get_eol_col(...)` returns 8 | Tab counted as 1 UTF-16 unit (not visual width) |
| **State: Repeated tokens on different lines** | `test_eol_col_per_token_line` | Perl code: multiple tokens on lines 0, 2, 5 with varying start cols | For each token, verify `eol_col` matches its actual line, not previous token | Each token's `eol_col` is independently correct per its line index |
| **Boundary: Token at line start (col 0)** | `test_eol_col_col_zero_multiline` | Multiline token starting at (0, 0) | Length = `get_eol_col(text, 0) - 0` = full EOL col | Length spans from column 0 to EOL |
| **Boundary: Token at line end** | `test_eol_col_col_eol_multiline` | Multiline token starting at (0, eol_col) | Length = `get_eol_col(text, 0) - eol_col` = 0 | Token at EOL has length 0 (correct edge case) |

## §Blast-Radius

| Consumer | Dependency | Impact | Boundary | Must Not Touch |
|----------|-----------|--------|----------|---|
| `crates/perl-lsp-rs` (LSP server) | Re-exports `collect_semantic_tokens` from `perl-lsp-rs-core::providers::semantic_tokens` | Semantic token encoding changes; LSP clients now see correct multiline token lengths. Old length=0 tokens are now length=N; client rendering changes. | Public function signature unchanged (stable API); only internal token computation changes. | LSP protocol handler signature; `to_pos16` function (already correct); `Token` struct fields |
| `crates/perl-lsp-rs/tests/lsp_semantic_tokens*.rs` | Indirect (via `collect_semantic_tokens` output) | Test expectations must be updated if they assert on token length for multiline tokens. Existing tests likely don't cover multiline scenarios; new tests will. | Test assertions on token length in acceptance criteria tests. | Test harness infrastructure; LSP client simulation code |
| `crates/perl-lsp-rs-core` (library consumers) | Public `collect_semantic_tokens` function | No breaking change (signature identical). Token output semantics improve (length no longer 0 for multiline). | Return type `Vec<EncodedToken>` unchanged. | Function parameter order; return type structure |
| `perl-lsp-rs` binary entry points | Transitive through LSP provider | End-user experience improves: multiline syntax highlighting now works (e.g., heredocs, method declarations spanning lines). | None; transparent improvement. | LSP server main loop; transport protocol |
| Downstream LSP clients (VSCode, Emacs, Vim, etc.) | LSP semanticTokens response encoding | Clients now receive non-zero lengths for multiline tokens; highlighting decorations render correctly across line boundaries. | LSP protocol compliance improves; no client code changes needed (spec-compliant). | Client-side token rendering (outside this repo). |

---

## §Coverage-Map

N/A — This is a bug fix, not a coverage/CI change. Existing test coverage of semantic tokens remains; new tests add multiline scenarios. No codecov configuration changes needed.
