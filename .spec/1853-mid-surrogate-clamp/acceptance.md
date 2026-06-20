# Acceptance Criteria: LineStartsCache Mid-Surrogate Clamp

Issue: #1853 — fix LineStartsCache::position_to_offset_rope wrong byte offset for mid-surrogate UTF-16 columns

---

## §Behavior

When an LSP client sends a UTF-16 column position that falls in the middle of a multi-byte character's UTF-16 representation (a "mid-surrogate" position), the byte-offset conversion must clamp to the start of that character, matching the behavior of `PositionMapper::lsp_pos_to_byte`.

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| text="a😀b", line=0, character=0 | Valid BMP column | Returns byte offset 0 (start of 'a') |
| text="a😀b", line=0, character=1 | Valid supplementary column (start of emoji) | Returns byte offset 1 (start of 😀) |
| text="a😀b", line=0, character=2 | **Mid-surrogate** (inside emoji) | **Returns byte offset 1** (clamped to start of 😀) |
| text="a😀b", line=0, character=3 | Valid column after emoji | Returns byte offset 5 (start of 'b') |
| text="a😀b", line=0, character=4 | Valid column at end | Returns byte offset 6 (end) |
| text="💖💖", line=0, character=1 | **Mid-surrogate in first emoji** | **Returns byte offset 0** (clamped to start) |
| text="💖💖", line=0, character=2 | Valid column between emojis | Returns byte offset 4 (start of second emoji) |
| text="aé💖ñ🎉b", line=0, character=3 | **Mid-surrogate in 💖** | **Returns byte offset 3** (clamped) |
| text="aé💖ñ🎉b", line=0, character=6 | **Mid-surrogate in 🎉** | **Returns byte offset 9** (clamped) |
| text="a🔥", line=0, character=2 | **Mid-surrogate** | **Returns byte offset 1** (clamped to start) |

---

## §Hazards

| Class | Surface | Invariant | Test |
|-------|---------|-----------|------|
| **UTF-16 Encoding** | `LineStartsCache::position_to_offset`, `LineStartsCache::position_to_offset_rope` | Mid-surrogate positions (columns that fall in the middle of a 2-unit UTF-16 sequence) must clamp to the character start, never advance past. Clamp occurs before incrementing `bo` (byte offset). | `test_line_starts_cache_position_to_offset_mid_surrogate_clamp`, `test_line_starts_cache_position_to_offset_rope_mid_surrogate_clamp`, `test_line_starts_cache_parity_with_mapper` |
| **Multi-Byte Characters** | `LineStartsCache::position_to_offset`, `LineStartsCache::position_to_offset_rope` | Characters with `len_utf16() == 2` (U+10000..U+10FFFF) must have mid-surrogate detection. Characters with `len_utf16() == 1` (BMP) must never trigger clamp. | `test_line_starts_cache_consecutive_surrogates`, `test_line_starts_cache_mixed_bmp_supplementary` |
| **Boundary Conditions** | `LineStartsCache::position_to_offset`, `LineStartsCache::position_to_offset_rope` | Zero-length input, empty lines, end-of-line positions, and out-of-bounds character columns must not panic or corrupt state. Clamp logic must not affect boundary behavior. | `test_line_starts_cache_zero_length`, `test_line_starts_cache_out_of_bounds`, `test_line_starts_cache_crlf_boundaries` |
| **Round-Trip Consistency** | `LineStartsCache::position_to_offset*` vs `PositionMapper::lsp_pos_to_byte` | For every UTF-16 column in a test string (including mid-surrogates), the two converters must produce identical byte offsets. This is the canonical parity invariant. | `test_line_starts_cache_parity_with_mapper` (comprehensive, every column 0..line_len) |
| **Line Ending Handling** | `LineStartsCache::position_to_offset`, `LineStartsCache::position_to_offset_rope` with CRLF | Mid-surrogate clamping must occur before line-ending boundary checks. CRLF, LF, and CR line endings must not interfere with surrogate detection. | `test_line_starts_cache_crlf_with_surrogates` |
| **No Regression** | `LineStartsCache::position_to_offset`, `LineStartsCache::position_to_offset_rope` | Existing tests for non-mid-surrogate positions, BMP-only text, and ASCII must continue to pass. Performance must not degrade (no extra allocations or unbounded loops). | `cargo test -p perl-position-tracking` (all existing tests) |

---

## §Contracts

**Parser contracts involved:** None — this is a pure position-tracking fix.

**LSP protocol contracts:**
- LSP spec: positions are 0-based line and UTF-16 code unit character offsets
- Per [LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/specification-current/#position): "If the character value is greater than the line length, it defaults to the line length."
- Mid-surrogate clamp extends this: positions in the middle of a surrogate pair are treated as if they were at the start of that character (clamped, not default-to-end)

**Related modules:**
- `crates/perl-position-tracking/src/mapper.rs` — `PositionMapper::lsp_pos_to_byte` (lines 136-152) already implements this clamping; we mirror it
- `crates/perl-position-tracking/src/convert.rs` — `utf16_line_col_to_offset` helper function (parity reference)
- `crates/perl-position-tracking/src/line_index.rs` — `LineIndex::position_to_offset` and `LineIndex::utf16_to_byte_offset` (separate implementation, verify parity separately)

---

## §API-Shape

**New public API:** None — these are bug fixes, not API additions.

**Changed signatures:**
- `LineStartsCache::position_to_offset(&self, text: &str, line: u32, character: u32) -> usize`
  - Behavior change: now clamps mid-surrogate positions (line 66-94)
  - No signature change; internal loop logic updated
- `LineStartsCache::position_to_offset_rope(&self, rope: &Rope, line: u32, character: u32) -> usize`
  - Behavior change: now clamps mid-surrogate positions (line 125-147)
  - No signature change; internal loop logic updated

**Callers of these methods:**
- `crates/perl-lsp-rs/src/runtime/diagnostics.rs` — uses `LineStartsCache` for position mapping
- `crates/perl-lsp-rs/src/runtime/text_sync/document_state.rs` — uses `LineStartsCache` for document edits
- `crates/perl-position-tracking/tests/comprehensive_unit_tests.rs` — test coverage
- `crates/perl-position-tracking/tests/extended_unit_tests.rs` — test coverage

**No public API surface added; no ID space, enum variant, or type additions.**

---

## §Test-Grid

| Category | Scenario | Test Name | Invariant |
|----------|----------|-----------|-----------|
| **Positive: Simple mid-surrogate** | Text "a😀b", UTF-16 column 2 (inside emoji) | `test_line_starts_cache_position_to_offset_mid_surrogate_clamp` | Byte offset clamped to 1 (emoji start), not 5 |
| **Positive: Rope variant** | Text "a😀b" in rope, UTF-16 column 2 | `test_line_starts_cache_position_to_offset_rope_mid_surrogate_clamp` | Same clamp behavior via rope interface |
| **Positive: Consecutive surrogates** | Text "💖💖", column 1 (mid first), column 3 (mid second) | `test_line_starts_cache_consecutive_surrogates` | Both clamp: col 1 → byte 0, col 3 → byte 4 |
| **Positive: Mixed BMP+supplementary** | Text "aé💖ñ🎉b", columns at every position | `test_line_starts_cache_mixed_bmp_supplementary` | BMP (é, ñ) return correct offsets; surrogates (💖, 🎉) mid-surrogates clamp |
| **Positive: Max Unicode** | Text "a\u{10FFFF}b" (U+10FFFF, max code point), column 2 | `test_line_starts_cache_max_unicode` | Clamps to byte 1 (U+10FFFF start) |
| **Positive: Zero-length** | Text "", all columns | `test_line_starts_cache_zero_length` | No panic; returns byte 0 (end of empty string) |
| **Positive: Parity with PositionMapper** | Mixed string "a😀b💖c\nx💡y", every column of line 0 | `test_line_starts_cache_parity_with_mapper` | `position_to_offset` byte offset == `PositionMapper::lsp_pos_to_byte` byte offset for each (line, col) |
| **Positive: CRLF boundaries** | Text "a😀\r\nb💖c", column 2 in line 0 (mid emoji) | `test_line_starts_cache_crlf_with_surrogates` | Clamps to byte 1; CRLF line ending boundary not affected |
| **Negative: Out-of-bounds column** | Text "a😀b", column=10 (past end) | Covered in existing tests; verify no regression | Returns byte offset of line end (clamped by existing logic) |
| **Negative: Out-of-bounds line** | Text "a\nb", line=5 (past end) | Covered in existing tests | Returns full rope/text byte length |
| **Adversarial: Back-to-back surrogates** | "🔥💧🎉" (3 consecutive supplementary), every column | `test_line_starts_cache_back_to_back_supplementary` | Mid-surrogates in each pair clamp correctly; no cross-pair contamination |
| **State-transition: Line with mixed endings** | Document with mixed CRLF and LF, surrogate in each line | Reuse `test_line_starts_cache_crlf_with_surrogates` | Clamp works across mixed line endings |

---

## §Blast-Radius

**Consumers of `LineStartsCache::position_to_offset*`:**
1. `crates/perl-lsp-rs/src/runtime/diagnostics.rs` — maps diagnostic positions
2. `crates/perl-lsp-rs/src/runtime/text_sync/document_state.rs` — incremental text sync
3. Internal tests in `perl-position-tracking` crate

**Downstream consumers (indirect):**
- LSP server (`perl-lsp-rs`) — uses position cache for all position-related operations
- DAP server (`perl-dap`) — may use position-tracking indirectly via LSP runtime

**Boundary to NOT cross:**
- Parser (`perl-parser`) — does not call `LineStartsCache` directly; uses `ByteSpan` instead
- Semantic analyzer — does not call `LineStartsCache` directly
- No other crates export or re-export `LineStartsCache`

**Risk level:** Low — this is a pure bug fix with no API changes. Existing callers benefit immediately. The only risk is if a caller was working around the bug; post-fix they will see correct behavior (not a regression).

**Testing scope:**
- Full `perl-position-tracking` test suite must pass
- Spot-check: LSP server tests with emoji in diagnostics, completion ranges (any mid-surrogate-heavy use case)
- No schema migration, no breaking changes
