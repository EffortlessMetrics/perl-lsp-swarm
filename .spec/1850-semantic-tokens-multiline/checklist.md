# Implementation Checklist: Issue #1850 — Semantic Tokens Multiline Length Fix

## Overview
Fix multiline semantic token length calculation to comply with LSP specification. When a token spans multiple lines, its length should be the number of UTF-16 characters from token start to the end of the starting line, not 0.

## Change Order

### Step 1: Add helper function for computing multiline token length
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** Add before `collect_semantic_tokens` function (~line 499)

Add a new helper function:
```rust
/// Get the end-of-line column position for a given line in UTF-16 character units.
fn get_eol_col(text: &str, line_idx: u32) -> u32 {
    text.lines()
        .nth(line_idx as usize)
        .map(|line| {
            let mut utf16_count = 0u32;
            for ch in line.chars() {
                utf16_count += ch.len_utf16() as u32;
            }
            utf16_count
        })
        .unwrap_or(0)
}
```

**Dependency:** None
**Verify:** `cargo fmt --all && cargo clippy -p perl-lsp-rs-core --lib`

### Step 2: Update main lexer token loop (line 521)
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** 519–650

Change line 521 from:
```rust
let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, sl);
let len = if sl == el { ec.saturating_sub(sc) } else { eol_col.saturating_sub(sc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 3: Update SQL keyword tokenization (line 365)
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** 349–370

Change line 365 from:
```rust
let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, sl);
let len = if sl == el { ec.saturating_sub(sc) } else { eol_col.saturating_sub(sc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 4: Update JSON key tokenization (line 394)
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** 372–399

Change line 394 from:
```rust
let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, sl);
let len = if sl == el { ec.saturating_sub(sc) } else { eol_col.saturating_sub(sc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 5: Update InterpolatedString literal parts (line 559)
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** 541–599

Change line 559 from:
```rust
let plen = if psl == pel { pec.saturating_sub(psc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, psl);
let plen = if psl == pel { pec.saturating_sub(psc) } else { eol_col.saturating_sub(psc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 6: Update InterpolatedString variable parts (line 579)
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** 541–599

Change line 579 from:
```rust
let plen = if psl == pel { pec.saturating_sub(psc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, psl);
let plen = if psl == pel { pec.saturating_sub(psc) } else { eol_col.saturating_sub(psc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 7: Update all remaining AST token length calculations
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`

Apply the same pattern to all remaining token length calculations:
- Line 671 (Package name token)
- Line 686 (Subroutine with name_span token)
- Line 701 (Subroutine without name_span token)
- Line 723 (Method declaration token)
- Line 738 (Class declaration token)
- Line 747 (PhaseBlock token)
- Line 762 (LabeledStatement token)
- Line 778 (LoopControl label token)
- Line 792 (MethodCall token)
- Line 819 (MethodCall first arg SQL string token)
- Line 832 (generic Variable/FunctionCall token)

For each occurrence, change from:
```rust
let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
```

To:
```rust
let eol_col = get_eol_col(text, sl);
let len = if sl == el { ec.saturating_sub(sc) } else { eol_col.saturating_sub(sc) };
```

Or for the `alen` case at line 819:
```rust
let eol_col = get_eol_col(text, asl);
let alen = if asl == ael { aec.saturating_sub(asc) } else { eol_col.saturating_sub(asc) };
```

**Dependency:** Step 1 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens && cargo clippy -p perl-lsp-rs-core --lib`

### Step 8: Add comprehensive multiline token tests
**File:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
**Lines:** In test module (~1078–1525)

Add new test cases:
- `test_collect_semantic_tokens_multiline_string` — verify heredoc spanning multiple lines has correct length
- `test_collect_semantic_tokens_multiline_variable` — verify interpolated string part spanning lines
- `test_compute_multiline_token_length_on_start_line` — unit test for multiline length calculation

**Dependency:** Steps 1–7 completed
**Verify:** `cargo test -p perl-lsp-rs-core --lib semantic_tokens`

### Step 9: Final verification and integration test
**Verify commands:**
```bash
cargo fmt -p perl-lsp-rs-core
cargo clippy -p perl-lsp-rs-core --lib --tests
cargo test -p perl-lsp-rs-core --lib semantic_tokens
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

## Summary

- **Total files modified:** 1 (`crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`)
- **Helper functions added:** 1 (`get_eol_col`)
- **Token length calculations fixed:** 16 locations (lexer + AST paths)
- **Tests added:** ~3 multiline token scenarios
- **Compilation check:** Will compile after Step 1 due to function addition
- **All changes are in:** `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
