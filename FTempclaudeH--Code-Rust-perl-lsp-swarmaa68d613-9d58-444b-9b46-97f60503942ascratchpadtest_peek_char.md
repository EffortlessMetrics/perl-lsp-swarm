# peek_char Offset Inconsistency Analysis

## Code Location
`crates/perl-lexer/src/lexer/helpers/cursor.rs` lines 46-67 (origin/main HEAD)

## The Bug

```rust
pub(crate) fn peek_char(&self, offset: usize) -> Option<char> {
    // ...
    let pos = self.position.checked_add(offset)?;  // Line 54: offset as BYTE offset
    if pos < self.input_bytes.len() {
        let byte = Self::byte_at(self.input_bytes, pos);
        if byte < 128 {
            Some(byte as char)  // ASCII path
        } else {
            self.input.get(self.position..).and_then(|s| s.chars().nth(offset))  // Line 62: offset as CHARACTER count
        }
    } else {
        None
    }
}
```

## Semantic Inconsistency

1. **Line 54**: `pos = position + offset` treats offset as a BYTE offset
2. **Line 57-58**: Checks byte at `pos` to decide ASCII vs non-ASCII
3. **Line 62**: Falls back to `chars().nth(offset)` which treats offset as CHARACTER count

## Problem

When you add a byte offset to a character boundary, you can land INSIDE a multi-byte UTF-8 character. The byte check becomes semantically wrong.

Example: Input "αβ" (each is 2-byte UTF-8)
- Bytes: [CE, B1, CE, B2]
- Position 0, peek_char(1):
  - pos = 0 + 1 = 1 (byte offset)
  - byte = bytes[1] = 0xB1 (CONTINUATION byte of α)
  - Since 0xB1 >= 128, use chars().nth(1) = 'β'
  - Result happens to be correct, but the logic is checking the wrong byte

## Call Site Analysis

All usage in codebase treats offset as CHARACTER offset:
- peek_char(1), peek_char(2) — looking ahead by character count
- No byte offsets are ever passed

## Verdict

**CONFIRMED**: peek_char has semantic offset inconsistency:
- Positive path: treats offset as byte offset (wrong)
- Fallback path: treats offset as character offset (correct)
- Function works in many cases by accident (when non-ASCII byte triggers correct fallback)
- But the inconsistency is a correctness/maintainability bug
