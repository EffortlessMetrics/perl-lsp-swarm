## Research verification (2026-07-11)

### 1. Current state on origin/main
**CONFIRMED — invalid byte-to-char cast at detection.rs:64,69 (commit eb4c81905, latest)**

File: `crates/perl-module/src/rename/detection.rs`
- **Line 64**: `let ch = line_bytes[abs - 1] as char;` (checking character BEFORE match)
- **Line 69**: `let ch = line_bytes[after] as char;` (checking character AFTER match)

### 2. Semantic issue (not safety, but correctness)
**Claim verified**: While Rust's u8→char cast is technically safe (all u8 values 0-255 are valid Unicode), the code has a **UTF-8 decoding error**:

- `line_bytes[abs - 1]` may point to a UTF-8 continuation byte (0x80-0xBF) if the character at `abs` is multi-byte
- Casting continuation bytes to char produces wrong characters
  - Example: "café MyModule" — the byte before 'M' (position 5) is 0xA9 (2nd byte of 'é')
  - Casting 0xA9 to char gives U+00A9 (©), not the actual previous character
  - This corrupts word-boundary detection for non-ASCII Perl code

**Evidence**: 
- Rust documentation confirms u8→char is safe but treats bytes as ISO-8859-1, not UTF-8: https://doc.rust-lang.org/std/primitive.char.html
- No existing tests cover non-ASCII input (facade_api_completeness.rs and rename_comprehensive_unit_tests.rs use ASCII-only test cases)

### 3. Scope & plan
**Same fix as #2371** (companion bug in rewriting.rs):
Replace byte casting with UTF-8-aware character extraction:
```rust
// Current (broken):
let ch = line_bytes[abs - 1] as char;

// Fix: Use char_indices() for proper UTF-8 boundaries
let ch = line[..abs].chars().rev().next().unwrap_or(' ');  // Character BEFORE abs
```

**Files to fix**: detection.rs:64, 69 (and companion #2371 in rewriting.rs)

### 4. Next-state triage
**Status**: builder-ready
- Issue is reproducible (non-ASCII Perl code breaks word-boundary detection)
- Fix strategy is clear (use char_indices or .chars())
- Should be fixed as part of the same PR addressing #2371

**Recommendation**: Open a builder PR fixing both #2370 and #2371 together (both in `perl-module/src/rename/`), add tests with non-ASCII input (e.g., Perl code with emoji, accented characters).
