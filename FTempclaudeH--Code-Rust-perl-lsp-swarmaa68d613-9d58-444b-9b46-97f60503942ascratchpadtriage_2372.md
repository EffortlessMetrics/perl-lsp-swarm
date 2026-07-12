<!-- research-triage-pass run_id:2026-07-11-char-indices-safety issue:2372 mode:direct-research -->

## Current state

**File and function:** `crates/perl-module/src/token_core/mod.rs:98-117` — function `left_context_is_module_char()`, called by public API `has_standalone_module_token_boundaries()`.

**Code unchanged since last review** (last commit: 2026-04-15, refactor #4422, structural reorganization only). No subsequent fixes applied.

**Test coverage:** Extensive (5 test files, including 5000-iteration fuzz suite, property-based, integration, and unit tests). Fuzz suite runs without panics.

## Verification: is line 116 unsafe?

**Claim:** Line 116 `line[..left_idx].chars().next_back()` could panic due to unsafe char_indices slicing.

**Analysis:**

1. **What char_indices() returns:** `char_indices()` yields `(byte_offset, char)` pairs where `byte_offset` is **always** a valid UTF-8 char boundary by contract. See [Rust docs: str::char_indices](https://doc.rust-lang.org/std/primitive.str.html#method.char_indices).

2. **Logic flow:**
   - Line 103: `let mut left = line[..start].char_indices();`
   - Line 104-106: `let Some((left_idx, ch)) = left.next_back()` — `left_idx` is guaranteed valid boundary
   - Line 112-114: Guard `if left_idx == 0 { return false; }`
   - Line 116: Slice `line[..left_idx]` — always safe because `left_idx` is a valid boundary and `left_idx >= 1`

3. **Issue's concerns addressed:**
   - "Adjacent surrogate pairs": Not applicable to UTF-8 (surrogates are UTF-16 only; Rust strings are always well-formed UTF-8).
   - "Decomposed characters": UTF-8 boundary calculation does not depend on Unicode normalization form; char_indices still returns valid offsets.

4. **Real precondition:** Function assumes caller passes valid `start` (char boundary). If caller passes mid-codepoint offset, line 103 `line[..start]` would panic. **This is not line 116's problem — it's a precondition at function entry.**

## Scope assessment

**Is line 116 unsafe?** No — `char_indices()` contract guarantees byte offsets are valid boundaries.

**Is there a latent bug elsewhere?** Potentially at caller sites, if they pass non-boundary `start` values. But this function is `pub(crate)`, called from within the same module's boundary detection logic, so callers are controlled.

**Test gap:** Fuzz suite uses ASCII-only characters. A gap-closure test with multi-byte UTF-8 (emoji, accents, etc.) would be nice for documentation, but not a blocker (char_indices is well-established and tested by libstd).

## Plan forward

**Verdict:** SAFE-BY-CONTRACT. No action required on line 116 specifically.

**Options:**
- **A) Close as valid-by-design:** Code is correct; issue is based on misunderstanding of UTF-8/char_indices.
- **B) Add debug_assert for precondition:** Add `debug_assert!(line.is_char_boundary(start))` at function entry to document precondition and catch caller bugs early. (Low overhead, improves debuggability.)
- **C) Add UTF-8 edge-case tests:** Extend fuzz suite to include multi-byte UTF-8 chars (emoji, combining marks). Purely for documentation—tests will pass because code is safe.

**Recommendation:** **(B) + optional (C).** Add precondition assertion to document the assumption; extend fuzz coverage for confidence. Then close.

---
Current-state research by research verifier (2026-07-11).
