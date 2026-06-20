# Implementation Checklist: LineStartsCache Mid-Surrogate Clamp

Issue: #1853 — fix LineStartsCache::position_to_offset_rope wrong byte offset for mid-surrogate UTF-16 columns

## Change Order

### Step 1: Add mid-surrogate clamping to `LineStartsCache::position_to_offset`

**File:** `crates/perl-position-tracking/src/line_index.rs`

**What changes:**
- Method: `impl LineStartsCache { fn position_to_offset(...) }`
- Lines: 66-94
- Add clamping logic inside the loop to detect and clamp mid-surrogate positions

**Signature (before):**
```rust
pub fn position_to_offset(&self, text: &str, line: u32, character: u32) -> usize {
    // ... existing code ...
    for ch in lt.chars() {
        if uc >= character as usize {
            break;
        }
        uc += ch.len_utf16();
        bo += ch.len_utf8();
    }
    // ... existing code ...
}
```

**Signature (after):**
```rust
pub fn position_to_offset(&self, text: &str, line: u32, character: u32) -> usize {
    // ... existing code ...
    for ch in lt.chars() {
        if uc >= character as usize {
            break;
        }
        let ch_utf16_len = if ch as u32 > 0xFFFF { 2 } else { 1 };
        let next_uc = uc + ch_utf16_len;
        // Clamp positions inside a surrogate pair to the start of the code point.
        if next_uc > character as usize {
            break;
        }
        uc = next_uc;
        bo += ch.len_utf8();
    }
    // ... existing code ...
}
```

**Dependencies:** None — this is a self-contained method change.

**Verify command:** `cargo test -p perl-position-tracking`

---

### Step 2: Add mid-surrogate clamping to `LineStartsCache::position_to_offset_rope`

**File:** `crates/perl-position-tracking/src/line_index.rs`

**What changes:**
- Method: `impl LineStartsCache { fn position_to_offset_rope(...) }`
- Lines: 125-147
- Add clamping logic inside the loop (same pattern as Step 1)

**Signature (before):**
```rust
pub fn position_to_offset_rope(&self, rope: &Rope, line: u32, character: u32) -> usize {
    // ... existing code ...
    for ch in sl.chars() {
        if uc >= character as usize {
            break;
        }
        uc += ch.len_utf16();
        bo += ch.len_utf8();
    }
    // ... existing code ...
}
```

**Signature (after):**
```rust
pub fn position_to_offset_rope(&self, rope: &Rope, line: u32, character: u32) -> usize {
    // ... existing code ...
    for ch in sl.chars() {
        if uc >= character as usize {
            break;
        }
        let ch_utf16_len = if ch as u32 > 0xFFFF { 2 } else { 1 };
        let next_uc = uc + ch_utf16_len;
        // Clamp positions inside a surrogate pair to the start of the code point.
        if next_uc > character as usize {
            break;
        }
        uc = next_uc;
        bo += ch.len_utf8();
    }
    // ... existing code ...
}
```

**Dependencies:** Step 1 — for consistency, both methods should use the same pattern.

**Verify command:** `cargo test -p perl-position-tracking`

---

### Step 3: Write failing tests for LineStartsCache mid-surrogate clamping

**File:** `crates/perl-position-tracking/tests/comprehensive_unit_tests.rs` (or create new test module)

**What changes:**
- Add test function `test_line_starts_cache_position_to_offset_mid_surrogate_clamp`
- Add test function `test_line_starts_cache_position_to_offset_rope_mid_surrogate_clamp`
- Mirror the tests from `mapper.rs` (lines 390-506) for the cache methods

**Test cases to cover:**
1. Simple emoji mid-surrogate clamp (e.g., "a😀b" at column 2)
2. Back-to-back supplementary chars (e.g., "💖💖")
3. Mixed BMP + supplementary (e.g., "aé💖ñ🎉b")
4. Max Unicode code point U+10FFFF
5. Zero-length input
6. Parity check: verify both `position_to_offset` and `position_to_offset_rope` give same result as `PositionMapper::lsp_pos_to_byte`

**Verify command:** `cargo test -p perl-position-tracking --test comprehensive_unit_tests` (or corresponding test file)

---

### Step 4: Run all tests to verify implementation

**Verify command:** `cargo test -p perl-position-tracking`

**Expected outcome:** All tests pass, including new mid-surrogate clamping tests.

---

## Summary

- **Crate touched:** `perl-position-tracking`
- **Files changed:** 1 (line_index.rs)
- **Methods changed:** 2 (position_to_offset, position_to_offset_rope)
- **Tests added:** ~5 new test functions covering mid-surrogate clamp scenarios
- **Lines of code:** ~8 per method (clamping logic) + ~100+ lines of test code
- **Complexity:** Low — copy proven pattern from `PositionMapper` to cache methods
