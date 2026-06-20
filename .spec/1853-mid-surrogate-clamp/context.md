# Context: LineStartsCache Mid-Surrogate Clamp

Issue: #1853

## Problem Statement

LSP clients send position requests using UTF-16 code unit offsets (per the LSP 3.17 spec). Characters outside the Basic Multilingual Plane (BMP), such as emoji and other supplementary-plane characters, require 2 UTF-16 code units (a "surrogate pair") per character.

When a client sends a column position that falls in the middle of a surrogate pair (a "mid-surrogate" position), the server must clamp that position to the start of the character, not attempt to index into the middle of the multi-byte sequence.

**Current issue:** `LineStartsCache::position_to_offset_rope` (and `position_to_offset`) do not perform this clamping. Instead, they increment the byte offset for the entire character, returning a position in the middle of the character's UTF-8 encoding. This violates the UTF-16 round-trip invariant and causes incorrect LSP position mappings.

### Example

Text: "a😀b"
- UTF-8 bytes: [a(1), 😀(4), b(1)] = "a" at byte 0, "😀" at bytes 1-4, "b" at byte 5
- UTF-16 units: [a(1), 😀(2), b(1)] = "a" at unit 0, "😀" at units 1-2, "b" at unit 3

If a client sends (line=0, character=2), they are pointing at the second UTF-16 unit of the "😀" surrogate pair (mid-surrogate). The correct byte offset is 1 (start of "😀"), not 5 (start of "b").

**Current behavior:** `position_to_offset_rope` would return 5 (after consuming the entire emoji character in the loop).
**Expected behavior:** Should return 1 (clamped to the start of the emoji).

---

## Decision: Copy Pattern from PositionMapper

The `PositionMapper::lsp_pos_to_byte` method (lines 136-152 in `mapper.rs`) already correctly implements mid-surrogate clamping:

```rust
let ch_utf16_len = if ch as u32 > 0xFFFF { 2 } else { 1 };
let next_utf16 = utf16_offset + ch_utf16_len;
// Clamp positions inside a surrogate pair to the start of the code point.
if next_utf16 > pos.character {
    break;
}
utf16_offset = next_utf16;
byte_offset += ch.len_utf8();
```

We will apply the identical pattern to both `LineStartsCache` methods:
1. Before updating `uc` (UTF-16 unit count), calculate the next expected count.
2. If the next count would overshoot the requested column, break without updating byte offset.
3. Otherwise, update both `uc` and `bo` together.

This ensures both position converters (PositionMapper and LineStartsCache) use the same logic.

---

## Alternatives Considered

### 1. Default-to-end behavior (LSP spec interpretation)
LSP 3.17 states: "If the character value is greater than the line length, it defaults to the line length."

However, mid-surrogate positions are not "beyond the line length"—they are positions that don't exist in UTF-16. The spec does not address this case. Clamping to character start is the safest interpretation and matches PositionMapper's behavior.

**Rejected** because it would create inconsistency between the two converters and break round-trip invariants.

### 2. Return an error/Option
Instead of clamping, return `None` for mid-surrogate positions.

**Rejected** because:
- PositionMapper returns `Some(byte_offset)`, not `None`
- LSP clients may send mid-surrogate positions; rejecting them breaks LSP compatibility
- Clamping is the simpler, proven approach

### 3. Precompute UTF-16 line lengths
Cache the UTF-16 column count for each line to detect out-of-bounds positions earlier.

**Rejected** because:
- Current implementation is simple and efficient
- Clamping happens naturally in the loop; no need for separate validation
- Adds complexity for a rare edge case

---

## Prior Art / Related Fixes

1. **PositionMapper::lsp_pos_to_byte** (mapper.rs, lines 136-152) — Already implements mid-surrogate clamping. Test coverage: `test_utf16_positions_clamp_mid_surrogate_to_char_start` (line 390) and comprehensive tests (lines 399-506).

2. **convert::utf16_line_col_to_offset** (convert.rs) — Another UTF-16 converter; must also verify it handles mid-surrogates. Tested against PositionMapper in `test_utf16_clamp_matches_convert_helper` (mapper.rs line 509).

3. **LineIndex::position_to_offset** (line_index.rs, lines 194-215) — Separate implementation of position mapping. Must verify it also implements clamping (or file a follow-up for it).

4. **Commit 475db61b4** — "fix(perl-position-tracking): clamp UTF-16 mid-surrogate positions (#0000) (#5755)" added comprehensive test coverage for PositionMapper. We will reuse these tests as templates for LineStartsCache.

---

## Testing Strategy

### Unit Tests
1. **Simple mid-surrogate** — "a😀b" at columns 0..4 (mirrors `test_utf16_surrogate_pair_boundaries`)
2. **Consecutive surrogates** — "💖💖" (mirrors `test_utf16_consecutive_surrogate_pairs`)
3. **Mixed BMP+supplementary** — "aé💖ñ🎉b" (mirrors `test_utf16_mixed_bmp_and_supplementary_plane`)
4. **Max Unicode** — U+10FFFF (mirrors `test_utf16_max_code_point`)
5. **Zero-length** — "" (mirrors `test_utf16_zero_length_input`)
6. **Parity** — Verify both `position_to_offset` and `position_to_offset_rope` match `PositionMapper::lsp_pos_to_byte` (mirrors `test_utf16_clamp_matches_convert_helper`)

### Integration Tests
- Reuse existing LSP server tests; verify no regression in position-dependent features (completions, diagnostics, hover)

---

## Risk Analysis

**Low risk:**
- No API changes
- Pure behavior fix (incorrect → correct)
- Pattern copied from proven implementation (PositionMapper)
- Comprehensive test coverage exists as template

**Potential issues:**
- If a caller was somehow *relying* on the buggy behavior (unlikely), they will see different byte offsets. But this would only happen if they were explicitly working around the bug—highly improbable.
- Performance: The clamping adds 2-3 lines of code per method, no allocations, no unbounded loops.

---

## Links

- **LSP 3.17 Position:** https://microsoft.github.io/language-server-protocol/specifications/specification-current/#position
- **UTF-16 surrogate pairs:** https://en.wikipedia.org/wiki/UTF-16#Code_units
- **PositionMapper test suite:** `crates/perl-position-tracking/src/mapper.rs` lines 390-530
- **Related issue:** #1853
- **Related commits:** 475db61b4, f2b3ce75c (PositionMapper clamping), dd0dd0f78 (CRLF guard in position_to_offset_rope)
